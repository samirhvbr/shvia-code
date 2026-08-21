//! O "cérebro" do `anna stats` — a tela **Wrapped** do SHVIA-CODE: agrega as
//! sessões locais (`~/.shvia-code/sessions/`) em números (sessões, mensagens,
//! tokens, dias ativos, sequência, horário de pico, modelo favorito), um
//! heatmap de contribuição e a quebra por modelo. Puro e testável: o `compute`
//! deriva os registros das sessões (epoch → data/hora **local**) e delega às
//! funções puras (`current_streak`, `peak_hour`, `build_heatmap`, formatação),
//! que são o que os testes exercem — sem depender do fuso da máquina.
//!
//! Render é dos vizinhos: `stats_term` (terminal ANSI) e `stats_html` (export).

use crate::session::Session;
use chrono::{Datelike, Duration, Local, NaiveDate, TimeZone, Timelike};
use std::collections::HashMap;

/// Tamanho do Moby-Dick em tokens, para a brincadeira do rodapé. Calibrado para
/// bater com a referência (132.8M tokens ⇒ ~495×).
pub const MOBY_DICK_TOKENS: i64 = 268_000;

/// Janela de tempo do dashboard.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Range {
    All,
    D30,
    D7,
}

impl Range {
    pub fn parse(s: &str) -> Range {
        match s.trim().to_lowercase().as_str() {
            "7" | "7d" | "semana" => Range::D7,
            "30" | "30d" | "mes" | "mês" => Range::D30,
            _ => Range::All,
        }
    }
    /// Rótulo curto para os chips (Todos · 30d · 7d).
    pub fn chip(self) -> &'static str {
        match self {
            Range::All => "Todos",
            Range::D30 => "30d",
            Range::D7 => "7d",
        }
    }
    /// Primeiro dia coberto (inclusivo), dado o "hoje".
    fn since(self, today: NaiveDate) -> Option<NaiveDate> {
        match self {
            Range::All => None,
            Range::D30 => Some(today - Duration::days(29)),
            Range::D7 => Some(today - Duration::days(6)),
        }
    }
}

/// Aba do dashboard.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Overview,
    Models,
}

impl Tab {
    pub fn parse(s: &str) -> Tab {
        match s.trim().to_lowercase().as_str() {
            "models" | "modelos" | "modelo" => Tab::Models,
            _ => Tab::Overview,
        }
    }
}

/// Uma linha da aba Modelos.
pub struct ModelRow {
    pub model: String,
    pub sessions: usize,
    pub messages: usize,
    pub tokens: i64,
    pub cost: f64,
}

/// Heatmap estilo GitHub: colunas = semanas, linhas = dias (0=segunda…6=domingo).
/// `None` = dia fora da janela (padding); `Some(n)` = n sessões naquele dia.
pub struct Heatmap {
    pub weeks: Vec<[Option<u32>; 7]>,
    pub max: u32,
}

/// Tudo que os renderers precisam.
pub struct Stats {
    pub range: Range,
    pub sessions: usize,
    pub messages: usize,
    pub tokens: i64,
    pub cost: f64,
    pub active_days: usize,
    pub current_streak: usize,
    pub longest_streak: usize,
    pub peak_hour: Option<u32>,
    pub favorite_model: Option<String>,
    pub heatmap: Heatmap,
    pub models: Vec<ModelRow>,
    /// Alguma sessão já trouxe tokens? (as antigas não → mostramos "—").
    pub tokens_known: bool,
    /// Alguma sessão já trouxe modelo?
    pub model_known: bool,
}

/// Registro derivado de uma sessão (interno).
struct Rec {
    date: NaiveDate,
    hour: u32,
    model: Option<String>,
    tokens: i64,
    cost: f64,
    msgs: usize,
}

/// Conta as mensagens "de conversa" de uma sessão: user + assistant (o `system`
/// e os resultados de ferramenta `tool` não entram).
fn conversa(s: &Session) -> usize {
    s.messages
        .iter()
        .filter(|m| m.role == "user" || m.role == "assistant")
        .count()
}

/// Deriva o registro de uma sessão (epoch do `id` → data/hora local). `None` se
/// o id não for um epoch válido (sessão malformada).
fn rec_of(s: &Session) -> Option<Rec> {
    let ts: i64 = s.id.parse().ok()?;
    let dt = Local.timestamp_opt(ts, 0).single()?;
    let model = s
        .model
        .as_ref()
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty());
    Some(Rec {
        date: dt.date_naive(),
        hour: dt.hour(),
        model,
        tokens: s.tokens.max(0),
        cost: s.cost.max(0.0),
        msgs: conversa(s),
    })
}

