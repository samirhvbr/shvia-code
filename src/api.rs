//! Cliente do gateway: `POST /api/v1/code/chat` + parser do SSE
//! (docs/protocolo.md). Uma chamada = uma "volta": streama os eventos (via
//! `on_event`, p/ a UI renderizar ao vivo) e devolve o agregado da volta.

use crate::config::Config;
use crate::interrupcao;
use crate::types::{ChatRequest, Event, Message, RawEvent, ToolCall, ToolDef, Usage};
use anyhow::{anyhow, Result};
use futures_util::StreamExt;
use std::sync::OnceLock;
use std::time::Duration;

/// Retries after a failure that happened before the stream opened (see `nova_tentativa`).
pub const MAX_TENTATIVAS: u32 = 4;
/// A `Retry-After` longer than this is reported instead of waited for: a turn that sits
/// silent for minutes looks hung, and the person can decide to come back later.
const RETRY_AFTER_MAX: Duration = Duration::from_secs(120);
/// Backoff without `Retry-After`: 2, 4, 8, 16 s (capped), plus up to 20% jitter.
const BACKOFF_BASE: Duration = Duration::from_secs(2);
const BACKOFF_MAX: Duration = Duration::from_secs(30);

/// Client HTTP compartilhado do processo: reusa pool de conexões e sessão TLS
/// entre voltas (antes: `Client::new()` a cada chamada = handshake TCP+TLS
/// por turno contra o gateway).
fn http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new)
}

/// Agregado de uma volta (o `on_event` já cuidou do render ao vivo).
#[derive(Default)]
pub struct TurnResult {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: Option<Usage>,
    pub error: Option<String>,
    pub model: Option<String>,
    pub server_label: Option<String>,
    /// The failure happened before any event reached the frontend (HTTP status before the
    /// stream, or no connection at all), and it is one that clears by itself: a retry repeats
    /// nothing the person has seen. See `nova_tentativa`.
    pub retentavel: bool,
    /// The gateway's `Retry-After`, in seconds (Laravel's throttle sends it with every 429).
    pub retry_after: Option<Duration>,
    /// What the gateway said about the target (0.15.0); default when it said nothing.
    pub limites: crate::types::Limites,
    /// The SSE error's classification (`provedor_recusou_pedido`, …), when it came with one.
    pub codigo: Option<String>,
    /// The person stopped the turn while the request was being sent or streamed (0.14.0).
    /// `text` holds what was already shown; tool calls of a cut-off answer are dropped.
    pub interrompido: bool,
}

/// How long to wait before retrying a failed round, or `None` when it must not be retried.
///
/// Why this exists: the gateway throttles each key (`/code/chat`, 20 requests/minute by
/// default) and every round of a turn is one request, while reads run without approval. A
/// fast turn goes over the limit, and until 0.13.10 the 429 ended the turn with an error and
/// the work stopped mid-way. `tentativa` counts retries already made (0 on the first failure).
///
/// The gateway's `Retry-After` is a floor raised to the backoff, never a wait shorter than it.
/// Laravel's throttle answers `Retry-After: 0` in the last second of its window, and until
/// 0.15.10 that was taken as given: the four retries went out in the same second, all four came
/// back 429, and the turn ended (measured 05/11/2026, two agents on one account).
pub fn nova_tentativa(r: &TurnResult, tentativa: u32) -> Option<Duration> {
    if !r.retentavel || tentativa >= MAX_TENTATIVAS {
        return None;
    }
    if r.retry_after.is_some_and(|espera| espera > RETRY_AFTER_MAX) {
        return None;
    }
    let base = BACKOFF_BASE.saturating_mul(1 << tentativa.min(8)).min(BACKOFF_MAX);
    // Jitter without a random-number crate: the clock's sub-second nanoseconds are
    // enough to keep two processes from retrying in the same instant.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let recuo = base + base.mul_f64(0.2 * f64::from(nanos % 1000) / 1000.0);
    Some(r.retry_after.map_or(recuo, |espera| espera.max(recuo)))
}

/// Statuses that clear by themselves: rate limit, request timeout, and the gateway or its
/// proxy being briefly unavailable (deploy, maintenance mode). A 500 is a gateway defect and
/// a 4xx is a request that will fail the same way again, so neither is retried.
fn status_retentavel(status: u16) -> bool {
    matches!(status, 408 | 429 | 502 | 503 | 504)
}

fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

