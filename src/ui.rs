//! `Ui` — o `Frontend` do terminal: streaming com markdown por linha (`render`),
//! linhas de ferramenta, rodapé de custo, E o gate (mostra o preview em ANSI +
//! pergunta no stdin). Quem mostra é quem pergunta. O `JsonSink` é o par headless.

use crate::diff;
use crate::frontend::Frontend;
use crate::gate::{self, Policy, Preview};
use crate::render;
use crate::telemetry::Telemetry;
use crate::term;
use crate::types::{Event, ToolCall};
use std::collections::HashSet;

#[derive(Default)]
pub struct Ui {
    label_shown: bool,
    line_buf: String,
    in_code: bool,
    /// Escopos liberados por "tudo nesta sessão".
    allowed: HashSet<String>,
    /// `anna -p`: nega escrita/bash por padrão (sem quem confirmar).
    non_interactive: bool,
}

impl Ui {
    pub fn new(non_interactive: bool) -> Self {
        Self { non_interactive, ..Default::default() }
    }

    /// Só para testes: pré-libera escopos (mesmo caminho do "tudo nesta sessão").
    #[cfg(test)]
    pub fn allowing(scopes: &[&str]) -> Self {
        let mut u = Self::default();
        for s in scopes {
            u.allowed.insert((*s).to_string());
        }
        u
    }

    pub fn banner(&self, text: &str) {
        println!("{}", term::accent(text));
    }

    pub fn note(&self, text: &str) {
        println!("{}", term::gray(text));
    }

    fn flush_pending(&mut self) {
        if !self.line_buf.is_empty() {
            let line = std::mem::take(&mut self.line_buf);
            self.print_line(&line);
        }
    }

    fn print_line(&mut self, line: &str) {
        if !self.label_shown {
            println!("{}", term::green("ANNA"));
            self.label_shown = true;
        }
        if line.trim_start().starts_with("```") {
            self.in_code = !self.in_code;
            println!("{}", term::gray("  ┄┄┄"));
            return;
        }
        println!("{}", render::style_line(line, self.in_code));
    }

    fn render_preview(&self, preview: &Preview) {
        match preview {
            Preview::Diff { path, old, new } => {
                print!("{}", diff::unified(path, old, new, term::caps().color))
            }
            Preview::Command { command, why } => {
                let suffix = if why.is_empty() { String::new() } else { format!("   # {why}") };
                println!("  $ {command}{suffix}");
            }
            Preview::Commit { stat, message } => {
                println!("  commit:\n{stat}\n  mensagem: {message}");
            }
        }
    }
}

impl Frontend for Ui {
    fn on_event(&mut self, ev: &Event) {
        match ev {
            Event::TextDelta(d) => {
                for ch in d.chars() {
                    if ch == '\n' {
                        let line = std::mem::take(&mut self.line_buf);
                        self.print_line(&line);
                    } else {
                        self.line_buf.push(ch);
                    }
                }
            }
            Event::ToolCall(c) => {
                self.flush_pending();
                println!("{}", term::gray(&format!("  ⚙ {}({})", c.name, compact_args(c))));
            }
            Event::ReasoningDelta | Event::Model { .. } | Event::Usage(_) | Event::Error(_)
            | Event::Done { .. } | Event::Other => {}
        }
    }

    fn end_stream(&mut self) {
        self.flush_pending();
        self.label_shown = false;
        self.in_code = false;
    }

    fn tool_result(&mut self, call: &ToolCall, out: &str) {
        let first = out.lines().next().unwrap_or("");
        let head = format!("  ✓ {} → {} bytes", call.name, out.len());
        let head = if out.starts_with("ERRO") || out.starts_with("RECUSADO") {
            term::red(&head)
        } else {
            term::gray(&head)
        };
        println!("{head}  {}", term::gray(&truncate(first, 72)));
    }

    fn turn_end(&mut self, tel: &Telemetry) {
        println!("{}", term::gray(&tel.footer()));
    }

    fn error(&mut self, msg: &str) {
        println!("{}", term::red(&format!("erro: {msg}")));
    }

    fn warn(&mut self, msg: &str) {
        println!("{}", term::yellow(msg));
    }

    fn aviso_teto(&mut self, teto: usize) {
        if self.non_interactive {
            // -p não abre o Store de sessões (só o autosave do REPL grava):
            // não existe "continua" possível — o conselho é o teto maior.
            self.warn(&format!(
                "teto de {teto} voltas do turno atingido — o modo -p não persiste sessão: \
                 rode de novo com um teto maior (env SHVIA_MAX_VOLTAS ou `max_voltas` no \
                 config.toml)."
            ));
        } else {
            self.warn(&format!(
                "teto de {teto} voltas do turno atingido — diga \"continua\" para retomar \
                 de onde parou, ou suba `max_voltas` no config.toml (env SHVIA_MAX_VOLTAS)."
            ));
        }
    }

    fn continuar_apos_teto(&mut self, teto: usize) -> bool {
        if self.non_interactive {
            // -p/CI: ninguém para responder — encerra o turno (o warn orienta).
            return false;
        }
        matches!(
            gate::ask(&format!(
                "  teto de {teto} voltas do turno atingido — continuar mais um lote? [s]im / [n]ão > "
            )),
            's' | 'S'
        )
    }

    fn approve(&mut self, policy: Policy, scope: &str, _id: &str, preview: &Preview) -> bool {
        match policy {
            Policy::Auto => true,
            Policy::Confirm if self.allowed.contains(scope) => true,
            _ => {
                self.render_preview(preview);
                if self.non_interactive {
                    return false;
                }
                if policy == Policy::Always {
                    matches!(gate::ask("  AÇÃO DESTRUTIVA — confirmar? [s]im / [n]ão > "), 'S' | 's')
                } else {
                    match gate::ask("  aplicar? [s]im / [n]ão / [t]udo nesta sessão > ") {
                        'S' | 's' => true,
                        'T' | 't' => {
                            self.allowed.insert(scope.to_string());
                            true
                        }
                        _ => false,
                    }
                }
            }
        }
    }
}

fn compact_args(call: &ToolCall) -> String {
    let s = serde_json::to_string(&call.arguments).unwrap_or_default();
    truncate(&s, 80)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests_teto {
    use super::*;

    #[test]
    fn nao_interativo_nunca_ganha_lote_extra() {
        // -p/CI: não há humano para responder — o teto é duro (A6/§2.7).
        assert!(!Ui::new(true).continuar_apos_teto(100));
    }
}
