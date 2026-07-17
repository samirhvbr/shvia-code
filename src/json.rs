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

    fn approve(&mut self, policy: Policy, scope: &str, id: &str, preview: &Preview) -> bool {
        if policy == Policy::Auto {
            return true;
        }
        if policy == Policy::Confirm && self.allowed.contains(scope) {
            return true;
        }
        self.emit(json!({
            "type": "gate_request",
            "id": id,
            "scope": scope,
            "policy": policy.as_str(),
            "preview": preview_json(preview),
        }));
        // Bloqueia até chegar a decisão do host no stdin:
        //   {"decision":"approve"|"always"|"reject"}
        // Robustez: linhas em branco ou sem "decision" (ex.: um {"type":"user"} que
        // chegou fora de hora) são IGNORADAS em vez de virarem um reject silencioso;
        // um {"type":"exit"} durante o gate cancela (nega, seguro); EOF → nega.
        loop {
            let mut line = String::new();
            if io::stdin().read_line(&mut line).unwrap_or(0) == 0 {
                return false; // stdin fechado → nega (seguro)
            }
            let t = line.trim();
            if t.is_empty() {
                continue;
            }
            let v: Value = serde_json::from_str(t).unwrap_or(Value::Null);
            if let Some(decision) = v.get("decision").and_then(|d| d.as_str()) {
                return match decision {
                    "approve" => true,
                    "always" if policy == Policy::Confirm => {
                        self.allowed.insert(scope.to_string());
                        true
                    }
                    _ => false,
                };
            }
            if v.get("type").and_then(|x| x.as_str()) == Some("exit") {
                return false; // cancela o gate; o motor encerra logo em seguida
            }
            // qualquer outra linha durante um gate pendente: ignora e segue esperando
        }
    }
}
