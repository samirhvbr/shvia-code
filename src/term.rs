//! What the terminal can do, decided once — and the only place that writes ANSI.
//!
//! Until 0.12.3 every module printed its own escape codes, so `anna -p ... | tee log` filled
//! the log with `ESC[92mANNA` and there was no way to turn colour off. Now the rule lives here:
//!
//! - colour is written only when stdout is a terminal (`TERM` is not `dumb`), or when
//!   `FORCE_COLOR` asks for it; `NO_COLOR` (no-color.org) wins over both;
//! - the width comes from the terminal when there is one, from `COLUMNS` otherwise;
//! - the spinner, which rewrites its own line, runs only where there is a terminal to write to;
//! - the approval menu reads single keys, so it needs a terminal on BOTH ends (stdin and stdout).
//!
//! The decision is a pure function of the environment ([`Caps::detect`]), so the tests cover
//! every combination without touching the real terminal.

use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    /// SGR colours and styles may be written.
    pub color: bool,
    /// stdout is a terminal.
    pub tty_out: bool,
    /// stdin is a terminal.
    pub tty_in: bool,
    /// `TERM=dumb`: a terminal that cannot move the cursor or paint.
    pub dumb: bool,
    /// 24-bit colour is available; otherwise the 256-colour palette is used.
    pub truecolor: bool,
}

impl Caps {
    /// The decision, from an environment lookup and whether stdout and stdin are terminals.
    pub fn detect(env: impl Fn(&str) -> Option<String>, tty_out: bool, tty_in: bool) -> Self {
        let get = |k: &str| env(k).filter(|v| !v.is_empty());
        let dumb = get("TERM").as_deref() == Some("dumb");
        let color = if get("NO_COLOR").is_some() {
            false
        } else if get("FORCE_COLOR").is_some_and(|v| v != "0") {
            true
        } else {
            tty_out && !dumb
        };
        let truecolor = get("COLORTERM")
            .is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "truecolor" | "24bit"));
        Self { color, tty_out, tty_in, dumb, truecolor }
    }

    /// Key-by-key input makes sense: a terminal on both ends.
    pub fn interactive(&self) -> bool {
        self.tty_out && self.tty_in && !self.dumb
    }

    /// A spinner rewrites its own line, so it needs a terminal to write to.
    pub fn can_animate(&self) -> bool {
        self.tty_out && !self.dumb
    }
}

/// The real environment, read once.
pub fn caps() -> &'static Caps {
    static CAPS: OnceLock<Caps> = OnceLock::new();
    CAPS.get_or_init(|| {
        Caps::detect(
            |k| std::env::var(k).ok(),
            std::io::stdout().is_terminal(),
            std::io::stdin().is_terminal(),
        )
    })
}

