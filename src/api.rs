//! Cliente do gateway: `POST /api/v1/code/chat` + parser do SSE
//! (docs/protocolo.md). Uma chamada = uma "volta": streama os eventos (via
//! `on_event`, p/ a UI renderizar ao vivo) e devolve o agregado da volta.

use crate::config::Config;
use crate::types::{ChatRequest, Event, Message, RawEvent, ToolCall, ToolDef, Usage};
use anyhow::{anyhow, Result};
use futures_util::StreamExt;

/// Agregado de uma volta (o `on_event` já cuidou do render ao vivo).
#[derive(Default)]
pub struct TurnResult {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: Option<Usage>,
    pub error: Option<String>,
    pub model: Option<String>,
    pub server_label: Option<String>,
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

    let body = ChatRequest { messages, tools, profile_name: cfg.profile.clone() };
    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/code/chat", cfg.url.trim_end_matches('/')))
        .header("Accept", "text/event-stream")
        .header("X-API-Key", api_key)
        .json(&body)
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        // Erro pré-stream (4xx/5xx): corpo {detail} ou texto cru.
        let txt = resp.text().await.unwrap_or_default();
        let detail = serde_json::from_str::<serde_json::Value>(&txt)
            .ok()
            .and_then(|v| v.get("detail").and_then(|d| d.as_str()).map(String::from))
            .unwrap_or_else(|| format!("HTTP {status}"));
        return Ok(TurnResult { error: Some(detail), ..Default::default() });
    }

    let mut result = TurnResult::default();
    let mut buf: Vec<u8> = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        buf.extend_from_slice(&chunk?);
        while let Some(pos) = find(&buf, b"\n\n") {
            let frame: Vec<u8> = buf.drain(..pos + 2).collect();
            let frame = String::from_utf8_lossy(&frame);
            for line in frame.lines() {
                let Some(json) = line.strip_prefix("data: ") else { continue };
                let Ok(raw) = serde_json::from_str::<RawEvent>(json) else { continue };
                let ev = Event::from_raw(raw);
                match &ev {
                    Event::Model { model, server_label } => {
                        result.model = Some(model.clone());
                        result.server_label = Some(server_label.clone());
                    }
                    Event::TextDelta(d) => result.text.push_str(d),
                    Event::ToolCall(c) => result.tool_calls.push(c.clone()),
                    Event::Usage(u) => result.usage = Some(u.clone()),
                    Event::Error(e) => result.error = Some(e.clone()),
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
