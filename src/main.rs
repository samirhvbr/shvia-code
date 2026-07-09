//! `anna` — CLI agêntico de código da Blue3, sobre o gateway do ShvIA.
//!
//! REPL (ou `-p` não-interativo) que conversa com a Anna: ela lê (read/grep),
//! edita (diff+confirma), roda bash e git com aprovação. Sessões locais
//! (retomar/listar), troca de modelo com `/model`. Modo `--json` = motor
//! headless (NDJSON) pra embutir noutra casca (docs/embedding.md). Protocolo:
//! docs/protocolo.md. Segurança: SECURITY_GUIDELINES.md.

mod agent;
mod api;
mod config;
mod diff;
mod frontend;
mod gate;
mod guard;
mod json;
mod project;
mod render;
mod session;
mod telemetry;
mod tools;
mod types;
mod ui;

use clap::{Parser, Subcommand};
use config::Config;
use frontend::Frontend;
use project::Project;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use session::{Session, Store};
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

    /// Modelo/perfil desta execução (senão o default do usuário).
    #[arg(long, value_name = "PERFIL")]
    model: Option<String>,

    /// Motor headless: emite eventos em NDJSON e lê comandos/decisões no stdin
    /// (pra embutir o `anna` noutra casca — docs/embedding.md).
    #[arg(long)]
    json: bool,

    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Mostra a config resolvida (url, perfil, de onde vem a chave).
    Config,
    /// Sessões locais (~/.shvia-code/sessions).
    Sessions {
        #[command(subcommand)]
        action: SessionAction,
    },
}

#[derive(Subcommand)]
enum SessionAction {
    /// Lista as sessões salvas (mais recentes primeiro).
    List,
    /// Retoma uma sessão pelo id (cai no REPL com o histórico).
    Resume { id: String },
    /// Apaga todas as sessões locais.
    Clear,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let mut cfg = Config::load(cli.url.clone())?;
    if let Some(m) = cli.model.clone() {
        cfg.profile = Some(m);
    }

    // Subcomandos.
    let mut resume_id: Option<String> = None;
    match &cli.cmd {
        Some(Cmd::Config) => {
            print_config(&cfg);
            return Ok(());
        }
        Some(Cmd::Sessions { action }) => match action {
            SessionAction::List => {
                match Store::open() {
                    Some(st) => {
                        let list = st.list();
                        if list.is_empty() {
                            println!("(sem sessões)");
                        } else {
                            for (id, title) in list {
                                println!("#{id}  {title}");
                            }
                        }
                    }
                    None => println!("(não consegui abrir ~/.shvia-code/sessions)"),
                }
                return Ok(());
            }
            SessionAction::Clear => {
                let n = Store::open().map(|st| st.clear()).unwrap_or(0);
                println!("{n} sessão(ões) apagada(s).");
                return Ok(());
            }
            SessionAction::Resume { id } => resume_id = Some(id.clone()),
        },
        None => {}
    }

    let project = Project::detect();
    let rt = tokio::runtime::Runtime::new()?;
    let non_interactive = cli.prompt.is_some();
    let mut telemetry = Telemetry::default();

    // Motor headless (NDJSON) — pra embutir noutra casca (docs/embedding.md).
    if cli.json {
        let mut fe = json::JsonSink::new();
        let mut messages = vec![Message::system(project.system_prompt())];
        if let Some(prompt) = cli.prompt {
            run_json_turn(&rt, &cfg, &mut messages, &project, &mut fe, &mut telemetry, prompt);
        } else {
            // Persistente: o host manda {"type":"user","text":"..."} por linha.
            let mut line = String::new();
            loop {
                line.clear();
                if std::io::stdin().read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                let t = line.trim();
                if t.is_empty() {
                    continue;
                }
                let v: serde_json::Value = serde_json::from_str(t).unwrap_or(serde_json::Value::Null);
                match v.get("type").and_then(|x| x.as_str()) {
                    Some("user") => {
                        let text = v.get("text").and_then(|x| x.as_str()).unwrap_or_default().to_string();
                        run_json_turn(&rt, &cfg, &mut messages, &project, &mut fe, &mut telemetry, text);
                    }
                    Some("exit") => break,
                    _ => {}
                }
            }
        }
        return Ok(());
    }

    let mut ui = Ui::new(non_interactive);
    let mut messages = vec![Message::system(project.system_prompt())];

    // Modo não-interativo: uma pergunta e sai.
    if let Some(prompt) = cli.prompt {
        messages.push(Message::user(prompt));
        rt.block_on(agent::run_turn(&cfg, &mut messages, &project, &mut ui, &mut telemetry))?;
        return Ok(());
    }

    // REPL.
    let store = Store::open();
    let mut sess = Session::new();

