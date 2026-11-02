# Embutindo o motor do `anna` (protocolo NDJSON)

Como uma casca externa (a aba **Agente do SShvTerm**, uma extensão de editor, um
job de CI) dirige o motor do `anna` sem falar ANSI. O mesmo loop que move o
terminal move o host — via um `Frontend` que fala **NDJSON** (`src/json.rs`).

> **Current lifecycle notes (2026-10-15):** JSON modes do not use REPL session autosave.
> `--json -p` can still request write approval from stdin; terminal `-p` refusal is not
> a restriction on this frontend. Pending gates/results have no application timeout.
> Hosts must handle `error` and process closure: some error paths do not emit `turn_done`.
> Model, usage and event fields below are the engine contract, not proof of a deployed host.

## Como subir

```bash
# Persistente (uma sessão, vários turnos):
anna --json                       # o host manda turnos pelo stdin

# One-shot (um turno e sai):
anna --json -p "explique o projeto"
```

- **cwd** = a raiz do projeto que a Anna vai inspecionar (a detecção de git/ANNA.md
  usa o diretório de trabalho do processo).
- **API key**: o host exporta `SHVIA_API_KEY` no ambiente do filho. **Nunca** por
  argumento (aparece em `ps`). `--model` (perfil `modelo@servidor`), `--effort`
  (`low`…`max`, repassado ao gateway) e `--url` são opcionais.

## Fluxo (stdin ↔ stdout)

Tudo é **uma linha JSON por mensagem** (NDJSON), nos dois sentidos.

### Host → `anna` (stdin)

| Linha | Quando |
|-------|--------|
| `{"type":"user","text":"...","images":["BASE64"]}` | starts a persistent turn; `images` is optional, pure base64 without a data URL prefix |
| `{"type":"exit"}` | encerra o motor (0.14.0+: also stops the turn in progress first) |
| `{"type":"cancel"}` | **stops the turn in progress** (0.14.0+): the stream is dropped, a running `bash` is killed with its process group, a pending gate is refused, and every tool call not yet run gets a `RECUSADO` result; then `interrupted`, `usage`, `turn_done`. Also stops a `user` turn already queued; ignored when nothing is running or queued |
| `{"id":"…","decision":"approve"\|"always"\|"reject"}` | **resposta a um `gate_request`** (hosts should correlate IDs; this engine currently consumes the next decision without enforcing its ID) |
| `{"id":"…","decision":"continue"\|"stop","message":"…"}` | **answer to a `stop_request`** (engines that hold a turn open — see the handshake below; `anna` never asks) |

### `anna` → Host (stdout)

| Evento | Payload | Significado |
|--------|---------|-------------|
| `model` | `model`, `server` | modelo/infra escolhidos no turno |
| `text` | `delta` | pedaço da resposta (streaming) |
| `tool_call` | `id`, `name`, `arguments` | a Anna vai usar uma ferramenta |
| `tool_result` | `id`, `name`, `bytes`, `content` | resultado da ferramenta (já executada) |
| `gate_request` | `id`, `scope`, `policy`, `preview` | **pede aprovação** — o motor BLOQUEIA até a decisão (`id` = o `tool_call.id`, casa com o `tool_result`) |
| `usage` | `tokens`, `cost`, `estimated` | cumulative telemetry since this process started, not a per-turn delta |
| `turn_done` | — | turno terminou; pode mandar o próximo `user` |
| `interrupted` | — | (0.14.0+) the turn was stopped by `cancel`/`exit`; `usage` and `turn_done` follow. What already ran stays done; the history stays valid for the next `user` |
| `stop_request` | `id`, `iteration`, `last`, `reason` | **optional, per engine** — the model wants to end the turn and the engine asks the host whether to continue; **BLOCKS** until the answer. `anna` does not emit it |
| `error` / `warn` | `message` | erro fatal do turno / aviso |

`policy` ∈ `auto` (nunca pede) · `confirm` (`approve`/`always`/`reject`) ·
`always` (destrutivo — `always` não vale, só `approve`/`reject`).

**Teto de voltas** (SECURITY §2.7): no host o motor NUNCA pergunta se continua —
estourou, emite `warn` + `turn_done`. No modo persistente a sessão segue viva no
processo: a casca pode oferecer um botão "continuar" que só reenvia
`{"type":"user","text":"continua"}` (campo `text`, como na tabela acima). Em
`--json -p` o processo termina sem persistir nada — o aviso já manda rodar de
novo com teto maior. (A pergunta não vira `gate_request` de propósito: o modo
Auto do host aprovaria sozinho — loop sem humano.)

`preview` (o que aprovar):
- `{"kind":"diff","path":"src/x.rs","diff":"...unificado sem cor..."}`
- `{"kind":"command","command":"cargo test","why":"..."}`
- `{"kind":"commit","stat":"...","message":"..."}`

The stdin stream is sequential. Normal completion emits `turn_done` before the next
`user` turn; meanwhile, answer the current gate or host tool result. Current HTTP/SSE
error paths can emit `error` without `turn_done`, so the host must also handle that
terminal error and process exit instead of waiting indefinitely for completion.
Since 0.14.0 one engine thread reads stdin, so a `cancel` reaches a turn that is streaming
or running a tool; other lines are queued in order for whoever reads next (the main loop,
a gate, or a host tool result). A gate or result wait still discards lines that are not its
answer. JSON state lasts only
as long as the process, and `anna sessions resume` does not restore it.

