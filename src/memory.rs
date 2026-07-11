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
pub fn init(cwd: &Path) {
    let _ = MEM.set(match std::env::var("AI_MEMORY_URL") {
        Ok(url) if !url.trim().is_empty() => {
            let sid = format!(
                "anna-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            );
            Some(Mem {
                url: url.trim().trim_end_matches('/').to_string(),
                token: std::env::var("AI_MEMORY_TOKEN").ok().filter(|t| !t.trim().is_empty()),
                session_id: sid,
                cwd: cwd.display().to_string(),
                client: reqwest::Client::new(),
            })
        }
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
