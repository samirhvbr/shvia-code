//! `JsonSink` — o `Frontend` que fala **NDJSON** (uma linha JSON por evento) em
//! vez de ANSI. É o que permite embutir o motor do `anna` numa casca externa
//! (aba Agente do SShvTerm, extensão de editor, CI): o host lê os eventos do
//! stdout e responde as aprovações escrevendo `{"decision":...}` no stdin.
//! Contrato completo: docs/embedding.md.

use crate::diff;
use crate::frontend::Frontend;
use crate::gate::{Policy, Preview};
use crate::telemetry::Telemetry;
use crate::types::{Event, ToolCall};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::{self, Write};

#[derive(Default)]
pub struct JsonSink {
    /// Escopos liberados por "sempre nesta sessão" (decision=always).
    allowed: HashSet<String>,
}

impl JsonSink {
    pub fn new() -> Self {
        Self::default()
    }

    fn emit(&self, v: Value) {
        if let Ok(s) = serde_json::to_string(&v) {
            let stdout = io::stdout();
            let mut lock = stdout.lock();
            let _ = writeln!(lock, "{s}");
            let _ = lock.flush();
        }
    }
}

fn preview_json(p: &Preview) -> Value {
    match p {
        Preview::Diff { path, old, new } => {
            json!({"kind":"diff","path":path,"diff":diff::unified(path, old, new, false)})
        }
        Preview::Command { command, why } => json!({"kind":"command","command":command,"why":why}),
        Preview::Commit { stat, message } => json!({"kind":"commit","stat":stat,"message":message}),
    }
}

impl Frontend for JsonSink {
    fn on_event(&mut self, ev: &Event) {
        match ev {
            Event::Model { model, server_label } => {
                self.emit(json!({"type":"model","model":model,"server":server_label}))
            }
            Event::TextDelta(d) => self.emit(json!({"type":"text","delta":d})),
            Event::ToolCall(c) => {
                self.emit(json!({"type":"tool_call","id":c.id,"name":c.name,"arguments":c.arguments}))
            }
            _ => {}
        }
    }

    fn end_stream(&mut self) {}

    fn tool_result(&mut self, call: &ToolCall, out: &str) {
        self.emit(json!({"type":"tool_result","id":call.id,"name":call.name,"bytes":out.len(),"content":out}));
    }

    fn turn_end(&mut self, tel: &Telemetry) {
        self.emit(json!({"type":"usage","tokens":tel.tokens,"cost":tel.cost,"estimated":tel.estimated}));
        self.emit(json!({"type":"turn_done"}));
    }

    fn error(&mut self, msg: &str) {
        self.emit(json!({"type":"error","message":msg}));
    }

    fn warn(&mut self, msg: &str) {
        self.emit(json!({"type":"warn","message":msg}));
    }

    fn approve(&mut self, policy: Policy, scope: &str, preview: &Preview) -> bool {
        if policy == Policy::Auto {
            return true;
        }
        if policy == Policy::Confirm && self.allowed.contains(scope) {
            return true;
        }
        self.emit(json!({
            "type": "gate_request",
            "scope": scope,
            "policy": policy.as_str(),
            "preview": preview_json(preview),
        }));
        // Bloqueia esperando UMA linha de decisão do host no stdin:
        //   {"decision":"approve"|"always"|"reject"}
        let mut line = String::new();
        if io::stdin().read_line(&mut line).unwrap_or(0) == 0 {
            return false; // stdin fechado → nega (seguro)
        }
        let v: Value = serde_json::from_str(line.trim()).unwrap_or(Value::Null);
        match v.get("decision").and_then(|d| d.as_str()).unwrap_or("reject") {
            "approve" => true,
            "always" if policy == Policy::Confirm => {
                self.allowed.insert(scope.to_string());
                true
            }
            _ => false,
        }
    }
}
