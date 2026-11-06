# Embutindo o motor do `anna` (protocolo NDJSON)

Como uma casca externa (a aba **Agente do SShvTerm**, uma extensão de editor, um
job de CI) dirige o motor do `anna` sem falar ANSI. O mesmo loop que move o
terminal move o host — via um `Frontend` que fala **NDJSON** (`src/json.rs`).

> **Current lifecycle notes (rechecked against 0.15.8 on 2026-11-05):** JSON modes do not
> use REPL session autosave. `--json -p` can still request write approval from stdin;
> terminal `-p` refusal is not a restriction on this frontend. Pending gates/results have
> no application timeout. Hosts must handle `error` and process closure: a gateway error
> (`error` in the stream or before it) and a transport failure emit `error` **without**
> `usage`/`turn_done` (`agent::run_turn`, `main::run_json_turn`). Model, usage and event
> fields below are the engine contract, not proof of a deployed host.
>
> **Hosts in the SHVIA repositories (source, 2026-11-05):** the Desktop's Code mode
> (`SHVIA-DESKTOP` `src-tauri/src/code_bridge/sessao.rs`) and the Workspace
> (`SHVIA-WORKSPACE` `services/api/src/codeSessions.ts`) both spawn
> `anna --json --tools local [--model …] [--effort …] [--url …]` with `SHVIA_API_KEY` in
> the child's environment and the project as working directory. Neither uses
> `--tools host`; the SShvTerm Agent tab named below was not checked.

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
| `{"type":"credential","api_key":"…"}` | **replaces the gateway key** (0.16.0+), at any time: between turns, during a turn, while a gate waits. The next request uses it. The line is applied by the stdin reader and never queued, logged or echoed; a blank key is ignored. See the credential handshake below |
| `{"id":"…","decision":"approve"\|"always"\|"reject"}` | **resposta a um `gate_request`**. Since 0.15.8 a decision whose `id` is another gate's is ignored with a `warn` (a late answer to a gate a `cancel` already closed must not decide the next one); a decision without `id` is still taken, for older hosts — send the `id` |
| `{"id":"…","decision":"continue"\|"stop","message":"…"}` | **answer to a `stop_request`** (engines that hold a turn open — see the handshake below; `anna` never asks) |

### `anna` → Host (stdout)

| Evento | Payload | Significado |
|--------|---------|-------------|
| `model` | `model`, `server` | model and server label of each gateway request (one per round, and again after a gateway failover); `server` is the gateway's `server_label` |
| `text` | `delta` | pedaço da resposta (streaming) |
| `tool_call` | `id`, `name`, `arguments` | a Anna vai usar uma ferramenta |
| `tool_result` | `id`, `name`, `bytes`, `content` | resultado da ferramenta (já executada) — local runner only; with `--tools host` the engine does not echo the result the host returned. `content` is what the model receives, including the notes `anna` appends (stall guard, subfolder instructions) |
| `gate_request` | `id`, `scope`, `policy`, `preview` | **pede aprovação** — o motor BLOQUEIA até a decisão (`id` = o `tool_call.id`, casa com o `tool_result`) |
| `usage` | `tokens`, `cost`, `estimated` | cumulative telemetry since this process started, not a per-turn delta |
| `turn_done` | — | turno terminou; pode mandar o próximo `user` |
| `credential_expired` | — | (0.16.0+) the gateway refused the key with 401, before any model ran. The engine waits up to 60 s for a `credential` line, then sends the same request again with it; with no answer, or with a second 401, the turn ends with `error` as before. The event carries no key |
| `interrupted` | — | (0.14.0+) the turn was stopped by `cancel`/`exit`; `usage` and `turn_done` follow. What already ran stays done; the history stays valid for the next `user` |
| `stop_request` | `id`, `iteration`, `last`, `reason` | **optional, per engine** — the model wants to end the turn and the engine asks the host whether to continue; **BLOCKS** until the answer. `anna` does not emit it |
| `error` / `warn` | `message` | erro fatal do turno / aviso |

`policy` ∈ `auto` (nunca pede) · `confirm` (`approve`/`always`/`reject`) ·
`always` (destrutivo — `always` não vale, só `approve`/`reject`).

**Teto de voltas** (SECURITY §2.7): no host o motor NUNCA pergunta se continua —
estourou, emite `warn` + `turn_done`. Since 0.15.2 a closing request comes first: its answer
streams as ordinary `text` events (what was done, what is left, the next step), so the turn's
last text is a summary instead of the empty text of a round that had asked for tools; a tool
call in that answer is neither run nor emitted.

**Stall guard** (0.15.3+): the same tool call with the same arguments and the same result three
times in a row (or a cycle of up to four calls repeated three times) adds a note to that
`tool_result`'s content for the model and emits a `warn` (*"o modelo está se repetindo — …"*). At
five the engine never asks a host either: it emits `warn` (*"turno parado: …"*), the same closing
request as the ceiling, then `turn_done`. No modo persistente a sessão segue viva no
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
host → {"id":"call_1","decision":"approve"}
anna → {"type":"tool_result","id":"call_1","name":"edit_file","bytes":43,"content":"ok — src/x.rs editado (1 ocorrência(s))."}
```

Se o stdin fechar com um `gate_request` pendente, o motor **nega** (seguro). A
`{"type":"exit"}` or a `cancel` while a gate waits also refuses it. Lines without a
`decision` are ignored while the gate waits.

### The credential handshake (0.16.0)

A host that hands `anna` a short-lived key renews it without restarting the process, so the
session keeps its history. Measured on 06/11/2026: SHVIA-DESKTOP passes the web session's Code
token, valid for 12 hours, as `SHVIA_API_KEY`; an autonomous run paused overnight waiting for the
person, the answer came the next morning, and the resumed iteration's first request got a 401.
Until 0.15.x the key was read once at start, so the only way out was a new process and an empty
history.

```
anna → {"type":"credential_expired"}           (the round waits, up to 60 s)
host → (asks its own source for a new key)
host → {"type":"credential","api_key":"<new key>"}
anna → {"type":"warn","message":"credencial renovada pela casca — repetindo o pedido."}
anna → (the same request again, with the new key)
```

- **Once per round.** A 401 with the key just received is not an expired key: the turn ends
  with the error.
- **A host may also send the key ahead of time**, before it expires; nothing is emitted then.
- **Older hosts** never answer: the round waits the 60 s and reports the 401 as before. Older
  engines ignore a `credential` line everywhere (the main loop, a gate, the host runner), so a
  host can send it without knowing the engine's version.
- `cancel` during the wait stops the turn as usual.

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
  The host mode also uses its own system prompt (propose one shell command at a time,
  never execute). The wait for the host's result matches the `tool_result` `id` when both
  sides have one, ignores every other line, and has no timeout; stdin closing returns
  `ERRO: host fechou o stdin sem devolver o resultado.` to the model, and a `cancel`
  returns a `RECUSADO` result. The planning pipeline does not run in this mode.

## Segurança

- Chave só via env (§ SECURITY 2.4); o host repassa no ambiente do filho. A renewed key
  (0.16.0) arrives on stdin, never as an argument, and is never written to an event, a log or
  the session file.
- Local filesystem tools use project path guards; an approved shell command runs with
  account permissions, and filename denylist coverage is incomplete. See
  [security](../SECURITY_GUIDELINES.md).
- Em `--tools host`, o confinamento e a execução são do host — o `anna` só propõe;
  a política do host (allow/ask/deny) é que manda.
