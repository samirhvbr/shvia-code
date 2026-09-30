//! Markdown for the terminal, one line at a time — so it works while the answer is still
//! streaming: each line is styled when it completes.
//!
//! A line is not always enough, so [`Md`] keeps a little state:
//! - inside a ``` fence it remembers the language, to colour keywords, strings and comments;
//! - a table needs every row to align its columns, so its rows wait until the table ends.
//!
//! Styles nest (`**bold with `code`**`) because each one is switched off by its own code, not by
//! a full reset. Without colour the same lines come out with the markers dropped and nothing else
//! added. [`Md::feed`] returns finished lines; the caller (`ui.rs`) puts the assistant's margin
//! in front of them.
//!
//! Known limits, on purpose: no multi-line constructs in code (a `/* ... */` or a triple-quoted
//! string colours only its first line), no nested emphasis inside `**`, no HTML.

use crate::term::{self, paint_with};
use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Align {
    Left,
    Center,
    Right,
}

pub struct Md {
    color: bool,
    /// `None` reads the terminal each time it matters (a table); tests fix it.
    width: Option<usize>,
    in_code: bool,
    lang: String,
    table: Vec<String>,
}

impl Default for Md {
    fn default() -> Self {
        Self::new()
    }
}

impl Md {
    pub fn new() -> Self {
        Self { color: term::caps().color, width: None, in_code: false, lang: String::new(), table: Vec::new() }
    }

    #[cfg(test)]
    fn with(color: bool, width: usize) -> Self {
        Self { color, width: Some(width), in_code: false, lang: String::new(), table: Vec::new() }
    }

    fn width(&self) -> usize {
        self.width.unwrap_or_else(term::width)
    }

    /// Forgets the state at the end of an answer, so an unclosed fence cannot leak into the next.
    pub fn reset(&mut self) {
        self.in_code = false;
        self.lang.clear();
        self.table.clear();
    }

    /// Lines still held back (a table that ended with the answer).
    pub fn finish(&mut self) -> Vec<String> {
        self.flush_table()
    }

    /// One completed source line in; zero or more styled lines out.
    pub fn feed(&mut self, line: &str) -> Vec<String> {
        let t = line.trim_start();

        if t.starts_with("```") {
            let mut out = self.flush_table();
            if self.in_code {
                self.in_code = false;
                self.lang.clear();
                out.push(self.dim("┄┄┄"));
            } else {
                self.in_code = true;
                self.lang = t.trim_start_matches('`').split_whitespace().next().unwrap_or("").to_lowercase();
                let label = if self.lang.is_empty() { "┄┄┄".to_string() } else { format!("┄┄┄ {}", self.lang) };
                out.push(self.dim(&label));
            }
            return out;
        }
        if self.in_code {
            return vec![format!("  {}", highlight(&self.lang, line, self.color))];
        }

        if t.starts_with('|') {
            self.table.push(line.to_string());
            return Vec::new();
        }
        let mut out = self.flush_table();
        out.push(self.block(line));
        out
    }

    // ── Blocks ──────────────────────────────────────────────────────────────

    fn block(&self, line: &str) -> String {
        if let Some(c) = heading_re().captures(line) {
            let level = c[1].len();
            let text = self.inline(c[2].trim());
            return match level {
                1 => self.wrap("1;4", "22;24", &text),
                2 => self.wrap("1;36", "22;39", &text),
                _ => self.wrap("1", "22", &text),
            };
        }
        if is_rule(line) {
            return self.dim(&"─".repeat(self.width().saturating_sub(4).clamp(3, 60)));
        }
        if let Some(c) = quote_re().captures(line) {
            return format!("{}{}", self.dim("▎ "), self.wrap("3", "23", &self.inline(&c[1])));
        }
        if let Some(c) = bullet_re().captures(line) {
            let depth = c[1].chars().count() / 2;
            let (mark, body) = checkbox(&c[2]);
            let mark = match mark {
                Some(true) => self.wrap("32", "39", "☑"),
                Some(false) => "☐".to_string(),
                None => ["•", "◦", "▪"][depth.min(2)].to_string(),
            };
            return format!("{}{mark} {}", " ".repeat(depth * 2), self.inline(body));
        }
        if let Some(c) = ordered_re().captures(line) {
            let depth = c[1].chars().count() / 2;
            return format!("{}{} {}", " ".repeat(depth * 2), self.dim(&format!("{}.", &c[2])), self.inline(&c[3]));
        }
        self.inline(line)
    }

