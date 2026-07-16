//! Render do `anna stats` no terminal (ANSI). Cabeçalho + chips de período +
//! abas; a Visão Geral em 8 tiles (box-drawing) + heatmap de contribuição
//! (blocos azuis em 256-cores) + a frase do Moby-Dick; a aba Modelos como
//! tabela. Só desenha o que o `stats` já agregou — nada de I/O aqui.

use crate::stats::{self, Range, Stats, Tab};

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const CYAN: &str = "\x1b[1;36m";
const GRAY: &str = "\x1b[90m";
const UNDER: &str = "\x1b[4m";

/// Largura interna de cada tile (cabe "Total de tokens" e "Horário de pico").
const W: usize = 15;
/// Máximo de semanas mostradas no heatmap do terminal (evita estourar 80 col).
const MAX_WEEKS: usize = 30;

/// Paleta azul do heatmap (código de cor 256), do vazio ao mais intenso.
fn palette(level: u8) -> u8 {
    match level {
        0 => 236, // dia sem atividade (cinza escuro)
        1 => 24,
        2 => 26,
        3 => 32,
        _ => 39, // mais intenso
    }
}

/// Desenha o dashboard inteiro no stdout.
pub fn render(s: &Stats, tab: Tab, name: &str) {
    header(s, name);
    tabs(tab);
    println!();
    match tab {
        Tab::Overview => overview(s),
        Tab::Models => models(s),
    }
}

fn header(s: &Stats, name: &str) {
    let title = format!("✳  O que vem a seguir, {name}?");
    let chips = [Range::All, Range::D30, Range::D7]
        .iter()
        .map(|r| {
            if *r == s.range {
                format!("{CYAN}{}{RESET}", r.chip())
            } else {
                format!("{GRAY}{}{RESET}", r.chip())
            }
        })
        .collect::<Vec<_>>()
        .join(&format!("{DIM} · {RESET}"));
    // título à esquerda, chips à direita (o pad usa a largura VISÍVEL do título).
    let pad = 62usize.saturating_sub(title.chars().count());
    println!("{BOLD}{title}{RESET}{}{chips}", " ".repeat(pad.max(2)));
}

fn tabs(active: Tab) {
    let one = |label: &str, is: bool| {
        if is {
            format!("{BOLD}{UNDER}{label}{RESET}")
        } else {
            format!("{GRAY}{label}{RESET}")
        }
    };
    println!(
        " {}    {}",
        one("Visão Geral", active == Tab::Overview),
        one("Modelos", active == Tab::Models),
    );
}

// ── Visão Geral ─────────────────────────────────────────────────────────────

fn overview(s: &Stats) {
    let tokens = if s.tokens_known {
        stats::fmt_compact(s.tokens)
    } else {
        "—".into()
    };
    let peak = s
        .peak_hour
        .map(stats::fmt_hour)
        .unwrap_or_else(|| "—".into());
    let model = s.favorite_model.clone().unwrap_or_else(|| "—".into());

    println!("{}", border('┌', '┬', '┐'));
    println!(
        "{}",
        grid_row(
            ["Sessões", "Mensagens", "Total de tokens", "Dias ativos"],
            GRAY
        )
    );
    println!(
        "{}",
        grid_row(
            [
                &stats::fmt_int(s.sessions as i64),
                &stats::fmt_int(s.messages as i64),
                &tokens,
                &stats::fmt_int(s.active_days as i64),
            ],
            CYAN,
        )
    );
    println!("{}", border('├', '┼', '┤'));
    println!(
        "{}",
        grid_row(
            ["Seq. atual", "Maior seq.", "Horário de pico", "Modelo favorito"],
            GRAY
        )
    );
    println!(
        "{}",
        grid_row(
            [
                &format!("{}d", s.current_streak),
                &format!("{}d", s.longest_streak),
                &peak,
                &model,
            ],
            CYAN,
        )
    );
    println!("{}", border('└', '┴', '┘'));

    println!();
    heatmap(s);

    if let Some(phrase) = stats::moby_phrase(s.tokens) {
        println!();
        println!("{DIM}{phrase}{RESET}");
    } else if !s.tokens_known && s.sessions > 0 {
        println!();
        println!("{DIM}(tokens passam a contar a partir das próximas sessões){RESET}");
    }
    if s.cost > 0.0 {
        println!("{DIM}custo total ≈ US$ {:.2}{RESET}", s.cost);
    }
    if s.sessions == 0 {
        println!("{GRAY}(sem sessões ainda — converse com a Anna e volte aqui){RESET}");
    }
}

fn border(l: char, m: char, r: char) -> String {
    let seg = "─".repeat(W + 2);
    format!("{GRAY}{l}{seg}{m}{seg}{m}{seg}{m}{seg}{r}{RESET}")
}

/// Uma linha de 4 células, já coloridas e alinhadas pela largura visível.
fn grid_row(cells: [&str; 4], color: &str) -> String {
    let c = |s: &str| format!("{color}{}{RESET}", pad(s, W));
    let bar = format!("{GRAY}│{RESET}");
    format!(
        "{bar} {} {bar} {} {bar} {} {bar} {} {bar}",
        c(cells[0]),
        c(cells[1]),
        c(cells[2]),
        c(cells[3]),
    )
}

fn heatmap(s: &Stats) {
    let hm = &s.heatmap;
    if hm.weeks.is_empty() {
        return;
    }
    let start = hm.weeks.len().saturating_sub(MAX_WEEKS);
    let shown = &hm.weeks[start..];
    let labels = ["seg", "   ", "qua", "   ", "sex", "   ", "   "];
    for (row, label) in labels.iter().enumerate() {
        print!("{GRAY}{label}{RESET} ");
        for col in shown {
            match col[row] {
                None => print!("  "),
                Some(cnt) => {
                    let lv = stats::level(cnt, hm.max);
                    print!("\x1b[48;5;{}m  {RESET}", palette(lv));
                }
            }
        }
        println!();
    }
    // Legenda.
    let scale: String = (0..=4)
        .map(|l| format!("\x1b[48;5;{}m  {RESET}", palette(l)))
        .collect();
    println!("{DIM}    menos {RESET}{scale}{DIM} mais{RESET}");
}

// ── Aba Modelos ─────────────────────────────────────────────────────────────

fn models(s: &Stats) {
    if s.models.is_empty() {
        println!("{GRAY}(sem dados de modelo neste período){RESET}");
        return;
    }
    println!(
        "{GRAY}{:<22}{:>9}{:>11}{:>10}{:>12}{RESET}",
        "Modelo", "Sessões", "Mensagens", "Tokens", "Custo"
    );
    for r in &s.models {
        let tok = if r.tokens > 0 {
            stats::fmt_compact(r.tokens)
        } else {
            "—".into()
        };
        let custo = if r.cost > 0.0 {
            format!("US$ {:.2}", r.cost)
        } else {
            "—".into()
        };
        println!(
            "{CYAN}{:<22}{RESET}{:>9}{:>11}{:>10}{:>12}",
            trunc(&r.model, 21),
            stats::fmt_int(r.sessions as i64),
            stats::fmt_int(r.messages as i64),
            tok,
            custo,
        );
    }
    if !s.model_known {
        println!();
        println!("{DIM}(o modelo passa a ser registrado a partir das próximas sessões){RESET}");
    }
}

// ── util ────────────────────────────────────────────────────────────────────

fn pad(s: &str, w: usize) -> String {
    let n = s.chars().count();
    if n >= w {
        s.chars().take(w).collect()
    } else {
        let mut t = s.to_string();
        t.extend(std::iter::repeat_n(' ', w - n));
        t
    }
}

fn trunc(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}
