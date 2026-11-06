//! The gateway key, replaceable while `anna` runs (0.16.0).
//!
//! Until 0.15.x the key was read once, at start (`SHVIA_API_KEY` or `config.toml`), and every
//! request of the process used it. A host that hands `anna` a short-lived key — SHVIA-DESKTOP's
//! Code mode passes the web session's Code token, valid for 12 hours — had no way to give it a
//! new one: when the token expired the next request got a 401, and the only way out was to kill
//! the process and lose the session's history.
//!
//! Measured on 06/11/2026: an autonomous run paused at 19:57 waiting for the person's answer,
//! the answer came at 07:51 the next day, and the first request of the resumed iteration was
//! refused with 401 — the token had expired overnight. The run ended as "interrupted: engine".
//!
//! Now an NDJSON host sends `{"type":"credential","api_key":"…"}` at any time — between turns,
//! during a turn, while a gate waits — and the next request uses it. The line is taken by the
//! stdin reader and never queued, logged or echoed (see `interrupcao`).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

static ATUAL: Mutex<Option<String>> = Mutex::new(None);
/// Bumped on every replacement, so a request that failed can tell whether a new key arrived
/// after it was sent.
static GERACAO: AtomicU64 = AtomicU64::new(0);

/// Replaces the key. An empty or blank key is ignored: it would turn a working session into
/// one that fails every request, and no host means that.
pub fn trocar(chave: &str) -> bool {
    let chave = chave.trim();
    if chave.is_empty() {
        return false;
    }
    *ATUAL.lock().unwrap_or_else(|e| e.into_inner()) = Some(chave.to_string());
    GERACAO.fetch_add(1, Ordering::SeqCst);
    true
}

/// The key for the next request: the last one the host sent, else the one read at start.
pub fn atual(da_config: Option<&str>) -> Option<String> {
    ATUAL
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .or_else(|| da_config.map(str::to_string))
}

pub fn geracao() -> u64 {
    GERACAO.load(Ordering::SeqCst)
}

/// The key a `credential` line carries, or `None` when the line is not one.
pub fn da_linha(v: &serde_json::Value) -> Option<&str> {
    if v.get("type").and_then(|t| t.as_str()) != Some("credential") {
        return None;
    }
    v.get("api_key").and_then(|k| k.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    // One test, because the state is process-wide: separate tests would race on it.
    #[test]
    fn a_chave_do_host_vence_a_da_config_e_a_vazia_nao_entra() {
        let antes = geracao();
        assert_eq!(atual(Some("da-config")).as_deref(), Some("da-config"));
        assert!(!trocar("   "), "a blank key must not replace a working one");
        assert_eq!(geracao(), antes);
        assert!(trocar(" nova "));
        assert_eq!(geracao(), antes + 1);
        assert_eq!(atual(Some("da-config")).as_deref(), Some("nova"));
        assert_eq!(atual(None).as_deref(), Some("nova"));

        let linha: serde_json::Value = serde_json::from_str(r#"{"type":"credential","api_key":"k"}"#).unwrap();
        assert_eq!(da_linha(&linha), Some("k"));
        let outra: serde_json::Value = serde_json::from_str(r#"{"type":"user","api_key":"k"}"#).unwrap();
        assert_eq!(da_linha(&outra), None);
    }
}
