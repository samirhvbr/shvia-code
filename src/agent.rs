//! O loop agêntico (docs/protocolo.md §4, arquitetura §4): POST → deltas +
//! tool_calls → executa as tools LOCALMENTE → reenvia role=tool → repete até a
//! resposta final (sem tool_calls) ou o teto de voltas. Agnóstico de casca:
//! recebe um `&mut dyn Frontend` (terminal ANSI ou host NDJSON) que renderiza
//! os eventos E decide as aprovações. Sem rede além do `api`.

use crate::api;
use crate::config::Config;
use crate::credencial;
use crate::estagnacao::{Estagnacao, Veredito};
use crate::frontend::Frontend;
use crate::interrupcao;
use crate::memory;
use crate::project::Project;
use crate::runner::ToolRunner;
use crate::telemetry::Telemetry;
use crate::types::{Event, Message, ToolCall, ToolDef};
use anyhow::Result;

// ── Per-request history ceiling ───────────────────────────────────────────────
// The gateway (`/code/chat`, SHVIA-WEB) validates `messages` with `max:200`, the
// anti-abuse ceiling of SECURITY_GUIDELINES §3.3. Going over does not degrade: the
// gateway REJECTS the whole request ("must not have more than 200 items") and the
// session gets STUCK, because every retry resends the same oversized history (real
// case, 18/08/2026: a long MiniMax M3 session, 26M tokens, no way out but clearing
// the chat). So the history is compacted client-side BEFORE each send: anna owns the
// history, and a deterministic cut here is cheap and auditable (no model call to
// summarise).
//
// 🔴 Until 0.13.6 the cut was a sliding window: once past 150 messages, EVERY round
// dropped the 2–4 oldest, so the person saw "histórico compactado" on every round,
// and each round's request started with a different prefix. And only the leading
// `system` messages were protected, so in a long turn the person's own request fell
// out of the context. The marker then told the model to re-verify with tools, the
// re-reads added messages, and the next round cut again. Seen on 31/10/2026 in the
// owner's Desktop session on MiniMax M3.1 Flash: a compaction notice before almost
// every tool call, the same three worktrees' `git diff` read twice each, and the
// 100-round ceiling reached at 153 tools and 4.47M tokens.
//
// 🔴 Until 0.15.0 the count was the only budget, and it said nothing about tokens. A local
// model served with `num_ctx` 32k got up to 150 messages, and the provider either cut the
// start of the prompt in silence or (Ollama with `truncate:false`) refused the request with
// a 400 that ended the turn. Since 0.15.0 the gateway's `model` event says the target's
// ceiling and window (`Limites`), the history is also budgeted in tokens, and a refusal is
// retried once with a harder cut (`run_turn`).
/// The gateway's per-request message ceiling before it announced one (and when it announces
/// one without a window: a count is then the only budget, so it stays where it was proven).
const TETO_PADRAO: usize = 200;
/// Compaction starts when the history goes over COMPACTA_EM (headroom under the
/// gateway's 200: the worst real case is ~33 messages in one round).
const COMPACTA_EM: usize = 150;
/// ...and cuts down to COMPACTA_PARA, so the next cut is ~25 rounds away instead of
/// on the next round, and the prefix the provider receives stays the same between
/// cuts. Other ceilings scale the same way (3/4 and 1/2 of the ceiling).
const COMPACTA_PARA: usize = 100;
/// Token budget against a known window: start cutting past 70% (the answer and the next
/// round's tool results need the rest), and cut down to 50%, for the same reason the count
/// cuts deep: few cuts, each one far from the next, so the prefix stays cacheable.
const JANELA_GATILHO: f64 = 0.70;
const JANELA_ALVO: f64 = 0.50;

/// The budget the history must fit before a request.
pub struct Orcamento {
    /// The message ceiling the gateway validates.
    teto_mensagens: usize,
    /// `(start cutting above, cut down to)`, in estimated tokens; `None` = window unknown.
    tokens: Option<(u64, u64)>,
    /// Calibration of the chars/4 estimate (`Telemetry::fator_tokens`).
    fator: f64,
    /// Chars sent besides the messages (tool definitions), counted in the estimate.
    extra_chars: usize,
}

impl Orcamento {
    /// Without limits from the gateway: the count ceiling it has always validated.
    #[cfg(test)]
    pub fn padrao() -> Self {
        Self { teto_mensagens: TETO_PADRAO, tokens: None, fator: 1.0, extra_chars: 0 }
    }

    pub fn para(limites: &crate::types::Limites, fator: Option<f64>, extra_chars: usize) -> Self {
        let teto_mensagens = match (limites.mensagens, limites.janela) {
            // With a window the tokens are the real budget, and the count is only the
            // gateway's validation, so it can be as high as the gateway allows.
            (Some(m), Some(_)) => m,
            (Some(m), None) => m.min(TETO_PADRAO),
            (None, _) => TETO_PADRAO,
        }
        .max(8);
        let tokens = limites
            .janela
            .filter(|&j| j > 0)
            .map(|j| ((j as f64 * JANELA_GATILHO) as u64, (j as f64 * JANELA_ALVO) as u64));
        Self { teto_mensagens, tokens, fator: fator.unwrap_or(1.0), extra_chars }
    }

    /// The harder cut after the provider refused the request: half of what was just sent.
    fn forcado(&self, messages: &[Message]) -> Self {
        let estimado = self.estimar(messages);
        let metade = (estimado / 2).max(1);
        Self {
            teto_mensagens: (messages.len() / 2).max(8),
            tokens: Some((metade, metade)),
            fator: self.fator,
            extra_chars: self.extra_chars,
        }
    }