    // ── Inline ──────────────────────────────────────────────────────────────

    fn inline(&self, s: &str) -> String {
        let mut out = String::new();
        let mut last = 0;
        for cap in inline_re().captures_iter(s) {
            let m = cap.get(0).expect("group 0 always exists");
            out.push_str(&s[last..m.start()]);
            last = m.end();
            if let Some(c) = cap.name("code") {
                out.push_str(&if self.color { self.wrap("36", "39", c.as_str()) } else { format!("`{}`", c.as_str()) });
            } else if let Some(b) = cap.name("bold") {
                out.push_str(&self.wrap("1", "22", &self.inline(b.as_str())));
            } else if let Some(x) = cap.name("strike") {
                out.push_str(&self.wrap("9", "29", &self.inline(x.as_str())));
            } else if let Some(i) = cap.name("ital") {
                out.push_str(&self.wrap("3", "23", &self.inline(i.as_str())));
            } else if let (Some(txt), Some(url)) = (cap.name("ltxt"), cap.name("lurl")) {
                out.push_str(&self.wrap("4", "24", &self.inline(txt.as_str())));
                out.push_str(&self.dim(&format!(" ({})", url.as_str())));
            }
        }
        out.push_str(&s[last..]);
        out
    }

    // ── Tables ──────────────────────────────────────────────────────────────

    fn flush_table(&mut self) -> Vec<String> {
        if self.table.is_empty() {
            return Vec::new();
        }
        let rows = std::mem::take(&mut self.table);
        self.render_table(&rows).unwrap_or_else(|| rows.iter().map(|r| self.inline(r)).collect())
    }

    /// `None` when the rows are not a table (no header separator), so they print as plain lines.
    fn render_table(&self, rows: &[String]) -> Option<Vec<String>> {
        let cells: Vec<Vec<String>> = rows.iter().map(|r| split_cells(r)).collect();
        let aligns = parse_separator(cells.get(1)?)?;
        let n = aligns.len();
        let header = normalize(&cells[0], n);
        let body: Vec<Vec<String>> = cells[2..].iter().map(|r| normalize(r, n)).collect();

        let styled = |row: &[String], bold: bool| -> Vec<String> {
            row.iter()
                .map(|c| {
                    let s = self.inline(c);
                    if bold { self.wrap("1", "22", &s) } else { s }
                })
                .collect()
        };
        let head = styled(&header, true);
        let body: Vec<Vec<String>> = body.iter().map(|r| styled(r, false)).collect();

        let mut widths = vec![1usize; n];
        for row in std::iter::once(&head).chain(body.iter()) {
            for (i, c) in row.iter().enumerate() {
                widths[i] = widths[i].max(term::visible_width(c));
            }
        }
        let total: usize = widths.iter().sum::<usize>() + 3 * n + 1;
        // Two columns go to the assistant's margin, one to the terminal's edge.
        if total + 3 > self.width() {
            let mut lines = vec![head.join(&self.dim(" │ "))];
            lines.extend(body.iter().map(|r| r.join(&self.dim(" │ "))));
            return Some(lines);
        }

        let rule = |l: &str, m: &str, r: &str| {
            let parts: Vec<String> = widths.iter().map(|w| "─".repeat(w + 2)).collect();
            self.dim(&format!("{l}{}{r}", parts.join(m)))
        };
        let bar = self.dim("│");
        let line = |row: &[String]| {
            let cells: Vec<String> = row
                .iter()
                .enumerate()
                .map(|(i, c)| pad(c, widths[i], aligns[i]))
                .collect();
            format!("{bar} {} {bar}", cells.join(&format!(" {bar} ")))
        };
        let mut out = vec![rule("┌", "┬", "┐"), line(&head), rule("├", "┼", "┤")];
        out.extend(body.iter().map(|r| line(r)));
        out.push(rule("└", "┴", "┘"));
        Some(out)
    }

    // ── Painting ────────────────────────────────────────────────────────────

    fn dim(&self, t: &str) -> String {
        paint_with(self.color, "90", t)
    }

    /// `text` between an SGR "on" and its own "off", so styles nest.
    fn wrap(&self, on: &str, off: &str, text: &str) -> String {
        if self.color && !text.is_empty() {
            format!("\x1b[{on}m{text}\x1b[{off}m")
        } else {
            text.to_string()
        }
    }
}

