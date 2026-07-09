//! Sessões locais (docs/arquitetura-cli.md §5). `~/.shvia-code/sessions/<id>.json`
//! com permissão 0600 (SECURITY §2.5): salvar/listar/retomar/limpar. Nunca sobem
//! ao servidor (D7). `id` = epoch em segundos; `title` = 1ª pergunta do usuário.

use crate::types::Message;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub messages: Vec<Message>,
}

impl Session {
    pub fn new() -> Self {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_else(|_| "0".into());
        Self { id, title: String::new(), messages: vec![] }
    }

    pub fn set_title_from(&mut self, first_user: &str) {
        if self.title.is_empty() && !first_user.trim().is_empty() {
            self.title = first_user.chars().take(56).collect::<String>().replace('\n', " ");
        }
    }
}

pub struct Store {
    dir: PathBuf,
}

impl Store {
    /// Abre (criando) `~/.shvia-code/sessions/`. `None` se não der pra resolver a home.
    pub fn open() -> Option<Self> {
        let home = directories::BaseDirs::new()?.home_dir().to_path_buf();
        let dir = home.join(".shvia-code").join("sessions");
        std::fs::create_dir_all(&dir).ok()?;
        set_perms(&dir, 0o700);
        Some(Self { dir })
    }

    pub fn save(&self, s: &Session) {
        let path = self.dir.join(format!("{}.json", s.id));
        if let Ok(json) = serde_json::to_string_pretty(s) {
            if std::fs::write(&path, json).is_ok() {
                set_perms(&path, 0o600);
            }
        }
    }

    /// (id, title) das sessões, mais recente primeiro.
    pub fn list(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&self.dir) {
            for e in rd.flatten() {
                if e.path().extension().and_then(|x| x.to_str()) == Some("json") {
                    if let Ok(txt) = std::fs::read_to_string(e.path()) {
                        if let Ok(s) = serde_json::from_str::<Session>(&txt) {
                            out.push((s.id, s.title));
                        }
                    }
                }
            }
        }
        out.sort_by(|a, b| b.0.cmp(&a.0));
        out
    }

    pub fn load(&self, id: &str) -> Option<Session> {
        let txt = std::fs::read_to_string(self.dir.join(format!("{id}.json"))).ok()?;
        serde_json::from_str(&txt).ok()
    }

    pub fn clear(&self) -> usize {
        let mut n = 0;
        if let Ok(rd) = std::fs::read_dir(&self.dir) {
            for e in rd.flatten() {
                if e.path().extension().and_then(|x| x.to_str()) == Some("json")
                    && std::fs::remove_file(e.path()).is_ok()
                {
                    n += 1;
                }
            }
        }
        n
    }
}

#[cfg(unix)]
fn set_perms(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
}
#[cfg(not(unix))]
fn set_perms(_: &Path, _: u32) {}
