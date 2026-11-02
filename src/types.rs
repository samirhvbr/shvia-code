//! Structs do protocolo (docs/protocolo.md) — o que vai/volta no
//! `POST /api/v1/code/chat`. Serde apenas; sem lógica.

use serde::{Deserialize, Serialize};

/// Uma mensagem do histórico. `content` sempre presente (mesmo vazio p/ um
/// assistant que só chamou tools). `tool_call_id` só em `role="tool"`;
/// `tool_calls` só em `role="assistant"`.
///
/// `images` é base64 PURO (sem o prefixo `data:`), a convenção do Ollama que o
/// gateway já fala: `ChatService` monta `$entry['images']` assim, e os drivers
/// Anthropic/OpenAI derivam o mime dos magic bytes (`ImageMime::detect`) — por
/// isso o mime NÃO viaja aqui. `skip_serializing_if` mantém o corpo idêntico ao
/// de antes em todo turno sem imagem, que é 100% dos turnos de hoje.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    #[serde(default)]
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<String>,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: "system".into(), content: content.into(), tool_call_id: None, tool_calls: vec![], images: vec![] }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: "user".into(), content: content.into(), tool_call_id: None, tool_calls: vec![], images: vec![] }
    }
    /// Turno do usuário COM imagem. Separado do `user()` de propósito: o caminho
    /// sem imagem é 100% dos turnos de hoje e não muda de assinatura por causa
    /// de um caso novo.
    pub fn user_com_imagens(content: impl Into<String>, images: Vec<String>) -> Self {
        Self { role: "user".into(), content: content.into(), tool_call_id: None, tool_calls: vec![], images }
    }
    pub fn assistant(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self { role: "assistant".into(), content: content.into(), tool_call_id: None, tool_calls, images: vec![] }
    }
    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self { role: "tool".into(), content: content.into(), tool_call_id: Some(tool_call_id.into()), tool_calls: vec![], images: vec![] }
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
    /// What the provider counted for the request just sent. Calibrates the estimate of the
    /// next request's size against the target's window (0.15.0).
    #[serde(default)]
    pub prompt_tokens: Option<u64>,
    #[serde(default)]
    pub cost: Option<f64>,
    #[serde(default)]
    pub cost_estimated: Option<f64>,
}

/// What the gateway says about the target in the `model` event (0.15.0; absent from older
/// gateways): `mensagens` = the per-request message ceiling it validates; `janela` = the
/// model's effective context window in tokens, when the gateway knows it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Limites {
    pub mensagens: Option<usize>,
    pub janela: Option<u64>,
}

/// Evento do stream SSE, já tipado (docs/protocolo.md §3).
#[derive(Debug, Clone)]
pub enum Event {
    Model { model: String, server_label: String, limites: Limites },
    TextDelta(String),
    /// "Pensando ao vivo" — o conteúdo é ignorado no C1 (render no C3).
    ReasoningDelta,
    ToolCall(ToolCall),
    Usage(Usage),
    /// `codigo` classifies it (`provedor_recusou_pedido` = the provider refused the request
    /// itself, as Ollama does for a prompt over `num_ctx`); `status` is the provider's HTTP code.
    Error { message: String, codigo: Option<String> },
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
            "model" => Event::Model {
                model: s("model"),
                server_label: s("server_label"),
                limites: Limites {
                    mensagens: d.get("messages_max").and_then(|v| v.as_u64()).map(|v| v as usize),
                    janela: d.get("context_window").and_then(|v| v.as_u64()),
                },
            },
            "text_delta" => Event::TextDelta(s("delta")),
            "reasoning_delta" => Event::ReasoningDelta,
            "tool_call" => serde_json::from_value::<ToolCall>(d).map(Event::ToolCall).unwrap_or(Event::Other),
            "usage" => Event::Usage(serde_json::from_value(d).unwrap_or_default()),
            "error" => Event::Error {
                message: s("message"),
                codigo: d.get("codigo").and_then(|v| v.as_str()).map(str::to_string),
            },
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
