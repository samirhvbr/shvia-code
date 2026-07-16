//! `anna` — CLI agêntico de código da Blue3, sobre o gateway do ShvIA.
//!
//! REPL (ou `-p` não-interativo) que conversa com a Anna: ela lê (read/grep),
//! edita (diff+confirma), roda bash e git com aprovação. Sessões locais
//! (retomar/listar), troca de modelo com `/model`. Modo `--json` = motor
//! headless (NDJSON) pra embutir noutra casca (docs/embedding.md). Protocolo:
//! docs/protocolo.md. Segurança: SECURITY_GUIDELINES.md.

mod agent;
mod api;
mod checkpoint;
mod config;
mod diff;
mod frontend;
mod gate;
mod guard;
mod json;
mod memory;
mod project;
mod render;
mod runner;
mod session;
mod stats;
mod stats_html;
mod stats_term;
mod telemetry;
mod tools;
mod types;
mod ui;

use clap::{Parser, Subcommand, ValueEnum};
use config::Config;
use frontend::Frontend;
use project::Project;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use session::{Session, Store};
use stats::{Range, Tab};
use std::path::{Path, PathBuf};
use std::process::Command;
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

    /// Esforço de raciocínio: low|medium|high|xhigh|max (o gateway valida).
    #[arg(long, value_name = "NÍVEL")]
    effort: Option<String>,

    /// Motor headless: emite eventos em NDJSON e lê comandos/decisões no stdin
    /// (pra embutir o `anna` noutra casca — docs/embedding.md).
    #[arg(long)]
    json: bool,

    /// Quem executa as ferramentas: `local` (o anna, default) ou `host` (o anna
    /// PROPÕE e a casca externa executa — só com --json; ADR-002 modelo B).
    #[arg(long, value_enum, default_value = "local")]
    tools: ToolsMode,

    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Clone, Copy, ValueEnum)]
