//! What a turn looks like, as pure functions: from the model's tool calls and the tools' results
//! to the lines the terminal shows. Nothing here prints, reads the terminal or keeps state, so the
//! whole look of a turn is testable without one (`ui.rs` does the printing, `term.rs` the painting).
//!
//! The shape follows Claude Code (ADR-004): `● Read(src/boleto.rs)` for the action and
//! `⎿  142 linhas` under it for the result — a summary, never the file. The model still gets the
//! full result; this is only what the person is shown.

use crate::term;
use crate::types::ToolCall;

/// How a result line should be painted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Dim,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub text: String,
    pub tone: Tone,
}

fn dim(text: impl Into<String>) -> Line {
    Line { text: text.into(), tone: Tone::Dim }
}

fn err(text: impl Into<String>) -> Line {
    Line { text: text.into(), tone: Tone::Error }
}

/// Lines of output shown under a command or a `git status` before "… +N linhas".
const MAX_BODY: usize = 4;

/// "1 linha" / "3 linhas".
pub fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// A tool result the model must correct or the user refused — shown in red.
pub fn is_error(out: &str) -> bool {
    out.starts_with("ERRO") || out.starts_with("RECUSADO")
}

/// `(verb, argument)` for `● Verb(argument)`.
pub fn call_title(call: &ToolCall) -> (String, String) {
    let a = &call.arguments;
    let s = |k: &str| a.get(k).and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let first_line = |t: String| t.lines().next().unwrap_or("").to_string();
    match call.name.as_str() {
        "read_file" => ("Read".into(), s("path")),
        "list_dir" => {
            let p = s("path");
            ("List".into(), if p.is_empty() { ".".into() } else { p })
        }
        "grep" => {
            let (path, glob) = (s("path"), s("glob"));
            let mut arg = s("pattern");
            if !path.is_empty() && path != "." {
                arg.push_str(&format!(" em {path}"));
            }
            if !glob.is_empty() {
                arg.push_str(&format!(" ({glob})"));
            }
            ("Grep".into(), arg)
        }
        "glob" => ("Glob".into(), s("pattern")),
        "edit_file" => ("Update".into(), s("path")),
        "write_file" => ("Write".into(), s("path")),
        "bash" => ("Bash".into(), first_line(s("command"))),
        "git_status" => ("Git".into(), "status".into()),
        "git_diff" => {
            let staged = a.get("staged").and_then(|v| v.as_bool()).unwrap_or(false);
            ("Git".into(), if staged { "diff --staged".into() } else { "diff".into() })
        }
        "git_commit" => ("Git".into(), "commit".into()),
        other => {
            let args = serde_json::to_string(a).unwrap_or_default();
            (other.to_string(), if args == "{}" || args == "null" { String::new() } else { args })
        }
    }
}

/// The summary lines for a result: the first carries the `⎿` marker, the rest are indented under it.
pub fn result_lines(name: &str, out: &str, width: usize) -> Vec<Line> {
    let room = width.saturating_sub(8).max(20);
    let cut = |t: &str| term::truncate_cols(t, room);
    let first = out.lines().next().unwrap_or("");

    if is_error(out) {
        return vec![err(cut(first))];
    }
    match name {
        "read_file" => {
            let mut n = out.lines().count();
            let truncated = out.lines().last().is_some_and(|l| l.starts_with("… [truncado"));
            if truncated {
                n -= 1;
            }
            let base = plural(n, "linha", "linhas");
            vec![dim(if truncated { format!("{base} (truncado)") } else { base })]
        }
        "list_dir" => {
            if out.trim() == "(vazio)" {
                return vec![dim("vazio")];
            }
            vec![dim(plural(out.lines().count(), "item", "itens"))]
        }
        "grep" => {
            if out.trim() == "(nenhum resultado)" {
                return vec![dim("nenhum resultado")];
            }
            let cut_marker = out.lines().last().is_some_and(|l| l.starts_with("… [cortado"));
            let n = out.lines().count() - usize::from(cut_marker);
            let base = plural(n, "resultado", "resultados");
            vec![dim(if cut_marker { format!("{base} (cortado)") } else { base })]
        }
        "glob" => {
            if out.trim() == "(nenhum arquivo)" {
                return vec![dim("nenhum arquivo")];
            }
            vec![dim(plural(out.lines().count(), "arquivo", "arquivos"))]
        }
        "edit_file" | "write_file" => {
            let msg = first.strip_prefix("ok — ").unwrap_or(first);
            vec![dim(cut(msg))]
        }
        "bash" => bash_lines(out, room),
        "git_status" => body_lines(out.lines().collect(), room),
        "git_diff" => {
            if out.trim() == "(sem saída)" {
                return vec![dim("sem mudanças")];
            }
            let add = out.lines().filter(|l| l.starts_with('+') && !l.starts_with("+++")).count();
            let del = out.lines().filter(|l| l.starts_with('-') && !l.starts_with("---")).count();
            vec![dim(format!("{} (+{add} −{del})", plural(out.lines().count(), "linha de diff", "linhas de diff")))]
        }
        "git_commit" => body_lines(out.lines().take(2).collect(), room),
        _ => vec![dim(cut(first))],
    }
}

