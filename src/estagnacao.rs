//! Stall guard for one turn (0.15.3): the model repeating itself without getting anywhere.
//!
//! The signal is a tool call with the same name, the same arguments AND the same result, again
//! and again — or the same short cycle of such calls (A, B, A, B…). A different result means
//! something changed (a file was edited, a test now fails differently), so it is progress and
//! resets the count. The round ceiling (`max_voltas`) catches a loop too, but late and blind: on
//! 31/10/2026 the owner's MiniMax session reached the 100-round ceiling after reading the same
//! three worktrees' `git diff` twice each, with 4.47M tokens spent, and nothing told the model.
//!
//! What happens (`agent::run_turn`): at `AVISO` laps the model is told, in a note appended to that
//! tool's result (the newest message, so the cached prefix is untouched, and the model reads it
//! next to the output it keeps asking for); at `PARADA` laps the turn stops — the terminal asks
//! the person, a host ends the turn with the closing summary, as at the ceiling.
//!
//! Hermes does the same (`agent/tool_guardrails.py`: notice at 3 identical calls, cycles up to
//! period 4, a hard stop by default only where nobody is watching; comparison of 02/11/2026). The
//! gateway has a cruder guard on its side (`ToolLoopGuard`), but it only sees what is left of the
//! history after the client's compaction; the client sees every call.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Laps of a repeat at which the model is told.
pub const AVISO: usize = 3;
/// Laps at which the turn stops.
pub const PARADA: usize = 5;
/// The longest cycle recognized: up to four calls per lap.
const PERIODO_MAX: usize = 4;
/// Calls remembered: `PARADA` laps of the longest cycle, with room.
const HISTORICO: usize = 64;

#[derive(Clone, PartialEq, Eq)]
struct Chamada {
    nome: String,
    marca: u64,
}

/// What a call means for the turn.
#[derive(Debug, PartialEq)]
pub enum Veredito {
    Segue,
    /// Append this note to the call's result: the model is repeating itself.
    Avisar(String),
    /// Stop the turn; the text says what was repeated.
    Parar(String),
}

#[derive(Default)]
pub struct Estagnacao {
    chamadas: Vec<Chamada>,
    avisado: bool,
}

impl Estagnacao {
    /// Records one executed call with its result, and says what it means.
    pub fn registrar(&mut self, nome: &str, args: &serde_json::Value, saida: &str) -> Veredito {
        let mut h = DefaultHasher::new();
        nome.hash(&mut h);
        args.to_string().hash(&mut h);
        saida.hash(&mut h);
        self.chamadas.push(Chamada { nome: nome.to_string(), marca: h.finish() });
        if self.chamadas.len() > HISTORICO {
            self.chamadas.remove(0);
        }
        let Some((periodo, voltas)) = self.repeticao() else {
            self.avisado = false;
            return Veredito::Segue;
        };
        let o_que = if periodo == 1 {
            format!("{nome} com os mesmos argumentos e o mesmo resultado, {voltas} vezes seguidas")
        } else {
            let nomes: Vec<&str> =
                self.chamadas[self.chamadas.len() - periodo..].iter().map(|c| c.nome.as_str()).collect();
            format!(
                "o mesmo ciclo de {periodo} chamadas ({}) com os mesmos resultados, {voltas} vezes seguidas",
                nomes.join(" → ")
            )
        };
        if voltas >= PARADA {
            return Veredito::Parar(o_que);
        }
        if voltas >= AVISO && !self.avisado {
            self.avisado = true;
            return Veredito::Avisar(format!(
                "\n\n[aviso do anna: {o_que}. O resultado já está acima: use-o ou mude de abordagem. \
                 Na {PARADA}ª repetição o turno para.]"
            ));
        }
        Veredito::Segue
    }

    /// Forgets the repeat (the person let the turn go on after a stop).
    pub fn zerar(&mut self) {
        self.chamadas.clear();
        self.avisado = false;
    }

    /// The repeat that ends the history: `(period, laps)` for the period with the most whole laps
    /// (two at least), or `None`. A cycle whose calls are all the same is the period-1 case.
    fn repeticao(&self) -> Option<(usize, usize)> {
        let c = &self.chamadas;
        let mut melhor: Option<(usize, usize)> = None;
        for p in 1..=PERIODO_MAX {
            if c.len() < 2 * p {
                break;
            }
            let bloco = &c[c.len() - p..];
            if p > 1 && bloco.iter().all(|x| *x == bloco[0]) {
                continue;
            }
            let mut voltas = 1;
            while c.len() >= (voltas + 1) * p && c[c.len() - (voltas + 1) * p..c.len() - voltas * p] == *bloco {
                voltas += 1;
            }
            if voltas >= 2 && melhor.is_none_or(|(_, v)| voltas > v) {
                melhor = Some((p, voltas));
            }
        }
        melhor
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn repeticao_sem_progresso_avisa_no_terceiro_e_para_no_quinto() {
        let ler = |e: &mut Estagnacao, path: &str, conteudo: &str| e.registrar("read_file", &json!({ "path": path }), conteudo);

        // The same read, the same content: told at the 3rd, once; stopped at the 5th.
        let mut e = Estagnacao::default();
        assert_eq!(ler(&mut e, "a.rs", "x"), Veredito::Segue);
        assert_eq!(ler(&mut e, "a.rs", "x"), Veredito::Segue);
        assert!(matches!(ler(&mut e, "a.rs", "x"), Veredito::Avisar(n) if n.contains("3 vezes seguidas")));
        assert_eq!(ler(&mut e, "a.rs", "x"), Veredito::Segue, "told once per repeat");
        assert!(matches!(ler(&mut e, "a.rs", "x"), Veredito::Parar(o) if o.starts_with("read_file")));

        // A different result is progress (the file changed): the count starts over.
        let mut e = Estagnacao::default();
        for _ in 0..4 {
            ler(&mut e, "a.rs", "x");
        }
        assert_eq!(ler(&mut e, "a.rs", "y"), Veredito::Segue);
        assert_eq!(ler(&mut e, "a.rs", "y"), Veredito::Segue);

        // A, B, A, B…: a cycle of two calls, told at the 3rd lap and stopped at the 5th.
        let mut e = Estagnacao::default();
        let mut vereditos = Vec::new();
        for _ in 0..5 {
            vereditos.push(ler(&mut e, "a.rs", "x"));
            vereditos.push(e.registrar("git_diff", &json!({}), "d"));
        }
        assert!(matches!(&vereditos[5], Veredito::Avisar(n) if n.contains("ciclo de 2 chamadas")), "{vereditos:?}");
        assert!(matches!(&vereditos[9], Veredito::Parar(_)), "{vereditos:?}");
        assert!(vereditos[..5].iter().all(|v| *v == Veredito::Segue));
    }
}