    fn tokens_de(&self, m: &Message) -> u64 {
        let args: usize = m.tool_calls.iter().map(|c| c.name.len() + c.arguments.to_string().len()).sum();
        ((m.content.len() + args) as f64 / 4.0 * self.fator).ceil() as u64
    }

    pub fn estimar(&self, messages: &[Message]) -> u64 {
        let extra = (self.extra_chars as f64 / 4.0 * self.fator).ceil() as u64;
        extra + messages.iter().map(|m| self.tokens_de(m)).sum::<u64>()
    }
}

/// Why a cut happened, for the warning and the marker.
#[derive(Debug, PartialEq)]
enum Motivo {
    TetoDoGateway,
    JanelaDoModelo,
}

/// What a compaction did: messages dropped, tool outputs replaced by a stub, and why.
#[derive(Debug, Default)]
struct Corte {
    removidas: usize,
    podadas: usize,
    motivo: Option<Motivo>,
}

/// Prefix of a pruned tool output (so it is never pruned twice).
const PODADA: &str = "[saída antiga";
/// Outputs smaller than this are kept: a stub would save little and lose the detail.
const PODA_MINIMO: usize = 600;

/// Replaces old, large tool outputs with a one-line stub before any message is dropped
/// (0.15.1). The trail of what was done stays — each call and a line saying what came back —
/// and only the bulk goes; dropping messages loses both. Two kinds:
///
/// - a `read_file` whose file a later `edit_file`/`write_file` changed: its content is stale
///   anyway, and a model that trusts it edits against text that is no longer there;
/// - outputs older than the protected tail (`protegido` tokens counted back from the end, and
///   always the latest assistant turn with its results) and larger than `PODA_MINIMO`.
///
/// Hermes prunes the same way before summarizing, and claw-code drops superseded reads
/// (comparison of 02/11/2026). Runs only when the token budget triggers, so the prefix
/// changes rarely.
fn podar_saidas(messages: &mut [Message], cabeca: usize, orc: &Orcamento, protegido: u64) -> usize {
    use std::collections::HashMap;
    // Who asked for each result, and where: the result only carries the call id.
    let mut chamadas: HashMap<String, (usize, String, serde_json::Value)> = HashMap::new();
    for (i, m) in messages.iter().enumerate() {
        for c in &m.tool_calls {
            chamadas.insert(c.id.clone(), (i, c.name.clone(), c.arguments.clone()));
        }
    }
    let caminho = |a: &serde_json::Value| a.get("path").and_then(|p| p.as_str()).map(str::to_string);
    // The last index at which each path was written.
    let mut escrito: HashMap<String, usize> = HashMap::new();
    for (i, m) in messages.iter().enumerate() {
        for c in m.tool_calls.iter().filter(|c| c.name == "edit_file" || c.name == "write_file") {
            if let Some(p) = caminho(&c.arguments) {
                escrito.insert(p, i);
            }
        }
    }

    let ultimo_assistente =
        (cabeca..messages.len()).rev().find(|&i| messages[i].role == "assistant").unwrap_or(messages.len());
    let mut limite = ultimo_assistente;
    let mut soma = 0u64;
    while limite > cabeca {
        soma += orc.tokens_de(&messages[limite - 1]);
        if soma > protegido {
            break;
        }
        limite -= 1;
    }

    let mut podadas = 0;
    for (i, m) in messages.iter_mut().enumerate().take(ultimo_assistente).skip(cabeca) {
        if m.role != "tool" || m.content.starts_with(PODADA) {
            continue;
        }
        let Some((pedido_em, nome, args)) = m.tool_call_id.as_ref().and_then(|id| chamadas.get(id)) else {
            continue;
        };
        let velha = nome == "read_file"
            && caminho(args).and_then(|p| escrito.get(&p)).is_some_and(|&w| w > *pedido_em)
            && m.content.len() > 200;
        let antiga = i < limite && m.content.len() > PODA_MINIMO;
        let stub = if velha {
            format!(
                "{PODADA}: conteúdo de {} lido antes de o arquivo ser alterado (edit_file/write_file) — releia se precisar.]",
                caminho(args).unwrap_or_default()
            )
        } else if antiga {
            let alvo = caminho(args)
                .or_else(|| args.get("command").and_then(|c| c.as_str()).map(|c| format!("`{}`", c.chars().take(80).collect::<String>())))
                .or_else(|| args.get("pattern").and_then(|c| c.as_str()).map(|c| format!("'{}'", c.chars().take(80).collect::<String>())))
                .map(|a| format!(" {a}"))
                .unwrap_or_default();
            format!(
                "{PODADA}: {nome}{alvo} devolveu {} caracteres, removidos para caber na janela do modelo — rode de novo se precisar.]",
                m.content.len()
            )
        } else {
            continue;
        };
        m.content = stub;
        podadas += 1;
    }
    podadas
}

/// Real `prompt_tokens` ÷ the chars/4 estimate of the request that produced them, clamped
/// so one odd response (a provider that counts nothing, a huge image) cannot swing the
/// budget by orders of magnitude.
fn fator_medido(prompt_tokens: u64, chars_enviados: usize) -> Option<f64> {
    let ingenuo = chars_enviados as f64 / 4.0;
    (prompt_tokens > 0 && ingenuo >= 1.0).then(|| (prompt_tokens as f64 / ingenuo).clamp(0.3, 4.0))
}

fn chars_do_pedido(messages: &[Message], extra_chars: usize) -> usize {
    extra_chars
        + messages
            .iter()
            .map(|m| m.content.len() + m.tool_calls.iter().map(|c| c.name.len() + c.arguments.to_string().len()).sum::<usize>())
            .sum::<usize>()
}
/// Prefix that identifies the compaction marker (so it is replaced, never piled up
/// one per cut).
const MARCADOR: &str = "[Contexto compactado]";

