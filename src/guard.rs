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
///
/// A name that already exists may be a symlink, and a write follows it. Until 27/10/2026 only the
/// parent was checked, so `notes.md -> ../outside.txt` passed and the approved write landed outside
/// the project (external review, 26/10/2026). An existing name is now resolved through every link
/// and must still land inside the root; a dangling link is refused, since there is nothing to check.
pub fn resolve_new(root: &Path, rel: &str) -> Option<PathBuf> {
    let rel = rel.trim().trim_start_matches('/');
    let joined = root.join(rel);
    let name = joined.file_name()?;
    let parent = joined.parent()?;
    let parent_c = parent.canonicalize().ok()?;
    let root_c = root.canonicalize().ok()?;
    if !parent_c.starts_with(&root_c) {
        return None;
    }
    let alvo = parent_c.join(name);
    if std::fs::symlink_metadata(&alvo).is_ok() {
        let real = alvo.canonicalize().ok()?;
        return real.starts_with(&root_c).then_some(real);
    }
    Some(alvo)
}

/// The file `full` really is, relative to the root, after every symlink — `None` when it is the
/// same path that was asked for. The approval card shows it, so "Write(notes.md)" never hides
/// which file is written.
pub fn link_target(root: &Path, rel: &str, full: &Path) -> Option<String> {
    let root_c = root.canonicalize().ok()?;
    let pedido = root.join(rel.trim().trim_start_matches('/'));
    let link = std::fs::symlink_metadata(&pedido).ok()?.file_type().is_symlink();
    link.then(|| rel_real(&root_c, full))
}

/// Is the file `full` points to — after every symlink — on the denylist?
///
/// `denied` sees only the string the model asked for. Through a link inside the project that is
/// not enough: `readme.txt -> .env` was read with no approval, and `hook.txt ->
/// .git/hooks/pre-commit` was rewritten (external review, 26/10/2026). Every file tool checks both.
pub fn denied_target(root: &Path, full: &Path) -> bool {
    let root_c = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let real = full.canonicalize().unwrap_or_else(|_| full.to_path_buf());
    denied(&rel_real(&root_c, &real))
}

fn rel_real(root_c: &Path, real: &Path) -> String {
    real.strip_prefix(root_c).unwrap_or(real).to_string_lossy().replace('\\', "/")
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
