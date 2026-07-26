//! Config resolvida (docs/arquitetura-cli.md §5).
//!
//! Precedência: flags/env > `~/.config/shvia-code/config.toml`.
//! **A API key NUNCA vem de argumento de linha** (aparece no `ps`) — só de
//! `SHVIA_API_KEY` ou do arquivo de config 0600 (SECURITY §2.4).

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

#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    url: Option<String>,
    profile: Option<String>,
    api_key: Option<String>,
    effort: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub url: String,
    pub profile: Option<String>,
    pub api_key: Option<String>,
    /// Esforço de raciocínio (low|medium|high|xhigh|max). O gateway valida.
    pub effort: Option<String>,
}

/// `true` se `url` puder receber a API key: https, ou http em LOOPBACK de verdade.
///
/// O host é comparado inteiro, nunca por prefixo. A versão anterior usava
/// `url.starts_with("http://localhost")`, e prefixo não é host:
/// `http://localhost.atacante.tld` passava pela exceção de dev e a chave saía em
/// CLARO para um domínio de terceiro (o `127.` era pior — `http://127.evil.com`).
fn url_seguro(url: &str) -> bool {
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

        // Chave: env tem precedência; senão o arquivo (0600). Nunca de arg.
        let api_key = std::env::var("SHVIA_API_KEY")
            .ok()
            .filter(|k| !k.is_empty())
            .or(file.api_key);

        Ok(Self { url, profile, api_key, effort })
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