/// Entrada pública: agrega usando o "hoje" local.
pub fn compute(sessions: &[Session], range: Range) -> Stats {
    compute_at(sessions, range, Local::now().date_naive())
}

/// Núcleo testável: recebe o "hoje" injetado.
pub fn compute_at(sessions: &[Session], range: Range, today: NaiveDate) -> Stats {
    let since = range.since(today);
    let recs: Vec<Rec> = sessions
        .iter()
        .filter_map(rec_of)
        .filter(|r| {
            since
                .map(|s| r.date >= s && r.date <= today)
                .unwrap_or(true)
        })
        .collect();

    let sessions_n = recs.len();
    let messages = recs.iter().map(|r| r.msgs).sum();
    let tokens = recs.iter().map(|r| r.tokens).sum();
    let cost = recs.iter().map(|r| r.cost).sum();
    let tokens_known = recs.iter().any(|r| r.tokens > 0);
    let model_known = recs.iter().any(|r| r.model.is_some());

    // Dias ativos / sequências.
    let mut days: Vec<NaiveDate> = recs.iter().map(|r| r.date).collect();
    days.sort_unstable();
    days.dedup();
    let active_days = days.len();
    let current_streak = current_streak(&days, today);
    let longest_streak = longest_streak(&days);

    // Horário de pico.
    let peak_hour = peak_hour(recs.iter().map(|r| r.hour));

    // Modelo favorito (moda).
    let favorite_model = mode_model(&recs);

    // Heatmap.
    let mut per_day: HashMap<NaiveDate, u32> = HashMap::new();
    for r in &recs {
        *per_day.entry(r.date).or_insert(0) += 1;
    }
    let first = days.first().copied();
    let heatmap = build_heatmap(&per_day, range, today, first);

    // Quebra por modelo.
    let models = model_rows(&recs);

    Stats {
        range,
        sessions: sessions_n,
        messages,
        tokens,
        cost,
        active_days,
        current_streak,
        longest_streak,
        peak_hour,
        favorite_model,
        heatmap,
        models,
        tokens_known,
        model_known,
    }
}

/// Sequência ATUAL: dias consecutivos terminando no dia ativo mais recente,
/// desde que esse dia seja hoje ou ontem (senão a sequência está quebrada → 0).
/// `days` deve estar ordenado e sem repetição.
pub fn current_streak(days: &[NaiveDate], today: NaiveDate) -> usize {
    let Some(&last) = days.last() else { return 0 };
    if last != today && last != today - Duration::days(1) {
        return 0;
    }
    let mut streak = 1;
    let mut expected = last - Duration::days(1);
    for &d in days.iter().rev().skip(1) {
        if d == expected {
            streak += 1;
            expected -= Duration::days(1);
        } else {
            break;
        }
    }
    streak
}

/// Maior sequência de dias consecutivos no histórico. `days` ordenado e único.
pub fn longest_streak(days: &[NaiveDate]) -> usize {
    let mut best = 0usize;
    let mut run = 0usize;
    let mut prev: Option<NaiveDate> = None;
    for &d in days {
        run = match prev {
            Some(p) if d == p + Duration::days(1) => run + 1,
            _ => 1,
        };
        best = best.max(run);
        prev = Some(d);
    }
    best
}

/// Hora (0–23) com mais sessões abertas. Empate → hora mais cedo. `None` se vazio.
pub fn peak_hour(hours: impl Iterator<Item = u32>) -> Option<u32> {
    let mut hist = [0u32; 24];
    let mut any = false;
    for h in hours {
        if h < 24 {
            hist[h as usize] += 1;
            any = true;
        }
    }
    if !any {
        return None;
    }
    let mut best = 0usize;
    for h in 1..24 {
        if hist[h] > hist[best] {
            best = h;
        }
    }
    Some(best as u32)
}

fn mode_model(recs: &[Rec]) -> Option<String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for r in recs {
        if let Some(m) = &r.model {
            *counts.entry(m.as_str()).or_insert(0) += 1;
        }
    }
    counts
        .into_iter()
        // mais frequente; empate → nome alfabético (determinístico)
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(a.0)))
        .map(|(k, _)| k.to_string())
}

