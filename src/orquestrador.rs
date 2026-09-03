//! Pipeline de papéis (v0): **arquiteto → gate do dev → executor → revisão**.
//!
//! Cada fase é um `agent::run_turn` com perfil, esforço, ferramentas e system
//! prompt próprios. O que atravessa a fronteira entre fases é o ARTEFATO
//! (`plano::Plano`, `plano::Veredito`), nunca o histórico bruto — cache de
//! prompt é escopado por modelo, então repassar contexto entre papéis pagaria
//! cache frio sem ganhar nada.
//!
//! A revisão tem dois gates, e o barato vem primeiro:
//!   0. **determinístico** — clippy/testes. Grátis, exato, e pega a maior fatia
//!      do que o executor erra. Rodar um modelo num diff que nem compila é
//!      pagar pra ouvir o que o compilador já disse melhor.
//!   1. **revisor-modelo** — só se o gate 0 passou. Julga o diff CONTRA O PLANO
//!      (os `aceite` são a régua), não "revise este código": pergunta aberta não
//!      converge, sempre dá pra apontar mais uma coisa.
//!
//! Freios contra ping-pong (revisor reprova → executor "conserta" → revisor
//! reprova outra coisa → ∞): teto de voltas E dedup por achado — o mesmo achado
//! voltando depois de uma tentativa significa que insistir não resolve.
//!
//! Escopo do v0: terminal (`/plano` no REPL), ferramentas locais. O modo `--json`
//! precisa de um evento de gate pro plano no protocolo NDJSON — fica pro v1.

use crate::agent;
use crate::config::Config;
use crate::frontend::Frontend;
use crate::gate;
use crate::papel::Papel;
use crate::plano::{self, Plano, Veredito};
use crate::project::Project;
use crate::runner::{LocalRunner, SomenteLeitura, ToolRunner};
use crate::telemetry::Telemetry;
use crate::tools;
use crate::types::Message;
use crate::ui::Ui;
use anyhow::{bail, Result};
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

/// Tentativas de conserto por gate. Dois é o suficiente: se o executor não
/// resolveu em duas, mais uma volta é queima de token com o dev esperando.
const TETO_VERIFICACAO: usize = 2;
const TETO_REVISAO: usize = 2;

const SCHEMA_PLANO: &str = r#"{
  "objetivo": "uma frase: o que o conjunto de passos entrega",
  "passos": [
    {
      "id": "p1",
      "arquivo": "caminho/real/relativo.rs",
      "acao": "editar|criar|deletar",
      "o_que": "instrução autossuficiente: o que muda, onde, e por quê",
      "aceite": ["critério verificável", "outro critério verificável"]
    }
  ],
  "fora_de_escopo": ["o que NÃO deve ser tocado"]
}"#;

const SCHEMA_VEREDITO: &str = r#"{
  "aprovado": false,
  "achados": [
    {
      "passo_id": "p1",
      "classe": "desvio|escopo|plano_errado",
      "aceite_violado": "o critério do passo que não foi cumprido",
      "onde": "arquivo.rs:88",
      "o_que_fazer": "instrução acionável pro executor"
    }
  ]
}"#;

fn prompt_arquiteto() -> String {
    format!(
        "Você é o ARQUITETO do pipeline do ShvIA Code. Sua ÚNICA entrega é um PLANO em JSON.\n\n\
         Você tem apenas ferramentas de LEITURA (read_file, list_dir, grep, glob, git_status, \
         git_diff): não edita, não roda comando, não commita. Inspecione o projeto antes de \
         planejar — não invente caminho nem conteúdo, prefira poucas leituras certeiras.\n\n\
         Quem executa o plano é OUTRO modelo, em outra conversa, que NÃO vê o seu raciocínio nem \
         esta conversa. Cada `o_que` precisa se sustentar sozinho: arquivo real, trecho real, o \
         que muda e por quê, em uma ou duas frases. \"Corrigir o bug\" não é instrução.\n\n\
         Cada passo precisa de `aceite`: critérios VERIFICÁVEIS (um comando que passa, um teste \
         que roda, um comportamento observável). É a régua do revisor — critério vago faz o loop \
         de revisão nunca fechar.\n\n\
         Faça a MENOR mudança que resolve. Não projete pra requisito hipotético. Liste em \
         `fora_de_escopo` o que não deve ser tocado.\n\n\
         Responda com o JSON e NADA MAIS, neste schema:\n{SCHEMA_PLANO}"
    )
}

