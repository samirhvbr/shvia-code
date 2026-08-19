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
use crate::types::Message;
use anyhow::Result;

const MAX_ROUNDS: usize = 25;

// ── Teto de histórico por requisição ──────────────────────────────────────────
// O gateway (`/code/chat`, SHVIA-WEB) valida `messages` com `max:200` — teto
// anti-abuso das SECURITY_GUIDELINES §3.3. Estourar não degrada: o gateway
// REJEITA a requisição inteira ("must not have more than 200 items") e a sessão
// fica PRESA — cada retry reenvia o mesmo histórico gigante (caso real de
// 18/08/2026: sessão longa no MiniMax M3, 26M tokens, sem saída além de zerar o
// chat). Compactamos client-side ANTES de cada envio: o dono do histórico é o
// anna, e o corte determinístico aqui é barato e auditável (nenhuma chamada de
// modelo para resumir).
/// Compacta em COMPACTA_EM (folga sob o teto de 200 do gateway: as voltas do
/// turno somam assistant+tools entre um trim e o próximo, e 50 de folga cobre
/// isso com sobra — o pior caso real é ~33 mensagens por volta).
const COMPACTA_EM: usize = 150;
/// Prefixo que identifica o marcador de compactação (para substituí-lo, nunca
/// acumular um por trim).
const MARCADOR: &str = "[Contexto compactado]";

/// Reduz `messages` para caber no teto do gateway, preservando:
/// - os `system` INICIAIS (o prompt-semente do agente e o contexto do projeto);
/// - as mensagens mais RECENTES, com a janela nunca começando em `role=tool`
///   (um resultado de ferramenta sem o assistant que o pediu é órfão — provedor
///   OpenAI-strict rejeita e qualquer modelo se perde);
/// - um único marcador `system` no ponto do corte, avisando o modelo do trecho
///   ausente (sem isso ele "lembra" do que não está mais lá e alucina).
///
/// Devolve quantas mensagens saíram (0 = nada a fazer).
fn compactar_historico(messages: &mut Vec<Message>) -> usize {
    // Marcador de compactação anterior sai antes da conta: ele é re-inserido no
    // corte novo (senão acumularia um marcador por trim, todos "verdadeiros").
    messages.retain(|m| !(m.role == "system" && m.content.starts_with(MARCADOR)));
    if messages.len() <= COMPACTA_EM {
        return 0;
    }

    // Cabeça = a sequência de `system` do INÍCIO (prompt-semente + contexto).
    // System no meio do histórico (ex.: nudge de loop) conta como conversa.
    let cabeca = messages.iter().take_while(|m| m.role == "system").count();

    // Janela = as mais recentes que cabem, deixando 1 vaga para o marcador.
    let orcamento = COMPACTA_EM.saturating_sub(cabeca + 1).max(1);
    let mut corte = messages.len().saturating_sub(orcamento).max(cabeca);
    // Nunca começar a janela num resultado de ferramenta órfão.
    while corte < messages.len() && messages[corte].role == "tool" {
        corte += 1;
    }

    let removidas = corte - cabeca;
    if removidas == 0 {
        return 0;
    }
    messages.drain(cabeca..corte);
    messages.insert(
        cabeca,
        Message::system(format!(
            "{MARCADOR} {removidas} mensagens antigas desta sessão saíram do contexto para \
             caber no teto por requisição do gateway. Há um trecho ausente entre as mensagens \
             acima e abaixo deste marcador — NÃO presuma o que estava nele: use as ferramentas \
             (read_file, grep, git_diff) para reverificar o que precisar."
        )),
    );
    removidas
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
    for _round in 0..MAX_ROUNDS {
        // A cada volta (as tools da volta anterior somam mensagens): o gateway
        // rejeita >200 itens, e uma sessão que estoura fica presa sem isto.
        let compactadas = compactar_historico(messages);
        if compactadas > 0 {
            fe.warn(&format!(
                "histórico compactado: {compactadas} mensagens antigas saíram do contexto \
                 (teto do gateway); o modelo foi avisado do corte."
            ));
        }
        let result =
            api::code_chat(cfg, messages, runner.tool_defs(), &mut |ev| fe.on_event(ev)).await?;
        fe.end_stream();

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
        if result.tool_calls.is_empty() {
            fe.turn_end(telemetry);
            return Ok(ultimo_texto);
        }

        // Modelo pediu ferramentas → o `runner` decide onde executam (local =
        // o anna; host = a casca externa) e devolve o resultado (role=tool).
        messages.push(Message::assistant(result.text.clone(), result.tool_calls.clone()));
        for call in &result.tool_calls {
            let out = runner.run(call, &project.root, fe);
            memory::tool_use(&call.name, &call.arguments, &out);
            messages.push(Message::tool(call.id.clone(), out));
        }
    }

    fe.warn("teto de voltas atingido — refine a pergunta.");
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
        assert!(m[1].content.starts_with(MARCADOR), "marcador no ponto do corte");
        assert_eq!(m.last().unwrap().content, ultima, "as recentes ficam");
        // A janela nunca começa num resultado de ferramenta órfão.
        assert_ne!(m[2].role, "tool", "janela não pode abrir em role=tool");
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
