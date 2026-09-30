//! Quem EXECUTA as ferramentas (ADR-002, modelo B). Dois corpos:
//! - `LocalRunner`: o `anna` roda as tools na máquina dele (default — terminal
//!   e `--json` sem host). A aprovação acontece dentro de `tools::run` (via `fe`).
//! - `HostRunner`: o `anna` só PROPÕE (o evento `tool_call` já saiu no stream);
//!   a casca externa executa com a política/PTY/auditoria dela e injeta o
//!   resultado no stdin como `{"type":"tool_result","id":...,"content":...}`.
//!   O `anna` NÃO executa nem aprova nada aqui — é cérebro, não mãos.

use crate::frontend::Frontend;
use crate::tools;
use crate::types::{ToolCall, ToolDef};
use std::path::Path;

pub trait ToolRunner {
    /// Devolve o `content` do `role=tool` pra próxima volta do loop.
    fn run(&mut self, call: &ToolCall, root: &Path, fe: &mut dyn Frontend) -> String;
    /// Ferramentas oferecidas ao modelo neste modo (local = todas; host = só bash).
    fn tool_defs(&self) -> Vec<ToolDef>;
}

pub struct LocalRunner;

impl ToolRunner for LocalRunner {
    fn run(&mut self, call: &ToolCall, root: &Path, fe: &mut dyn Frontend) -> String {
        let out = tools::run(call, root, fe);
        fe.tool_result(call, &out);
        out
    }

    fn tool_defs(&self) -> Vec<ToolDef> {
        tools::defs()
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

    fn tool_defs(&self) -> Vec<ToolDef> {
        tools::defs_host()
    }
}

/// Decorador que corta as ferramentas de ESCRITA — é o que faz o arquiteto e o
/// revisor do orquestrador (`papel::Papel::so_leitura`) serem inofensivos por
/// construção, não por instrução de prompt.
///
/// Filtra dos dois lados de propósito: o modelo nem enxerga a ferramenta (não é
/// tentado), e se pedir mesmo assim — alucinando o nome, ou porque o histórico
/// de outra fase sugeriu — a chamada é RECUSADA antes de chegar no executor.
/// `bash` fica de fora da lista: comando de leitura e comando que grava entram
/// pela mesma porta.
pub struct SomenteLeitura(pub Box<dyn ToolRunner>);

// `todo_write` writes nothing on disk: a plan turn keeps its task list too (0.13.0).
const LEITURA: &[&str] =
    &["read_file", "list_dir", "grep", "glob", "git_status", "git_diff", "todo_write"];

impl ToolRunner for SomenteLeitura {
    fn run(&mut self, call: &ToolCall, root: &Path, fe: &mut dyn Frontend) -> String {
        if !LEITURA.contains(&call.name.as_str()) {
            return format!(
                "RECUSADO: este papel é só-leitura e não pode chamar '{}'. \
                 Disponíveis: {}.",
                call.name,
                LEITURA.join(", ")
            );
        }
        self.0.run(call, root, fe)
    }

    fn tool_defs(&self) -> Vec<ToolDef> {
        self.0
            .tool_defs()
            .into_iter()
            .filter(|d| LEITURA.contains(&d.name.as_str()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn so_leitura_filtra_defs_e_recusa_escrita() {
        let ro = SomenteLeitura(Box::new(LocalRunner));
        let nomes: Vec<String> = ro.tool_defs().into_iter().map(|d| d.name).collect();
        assert!(nomes.contains(&"read_file".to_string()));
        assert!(nomes.contains(&"todo_write".to_string()), "a plan turn keeps its task list: it writes nothing on disk");
        for proibida in ["edit_file", "write_file", "bash", "git_commit"] {
            assert!(!nomes.contains(&proibida.to_string()), "{proibida} não devia ser oferecida");
        }
    }

    #[test]
    fn so_leitura_recusa_mesmo_se_o_modelo_insistir() {
        let mut ro = SomenteLeitura(Box::new(LocalRunner));
        let mut fe = crate::ui::Ui::new(true);
        let call = ToolCall {
            id: "t1".into(),
            name: "write_file".into(),
            arguments: serde_json::json!({"path": "x.txt", "content": "x"}),
        };
        let out = ro.run(&call, Path::new("."), &mut fe);
        assert!(out.starts_with("RECUSADO"), "{out}");
    }
}
