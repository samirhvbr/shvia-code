//! Diff unificado (crate `similar`) — o preview de `edit_file`/`write_file`.
//! `color=true` p/ o terminal (ANSI); `false` p/ o preview NDJSON do host.
//! Agrupa em hunks com 3 linhas de contexto.

use similar::{ChangeTag, TextDiff};

const RED: &str = "\x1b[91m";
const GREEN: &str = "\x1b[92m";
const GRAY: &str = "\x1b[90m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

pub fn unified(path: &str, old: &str, new: &str, color: bool) -> String {
    let (red, green, gray, bold, reset) = if color {
        (RED, GREEN, GRAY, BOLD, RESET)
    } else {
        ("", "", "", "", "")
    };
    let diff = TextDiff::from_lines(old, new);
    let mut out = format!("{bold}  ✎ {path}{reset}\n");
    let groups = diff.grouped_ops(3);
    if groups.is_empty() {
        out.push_str(&format!("{gray}    (sem mudanças){reset}\n"));
        return out;
    }
    for (gi, group) in groups.iter().enumerate() {
        if gi > 0 {
            out.push_str(&format!("{gray}    ⋮{reset}\n"));
        }
        for op in group {
            for change in diff.iter_changes(op) {
                let (sign, code) = match change.tag() {
                    ChangeTag::Delete => ("-", red),
                    ChangeTag::Insert => ("+", green),
                    ChangeTag::Equal => (" ", gray),
                };
                let line = change.value();
                let line = line.strip_suffix('\n').unwrap_or(line);
                out.push_str(&format!("{code}  {sign} {line}{reset}\n"));
            }
        }
    }
    out
}
