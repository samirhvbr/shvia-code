//! `Ui` — o `Frontend` do terminal: streaming com markdown por linha (`render`),
//! títulos e resumos de ferramenta (`view`), spinner (`term`), rodapé de custo, E o gate
//! (mostra o preview em ANSI + pergunta no stdin). Quem mostra é quem pergunta. O `JsonSink`
//! é o par headless.
//!
//! O spinner é dono da linha enquanto roda: todo método que imprime o para antes.

use crate::diff;
use crate::frontend::Frontend;
use crate::gate::{self, Policy, Preview};
use crate::menu::{self, Answer, Item};
use crate::render::Md;
use crate::telemetry::Telemetry;
use crate::term::{self, Spinner};
use crate::types::{Event, ToolCall};
use crate::view::{self, Tone};
use std::collections::HashSet;
use std::time::Instant;

#[derive(Default)]
pub struct Ui {
    /// O `●` do assistente já saiu para o bloco de texto em andamento.
    bullet_shown: bool,
    line_buf: String,
    /// O markdown da resposta em andamento (cerca aberta, tabela pendente).
    md: Md,
    /// Escopos liberados por "tudo nesta sessão".
    allowed: HashSet<String>,
    /// `anna -p`: nega escrita/bash por padrão (sem quem confirmar).
    non_interactive: bool,
    spinner: Option<Spinner>,
    /// Quando o turno começou (a primeira espera); o rodapé mostra quanto durou.
    turn_started: Option<Instant>,
    /// Uma linha em branco antes do próximo texto do assistente (depois do prompt e depois
    /// do resultado de uma ferramenta).
    gap_before_text: bool,
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

    /// A caixa de abertura do REPL.
    pub fn welcome(&self, version: &str, model: &str, server: &str, cwd: &str, branch: Option<&str>) {
        for line in view::welcome(version, model, server, cwd, branch, term::width()) {
            println!("{line}");
        }
    }

    pub fn note(&self, text: &str) {
        println!("{}", term::gray(text));
    }

    fn stop_spinner(&mut self) {
        if let Some(mut sp) = self.spinner.take() {
            sp.stop();
        }
    }

    fn start_spinner(&mut self, label: &str) {
        self.stop_spinner();
        self.spinner = Spinner::start(label);
    }

    /// Linha em branco entre blocos — só num terminal; num pipe a saída fica compacta.
    fn gap(&self) {
        if term::caps().tty_out {
            println!();
        }
    }

    fn flush_pending(&mut self) {
        if !self.line_buf.is_empty() {
            let line = std::mem::take(&mut self.line_buf);
            self.print_line(&line);
        }
    }

    fn print_line(&mut self, line: &str) {
        for styled in self.md.feed(line) {
            self.print_styled(&styled);
        }
    }

    /// O que o markdown ainda segurava (uma tabela que terminou com a resposta).
    fn flush_md(&mut self) {
        for styled in self.md.finish() {
            self.print_styled(&styled);
        }
    }

    /// Uma linha já estilizada, com a margem do assistente: o `●` na primeira, dois espaços nas outras.
    fn print_styled(&mut self, styled: &str) {
        let lead = if self.bullet_shown {
            "  ".to_string()
        } else {
            if styled.trim().is_empty() {
                return; // um bloco não abre com linha em branco
            }
            if std::mem::take(&mut self.gap_before_text) {
                self.gap();
            }
            self.bullet_shown = true;
            format!("{} ", term::accent("●"))
        };
        if styled.trim().is_empty() {
            println!();
        } else {
            println!("{lead}{styled}");
        }
    }

    fn render_preview(&self, preview: &Preview) {
        match preview {
            Preview::Diff { path, old, new } => {
                print!("{}", diff::pretty(path, old, new, term::width(), term::caps()))
            }
            Preview::Command { command, why } => {
                for (i, l) in command.lines().enumerate() {
                    let lead = if i == 0 { term::gray("$") } else { " ".to_string() };
                    println!("  {lead} {l}");
                }
                if !why.is_empty() {
                    println!("  {}", term::gray(&format!("# {why}")));
                }
            }
            Preview::Commit { stat, message } => {
                for l in stat.lines() {
                    println!("  {}", term::gray(l));
                }
                println!("  {} {message}", term::bold("mensagem:"));
            }
        }
    }

    /// A decisão do gate (o que `approve` fazia antes do spinner entrar).
    fn decide(&mut self, policy: Policy, scope: &str, preview: &Preview) -> bool {
        match policy {
            Policy::Auto => true,
            Policy::Confirm if self.allowed.contains(scope) => {
                // "Tudo nesta sessão" dispensa a pergunta, não a vista: o que vai ser gravado
                // continua na tela.
                self.render_preview(preview);
                true
            }
            _ => {
                self.render_preview(preview);
                if self.non_interactive {
                    return false;
                }
                self.ask(policy, scope)
            }
        }
    }