fn eh_marcador(m: &Message) -> bool {
    m.role == "system" && m.content.starts_with(MARCADOR)
}

/// Shrinks `messages` to fit the budget (the gateway's ceiling and, when the target's window
/// is known, the tokens), keeping:
/// - the INITIAL `system` messages (the agent's seed prompt and project context);
/// - the person's requests that anchor the work: the FIRST `user` message of the
///   session and the LATEST one. A turn that the person resumed with "continua", or
///   that the Run continued with the orchestrator's message, would otherwise keep
///   only the continuation and lose what it is continuing;
/// - the most RECENT messages, with the window never starting at `role=tool` (a tool
///   result without the assistant that asked for it is an orphan: an OpenAI-strict
///   provider rejects it and any model gets lost);
/// - a single `system` marker at the cut, telling the model a stretch is missing
///   (without it, the model "remembers" what is no longer there and hallucinates).
///
/// Below both triggers nothing changes, including the marker of an earlier cut.
/// When the tokens triggered, old tool outputs are pruned first (`podar_saidas`), and
/// messages are dropped only if that was not enough.
fn compactar_historico(messages: &mut Vec<Message>, orc: &Orcamento) -> Corte {
    let gatilho = orc.teto_mensagens * COMPACTA_EM / TETO_PADRAO;
    let alvo = orc.teto_mensagens * COMPACTA_PARA / TETO_PADRAO;
    let por_contagem = messages.len() > gatilho;
    let mut por_tokens = orc.tokens.filter(|&(acima, _)| orc.estimar(messages) > acima).map(|(_, alvo)| alvo);
    if !por_contagem && por_tokens.is_none() {
        return Corte::default();
    }

    // Head = the run of `system` messages at the START (seed prompt + context). A
    // `system` in the middle of the history (e.g. a loop nudge) counts as conversation.
    let cabeca = messages.iter().take_while(|m| m.role == "system" && !eh_marcador(m)).count();

    let mut podadas = 0;
    if let Some(alvo_tokens) = por_tokens {
        // The recent outputs (40% of the target, ~20% of the window) stay whole.
        podadas = podar_saidas(messages, cabeca, orc, alvo_tokens * 2 / 5);
        if orc.estimar(messages) <= alvo_tokens {
            por_tokens = None;
            if !por_contagem {
                return Corte { removidas: 0, podadas, motivo: Some(Motivo::JanelaDoModelo) };
            }
        }
    }

    // Window = the most recent messages that fit in the count target, leaving room for
    // the marker and the two kept requests...
    let orcamento = alvo.saturating_sub(cabeca + 3).max(1);
    let mut corte = if por_contagem { messages.len().saturating_sub(orcamento) } else { cabeca }.max(cabeca);
    // ...and, when the tokens triggered, in the token target: walk back from the end while
    // the head, the two kept requests and the marker still leave room.
    if let Some(alvo_tokens) = por_tokens {
        let pedido = |de_tras: bool| {
            let mut it = (cabeca..messages.len()).filter(|&i| messages[i].role == "user");
            if de_tras { it.next_back() } else { it.next() }
        };
        let fixos: u64 = messages[..cabeca].iter().map(|m| orc.tokens_de(m)).sum::<u64>()
            + [pedido(false), pedido(true)].into_iter().flatten().map(|i| orc.tokens_de(&messages[i])).sum::<u64>()
            + orc.estimar(&[]) // the tool definitions
            + 150; // the marker
        let mut soma = fixos;
        let mut i = messages.len();
        while i > cabeca {
            let t = orc.tokens_de(&messages[i - 1]);
            if soma + t > alvo_tokens {
                break;
            }
            soma += t;
            i -= 1;
        }
        // The token cut never takes the latest assistant turn and its tool results: they
        // are what the model is about to act on, however large (a single huge result is
        // the gateway's to trim). The count cut keeps its own rule, which bounds the count.
        if let Some(ultimo) = (cabeca..messages.len()).rev().find(|&j| messages[j].role == "assistant") {
            i = i.min(ultimo);
        }
        corte = corte.max(i);
    }
    // Never open the window on an orphan tool result.
    while corte < messages.len() && messages[corte].role == "tool" {
        corte += 1;
    }

    let primeiro_pedido = (cabeca..corte).find(|&i| messages[i].role == "user");
    let ultimo_pedido = (cabeca..messages.len()).rev().find(|&i| messages[i].role == "user");
    let mut mantidos: Vec<usize> =
        [primeiro_pedido, ultimo_pedido].into_iter().flatten().filter(|&i| i < corte).collect();
    mantidos.dedup();

    let removidas = (cabeca..corte)
        .filter(|&i| !eh_marcador(&messages[i]) && !mantidos.contains(&i))
        .count();
    if removidas == 0 {
        let motivo = (podadas > 0).then_some(Motivo::JanelaDoModelo);
        return Corte { removidas: 0, podadas, motivo };
    }
    let motivo = if por_tokens.is_some() { Motivo::JanelaDoModelo } else { Motivo::TetoDoGateway };
    let onde = match motivo {
        Motivo::TetoDoGateway => "no teto por requisição do gateway",
        Motivo::JanelaDoModelo => "na janela de contexto do modelo",
    };

    let tem_pedido = messages[cabeca..].iter().any(|m| m.role == "user");
    let pedidos = if tem_pedido {
        " O primeiro pedido do usuário nesta sessão e o mais recente continuam no \
         contexto (acima deste marcador ou nas mensagens recentes)."
    } else {
        ""
    };
    let marcador = Message::system(format!(
        "{MARCADOR} {removidas} mensagens antigas desta sessão saíram do contexto para \
         caber {onde}.{pedidos} Continue de onde as mensagens \
         recentes param. NÃO presuma o que estava no trecho ausente, mas também não refaça \
         o que as mensagens recentes já mostram: use as ferramentas (read_file, grep, \
         git_diff) só para o que o próximo passo precisa e não está aqui."
    ));

    let mut janela: Vec<Message> = messages.drain(corte..).filter(|m| !eh_marcador(m)).collect();
    let pedidos_mantidos: Vec<Message> = mantidos.iter().map(|&i| messages[i].clone()).collect();
    messages.truncate(cabeca);
    messages.extend(pedidos_mantidos);
    messages.push(marcador);
    messages.append(&mut janela);
    Corte { removidas, podadas, motivo: Some(motivo) }
}

