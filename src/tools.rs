//! Ferramentas locais v1 — leitura (docs/ferramentas.md). Registry: nome →
//! (schema JSON enviado ao modelo) + executor local. Tudo confinado à raiz do
//! projeto (guard) e respeitando `.gitignore` (ignore). No C1 todas são `Auto`;
//! escrita/bash/git entram no C2.

use crate::gate::{Gate, Policy};
use crate::guard;
use crate::types::{ToolCall, ToolDef};
use globset::Glob;
use ignore::WalkBuilder;
use serde_json::json;
use std::path::Path;

const MAX_BYTES: usize = 64 * 1024;
const MAX_HITS: usize = 200;

/// Defs enviadas ao modelo (`tools[]` do protocolo).
pub fn defs() -> Vec<ToolDef> {
    let obj = |props: serde_json::Value, req: serde_json::Value| {
        json!({ "type": "object", "properties": props, "required": req })
    };
    vec![
        ToolDef {
            name: "read_file".into(),
            description: "Lê um arquivo de texto do projeto.".into(),
            parameters: obj(
                json!({
                    "path": {"type":"string","description":"Caminho relativo à raiz do projeto."},
                    "max_bytes": {"type":"integer","description":"Teto opcional (default 65536); acima disso recorta."}
                }),
                json!(["path"]),
            ),
        },
        ToolDef {
            name: "list_dir".into(),
            description: "Lista arquivos e pastas de um diretório (respeita .gitignore).".into(),
            parameters: obj(
                json!({"path": {"type":"string","description":"Relativo à raiz ('.' = raiz)."}}),
                json!(["path"]),
            ),
        },
        ToolDef {
            name: "grep".into(),
            description: "Busca um regex nos arquivos do projeto.".into(),
            parameters: obj(
                json!({
                    "pattern": {"type":"string","description":"Regex (sintaxe Rust)."},
                    "path": {"type":"string","description":"Subárvore (default: raiz)."},
                    "glob": {"type":"string","description":"Filtro de arquivo, ex.: '*.rs'."}
                }),
                json!(["pattern"]),
            ),
        },
        ToolDef {
            name: "glob".into(),
            description: "Lista arquivos por padrão glob (ex.: src/**/*.rs).".into(),
            parameters: obj(
                json!({"pattern": {"type":"string"}}),
                json!(["pattern"]),
            ),
        },
    ]
}

/// Executa um tool call localmente e devolve o `content` da mensagem role=tool.
/// Passa pelo gate: no C1 todas as tools são read-only (`Auto`, roda direto); o
/// C2 mapeia edit/write→`Confirm` e bash destrutivo→`Always` aqui.
pub fn run(call: &ToolCall, root: &Path, gate: &mut Gate) -> String {
    if !gate.approve(Policy::Auto, &call.name) {
        return "RECUSADO: cancelado pelo usuário.".into();
    }
    let a = &call.arguments;
    let str_arg = |k: &str| a.get(k).and_then(|v| v.as_str()).unwrap_or_default().to_string();
    match call.name.as_str() {
        "read_file" => {
            let bytes = a.get("max_bytes").and_then(|v| v.as_u64()).map(|n| n as usize).unwrap_or(MAX_BYTES);
            read_file(root, &str_arg("path"), bytes)
        }
        "list_dir" => list_dir(root, &str_arg("path")),
        "grep" => grep(root, &str_arg("pattern"), &str_arg("path"), &str_arg("glob")),
        "glob" => glob_files(root, &str_arg("pattern")),
        other => format!("ERRO: ferramenta desconhecida '{other}'."),
    }
}

fn rel_of(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

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
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                format!("{name}/")
            } else {
                name
            }
        })
        .collect();
    entries.sort();
    if entries.is_empty() {
        "(vazio)".into()
    } else {
        entries.join("\n")
    }
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
    let base = if path.is_empty() { root.to_path_buf() } else {
        match guard::resolve(root, path) {
            Some(p) => p,
            None => return format!("ERRO: '{path}' inexistente ou fora do projeto."),
        }
    };

    let mut hits: Vec<String> = Vec::new();
    for entry in WalkBuilder::new(&base).build().filter_map(Result::ok) {
        if hits.len() >= MAX_HITS {
            break;
        }
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
                    break;
                }
            }
        }
    }
    if hits.is_empty() {
        "(nenhum resultado)".into()
    } else {
        hits.join("\n")
    }
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
    if found.is_empty() {
        "(nenhum arquivo)".into()
    } else {
        found.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("anna-tools-test");
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
}
