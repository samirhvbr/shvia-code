//! Config resolvida (docs/arquitetura-cli.md §5).
//!
//! Precedência: flags/env > `~/.config/shvia-code/config.toml`.
//! **A API key NUNCA vem de argumento de linha** (aparece no `ps`) — só de
//! `SHVIA_API_KEY` ou do arquivo de config 0600 (SECURITY §2.4).

use crate::papel::{Elenco, Papel};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

/// Servidor usado quando não há `--url`, `SHVIA_URL` nem `url` no config.toml.
///
/// Domínio próprio desde 26/07/2026. Este valor é COMPILADO no binário, então um
/// `anna` antigo continua batendo no host velho até ser reinstalado — o legado
/// `ia.blue3.com.br` serve a MESMA instância (mesmo IP) justamente para essa
/// transição. Quem chama pela ponte do desktop (Modo Code) não depende daqui: o
/// SHVIA-WEB passa `url: location.origin` no spawn, então o agente sempre fala
/// com o servidor que serviu a página.
pub const DEFAULT_URL: &str = "https://ai.shvia.org";

/// Teto de voltas (idas ao modelo) por turno sem `max_voltas` no config nem
/// `SHVIA_MAX_VOLTAS` no ambiente. É a proteção A6 (SECURITY §2.7): o loop é
/// SEMPRE limitado — o valor só define onde o dev é chamado a decidir. Subiu de
/// 25 em 19/08/2026: tarefa legítima (testes+correção num projeto Godot) morreu
/// no meio com 26 ferramentas, e "refine a pergunta" era conselho errado.
pub const DEFAULT_MAX_VOLTAS: usize = 100;

#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    url: Option<String>,
    profile: Option<String>,
    api_key: Option<String>,
    effort: Option<String>,
    max_voltas: Option<usize>,
    elenco: Option<Elenco>,
    verificacao: Option<Vec<String>>,
    bash_timeout: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub url: String,
    pub profile: Option<String>,
    pub api_key: Option<String>,
    /// Esforço de raciocínio (low|medium|high|xhigh|max). O gateway valida.
    pub effort: Option<String>,
    /// Teto de voltas por turno (SECURITY §2.7). Estourou: terminal interativo
    /// pergunta se continua mais um lote; host NDJSON/`-p` encerram o turno
    /// avisando que "continua" retoma. `load()` garante ≥ 1.
    pub max_voltas: usize,
    /// Tabela papel → (perfil, esforço) do orquestrador. Vazia = todos os papéis
    /// no perfil global (o pipeline roda, mas avisa que não é blend nenhum).
    pub elenco: Elenco,
    /// Comandos do gate determinístico do orquestrador. `None` = detecta pela
    /// stack (Cargo.toml → cargo clippy/test etc.).
    pub verificacao: Option<Vec<String>>,
    /// Teto de tempo de cada `bash`, em segundos. `0` desliga (achado F-04).
    ///
    /// Existe como número, e não como booleano, porque o valor certo depende do
    /// projeto: uma suíte de integração legítima passa de 120 s, e um teto que só
    /// pode ser ligado ou desligado empurra quem tem suíte lenta a desligá-lo — que
    /// é o pior dos dois mundos.
    pub bash_timeout: u64,
}

/// Teto padrão do `bash`, em segundos. Espelha `tools::TIMEOUT_BASH_PADRAO_S`, e a
/// prova `padrao_de_timeout_bate_com_o_das_tools` reprova se os dois divergirem.
pub const DEFAULT_BASH_TIMEOUT_S: u64 = 120;

