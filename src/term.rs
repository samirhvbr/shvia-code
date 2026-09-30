//! What the terminal can do, decided once — and the only place that writes ANSI.
//!
//! Until 0.12.3 every module printed its own escape codes, so `anna -p ... | tee log` filled
//! the log with `ESC[92mANNA` and there was no way to turn colour off. Now the rule lives here:
//!
//! - colour is written only when stdout is a terminal (`TERM` is not `dumb`), or when
//!   `FORCE_COLOR` asks for it; `NO_COLOR` (no-color.org) wins over both.
//!
//! The decision is a pure function of the environment ([`Caps::detect`]), so the tests cover
//! every combination without touching the real terminal.

use std::io::IsTerminal;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    /// SGR colours and styles may be written.
    pub color: bool,
}

impl Caps {
    /// The decision, from an environment lookup and whether stdout is a terminal.
    pub fn detect(env: impl Fn(&str) -> Option<String>, tty_out: bool) -> Self {
        let get = |k: &str| env(k).filter(|v| !v.is_empty());
        let dumb = get("TERM").as_deref() == Some("dumb");
        let color = if get("NO_COLOR").is_some() {
            false
        } else if get("FORCE_COLOR").is_some_and(|v| v != "0") {
            true
        } else {
            tty_out && !dumb
        };
        Self { color }
    }
}

/// The real environment, read once.
pub fn caps() -> &'static Caps {
    static CAPS: OnceLock<Caps> = OnceLock::new();
    CAPS.get_or_init(|| Caps::detect(|k| std::env::var(k).ok(), std::io::stdout().is_terminal()))
}

// ── Painting ────────────────────────────────────────────────────────────────

/// Wraps `text` in an SGR sequence when `color` is on; returns it untouched otherwise.
pub fn paint_with(color: bool, sgr: &str, text: &str) -> String {
    if color && !text.is_empty() {
        format!("\x1b[{sgr}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

pub fn paint(sgr: &str, text: &str) -> String {
    paint_with(caps().color, sgr, text)
}

pub fn bold(t: &str) -> String {
    paint("1", t)
}
pub fn dim(t: &str) -> String {
    paint("2", t)
}
pub fn red(t: &str) -> String {
    paint("91", t)
}
pub fn green(t: &str) -> String {
    paint("92", t)
}
pub fn yellow(t: &str) -> String {
    paint("93", t)
}
pub fn gray(t: &str) -> String {
    paint("90", t)
}
/// The brand colour: what the banner uses.
pub fn accent(t: &str) -> String {
    paint("1;36", t)
}

/// Removes CSI escape sequences (`ESC [ ... final`), leaving the text a person would read.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            // Parameters and intermediates run until a final byte in @..~.
            for n in chars.by_ref() {
                if ('@'..='~').contains(&n) {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Prints text that may already carry escape codes (modules that build a whole screen at once),
/// stripping them when colour is off.
pub fn emit(s: &str) {
    if caps().color {
        print!("{s}");
    } else {
        print!("{}", strip_ansi(s));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
    }

    #[test]
    fn a_terminal_gets_colour_and_a_pipe_does_not() {
        assert!(Caps::detect(env(&[]), true).color);
        assert!(!Caps::detect(env(&[]), false).color, "stdout piped");
    }

    #[test]
    fn no_color_wins_over_everything() {
        assert!(!Caps::detect(env(&[("NO_COLOR", "1")]), true).color);
        assert!(!Caps::detect(env(&[("NO_COLOR", "1"), ("FORCE_COLOR", "1")]), true).color);
        // An empty NO_COLOR is "not set" (no-color.org).
        assert!(Caps::detect(env(&[("NO_COLOR", "")]), true).color);
    }

    #[test]
    fn force_color_paints_a_pipe_but_zero_does_not() {
        assert!(Caps::detect(env(&[("FORCE_COLOR", "1")]), false).color);
        assert!(!Caps::detect(env(&[("FORCE_COLOR", "0")]), false).color);
    }

    #[test]
    fn a_dumb_terminal_gets_no_colour() {
        assert!(!Caps::detect(env(&[("TERM", "dumb")]), true).color);
    }

    #[test]
    fn paint_is_plain_when_colour_is_off() {
        assert_eq!(paint_with(false, "91", "erro"), "erro");
        assert_eq!(paint_with(true, "91", "erro"), "\x1b[91merro\x1b[0m");
        assert_eq!(paint_with(true, "91", ""), "", "nothing to paint, nothing to reset");
    }

    #[test]
    fn strip_ansi_leaves_what_a_person_reads() {
        assert_eq!(strip_ansi("\x1b[1;36mANNA\x1b[0m pronto"), "ANNA pronto");
        assert_eq!(strip_ansi("\x1b[48;2;1;2;3m  \x1b[0m"), "  ");
        assert_eq!(strip_ansi("sem nada"), "sem nada");
    }
}
