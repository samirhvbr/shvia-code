//! O que o loop precisa do "mundo": renderizar os eventos do turno e DECIDIR
//! aprovações. Abstrair isso num trait é o que deixa o motor **embutível** —
//! o mesmo `agent::run_turn` serve o terminal (`Ui`, ANSI+stdin) e um host
//! externo (`JsonSink`, NDJSON — ex.: a aba Agente do SShvTerm). Quem mostra o
//! preview é quem pergunta: por isso `approve` mora aqui, não num gate à parte.
//! Protocolo NDJSON: docs/embedding.md.

use crate::gate::{Policy, Preview};
use crate::telemetry::Telemetry;
use crate::types::{Event, ToolCall};

pub trait Frontend {
    /// O agente vai esperar o modelo (antes de cada volta). O terminal mostra um spinner; o
    /// host NDJSON não precisa de nada, então o padrão é não fazer nada.
    fn waiting(&mut self) {}
    /// Evento ao vivo do stream (model / text_delta / tool_call).
    fn on_event(&mut self, ev: &Event);
    /// Fecha o bloco de texto da volta.
    fn end_stream(&mut self);
    /// Resultado de uma ferramenta executada localmente.
    fn tool_result(&mut self, call: &ToolCall, out: &str);
    /// Fim do turno: telemetria (tokens/custo).
    fn turn_end(&mut self, tel: &Telemetry);
    fn error(&mut self, msg: &str);
    fn warn(&mut self, msg: &str);
    /// Mostra o `preview` e decide se a ação roda (o gate). `Auto` nunca pergunta.
    /// `id` = o `tool_call.id` (correlaciona a proposta com a decisão e o resultado
    /// no host — mesmo id no `gate_request` e no `tool_result`).
    fn approve(&mut self, policy: Policy, scope: &str, id: &str, preview: &Preview) -> bool;
    /// Estouro do teto de voltas do turno (SECURITY §2.7): decide se o agente
    /// ganha MAIS UM LOTE de `max_voltas`. Só o terminal interativo pergunta de
    /// verdade; host NDJSON e `-p` devolvem `false` — o turno encerra com a
    /// sessão preservada (um "continua" retoma de onde parou).
    fn continuar_apos_teto(&mut self, teto: usize) -> bool;
    /// Aviso do estouro (depois de `continuar_apos_teto` negar). Mora no
    /// Frontend porque o conselho honesto depende da casca: REPL e host NDJSON
    /// persistente seguem com a sessão viva ("continua" retoma); `-p` e
    /// `--json -p` terminam o processo SEM persistir nada — o único conselho
    /// possível é rodar de novo com teto maior.
    fn aviso_teto(&mut self, teto: usize);
    /// The model repeated the same call with the same result `estagnacao::PARADA` times (0.15.3):
    /// does the turn go on? Only the interactive terminal asks; a host and `-p` stop, for the
    /// reason the ceiling never asks a host (its Auto mode would answer for nobody). Stopping ends
    /// the turn with the closing summary.
    fn continuar_apos_estagnacao(&mut self, o_que: &str) -> bool {
        let _ = o_que;
        false
    }
    /// The person stopped the turn (Ctrl-C or the host's `cancel`); `turn_end` follows. What
    /// already ran stays done and in the history (0.14.0).
    fn turno_interrompido(&mut self) {
        self.warn("turno interrompido — o que já rodou ficou feito; mande a próxima mensagem para seguir.");
    }
}
