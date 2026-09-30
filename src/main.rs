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
mod menu;
mod orquestrador;
mod papel;
mod plano;
mod project;
mod render;
mod runner;
mod session;
mod stats;
mod stats_html;
mod stats_term;
mod telemetry;
mod term;
mod tools;
mod types;
mod ui;
mod view;

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

// version.md ganhou um bloco de aviso em 0.10.1 — só a 1ª linha é a versão.
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
    Config {
        #[command(subcommand)]
        action: Option<ConfigAction>,
    },
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
enum ConfigAction {
    /// Grava a `api_key` no config.toml com permissão 0600 (achado F-06).
    ///
    /// Existe porque a doc prometia 0600 e nada criava o arquivo: o usuário o fazia à
    /// mão, com o umask dele — tipicamente 0644, legível por qualquer conta da máquina.
    SetKey {
        /// A chave. Sem isto, lê do stdin — que é o modo que NÃO deixa a chave no
        /// histórico do shell.
        chave: Option<String>,
    },
}

#[derive(Subcommand)]
enum SessionAction {
    /// Lista as sessões salvas (mais recentes primeiro).
    List,
    /// Retoma uma sessão pelo id (cai no REPL com o histórico).
    Resume { id: String },
    /// Apaga sessões locais (todas, ou só as mais antigas que N dias).
    Clear {
        /// Só as sem uso há mais de N dias. Sem isto, apaga todas.
        #[arg(long, value_name = "DIAS")]
        older_than: Option<u64>,
    },
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
        Some(Cmd::Config { action: None }) => {
            print_config(&cfg);
            return Ok(());
        }
        Some(Cmd::Config { action: Some(ConfigAction::SetKey { chave }) }) => {
            let k = match chave {
                Some(k) => k.trim().to_string(),
                None => {
                    // Sem argumento a chave não passa pela linha de comando, e portanto
                    // não fica no `~/.bash_history` nem na lista de processos.
                    eprint!("chave: ");
                    use std::io::Write;
                    let _ = std::io::stderr().flush();
                    let mut buf = String::new();
                    std::io::stdin().read_line(&mut buf)?;
                    buf.trim().to_string()
                }
            };
            if k.is_empty() {
                anyhow::bail!("chave vazia — nada gravado");
            }
            let p = config::gravar_api_key(&k)?;
            println!("chave gravada em {} (0600).", p.display());
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
            SessionAction::Clear { older_than } => {
                let n = Store::open()
                    .map(|st| match older_than {
                        Some(d) => st.clear_older_than(*d),
                        None => st.clear(),
                    })
                    .unwrap_or(0);
                match older_than {
                    Some(d) => println!("{n} sessão(ões) sem uso há mais de {d} dia(s) apagada(s)."),
                    None => println!("{n} sessão(ões) apagada(s)."),
                }
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

    // Teto de tempo do `bash` (achado F-04). Antes de qualquer ferramenta rodar.
    tools::configurar_timeout_bash(cfg.bash_timeout);

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
        let mut fe = json::JsonSink::new(non_interactive);
        let mut messages = vec![Message::system(project.system_prompt(host_mode))];
        if let Some(prompt) = cli.prompt {
            // One-shot (`--json --prompt`): CLI não tem por onde passar imagem.
            run_json_turn(&rt, &cfg, &mut messages, &project, &mut fe, &mut telemetry, runner.as_mut(), prompt, vec![]);
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
                        // `images`: base64 PURO, opcional. Item que não for string
                        // é descartado em vez de virar `""` — base64 vazio chega ao
                        // provedor como imagem corrompida, e o erro voltaria falando
                        // de outra coisa.
                        let images: Vec<String> = v
                            .get("images")
                            .and_then(|x| x.as_array())
                            .map(|a| a.iter().filter_map(|i| i.as_str()).map(str::to_string).collect())
                            .unwrap_or_default();
                        run_json_turn(&rt, &cfg, &mut messages, &project, &mut fe, &mut telemetry, runner.as_mut(), text, images);
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
    let home = std::env::var("HOME").ok();
    let cwd = view::tildify(&project.root.display().to_string(), home.as_deref());
    ui.welcome(VERSION.lines().next().unwrap_or("").trim(), &modelo, "ShvIA", &cwd, project.git_branch.as_deref());
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

    ui.note("Enter envia · /help ajuda · /plano pipeline · /model troca modelo · /sessions lista · /stats uso · /clear novo · /exit sai");

    let mut rl = DefaultEditor::new()?;
    loop {
        match rl.readline(&format!("{} ", term::bold("você ›"))) {
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
                    ui.note("/plano <pedido> roda o pipeline: arquiteto planeja → você aprova → executor aplica → revisão fecha");
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

                // Pipeline de papéis (docs/orquestrador.md): o arquiteto planeja
                // em só-leitura, VOCÊ aprova o plano, o executor aplica passo a
                // passo e a revisão (clippy/testes + revisor-modelo) fecha.
                if let Some(rest) = line.strip_prefix("/plano") {
                    if rest.is_empty() || rest.starts_with(' ') {
                        let pedido = rest.trim().to_string();
                        if pedido.is_empty() {
                            ui.note("/plano <o que você quer> — arquiteto → você aprova → executor → revisão");
                        } else if let Err(e) = rt.block_on(orquestrador::executar(
                            &cfg, &project, &mut ui, &mut telemetry, &pedido,
                        )) {
                            ui.error(&e.to_string());
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

/// Empurra o turno do usuário — e **apaga as imagens dos turnos anteriores**.
///
/// ⚠️ Por que a limpeza existe. O `anna` guarda o histórico em memória e reenvia
/// a lista INTEIRA a cada volta do laço agêntico. Sem isto, uma figura colada uma
/// vez subiria de novo em cada chamada de ferramenta, de todo turno seguinte, até
/// a sessão acabar — num turno de código com 30 voltas isso é a mesma imagem paga
/// 30 vezes, e ninguém veria: a resposta sai certa. O chat da casa não tem o
/// problema porque monta `images` só no turno corrente (`ChatController:744`);
/// aqui a lista é viva, então a regra precisa ser dita.
///
/// Dentro do turno a figura SOBREVIVE a todas as voltas, de propósito: o modelo
/// precisa poder olhar a imagem outra vez depois de ler um arquivo.
fn empurrar_turno_usuario(messages: &mut Vec<Message>, text: String, images: Vec<String>) {
    for m in messages.iter_mut() {
        m.images.clear();
    }
    messages.push(if images.is_empty() {
        Message::user(text)
    } else {
        Message::user_com_imagens(text, images)
    });
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
    images: Vec<String>,
) {
    empurrar_turno_usuario(messages, text, images);
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
    println!("anna {}", VERSION.lines().next().unwrap_or("").trim());
    println!("url    : {}", cfg.url);
    println!("perfil : {}", cfg.profile.clone().unwrap_or_else(|| "(default do usuário)".into()));
    println!("esforço: {}", cfg.effort.clone().unwrap_or_else(|| "(default do servidor)".into()));
    // O teto é um parâmetro de SEGURANÇA (§2.7) que virou configurável — o
    // comando que audita a config resolvida tem que mostrá-lo.
    println!("voltas : teto de {} por turno (env SHVIA_MAX_VOLTAS / `max_voltas` no config)", cfg.max_voltas);
    println!(
        "chave  : {}",
        if cfg.api_key.is_some() { "definida (env/config)" } else { "AUSENTE — exporte SHVIA_API_KEY" }
    );
    println!("config : {}", Config::path().map(|p| p.display().to_string()).unwrap_or_default());
    // A tabela de roteamento do orquestrador é estática justamente pra ser
    // auditável — então ela tem que aparecer aqui.
    println!("elenco (/plano):");
    for (papel, perfil, esforco) in cfg.elenco_resolvido() {
        println!("  {papel:<10} {perfil}  ·  esforço {esforco}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O corpo de um turno SEM imagem tem de sair byte a byte como saía antes de
    /// a fatia existir — 100% dos turnos de hoje passam por aqui, e um campo a
    /// mais no JSON mudaria o contrato de todo mundo por causa de um caso novo.
    #[test]
    fn turno_sem_imagem_nao_ganha_chave_nenhuma() {
        let j = serde_json::to_string(&Message::user("oi")).unwrap();
        assert_eq!(j, r#"{"role":"user","content":"oi"}"#);
    }

    /// E com imagem ela viaja como base64 PURO, na convenção que o gateway já
    /// fala (`$entry['images']`) — sem mime, que os drivers derivam dos magic
    /// bytes (`ImageMime::detect`).
    #[test]
    fn turno_com_imagem_leva_base64_puro() {
        let j = serde_json::to_string(&Message::user_com_imagens("olha", vec!["QUJD".into()])).unwrap();
        assert_eq!(j, r#"{"role":"user","content":"olha","images":["QUJD"]}"#);
    }

    /// A régua que impede a conta de crescer sozinha: turno novo apaga a figura
    /// do turno velho. Sem ela a mesma imagem sobe em cada volta do laço, de
    /// todo turno seguinte — e a resposta sai certa, então ninguém nota.
    #[test]
    fn turno_novo_apaga_a_imagem_do_turno_velho() {
        let mut m = vec![
            Message::system("s"),
            Message::user_com_imagens("primeira", vec!["IMG1".into()]),
            Message::assistant("respondi", vec![]),
        ];
        empurrar_turno_usuario(&mut m, "segunda".into(), vec![]);

        assert_eq!(m.len(), 4);
        assert!(m[1].images.is_empty(), "a figura do turno anterior tem de sair");
        assert_eq!(m[1].content, "primeira", "o TEXTO do turno anterior fica — só a figura sai");
        assert!(m[3].images.is_empty());
    }

    /// E o turno novo carrega a sua. Uma coisa não pode apagar a outra.
    #[test]
    fn o_turno_novo_carrega_a_figura_dele() {
        let mut m = vec![Message::system("s"), Message::user_com_imagens("velha", vec!["IMG1".into()])];
        empurrar_turno_usuario(&mut m, "nova".into(), vec!["IMG2".into()]);

        assert!(m[1].images.is_empty());
        assert_eq!(m[2].images, vec!["IMG2".to_string()]);
    }

    /// Item que não é string sai fora em vez de virar `""`: base64 vazio chega
    /// ao provedor como imagem corrompida, e o erro voltaria falando de outra
    /// coisa. É a leitura do stdin do host, transcrita.
    #[test]
    fn images_do_host_descarta_o_que_nao_e_string() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{"type":"user","text":"x","images":["AAA",null,42,"BBB"]}"#).unwrap();
        let images: Vec<String> = v
            .get("images")
            .and_then(|x| x.as_array())
            .map(|a| a.iter().filter_map(|i| i.as_str()).map(str::to_string).collect())
            .unwrap_or_default();
        assert_eq!(images, vec!["AAA".to_string(), "BBB".to_string()]);
    }
}

/// Réguas do próprio repositório — coisas que nenhum teste de produto pega porque
/// não estão no caminho do produto, e que envelhecem em silêncio.
#[cfg(test)]
mod reguas_do_repo {
    use std::path::Path;

    /// Toda `uses:` do CI está pinada por SHA de commit — achado F-09.
    ///
    /// Tag de GitHub Action é **ponteiro móvel**: `actions/checkout@v4` roda o que o dono
    /// do repositório publicar amanhã sob aquela tag, com as permissões deste workflow e
    /// acesso ao token do job. Não é hipótese remota — é o vetor de `tj-actions/changed-files`
    /// (03/2025), em que uma tag movida passou a vazar segredos de milhares de repositórios.
    ///
    /// A régua persegue a causa, não o sintoma: o defeito não é "esta action está solta", é
    /// **uma action nova entrar sem pin**. Por isso ela varre o diretório inteiro e não uma
    /// lista — um workflow novo já nasce medido.
    ///
    /// O comentário `# vX.Y.Z` ao lado do SHA não é enfeite: sem ele, subir o pin vira
    /// arqueologia. A régua exige os dois.
    #[test]
    fn toda_action_do_ci_esta_pinada_por_sha() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows");
        let mut vistos = 0usize;
        let mut soltas: Vec<String> = Vec::new();

        let entradas = std::fs::read_dir(&dir).expect("o repositório tem .github/workflows");
        for e in entradas.flatten() {
            let caminho = e.path();
            let ext = caminho.extension().and_then(|x| x.to_str()).unwrap_or("");
            if ext != "yml" && ext != "yaml" {
                continue;
            }
            let arquivo = caminho.file_name().unwrap().to_string_lossy().to_string();
            let texto = std::fs::read_to_string(&caminho).expect("workflow legível");
            for (n, linha) in texto.lines().enumerate() {
                let corte = linha.trim_start();
                // Só `uses:` de action; `uses:` dentro de comentário não conta.
                if corte.starts_with('#') {
                    continue;
                }
                let Some(resto) = corte.strip_prefix("- uses:").or_else(|| corte.strip_prefix("uses:")) else {
                    continue;
                };
                let referencia = resto.trim().split('#').next().unwrap_or("").trim();
                // `uses:` local (`./algo`) e de container (`docker://`) não têm SHA a pinar.
                if referencia.starts_with('.') || referencia.starts_with("docker://") {
                    continue;
                }
                vistos += 1;
                let sha = referencia.rsplit('@').next().unwrap_or("");
                let pinada = sha.len() == 40 && sha.chars().all(|c| c.is_ascii_hexdigit());
                // O comentário que diz QUAL versão o SHA é.
                let tem_nota = linha.contains('#');
                if !pinada || !tem_nota {
                    let porque = if pinada { "sem o comentário dizendo a versão" } else { "não é SHA de 40 hex" };
                    soltas.push(format!("{arquivo}:{}  {referencia}  ({porque})", n + 1));
                }
            }
        }

        assert!(vistos > 0, "nenhuma `uses:` encontrada — a régua estaria medindo o vazio");
        assert!(
            soltas.is_empty(),
            "action(s) do CI sem pin por SHA (+ comentário da versão):\n  {}\n\
             Resolva a tag com:\n  \
             gh api repos/<dono>/<repo>/git/ref/tags/<tag> --jq '.object.sha'",
            soltas.join("\n  ")
        );
    }
    /// A árvore de `docs/arquitetura-cli.md` §2 bate com o `src/` — achado F-10.
    ///
    /// A seção descrevia `api/`, `tools/`, `ui/`, `cli.rs` e `error.rs`: um desenho por
    /// subdiretório que foi abandonado cedo e nunca chegou ao disco. Doc velha é comum; o
    /// que faz esta valer uma régua é que ela estava sendo **mantida velha** — as notas dos
    /// achados F-04 e F-05 foram escritas dentro da descrição de um `shell.rs` que não
    /// existe. Quem edita a doc não olha a árvore, e quem cria módulo não abre a doc.
    ///
    /// Então a régua fecha os dois lados: módulo do `src/` que não está documentado, e
    /// módulo documentado que não está no `src/`. Um arquivo novo em `src/` deixa o
    /// `cargo test` vermelho até ganhar uma linha na doc.
    #[test]
    fn arvore_da_doc_bate_com_src() {
        let raiz = Path::new(env!("CARGO_MANIFEST_DIR"));
        let doc = std::fs::read_to_string(raiz.join("docs/arquitetura-cli.md"))
            .expect("docs/arquitetura-cli.md legível");
        // A Windows checkout with core.autocrlf=true has CRLF here, and "\nsrc/\n" never
        // matched: the ruler panicked before measuring anything (Windows CI, 0.11.27).
        let doc = doc.replace("\r\n", "\n");

        // O bloco de código da §2 — entre a linha `src/` e a cerca que o fecha.
        let inicio = doc.find("\nsrc/\n").expect("a §2 tem o bloco da árvore começando em `src/`");
        let bloco = &doc[inicio..];
        let fim = bloco.find("\n```").expect("o bloco da árvore é fechado");
        let bloco = &bloco[..fim];

        let mut na_doc: Vec<String> = bloco
            .lines()
            .filter_map(|l| {
                let t = l.trim();
                // Só as linhas que ABREM a descrição de um módulo (`  nome.rs   texto`);
                // as linhas de continuação são indentadas além e não começam com o nome.
                if !l.starts_with("  ") || l.starts_with("   ") {
                    return None;
                }
                t.split_whitespace().next().filter(|n| n.ends_with(".rs")).map(str::to_string)
            })
            .collect();

        let mut no_src: Vec<String> = std::fs::read_dir(raiz.join("src"))
            .expect("src/ legível")
            .flatten()
            .filter_map(|e| {
                let n = e.file_name().to_string_lossy().to_string();
                n.ends_with(".rs").then_some(n)
            })
            .collect();

        na_doc.sort();
        no_src.sort();

        let faltam: Vec<&String> = no_src.iter().filter(|f| !na_doc.contains(f)).collect();
        let sobram: Vec<&String> = na_doc.iter().filter(|f| !no_src.contains(f)).collect();

        assert!(
            faltam.is_empty() && sobram.is_empty(),
            "docs/arquitetura-cli.md §2 divergiu do src/:\n  \
             no src/ e FORA da doc: {faltam:?}\n  \
             na doc e FORA do src/: {sobram:?}"
        );
        assert!(no_src.len() > 10, "o src/ tem {} arquivos — a régua estaria medindo o vazio", no_src.len());
    }

    /// `AGENTS.md` e `CLAUDE.md` são o mesmo texto abaixo do H1 — achado F-21.
    ///
    /// Os dois arquivos JÁ exigiam isso, por escrito, de si mesmos. E os dois violavam:
    /// no DESKTOP o comentário HTML do topo, no MOBILE o blockquote "Leia também". Não por
    /// desleixo — o bloco que divergia era exatamente o que dizia *"este arquivo é espelho
    /// do outro"*, e escrito em 1ª pessoa ele **não pode** ser idêntico nos dois. A regra
    /// era impossível de cumprir, o que é a razão pela qual instrução sem guarda apodrece:
    /// ninguém percebe que está pedindo o impossível.
    ///
    /// O ponteiro foi reescrito na 3ª pessoa (nomeia os dois arquivos, não "este"), e agora
    /// a régua vale de verdade. Um `diff` de uma linha, que roda a cada `cargo test`.
    #[test]
    fn agents_e_claude_sao_espelho() {
        let raiz = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        // Nos dois repos Tauri o Cargo.toml vive em `src-tauri/`; no CODE, na raiz.
        let base = if raiz.join("../AGENTS.md").exists() { raiz.join("..") } else { raiz.to_path_buf() };
        let ler = |n: &str| {
            let p = base.join(n);
            let t = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{} ilegível: {e}", p.display()));
            // Fora o H1 (a única linha que PODE diferir: cada arquivo tem seu título).
            t.lines().skip(1).collect::<Vec<_>>().join("\n")
        };
        let agents = ler("AGENTS.md");
        let claude = ler("CLAUDE.md");
        assert!(agents.len() > 500, "AGENTS.md tem {} bytes abaixo do H1 — a régua estaria medindo o vazio", agents.len());
        if agents != claude {
            let a: Vec<&str> = agents.lines().collect();
            let c: Vec<&str> = claude.lines().collect();
            let primeira = (0..a.len().max(c.len()))
                .find(|&i| a.get(i) != c.get(i))
                .map(|i| format!("linha {} abaixo do H1:\n    AGENTS.md: {:?}\n    CLAUDE.md: {:?}",
                    i + 2, a.get(i).unwrap_or(&"<fim>"), c.get(i).unwrap_or(&"<fim>")))
                .unwrap_or_default();
            panic!("AGENTS.md e CLAUDE.md divergem abaixo do H1 — {primeira}");
        }
    }

}