pub async fn code_chat(
    cfg: &Config,
    messages: &[Message],
    tools: Vec<ToolDef>,
    on_event: &mut dyn FnMut(&Event),
) -> Result<TurnResult> {
    let api_key = cfg
        .api_key
        .clone()
        .ok_or_else(|| anyhow!("sem API key — exporte SHVIA_API_KEY (ou configure ~/.config/shvia-code/config.toml)"))?;

    let body = ChatRequest {
        messages,
        tools,
        profile_name: cfg.profile.clone(),
        effort: cfg.effort.clone(),
    };
    let envio = http_client()
        .post(format!("{}/api/v1/code/chat", cfg.url.trim_end_matches('/')))
        .header("Accept", "text/event-stream")
        .header("X-API-Key", api_key)
        .json(&body)
        .send();
    let enviado = tokio::select! {
        r = envio => r,
        _ = interrupcao::esperar() => return Ok(TurnResult { interrompido: true, ..Default::default() }),
    };
    let resp = match enviado {
        Ok(resp) => resp,
        // Only a connection that was never established is retried: the request did not reach
        // the gateway, so nothing ran or was billed. A failure after the request was sent may
        // have started an inference, and repeating it could pay for it twice.
        Err(e) if e.is_connect() => {
            return Ok(TurnResult {
                error: Some(format!("sem conexão com o gateway ({}): {e}", cfg.url)),
                retentavel: true,
                ..Default::default()
            });
        }
        Err(e) => return Err(e.into()),
    };

    let status = resp.status();
    if !status.is_success() {
        let retentavel = status_retentavel(status.as_u16());
        let espera = retry_after(resp.headers());
        // Erro pré-stream (4xx/5xx): mensagem amigável nos casos comuns, senão
        // o corpo {detail}/{message} ou o código cru.
        let txt = resp.text().await.unwrap_or_default();
        let body = serde_json::from_str::<serde_json::Value>(&txt).ok();
        let body_detail = body.as_ref().and_then(|v| {
            v.get("detail").or_else(|| v.get("message")).and_then(|d| d.as_str()).map(String::from)
        });
        let detail = match status.as_u16() {
            401 => "não autorizado (401): chave ausente, inválida ou expirada. \
                    Gere em ai.shvia.org → Conta → \"Gerar Chave de API\" e exporte \
                    SHVIA_API_KEY (formato shvia_usr_…) — a APP_KEY base64: NÃO serve."
                .to_string(),
            403 => format!(
                "proibido (403): {}",
                body_detail.unwrap_or_else(|| "sua conta não tem permissão de API (can_use_api) — fale com um admin.".into())
            ),
            // The throttle counts per ACCOUNT, every agent of it together (Laravel keys it by user id),
            // not per key: two agents open at once share it.
            429 => "limite atingido (429): pedidos por minuto da sua conta no gateway (20 por padrão), \
                    somando todos os agentes abertos — espere um instante."
                .to_string(),
            _ => body_detail.unwrap_or_else(|| format!("HTTP {status}")),
        };
        return Ok(TurnResult { error: Some(detail), retentavel, retry_after: espera, ..Default::default() });
    }

    let mut result = TurnResult::default();
    let mut buf: Vec<u8> = Vec::new();
    let mut stream = resp.bytes_stream();
    loop {
        // Dropping the stream on a stop closes the connection; the gateway sees the client
        // leave, as it does when a browser tab closes.
        let chunk = tokio::select! {
            c = stream.next() => c,
            _ = interrupcao::esperar() => {
                result.interrompido = true;
                result.tool_calls.clear();
                return Ok(result);
            }
        };
        let Some(chunk) = chunk else { break };
        buf.extend_from_slice(&chunk?);
        while let Some(pos) = find(&buf, b"\n\n") {
            let frame: Vec<u8> = buf.drain(..pos + 2).collect();
            let frame = String::from_utf8_lossy(&frame);
            for line in frame.lines() {
                let Some(json) = line.strip_prefix("data: ") else { continue };
                let Ok(raw) = serde_json::from_str::<RawEvent>(json) else { continue };
                let ev = Event::from_raw(raw);
                match &ev {
                    Event::Model { model, server_label, limites } => {
                        result.model = Some(model.clone());
                        result.server_label = Some(server_label.clone());
                        result.limites = *limites;
                    }
                    Event::TextDelta(d) => result.text.push_str(d),
                    Event::ToolCall(c) => result.tool_calls.push(c.clone()),
                    Event::Usage(u) => result.usage = Some(u.clone()),
                    Event::Error { message, codigo } => {
                        result.error = Some(message.clone());
                        result.codigo = codigo.clone();
                    }
                    Event::Done { text, tool_calls } => {
                        if result.text.is_empty() {
                            result.text = text.clone();
                        }
                        if result.tool_calls.is_empty() {
                            result.tool_calls = tool_calls.clone();
                        }
                    }
                    Event::ReasoningDelta | Event::Other => {}
                }
                on_event(&ev);
            }
        }
    }
    Ok(result)
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use crate::types::{Event, RawEvent};

    #[test]
    fn nova_tentativa_so_para_falha_que_passa_sozinha() {
        use super::{nova_tentativa, TurnResult, MAX_TENTATIVAS};
        use std::time::Duration;
        let falha = |retentavel, segundos: Option<u64>| TurnResult {
            error: Some("x".into()),
            retentavel,
            retry_after: segundos.map(Duration::from_secs),
            ..Default::default()
        };
        // A Retry-After longer than the backoff is what the throttle needs, so it is used as given.
        assert_eq!(nova_tentativa(&falha(true, Some(7)), 0), Some(Duration::from_secs(7)));
        // A shorter one (Laravel sends 0 in the window's last second) is raised to the backoff:
        // four retries in the same second all come back 429 and end the turn.
        let zero = nova_tentativa(&falha(true, Some(0)), 0).unwrap();
        let zero_terceira = nova_tentativa(&falha(true, Some(0)), 2).unwrap();
        assert!(zero >= Duration::from_secs(2) && zero < Duration::from_secs(3), "{zero:?}");
        assert!(zero_terceira >= Duration::from_secs(8), "{zero_terceira:?}");
        // 401/403/422 and SSE errors: the same request fails the same way again.
        assert_eq!(nova_tentativa(&falha(false, Some(7)), 0), None);
        // A wait longer than the ceiling is reported, not sat through in silence.
        assert_eq!(nova_tentativa(&falha(true, Some(600)), 0), None);
        // Without Retry-After: growing backoff with jitter, never past the attempt limit.
        let primeira = nova_tentativa(&falha(true, None), 0).unwrap();
        let terceira = nova_tentativa(&falha(true, None), 2).unwrap();
        assert!(primeira >= Duration::from_secs(2) && primeira < Duration::from_secs(3), "{primeira:?}");
        assert!(terceira >= Duration::from_secs(8) && terceira < Duration::from_secs(10), "{terceira:?}");
        assert_eq!(nova_tentativa(&falha(true, None), MAX_TENTATIVAS), None);
    }

    #[test]
    fn chat_request_serializa_effort() {
        use crate::types::{ChatRequest, Message};
        let msgs = vec![Message::user("oi")];
        let com = ChatRequest { messages: &msgs, tools: vec![], profile_name: None, effort: Some("high".into()) };
        assert!(serde_json::to_string(&com).unwrap().contains("\"effort\":\"high\""));
        let sem = ChatRequest { messages: &msgs, tools: vec![], profile_name: None, effort: None };
        assert!(!serde_json::to_string(&sem).unwrap().contains("effort"));
    }

    #[test]
    fn o_evento_model_traz_os_limites_do_alvo() {
        use crate::types::Limites;
        let com: RawEvent = serde_json::from_str(
            r#"{"kind":"model","data":{"model":"m","server_label":"S","messages_max":5000,"context_window":32768}}"#,
        )
        .unwrap();
        let sem: RawEvent = serde_json::from_str(r#"{"kind":"model","data":{"model":"m","server_label":"S"}}"#).unwrap();
        match (Event::from_raw(com), Event::from_raw(sem)) {
            (Event::Model { limites: a, .. }, Event::Model { limites: b, .. }) => {
                assert_eq!(a, Limites { mensagens: Some(5000), janela: Some(32768) });
                // An older gateway says nothing: the client keeps the count it always used.
                assert_eq!(b, Limites::default());
            }
            _ => panic!("esperava Model"),
        }
    }

    #[test]
    fn parse_eventos_do_protocolo() {
        let raw: RawEvent = serde_json::from_str(
            r#"{"kind":"tool_call","data":{"id":"c1","name":"read_file","arguments":{"path":"x"}}}"#,
        )
        .unwrap();
        match Event::from_raw(raw) {
            Event::ToolCall(c) => {
                assert_eq!(c.name, "read_file");
                assert_eq!(c.arguments["path"], "x");
            }
            _ => panic!("esperava ToolCall"),
        }
    }
}
