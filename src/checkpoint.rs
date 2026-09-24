//! Checkpoint automático de segurança: antes de toda mutação aprovada
//! (edit_file/write_file e bash destrutivo), fotografa o working tree num
//! commit FORA da branch — índice TEMPORÁRIO (`GIT_INDEX_FILE`), `write-tree`
//! e `commit-tree` — guardado em `refs/anna/checkpoint`. Nada toca o índice
//! real nem a working tree: `git status` do dev continua idêntico.
//!
//! Rede de segurança, não feature de fluxo: QUALQUER falha (sem git, repo
//! vazio, git antigo) é silenciosa — jamais bloqueia a edição. O restore é
//! ação do DEV no REPL (`/rollback`), nunca uma tool do modelo.
//!
//! Limite documentado: o restore devolve o CONTEÚDO rastreado da foto;
//! arquivos criados DEPOIS dela permanecem no disco (remoção é manual).

use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub const REF: &str = "refs/anna/checkpoint";

/// Edições em rajada (mesmo turno agêntico) compartilham uma foto só —
/// `git add -A` num repo grande não é grátis.
const DEBOUNCE_SECS: i64 = 30;

static LAST_SNAPSHOT: AtomicI64 = AtomicI64::new(0);

fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn git(root: &Path, envs: &[(&str, String)], args: &[&str]) -> Option<String> {
    let mut c = Command::new("git");
    // `core.autocrlf=false`: the photo and the rollback must be byte-exact. With the
    // user's autocrlf=true (the Windows default), `restore` converted LF to CRLF on the way
    // back, so a rollback rewrote the line endings of files the dev never touched.
    // Repository .gitattributes still apply, and they round-trip on their own.
    c.current_dir(root).args(["-c", "core.autocrlf=false"]).args(args);
    for (k, v) in envs {
        c.env(k, v);
    }
    let out = c.output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn is_git_repo(root: &Path) -> bool {
    git(root, &[], &["rev-parse", "--git-dir"]).is_some()
}

/// Fotografa o estado atual ANTES de uma escrita. Retorna o sha (ou None,
/// silencioso). O `label` vai na mensagem do commit-objeto — útil no
/// `git log refs/anna/checkpoint` pra saber o que disparou a foto.
pub fn snapshot(root: &Path, label: &str) -> Option<String> {
    if !is_git_repo(root) {
        return None;
    }
    let now = now_secs();
    let last = LAST_SNAPSHOT.load(Ordering::Relaxed);
    if now - last < DEBOUNCE_SECS {
        return None; // foto recente cobre esta rajada
    }

    // Índice temporário: o add -A acontece NELE — índice real intocado.
    let tmp = std::env::temp_dir().join(format!("anna-ckpt-index-{}", std::process::id()));
    let envs: Vec<(&str, String)> = vec![("GIT_INDEX_FILE", tmp.to_string_lossy().into_owned())];

    git(root, &envs, &["add", "-A"])?;
    let tree = git(root, &envs, &["write-tree"])?;
    let _ = std::fs::remove_file(&tmp);

    // Parent = HEAD quando existe (repo com histórico); repo virgem = sem parent.
    let msg = format!("anna checkpoint: {label}");
    let sha = match git(root, &[], &["rev-parse", "--verify", "--quiet", "HEAD"]) {
        Some(head) => git(root, &[], &["commit-tree", &tree, "-p", &head, "-m", &msg])?,
        None => git(root, &[], &["commit-tree", &tree, "-m", &msg])?,
    };
    git(root, &[], &["update-ref", REF, &sha])?;
    LAST_SNAPSHOT.store(now, Ordering::Relaxed);
    Some(sha)
}

/// Sha do último checkpoint, se houver.
pub fn last(root: &Path) -> Option<String> {
    git(root, &[], &["rev-parse", "--verify", "--quiet", REF])
}

/// Resumo (diff --stat) do que mudou DESDE a foto — o que o /rollback desfaria.
pub fn diff_stat(root: &Path) -> Option<String> {
    let sha = last(root)?;
    git(root, &[], &["diff", "--stat", &sha])
}

/// Restaura o conteúdo rastreado da foto na working tree (índice intocado).
/// `git restore --source` (git ≥ 2.23), com fallback pro checkout pathspec.
pub fn restore(root: &Path) -> Result<String, String> {
    let Some(sha) = last(root) else {
        return Err("sem checkpoint ainda — a foto é tirada antes da primeira edição aprovada.".into());
    };
    if git(root, &[], &["restore", "--source", &sha, "--worktree", "--", "."]).is_none()
        && git(root, &[], &["checkout", &sha, "--", "."]).is_none()
    {
        return Err("git restore/checkout falharam — restaure manualmente (git log refs/anna/checkpoint).".into());
    }
    Ok(sha)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `LAST_SNAPSHOT` is a process-wide debounce (30 s), so two tests that snapshot in
    /// parallel would make the second one get `None`. They take this lock and reset it.
    static FOTO: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn git_sandbox(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("anna-ckpt-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let run = |args: &[&str]| {
            let o = Command::new("git").current_dir(&dir).args(args).output().unwrap();
            assert!(o.status.success(), "git {args:?} falhou");
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "t@t"]);
        run(&["config", "user.name", "t"]);
        std::fs::write(dir.join("a.txt"), "original\n").unwrap();
        run(&["add", "-A"]);
        run(&["commit", "-qm", "init"]);
        dir
    }

    #[test]
    fn snapshot_e_restore_devolvem_o_estado_da_foto() {
        let _foto = FOTO.lock().unwrap_or_else(|e| e.into_inner());
        LAST_SNAPSHOT.store(0, Ordering::Relaxed);
        let dir = git_sandbox("roundtrip");
        std::fs::write(dir.join("a.txt"), "estado na foto\n").unwrap();
        let sha = snapshot(&dir, "teste").expect("snapshot deveria fotografar");
        assert!(!sha.is_empty());
        std::fs::write(dir.join("a.txt"), "estragado depois\n").unwrap();
        restore(&dir).expect("restore");
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "estado na foto\n");
        // Branch/índice intocados: HEAD segue no commit "init".
        let head = git(&dir, &[], &["log", "-1", "--pretty=%s"]).unwrap();
        assert_eq!(head, "init");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn o_restore_devolve_os_bytes_exatos_mesmo_com_autocrlf() {
        // Windows runners (and many Windows installs) have core.autocrlf=true, and with it
        // the restore wrote "estado na foto\r\n" back over a file that was LF in the photo:
        // a rollback that changes the user's line endings. autocrlf=true converts on
        // checkout on every OS, so this reproduces here what the Windows CI measured.
        let _foto = FOTO.lock().unwrap_or_else(|e| e.into_inner());
        let dir = git_sandbox("autocrlf");
        let o = Command::new("git")
            .current_dir(&dir)
            .args(["config", "core.autocrlf", "true"])
            .output()
            .unwrap();
        assert!(o.status.success());
        std::fs::write(dir.join("a.txt"), "estado na foto\n").unwrap();
        LAST_SNAPSHOT.store(0, Ordering::Relaxed);
        snapshot(&dir, "teste").expect("snapshot deveria fotografar");
        std::fs::write(dir.join("a.txt"), "estragado depois\n").unwrap();
        restore(&dir).expect("restore");
        assert_eq!(std::fs::read(dir.join("a.txt")).unwrap(), b"estado na foto\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fora_de_repo_git_e_silencioso() {
        let dir = std::env::temp_dir().join(format!("anna-ckpt-nogit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(snapshot(&dir, "x").is_none());
        assert!(restore(&dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