fn model_rows(recs: &[Rec]) -> Vec<ModelRow> {
    let mut map: HashMap<String, (usize, usize, i64, f64)> = HashMap::new();
    for r in recs {
        let key = r.model.clone().unwrap_or_else(|| "(sem modelo)".into());
        let e = map.entry(key).or_insert((0, 0, 0, 0.0));
        e.0 += 1;
        e.1 += r.msgs;
        e.2 += r.tokens;
        e.3 += r.cost;
    }
    let mut rows: Vec<ModelRow> = map
        .into_iter()
        .map(|(model, (sessions, messages, tokens, cost))| ModelRow {
            model,
            sessions,
            messages,
            tokens,
            cost,
        })
        .collect();
    rows.sort_by(|a, b| {
        b.tokens
            .cmp(&a.tokens)
            .then(b.sessions.cmp(&a.sessions))
            .then(a.model.cmp(&b.model))
    });
    rows
}

fn build_heatmap(
    per_day: &HashMap<NaiveDate, u32>,
    range: Range,
    today: NaiveDate,
    first_active: Option<NaiveDate>,
) -> Heatmap {
    // Início bruto da janela (inclusivo).
    let start_raw = match range {
        Range::D7 => today - Duration::days(6),
        Range::D30 => today - Duration::days(29),
        // Tudo: da primeira atividade, com teto de ~53 semanas.
        Range::All => first_active
            .unwrap_or(today)
            .max(today - Duration::days(370)),
    };
    // Alinha o começo à segunda-feira da semana do start_raw.
    let back = start_raw.weekday().num_days_from_monday() as i64;
    let start = start_raw - Duration::days(back);
    let total = (today - start).num_days().max(0) + 1;
    let weeks_n = ((total + 6) / 7) as usize;

    let mut weeks = Vec::with_capacity(weeks_n);
    for w in 0..weeks_n {
        let mut col = [None; 7];
        for (d, cell) in col.iter_mut().enumerate() {
            let date = start + Duration::days((w * 7 + d) as i64);
            if date >= start_raw && date <= today {
                *cell = Some(per_day.get(&date).copied().unwrap_or(0));
            }
        }
        weeks.push(col);
    }
    let max = weeks.iter().flatten().filter_map(|x| *x).max().unwrap_or(0);
    Heatmap { weeks, max }
}

/// Nível de intensidade (0–4) de uma célula do heatmap, compartilhado pelos
/// renderers (terminal e HTML) para as duas telas baterem.
pub fn level(count: u32, max: u32) -> u8 {
    if count == 0 {
        0
    } else if max <= 1 {
        3
    } else {
        let q = count as f64 / max as f64;
        if q > 0.75 {
            4
        } else if q > 0.5 {
            3
        } else if q > 0.25 {
            2
        } else {
            1
        }
    }
}

// ── Formatação (pt-BR-ish, calibrada pelo print) ────────────────────────────

/// Inteiro com separador de milhar "." → 113481 vira "113.481".
pub fn fmt_int(n: i64) -> String {
    let neg = n < 0;
    let digits = n.abs().to_string();
    let len = digits.len();
    let mut out = String::with_capacity(len + len / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(c);
    }
    if neg {
        format!("-{out}")
    } else {
        out
    }
}

/// Compacto para tokens: 132.8M / 12.4k (ponto decimal, como no print).
pub fn fmt_compact(n: i64) -> String {
    let a = n.abs();
    if a < 1_000 {
        return n.to_string();
    }
    let (v, suf) = if a < 1_000_000 {
        (n as f64 / 1_000.0, "k")
    } else if a < 1_000_000_000 {
        (n as f64 / 1_000_000.0, "M")
    } else {
        (n as f64 / 1_000_000_000.0, "B")
    };
    let s = format!("{v:.1}");
    let s = s.strip_suffix(".0").map(str::to_string).unwrap_or(s);
    format!("{s}{suf}")
}

/// A frase do rodapé comparando com o Moby-Dick. `None` se ainda não há tokens.
pub fn moby_phrase(tokens: i64) -> Option<String> {
    if tokens <= 0 {
        return None;
    }
    let r = tokens as f64 / MOBY_DICK_TOKENS as f64;
    if r >= 1.0 {
        let n = if r >= 10.0 {
            format!("{}", r.floor() as i64)
        } else {
            format!("{:.1}", r).replace('.', ",")
        };
        Some(format!("Você usou ~{n}× mais tokens do que o Moby-Dick."))
    } else {
        let pct = (r * 100.0).round().max(1.0) as i64;
        Some(format!("Você usou ~{pct}% dos tokens de um Moby-Dick."))
    }
}

