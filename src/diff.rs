//! Diff unificado colorido (crate `similar`) — o preview de `edit_file`/
//! `write_file` mostrado ANTES da confirmação do gate (nada escreve sem o dev
//! ver, docs/ferramentas.md §1). Agrupa em hunks com 3 linhas de contexto.

use similar::{ChangeTag, TextDiff};

const RED: &str = "\x1b[91m";
const GREEN: &str = "\x1b[92m";
const GRAY: &str = "\x1b[90m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

pub fn unified(path: &str, old: &str, new: &str) -> String {
    let diff = TextDiff::from_lines(old, new);
    let mut out = format!("{BOLD}  ✎ {path}{RESET}\n");
    let groups = diff.grouped_ops(3);
    if groups.is_empty() {
        out.push_str(&format!("{GRAY}    (sem mudanças){RESET}\n"));
        return out;
    }
    for (gi, group) in groups.iter().enumerate() {
        if gi > 0 {
            out.push_str(&format!("{GRAY}    ⋮{RESET}\n"));
        }
        for op in group {
            for change in diff.iter_changes(op) {
                let (sign, color) = match change.tag() {
                    ChangeTag::Delete => ("-", RED),
                    ChangeTag::Insert => ("+", GREEN),
                    ChangeTag::Equal => (" ", GRAY),
                };
                let line = change.value();
                let line = line.strip_suffix('\n').unwrap_or(line);
                out.push_str(&format!("{color}  {sign} {line}{RESET}\n"));
            }
        }
    }
    out
}
