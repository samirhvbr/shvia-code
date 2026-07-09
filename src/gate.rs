//! Política de aprovação (docs/ferramentas.md §1). No C1 todas as tools são
//! read-only (`Auto`) — o gate não pergunta nada. `Confirm`/`Always` já existem
//! aqui pro C2 (edit/write/bash), com memória de "tudo nesta sessão" por escopo.
//! **Não existe modo yolo.**

use std::collections::HashSet;
use std::io::{self, Write};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
// Confirm/Always são usados no C2 (edit/write/bash); o C1 só usa Auto.
#[allow(dead_code)]
pub enum Policy {
    /// Read-only, sem efeito colateral → roda sem perguntar.
    Auto,
    /// Mostra e pergunta [s]im/[n]ão/[t]udo-nesta-sessão.
    Confirm,
    /// Destrutivo: confirma SEMPRE; `t` não vale.
    Always,
}

#[derive(Default)]
pub struct Gate {
    /// Escopos liberados por "tudo nesta sessão".
    allowed: HashSet<String>,
    /// Modo não-interativo (`anna -p`): nega escrita/bash por padrão.
    non_interactive: bool,
}

impl Gate {
    pub fn new(non_interactive: bool) -> Self {
        Self { allowed: HashSet::new(), non_interactive }
    }

    /// Decide se a ação pode rodar. `scope` = tool + alvo (p/ o "tudo": ex.
    /// "edit_file"). `preview` = o que será feito (diff/comando) já renderizado
    /// pelo chamador; aqui só perguntamos.
    pub fn approve(&mut self, policy: Policy, scope: &str) -> bool {
        match policy {
            Policy::Auto => true,
            Policy::Confirm if self.allowed.contains(scope) => true,
            Policy::Confirm => {
                if self.non_interactive {
                    return false; // -p sem --allow-writes nega escrita
                }
                match prompt("  aplicar? [s]im / [n]ão / [t]udo nesta sessão > ") {
                    'S' | 's' => true,
                    'T' | 't' => {
                        self.allowed.insert(scope.to_string());
                        true
                    }
                    _ => false,
                }
            }
            Policy::Always => {
                if self.non_interactive {
                    return false;
                }
                matches!(prompt("  AÇÃO DESTRUTIVA — confirmar? [s]im / [n]ão > "), 'S' | 's')
            }
        }
    }
}

fn prompt(msg: &str) -> char {
    print!("{msg}");
    let _ = io::stdout().flush();
    let mut line = String::new();
    if io::stdin().read_line(&mut line).is_err() {
        return 'n';
    }
    line.trim().chars().next().unwrap_or('n')
}