fn prompt_executor(p: &Plano) -> String {
    let plano_json = serde_json::to_string_pretty(p).unwrap_or_default();
    format!(
        "Você é o EXECUTOR do pipeline do ShvIA Code. Você aplica um plano que JÁ foi aprovado \
         pelo dev, um passo por mensagem.\n\n\
         Aplique EXATAMENTE o passo pedido, nada além: não refatore o que está em volta, não \
         \"melhore\" o que não foi pedido, não crie abstração nem tratamento de erro pra caso que \
         não acontece. O que está em `fora_de_escopo` não se toca.\n\n\
         Criar ou alterar arquivo é SÓ chamando edit_file/write_file — é a ferramenta que mostra o \
         diff e pede a aprovação do dev. NUNCA cole o conteúdo no chat pedindo \"posso escrever?\": \
         texto no chat não cria nada. write_file cria ou reescreve por inteiro; edit_file troca um \
         trecho EXATO de arquivo que já existe.\n\n\
         Ao terminar, responda em UMA frase o que foi feito. Se não deu pra aplicar, diga o quê e \
         por quê — nunca afirme ter feito o que não fez.\n\n\
         O plano aprovado:\n{plano_json}"
    )
}

fn prompt_revisor() -> String {
    format!(
        "Você é o REVISOR do pipeline do ShvIA Code. Sua ÚNICA entrega é um VEREDITO em JSON.\n\n\
         Você tem apenas ferramentas de LEITURA — você julga, não conserta.\n\n\
         Sua pergunta é FECHADA: o que o plano prometeu que o diff não entregou? Os `aceite` de \
         cada passo são a régua. Isto NÃO é \"revise este código\" — não aponte estilo, preferência \
         de nome nem melhoria que o plano não pediu. Crítica aberta faz o loop nunca fechar.\n\n\
         Classifique cada achado:\n\
         · `desvio`       — o diff não faz o que o passo mandou\n\
         · `escopo`       — o diff mexe no que o plano não pediu (ou no fora_de_escopo)\n\
         · `plano_errado` — o diff é fiel ao plano, mas o plano não resolve o objetivo\n\n\
         `plano_errado` PARA o pipeline e devolve pro dev — use só quando o problema for mesmo do \
         plano, nunca pra algo que o executor consegue consertar.\n\n\
         `o_que_fazer` vai direto pro executor, que NÃO vê o seu raciocínio nem esta conversa: \
         escreva instrução acionável, com arquivo e trecho.\n\n\
         Arquivo NOVO não aparece no `git diff` — use read_file no que o git status listar como \
         não rastreado.\n\n\
         Se tudo que o plano prometeu foi entregue, responda `{{\"aprovado\": true}}`.\n\n\
         Responda com o JSON e NADA MAIS, neste schema:\n{SCHEMA_VEREDITO}"
    )
}

// ── Pipeline ────────────────────────────────────────────────────────────────

