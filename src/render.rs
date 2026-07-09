//! Render leve de markdown **por linha** — funciona com streaming (cada linha é
//! estilizada quando completa). Cobre headings, listas, e inline
//! negrito/itálico/código. Blocos de código (```) são tratados no `ui` (estado
//! `in_code`). Suficiente pro terminal; render rico/TUI é evolução do C3.

use regex::Regex;
use std::sync::OnceLock;

const BOLD: &str = "\x1b[1m";
const ITALIC: &str = "\x1b[3m";
const CYAN: &str = "\x1b[36m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

/// Estiliza uma linha já completa. `in_code` = estamos dentro de um bloco ```.
pub fn style_line(line: &str, in_code: bool) -> String {
    if in_code {
        return format!("{DIM}  {line}{RESET}");
    }
    let t = line.trim_start();
    if let Some(rest) = t
        .strip_prefix("#### ")
        .or_else(|| t.strip_prefix("### "))
        .or_else(|| t.strip_prefix("## "))
        .or_else(|| t.strip_prefix("# "))
    {
        return format!("{BOLD}{}{RESET}", inline(rest));
    }
    if let Some(rest) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
        return format!("  • {}", inline(rest));
    }
    inline(line)
}

fn cache(re: &'static OnceLock<Regex>, pat: &str) -> &'static Regex {
    re.get_or_init(|| Regex::new(pat).unwrap())
}

fn inline(s: &str) -> String {
    static CODE: OnceLock<Regex> = OnceLock::new();
    static BOLD_RE: OnceLock<Regex> = OnceLock::new();
    static ITALIC_RE: OnceLock<Regex> = OnceLock::new();
    let s = cache(&CODE, r"`([^`]+)`").replace_all(s, format!("{CYAN}${{1}}{RESET}").as_str());
    let s = cache(&BOLD_RE, r"\*\*([^*]+)\*\*").replace_all(&s, format!("{BOLD}${{1}}{RESET}").as_str());
    let s = cache(&ITALIC_RE, r"\*([^*\s][^*]*)\*").replace_all(&s, format!("{ITALIC}${{1}}{RESET}").as_str());
    s.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estiliza_inline_e_headings() {
        assert!(style_line("# Título", false).contains("Título"));
        assert!(style_line("- item", false).contains("• item"));
        assert!(inline("use `cargo test` e **negrito**").contains("cargo test"));
        assert!(style_line("linha crua", true).contains("linha crua")); // code block: sem parse
    }
}
