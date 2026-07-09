//! Structs do protocolo (docs/protocolo.md) — o que vai/volta no
//! `POST /api/v1/code/chat`. Serde apenas; sem lógica.

use serde::{Deserialize, Serialize};

/// Uma mensagem do histórico. `content` sempre presente (mesmo vazio p/ um
/// assistant que só chamou tools). `tool_call_id` só em `role="tool"`;
/// `tool_calls` só em `role="assistant"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    #[serde(default)]
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: "system".into(), content: content.into(), tool_call_id: None, tool_calls: vec![] }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: "user".into(), content: content.into(), tool_call_id: None, tool_calls: vec![] }
    }
    pub fn assistant(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self { role: "assistant".into(), content: content.into(), tool_call_id: None, tool_calls }
    }
    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self { role: "tool".into(), content: content.into(), tool_call_id: Some(tool_call_id.into()), tool_calls: vec![] }
    }
}

/// Chamada de ferramenta (a "Action"). `arguments` é um objeto JSON já
/// decodificado — reenviado tal e qual ao servidor (o driver tolera).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default = "empty_obj")]
    pub arguments: serde_json::Value,
}

fn empty_obj() -> serde_json::Value {
    serde_json::json!({})
}

/// Definição de ferramenta enviada ao modelo (`tools[]`).
#[derive(Debug, Clone, Serialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Corpo do POST.
#[derive(Debug, Serialize)]
pub struct ChatRequest<'a> {
    pub messages: &'a [Message],
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

/// Uso/telemetria do evento `usage`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub total_tokens: i64,
    #[serde(default)]
    pub cost: Option<f64>,
    #[serde(default)]
    pub cost_estimated: Option<f64>,
}

/// Evento do stream SSE, já tipado (docs/protocolo.md §3).
#[derive(Debug, Clone)]
pub enum Event {
    Model { model: String, server_label: String },
    TextDelta(String),
    /// "Pensando ao vivo" — o conteúdo é ignorado no C1 (render no C3).
    ReasoningDelta,
    ToolCall(ToolCall),
    Usage(Usage),
    Error(String),
    Done { text: String, tool_calls: Vec<ToolCall> },
    Other,
}

/// Frame cru `{kind, data}` antes de virar `Event`.
#[derive(Debug, Deserialize)]
pub struct RawEvent {
    pub kind: String,
    #[serde(default)]
    pub data: serde_json::Value,
}

impl Event {
    pub fn from_raw(raw: RawEvent) -> Self {
        let d = raw.data;
        let s = |k: &str| d.get(k).and_then(|v| v.as_str()).unwrap_or_default().to_string();
        match raw.kind.as_str() {
            "model" => Event::Model { model: s("model"), server_label: s("server_label") },
            "text_delta" => Event::TextDelta(s("delta")),
            "reasoning_delta" => Event::ReasoningDelta,
            "tool_call" => serde_json::from_value::<ToolCall>(d).map(Event::ToolCall).unwrap_or(Event::Other),
            "usage" => Event::Usage(serde_json::from_value(d).unwrap_or_default()),
            "error" => Event::Error(s("message")),
            "done" => {
                let text = d.get("text").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                let tool_calls = d
                    .get("tool_calls")
                    .and_then(|v| serde_json::from_value::<Vec<ToolCall>>(v.clone()).ok())
                    .unwrap_or_default();
                Event::Done { text, tool_calls }
            }
            _ => Event::Other,
        }
    }
}