/// "09h" a partir da hora crua.
pub fn fmt_hour(h: u32) -> String {
    format!("{h:02}h")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn streak_atual_conta_ate_hoje() {
        let hoje = d(2026, 7, 16);
        let dias = vec![d(2026, 7, 14), d(2026, 7, 15), d(2026, 7, 16)];
        assert_eq!(current_streak(&dias, hoje), 3);
    }

    #[test]
    fn streak_atual_aceita_ontem_como_ponta() {
        let hoje = d(2026, 7, 16);
        let dias = vec![d(2026, 7, 14), d(2026, 7, 15)]; // termina ontem
        assert_eq!(current_streak(&dias, hoje), 2);
    }

    #[test]
    fn streak_atual_zera_se_ha_um_buraco_ate_hoje() {
        let hoje = d(2026, 7, 16);
        let dias = vec![d(2026, 7, 10), d(2026, 7, 11)]; // parou faz dias
        assert_eq!(current_streak(&dias, hoje), 0);
    }

    #[test]
    fn maior_streak_ignora_buracos() {
        let dias = vec![
            d(2026, 7, 1),
            d(2026, 7, 2),
            d(2026, 7, 3), // run de 3
            d(2026, 7, 10),
            d(2026, 7, 11), // run de 2
        ];
        assert_eq!(longest_streak(&dias), 3);
    }

    #[test]
    fn pico_pega_a_hora_mais_comum() {
        let horas = vec![9u32, 9, 14, 9, 14].into_iter();
        assert_eq!(peak_hour(horas), Some(9));
        assert_eq!(peak_hour(std::iter::empty()), None);
    }

    #[test]
    fn fmt_inteiro_com_milhar() {
        assert_eq!(fmt_int(113_481), "113.481");
        assert_eq!(fmt_int(138), "138");
        assert_eq!(fmt_int(1_000), "1.000");
    }

    #[test]
    fn fmt_compacto_bate_o_print() {
        assert_eq!(fmt_compact(132_800_000), "132.8M");
        assert_eq!(fmt_compact(12_400), "12.4k");
        assert_eq!(fmt_compact(5_000_000), "5M");
        assert_eq!(fmt_compact(999), "999");
    }

    #[test]
    fn moby_bate_a_referencia() {
        // 132.8M tokens ⇒ ~495× (calibração do MOBY_DICK_TOKENS).
        assert_eq!(
            moby_phrase(132_800_000).unwrap(),
            "Você usou ~495× mais tokens do que o Moby-Dick."
        );
        assert!(moby_phrase(0).is_none());
        assert!(moby_phrase(134_000).unwrap().contains('%')); // < 1 Moby
    }

    #[test]
    fn niveis_do_heatmap() {
        assert_eq!(level(0, 10), 0);
        assert_eq!(level(10, 10), 4);
        assert_eq!(level(1, 1), 3); // dado de um nível só cai no meio
    }

    #[test]
    fn compute_conta_sessoes_mensagens_e_dias() {
        // Duas sessões no mesmo epoch-day, uma noutro; conta user+assistant.
        let mk = |id: &str, msgs: &[&str]| Session {
            id: id.into(),
            title: "t".into(),
            messages: msgs
                .iter()
                .map(|r| crate::types::Message {
                    role: (*r).into(),
                    content: String::new(),
                    tool_call_id: None,
                    tool_calls: vec![],
                    images: vec![],
                })
                .collect(),
            tokens: 100,
            cost: 0.0,
            model: Some("opus".into()),
        };
        // ids são epochs; o "hoje" do compute_at não filtra em Range::All.
        let sessions = vec![
            mk("1783598519", &["system", "user", "assistant", "tool"]),
            mk("1783598625", &["user", "assistant"]),
        ];
        let s = compute_at(&sessions, Range::All, d(2026, 7, 16));
        assert_eq!(s.sessions, 2);
        assert_eq!(s.messages, 4); // (user+assistant) + (user+assistant)
        assert_eq!(s.tokens, 200);
        assert!(s.tokens_known);
        assert!(s.model_known);
        assert_eq!(s.favorite_model.as_deref(), Some("opus"));
        assert_eq!(s.models.len(), 1);
    }
}
