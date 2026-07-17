//! Confinamento e denylist (SECURITY §2.3). As tools de arquivo só enxergam a
//! raiz do projeto; caminhos que escapam ou que caem na denylist de segredos
//! são recusados — nem o modelo escolhendo path malicioso passa.

use std::path::{Path, PathBuf};

/// Resolve `rel` (relativo à `root`) para um caminho absoluto CANÔNICO, exigindo
/// que ele permaneça dentro de `root`. `None` = escapou ou não existe.
pub fn resolve(root: &Path, rel: &str) -> Option<PathBuf> {
    let rel = rel.trim();
    let rel = if rel == "." || rel.is_empty() { "" } else { rel.trim_start_matches('/') };
    let joined = root.join(rel);
    let full = joined.canonicalize().ok()?;
    let root_c = root.canonicalize().ok()?;
    full.starts_with(&root_c).then_some(full)
}

/// Como `resolve`, mas para um arquivo que PODE não existir ainda (write_file):
/// exige que o **diretório-pai** exista e esteja dentro da raiz. `None` = pai
/// inexistente ou fora do projeto.
pub fn resolve_new(root: &Path, rel: &str) -> Option<PathBuf> {
    let rel = rel.trim().trim_start_matches('/');
    let joined = root.join(rel);
    let name = joined.file_name()?;
    let parent = joined.parent()?;
    let parent_c = parent.canonicalize().ok()?;
    let root_c = root.canonicalize().ok()?;
    parent_c.starts_with(&root_c).then(|| parent_c.join(name))
}

/// Caminho na denylist de segredos? Vale mesmo com override — o override é
/// decisão do dev na hora (SECURITY §2.3), nunca do modelo.
pub fn denied(rel: &str) -> bool {
    // Case-insensitive: em FS case-insensitive (macOS/Windows) `.ENV`/`KEY.PEM`
    // apontam pro MESMO arquivo que `.env`/`key.pem` — comparar em minúsculas.
    let base_raw = Path::new(rel).file_name().and_then(|s| s.to_str()).unwrap_or(rel);
    let base = base_raw.to_lowercase();
    let low = rel.to_lowercase().replace('\\', "/");
    // Nunca escrever em .git/ — um hook (.git/hooks/*) ou .git/config vira execução
    // de código arbitrária na máquina do dev quando o git roda.
    if base == ".git" || low == ".git" || low.starts_with(".git/") || low.contains("/.git/") || low.ends_with("/.git") {
        return true;
    }
    base == ".env"
        // .env.example é template público — não é segredo.
        || (base.starts_with(".env.") && base != ".env.example")
        || matches!(
            Path::new(base.as_str()).extension().and_then(|s| s.to_str()),
            Some("pem") | Some("key") | Some("p8") | Some("p12") | Some("pfx")
        )
        || base.starts_with("id_rsa")
        || low.contains("credentials")
        || low.contains("/.ssh/")
        || low.contains("/.aws/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denylist_pega_segredos() {
        for p in [".env", ".env.local", "certs/server.pem", "deploy/id_rsa", "aws_credentials.txt"] {
            assert!(denied(p), "deveria recusar: {p}");
        }
        for p in ["src/main.rs", "README.md", ".env.example", "config.toml"] {
            assert!(!denied(p), "não deveria recusar: {p}");
        }
    }

    #[test]
    fn resolve_confina_na_raiz() {
        let tmp = std::env::temp_dir().join("anna-guard-test");
        let _ = std::fs::create_dir_all(tmp.join("sub"));
        std::fs::write(tmp.join("sub/a.txt"), b"x").unwrap();
        assert!(resolve(&tmp, "sub/a.txt").is_some());
        assert!(resolve(&tmp, "../..").is_none(), "não pode escapar da raiz");
        assert!(resolve(&tmp, "inexistente").is_none());
    }
}
