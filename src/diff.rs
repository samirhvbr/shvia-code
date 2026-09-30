//! Diff unificado (crate `similar`) — o preview de `edit_file`/`write_file`.
//! `color=true` p/ o terminal (ANSI); `false` p/ o preview NDJSON do host.
//! Agrupa em hunks com 3 linhas de contexto.

use crate::term::paint_with;
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
