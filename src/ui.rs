//! Render no terminal: streaming da resposta, linhas de ferramenta, rodapé de
//! custo. C1 = REPL linha-a-linha (o input mora no main via rustyline); aqui é
//! só a saída. TUI (ratatui) é C3. Cores por ANSI simples (sem dep extra no C1).

use crate::types::{Event, ToolCall};
use std::io::{self, Write};

const CYAN: &str = "\x1b[1;36m";
const GREEN: &str = "\x1b[92m";
const GRAY: &str = "\x1b[90m";
const RED: &str = "\x1b[91m";
const YELLOW: &str = "\x1b[93m";
const RESET: &str = "\x1b[0m";

#[derive(Default)]
pub struct Ui {
    streamed: bool,
    label_shown: bool,
}

impl Ui {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn banner(&self, text: &str) {
        println!("{CYAN}{text}{RESET}");
    }

    pub fn note(&self, text: &str) {
        println!("{GRAY}{text}{RESET}");
    }

    /// Renderiza um evento ao vivo (chamado pelo api::code_chat).
    pub fn on_event(&mut self, ev: &Event) {
        match ev {
            Event::TextDelta(d) => {
                if !self.label_shown {
                    print!("{GREEN}ANNA{RESET}  ");
                    self.label_shown = true;
                }
                print!("{d}");
                self.streamed = true;
                let _ = io::stdout().flush();
            }
            Event::ToolCall(c) => {
                println!("\n{GRAY}  ⚙ {}({}){RESET}", c.name, compact_args(c));
            }
            // Raciocínio oculto no C1.
            Event::ReasoningDelta | Event::Model { .. } | Event::Usage(_) | Event::Error(_)
            | Event::Done { .. } | Event::Other => {}
        }
    }

    /// Fecha o bloco de texto da volta (quebra de linha se streamou algo).
    pub fn end_stream(&mut self) {
        if self.streamed {
            println!();
        }
        self.streamed = false;
        self.label_shown = false;
    }

    /// Resultado de uma ferramenta executada localmente.
    pub fn tool_result(&self, call: &ToolCall, out: &str) {
        let first = out.lines().next().unwrap_or("");
        let mark = if out.starts_with("ERRO") || out.starts_with("RECUSADO") { RED } else { GRAY };
        println!(
            "{mark}  ✓ {} → {} bytes{RESET}  {GRAY}{}{RESET}",
            call.name,
            out.len(),
            truncate(first, 72)
        );
    }

    pub fn footer(&self, text: &str) {
        println!("{GRAY}{text}{RESET}");
    }

    pub fn error(&self, text: &str) {
        println!("{RED}erro: {text}{RESET}");
    }

    pub fn warn(&self, text: &str) {
        println!("{YELLOW}{text}{RESET}");
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