enum ToolsMode {
    Local,
    Host,
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
    /// Dashboard de uso (Wrapped) das suas sessões locais.
    Stats {
        /// Aba: overview (padrão) ou models.
        #[arg(long, default_value = "overview")]
        tab: String,
        /// Período: all (padrão), 30d, 7d.
        #[arg(long, default_value = "all")]
        range: String,
        /// Em vez do terminal, gera uma página HTML fiel (abas/filtros ao vivo).
        #[arg(long)]
        html: bool,
        /// Com --html, abre no navegador (xdg-open/open).
        #[arg(long)]
        open: bool,
        /// Caminho do HTML (padrão ~/.shvia-code/stats.html).
        #[arg(long, value_name = "ARQUIVO")]
        out: Option<String>,
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
    if let Some(e) = cli.effort.clone() {
        cfg.effort = Some(e);
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
        Some(Cmd::Stats { tab, range, html, open, out }) => {
            run_stats(tab, range, *html, *open, out.clone());
            return Ok(());
        }
        None => {}
    }

    let project = Project::detect();
    // Memória de longo prazo (ai-memory) — no-op sem AI_MEMORY_URL.
    memory::init(&project.root);
    let rt = tokio::runtime::Runtime::new()?;
    let non_interactive = cli.prompt.is_some();
    let mut telemetry = Telemetry::default();

    // Quem executa as tools: local (o anna) ou host (só com --json; ADR-002 B).
    // No modo host o anna só PROPÕE — oferece só `bash` e usa um system prompt
    // próprio (a casca externa executa/aprova).
    let host_mode = cli.json && matches!(cli.tools, ToolsMode::Host);
    let mut runner: Box<dyn runner::ToolRunner> = if host_mode {
        Box::new(runner::HostRunner)
    } else {
        Box::new(runner::LocalRunner)
    };

    // Motor headless (NDJSON) — pra embutir noutra casca (docs/embedding.md).
    if cli.json {
        let mut fe = json::JsonSink::new();
        let mut messages = vec![Message::system(project.system_prompt(host_mode))];
        if let Some(prompt) = cli.prompt {
            run_json_turn(&rt, &cfg, &mut messages, &project, &mut fe, &mut telemetry, runner.as_mut(), prompt);
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
                        run_json_turn(&rt, &cfg, &mut messages, &project, &mut fe, &mut telemetry, runner.as_mut(), text);
                    }
                    Some("exit") => break,
                    _ => {}
                }
            }
        }
        memory::session_end_blocking(&rt);
        return Ok(());
    }

    let mut ui = Ui::new(non_interactive);
    let mut messages = vec![Message::system(project.system_prompt(host_mode))];

    // Modo não-interativo: uma pergunta e sai.
    if let Some(prompt) = cli.prompt {
        messages.push(Message::user(prompt));
        let turn = rt.block_on(agent::run_turn(&cfg, &mut messages, &project, &mut ui, &mut telemetry, runner.as_mut()));
        memory::session_end_blocking(&rt);
        turn?;
        return Ok(());
    }

    // REPL.
    let store = Store::open();
    let mut sess = Session::new();
    let name = display_name();

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

    ui.note("Enter envia · /help ajuda · /model troca modelo · /sessions lista · /stats uso · /clear novo · /exit sai");

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
                    ui.note("/stats [models] [7d|30d] mostra o dashboard de uso · /stats html gera a página HTML");
                    ui.note("/rollback mostra o que mudou desde o último checkpoint · /rollback aplicar restaura");
                    continue;
                }
                // Checkpoint de segurança: toda edição aprovada tira uma foto
                // (refs/anna/checkpoint). Duas etapas de propósito — ver antes
                // de desfazer; restaurar é decisão explícita, nunca um Enter só.
                if line == "/rollback" {
                    match checkpoint::diff_stat(&project.root) {
                        Some(stat) if !stat.trim().is_empty() => {
                            ui.note("mudou desde o último checkpoint:");
                            ui.note(&stat);
                            ui.note("restaure com: /rollback aplicar (arquivos NOVOS pós-foto permanecem)");
                        }
                        Some(_) => ui.note("(nada mudou desde o último checkpoint)"),
                        None => ui.note("(sem checkpoint ainda — a foto é tirada antes da primeira edição aprovada)"),
                    }
                    continue;
                }
                if line == "/rollback aplicar" {
                    match checkpoint::restore(&project.root) {
                        Ok(sha) => ui.note(&format!("working tree restaurada pro checkpoint {} (índice/branch intocados).", &sha[..sha.len().min(10)])),
                        Err(e) => ui.error(&e),
                    }
                    continue;
                }
                if line == "/clear" {
                    messages = vec![Message::system(project.system_prompt(host_mode))];
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
                if let Some(rest) = line.strip_prefix("/stats") {
                    if rest.is_empty() || rest.starts_with(' ') {
                        // /stats [models] [7d|30d|todos] [html]
                        let mut tab = Tab::Overview;
                        let mut range = Range::All;
                        let mut want_html = false;
                        for tok in rest.split_whitespace() {
                            match tok.to_lowercase().as_str() {
                                "models" | "modelos" | "modelo" => tab = Tab::Models,
                                "overview" | "geral" | "visao" | "visão" => tab = Tab::Overview,
                                "html" => want_html = true,
                                other => range = Range::parse(other),
                            }
                        }
                        let sessions = store.as_ref().map(|st| st.all()).unwrap_or_default();
                        if want_html {
                            let doc = stats_html::build(&sessions, &name);
                            match write_html(&doc, None) {
                                Ok(p) => ui.note(&format!("dashboard salvo em {} (abra no navegador)", p.display())),
                                Err(e) => ui.error(&e),
                            }
                        } else {
                            stats_term::render(&stats::compute(&sessions, range), tab, &name);
                        }
                        continue;
                    }
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
                    &cfg, &mut messages, &project, &mut ui, &mut telemetry, runner.as_mut(),
                )) {
                    ui.error(&e.to_string());
                }

                // Autosave da sessão (título = 1ª pergunta).
                if let Some(st) = &store {
                    if let Some(first) = messages.iter().find(|m| m.role == "user") {
                        sess.set_title_from(&first.content);
                    }
                    sess.messages = messages.clone();
                    sess.tokens = telemetry.tokens;
                    sess.cost = telemetry.cost;
                    sess.model = telemetry.model.clone().or_else(|| cfg.profile.clone());
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
    memory::session_end_blocking(&rt);
    Ok(())
}

