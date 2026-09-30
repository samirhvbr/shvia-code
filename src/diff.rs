//! Diff unificado (crate `similar`) — o preview de `edit_file`/`write_file`.
//! `color=true` p/ o terminal (ANSI); `false` p/ o preview NDJSON do host.
//! Agrupa em hunks com 3 linhas de contexto.

use crate::term::{self, paint_with, Caps};
use crate::view;
use similar::{ChangeTag, TextDiff};

pub fn unified(path: &str, old: &str, new: &str, color: bool) -> String {
    let p = |sgr: &str, t: &str| paint_with(color, sgr, t);
    let diff = TextDiff::from_lines(old, new);
    let mut out = format!("{}\n", p("1", &format!("  ✎ {path}")));
    let groups = diff.grouped_ops(3);
    if groups.is_empty() {
        out.push_str(&format!("{}\n", p("90", "    (sem mudanças)")));
        return out;
    }
    for (gi, group) in groups.iter().enumerate() {
        if gi > 0 {
            out.push_str(&format!("{}\n", p("90", "    ⋮")));
        }
        for op in group {
            for change in diff.iter_changes(op) {
                let (sign, sgr) = match change.tag() {
                    ChangeTag::Delete => ("-", "91"),
                    ChangeTag::Insert => ("+", "92"),
                    ChangeTag::Equal => (" ", "90"),
                };
                let line = change.value();
                let line = line.strip_suffix('\n').unwrap_or(line);
                out.push_str(&format!("{}\n", p(sgr, &format!("  {sign} {line}"))));
            }
        }
    }
    out
}

/// The diff the person approves in the terminal: a summary line, then each changed line with its
/// number, its sign and — on a terminal with colour — a background over the whole width, red for
/// what goes and green for what comes. Unchanged context is grey. Without colour the same lines
/// come out plain, signs and numbers intact. Nothing is left out: the approval is for the whole
/// change, so the preview is too.
pub fn pretty(path: &str, old: &str, new: &str, width: usize, caps: &Caps) -> String {
    let _ = path; // the `● Update(path)` title already names the file
    let diff = TextDiff::from_lines(old, new);
    let adds = diff.iter_all_changes().filter(|c| c.tag() == ChangeTag::Insert).count();
    let dels = diff.iter_all_changes().filter(|c| c.tag() == ChangeTag::Delete).count();
    let summary = view::diff_summary(adds, dels, old.is_empty() && !new.is_empty());
    let mut out = format!("  {}\n", paint_with(caps.color, "90", &summary));

    let groups = diff.grouped_ops(3);
    if groups.is_empty() {
        return out;
    }
    let digits = old.lines().count().max(new.lines().count()).to_string().len().max(2);
    let bg_del = term::bg_params(caps, (74, 24, 24), 52);
    let bg_ins = term::bg_params(caps, (24, 62, 32), 22);
    for (gi, group) in groups.iter().enumerate() {
        if gi > 0 {
            out.push_str(&format!("{}\n", paint_with(caps.color, "90", &format!("  {:>digits$}", "⋮"))));
        }
        for op in group {
            for change in diff.iter_changes(op) {
                let (sign, number, bg) = match change.tag() {
                    ChangeTag::Delete => ('-', change.old_index(), Some(&bg_del)),
                    ChangeTag::Insert => ('+', change.new_index(), Some(&bg_ins)),
                    ChangeTag::Equal => (' ', change.new_index(), None),
                };
                let text = change.value().strip_suffix('\n').unwrap_or(change.value()).replace('\t', "    ");
                let n = number.map(|i| (i + 1).to_string()).unwrap_or_default();
                let line = format!("  {n:>digits$} {sign} {text}");
                let painted = match bg {
                    Some(bg) if caps.color => {
                        let pad = width.saturating_sub(1).saturating_sub(term::visible_width(&line));
                        paint_with(true, bg, &format!("{line}{}", " ".repeat(pad)))
                    }
                    Some(_) => line,
                    None => paint_with(caps.color, "90", &line),
                };
                out.push_str(&painted);
                out.push('\n');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(color: bool, truecolor: bool) -> Caps {
        Caps { color, tty_out: true, tty_in: true, dumb: false, truecolor }
    }

    const OLD: &str = "um\ndois\ntres\n";
    const NEW: &str = "um\nDOIS\nextra\ntres\n";

    #[test]
    fn plain_diff_keeps_numbers_and_signs() {
        let d = pretty("a.txt", OLD, NEW, 80, &caps(false, false));
        let lines: Vec<&str> = d.lines().collect();
        assert_eq!(lines[0], "  2 linhas adicionadas, 1 linha removida");
        assert!(d.contains("   1   um"), "context keeps its number:\n{d}");
        assert!(d.contains("   2 - dois"), "a removal takes the OLD number:\n{d}");
        assert!(d.contains("   2 + DOIS"), "an addition takes the NEW number:\n{d}");
        assert!(d.contains("   3 + extra"));
        assert!(!d.contains('\x1b'), "no escape codes without colour");
    }

    #[test]
    fn a_coloured_diff_fills_the_line_with_a_background() {
        let d = pretty("a.txt", OLD, NEW, 60, &caps(true, false));
        assert!(d.contains("48;5;52"), "removed lines are red on 256 colours:\n{d:?}");
        assert!(d.contains("48;5;22"), "added lines are green on 256 colours");
        for l in d.lines().filter(|l| l.contains("48;5;")) {
            assert_eq!(term::visible_width(l), 59, "the band spans the width less one column: {l:?}");
        }
        let tc = pretty("a.txt", OLD, NEW, 60, &caps(true, true));
        assert!(tc.contains("48;2;74;24;24") && tc.contains("48;2;24;62;32"));
    }

    #[test]
    fn distant_hunks_are_separated() {
        let old: String = (1..=20).map(|i| format!("l{i}\n")).collect();
        let new = old.replace("l2\n", "L2\n").replace("l19\n", "L19\n");
        let d = pretty("a", &old, &new, 80, &caps(false, false));
        assert!(d.lines().any(|l| l.trim() == "⋮"), "hunks are split by a marker:\n{d}");
    }

    #[test]
    fn a_new_file_says_so_and_shows_every_line() {
        let d = pretty("n.txt", "", "a\nb\nc\n", 80, &caps(false, false));
        assert!(d.starts_with("  Novo arquivo · 3 linhas"));
        assert_eq!(d.lines().filter(|l| l.contains(" + ")).count(), 3);
    }

    #[test]
    fn identical_content_is_reported_as_no_change() {
        let d = pretty("a", "x\n", "x\n", 80, &caps(false, false));
        assert_eq!(d.trim(), "sem mudanças");
    }

    #[test]
    fn tabs_do_not_break_the_band() {
        let d = pretty("a", "a\n", "a\n\tb\n", 40, &caps(true, false));
        for l in d.lines().filter(|l| l.contains("48;5;")) {
            assert_eq!(term::visible_width(l), 39);
        }
    }
}