/// Up to [`MAX_BODY`] lines, then "… +N linhas".
fn body_lines(lines: Vec<&str>, room: usize) -> Vec<Line> {
    let lines: Vec<&str> = lines.into_iter().filter(|l| l.trim() != "[stderr]").collect();
    let mut out: Vec<Line> = lines.iter().take(MAX_BODY).map(|l| dim(term::truncate_cols(l, room))).collect();
    if lines.len() > MAX_BODY {
        out.push(dim(format!("… +{}", plural(lines.len() - MAX_BODY, "linha", "linhas"))));
    }
    if out.is_empty() {
        out.push(dim("(sem saída)"));
    }
    out
}

/// `bash` results end in `[exit N]`; a timeout carries an `ERRO: timeout` line instead.
fn bash_lines(out: &str, room: usize) -> Vec<Line> {
    let mut lines: Vec<&str> = out.lines().collect();
    let exit = lines
        .last()
        .and_then(|l| l.strip_prefix("[exit "))
        .and_then(|r| r.strip_suffix(']'))
        .and_then(|n| n.parse::<i32>().ok());
    if exit.is_some() {
        lines.pop();
    }
    let timeout = lines.iter().find(|l| l.starts_with("ERRO: timeout")).map(|l| l.to_string());
    lines.retain(|l| !l.starts_with("ERRO: timeout"));
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    let mut shown = body_lines(lines, room);
    if let Some(t) = timeout {
        // The advice after the first sentence is for the model, not for the person.
        let head = t.split(" — ").next().unwrap_or(&t);
        shown.push(err(term::truncate_cols(head, room)));
    } else if let Some(code) = exit.filter(|c| *c != 0) {
        shown.push(err(format!("exit {code}")));
    }
    shown
}

// ── Numbers ─────────────────────────────────────────────────────────────────

/// "14s", "1m 05s".
pub fn secs(s: u64) -> String {
    if s < 60 {
        format!("{s}s")
    } else {
        format!("{}m {:02}s", s / 60, s % 60)
    }
}

/// 842 → "842", 3554 → "3,6k", 12000 → "12k", 1_260_000 → "1,3M" (decimal comma, pt-BR).
pub fn tokens(n: i64) -> String {
    let short = |v: f64, unit: &str| {
        let t = format!("{v:.1}");
        format!("{}{unit}", t.strip_suffix(".0").unwrap_or(&t).replace('.', ","))
    };
    if n < 1_000 {
        n.to_string()
    } else if n < 1_000_000 {
        short(n as f64 / 1_000.0, "k")
    } else {
        short(n as f64 / 1_000_000.0, "M")
    }
}

/// "US$ 0,0116", with "≈" when the provider did not report the cost.
pub fn cost(c: f64, estimated: bool) -> String {
    let sign = if estimated { "≈" } else { "" };
    if c <= 0.0 {
        return format!("{sign}US$ 0");
    }
    if c < 0.0001 {
        return format!("{sign}US$ <0,0001");
    }
    format!("{sign}US$ {}", format!("{c:.4}").replace('.', ","))
}

/// The line under a finished turn: `✻ 14s · 3,6k tokens · US$ 0,0116`.
pub fn footer(elapsed: Option<u64>, tokens_n: i64, cost_usd: f64, estimated: bool) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(e) = elapsed {
        parts.push(secs(e));
    }
    parts.push(format!("{} tokens", tokens(tokens_n)));
    parts.push(cost(cost_usd, estimated));
    format!("✻ {}", parts.join(" · "))
}

// ── Welcome box ─────────────────────────────────────────────────────────────

/// `/home/samir/x/proj` → `~/x/proj` when it sits under `home`.
pub fn tildify(path: &str, home: Option<&str>) -> String {
    match home.filter(|h| !h.is_empty()) {
        Some(h) if path == h => "~".to_string(),
        Some(h) if path.strip_prefix(h).is_some_and(|r| r.starts_with('/')) => {
            format!("~{}", &path[h.len()..])
        }
        _ => path.to_string(),
    }
}

