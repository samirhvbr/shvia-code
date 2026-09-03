//! Captura opcional para o **ai-memory** (memória de longo prazo multi-agente,
//! github.com/akitaonrails/ai-memory): cada prompt do usuário, execução de
//! ferramenta e fronteira de sessão vira um `POST {AI_MEMORY_URL}/hook` — o
//! MESMO protocolo dos lifecycle hooks do Claude Code/Codex, com `agent=anna`
//! (agente desconhecido é aceito e vira `Other`; o payload cru é preservado).
//!
//! LIGA com a env `AI_MEMORY_URL` (+ `AI_MEMORY_TOKEN` se o servidor exigir
//! bearer); sem ela é NO-OP absoluto. Fire-and-forget: cada evento é uma task
//! tokio com timeout curto — falha de rede/servidor NUNCA atrapalha o loop do
//! agente (sem retry, sem log). Só o `session-end` é síncrono (na saída, para
//! o evento não morrer com o processo).

use serde_json::json;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

struct Mem {
    url: String,
    token: Option<String>,
    session_id: String,
    cwd: String,
    client: reqwest::Client,
}

static MEM: OnceLock<Option<Mem>> = OnceLock::new();
static STARTED: AtomicBool = AtomicBool::new(false);

/// Chame uma vez no boot (main), com a raiz do projeto. Sem `AI_MEMORY_URL`,
/// tudo daqui pra frente é no-op.
/// A `AI_MEMORY_URL` pode receber o hook? `Ok(url normalizada)` ou `Err(motivo)`.
///
/// ## Por que é função separada (achado F-07 da revisão de 01/09/2026)
///
/// O `init` grava num `OnceLock` — uma vez por processo, e portanto impossível de exercitar
/// mais de uma vez num teste. A decisão em si é pura e é o que precisa de prova, então ela
/// sai daqui e o `init` passa a só aplicá-la. Mesma escolha do `politica.mjs` no
/// SHVIA-DESKTOP e das guardas do `deploy.sh` no WORKSPACE.
///
/// ## O que ela decide
///
/// O hook manda `bearer_auth` + prompts + 400 chars de cada resultado de tool. Em `http://`
/// fora do loopback, isso é token e conteúdo de trabalho em claro na rede.
///
/// A regra é a MESMA do `SHVIA_URL` — `config::url_seguro`, importada e não recopiada. Ela já
/// existe, já tem prova (incluindo `http://localhost@atacante.tld` e
/// `http://127.0.0.1.atacante.tld`), e uma segunda cópia divergiria: divergência em guarda de
/// segredo é o defeito que esta revisão mais encontrou.
pub(crate) fn url_de_memoria(bruta: &str) -> Result<String, String> {
    let u = bruta.trim();
    if u.is_empty() {
        return Err("vazia".into());
    }
    if !crate::config::url_seguro(u) {
        return Err(format!(
            "AI_MEMORY_URL insegura ({u}) — o hook manda token e conteúdo dos turnos. \
             Use https:// (exceção: http://localhost|127.x|[::1])."
        ));
    }
    Ok(u.trim_end_matches('/').to_string())
}

pub fn init(cwd: &Path) {
    let _ = MEM.set(match std::env::var("AI_MEMORY_URL") {
        Ok(bruta) if !bruta.trim().is_empty() => match url_de_memoria(&bruta) {
            Ok(url) => {
                let sid = format!(
                    "anna-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0),
                );
                Some(Mem {
                    url,
                    token: std::env::var("AI_MEMORY_TOKEN").ok().filter(|t| !t.trim().is_empty()),
                    session_id: sid,
                    cwd: cwd.display().to_string(),
                    client: reqwest::Client::new(),
                })
            }
            Err(motivo) => {
                // DESLIGA em vez de recusar o boot: memória é opcional aqui — o `anna`
                // funciona sem ela, e derrubar a ferramenta por causa de um hook mal
                // configurado trocaria um vazamento por uma indisponibilidade.
                eprintln!("aviso: {motivo}\n       Memória de longo prazo DESLIGADA.");
                None
            }
        },
        _ => None,
    });
}

fn mem() -> Option<&'static Mem> {
    MEM.get().and_then(|o| o.as_ref())
}