pub async fn executar(
    cfg: &Config,
    project: &Project,
    ui: &mut Ui,
    tel_total: &mut Telemetry,
    pedido: &str,
) -> Result<()> {
    if cfg.elenco.vazio() {
        ui.warn(
            "elenco não configurado — os três papéis vão rodar no mesmo perfil. \
             Configure [elenco.arquiteto|executor|revisor] no config.toml.",
        );
    }

    // ── Fase 1: arquiteto (só leitura) ──────────────────────────────────────
    ui.note("── arquiteto ──────────────────────────────────────────");
    let mut tel_arq = Telemetry::default();
    let mut msgs_arq = vec![
        Message::system(format!("{}\n\n{}", prompt_arquiteto(), project.bloco_contexto())),
        Message::user(pedido),
    ];
    let plano: Plano = fase_json(
        cfg, Papel::Arquiteto, project, ui, &mut tel_arq, &mut msgs_arq, plano::parse_plano,
    )
    .await?;

    // ── Gate do dev: nada roda sem ele ver o plano ───────────────────────────
    ui.note("── plano ──────────────────────────────────────────────");
    ui.note(&plano.resumo());
    if !matches!(gate::ask("  executar este plano? [s]im / [n]ão > "), 'S' | 's') {
        ui.note("(plano descartado — nada foi alterado)");
        tel_total.absorver(&tel_arq);
        return Ok(());
    }

    // ── Fase 2: executor (ferramentas completas, gate por edição como sempre) ─
    ui.note("── executor ───────────────────────────────────────────");
    let mut tel_exec = Telemetry::default();
    let mut msgs_exec = vec![Message::system(format!(
        "{}\n\n{}",
        prompt_executor(&plano),
        project.bloco_contexto()
    ))];
    for passo in &plano.passos {
        ui.note(&format!("  [{}] {} {}", passo.id, passo.acao, passo.arquivo));
        msgs_exec.push(Message::user(format!(
            "Aplique agora, e só, o passo {}:\n{}",
            passo.id,
            serde_json::to_string_pretty(passo).unwrap_or_default()
        )));
        fase(cfg, Papel::Executor, project, ui, &mut tel_exec, &mut msgs_exec).await?;
    }

    // ── Fase 3: gates de revisão ────────────────────────────────────────────
    let mut tel_rev = Telemetry::default();
    let resultado = revisar(
        cfg, project, ui, &plano, &mut msgs_exec, &mut tel_exec, &mut tel_rev,
    )
    .await;

    ui.note("── custo por papel ────────────────────────────────────");
    ui.note(&format!("  arquiteto  {}", tel_arq.footer()));
    ui.note(&format!("  executor   {}", tel_exec.footer()));
    ui.note(&format!("  revisor    {}", tel_rev.footer()));
    tel_total.absorver(&tel_arq);
    tel_total.absorver(&tel_exec);
    tel_total.absorver(&tel_rev);

    resultado
}

/// Gate 0 (determinístico) + gate 1 (revisor-modelo), com teto e dedup.
#[allow(clippy::too_many_arguments)]
async fn revisar(
    cfg: &Config,
    project: &Project,
    ui: &mut Ui,
    plano: &Plano,
    msgs_exec: &mut Vec<Message>,
    tel_exec: &mut Telemetry,
    tel_rev: &mut Telemetry,
) -> Result<()> {
    let cmds = comandos_verificacao(cfg, &project.root);
    if cmds.is_empty() {
        ui.warn(
            "sem verificação determinística pra esta stack — o revisor-modelo vai julgar um diff \
             que ninguém compilou. Configure `verificacao = [...]` no config.toml.",
        );
    }

    let mut vistos: HashSet<String> = HashSet::new();
    let mut voltas_det = 0usize;
    let mut voltas_rev = 0usize;

    loop {
        // Gate 0 — grátis, exato, primeiro.
        if let Some(saida) = falha_verificacao(&project.root, &cmds, ui) {
            if voltas_det >= TETO_VERIFICACAO {
                ui.warn("verificação ainda falhando após 2 tentativas — parando e devolvendo pro dev.");
                bail!("verificação determinística não passou");
            }
            voltas_det += 1;
            ui.note(&format!("── conserto {voltas_det}/{TETO_VERIFICACAO} (verificação) ──"));
            msgs_exec.push(Message::user(format!(
                "A verificação do projeto falhou. Conserte SÓ o que causou a falha — não refatore \
                 nem mude escopo. Saída:\n\n{saida}"
            )));
            fase(cfg, Papel::Executor, project, ui, tel_exec, msgs_exec).await?;
            continue;
        }

        // Gate 1 — revisor-modelo, contra o plano.
        ui.note("── revisor ────────────────────────────────────────────");
        let mut msgs_rev = vec![
            Message::system(format!("{}\n\n{}", prompt_revisor(), project.bloco_contexto())),
            Message::user(entrada_revisor(plano, &project.root)),
        ];
        let veredito: Veredito = fase_json(
            cfg, Papel::Revisor, project, ui, tel_rev, &mut msgs_rev, plano::parse_veredito,
        )
        .await?;
        ui.note(&veredito.resumo());

        if veredito.aprovado {
            ui.note("✓ plano entregue e verificado.");
            return Ok(());
        }

        // Plano furado não é problema do executor — o dev decide.
        if veredito.achados.iter().any(|a| a.plano_errado()) {
            ui.warn("o revisor aponta falha no PLANO, não na execução — replanejar é decisão sua.");
            return Ok(());
        }

        // Achado reincidente: ou o executor não consegue, ou o achado está
        // errado. Nos dois casos mais uma volta não resolve.
        if veredito.achados.iter().any(|a| !vistos.insert(a.chave())) {
            ui.warn("achado reincidente — parando e devolvendo pro dev.");
            return Ok(());
        }

        if voltas_rev >= TETO_REVISAO {
            ui.warn("teto de revisões atingido — devolvendo pro dev com os achados acima.");
            return Ok(());
        }
        voltas_rev += 1;
        ui.note(&format!("── conserto {voltas_rev}/{TETO_REVISAO} (revisão) ──"));
        msgs_exec.push(Message::user(format!(
            "O revisor reprovou o resultado. Corrija SÓ os achados abaixo, nada além:\n\n{}",
            serde_json::to_string_pretty(&veredito.achados).unwrap_or_default()
        )));
        fase(cfg, Papel::Executor, project, ui, tel_exec, msgs_exec).await?;
    }
}

