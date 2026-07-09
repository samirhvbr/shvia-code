//! `anna` — CLI agêntico de código da Blue3, sobre o gateway do ShvIA.
//!
//! **Esqueleto do C1** (ver `docs/arquitetura-cli.md`): compila e expõe a casca
//! (args, config resolvida, banner). O loop agêntico, as ferramentas e o REPL
//! entram nas fases C1–C3 — este binário é o ponto de partida da produção, não
//! o produto. Protocolo com o servidor: `docs/protocolo.md`. Regras/segurança:
//! `CLAUDE.md` e `SECURITY_GUIDELINES.md`.

use clap::{Parser, Subcommand};

/// Versão vem do `version.md` (fonte única) em tempo de compilação.
const VERSION: &str = include_str!("../version.md");
const DEFAULT_URL: &str = "https://ia.blue3.com.br";

#[derive(Parser)]
#[command(name = "anna", version, about = "Anna — ShvIA Code (agente de código no terminal)")]
struct Cli {
    /// Pergunta única (modo não-interativo, p/ scripts/CI). Sem isto → REPL.
    #[arg(short = 'p', long = "print", value_name = "TEXTO")]
    prompt: Option<String>,

    /// URL do gateway do ShvIA (senão env SHVIA_URL ou o default).
    #[arg(long, value_name = "URL")]
    url: Option<String>,

    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Mostra a config resolvida (url, caminho, de onde vem a chave).
    Config,
}

/// Precedência: --url > env SHVIA_URL > default. A API key NUNCA vem de arg
/// (aparece no `ps`) — só de env SHVIA_API_KEY ou da config 0600 (SECURITY §2.4).
fn shvia_url(cli_url: &Option<String>) -> String {
    cli_url
        .clone()
        .or_else(|| std::env::var("SHVIA_URL").ok())
        .unwrap_or_else(|| DEFAULT_URL.to_string())
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let version = VERSION.trim();

    if let Some(Cmd::Config) = cli.cmd {
        println!("anna {version}");
        println!("url    : {}", shvia_url(&cli.url));
        println!("config : ~/.config/shvia-code/config.toml (0600)");
        println!("sessões: ~/.shvia-code/sessions/ (0600)");
        println!("chave  : env SHVIA_API_KEY (ou config — nunca em argumento)");
        return Ok(());
    }

    println!("ANNA · ShvIA Code {version}  —  {}", shvia_url(&cli.url));
    match cli.prompt {
        Some(p) => eprintln!("[esqueleto C1] pergunta recebida: {p:?} — o loop agêntico entra no C1."),
        None => eprintln!(
            "[esqueleto C1] o REPL entra no C1. Veja docs/arquitetura-cli.md e docs/protocolo.md."
        ),
    }
    Ok(())
}
