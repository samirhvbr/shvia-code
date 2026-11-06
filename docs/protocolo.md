# Protocolo do gateway — `POST /api/v1/code/chat`

> **Source review, 2026-11-05:** this describes the CLI request/SSE contract of `anna`
> 0.15.8, checked against ShvIA Web `origin/master` `b24e45a5` (2.114.133). It is a
> source comparison, not a deployment attestation: which ShvIA Web version serves
> `ai.shvia.org` was not checked. Client serialization lives in [`types.rs`](../src/types.rs) and
> [`api.rs`](../src/api.rs); server validation lives in
> [CodeChatController](https://github.com/samirhvbr/shvia-web/blob/b24e45a5/app/Http/Controllers/Api/CodeChatController.php).
>
> **What `anna` actually sends:** the `X-API-Key` header and a body with `messages`,
> `tools` (left out when empty), `profile_name` and `effort` (each left out when unset).
> `profile_name` is the `--model` / `SHVIA_PROFILE` / `profile` string, sent verbatim —
> normally `model@server` (the gateway also parses `model#server`); `anna` never splits
> it into `server`/`model`. `anna` never sends `server`, `model`, `tool_choice` or
> `temperature`; those request fields below belong to the wider gateway API, not to
> CLI flags. Conversation is resent for inference even when session files stay local.

---

## 1. Chamada

```
POST {SHVIA_URL}/api/v1/code/chat          # SHVIA_URL default = https://ai.shvia.org
Content-Type: application/json
Accept: text/event-stream
X-API-Key: <chave do ShvIA>                # ou Authorization: Bearer <token>
```

- **HTTPS is required**, except HTTP on loopback for local development (SECURITY §2.6).
  The rate limit uses `SHVIA_API_KEY_RATE_LIMIT`, default 20 requests/minute, under the
  named limiter `api-key-inference`, which the gateway also applies to
  `POST /api/v1/agent/chat`: both routes count against the same per-user budget. Every
  round of a turn is one request.
- Scoped bearer tokens require the `code:chat` ability (`token.ability:code:chat`, 403
  otherwise); the CLI sends its configured key using `X-API-Key`. The route's own comment
  says the panel token (`*`) and the long user API key pass, and a token delegated for
  another purpose (`workspace-inference`) is refused there.
- Resposta de sucesso: `200` + `text/event-stream` (SSE). Headers úteis:
  `X-Inference-Server`, `X-Inference-Model`, `X-Inference-Request-Id`,
  `X-Token-Saver-Saved-Chars`. `anna` reads none of them; it reads the `model` event.

## 2. Corpo da requisição (JSON)

```jsonc
{
  "messages": [                     // required; 1..5000 on this door since 02/11/2026 (1..200 before)
    { "role": "system",    "content": "..." },
    { "role": "user",      "content": "...", "images": ["<base64 puro>"] },
    { "role": "assistant", "content": "...", "tool_calls": [{"id":"call_1","name":"read_file","arguments":{"path":"README.md"}}] },
    { "role": "tool",      "tool_call_id": "call_1", "content": "<resultado da ferramenta>" }
  ],
  "tools": [                        // opcional, 0..32 — defs no formato JSON Schema
    { "name": "read_file", "description": "Read a file", "parameters": { "type":"object", "properties":{"path":{"type":"string"}}, "required":["path"] } }
  ],
  "tool_choice": "auto",            // optional gateway field: auto|none|required|<tool name>; not sent by anna
  "profile_name": "model@server",   // optional — what anna sends (verbatim --model); see "Target" below
  "server": "shvia1",               // optional gateway field; not sent by anna
  "model":  "qwen-coder",           // optional gateway field; not sent by anna
  "effort": "medium",               // opcional: low|medium|high|xhigh|max
  "temperature": 0.2                // optional gateway field: 0..2; not sent by anna
}
```

**Validation limits:** `content` ≤ 200,000 characters; `images` ≤ 8 per message,
≤ 8,000,000 base64 characters **each** and ≤ 24,000,000 **across the request**;
`tools` ≤ 32 and `tool_calls` ≤ 32 per message. Tool arguments can be an object/array
or a JSON string. The PHP-FPM `post_max_size` also limits the whole request body;
exceeding it can leave Laravel with an empty body before these field rules apply.
Validation failures return 422, with one exception: oversized `role:tool` content is
trimmed, with an omission marker, before field validation. Non-tool content above the
limit is refused.

**Target.** With `profile_name` set, the gateway resolves that profile. With nothing set,
the current ShvIA Web source falls back to the `model_pin`/`effort_pin` of the person's
active Code project (`folders.is_current`, `type = code`), then to the user's default,
then to the server configuration. A later source recheck on 2026-11-05 against Web
`2db8252a` (2.114.138) confirms that an explicit `model` **or** `profile_name` prevents
both halves of the project pin from filling the target. This fixes the earlier finding
at Web `b24e45a5`, where the pin's model could replace the model in anna's profile.
See the [dated status](DOCUMENTATION-REVIEW-20261015.md#status-on-2026-11-05).
This was checked in `CodeChatController::stream` and `PinDoProjetoDeCodeTest`, not
against a deployed gateway. Pre-stream routing errors return `400 {detail, server, models}`.

**Stateless conversation:** each request carries the retained history. Without limits
from the gateway (an older gateway, or a `model` event without `context_window`), the CLI
cuts the history down to 100 messages when it goes over 150 (0.13.6+; earlier versions slid
a 150-message window on every round), preserving initial system context, the session's
first and latest `user` messages, and avoiding orphan tool results. Those numbers are 3/4
and 1/2 of the 200-message ceiling the gateway validated until 02/11/2026; changing either
side requires checking the other.

**Budget from the gateway (0.15.0+).** When the `model` event carries `messages_max` and
`context_window`, the client budgets against them instead of the fixed numbers. The current
ShvIA Web source sends `messages_max: 5000` (`TetosDoTurno::MESSAGES_MAX_AGENTE`, the same
constant its validation uses) and the target's window capped at `CODE_CHAT_HISTORY_TOKENS`
(default 128,000), so with a known window a long turn is budgeted to roughly 64k–90k
estimated tokens per request and the count cut (past 3,750, down to 2,500) rarely applies:

- the count ceiling becomes `messages_max` (cut past 3/4, down to 1/2). Without a window
  the count stays capped at 200, because a count is then the only budget;
- with a window, the history is also cut when its estimate passes 70% of the window, down
  to 50%. The estimate is chars/4 of the messages and tool definitions, scaled by the last
  `usage.prompt_tokens` divided by the chars/4 of the request that produced it (clamped to
  0.3–4). A token cut never removes the latest assistant turn and its tool results.
  Before dropping any message it prunes (0.15.1+): a `read_file` result whose file a later
  `edit_file`/`write_file` changed, and outputs over 600 characters older than the recent
  ~20% of the window, become a one-line `[saída antiga: …]` stub naming the tool and its
  target. Messages are dropped only if pruning was not enough;
- an SSE `error` with `codigo: "provedor_recusou_pedido"` (the provider refused the request
  itself, as Ollama does for a prompt over `num_ctx`) is retried **once** in the same round,
  with the history cut to half of the estimate just sent. Any other cause fails the same
  way again and is reported.

An older gateway sends neither field and the client behaves as 0.13.6–0.14.x. A gateway
that sends `context_window: null` (window unknown) keeps the count at most 200.

**What the gateway does to the request (ShvIA Web source, not this repository).** Before
inference it trims oversized `role:"tool"` content with a marker, may compress large tool
outputs (`ToolResultCompressor`, skipped for a request with `X-Shvia-Token-Saver: off`,
which `anna` does not send), and appends a `user` nudge when its `ToolLoopGuard` sees a
tool-call loop. On a request whose last message is `user` (a new turn, not a tool round)
and an on-premises target — or a cloud target with the operator's opt-in — it prefixes that
message with the person's ai-memory wiki block and the active Code project's skills. None of
this changes the history `anna` keeps; it changes what the model receives.

### Imagem no turno do usuário (0.11.4+)

`images` é **opcional**, só existe em `role:"user"` e leva **base64 puro** — sem o
prefixo `data:<mime>;base64,`. É a convenção do Ollama, que o gateway já falava
para o chat (`ChatService` monta `$entry['images']` assim); os drivers de nuvem
derivam o mime dos **magic bytes** (`ImageMime::detect`), então o tipo **não
viaja**. Ausente e vazio são a mesma coisa: `skip_serializing_if` mantém o corpo
byte a byte idêntico ao de antes em todo turno sem figura.

⚠️ **Só o ÚLTIMO turno do usuário carrega imagem.** O `anna` guarda o histórico em
memória e reenvia a lista inteira a cada volta do laço agêntico — se a figura
ficasse presa ao turno velho, ela subiria de novo em cada chamada de ferramenta,
de todo turno seguinte, até a sessão acabar. Num turno de código com 30 voltas é a
mesma imagem paga 30 vezes, **com a resposta saindo certa** — o tipo de conta que
ninguém audita porque nada parece errado. O turno novo apaga a figura do anterior
e mantém o texto dele (`empurrar_turno_usuario`, `main.rs`). Dentro do turno a
figura sobrevive a todas as voltas, de propósito: o modelo precisa poder olhar a
imagem outra vez depois de ler um arquivo.

⚠️ **O modelo precisa ter visão.** O gateway recusa com **422** e `{detail}` quando
o alvo resolvido AFIRMA não aceitar imagem. Tri-estado: `null` (capability
desconhecida) passa — o operador costuma saber o que selecionou.

### Formato das mensagens de ferramenta
- **Assistente que chamou tools:** `role:"assistant"` + `tool_calls: [{id,name,arguments}]`
  (é o que veio no evento `tool_call`/`done`, reenviado tal e qual).
- **Resultado da tool:** `role:"tool"` + `tool_call_id` (o `id` do call) +
  `content` = a saída (string). Um `role:"tool"` por call.
- **Text `anna` adds to a tool result:** the stall-guard note at the third repeat (0.15.3+),
  a one-line pointer to a subfolder's own instruction file on first touch (0.15.6+), a
  `[saída antiga: …]` stub replacing a pruned output (0.15.1+), and
  `RECUSADO: o usuário interrompeu o turno…` for a call a stop kept from running (0.14.0+).
- **Messages `anna` writes itself:** the `system` compaction marker (`[Contexto compactado] …`,
  one at a time), an interrupted answer kept as `assistant` content ending in
  `[interrompido pelo usuário]`, and the closing request of a turn that cannot go on
  (a `user` message starting with `[anna]`, sent once and not kept in the history).

## 3. Stream de eventos (SSE)

Cada evento é uma linha `data: {json}` terminada por linha em branco (`\n\n`):

```
data: {"kind":"<tipo>","data":{...}}
```

| `kind` | quando | `data` |
|--------|--------|--------|
| `model` | 1º, sempre | `{name, model, server, server_label, target_source, request_id, messages_max?, context_window?}` — cabeçalho da sessão; the last two (sent since the shvia-web change of 02/11/2026; absent before) are the target's per-request message ceiling and effective context window in tokens (`null` when unknown). Sent **again** after a failover, with `target_source: "combo_fallback"` and the limits of the target that answered; the client keeps the last one. `anna` reads only `model`, `server_label` and the two limits |
| `text_delta` | streaming da resposta | `{delta:"<pedaço de texto>"}` — concatenar |
| `reasoning_delta` | modelos com "pensar" | `{delta:"..."}` — pensamento (the gateway's `ReasoningSink` redacts instructions it quotes); `anna` discards the content |
| `tool_call` | modelo pediu ferramenta | `{id, name, arguments:{...}}` — **arguments já é objeto** (decodificado) |
| `usage` | antes do `done` | `{model, profile, server, server_label, prompt_tokens, completion_tokens, total_tokens, thinking_time_ms, response_time_ms, effort, cost, cost_estimated}` — `anna` reads `total_tokens`, `prompt_tokens`, `cost` and `cost_estimated` |
| `error` | falha | `{message:"...", codigo?, status?}` — seguido de `done`; `codigo` classifies it (`provedor_recusou_pedido` = 4xx from the provider, `provedor_indisponivel` = 5xx, `resposta_vazia` = no text and no tool call, sent as status 502). A read timeout that cut the answer sends `error`, then `usage` and a `done` carrying what arrived; `anna` reports the error and ends the turn without recording that partial text |
| `done` | fim, sempre | `{text:"<resposta completa>", tool_calls:[{id,name,arguments}]}` |

- **`cost` vs `cost_estimated`** (padrão 2.15.9): `cost` = custo real (US$, provedor
  informou, > 0); senão `cost` é `null` e `cost_estimated` traz a estimativa
  (tokens × pricing). O CLI mostra `US$` exato ou `≈US$` estimado.
- **`done.tool_calls`**: repete os tool calls completos — fallback pra quem não
  acumulou os eventos `tool_call` em stream. Use um ou outro, não os dois.

## 4. O loop agêntico (contrato)

```
1. CLI monta messages (system + histórico) + tools[]
2. POST /api/v1/code/chat  (SSE)
3. lê deltas (mostra ao vivo) e coleta tool_calls + usage
4. done:
     - tem tool_calls?  → executa CADA um LOCALMENTE (com gates), acrescenta:
         { role:"assistant", content:<text>, tool_calls:[...] }
         { role:"tool", tool_call_id:<id>, content:<resultado> }  (um por call)
       → volta ao passo 2 com o histórico crescido
     - no tool_calls?   → append { role:"assistant", content:<text> } and end the turn
5. teto de voltas por turno (SECURITY §2.7, default 100; `max_voltas` no
   config.toml ou env `SHVIA_MAX_VOLTAS`) — estoura: terminal interativo
   pergunta se continua mais um lote; host NDJSON persistente encerra o
   turno com a sessão viva (reenviar `{"type":"user","text":"continua"}`
   retoma); `-p`/`--json -p` terminam sem persistir nada — o aviso manda
   rodar de novo com teto maior. Since 0.15.2, when no batch is granted, one
   more request (the same `tools`, never `tool_choice: none`, which would change
   the rendered prompt and its cache) asks for the turn's last word: done, left,
   next step. Its `user` message is not kept in the history; the answer is
   streamed like any other, becomes the turn's final text and stays in the
   history as the assistant's last word. A tool call in it is neither run nor
   shown.
6. stall guard (0.15.3+): the same call with the same arguments and the same
   result (or the same cycle of up to four calls) three times in a row appends
   a note to that tool result; at five the turn stops — the interactive terminal
   asks, a host and `-p` get the closing request of step 5, worded for the stall.
7. a stop (Ctrl-C, NDJSON `cancel`/`exit`; 0.14.0+) ends the turn with a valid
   history: see "Messages `anna` writes itself" in §2.
```

O **servidor nunca executa ferramenta** — só devolve `tool_call`. Execução (e os
gates de aprovação) são 100% do cliente. Ver `SECURITY_GUIDELINES.md` §3.

## 5. Erros

| Situação | Forma |
|----------|-------|
| Missing, invalid or expired key | `401` (before the stream); `anna` prints how to generate a user key. Under an NDJSON host (0.16.0) it first emits `credential_expired` and waits up to 60 s for a new key, then repeats the request once ([embedding](embedding.md)) |
| Perfil mal-formado / servidor inválido | `400 {detail, server, models}` (antes do stream) |
| Perfil bloqueado pro usuário, token without `code:chat`, account without API permission | `403 {detail}` |
| Validação de campos | `422 {errors}`; an image sent to a target the catalog says has no vision: `422 {detail}` |
| Servidor de inferência off/erro, empty answer | evento SSE `error` + `done` (stream já aberto) |
| Rate limit | `429` (Retry-After) |

O CLI deve tratar tanto o **JSON de erro pré-stream** (4xx antes do `text/event-stream`)
quanto o **evento `error` no meio do stream**. Nunca ecoar a API key em log.

**Retries (0.13.10+).** A round that fails before the stream opens with 429, 408, 502,
503 or 504, or that never connected, is sent again up to 4 times: the CLI waits the
2, 4, 8, 16 s backoff (capped at 30 s) with jitter, or the gateway's `Retry-After` when that is
longer (the per-account throttle sends it with every 429), and shows a `warn` each time. A
shorter `Retry-After` is raised to the backoff (0.15.10): Laravel answers `Retry-After: 0` in the
last second of its window, and taking it as given sent all four retries in the same second. A
`Retry-After` above 120 s ends the turn with the error instead. Not retried: 400/401/403/422,
500 (a gateway defect), an SSE `error` event (the gateway already ran its own failover),
and a connection that broke after the request was sent, which may already have started a
billed inference.

## 6. Exemplo (uma volta com ferramenta)

```
→ POST { messages:[system, {user:"o que faz o config.php?"}], tools:[read_file,list_dir] }
← data: {"kind":"model","data":{"model":"qwen-coder","server_label":"SHVIA-GPU"}}
← data: {"kind":"tool_call","data":{"id":"call_1","name":"read_file","arguments":{"path":"config.php"}}}
← data: {"kind":"usage","data":{"total_tokens":320,"cost":null,"cost_estimated":0.0007}}
← data: {"kind":"done","data":{"text":"","tool_calls":[{"id":"call_1","name":"read_file","arguments":{"path":"config.php"}}]}}

  (CLI executa read_file(config.php) localmente → "<?php return [...];")

→ POST { messages:[..., {assistant, tool_calls:[call_1]}, {tool, tool_call_id:"call_1", content:"<?php ..."}], tools:[...] }
← data: {"kind":"text_delta","data":{"delta":"O config.php define timezone, retries e o scheme (com um bug: 'htps')."}}
← data: {"kind":"usage","data":{...}}
← data: {"kind":"done","data":{"text":"O config.php ...","tool_calls":[]}}   ← sem tools = resposta final
```

Prova de referência: `src/api.rs` — o cliente que o `anna` usa em produção, com os
testes de (de)serialização do request e dos eventos ao lado, em `mod tests` do mesmo
arquivo.

> Até 02/09/2026 a referência apontava para `spikes/c0-gateway.php`, um cliente PHP de
> 08/07 que nada compilava, testava ou executava — congelado no protocolo daquele dia,
> num repositório Rust. Referência que ninguém roda não prova nada e envelhece calada;
> a implementação real, que o CI compila e testa a cada push, prova. O spike foi
> removido (achado F-10).
