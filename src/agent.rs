//! O loop agêntico (docs/protocolo.md §4, arquitetura §4): POST → deltas +
//! tool_calls → executa as tools LOCALMENTE → reenvia role=tool → repete até a
//! resposta final (sem tool_calls) ou o teto de voltas. Agnóstico de casca:
//! recebe um `&mut dyn Frontend` (terminal ANSI ou host NDJSON) que renderiza
//! os eventos E decide as aprovações. Sem rede além do `api`.

use crate::api;
use crate::config::Config;
use crate::frontend::Frontend;
use crate::project::Project;
use crate::telemetry::Telemetry;
use crate::tools;
use crate::types::Message;
use anyhow::Result;

const MAX_ROUNDS: usize = 25;

pub async fn run_turn(
    cfg: &Config,
    messages: &mut Vec<Message>,
    project: &Project,
    fe: &mut dyn Frontend,
    telemetry: &mut Telemetry,
) -> Result<()> {
    for _round in 0..MAX_ROUNDS {
        let result =
            api::code_chat(cfg, messages, tools::defs(), &mut |ev| fe.on_event(ev)).await?;
        fe.end_stream();

        if let Some(u) = &result.usage {
            telemetry.add(u);
        }
        if let Some(err) = result.error {
            fe.error(&err);
            return Ok(());
        }
        if result.tool_calls.is_empty() {
            fe.turn_end(telemetry);
            return Ok(());
        }

        // Modelo pediu ferramentas → executa cada uma LOCALMENTE (com aprovação
        // via `fe`) e devolve os resultados (role=tool) pra próxima volta.
        messages.push(Message::assistant(result.text.clone(), result.tool_calls.clone()));
        for call in &result.tool_calls {
            let out = tools::run(call, &project.root, fe);
            fe.tool_result(call, &out);
            messages.push(Message::tool(call.id.clone(), out));
        }
    }

    fe.warn("teto de voltas atingido — refine a pergunta.");
    fe.turn_end(telemetry);
    Ok(())
}