// ── Execução de uma fase ────────────────────────────────────────────────────

/// Uma fase = um `run_turn` com a `Config` derivada do papel e o runner que o
/// papel pode usar. Devolve o texto final do modelo.
async fn fase(
    cfg: &Config,
    papel: Papel,
    project: &Project,
    ui: &mut Ui,
    tel: &mut Telemetry,
    msgs: &mut Vec<Message>,
) -> Result<String> {
    let cfg_papel = cfg.para(papel);
    let mut runner: Box<dyn ToolRunner> = if papel.so_leitura() {
        Box::new(SomenteLeitura(Box::new(LocalRunner)))
    } else {
        Box::new(LocalRunner)
    };
    agent::run_turn(&cfg_papel, msgs, project, ui, tel, runner.as_mut()).await
}

/// Fase que precisa devolver artefato. Uma segunda tentativa com o erro de
/// validação anexado — o modelo conserta o formato quase sempre; se não
/// conserta, a fase falha ALTO em vez de deixar lixo passar pra próxima.
#[allow(clippy::too_many_arguments)]
async fn fase_json<T>(
    cfg: &Config,
    papel: Papel,
    project: &Project,
    ui: &mut Ui,
    tel: &mut Telemetry,
    msgs: &mut Vec<Message>,
    parse: fn(&str) -> Result<T, String>,
) -> Result<T> {
    let texto = fase(cfg, papel, project, ui, tel, msgs).await?;
    let erro = match parse(&texto) {
        Ok(v) => return Ok(v),
        Err(e) => e,
    };
    ui.warn(&format!("({}: resposta fora do contrato — {erro}; tentando de novo)", papel.chave()));
    msgs.push(Message::assistant(texto, vec![]));
    msgs.push(Message::user(format!(
        "Sua resposta não passou na validação: {erro}\n\nResponda AGORA só com o JSON válido, \
         sem cerca de código e sem texto em volta."
    )));
    let texto2 = fase(cfg, papel, project, ui, tel, msgs).await?;
    parse(&texto2).map_err(|e| anyhow::anyhow!("{} não devolveu artefato válido: {e}", papel.chave()))
}

// ── Entrada do revisor ──────────────────────────────────────────────────────

fn entrada_revisor(plano: &Plano, root: &Path) -> String {
    let plano_json = serde_json::to_string_pretty(plano).unwrap_or_default();
    let diff = git(root, &["diff"]);
    let status = git(root, &["status", "--short"]);
    format!(
        "PLANO APROVADO:\n{plano_json}\n\n\
         git status --short:\n{status}\n\n\
         git diff (arquivos rastreados; os NOVOS aparecem só no status acima — leia com read_file):\n{diff}"
    )
}