    /// Pergunta ao dev. No terminal é o menu de setas; sem terminal, a linha no stdin de sempre.
    /// Esc, Ctrl-C e erro do terminal são "não".
    fn ask(&mut self, policy: Policy, scope: &str) -> bool {
        let destructive = policy == Policy::Always;
        let question = question_for(scope, destructive);
        // Destrutivo abre em "Não": o Enter distraído recusa. "Tudo nesta sessão" não existe aqui.
        let (items, default): (Vec<Item>, usize) = if destructive {
            (vec![Item { label: "Sim, executar", hotkeys: "sy" }, Item { label: "Não", hotkeys: "n" }], 1)
        } else {
            (
                vec![
                    Item { label: "Sim", hotkeys: "sy" },
                    Item { label: "Sim, e não perguntar de novo nesta sessão", hotkeys: "t" },
                    Item { label: "Não", hotkeys: "n" },
                ],
                0,
            )
        };
        match menu::choose(&question, &items, default) {
            Answer::Picked(i) => {
                let yes = if destructive { i == 0 } else { i < 2 };
                if yes && !destructive && i == 1 {
                    self.allowed.insert(scope.to_string());
                }
                let label = items[i].label;
                if yes {
                    println!("  {} {label}", term::green("✔"));
                } else {
                    println!("  {} {label}", term::red("✘"));
                }
                yes
            }
            Answer::Cancelled => {
                println!("  {} Não", term::red("✘"));
                false
            }
            Answer::Unavailable => {
                if destructive {
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

/// O que se pergunta, por ferramenta.
fn question_for(scope: &str, destructive: bool) -> String {
    match (scope, destructive) {
        ("bash", true) => "Ação destrutiva — executar este comando?".into(),
        ("bash", false) => "Executar este comando?".into(),
        ("edit_file", _) => "Aplicar esta edição?".into(),
        ("write_file", _) => "Gravar este arquivo?".into(),
        ("git_commit", _) => "Criar este commit?".into(),
        (_, true) => "Ação destrutiva — confirmar?".into(),
        _ => "Permitir?".into(),
    }
}

impl Frontend for Ui {
    fn waiting(&mut self) {
        if self.turn_started.is_none() {
            self.turn_started = Some(Instant::now());
            self.gap_before_text = true;
        }
        self.start_spinner("Pensando…");
    }

    fn on_event(&mut self, ev: &Event) {
        match ev {
            Event::TextDelta(d) => {
                if !d.is_empty() {
                    self.stop_spinner();
                }
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
                self.stop_spinner();
                self.flush_pending();
                self.flush_md();
                if self.bullet_shown {
                    self.gap();
                }
                let (verb, arg) = view::call_title(c);
                let room = term::width().saturating_sub(verb.chars().count() + 6);
                let arg = term::truncate_cols(&arg, room);
                println!("{} {}({arg})", term::green("●"), term::bold(&verb));
            }
            Event::ReasoningDelta | Event::Model { .. } | Event::Usage(_) | Event::Error { .. }
            | Event::Done { .. } | Event::Other => {}
        }
    }

    fn end_stream(&mut self) {
        self.stop_spinner();
        self.flush_pending();
        self.flush_md();
        self.md.reset();
        self.bullet_shown = false;
    }

    fn tool_result(&mut self, call: &ToolCall, out: &str) {
        self.stop_spinner();
        for (i, l) in view::result_lines(&call.name, out, term::width()).iter().enumerate() {
            let lead = if i == 0 { "  ⎿  " } else { "     " };
            let text = match l.tone {
                Tone::Dim => term::gray(&l.text),
                Tone::Error => term::red(&l.text),
            };
            println!("{}{text}", term::gray(lead));
        }
        self.gap_before_text = true;
    }

    fn turn_end(&mut self, tel: &Telemetry) {
        self.stop_spinner();
        let elapsed = self.turn_started.take().map(|t| t.elapsed().as_secs());
        self.gap();
        println!("{}", term::gray(&view::footer(elapsed, tel.tokens, tel.cost, tel.estimated)));
    }

    fn error(&mut self, msg: &str) {
        self.stop_spinner();
        self.turn_started = None;
        println!("{}", term::red(&format!("erro: {msg}")));
    }

    fn warn(&mut self, msg: &str) {
        self.stop_spinner();
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
        gate::confirm(&format!("Teto de {teto} voltas do turno atingido — continuar mais um lote?"))
    }

    fn approve(&mut self, policy: Policy, scope: &str, _id: &str, preview: &Preview) -> bool {
        self.stop_spinner();
        let go = self.decide(policy, scope, preview);
        if go && scope == "bash" {
            // O comando pode demorar: quem esperava o modelo agora espera o shell.
            self.start_spinner("Executando…");
        }
        go
    }
}

#[cfg(test)]
mod tests_perguntas {
    use super::*;

    #[test]
    fn cada_ferramenta_faz_a_sua_pergunta() {
        assert_eq!(question_for("edit_file", false), "Aplicar esta edição?");
        assert_eq!(question_for("write_file", false), "Gravar este arquivo?");
        assert_eq!(question_for("git_commit", false), "Criar este commit?");
        assert_eq!(question_for("bash", false), "Executar este comando?");
        assert!(question_for("bash", true).starts_with("Ação destrutiva"));
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
