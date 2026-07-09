//! `anna` — CLI agêntico de código da Blue3, sobre o gateway do ShvIA.
//!
//! **C1 (MVP leitura):** REPL que conversa com a Anna; ela inspeciona o projeto
//! com ferramentas de leitura (read_file/list_dir/grep/glob) e explica/aponta.
//! Escrita/bash/git entram no C2. Protocolo: docs/protocolo.md. Segurança:
//! SECURITY_GUIDELINES.md.

mod agent;
mod api;
mod config;
mod gate;
mod guard;
mod project;
mod telemetry;
mod tools;
mod types;
mod ui;

use clap::{Parser, Subcommand};
use config::Config;
use gate::Gate;
use project::Project;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use telemetry::Telemetry;
use types::Message;
use ui::Ui;

const VERSION: &str = include_str!("../version.md");

#[derive(Parser)]
#[command(name = "anna", version, about = "Anna — ShvIA Code (agente de código no terminal)")]
struct Cli {
    /// Pergunta única (não-interativo, p/ scripts/CI). Sem isto → REPL.
    #[arg(short = 'p', long = "print", value_name = "TEXTO")]
    prompt: Option<String>,

    /// URL do gateway (senão env SHVIA_URL ou o default).
    #[arg(long, value_name = "URL")]
    url: Option<String>,

    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Mostra a config resolvida (url, perfil, de onde vem a chave).
    Config,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cfg = Config::load(cli.url.clone())?;

    if let Some(Cmd::Config) = cli.cmd {
        println!("anna {}", VERSION.trim());
        println!("url    : {}", cfg.url);
        println!(
            "perfil : {}",
            cfg.profile.clone().unwrap_or_else(|| "(default do usuário)".into())
        );
        println!(
            "chave  : {}",
            if cfg.api_key.is_some() { "definida (env/config)" } else { "AUSENTE — exporte SHVIA_API_KEY" }
        );
        println!(
            "config : {}",
            Config::path().map(|p| p.display().to_string()).unwrap_or_default()
        );
        return Ok(());
    }

    let project = Project::detect();
    let rt = tokio::runtime::Runtime::new()?;
    let mut ui = Ui::new();
    let non_interactive = cli.prompt.is_some();
    let mut gate = Gate::new(non_interactive);
    let mut telemetry = Telemetry::default();
    let mut messages = vec![Message::system(project.system_prompt())];

    // Modo não-interativo: uma pergunta e sai.
    if let Some(prompt) = cli.prompt {
        messages.push(Message::user(prompt));
        rt.block_on(agent::run_turn(&cfg, &mut messages, &project, &mut ui, &mut telemetry, &mut gate))?;
        return Ok(());
    }

    // REPL.
    let modelo = cfg.profile.clone().unwrap_or_else(|| "modelo default".into());
    ui.banner(&project.header(&modelo, "ShvIA"));
    if cfg.api_key.is_none() {
        ui.warn("sem API key: exporte SHVIA_API_KEY (ou ~/.config/shvia-code/config.toml). 'anna config' mostra o estado.");
    }
    ui.note("Enter envia · /help ajuda · /clear limpa o contexto · /exit sai");

    let mut rl = DefaultEditor::new()?;
    loop {
        match rl.readline("\x1b[1mvocê ›\x1b[0m ") {
            Ok(line) => {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }
                let _ = rl.add_history_entry(line.as_str());
                match line.as_str() {
                    "/exit" | "/quit" | "/q" => break,
                    "/help" => {
                        ui.note("pergunte sobre o projeto; a Anna lê com read_file/list_dir/grep/glob. /clear reinicia, /exit sai.");
                        continue;
                    }
                    "/clear" => {
                        messages = vec![Message::system(project.system_prompt())];
                        telemetry = Telemetry::default();
                        ui.note("(contexto reiniciado)");
                        continue;
                    }
                    _ => {}
                }
                messages.push(Message::user(line));
                if let Err(e) = rt.block_on(agent::run_turn(
                    &cfg, &mut messages, &project, &mut ui, &mut telemetry, &mut gate,
                )) {
                    ui.error(&e.to_string());
                }
            }
            Err(ReadlineError::Interrupted) | Err(ReadlineError::Eof) => break,
            Err(e) => {
                ui.error(&e.to_string());
                break;
            }
        }
    }
    Ok(())
}