fn git(root: &Path, args: &[&str]) -> String {
    match Command::new("git").args(args).current_dir(root).output() {
        Ok(o) => {
            let s = String::from_utf8_lossy(&o.stdout).into_owned();
            if s.trim().is_empty() { "(vazio)".into() } else { s }
        }
        Err(e) => format!("(git indisponível: {e})"),
    }
}

// ── Gate 0: verificação determinística ──────────────────────────────────────

/// Comandos do gate 0: o `verificacao` do config.toml, senão o padrão da stack.
/// Vazio = stack não reconhecida (o pipeline avisa e segue).
fn comandos_verificacao(cfg: &Config, root: &Path) -> Vec<String> {
    if let Some(v) = &cfg.verificacao {
        return v.clone();
    }
    if root.join("Cargo.toml").exists() {
        return vec!["cargo clippy --all-targets -- -D warnings".into(), "cargo test".into()];
    }
    if tem_script_test(root, "package.json") {
        return vec!["npm test".into()];
    }
    if tem_script_test(root, "composer.json") {
        return vec!["composer test".into()];
    }
    vec![]
}

fn tem_script_test(root: &Path, manifesto: &str) -> bool {
    std::fs::read_to_string(root.join(manifesto))
        .map(|s| s.contains("\"test\""))
        .unwrap_or(false)
}

/// Roda os comandos em ordem; devolve a saída do PRIMEIRO que falhar.
///
/// Só roda o que o `bash` já aprovaria sozinho (`Policy::Auto`) — o gate não é
/// uma porta dos fundos pra rodar comando sem o dev ver. O que não passar na
/// allowlist é pulado com aviso, não executado em silêncio.
fn falha_verificacao(root: &Path, cmds: &[String], ui: &mut Ui) -> Option<String> {
    for cmd in cmds {
        if !tools::is_simple_allowlisted(cmd) {
            ui.warn(&format!(
                "verificação '{cmd}' fora da allowlist do bash — pulada (o gate não roda comando \
                 que precisaria da sua aprovação)."
            ));
            continue;
        }
        ui.note(&format!("  $ {cmd}"));
        let Ok(o) = Command::new("sh").arg("-c").arg(cmd).current_dir(root).output() else {
            ui.warn(&format!("não consegui executar '{cmd}' — pulando."));
            continue;
        };
        if o.status.success() {
            continue;
        }
        let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
        s.push_str(&String::from_utf8_lossy(&o.stderr));
        // Cauda: o erro útil do cargo/npm fica no fim, e o começo é ruído de build.
        let cauda: Vec<&str> = s.lines().rev().take(80).collect();
        let cauda = cauda.into_iter().rev().collect::<Vec<_>>().join("\n");
        return Some(format!("$ {cmd}\n{cauda}"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O gate determinístico só roda o que o `bash` já auto-aprovaria. Se alguém
    /// mexer na ALLOWLIST de tools.rs e um default de stack cair fora dela, o
    /// gate vira no-op silencioso — o diff passa direto pro revisor sem nunca
    /// ter sido compilado. Este teste é o alarme.
    #[test]
    fn padroes_de_stack_passam_na_allowlist_do_bash() {
        for cmd in [
            "cargo clippy --all-targets -- -D warnings",
            "cargo test",
            "npm test",
            "composer test",
        ] {
            assert!(
                tools::is_simple_allowlisted(cmd),
                "'{cmd}' é default de verificação mas não passa na allowlist do bash"
            );
        }
    }

    #[test]
    fn verificacao_do_config_ganha_da_deteccao_por_stack() {
        let mut cfg = Config {
            url: "https://ai.shvia.org".into(),
            profile: None,
            api_key: None,
            effort: None,
            max_voltas: crate::config::DEFAULT_MAX_VOLTAS,
            elenco: Default::default(),
            verificacao: None,
            bash_timeout: crate::config::DEFAULT_BASH_TIMEOUT_S,
        };
        // Este repo tem Cargo.toml → default de Rust.
        let raiz = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert!(comandos_verificacao(&cfg, raiz).iter().any(|c| c.starts_with("cargo clippy")));

        cfg.verificacao = Some(vec!["make test".into()]);
        assert_eq!(comandos_verificacao(&cfg, raiz), vec!["make test".to_string()]);
    }
}