// ── Patterns ────────────────────────────────────────────────────────────────

fn heading_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\s{0,3}(#{1,6})\s+(.*?)\s*#*\s*$").expect("valid pattern"))
}
fn quote_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\s{0,3}>\s?(.*)$").expect("valid pattern"))
}
fn bullet_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(\s*)[-*+]\s+(.*)$").expect("valid pattern"))
}
fn ordered_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(\s*)(\d{1,3})[.)]\s+(.*)$").expect("valid pattern"))
}
fn inline_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(
            r"`(?P<code>[^`]+)`|\*\*(?P<bold>[^*]+)\*\*|~~(?P<strike>[^~]+)~~|\*(?P<ital>[^*\s][^*]*)\*|\[(?P<ltxt>[^\]]+)\]\((?P<lurl>[^)\s]+)\)",
        )
        .expect("valid pattern")
    })
}

/// `---`, `***`, `___`: three or more of one of them, spaces allowed between.
fn is_rule(line: &str) -> bool {
    let chars: Vec<char> = line.chars().filter(|c| !c.is_whitespace()).collect();
    chars.len() >= 3 && matches!(chars[0], '-' | '*' | '_') && chars.iter().all(|c| *c == chars[0])
}

/// `[x] text` / `[ ] text` → (Some(checked), text).
fn checkbox(body: &str) -> (Option<bool>, &str) {
    if let Some(rest) = body.strip_prefix("[ ] ") {
        (Some(false), rest)
    } else if let Some(rest) = body.strip_prefix("[x] ").or_else(|| body.strip_prefix("[X] ")) {
        (Some(true), rest)
    } else {
        (None, body)
    }
}

// ── Table helpers ───────────────────────────────────────────────────────────

/// Cells of one row, without the outer pipes; `\|` stays inside its cell.
fn split_cells(row: &str) -> Vec<String> {
    let t = row.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').filter(|_| !t.ends_with("\\|")).unwrap_or(t);
    let mut cells = vec![String::new()];
    let mut prev = ' ';
    for ch in t.chars() {
        if ch == '|' && prev != '\\' {
            cells.push(String::new());
        } else {
            cells.last_mut().expect("never empty").push(ch);
        }
        prev = ch;
    }
    cells.into_iter().map(|c| c.trim().replace("\\|", "|")).collect()
}

/// The `|:--|--:|` row: alignments, or `None` when it is not a separator.
fn parse_separator(cells: &[String]) -> Option<Vec<Align>> {
    if cells.is_empty() {
        return None;
    }
    cells
        .iter()
        .map(|c| {
            let core = c.trim_matches(':');
            if core.is_empty() || !core.chars().all(|x| x == '-') {
                return None;
            }
            Some(match (c.starts_with(':'), c.ends_with(':')) {
                (true, true) => Align::Center,
                (false, true) => Align::Right,
                _ => Align::Left,
            })
        })
        .collect()
}

fn normalize(row: &[String], n: usize) -> Vec<String> {
    let mut r: Vec<String> = row.iter().take(n).cloned().collect();
    r.resize(n, String::new());
    r
}

fn pad(styled: &str, width: usize, align: Align) -> String {
    let gap = width.saturating_sub(term::visible_width(styled));
    match align {
        Align::Left => format!("{styled}{}", " ".repeat(gap)),
        Align::Right => format!("{}{styled}", " ".repeat(gap)),
        Align::Center => format!("{}{styled}{}", " ".repeat(gap / 2), " ".repeat(gap - gap / 2)),
    }
}

// ── Code highlighting ───────────────────────────────────────────────────────

struct Lang {
    keywords: &'static [&'static str],
    line_comments: &'static [&'static str],
    /// Is a single quote a string (Python, JS, shell...) or something else (Rust lifetimes)?
    single_quote_strings: bool,
    backtick_strings: bool,
    /// SQL keywords are matched without regard to case.
    ignore_case: bool,
}