/// Terminal width in columns, clamped to something a layout can live with.
pub fn width() -> usize {
    let from_terminal = if caps().tty_out {
        crossterm::terminal::size().ok().map(|(w, _)| w as usize)
    } else {
        None
    };
    from_terminal
        .or_else(|| std::env::var("COLUMNS").ok().and_then(|v| v.trim().parse().ok()))
        .unwrap_or(80)
        .clamp(40, 240)
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
/// The brand colour: the welcome box, the assistant's marker and the spinner.
pub fn accent(t: &str) -> String {
    paint("1;36", t)
}

/// SGR parameters for a background: true colour when the terminal has it, else the closest
/// entry of the 256-colour palette.
pub fn bg_params(caps: &Caps, rgb: (u8, u8, u8), fallback256: u8) -> String {
    if caps.truecolor {
        format!("48;2;{};{};{}", rgb.0, rgb.1, rgb.2)
    } else {
        format!("48;5;{fallback256}")
    }
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

/// Columns the text occupies on screen, ignoring escape sequences.
pub fn visible_width(s: &str) -> usize {
    strip_ansi(s).width()
}

/// Cuts `s` to at most `max` columns, ending in `…` when something was dropped.
pub fn truncate_cols(s: &str, max: usize) -> String {
    if s.width() <= max {
        return s.to_string();
    }
    let mut out = String::new();
    let mut used = 0usize;
    for ch in s.chars() {
        let w = ch.width().unwrap_or(0);
        if used + w + 1 > max {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.push('…');
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

// ── Spinner ─────────────────────────────────────────────────────────────────

/// `✻ Pensando… (12s)`, redrawn in place until it is stopped.
///
/// It owns the current line while it runs, so whoever prints next must stop it first (`ui.rs`
/// does, at the top of every method that prints). `stop` joins the thread, so by the time it
/// returns the line is clear and nothing else writes concurrently. Dropping stops it too: an
/// error that unwinds out of a turn cannot leave a spinner running over the message.
pub struct Spinner {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Spinner {
    /// `None` when there is no terminal to animate on (a pipe, `TERM=dumb`).
    pub fn start(label: &str) -> Option<Self> {
        if !caps().can_animate() {
            return None;
        }
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let label = label.to_string();
        let handle = std::thread::spawn(move || {
            const FRAMES: [&str; 6] = ["·", "✢", "✳", "✶", "✻", "✽"];
            let started = Instant::now();
            let mut frame = 0usize;
            while !flag.load(Ordering::Relaxed) {
                let elapsed = crate::view::secs(started.elapsed().as_secs());
                let line = format!(
                    "{} {label} {}",
                    accent(FRAMES[frame % FRAMES.len()]),
                    gray(&format!("({elapsed})"))
                );
                {
                    let mut out = std::io::stdout().lock();
                    let _ = write!(out, "\r\x1b[2K{line}");
                    let _ = out.flush();
                }
                frame += 1;
                // `unpark` in `stop` wakes this at once, so stopping never waits for a frame.
                std::thread::park_timeout(Duration::from_millis(100));
            }
            let mut out = std::io::stdout().lock();
            let _ = write!(out, "\r\x1b[2K");
            let _ = out.flush();
        });
        Some(Self { stop, handle: Some(handle) })
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            h.thread().unpark();
            let _ = h.join();
        }
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.stop();
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
        assert!(Caps::detect(env(&[]), true, true).color);
        assert!(!Caps::detect(env(&[]), false, false).color, "stdout piped");
    }

    #[test]
    fn no_color_wins_over_everything() {
        assert!(!Caps::detect(env(&[("NO_COLOR", "1")]), true, true).color);
        assert!(!Caps::detect(env(&[("NO_COLOR", "1"), ("FORCE_COLOR", "1")]), true, true).color);
        // An empty NO_COLOR is "not set" (no-color.org).
        assert!(Caps::detect(env(&[("NO_COLOR", "")]), true, true).color);
    }

    #[test]
    fn force_color_paints_a_pipe_but_zero_does_not() {
        assert!(Caps::detect(env(&[("FORCE_COLOR", "1")]), false, false).color);
        assert!(!Caps::detect(env(&[("FORCE_COLOR", "0")]), false, false).color);
    }

    #[test]
    fn a_dumb_terminal_gets_no_colour_and_no_animation() {
        let c = Caps::detect(env(&[("TERM", "dumb")]), true, true);
        assert!(c.dumb && !c.color && !c.can_animate());
    }

    #[test]
    fn interactive_needs_a_terminal_on_both_ends() {
        assert!(Caps::detect(env(&[]), true, true).interactive());
        assert!(!Caps::detect(env(&[]), true, false).interactive(), "stdin piped: no menu");
        assert!(!Caps::detect(env(&[]), false, true).interactive(), "stdout piped: no menu");
        assert!(!Caps::detect(env(&[("TERM", "dumb")]), true, true).interactive());
    }

    #[test]
    fn truecolor_comes_from_colorterm_and_the_background_falls_back_to_256() {
        let tc = Caps::detect(env(&[("COLORTERM", "truecolor")]), true, true);
        let no = Caps::detect(env(&[("TERM", "xterm-256color")]), true, true);
        assert!(tc.truecolor && !no.truecolor);
        assert_eq!(bg_params(&tc, (1, 2, 3), 22), "48;2;1;2;3");
        assert_eq!(bg_params(&no, (1, 2, 3), 22), "48;5;22");
    }

    #[test]
    fn only_a_real_terminal_animates() {
        assert!(Caps::detect(env(&[]), true, true).can_animate());
        assert!(!Caps::detect(env(&[]), false, false).can_animate(), "a pipe has no line to rewrite");
        // Forcing colour into a pipe does not make it animatable.
        assert!(!Caps::detect(env(&[("FORCE_COLOR", "1")]), false, false).can_animate());
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

    #[test]
    fn visible_width_ignores_escapes_and_counts_wide_chars() {
        assert_eq!(visible_width("\x1b[1mabc\x1b[0m"), 3);
        assert_eq!(visible_width("日本"), 4, "CJK is two columns per char");
    }

    #[test]
    fn truncate_cols_marks_the_cut() {
        assert_eq!(truncate_cols("curto", 10), "curto");
        assert_eq!(truncate_cols("0123456789", 6), "01234…");
        assert!(truncate_cols("日本語の文字", 7).width() <= 7);
    }

    #[test]
    fn width_never_leaves_the_range_a_layout_can_use() {
        let w = width();
        assert!((40..=240).contains(&w), "{w}");
    }

    #[test]
    fn a_pipe_gets_no_spinner() {
        // Under `cargo test` stdout is not a terminal.
        assert!(Spinner::start("Pensando…").is_none());
    }
}