/// `true` se `url` puder receber a API key: https, ou http em LOOPBACK de verdade.
///
/// O host é comparado inteiro, nunca por prefixo. A versão anterior usava
/// `url.starts_with("http://localhost")`, e prefixo não é host:
/// `http://localhost.atacante.tld` passava pela exceção de dev e a chave saía em
/// CLARO para um domínio de terceiro (o `127.` era pior — `http://127.evil.com`).
pub fn url_seguro(url: &str) -> bool {
    let (esquema, resto) = match url.split_once("://") {
        Some(p) => p,
        None => return false,
    };
    if esquema == "https" {
        return true;
    }
    if esquema != "http" {
        return false;
    }
    // Autoridade = até o primeiro `/`, `?` ou `#`; tira userinfo (antes do `@`,
    // que é o truque clássico de `http://localhost@atacante.tld`) e a porta.
    let autoridade = resto.split(['/', '?', '#']).next().unwrap_or("");
    let host_porta = autoridade.rsplit_once('@').map_or(autoridade, |(_, h)| h);
    let host = match host_porta.strip_prefix('[') {
        // IPv6 literal: `[::1]:8000` → `::1`
        Some(r) => return matches!(r.split_once(']'), Some(("::1", _))),
        None => host_porta.split(':').next().unwrap_or(""),
    };
    // 127.x é validado como IPv4 DE VERDADE (127.0.0.0/8), não por prefixo de
    // texto: `starts_with("127.0.0.")` aceitaria `127.0.0.1.atacante.tld`, que é
    // um domínio comum. Este teste falhou de propósito antes de virar parse.
    host == "localhost"
        || host
            .parse::<std::net::Ipv4Addr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// Resolve o teto de voltas: env > arquivo > default, nunca abaixo de 1.
/// Env que não parseia (ex.: "abc") cai para o arquivo em vez de virar pânico —
/// mesmo espírito do `effort`, que ignora env vazio.
fn resolver_max_voltas(env: Option<String>, arquivo: Option<usize>) -> usize {
    env.and_then(|v| v.trim().parse::<usize>().ok())
        .or(arquivo)
        .unwrap_or(DEFAULT_MAX_VOLTAS)
        .max(1)
}


/// O `config.toml` guarda `api_key`. Ele está com permissão frouxa?
///
/// ## Por que existe (achado F-06 da revisão de 01/09/2026)
///
/// O README e o `docs/arquitetura-cli.md` prometiam `0600` e "keychain se der". Nenhum dos
/// dois existia: o `anna` não tinha comando que escrevesse o config, então o usuário o criava
/// à mão com o umask dele — tipicamente `0644`, legível por qualquer conta da máquina — e a
/// leitura não dizia nada. Uma promessa de permissão que ninguém cria nem confere é pior que
/// silêncio: quem leu a doc acredita que está protegido.
///
/// Só avisa quando há **chave dentro**. Config sem `api_key` em `0644` não é problema, e
/// avisar ali treinaria o dev a ignorar o aviso — que é como uma guarda morre.
///
/// Devolve `Some(modo)` quando está frouxo. Em não-Unix, sempre `None`: não há modo POSIX
/// para conferir, e inventar um aviso que não mede nada seria teatro.
#[cfg(unix)]
pub fn permissao_frouxa(caminho: &std::path::Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    let modo = std::fs::metadata(caminho).ok()?.permissions().mode() & 0o777;
    // Qualquer bit para grupo ou outros. `0600` e `0400` passam.
    (modo & 0o077 != 0).then_some(modo)
}

#[cfg(not(unix))]
pub fn permissao_frouxa(_caminho: &std::path::Path) -> Option<u32> {
    None
}

/// A new file only its owner can read, created with that mode (`0600` on Unix) rather than
/// tightened after the fact.
fn criar_privado(caminho: &std::path::Path) -> std::io::Result<std::fs::File> {
    let _ = std::fs::remove_file(caminho);
    let mut abrir = std::fs::OpenOptions::new();
    abrir.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        abrir.mode(0o600);
    }
    abrir.open(caminho)
}

/// Grava `api_key` no `config.toml` com `0600`, preservando o resto do arquivo.
///
/// Escreve num temporário no MESMO diretório e renomeia: `rename` dentro do mesmo sistema de
/// arquivos é atômico, então uma queda no meio deixa o config antigo intacto em vez de um
/// arquivo pela metade — e um config truncado aqui significa perder url, elenco e verificação
/// junto com a chave.
///
/// The temporary file is created already `0600`, before the key is written into it: writing
/// first and tightening after (what this did until 0.16.4) left the key on disk readable by
/// everyone between the two calls. A leftover temporary from a crash is removed first, since
/// the mode only applies to a file being created.
pub fn gravar_api_key(chave: &str) -> Result<PathBuf> {
    let caminho = Config::path().context("não consegui resolver o diretório de config do SO")?;
    let dir = caminho.parent().context("caminho de config sem diretório pai")?;
    std::fs::create_dir_all(dir).with_context(|| format!("criando {}", dir.display()))?;

    let atual = std::fs::read_to_string(&caminho).unwrap_or_default();
    let mut doc: toml::Table = toml::from_str(&atual)
        .with_context(|| format!("parse de {} (conserte à mão antes)", caminho.display()))?;
    doc.insert("api_key".into(), toml::Value::String(chave.to_string()));
    let texto = toml::to_string_pretty(&doc).context("serializando o config")?;

    let tmp = caminho.with_extension("toml.tmp");
    {
        use std::io::Write;
        let mut f = criar_privado(&tmp).with_context(|| format!("criando {}", tmp.display()))?;
        f.write_all(texto.as_bytes()).with_context(|| format!("escrevendo {}", tmp.display()))?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("0600 em {}", tmp.display()))?;
    }
    std::fs::rename(&tmp, &caminho)
        .with_context(|| format!("renomeando para {}", caminho.display()))?;
    Ok(caminho)
}