fn lang(name: &str) -> Option<Lang> {
    let l = |keywords, line_comments, single_quote_strings, backtick_strings, ignore_case| Lang {
        keywords,
        line_comments,
        single_quote_strings,
        backtick_strings,
        ignore_case,
    };
    Some(match name {
        "rust" | "rs" => l(
            &["fn", "let", "mut", "pub", "struct", "enum", "impl", "trait", "use", "mod", "match", "if", "else", "for", "while", "loop", "return", "self", "Self", "crate", "super", "const", "static", "as", "in", "where", "async", "await", "move", "ref", "type", "unsafe", "dyn", "true", "false", "break", "continue"],
            &["//"], false, false, false,
        ),
        "python" | "py" => l(
            &["def", "class", "return", "if", "elif", "else", "for", "while", "in", "not", "and", "or", "is", "import", "from", "as", "with", "try", "except", "finally", "raise", "lambda", "pass", "None", "True", "False", "yield", "async", "await", "global", "nonlocal", "assert", "del", "break", "continue"],
            &["#"], true, false, false,
        ),
        "javascript" | "js" | "jsx" | "mjs" | "typescript" | "ts" | "tsx" => l(
            &["function", "const", "let", "var", "return", "if", "else", "for", "while", "do", "switch", "case", "break", "continue", "new", "class", "extends", "import", "export", "from", "default", "async", "await", "try", "catch", "finally", "throw", "typeof", "instanceof", "in", "of", "this", "null", "undefined", "true", "false", "interface", "type", "enum", "implements", "public", "private", "readonly"],
            &["//"], true, true, false,
        ),
        "bash" | "sh" | "shell" | "zsh" => l(
            &["if", "then", "else", "elif", "fi", "for", "while", "do", "done", "case", "esac", "function", "in", "echo", "export", "local", "return", "exit", "set", "unset"],
            &["#"], true, false, false,
        ),
        "php" => l(
            &["function", "class", "public", "private", "protected", "static", "return", "if", "else", "elseif", "foreach", "for", "while", "new", "use", "namespace", "echo", "null", "true", "false", "try", "catch", "finally", "throw"],
            &["//", "#"], true, false, false,
        ),
        "go" | "golang" => l(
            &["func", "package", "import", "var", "const", "type", "struct", "interface", "return", "if", "else", "for", "range", "switch", "case", "default", "go", "defer", "chan", "map", "nil", "true", "false", "break", "continue", "select"],
            &["//"], false, true, false,
        ),
        "sql" => l(
            &["select", "from", "where", "insert", "into", "values", "update", "set", "delete", "create", "table", "drop", "alter", "join", "left", "right", "inner", "outer", "on", "group", "by", "order", "limit", "and", "or", "not", "null", "as", "in", "like"],
            &["--"], true, false, true,
        ),
        "json" | "jsonc" => l(&["true", "false", "null"], &[], false, false, false),
        "toml" => l(&["true", "false"], &["#"], true, false, false),
        "yaml" | "yml" => l(&["true", "false", "null"], &["#"], true, false, false),
        _ => return None,
    })
}

