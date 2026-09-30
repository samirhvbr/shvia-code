//! Render leve de markdown **por linha** — funciona com streaming (cada linha é
//! estilizada quando completa). Cobre headings, listas, e inline
//! negrito/itálico/código. Blocos de código (```) são tratados no `ui` (estado
//! `in_code`). Suficiente pro terminal; render rico/TUI é evolução do C3.

use crate::term;
use regex::Regex;
use std::sync::OnceLock;

/// Estiliza uma linha já completa. `in_code` = estamos dentro de um bloco ```.
pub fn style_line(line: &str, in_code: bool) -> String {
    if in_code {
        return term::dim(&format!("  {line}"));
    }
    let t = line.trim_start();
    if let Some(rest) = t
        .strip_prefix("#### ")
        .or_else(|| t.strip_prefix("### "))
        .or_else(|| t.strip_prefix("## "))
        .or_else(|| t.strip_prefix("# "))
    {
        return term::bold(&inline(rest));
    }
    if let Some(rest) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
        return format!("  • {}", inline(rest));
    }
    inline(line)
}

fn cache(re: &'static OnceLock<Regex>, pat: &str) -> &'static Regex {
    re.get_or_init(|| Regex::new(pat).unwrap())
}

/// Replacement text that wraps capture group 1 in an SGR sequence — or leaves it bare when
/// colour is off, so a piped `anna -p` shows the words and not the escape codes.
fn wrap(sgr: &str) -> String {
    if term::caps().color {
        format!("\x1b[{sgr}m${{1}}\x1b[0m")
    } else {
        "${1}".to_string()
    }
}

fn inline(s: &str) -> String {
    static CODE: OnceLock<Regex> = OnceLock::new();
    static BOLD_RE: OnceLock<Regex> = OnceLock::new();
    static ITALIC_RE: OnceLock<Regex> = OnceLock::new();
    let s = cache(&CODE, r"`([^`]+)`").replace_all(s, wrap("36").as_str());
    let s = cache(&BOLD_RE, r"\*\*([^*]+)\*\*").replace_all(&s, wrap("1").as_str());
    let s = cache(&ITALIC_RE, r"\*([^*\s][^*]*)\*").replace_all(&s, wrap("3").as_str());
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
