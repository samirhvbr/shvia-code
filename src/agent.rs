//! O loop agêntico (docs/protocolo.md §4, arquitetura §4): POST → deltas +
//! tool_calls → executa as tools LOCALMENTE → reenvia role=tool → repete até a
//! resposta final (sem tool_calls) ou o teto de voltas. Agnóstico de casca:
//! recebe um `&mut dyn Frontend` (terminal ANSI ou host NDJSON) que renderiza
//! os eventos E decide as aprovações. Sem rede além do `api`.

use crate::api;
use crate::config::Config;
use crate::frontend::Frontend;
use crate::memory;
use crate::project::Project;
use crate::runner::ToolRunner;
use crate::telemetry::Telemetry;
use crate::types::{Message, ToolCall};
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
/// Compaction starts when the history goes over COMPACTA_EM (headroom under the
/// gateway's 200: the worst real case is ~33 messages in one round).
const COMPACTA_EM: usize = 150;
/// ...and cuts down to COMPACTA_PARA, so the next cut is ~25 rounds away instead of
/// on the next round, and the prefix the provider receives stays the same between
/// cuts.
const COMPACTA_PARA: usize = 100;
/// Prefix that identifies the compaction marker (so it is replaced, never piled up
/// one per cut).
const MARCADOR: &str = "[Contexto compactado]";

fn eh_marcador(m: &Message) -> bool {
    m.role == "system" && m.content.starts_with(MARCADOR)
}

/// Shrinks `messages` to fit the gateway's ceiling, keeping:
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
/// Below COMPACTA_EM nothing changes, including the marker of an earlier cut.
/// Returns how many messages left (0 = nothing to do).
fn compactar_historico(messages: &mut Vec<Message>) -> usize {
    if messages.len() <= COMPACTA_EM {
        return 0;
    }

    // Head = the run of `system` messages at the START (seed prompt + context). A
    // `system` in the middle of the history (e.g. a loop nudge) counts as conversation.
    let cabeca = messages.iter().take_while(|m| m.role == "system" && !eh_marcador(m)).count();

    // Window = the most recent messages that fit in COMPACTA_PARA, leaving room for
    // the marker and the two kept requests.
    let orcamento = COMPACTA_PARA.saturating_sub(cabeca + 3).max(1);
    let mut corte = messages.len().saturating_sub(orcamento).max(cabeca);
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
        return 0;
    }

    let tem_pedido = messages[cabeca..].iter().any(|m| m.role == "user");
    let pedidos = if tem_pedido {
        " O primeiro pedido do usuário nesta sessão e o mais recente continuam no \
         contexto (acima deste marcador ou nas mensagens recentes)."
    } else {
        ""
    };
    let marcador = Message::system(format!(
        "{MARCADOR} {removidas} mensagens antigas desta sessão saíram do contexto para \
         caber no teto por requisição do gateway.{pedidos} Continue de onde as mensagens \
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
    removidas
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

/// Devolve o TEXTO final do turno (o que o modelo respondeu sem pedir mais
/// ferramenta). O REPL descarta; o orquestrador precisa — é dali que sai o
/// artefato JSON das fases arquiteto/revisor. Erro do gateway devolve texto
/// vazio (o `fe.error` já reportou), o que a fase que espera artefato trata
/// como "não achei JSON na resposta".
pub async fn run_turn(
    cfg: &Config,
    messages: &mut Vec<Message>,
    project: &Project,
    fe: &mut dyn Frontend,
    telemetry: &mut Telemetry,
    runner: &mut dyn ToolRunner,
) -> Result<String> {
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
    while volta < teto || fe.continuar_apos_teto(teto) {
        if volta >= teto {
            teto += lote;
        }
        volta += 1;
        // A cada volta (as tools da volta anterior somam mensagens): o gateway
        // rejeita >200 itens, e uma sessão que estoura fica presa sem isto.
        let compactadas = compactar_historico(messages);
        if compactadas > 0 {
            fe.warn(&format!(
                "histórico compactado: {compactadas} mensagens antigas saíram do contexto \
                 (teto do gateway); seus pedidos ficaram e o modelo foi avisado do corte."
            ));
        }
        let mut tentativa = 0;
        let result = loop {
            fe.waiting();
            let r = api::code_chat(cfg, messages, runner.tool_defs(), &mut |ev| fe.on_event(ev)).await?;
            fe.end_stream();
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
                    tokio::time::sleep(espera).await;
                }
                None => break r,
            }
        };

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
        for call in &result.tool_calls {
            let out = runner.run(call, &project.root, fe);
            memory::tool_use(&call.name, &call.arguments, &out);
            messages.push(Message::tool(call.id.clone(), out));
        }
    }

    fe.aviso_teto(teto);
    fe.turn_end(telemetry);
    Ok(ultimo_texto)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(compactar_historico(&mut m), 0);
        assert_eq!(m.len(), antes);
    }

    #[test]
    fn acima_do_teto_compacta_preservando_semente_e_recentes() {
        let mut m = sessao(400);
        let ultima = m.last().unwrap().content.clone();
        let removidas = compactar_historico(&mut m);
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
            compactar_historico(&mut m);
            assert!(tem(&m, "user", "corrija o parcelamento"), "request lost at round {i}");
            volta(&mut m, i);
        }
        m.push(Message::user("continua"));
        for i in 100..200 {
            compactar_historico(&mut m);
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
            if compactar_historico(&mut m) > 0 {
                cortes += 1;
                assert!(m.len() <= COMPACTA_PARA, "cut to {} (> {COMPACTA_PARA})", m.len());
                // The very next round has nothing to cut.
                volta(&mut m, i);
                assert_eq!(compactar_historico(&mut m), 0, "cut again right after a cut");
            } else {
                volta(&mut m, i);
            }
        }
        assert!((5..=12).contains(&cortes), "{cortes} cuts in 300 rounds");
    }

    #[test]
    fn abaixo_do_teto_o_marcador_de_um_corte_anterior_fica() {
        let mut m = sessao(400);
        assert!(compactar_historico(&mut m) > 0);
        volta(&mut m, 1000);
        assert_eq!(compactar_historico(&mut m), 0);
        assert_eq!(m.iter().filter(|x| eh_marcador(x)).count(), 1, "the marker must stay");
    }

    #[test]
    fn recompactar_substitui_o_marcador_sem_acumular() {
        let mut m = sessao(400);
        compactar_historico(&mut m);
        // Sessão segue crescendo e estoura de novo.
        for i in 0..300 {
            m.push(Message::user(format!("mais {i}")));
        }
        compactar_historico(&mut m);
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
            compactar_historico(&mut m);
            assert!(m.len() <= 200, "n={n} resultou em {}", m.len());
        }
    }
}