/// One source line, coloured: keywords magenta, strings green, numbers yellow, comments grey.
/// A language it does not know, or no colour, leaves the line as it was.
pub fn highlight(lang_name: &str, line: &str, color: bool) -> String {
    let Some(lg) = lang(lang_name).filter(|_| color) else {
        return line.to_string();
    };
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let is_ident = |c: char| c.is_alphanumeric() || c == '_';
    while i < chars.len() {
        let rest: String = chars[i..].iter().collect();
        if lg.line_comments.iter().any(|c| rest.starts_with(c)) {
            out.push_str(&paint_with(true, "90", &rest));
            break;
        }
        let c = chars[i];
        let quote = match c {
            '"' => true,
            '\'' => lg.single_quote_strings,
            '`' => lg.backtick_strings,
            _ => false,
        };
        if quote {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != c {
                j += if chars[j] == '\\' { 2 } else { 1 };
            }
            let end = (j + 1).min(chars.len());
            let s: String = chars[i..end].iter().collect();
            out.push_str(&paint_with(true, "32", &s));
            i = end;
        } else if c.is_ascii_digit() && (i == 0 || !is_ident(chars[i - 1])) {
            let mut j = i;
            while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_' || chars[j] == '.') {
                j += 1;
            }
            let s: String = chars[i..j].iter().collect();
            out.push_str(&paint_with(true, "33", &s));
            i = j;
        } else if c.is_alphabetic() || c == '_' {
            let mut j = i;
            while j < chars.len() && is_ident(chars[j]) {
                j += 1;
            }
            let word: String = chars[i..j].iter().collect();
            let hit = if lg.ignore_case {
                lg.keywords.contains(&word.to_lowercase().as_str())
            } else {
                lg.keywords.contains(&word.as_str())
            };
            out.push_str(&if hit { paint_with(true, "35", &word) } else { word });
            i = j;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn md(color: bool) -> Md {
        Md::with(color, 80)
    }

    fn one(color: bool, line: &str) -> String {
        let mut m = md(color);
        let mut out = m.feed(line);
        out.extend(m.finish());
        out.join("\n")
    }

    #[test]
    fn plain_text_passes_through() {
        assert_eq!(one(false, "só texto"), "só texto");
    }

    #[test]
    fn inline_styles_are_dropped_without_colour_and_painted_with_it() {
        assert_eq!(one(false, "use **negrito**, *itálico* e ~~riscado~~"), "use negrito, itálico e riscado");
        assert_eq!(one(false, "rode `cargo test`"), "rode `cargo test`", "backticks stay: they carry meaning");
        let c = one(true, "**forte**");
        assert_eq!(c, "\x1b[1mforte\x1b[22m");
    }

    #[test]
    fn styles_nest_without_cancelling_each_other() {
        let c = one(true, "**negrito com `código`**");
        assert!(c.contains("\x1b[1m") && c.contains("\x1b[36mcódigo\x1b[39m"), "{c:?}");
        assert!(!c.contains("\x1b[0m"), "no full reset inside a nested style: {c:?}");
        assert!(c.ends_with("\x1b[22m"), "the bold is closed after the code: {c:?}");
    }

    #[test]
    fn code_spans_are_not_parsed_inside() {
        assert_eq!(one(false, "`a **b** c`"), "`a **b** c`");
    }

    #[test]
    fn a_link_shows_its_text_and_its_address() {
        assert_eq!(one(false, "veja [o site](https://ai.shvia.org) agora"), "veja o site (https://ai.shvia.org) agora");
    }

    #[test]
    fn headings_lose_their_hashes() {
        assert_eq!(one(false, "# Título"), "Título");
        assert_eq!(one(false, "### Sub *ênfase*"), "Sub ênfase");
        assert!(one(true, "## Dois").contains("1;36"));
    }

    #[test]
    fn bullets_nest_and_checkboxes_render() {
        assert_eq!(one(false, "- item"), "• item");
        assert_eq!(one(false, "  - filho"), "  ◦ filho");
        assert_eq!(one(false, "    * neto"), "    ▪ neto");
        assert_eq!(one(false, "- [x] feito"), "☑ feito");
        assert_eq!(one(false, "- [ ] a fazer"), "☐ a fazer");
    }

    #[test]
    fn ordered_lists_keep_their_numbers() {
        assert_eq!(one(false, "1. um"), "1. um");
        assert_eq!(one(false, "10) dez"), "10. dez");
        assert_eq!(one(false, "  2. aninhado"), "  2. aninhado");
    }

    #[test]
    fn quotes_and_rules() {
        assert_eq!(one(false, "> citado"), "▎ citado");
        assert_eq!(one(false, "---"), "─".repeat(60));
        assert_eq!(one(false, "* * *"), "─".repeat(60));
        assert_eq!(one(false, "a - b"), "a - b", "a dash inside text is not a rule");
    }

    #[test]
    fn a_fence_carries_its_language_and_indents_the_code() {
        let mut m = md(false);
        assert_eq!(m.feed("```rust"), ["┄┄┄ rust"]);
        assert_eq!(m.feed("let x = 1; // um"), ["  let x = 1; // um"]);
        assert_eq!(m.feed("```"), ["┄┄┄"]);
        assert_eq!(m.feed("depois"), ["depois"]);
    }

    #[test]
    fn markdown_inside_a_fence_is_left_alone() {
        let mut m = md(false);
        m.feed("```");
        assert_eq!(m.feed("# não é título"), ["  # não é título"]);
    }

    #[test]
    fn an_unclosed_fence_does_not_leak_into_the_next_answer() {
        let mut m = md(false);
        m.feed("```py");
        m.reset();
        assert_eq!(m.feed("# título"), ["título"]);
    }

    #[test]
    fn a_table_waits_for_its_last_row_then_aligns() {
        let mut m = md(false);
        assert!(m.feed("| Infra | Modelos |").is_empty(), "held back");
        assert!(m.feed("|:------|--------:|").is_empty());
        assert!(m.feed("| kilo | 392 |").is_empty());
        let out = m.feed("depois da tabela");
        let expected = [
            "┌───────┬─────────┐",
            "│ Infra │ Modelos │",
            "├───────┼─────────┤",
            "│ kilo  │     392 │",
            "└───────┴─────────┘",
            "depois da tabela",
        ];
        assert_eq!(out, expected);
    }

    #[test]
    fn a_table_that_ends_the_answer_is_flushed_by_finish() {
        let mut m = md(false);
        m.feed("| a |");
        m.feed("|---|");
        m.feed("| 1 |");
        let out = m.finish();
        assert_eq!(out.len(), 5, "{out:?}");
        assert!(m.finish().is_empty(), "flushed once");
    }

    #[test]
    fn every_row_of_a_table_has_the_same_visible_width_even_with_styled_cells() {
        let mut m = md(true);
        m.feed("| Nome | N |");
        m.feed("|---|---:|");
        m.feed("| **forte** | 1 |");
        m.feed("| `x` | 22 |");
        let out = m.finish();
        let w: Vec<usize> = out.iter().map(|l| term::visible_width(l)).collect();
        assert!(w.iter().all(|x| *x == w[0]), "{w:?}\n{out:?}");
    }

    #[test]
    fn a_center_aligned_column_pads_both_sides() {
        let mut m = md(false);
        m.feed("| abcde |");
        m.feed("|:-:|");
        m.feed("| x |");
        let out = m.finish();
        assert_eq!(out[3], "│   x   │");
    }

    #[test]
    fn a_table_wider_than_the_screen_falls_back_to_plain_rows() {
        let mut m = Md::with(false, 30);
        m.feed("| uma coluna bem larga | outra coluna bem larga |");
        m.feed("|---|---|");
        m.feed("| a | b |");
        let out = m.finish();
        assert_eq!(out, ["uma coluna bem larga │ outra coluna bem larga", "a │ b"]);
    }

    #[test]
    fn pipes_without_a_separator_row_are_just_text() {
        let mut m = md(false);
        m.feed("| isto | não é tabela |");
        m.feed("| nem isto |");
        assert_eq!(m.finish(), ["| isto | não é tabela |", "| nem isto |"]);
    }

    #[test]
    fn an_escaped_pipe_stays_in_its_cell() {
        assert_eq!(split_cells("| a \\| b | c |"), ["a | b", "c"]);
    }

    #[test]
    fn highlight_colours_keywords_strings_numbers_and_comments() {
        let h = highlight("rust", "let s = \"oi\"; // fim 42", true);
        assert!(h.contains("\x1b[35mlet\x1b[0m"), "{h:?}");
        assert!(h.contains("\x1b[32m\"oi\"\x1b[0m"));
        assert!(h.contains("\x1b[90m// fim 42\x1b[0m"), "the comment swallows the rest: {h:?}");
        let n = highlight("rust", "a + 42", true);
        assert!(n.contains("\x1b[33m42\x1b[0m"));
    }

    #[test]
    fn highlight_leaves_identifiers_that_contain_keywords_alone() {
        let h = highlight("rust", "letter = format", true);
        assert!(!h.contains('\x1b'), "'letter' is not 'let': {h:?}");
    }

    #[test]
    fn highlight_needs_colour_and_a_known_language() {
        assert_eq!(highlight("rust", "let x = 1;", false), "let x = 1;");
        assert_eq!(highlight("brainfuck", "+++", true), "+++");
        assert_eq!(highlight("", "let", true), "let");
    }

    #[test]
    fn a_rust_lifetime_is_not_a_string_but_a_python_quote_is() {
        assert!(!highlight("rust", "fn f<'a>(x: &'a str)", true).contains("\x1b[32m"));
        assert!(highlight("python", "x = 'oi'", true).contains("\x1b[32m'oi'"));
    }

    #[test]
    fn sql_keywords_ignore_case_and_dashes_start_a_comment() {
        let h = highlight("sql", "Select * from t -- pronto", true);
        assert!(h.contains("\x1b[35mSelect\x1b[0m") && h.contains("\x1b[90m-- pronto"), "{h:?}");
    }

    #[test]
    fn an_unterminated_string_runs_to_the_end_of_the_line() {
        let h = highlight("python", "x = \"aberta", true);
        assert!(h.ends_with("\x1b[32m\"aberta\x1b[0m"), "{h:?}");
    }
}