/// The gateway's code for a request the provider refused as such (4xx), `CodigoDeErro` in
/// SHVIA-WEB. A context overflow is the case that matters here.
const PROVEDOR_RECUSOU: &str = "provedor_recusou_pedido";

/// How long a 401 waits for the host's new key (0.16.0). A host renews it over the network
/// (SHVIA-DESKTOP asks the web session for a new Code token), which takes a second or two;
/// one that never answers — an older host — costs this much before the 401 is reported.
const CREDENCIAL_ESPERA: std::time::Duration = std::time::Duration::from_secs(60);

/// Waits for a key newer than generation `antes`. `false` on timeout or when the turn is
/// stopped, and then the 401 is reported as before.
async fn esperar_credencial(antes: u64) -> bool {
    let ate = tokio::time::Instant::now() + CREDENCIAL_ESPERA;
    while tokio::time::Instant::now() < ate {
        if credencial::geracao() != antes {
            return true;
        }
        if interrupcao::pedido() {
            return false;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    credencial::geracao() != antes
}

fn avisar_corte(fe: &mut dyn Frontend, c: &Corte) {
    let Some(motivo) = &c.motivo else { return };
    let onde = match motivo {
        Motivo::TetoDoGateway => "teto do gateway",
        Motivo::JanelaDoModelo => "janela de contexto do modelo",
    };
    let mut partes = Vec::new();
    if c.podadas > 0 {
        partes.push(format!("{} saídas antigas de ferramenta viraram resumo de uma linha", c.podadas));
    }
    if c.removidas > 0 {
        partes.push(format!("{} mensagens antigas saíram do contexto", c.removidas));
    }
    if partes.is_empty() {
        return;
    }
    fe.warn(&format!(
        "histórico compactado ({onde}): {}; seus pedidos ficaram e o modelo foi avisado.",
        partes.join(" e ")
    ));
}

/// Appends the model's response to the history, once, whether or not it asks for tools.
///
/// An empty final answer (no text, no tool calls) is left out: it carries nothing for the next
/// request to remember, and the gateway's OpenAI-compatible driver would send it as
/// `{"role":"assistant","content":""}` in the middle of the history, which some providers
/// reject. The gateway's Anthropic driver already drops it.
fn registrar_resposta(messages: &mut Vec<Message>, text: &str, tool_calls: &[ToolCall]) {
    if text.is_empty() && tool_calls.is_empty() {
        return;
    }
    messages.push(Message::assistant(text, tool_calls.to_vec()));
}

/// Closes the history when the person stops the turn while an answer was streaming (0.14.0).
///
/// Whatever was already on screen stays, marked as cut, so the next request tells the model
/// what it had said and that it was stopped. With nothing shown, the note alone keeps the
/// history alternating: two `user` messages in a row are refused by some providers. Tool
/// calls of a cut-off answer are dropped and never run.
fn registrar_interrupcao(messages: &mut Vec<Message>, parcial: &str) {
    let content = if parcial.trim().is_empty() {
        format!("{INTERROMPIDO} (antes da resposta)")
    } else {
        format!("{parcial}\n\n{INTERROMPIDO}")
    };
    messages.push(Message::assistant(content, vec![]));
}

const INTERROMPIDO: &str = "[interrompido pelo usuário]";

/// The result of a tool call the stop kept from running. Every id in an assistant's
/// `tool_calls` needs its `role=tool` answer, or the next request carries an orphan call that
/// strict providers reject.
pub const NAO_EXECUTADA: &str = "RECUSADO: o usuário interrompeu o turno antes desta ferramenta rodar.";

/// Prefix of the messages `anna` itself writes into a request (not the person's words).
const ANNA: &str = "[anna]";

/// One more request for the last word of a turn that cannot go on (0.15.2): the model writes,
/// without running tools, what it did, what is left and the next step, so the turn ends with an
/// answer instead of being cut mid-work. Before, the round ceiling ended the turn with whatever
/// the last round said (usually nothing: that round had asked for tools), and the person or the
/// Run had to say "continua" with no idea where it stood.
///
/// Hermes does the same at its iteration cap (`handle_max_iterations`, comparison of 02/11/2026)
/// and keeps the tools in the request on purpose: dropping them or forcing `tool_choice: none`
/// changes the prompt the provider renders, which throws away the cached prefix (and the
/// gateway's Ollama driver ignores `tool_choice` anyway). So the request carries the same tools,
/// and a tool call in the answer is neither run nor shown.
///
/// The asking message does not stay in the history: compaction keeps the latest `user` message
/// as the person's request, and this one is not the person's. The summary stays, as the
/// assistant's last word of the turn. A failure here only costs the summary.
async fn volta_de_fechamento(
    cfg: &Config,
    messages: &mut Vec<Message>,
    defs: Vec<ToolDef>,
    fe: &mut dyn Frontend,
    telemetry: &mut Telemetry,
    pedido: String,
) -> String {
    let defs_chars = serde_json::to_string(&defs).map(|s| s.len()).unwrap_or(0);
    let orc = Orcamento::para(&telemetry.limites, telemetry.fator_tokens, defs_chars);
    avisar_corte(fe, &compactar_historico(messages, &orc));
    messages.push(Message::user(pedido));
    fe.waiting();
    let resposta = api::code_chat(cfg, messages, defs, &mut |ev| {
        if !matches!(ev, Event::ToolCall(_)) {
            fe.on_event(ev);
        }
    })
    .await;
    fe.end_stream();
    messages.pop();
    let r = match resposta {
        Ok(r) => r,
        Err(e) => {
            fe.warn(&format!("sem o resumo de fechamento: {e}"));
            return String::new();
        }
    };
    if let Some(u) = &r.usage {
        telemetry.add(u);
    }
    if let Some(e) = r.error.as_deref().filter(|_| !r.interrompido) {
        fe.warn(&format!("sem o resumo de fechamento: {e}"));
    }
    if r.interrompido || r.error.is_some() {
        return String::new();
    }
    registrar_resposta(messages, &r.text, &[]);
    r.text
}

/// Devolve o TEXTO final do turno (o que o modelo respondeu sem pedir mais
/// ferramenta). O REPL descarta; o orquestrador precisa — é dali que sai o
/// artefato JSON das fases arquiteto/revisor. Erro do gateway devolve texto
/// vazio (o `fe.error` já reportou), o que a fase que espera artefato trata
/// como "não achei JSON na resposta".
///
/// `ao_fim_da_volta` runs after every round that ran tools and when the turn is stopped: the
/// REPL saves the session there, so a long turn is not lost with the process (0.14.0). Before,
/// the session was saved only after the turn.
pub async fn run_turn(
    cfg: &Config,
    messages: &mut Vec<Message>,
    project: &Project,
    fe: &mut dyn Frontend,
    telemetry: &mut Telemetry,
    runner: &mut dyn ToolRunner,
    ao_fim_da_volta: &mut dyn FnMut(&[Message], &Telemetry),
) -> Result<String> {
    // A stop (Ctrl-C, or `cancel` from a host) ends this turn, never the process.
    let _turno = interrupcao::Turno::comecar();
    // Captura opcional (ai-memory): o prompt que abre o turno. No-op sem env.
    if let Some(last) = messages.iter().rev().find(|m| m.role == "user") {
        memory::user_prompt(&last.content);
    }
    let mut ultimo_texto = String::new();
    // Teto de voltas por turno (SECURITY §2.7, default em config::DEFAULT_MAX_VOLTAS):
    // o loop é SEMPRE limitado — estourou, quem decide se vale mais um lote é o
    // dev (Frontend::continuar_apos_teto), nunca o modelo. `.max(1)` é cinto de
    // segurança para Config montada na mão (o `load()` já garante ≥ 1).
    let lote = cfg.max_voltas.max(1);
    let mut teto = lote;
    let mut volta = 0usize;
    // The same call with the same result, again and again (0.15.3): told at 3, stopped at 5.
    let mut estagnacao = Estagnacao::default();
    while volta < teto || fe.continuar_apos_teto(teto) {
        if volta >= teto {
            teto += lote;
        }
        volta += 1;
        if interrupcao::pedido() {
            fe.turno_interrompido();
            fe.turn_end(telemetry);
            return Ok(String::new());
        }
        // A cada volta (as tools da volta anterior somam mensagens): o gateway
        // rejeita >200 itens, e uma sessão que estoura fica presa sem isto. Since 0.15.0
        // also against the target's window, when the gateway said it.
        let defs = runner.tool_defs();
        let defs_chars = serde_json::to_string(&defs).map(|s| s.len()).unwrap_or(0);
        let orc = Orcamento::para(&telemetry.limites, telemetry.fator_tokens, defs_chars);
        avisar_corte(fe, &compactar_historico(messages, &orc));
        let mut tentativa = 0;
        let mut recuperou = false;
        let mut pediu_credencial = false;
        let result = loop {
            fe.waiting();
            let enviados = chars_do_pedido(messages, defs_chars);
            let r = api::code_chat(cfg, messages, defs.clone(), &mut |ev| fe.on_event(ev)).await?;
            fe.end_stream();
            if r.interrompido {
                break r;
            }
            if r.limites != Default::default() {
                telemetry.limites = r.limites;
            }
            if let Some(f) = r.usage.as_ref().and_then(|u| u.prompt_tokens).and_then(|p| fator_medido(p, enviados)) {
                telemetry.fator_tokens = Some(f);
            }
            // The provider refused the request itself (Ollama answers 400 to a prompt over
            // its `num_ctx`; the gateway classifies it, never by the provider's wording). The
            // usual cause in a long turn is the context, so cut harder and try once more; any
            // other cause fails the same way again and is reported as before.
            if r.codigo.as_deref() == Some(PROVEDOR_RECUSOU) && !recuperou {
                recuperou = true;
                let forcado = Orcamento::para(&telemetry.limites, telemetry.fator_tokens, defs_chars).forcado(messages);
                let corte = compactar_historico(messages, &forcado);
                if corte.removidas + corte.podadas > 0 {
                    fe.warn(&format!(
                        "o provedor recusou o pedido ({}) — compactando o histórico e tentando de novo.",
                        r.error.as_deref().unwrap_or("sem detalhe")
                    ));
                    avisar_corte(fe, &corte);
                    continue;
                }
            }
            // The key was refused (0.16.0). A host that hands out short-lived keys can send a
            // new one; the request never reached the model, so sending it again repeats nothing.
            // Once per round: a second 401 with a fresh key is not an expired key.
            if r.codigo.as_deref() == Some(api::NAO_AUTORIZADO) && !pediu_credencial {
                pediu_credencial = true;
                let antes = credencial::geracao();
                if fe.pedir_credencial() && esperar_credencial(antes).await {
                    fe.warn("credencial renovada pela casca — repetindo o pedido.");
                    continue;
                }
            }
            // A failure before the stream opened (429 from the per-key throttle, the gateway
            // briefly down) is waited out instead of ending the turn; nothing was shown or
            // run, so repeating the request repeats nothing the person has seen.
            match api::nova_tentativa(&r, tentativa) {
                Some(espera) => {
                    tentativa += 1;
                    fe.warn(&format!(
                        "nova tentativa em {} s ({tentativa} de {}) — {}",
                        espera.as_secs() + u64::from(espera.subsec_nanos() > 0),
                        api::MAX_TENTATIVAS,
                        r.error.as_deref().unwrap_or("falha antes da resposta"),
                    ));
                    // A stop during the wait makes the next request return at once.
                    tokio::select! {
                        _ = tokio::time::sleep(espera) => {}
                        _ = interrupcao::esperar() => {}
                    }
                }
                None => break r,
            }
        };

        if result.interrompido {
            registrar_interrupcao(messages, &result.text);
            ao_fim_da_volta(messages, telemetry);
            fe.turno_interrompido();
            fe.turn_end(telemetry);
            return Ok(String::new());
        }
        if let Some(u) = &result.usage {
            telemetry.add(u);
        }
        if let Some(m) = &result.model {
            telemetry.model = Some(m.clone());
        }
        if let Some(err) = result.error {
            fe.error(&err);
            return Ok(String::new());
        }
        ultimo_texto = result.text.clone();
        registrar_resposta(messages, &result.text, &result.tool_calls);
        if result.tool_calls.is_empty() {
            fe.turn_end(telemetry);
            return Ok(ultimo_texto);
        }

        // Modelo pediu ferramentas → o `runner` decide onde executam (local =
        // o anna; host = a casca externa) e devolve o resultado (role=tool).
        // After a stop, the calls not yet run are answered without running.
        let mut parada = None;
        for call in &result.tool_calls {
            let out = if interrupcao::pedido() {
                fe.tool_result(call, NAO_EXECUTADA);
                NAO_EXECUTADA.to_string()
            } else {
                let mut out = runner.run(call, &project.root, fe);
                memory::tool_use(&call.name, &call.arguments, &out);
                match estagnacao.registrar(&call.name, &call.arguments, &out) {
                    Veredito::Segue => {}
                    // To the model, next to the output it keeps asking for; and said to the person.
                    Veredito::Avisar(nota) => {
                        fe.warn(&format!("o modelo está se repetindo — {}", nota.trim()));
                        out.push_str(&nota);
                    }
                    Veredito::Parar(o_que) => parada = Some(o_que),
                }
                // A subfolder with rules of its own, the first time a tool goes there (0.15.6).
                if let Some(nota) = call.arguments.get("path").and_then(|p| p.as_str()).and_then(|p| project.nota_de_instrucoes(p)) {
                    out.push_str(&nota);
                }
                out
            };
            messages.push(Message::tool(call.id.clone(), out));
        }
        ao_fim_da_volta(messages, telemetry);
        if interrupcao::pedido() {
            fe.turno_interrompido();
            fe.turn_end(telemetry);
            return Ok(String::new());
        }
        if let Some(o_que) = parada {
            if fe.continuar_apos_estagnacao(&o_que) {
                estagnacao.zerar();
            } else {
                fe.warn(&format!("turno parado: o modelo repetiu {o_que}."));
                let pedido = format!(
                    "{ANNA} Este turno vai parar aqui: você repetiu {o_que}. NÃO chame ferramentas nesta \
                     resposta (se chamar, ela não roda). Em poucas linhas, para o dev: o que você tentou, por \
                     que não avançou e o que precisa dele para seguir."
                );
                let resumo = volta_de_fechamento(cfg, messages, runner.tool_defs(), fe, telemetry, pedido).await;
                if interrupcao::pedido() {
                    fe.turno_interrompido();
                }
                fe.turn_end(telemetry);
                return Ok(if resumo.is_empty() { ultimo_texto } else { resumo });
            }
        }
    }

    // The ceiling with nobody granting another batch: one more request for the turn's last word.
    let pedido = format!(
        "{ANNA} Este turno chegou ao teto de {teto} voltas e vai parar aqui. NÃO chame ferramentas nesta \
         resposta (se chamar, ela não roda). Em poucas linhas, para o dev: o que já foi feito, o que \
         falta e qual seria o próximo passo. Ele decide se continua."
    );
    let resumo = volta_de_fechamento(cfg, messages, runner.tool_defs(), fe, telemetry, pedido).await;
    if !resumo.is_empty() {
        ultimo_texto = resumo;
    }
    if interrupcao::pedido() {
        fe.turno_interrompido();
    } else {
        fe.aviso_teto(teto);
    }
    fe.turn_end(telemetry);
    Ok(ultimo_texto)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cut as it ran before 0.15.0: no limits from the gateway, count only.
    fn compacta(m: &mut Vec<Message>) -> usize {
        compactar_historico(m, &Orcamento::padrao()).removidas
    }

    #[test]
    fn com_a_janela_conhecida_o_corte_e_por_tokens_e_guarda_o_que_importa() {
        use crate::types::Limites;
        // Without a window the count stays the budget, even if the gateway allows more.
        assert_eq!(Orcamento::para(&Limites { mensagens: Some(5000), janela: None }, None, 0).teto_mensagens, 200);
        assert_eq!(Orcamento::para(&Limites { mensagens: Some(5000), janela: Some(32_000) }, None, 0).teto_mensagens, 5000);

        // 120 rounds of 500-char tool outputs (small enough that pruning leaves them alone)
        // against a 16k window: far under the 3 750-message count, over the 70% token trigger.
        let orc = Orcamento::para(&Limites { mensagens: Some(5000), janela: Some(16_000) }, Some(1.0), 0);
        let mut m = vec![Message::system("prompt-semente"), Message::user("corrija o parcelamento")];
        for i in 0..120 {
            m.push(Message::assistant(format!("lendo {i}"), vec![]));
            m.push(Message::tool(format!("id-{i}"), "x".repeat(500)));
        }
        m.push(Message::user("continua"));
        m.push(Message::assistant("lendo de novo", vec![]));
        m.push(Message::tool("id-final", "y".repeat(500)));
        assert!(orc.estimar(&m) > 11_200, "the fixture must start over the trigger");

        let corte = compactar_historico(&mut m, &orc);
        assert!(corte.removidas > 0);
        assert_eq!(corte.motivo, Some(Motivo::JanelaDoModelo));
        assert!(orc.estimar(&m) <= 8_000, "cut down to the 50% target, got {}", orc.estimar(&m));
        assert!(tem(&m, "user", "corrija o parcelamento") && tem(&m, "user", "continua"));
        assert_eq!(m.last().unwrap().tool_call_id.as_deref(), Some("id-final"), "the latest round stays");
        let marcador = m.iter().find(|x| eh_marcador(x)).unwrap();
        assert!(marcador.content.contains("janela de contexto do modelo"), "{}", marcador.content);
        // The very next round has nothing to cut: the target sits well under the trigger.
        assert_eq!(compactar_historico(&mut m, &orc).removidas, 0);
    }

    #[test]
    fn saida_velha_vira_resumo_antes_de_qualquer_mensagem_sair() {
        use crate::types::{Limites, ToolCall};
        let chamada = |id: &str, nome: &str, path: &str| ToolCall {
            id: id.into(),
            name: nome.into(),
            arguments: serde_json::json!({ "path": path }),
        };
        // 30 reads of 2 000 chars (~500 tokens each) against a 16k window: over the trigger.
        let orc = Orcamento::para(&Limites { mensagens: Some(5000), janela: Some(16_000) }, Some(1.0), 0);
        let mut m = vec![Message::system("prompt-semente"), Message::user("corrija o parcelamento")];
        for i in 0..30 {
            m.push(Message::assistant("", vec![chamada(&format!("r{i}"), "read_file", &format!("src/f{i}.rs"))]));
            m.push(Message::tool(format!("r{i}"), "x".repeat(2000)));
        }
        // A recent read of src/f29.rs, then an edit of it: that read is stale.
        m.push(Message::assistant("", vec![chamada("e1", "edit_file", "src/f29.rs")]));
        m.push(Message::tool("e1", "ok (1 hunks)"));
        m.push(Message::assistant("", vec![chamada("last", "read_file", "src/g.rs")]));
        m.push(Message::tool("last", "z".repeat(2000)));
        let antes = m.len();

        let corte = compactar_historico(&mut m, &orc);
        assert_eq!(corte.removidas, 0, "pruning was enough: no message leaves");
        assert_eq!(m.len(), antes);
        assert!(corte.podadas > 20, "{corte:?}");
        assert!(orc.estimar(&m) <= 8_000, "down to the 50% target, got {}", orc.estimar(&m));
        let conteudo = |id: &str| m.iter().find(|x| x.tool_call_id.as_deref() == Some(id)).unwrap().content.clone();
        assert!(conteudo("r0").starts_with(PODADA) && conteudo("r0").contains("read_file src/f0.rs"), "{}", conteudo("r0"));
        assert!(conteudo("r29").contains("lido antes de o arquivo ser alterado"), "a stale read goes even when recent: {}", conteudo("r29"));
        assert_eq!(conteudo("r28"), "x".repeat(2000), "recent outputs stay whole");
        assert_eq!(conteudo("last"), "z".repeat(2000), "the latest round stays whole");
        // Pruning twice changes nothing, so the prefix stays stable between cuts.
        let depois = m.clone();
        compactar_historico(&mut m, &orc);
        assert_eq!(m.iter().map(|x| &x.content).collect::<Vec<_>>(), depois.iter().map(|x| &x.content).collect::<Vec<_>>());
    }

    fn sessao(n_conversa: usize) -> Vec<Message> {
        // [system-semente] + alternância user/assistant/tool como numa sessão real.
        let mut m = vec![Message::system("prompt-semente")];
        for i in 0..n_conversa {
            match i % 4 {
                0 => m.push(Message::user(format!("pergunta {i}"))),
                1 => m.push(Message::assistant(format!("chamando tool {i}"), vec![])),
                2 => m.push(Message::tool(format!("id-{i}"), format!("saida {i}"))),
                _ => m.push(Message::assistant(format!("resposta {i}"), vec![])),
            }
        }
        m
    }

    #[test]
    fn a_resposta_entra_no_historico_e_a_vazia_fica_de_fora() {
        let chamada = ToolCall { id: "c1".into(), name: "read_file".into(), arguments: serde_json::json!({}) };
        let mut m = vec![Message::user("pedido")];
        registrar_resposta(&mut m, "", &[]);
        assert_eq!(m.len(), 1, "an empty final answer must not become an empty assistant turn");
        registrar_resposta(&mut m, "", std::slice::from_ref(&chamada));
        assert_eq!((m.len(), m[1].tool_calls.len()), (2, 1), "a tool call with no text still goes in");
        registrar_resposta(&mut m, "resposta final", &[]);
        assert_eq!((m[2].role.as_str(), m[2].content.as_str()), ("assistant", "resposta final"));
    }

    #[test]
    fn abaixo_do_teto_nao_mexe() {
        let mut m = sessao(100);
        let antes = m.len();
        assert_eq!(compacta(&mut m), 0);
        assert_eq!(m.len(), antes);
    }

    #[test]
    fn acima_do_teto_compacta_preservando_semente_e_recentes() {
        let mut m = sessao(400);
        let ultima = m.last().unwrap().content.clone();
        let removidas = compacta(&mut m);
        assert!(removidas > 0);
        assert!(m.len() <= COMPACTA_EM, "ficou com {} (> {COMPACTA_EM})", m.len());
        assert_eq!(m[0].content, "prompt-semente", "system-semente tem que sobreviver");
        assert_eq!(m[1].content, "pergunta 0", "the session's first request stays");
        let marcador = m.iter().position(eh_marcador).expect("marker at the cut");
        assert_eq!(m.last().unwrap().content, ultima, "as recentes ficam");
        // A janela nunca começa num resultado de ferramenta órfão.
        assert_ne!(m[marcador + 1].role, "tool", "janela não pode abrir em role=tool");
    }

    /// One round of a turn, as `run_turn` appends it: the assistant asking for a
    /// tool, then the tool's result.
    fn volta(m: &mut Vec<Message>, i: usize) {
        m.push(Message::assistant(format!("chamando tool {i}"), vec![]));
        m.push(Message::tool(format!("id-{i}"), format!("saida {i}")));
    }

    fn tem(m: &[Message], role: &str, content: &str) -> bool {
        m.iter().any(|x| x.role == role && x.content == content)
    }

    #[test]
    fn o_pedido_sobrevive_a_um_turno_longo_e_ao_continua() {
        // The 31/10/2026 case: one request, a long turn, the 100-round ceiling,
        // "continua", and another long turn. Compaction runs before every round, as
        // in `run_turn`. Before 0.13.6 the request left the context at ~75 rounds.
        let mut m = vec![Message::system("prompt-semente"), Message::user("corrija o parcelamento")];
        for i in 0..100 {
            compacta(&mut m);
            assert!(tem(&m, "user", "corrija o parcelamento"), "request lost at round {i}");
            volta(&mut m, i);
        }
        m.push(Message::user("continua"));
        for i in 100..200 {
            compacta(&mut m);
            assert!(m.len() <= 200, "round {i} would send {} messages", m.len());
            assert!(tem(&m, "user", "corrija o parcelamento"), "request lost at round {i}");
            assert!(tem(&m, "user", "continua"), "the latest request lost at round {i}");
            volta(&mut m, i);
        }
        assert_eq!(m[0].content, "prompt-semente");
        assert_eq!(m.iter().filter(|x| eh_marcador(x)).count(), 1, "one marker");
    }

    #[test]
    fn compacta_de_vez_em_quando_nao_a_cada_volta() {
        // 300 rounds of 2 messages each. A sliding window cut on every round past
        // 150 messages (~225 notices); cutting down to COMPACTA_PARA leaves ~25
        // rounds between cuts: the first at round ~75, then 8 more.
        let mut m = vec![Message::system("prompt-semente"), Message::user("pedido")];
        let mut cortes = 0;
        for i in 0..300 {
            if compacta(&mut m) > 0 {
                cortes += 1;
                assert!(m.len() <= COMPACTA_PARA, "cut to {} (> {COMPACTA_PARA})", m.len());
                // The very next round has nothing to cut.
                volta(&mut m, i);
                assert_eq!(compacta(&mut m), 0, "cut again right after a cut");
            } else {
                volta(&mut m, i);
            }
        }
        assert!((5..=12).contains(&cortes), "{cortes} cuts in 300 rounds");
    }

    #[test]
    fn abaixo_do_teto_o_marcador_de_um_corte_anterior_fica() {
        let mut m = sessao(400);
        assert!(compacta(&mut m) > 0);
        volta(&mut m, 1000);
        assert_eq!(compacta(&mut m), 0);
        assert_eq!(m.iter().filter(|x| eh_marcador(x)).count(), 1, "the marker must stay");
    }

    #[test]
    fn recompactar_substitui_o_marcador_sem_acumular() {
        let mut m = sessao(400);
        compacta(&mut m);
        // Sessão segue crescendo e estoura de novo.
        for i in 0..300 {
            m.push(Message::user(format!("mais {i}")));
        }
        compacta(&mut m);
        let marcadores = m
            .iter()
            .filter(|x| x.role == "system" && x.content.starts_with(MARCADOR))
            .count();
        assert_eq!(marcadores, 1, "um único marcador por sessão");
        assert!(m.len() <= COMPACTA_EM);
    }

    #[test]
    fn nunca_envia_acima_do_teto_do_gateway() {
        // Propriedade de contrato: para qualquer tamanho, o resultado cabe no
        // que o /code/chat aceita (max:200) — com folga.
        for n in [149, 150, 151, 199, 200, 201, 500, 2000] {
            let mut m = sessao(n);
            compacta(&mut m);
            assert!(m.len() <= 200, "n={n} resultou em {}", m.len());
        }
    }
}