/// Uma volta no modo headless: empurra a pergunta, roda o loop, e erro vira evento.
#[allow(clippy::too_many_arguments)]
fn run_json_turn(
    rt: &tokio::runtime::Runtime,
    cfg: &Config,
    messages: &mut Vec<Message>,
    project: &Project,
    fe: &mut dyn Frontend,
    tel: &mut Telemetry,
    runner: &mut dyn runner::ToolRunner,
    text: String,
) {
    messages.push(Message::user(text));
    if let Err(e) = rt.block_on(agent::run_turn(cfg, messages, project, fe, tel, runner)) {
        fe.error(&e.to_string());
    }
}

/// `anna stats` (subcomando): renderiza o dashboard no terminal ou exporta HTML.
fn run_stats(tab: &str, range: &str, html: bool, open: bool, out: Option<String>) {
    let name = display_name();
    let sessions = Store::open().map(|st| st.all()).unwrap_or_default();
    if html {
        let doc = stats_html::build(&sessions, &name);
        match write_html(&doc, out) {
            Ok(path) => {
                println!("dashboard salvo em {}", path.display());
                if open {
                    open_in_browser(&path);
                } else {
                    println!("abra no navegador (ou rode com --open)");
                }
            }
            Err(e) => eprintln!("{e}"),
        }
    } else {
        let st = stats::compute(&sessions, Range::parse(range));
        stats_term::render(&st, Tab::parse(tab), &name);
    }
}

/// Escreve o HTML no caminho dado (ou o padrão ~/.shvia-code/stats.html) e
/// devolve o caminho gravado.
fn write_html(doc: &str, out: Option<String>) -> Result<PathBuf, String> {
    let path = match out {
        Some(p) => PathBuf::from(p),
        None => {
            default_html_path().ok_or_else(|| "não consegui resolver ~/.shvia-code".to_string())?
        }
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&path, doc).map_err(|e| format!("erro ao escrever {}: {e}", path.display()))?;
    Ok(path)
}

fn default_html_path() -> Option<PathBuf> {
    let home = directories::BaseDirs::new()?.home_dir().to_path_buf();
    Some(home.join(".shvia-code").join("stats.html"))
}

/// Nome de exibição do cabeçalho ("…, Samir?"): 1ª letra do usuário do SO em
/// maiúscula; "dev" se não der pra resolver.
fn display_name() -> String {
    let raw = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_default();
    let raw = raw.trim();
    if raw.is_empty() {
        return "dev".into();
    }
    let mut chars = raw.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => "dev".into(),
    }
}

fn open_in_browser(path: &Path) {
    let p = path.to_string_lossy().to_string();
    #[cfg(target_os = "macos")]
    let _ = Command::new("open").arg(&p).status();
    #[cfg(target_os = "linux")]
    let _ = Command::new("xdg-open").arg(&p).status();
    #[cfg(target_os = "windows")]
    let _ = Command::new("cmd").args(["/C", "start", "", &p]).status();
}

fn print_config(cfg: &Config) {
    println!("anna {}", VERSION.trim());
    println!("url    : {}", cfg.url);
    println!("perfil : {}", cfg.profile.clone().unwrap_or_else(|| "(default do usuário)".into()));
    println!("esforço: {}", cfg.effort.clone().unwrap_or_else(|| "(default do servidor)".into()));
    println!(
        "chave  : {}",
        if cfg.api_key.is_some() { "definida (env/config)" } else { "AUSENTE — exporte SHVIA_API_KEY" }
    );
    println!("config : {}", Config::path().map(|p| p.display().to_string()).unwrap_or_default());
}
