//! Política de aprovação + o que mostrar antes de aprovar. A DECISÃO e o RENDER
//! do preview moram no `Frontend` (CLI = ANSI+stdin; JSON = evento+stdin),
//! porque quem mostra é quem pergunta. **Não existe modo yolo.**

use crate::menu::{self, Answer, Item};
use crate::term;
use std::io::{self, Write};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    /// Read-only, sem efeito colateral → roda sem perguntar.
    Auto,
    /// Mostra e pergunta [s]im/[n]ão/[t]udo-nesta-sessão.
    Confirm,
    /// Destrutivo: confirma SEMPRE; `t` não vale.
    Always,
}

impl Policy {
    pub fn as_str(self) -> &'static str {
        match self {
            Policy::Auto => "auto",
            Policy::Confirm => "confirm",
            Policy::Always => "always",
        }
    }
}

/// O que será aprovado, montado pela ferramenta e renderizado pelo frontend.
pub enum Preview<'a> {
    Diff { path: &'a str, old: &'a str, new: &'a str },
    Command { command: &'a str, why: &'a str },
    Commit { stat: &'a str, message: &'a str },
}

/// Prompt de 1 caractere no stdin (usado pelo frontend CLI).
pub fn ask(msg: &str) -> char {
    print!("{msg}");
    let _ = io::stdout().flush();
    let mut line = String::new();
    if io::stdin().read_line(&mut line).is_err() {
        return 'n';
    }
    line.trim().chars().next().unwrap_or('n')
}

/// A yes/no question. On a terminal it is the arrow-key menu with "Não" one key away; elsewhere
/// it is the old line on stdin. Anything but an explicit yes is a no.
pub fn confirm(question: &str) -> bool {
    let items = [Item { label: "Sim", hotkeys: "sy" }, Item { label: "Não", hotkeys: "n" }];
    match menu::choose(question, &items, 0) {
        Answer::Picked(0) => {
            println!("  {} Sim", term::green("✔"));
            true
        }
        Answer::Picked(_) | Answer::Cancelled => {
            println!("  {} Não", term::red("✘"));
            false
        }
        Answer::Unavailable => matches!(ask(&format!("  {question} [s]im / [n]ão > ")), 'S' | 's'),
    }
}
