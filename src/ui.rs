//! Render no terminal: streaming da resposta (com markdown leve por linha),
//! linhas de ferramenta, rodapé de custo. O input mora no `main` (rustyline);
//! aqui é só a saída. Cores por ANSI simples. Markdown: `render`. TUI = evolução.

use crate::render;
use crate::types::{Event, ToolCall};

const CYAN: &str = "\x1b[1;36m";
const GREEN: &str = "\x1b[92m";
const GRAY: &str = "\x1b[90m";
const RED: &str = "\x1b[91m";
const YELLOW: &str = "\x1b[93m";
const RESET: &str = "\x1b[0m";

#[derive(Default)]
pub struct Ui {
    label_shown: bool,
    line_buf: String,
    in_code: bool,
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

    /// Renderiza um evento ao vivo (chamado pelo api::code_chat). O texto é
    /// acumulado e liberado **por linha** — assim o markdown é estilizado quando
    /// a linha fica completa, sem quebrar o streaming.
    pub fn on_event(&mut self, ev: &Event) {
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
                println!("{GRAY}  ⚙ {}({}){RESET}", c.name, compact_args(c));
            }
            // Raciocínio, model, usage, etc. não renderizam aqui.
            Event::ReasoningDelta | Event::Model { .. } | Event::Usage(_) | Event::Error(_)
            | Event::Done { .. } | Event::Other => {}
        }
    }

    fn flush_pending(&mut self) {
        if !self.line_buf.is_empty() {
            let line = std::mem::take(&mut self.line_buf);
            self.print_line(&line);
        }
    }

    fn print_line(&mut self, line: &str) {
        if !self.label_shown {
            println!("{GREEN}ANNA{RESET}");
            self.label_shown = true;
        }
        if line.trim_start().starts_with("```") {
            self.in_code = !self.in_code;
            println!("{GRAY}  ┄┄┄{RESET}");
            return;
        }
        println!("{}", render::style_line(line, self.in_code));
    }

    /// Fecha o bloco de texto da volta (libera a última linha pendente).
    pub fn end_stream(&mut self) {
        self.flush_pending();
        self.label_shown = false;
        self.in_code = false;
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