impl Config {
    /// Caminho do config.toml (`~/.config/shvia-code/config.toml`).
    pub fn path() -> Option<PathBuf> {
        directories::BaseDirs::new().map(|b| b.config_dir().join("shvia-code").join("config.toml"))
    }

    /// Carrega o arquivo (se existir) e sobrepõe com flags/env.
    pub fn load(cli_url: Option<String>) -> Result<Self> {
        let file = Self::path()
            .filter(|p| p.exists())
            .map(|p| -> Result<FileConfig> {
                let raw = std::fs::read_to_string(&p)
                    .with_context(|| format!("lendo {}", p.display()))?;
                toml::from_str(&raw).with_context(|| format!("parse de {}", p.display()))
            })
            .transpose()?
            .unwrap_or_default();

        let url = cli_url
            .or_else(|| std::env::var("SHVIA_URL").ok())
            .or(file.url)
            .unwrap_or_else(|| DEFAULT_URL.to_string());

        // HTTPS obrigatório (SECURITY_GUIDELINES §2.2): a API key viaja em
        // header a cada request. Exceção única: loopback, para dev local
        // (`php artisan serve` / gateway na própria máquina).
        if !url_seguro(&url) {
            anyhow::bail!(
                "SHVIA_URL insegura: {url} — use https:// (a API key viaja em header). \
                 Exceção permitida: http://localhost|127.x|[::1] para dev local."
            );
        }

        let profile = std::env::var("SHVIA_PROFILE").ok().or(file.profile);
        let effort = std::env::var("SHVIA_EFFORT").ok().filter(|e| !e.is_empty()).or(file.effort);
        let max_voltas =
            resolver_max_voltas(std::env::var("SHVIA_MAX_VOLTAS").ok(), file.max_voltas);

        // Chave: env tem precedência; senão o arquivo. Nunca de arg.
        let do_arquivo = file.api_key;

        // F-06: a doc prometia 0600 e nada criava nem conferia. Avisa só quando há
        // chave DENTRO — aviso em config sem chave treina o dev a ignorá-lo.
        if do_arquivo.is_some() {
            if let Some(p) = Self::path() {
                if let Some(modo) = permissao_frouxa(&p) {
                    eprintln!(
                        "aviso: {} está {:o} e contém api_key — qualquer conta da máquina consegue ler.\n\
                         \x20      Conserte com: chmod 600 {}   (ou use `anna config set-key`, que grava 0600)",
                        p.display(), modo, p.display()
                    );
                }
            }
        }

        let api_key = std::env::var("SHVIA_API_KEY")
            .ok()
            .filter(|k| !k.is_empty())
            .or(do_arquivo);

        let elenco = file.elenco.unwrap_or_default();
        let verificacao = file.verificacao;
        let bash_timeout = std::env::var("SHVIA_BASH_TIMEOUT")
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .or(file.bash_timeout)
            .unwrap_or(DEFAULT_BASH_TIMEOUT_S);

        Ok(Self { url, profile, api_key, effort, max_voltas, elenco, verificacao, bash_timeout })
    }

    /// `Config` derivada para um papel do orquestrador: troca só perfil/esforço,
    /// mantém url e chave.
    ///
    /// Precedência do esforço: elenco do papel > `effort` global (escolha
    /// EXPLÍCITA do dev, seja por flag, env ou config) > padrão do papel. Sem
    /// essa ordem, quem rodou `anna --effort low` de propósito veria o arquiteto
    /// subir pra `xhigh` sozinho.
    pub fn para(&self, papel: Papel) -> Config {
        let cfg = self.elenco.get(papel);
        let profile = cfg
            .and_then(|c| c.profile.clone())
            .or_else(|| self.profile.clone());
        let effort = cfg
            .and_then(|c| c.effort.clone())
            .or_else(|| self.effort.clone())
            .or_else(|| Some(papel.esforco_padrao().to_string()));
        Config { profile, effort, ..self.clone() }
    }

