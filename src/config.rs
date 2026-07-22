//! Config resolvida (docs/arquitetura-cli.md §5).
//!
//! Precedência: flags/env > `~/.config/shvia-code/config.toml`.
//! **A API key NUNCA vem de argumento de linha** (aparece no `ps`) — só de
//! `SHVIA_API_KEY` ou do arquivo de config 0600 (SECURITY §2.4).

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

pub const DEFAULT_URL: &str = "https://ia.blue3.com.br";

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
        let is_loopback = url.starts_with("http://localhost")
            || url.starts_with("http://127.")
            || url.starts_with("http://[::1]");
        if !url.starts_with("https://") && !is_loopback {
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
