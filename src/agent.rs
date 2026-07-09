//! O loop agêntico (docs/protocolo.md §4, arquitetura §4): POST → deltas +
//! tool_calls → executa as tools LOCALMENTE → reenvia role=tool → repete até a
//! resposta final (sem tool_calls) ou o teto de voltas. Agnóstico de terminal
//! (recebe `&mut Ui`) e de rede além do `api`.

use crate::api;
use crate::config::Config;
use crate::gate::Gate;
use crate::project::Project;
use crate::telemetry::Telemetry;
use crate::tools;
use crate::types::Message;
use crate::ui::Ui;
use anyhow::Result;

const MAX_ROUNDS: usize = 25;

pub async fn run_turn(
    cfg: &Config,
    messages: &mut Vec<Message>,
    project: &Project,
    ui: &mut Ui,
    telemetry: &mut Telemetry,
    gate: &mut Gate,
) -> Result<()> {
    for _round in 0..MAX_ROUNDS {
        let result =
            api::code_chat(cfg, messages, tools::defs(), &mut |ev| ui.on_event(ev)).await?;
        ui.end_stream();

        if let Some(u) = &result.usage {
            telemetry.add(u);
        }
        if let Some(err) = result.error {
            ui.error(&err);
            return Ok(());
        }
        if result.tool_calls.is_empty() {
            ui.footer(&telemetry.footer());
            return Ok(());
        }

        // Modelo pediu ferramentas → executa cada uma LOCALMENTE (com gate) e
        // devolve os resultados (role=tool) pra próxima volta.
        messages.push(Message::assistant(result.text.clone(), result.tool_calls.clone()));
        for call in &result.tool_calls {
            let out = tools::run(call, &project.root, gate);
            ui.tool_result(call, &out);
            messages.push(Message::tool(call.id.clone(), out));
        }
    }

    ui.warn("teto de voltas atingido — refine a pergunta.");
    ui.footer(&telemetry.footer());
    Ok(())
}