/// The boxed header printed when the REPL opens. Rows are laid out by their visible width, so
/// styled text does not push the right border out of line.
pub fn welcome(version: &str, model: &str, server: &str, cwd: &str, branch: Option<&str>, max_width: usize) -> Vec<String> {
    let room = max_width.saturating_sub(6).max(30);
    let branch_txt = branch.map(|b| format!("  ({b})")).unwrap_or_default();
    let cwd_txt = term::truncate_cols(&format!("{cwd}{branch_txt}"), room);
    let rows = [
        format!("{} {} {}", term::accent("✻"), term::bold("Anna"), term::gray(version)),
        format!("  {} {}", term::truncate_cols(model, room.saturating_sub(server.chars().count() + 5)), term::gray(&format!("· {server}"))),
        format!("  {}", term::gray(&cwd_txt)),
    ];
    let inner = rows.iter().map(|r| term::visible_width(r)).max().unwrap_or(0).max(40);
    let bar = "─".repeat(inner + 2);
    let mut out = vec![term::gray(&format!("╭{bar}╮"))];
    for r in &rows {
        let pad = " ".repeat(inner - term::visible_width(r));
        out.push(format!("{} {r}{pad} {}", term::gray("│"), term::gray("│")));
    }
    out.push(term::gray(&format!("╰{bar}╯")));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn call(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall { id: "c1".into(), name: name.into(), arguments: args }
    }

    fn texts(l: &[Line]) -> Vec<String> {
        l.iter().map(|x| x.text.clone()).collect()
    }

    #[test]
    fn the_title_names_the_action_and_its_object() {
        assert_eq!(call_title(&call("read_file", json!({"path":"src/a.rs"}))), ("Read".into(), "src/a.rs".into()));
        assert_eq!(call_title(&call("edit_file", json!({"path":"x"}))).0, "Update");
        assert_eq!(call_title(&call("write_file", json!({"path":"x"}))).0, "Write");
        assert_eq!(
            call_title(&call("grep", json!({"pattern":"fn main","path":"src","glob":"*.rs"}))),
            ("Grep".into(), "fn main em src (*.rs)".into())
        );
        assert_eq!(call_title(&call("git_diff", json!({"staged":true}))).1, "diff --staged");
        assert_eq!(call_title(&call("list_dir", json!({}))).1, ".");
    }

    #[test]
    fn a_bash_title_is_the_first_line_of_the_command() {
        let t = call_title(&call("bash", json!({"command":"cargo test\n  --all"})));
        assert_eq!(t, ("Bash".into(), "cargo test".into()));
    }

    #[test]
    fn an_unknown_tool_still_gets_a_title() {
        assert_eq!(call_title(&call("nova", json!({"a":1}))), ("nova".into(), "{\"a\":1}".into()));
        assert_eq!(call_title(&call("nova", json!({}))), ("nova".into(), String::new()));
    }

    #[test]
    fn a_read_is_summarised_by_its_length_not_shown() {
        let l = result_lines("read_file", "a\nb\nc", 80);
        assert_eq!(texts(&l), ["3 linhas"]);
        let t = result_lines("read_file", "a\nb\n… [truncado: 900 bytes no total, mostrando 4]", 80);
        assert_eq!(texts(&t), ["2 linhas (truncado)"]);
        assert_eq!(texts(&result_lines("read_file", "só uma", 80)), ["1 linha"]);
    }

    #[test]
    fn search_and_listing_results_are_counted() {
        assert_eq!(texts(&result_lines("grep", "(nenhum resultado)", 80)), ["nenhum resultado"]);
        assert_eq!(texts(&result_lines("grep", "a:1: x\nb:2: y", 80)), ["2 resultados"]);
        assert_eq!(
            texts(&result_lines("grep", "a:1: x\n… [cortado em 200 resultados]", 80)),
            ["1 resultado (cortado)"]
        );
        assert_eq!(texts(&result_lines("glob", "a.rs\nb.rs", 80)), ["2 arquivos"]);
        assert_eq!(texts(&result_lines("list_dir", "(vazio)", 80)), ["vazio"]);
        assert_eq!(texts(&result_lines("list_dir", "a\nb/", 80)), ["2 itens"]);
    }

    #[test]
    fn an_error_is_red_and_short() {
        let l = result_lines("read_file", "ERRO: 'x' inexistente ou fora do projeto.", 80);
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].tone, Tone::Error);
        let r = result_lines("edit_file", "RECUSADO: edição cancelada pelo usuário.", 80);
        assert_eq!(r[0].tone, Tone::Error);
    }

    #[test]
    fn a_successful_write_shows_the_tools_own_message() {
        let l = result_lines("edit_file", "ok — a.txt editado (1 ocorrência(s)).", 80);
        assert_eq!(texts(&l), ["a.txt editado (1 ocorrência(s))."]);
        assert_eq!(l[0].tone, Tone::Dim);
    }

    #[test]
    fn bash_shows_a_few_lines_and_counts_the_rest() {
        let out = "l1\nl2\nl3\nl4\nl5\nl6\n[exit 0]";
        let l = result_lines("bash", out, 80);
        assert_eq!(texts(&l), ["l1", "l2", "l3", "l4", "… +2 linhas"]);
        assert!(l.iter().all(|x| x.tone == Tone::Dim), "exit 0 is not an error");
    }

    #[test]
    fn a_failing_command_shows_its_exit_code_in_red() {
        let l = result_lines("bash", "boom\n[exit 2]", 80);
        assert_eq!(texts(&l), ["boom", "exit 2"]);
        assert_eq!(l[1].tone, Tone::Error);
    }

    #[test]
    fn a_command_with_no_output_says_so() {
        assert_eq!(texts(&result_lines("bash", "\n[exit 0]", 80)), ["(sem saída)"]);
    }

    #[test]
    fn a_timeout_keeps_the_sentence_and_drops_the_advice_for_the_model() {
        let out = "parcial\nERRO: timeout de 30s — o comando foi encerrado (grupo de processos morto).\n   rode em background";
        let l = result_lines("bash", out, 120);
        assert_eq!(l[0].text, "parcial");
        let last = l.last().unwrap();
        assert_eq!(last.tone, Tone::Error);
        assert_eq!(last.text, "ERRO: timeout de 30s");
    }

    #[test]
    fn stderr_marker_is_not_shown() {
        let l = result_lines("bash", "ok\n[stderr]\naviso\n[exit 0]", 80);
        assert_eq!(texts(&l), ["ok", "aviso"]);
    }

    #[test]
    fn a_diff_is_counted() {
        let out = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-velho\n+novo\n+outro";
        assert_eq!(texts(&result_lines("git_diff", out, 80)), ["7 linhas de diff (+2 −1)"]);
        assert_eq!(texts(&result_lines("git_diff", "(sem saída)", 80)), ["sem mudanças"]);
    }

    #[test]
    fn numbers_use_the_decimal_comma() {
        assert_eq!(tokens(842), "842");
        assert_eq!(tokens(3554), "3,6k");
        assert_eq!(tokens(12_000), "12k");
        assert_eq!(tokens(1_260_000), "1,3M");
        assert_eq!(cost(0.0116, false), "US$ 0,0116");
        assert_eq!(cost(0.0116, true), "≈US$ 0,0116");
        assert_eq!(cost(0.0, false), "US$ 0");
        assert_eq!(cost(0.00001, false), "US$ <0,0001");
        assert_eq!(secs(14), "14s");
        assert_eq!(secs(65), "1m 05s");
    }

    #[test]
    fn the_footer_joins_time_tokens_and_cost() {
        assert_eq!(footer(Some(14), 3554, 0.0116, false), "✻ 14s · 3,6k tokens · US$ 0,0116");
        assert_eq!(footer(None, 320, 0.0007, true), "✻ 320 tokens · ≈US$ 0,0007");
    }

    #[test]
    fn home_becomes_a_tilde_only_on_a_path_boundary() {
        assert_eq!(tildify("/home/samir/x/p", Some("/home/samir")), "~/x/p");
        assert_eq!(tildify("/home/samir", Some("/home/samir")), "~");
        assert_eq!(tildify("/home/samirb/x", Some("/home/samir")), "/home/samirb/x", "prefix is not a parent");
        assert_eq!(tildify("/srv/x", Some("/home/samir")), "/srv/x");
        assert_eq!(tildify("/srv/x", None), "/srv/x");
    }

    #[test]
    fn every_row_of_the_welcome_box_has_the_same_width() {
        // Colour is off under `cargo test` (stdout is not a terminal), which is also the
        // strictest case for alignment: the painted case only adds zero-width escapes.
        let b = welcome("0.12.4", "deepseek/deepseek-v3.2", "ShvIA", "~/x/SHVIA/SHVIA-CODE", Some("master"), 100);
        assert_eq!(b.len(), 5);
        let w: Vec<usize> = b.iter().map(|l| term::visible_width(l)).collect();
        assert!(w.iter().all(|x| *x == w[0]), "rows differ: {w:?}");
        assert!(b[0].starts_with('╭') && b[4].starts_with('╰'));
        assert!(b[3].contains("(master)"));
    }

    #[test]
    fn a_long_path_is_cut_to_fit_the_box() {
        let long = format!("~/{}", "pasta/".repeat(40));
        let b = welcome("0.12.4", "m", "ShvIA", &long, None, 60);
        let w: Vec<usize> = b.iter().map(|l| term::visible_width(l)).collect();
        assert!(w.iter().all(|x| *x == w[0] && *x <= 60), "rows: {w:?}");
    }
}
