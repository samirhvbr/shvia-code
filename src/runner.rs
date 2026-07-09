//! Quem EXECUTA as ferramentas (ADR-002, modelo B). Dois corpos:
//! - `LocalRunner`: o `anna` roda as tools na máquina dele (default — terminal
//!   e `--json` sem host). A aprovação acontece dentro de `tools::run` (via `fe`).
//! - `HostRunner`: o `anna` só PROPÕE (o evento `tool_call` já saiu no stream);
//!   a casca externa executa com a política/PTY/auditoria dela e injeta o
//!   resultado no stdin como `{"type":"tool_result","id":...,"content":...}`.
//!   O `anna` NÃO executa nem aprova nada aqui — é cérebro, não mãos.

use crate::frontend::Frontend;
use crate::tools;
use crate::types::ToolCall;
use std::path::Path;

pub trait ToolRunner {
    /// Devolve o `content` do `role=tool` pra próxima volta do loop.
    fn run(&mut self, call: &ToolCall, root: &Path, fe: &mut dyn Frontend) -> String;
}

pub struct LocalRunner;

impl ToolRunner for LocalRunner {
    fn run(&mut self, call: &ToolCall, root: &Path, fe: &mut dyn Frontend) -> String {
        let out = tools::run(call, root, fe);
        fe.tool_result(call, &out);
        out
    }
}

pub struct HostRunner;

impl ToolRunner for HostRunner {
    fn run(&mut self, call: &ToolCall, _root: &Path, _fe: &mut dyn Frontend) -> String {
        // Espera o resultado do host PRA ESTE tool_call (casando por id quando há).
        loop {
            let mut line = String::new();
            if std::io::stdin().read_line(&mut line).unwrap_or(0) == 0 {
                return "ERRO: host fechou o stdin sem devolver o resultado.".into();
            }
            let t = line.trim();
            if t.is_empty() {
                continue;
            }
            let v: serde_json::Value = serde_json::from_str(t).unwrap_or(serde_json::Value::Null);
            if v.get("type").and_then(|x| x.as_str()) != Some("tool_result") {
                continue; // ignora qualquer coisa que não seja o resultado
            }
            if let Some(id) = v.get("id").and_then(|x| x.as_str()) {
                if !call.id.is_empty() && id != call.id {
                    continue; // resultado de outro call — segue esperando o certo
                }
            }
            return v.get("content").and_then(|x| x.as_str()).unwrap_or_default().to_string();
        }
    }
}