/// Monta o request FORA do future (nada de empréstimos dentro da task).
fn post(m: &Mem, event: &'static str, mut body: serde_json::Value) -> impl std::future::Future<Output = ()> + Send {
    if let Some(o) = body.as_object_mut() {
        o.insert("session_id".into(), json!(m.session_id));
        o.insert("cwd".into(), json!(m.cwd));
    }
    let mut req = m
        .client
        .post(format!("{}/hook", m.url))
        .query(&[("event", event), ("agent", "anna"), ("cwd", m.cwd.as_str())])
        .timeout(Duration::from_millis(1500))
        .json(&body);
    if let Some(t) = &m.token {
        req = req.bearer_auth(t);
    }
    async move {
        let _ = req.send().await; // best-effort: silêncio em qualquer falha
    }
}

/// Dispara em background. Precisa de runtime tokio ativo — os call sites do
/// `run_turn` rodam dentro do `block_on` do main; fora de runtime, descarta.
fn spawn(event: &'static str, body: serde_json::Value) {
    let Some(m) = mem() else { return };
    let Ok(handle) = tokio::runtime::Handle::try_current() else { return };
    // O primeiro evento da sessão emite o session-start antes (uma vez).
    if !STARTED.swap(true, Ordering::SeqCst) {
        handle.spawn(post(m, "session-start", json!({})));
    }
    handle.spawn(post(m, event, body));
}

/// Prompt do usuário que abre o turno.
pub fn user_prompt(text: &str) {
    spawn("user-prompt", json!({ "prompt": text }));
}

/// Ferramenta executada (resultado truncado — o resumo basta pra memória).
pub fn tool_use(name: &str, arguments: &serde_json::Value, result: &str) {
    let summary: String = result.chars().take(400).collect();
    spawn(
        "post-tool-use",
        json!({
            "tool_name": name,
            "tool_input": arguments,
            "tool_result_summary": summary,
        }),
    );
}

/// Fim de sessão — SÍNCRONO (com o timeout do post) para o evento não morrer
/// com o processo. Chame nos pontos de saída do main. No-op se a sessão nem
/// chegou a emitir nada.
pub fn session_end_blocking(rt: &tokio::runtime::Runtime) {
    let Some(m) = mem() else { return };
    if !STARTED.load(Ordering::SeqCst) {
        return;
    }
    rt.block_on(post(m, "session-end", json!({})));
}

#[cfg(test)]
mod tests_url_de_memoria {
    use super::url_de_memoria;

    /// Achado F-07: o hook aceitava qualquer esquema e mandava token + conteúdo.
    #[test]
    fn https_passa_e_normaliza() {
        assert_eq!(url_de_memoria("https://mem.blue3.local/").unwrap(), "https://mem.blue3.local");
        assert_eq!(url_de_memoria("  https://x.tld/api//  ").unwrap(), "https://x.tld/api");
    }

    #[test]
    fn loopback_em_http_passa_porque_nao_sai_da_maquina() {
        for u in ["http://localhost:8000", "http://127.0.0.1:7000", "http://[::1]:9000"] {
            assert!(url_de_memoria(u).is_ok(), "deveria passar: {u}");
        }
    }

    #[test]
    fn http_para_fora_e_recusado_com_motivo() {
        for u in ["http://mem.blue3.local", "http://10.0.0.5:7000", "ftp://x"] {
            let e = url_de_memoria(u).unwrap_err();
            assert!(e.contains("insegura"), "motivo pobre para {u}: {e}");
            // A mensagem tem de dizer O QUE fazer: sem isso o dev desliga a memória
            // em vez de consertar a URL.
            assert!(e.contains("https://"), "a mensagem não diz o caminho: {e}");
        }
    }

    #[test]
    fn os_disfarces_de_loopback_nao_passam() {
        // Herdados da prova do `url_seguro` — o ponto de reusar a função é justamente
        // não ter de reencontrar estes casos.
        for u in [
            "http://localhost.atacante.tld",
            "http://localhost@atacante.tld",
            "http://127.0.0.1.atacante.tld",
            "http://[::1]@atacante.tld",
        ] {
            assert!(url_de_memoria(u).is_err(), "deveria recusar: {u}");
        }
    }

    #[test]
    fn vazia_e_recusada_sem_alarde() {
        assert_eq!(url_de_memoria("   ").unwrap_err(), "vazia");
    }
}