    /// Uma linha por papel, pro `anna config` — a tabela de roteamento tem que
    /// ser visível, é o ponto de ser estática.
    pub fn elenco_resolvido(&self) -> Vec<(&'static str, String, String)> {
        [Papel::Arquiteto, Papel::Executor, Papel::Revisor]
            .into_iter()
            .map(|p| {
                let c = self.para(p);
                (
                    p.chave(),
                    c.profile.unwrap_or_else(|| "(default do usuário)".into()),
                    c.effort.unwrap_or_else(|| "(default do servidor)".into()),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests_elenco {
    use super::*;
    use crate::papel::PapelCfg;

    fn base() -> Config {
        Config {
            url: "https://ai.shvia.org".into(),
            profile: Some("global".into()),
            api_key: None,
            effort: None,
            max_voltas: DEFAULT_MAX_VOLTAS,
            elenco: Elenco::default(),
            verificacao: None,
            bash_timeout: DEFAULT_BASH_TIMEOUT_S,
        }
    }

    #[test]
    fn papel_sem_elenco_cai_no_perfil_global_com_esforco_padrao() {
        let c = base().para(Papel::Arquiteto);
        assert_eq!(c.profile.as_deref(), Some("global"));
        assert_eq!(c.effort.as_deref(), Some("xhigh"), "arquiteto pensa por padrão");
        assert_eq!(base().para(Papel::Executor).effort.as_deref(), Some("high"));
    }

    /// O motivo da precedência ser (elenco > global > padrão do papel): quem
    /// rodou `anna --effort low` de propósito não pode ver o arquiteto subir
    /// pra `xhigh` sozinho e triplicar a conta.
    #[test]
    fn effort_explicito_do_dev_ganha_do_padrao_do_papel() {
        let mut c = base();
        c.effort = Some("low".into());
        assert_eq!(c.para(Papel::Arquiteto).effort.as_deref(), Some("low"));
    }

    #[test]
    fn elenco_ganha_do_global_e_nao_vaza_entre_papeis() {
        let mut c = base();
        c.effort = Some("low".into());
        c.elenco.arquiteto =
            Some(PapelCfg { profile: Some("opus5@shvia".into()), effort: Some("max".into()) });
        let a = c.para(Papel::Arquiteto);
        assert_eq!(a.profile.as_deref(), Some("opus5@shvia"));
        assert_eq!(a.effort.as_deref(), Some("max"));
        let e = c.para(Papel::Executor);
        assert_eq!(e.profile.as_deref(), Some("global"), "executor não herda o do arquiteto");
        assert_eq!(e.effort.as_deref(), Some("low"));
    }

    /// Trava a superfície de config documentada no README.
    #[test]
    fn toml_do_elenco_parseia() {
        let raw = r#"
profile = "global"
verificacao = ["cargo test"]

[elenco.arquiteto]
profile = "opus5@shvia"
effort  = "xhigh"

[elenco.executor]
profile = "sonnet5@shvia"
"#;
        let f: FileConfig = toml::from_str(raw).expect("config.toml do elenco deve parsear");
        let e = f.elenco.expect("elenco presente");
        assert_eq!(e.arquiteto.unwrap().effort.as_deref(), Some("xhigh"));
        assert!(e.executor.unwrap().effort.is_none(), "papel pode fixar só o perfil");
        assert!(e.revisor.is_none(), "papel omitido é opcional");
        assert_eq!(f.verificacao.unwrap(), vec!["cargo test".to_string()]);
    }
}

#[cfg(test)]
mod tests_url_seguro {
    use super::url_seguro;

    #[test]
    fn https_sempre_passa() {
        assert!(url_seguro("https://ai.shvia.org"));
        assert!(url_seguro("https://ia.blue3.com.br/api"));
    }

    #[test]
    fn loopback_de_verdade_passa() {
        assert!(url_seguro("http://localhost"));
        assert!(url_seguro("http://localhost:8000"));
        assert!(url_seguro("http://127.0.0.1:8000"));
        assert!(url_seguro("http://[::1]:8000"));
    }

    /// O motivo do helper existir: prefixo não é host. Cada linha abaixo passava
    /// no gate antigo e mandaria a API key em CLARO para um terceiro.
    #[test]
    fn sosia_de_loopback_e_rejeitado() {
        assert!(!url_seguro("http://localhost.atacante.tld"));
        assert!(!url_seguro("http://localhost@atacante.tld"));
        assert!(!url_seguro("http://127.evil.com"));
        assert!(!url_seguro("http://127.0.0.1.atacante.tld"));
        assert!(!url_seguro("http://[::1]@atacante.tld"));
    }

    #[test]
    fn http_comum_e_lixo_sao_rejeitados() {
        assert!(!url_seguro("http://ai.shvia.org"));
        assert!(!url_seguro("ftp://localhost"));
        assert!(!url_seguro("nao-e-url"));
        assert!(!url_seguro(""));
    }
}

#[cfg(test)]
mod tests_max_voltas {
    use super::*;

    #[test]
    fn precedencia_env_arquivo_default() {
        assert_eq!(resolver_max_voltas(Some("40".into()), Some(60)), 40, "env ganha do arquivo");
        assert_eq!(resolver_max_voltas(None, Some(60)), 60, "arquivo ganha do default");
        assert_eq!(resolver_max_voltas(None, None), DEFAULT_MAX_VOLTAS);
    }

    #[test]
    fn env_invalido_cai_para_o_arquivo_e_zero_vira_um() {
        assert_eq!(resolver_max_voltas(Some("abc".into()), Some(60)), 60);
        // 0 seria um agente que nunca chama o modelo — erro de config vira o mínimo útil.
        assert_eq!(resolver_max_voltas(Some("0".into()), None), 1);
        assert_eq!(resolver_max_voltas(None, Some(0)), 1);
    }
}

#[cfg(test)]
mod tests_permissao_e_chave {
    use super::*;

    fn temp(nome: &str) -> PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let id = N.fetch_add(1, Ordering::SeqCst);
        let d = std::env::temp_dir().join(format!("anna-cfg-{}-{id}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        d.join(nome)
    }

    /// Achado F-06: a doc prometia 0600 e nada criava nem conferia.
    #[cfg(unix)]
    #[test]
    fn permissao_frouxa_reclama_do_que_o_grupo_ou_outros_leem() {
        use std::os::unix::fs::PermissionsExt;
        let p = temp("c.toml");
        std::fs::write(&p, "api_key='x'").unwrap();

        for (modo, deve_reclamar) in [
            (0o600, false), // o prometido
            (0o400, false), // só leitura do dono também está fechado
            (0o644, true),  // o que o umask típico produz
            (0o640, true),  // o grupo lê
            (0o604, true),  // "outros" leem
            (0o666, true),
        ] {
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(modo)).unwrap();
            assert_eq!(
                permissao_frouxa(&p).is_some(),
                deve_reclamar,
                "modo {modo:o} deveria {} reclamar",
                if deve_reclamar { "" } else { "NÃO" }
            );
        }
        let _ = std::fs::remove_file(&p);
    }

    #[cfg(unix)]
    #[test]
    fn the_key_file_is_private_before_anything_is_written() {
        use std::os::unix::fs::PermissionsExt;
        let p = temp("privado.toml.tmp");
        // A leftover from a crash, readable by everyone, must not lend its mode to the new file.
        std::fs::write(&p, "old").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        let f = super::criar_privado(&p).unwrap();
        assert_eq!(f.metadata().unwrap().permissions().mode() & 0o777, 0o600, "0600 from creation, still empty");
        assert_eq!(f.metadata().unwrap().len(), 0, "the leftover is gone, not reused");
        drop(f);
        let _ = std::fs::remove_file(&p);
    }

    #[cfg(unix)]
    #[test]
    fn permissao_frouxa_devolve_o_modo_para_a_mensagem() {
        use std::os::unix::fs::PermissionsExt;
        let p = temp("modo.toml");
        std::fs::write(&p, "x=1").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        // O número tem de chegar à mensagem: "está frouxo" sem dizer QUANTO manda o
        // dev descobrir sozinho o que consertar.
        assert_eq!(permissao_frouxa(&p), Some(0o644));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn arquivo_inexistente_nao_reclama() {
        assert!(permissao_frouxa(&temp("nao-existe.toml")).is_none());
    }

    /// Prova de paridade: os dois defaults de timeout são o MESMO número.
    ///
    /// Ele existe em dois lugares por necessidade — `tools` não conhece `config` (é ela
    /// que empurra o valor), e `config` precisa de um padrão quando o arquivo é omisso.
    /// Dois números que deveriam ser um é exatamente a forma de defeito que esta revisão
    /// mais encontrou; aqui ela falha no teste em vez de virar "o teto não é o que a doc diz".
    #[test]
    fn padrao_de_timeout_bate_com_o_das_tools() {
        assert_eq!(DEFAULT_BASH_TIMEOUT_S, crate::tools::TIMEOUT_BASH_PADRAO_S);
    }
}
