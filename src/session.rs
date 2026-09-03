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
    /// Tokens acumulados da sessão (do `usage`; 0 nas sessões antigas ao
    /// dashboard — nunca foram registrados). Alimenta o `anna stats`.
    #[serde(default)]
    pub tokens: i64,
    /// Custo acumulado (US$; real ou estimado). 0 quando desconhecido.
    #[serde(default)]
    pub cost: f64,
    /// Modelo/perfil efetivo da sessão (o `model` reportado pelo gateway, ou o
    /// perfil escolhido). `None` nas sessões antigas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

impl Session {
    pub fn new() -> Self {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_else(|_| "0".into());
        Self { id, title: String::new(), messages: vec![], tokens: 0, cost: 0.0, model: None }
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

    /// Todas as sessões, na íntegra (para o `anna stats`). Ignora arquivos
    /// ilegíveis/corrompidos em silêncio — o dashboard nunca quebra por um
    /// arquivo ruim.
    pub fn all(&self) -> Vec<Session> {
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&self.dir) {
            for e in rd.flatten() {
                if e.path().extension().and_then(|x| x.to_str()) == Some("json") {
                    if let Ok(txt) = std::fs::read_to_string(e.path()) {
                        if let Ok(s) = serde_json::from_str::<Session>(&txt) {
                            out.push(s);
                        }
                    }
                }
            }
        }
        out
    }

    /// Apaga as sessões mais antigas que `dias`, pela data de MODIFICAÇÃO do arquivo.
    ///
    /// ## Por que existe (achado F-07 da revisão de 01/09/2026)
    ///
    /// A sessão guarda o histórico inteiro — inclusive `role=tool`, ou seja, tudo que o
    /// `read_file` leu, e a última imagem em base64. Fica em `0600`, mas **para sempre**: a
    /// única limpeza era `anna sessions clear`, tudo ou nada. Quem quer manter o trabalho
    /// desta semana e não quer o `.env` que foi lido há três meses não tinha o que fazer.
    ///
    /// ## Por que `mtime`, e não um campo de data
    ///
    /// `Session` não tem carimbo, e acrescentar um só valeria para as sessões NOVAS —
    /// justamente as que não interessam a uma limpeza por idade. O `mtime` do arquivo
    /// existe para todas, inclusive as antigas, e responde à pergunta certa: "quando foi a
    /// última vez que mexi nisto?".
    ///
    /// Arquivo sem `mtime` legível é **preservado**. Numa operação que apaga, a dúvida vota
    /// em não apagar.
    pub fn clear_older_than(&self, dias: u64) -> usize {
        // `checked_mul` ANTES do `checked_sub`: `dias * 24 * 3600` estoura sozinho para
        // um `dias` grande e derrubava o processo com "attempt to multiply with overflow"
        // — encontrado por teste. Numa operação que apaga, estouro não pode virar pânico
        // nem, pior, um corte no futuro que apagaria tudo.
        let corte = match dias
            .checked_mul(24 * 3600)
            .map(std::time::Duration::from_secs)
            .and_then(|d| std::time::SystemTime::now().checked_sub(d))
        {
            Some(c) => c,
            None => return 0, // `dias` absurdo: não apaga nada
        };
        let mut n = 0;
        if let Ok(rd) = std::fs::read_dir(&self.dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) != Some("json") {
                    continue;
                }
                let Ok(modificado) = e.metadata().and_then(|m| m.modified()) else {
                    continue; // sem data confiável: preserva
                };
                if modificado < corte && std::fs::remove_file(&p).is_ok() {
                    n += 1;
                }
            }
        }
        n
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

#[cfg(test)]
mod tests_retencao {
    use super::*;

    /// Achado F-07: a sessão guarda o histórico inteiro — inclusive tudo que o
    /// `read_file` leu — em 0600, mas para sempre. A única limpeza era tudo ou nada.
    fn store_temp() -> Store {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let id = N.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("anna-sess-{}-{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Store { dir }
    }

    /// Cria um arquivo de sessão com `mtime` de `dias_atras`.
    fn sessao_com_idade(st: &Store, id: &str, dias_atras: u64) {
        let p = st.dir.join(format!("{id}.json"));
        // O `list()` lê o id de DENTRO do json, não do nome do arquivo — o fixture tem
        // de escrever o id certo, senão o teste compara nomes que ele mesmo inventou.
        std::fs::write(&p, format!(r#"{{"id":"{id}","title":"t","messages":[]}}"#)).unwrap();
        let quando = std::time::SystemTime::now() - std::time::Duration::from_secs(dias_atras * 24 * 3600);
        // `set_times` é estável desde 1.75 e evita depender do `touch` do sistema.
        let f = std::fs::File::options().write(true).open(&p).unwrap();
        f.set_times(std::fs::FileTimes::new().set_modified(quando)).unwrap();
    }

    #[test]
    fn apaga_so_o_que_passou_do_corte() {
        let st = store_temp();
        sessao_com_idade(&st, "antiga", 100);
        sessao_com_idade(&st, "media", 40);
        sessao_com_idade(&st, "recente", 2);

        assert_eq!(st.clear_older_than(30), 2, "deveria apagar as de 100 e 40 dias");
        let restantes: Vec<_> = st.list().into_iter().map(|(id, _)| id).collect();
        assert_eq!(restantes, vec!["recente".to_string()]);
    }

    #[test]
    fn corte_grande_nao_apaga_nada() {
        let st = store_temp();
        sessao_com_idade(&st, "a", 10);
        assert_eq!(st.clear_older_than(365), 0);
        assert_eq!(st.list().len(), 1);
    }

    #[test]
    fn corte_zero_apaga_tudo_mas_clear_continua_existindo() {
        let st = store_temp();
        sessao_com_idade(&st, "a", 1);
        sessao_com_idade(&st, "b", 0);
        assert_eq!(st.clear_older_than(0), 2);
    }

    #[test]
    fn nao_toca_no_que_nao_e_sessao() {
        let st = store_temp();
        sessao_com_idade(&st, "a", 100);
        let outro = st.dir.join("anotacao.txt");
        std::fs::write(&outro, "não é sessão").unwrap();
        assert_eq!(st.clear_older_than(30), 1);
        assert!(outro.exists(), "apagou um arquivo que não é sessão");
    }

    #[test]
    fn dias_absurdo_nao_apaga_tudo() {
        // `now - dias` estourando não pode virar "corte no futuro", que apagaria tudo.
        // Numa operação destrutiva, a dúvida vota em não apagar.
        let st = store_temp();
        sessao_com_idade(&st, "a", 1);
        assert_eq!(st.clear_older_than(u64::MAX), 0);
        assert_eq!(st.list().len(), 1);
    }
}
