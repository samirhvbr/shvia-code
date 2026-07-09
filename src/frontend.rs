//! O que o loop precisa do "mundo": renderizar os eventos do turno e DECIDIR
//! aprovações. Abstrair isso num trait é o que deixa o motor **embutível** —
//! o mesmo `agent::run_turn` serve o terminal (`Ui`, ANSI+stdin) e um host
//! externo (`JsonSink`, NDJSON — ex.: a aba Agente do SShvTerm). Quem mostra o
//! preview é quem pergunta: por isso `approve` mora aqui, não num gate à parte.
//! Protocolo NDJSON: docs/embedding.md.

use crate::gate::{Policy, Preview};
use crate::telemetry::Telemetry;
use crate::types::{Event, ToolCall};

pub trait Frontend {
    /// Evento ao vivo do stream (model / text_delta / tool_call).
    fn on_event(&mut self, ev: &Event);
    /// Fecha o bloco de texto da volta.
    fn end_stream(&mut self);
    /// Resultado de uma ferramenta executada localmente.
    fn tool_result(&mut self, call: &ToolCall, out: &str);
    /// Fim do turno: telemetria (tokens/custo).
    fn turn_end(&mut self, tel: &Telemetry);
    fn error(&mut self, msg: &str);
    fn warn(&mut self, msg: &str);
    /// Mostra o `preview` e decide se a ação roda (o gate). `Auto` nunca pergunta.
    /// `id` = o `tool_call.id` (correlaciona a proposta com a decisão e o resultado
    /// no host — mesmo id no `gate_request` e no `tool_result`).
    fn approve(&mut self, policy: Policy, scope: &str, id: &str, preview: &Preview) -> bool;
}