### O handshake de aprovação

```
anna → {"type":"gate_request","id":"call_1","scope":"edit_file","policy":"confirm",
        "preview":{"kind":"diff","path":"src/x.rs","diff":"  ✎ src/x.rs\n  - a\n  + b\n"}}
        (BLOQUEIA)
host → (renderiza o diff, mostra Aprovar/Negar)
host → {"decision":"approve"}
anna → {"type":"tool_result","name":"edit_file","content":"ok — src/x.rs editado..."}
```

Se o stdin fechar com um `gate_request` pendente, o motor **nega** (seguro).

### The stop handshake (`stop_request`) — optional, per engine

> Added on 10/09/2026 for the **Run** of the desktop's Code mode
> (`SHVIA-DESKTOP/docs/code/RUN-20260910.md`, ADR-034). It exists so that a host can
> continue a turn **without paying a new turn**: on an engine that can hold the turn
> open, the "continue" keeps the model's context intact. `anna` cannot hold a turn open
> — its turn ends when the model returns no tool call — so `anna` **never emits it**; for
> `anna` the host continues between turns, by sending the next `user` line as always.

Who emits it today: the `claude-runner` of SHVIA-DESKTOP (its SDK has a `Stop` hook).
The `codex-runner` has no hook surface and does not emit it either.

```
engine → host   {"type":"stop_request","id":"s-7","iteration":7,
                 "last":"Etapa 1 concluída… Posso seguir para a API?",
                 "reason":"model_stopped"}                        (BLOCKS)
host → engine   {"id":"s-7","decision":"continue","message":"Continue conforme o plano. Não peça confirmação entre etapas."}
             |  {"id":"s-7","decision":"stop"}
```

- `last` is the last assistant text, cut at 4.000 characters **keeping the tail, not the
  head**; the host already has the full text from the `text` deltas. The end is what must
  survive: the execution contract asks the model to close with its marker
  (`RUN_DONE:`/`NEEDS_INPUT:`) and the host's rule reads the closing paragraphs, so the
  head is the report and the tail is the decision. Measured on 10/09/2026: with the head
  kept, a closing summary over 4.000 characters flipped every tier of the rule to
  `continue`.
- `iteration` counts the stop requests of this turn in the engine, so the host can
  correlate it with its own count.
- `continue` keeps the turn alive: the engine hands `message` to the model as the reason
  to go on, and the turn proceeds in the same context. `stop` lets the turn end exactly
  as it would without the handshake — `usage`, then `turn_done`.
- **Only emitted when the host asked for it at spawn** (a flag of the engine). A host
  that does not know the event never receives it, so an old host with a new engine keeps
  working unchanged.
- **Ceiling of 60 s per request.** No answer in time → `stop`. This is what keeps a
  closed or busy host from hanging a turn: the turn ends normally and the host can still
  continue between turns.
- stdin closed with a `stop_request` pending → `stop`, the same safe default the gates
  use for `reject`.
- The decision is never the engine's to make: whoever answers (the desktop page, on
  behalf of the server's orchestrator) is who decides. The engine only asks.

## Exemplo de turno

```
→ {"type":"user","text":"onde está o bug no cálculo de boleto?"}
← {"type":"model","model":"...","server":"on-prem"}
← {"type":"text","delta":"Vou procurar."}
← {"type":"tool_call","id":"c1","name":"grep","arguments":{"pattern":"boleto"}}
← {"type":"tool_result","id":"c1","name":"grep","bytes":83,"content":"src/Boleto.php:88: ..."}
← {"type":"text","delta":"Achei: src/Boleto.php:88 — a data usa d/m/Y..."}
← {"type":"usage","tokens":2450,"cost":0.0012,"estimated":false}
← {"type":"turn_done"}
```

## Execução: `--tools local` vs `--tools host` (ADR-002)

Quem roda as ferramentas:

- **`--tools local`** (default): o **próprio `anna`** executa. `bash` roda na
  máquina onde o `anna` está; a aprovação sai como `gate_request` (Modelo A).
- **`--tools host`** requires `--json` to select the host runner; otherwise execution
  remains local. This runner offers only `bash`. It emits `tool_call` without executing
  or approving it. The host applies its own policy/PTY/SSH/audit and returns the result
  through stdin:

  ```
  anna → {"type":"tool_call","id":"c1","name":"bash","arguments":{"command":"ls"}}
  host → (aplica SUA política; executa onde o usuário está)
  host → {"type":"tool_result","id":"c1","content":"a.rs\nb.rs\n"}
  ```

  Neste modo **não há `gate_request`** — quem aprova é o host. É o **Modelo B** do
  ADR-002, feito pra aba Agente do SShvTerm (que já tem política + exec no host).

## Segurança

- Chave só via env (§ SECURITY 2.4); o host repassa no ambiente do filho.
- Local filesystem tools use project path guards; an approved shell command runs with
  account permissions, and filename denylist coverage is incomplete. See
  [security](../SECURITY_GUIDELINES.md).
- Em `--tools host`, o confinamento e a execução são do host — o `anna` só propõe;
  a política do host (allow/ask/deny) é que manda.