    let modelo = cfg.profile.clone().unwrap_or_else(|| "modelo default".into());
    ui.banner(&project.header(&modelo, "ShvIA"));
    if cfg.api_key.is_none() {
        ui.warn("sem API key: exporte SHVIA_API_KEY (ou ~/.config/shvia-code/config.toml). 'anna config' mostra o estado.");
    }

    // Retomar sessão, se pedido (anna sessions resume <id>).
    if let (Some(id), Some(st)) = (&resume_id, &store) {
        match st.load(id) {
            Some(s) => {
                if !s.messages.is_empty() {
                    messages = s.messages.clone();
                }
                ui.note(&format!("(retomado #{} — {})", s.id, s.title));
                sess = s;
            }
            None => ui.warn(&format!("sessão '{id}' não encontrada — começando nova.")),
        }
    }

    ui.note("Enter envia · /help ajuda · /model troca modelo · /sessions lista · /clear novo · /exit sai");

    let mut rl = DefaultEditor::new()?;
    loop {
        match rl.readline("\x1b[1mvocê ›\x1b[0m ") {
            Ok(line) => {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }
                let _ = rl.add_history_entry(line.as_str());

                // Slash-commands.
                if matches!(line.as_str(), "/exit" | "/quit" | "/q") {
                    break;
                }
                if line == "/help" {
                    ui.note("Pergunte sobre o projeto — a Anna lê (read/grep), edita (diff+confirma), roda bash e git com aprovação.");
                    ui.note("/model [perfil] troca o modelo · /sessions lista · /tools ferramentas · /clear novo contexto · /exit");
                    continue;
                }
                if line == "/clear" {
                    messages = vec![Message::system(project.system_prompt())];
                    telemetry = Telemetry::default();
                    sess = Session::new();
                    ui.note("(contexto e sessão reiniciados)");
                    continue;
                }
                if line == "/sessions" {
                    match &store {
                        Some(st) => {
                            let list = st.list();
                            if list.is_empty() {
                                ui.note("(sem sessões)");
                            } else {
                                for (id, title) in list.iter().take(15) {
                                    ui.note(&format!("  #{id}  {title}"));
                                }
                                ui.note("retome com: anna sessions resume <id>");
                            }
                        }
                        None => ui.note("(sessões indisponíveis)"),
                    }
                    continue;
                }
                if line == "/tools" {
                    let names: Vec<String> = tools::defs().into_iter().map(|d| d.name).collect();
                    ui.note(&format!("ferramentas: {}", names.join(", ")));
                    continue;
                }
                if let Some(rest) = line.strip_prefix("/model") {
                    if rest.is_empty() || rest.starts_with(' ') {
                        let arg = rest.trim();
                        if arg.is_empty() {
                            ui.note(&format!(
                                "modelo atual: {}",
                                cfg.profile.clone().unwrap_or_else(|| "(default do usuário)".into())
                            ));
                        } else {
                            cfg.profile = Some(arg.to_string());
                            ui.note(&format!("modelo → {arg}"));
                        }
                        continue;
                    }
                }

                // Pergunta normal.
                messages.push(Message::user(line));
                if let Err(e) = rt.block_on(agent::run_turn(
                    &cfg, &mut messages, &project, &mut ui, &mut telemetry,
                )) {
                    ui.error(&e.to_string());
                }

                // Autosave da sessão (título = 1ª pergunta).
                if let Some(st) = &store {
                    if let Some(first) = messages.iter().find(|m| m.role == "user") {
                        sess.set_title_from(&first.content);
                    }
                    sess.messages = messages.clone();
                    st.save(&sess);
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

/// Uma volta no modo headless: empurra a pergunta, roda o loop, e erro vira evento.
fn run_json_turn(
    rt: &tokio::runtime::Runtime,
    cfg: &Config,
    messages: &mut Vec<Message>,
    project: &Project,
    fe: &mut dyn Frontend,
    tel: &mut Telemetry,
    text: String,
) {
    messages.push(Message::user(text));
    if let Err(e) = rt.block_on(agent::run_turn(cfg, messages, project, fe, tel)) {
        fe.error(&e.to_string());
    }
}

fn print_config(cfg: &Config) {
    println!("anna {}", VERSION.trim());
    println!("url    : {}", cfg.url);
    println!("perfil : {}", cfg.profile.clone().unwrap_or_else(|| "(default do usuário)".into()));
    println!(
        "chave  : {}",
        if cfg.api_key.is_some() { "definida (env/config)" } else { "AUSENTE — exporte SHVIA_API_KEY" }
    );
    println!("config : {}", Config::path().map(|p| p.display().to_string()).unwrap_or_default());
}
