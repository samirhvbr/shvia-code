# Protocolo do gateway — `POST /api/v1/code/chat`

> Contrato **CLI ↔ servidor**. É a interface que o `anna` (Rust) implementa
> contra o `CodeChatController` do SHVIA-WEB (2.16.0). Fonte da verdade do wire
> format; o cliente segue **isto**, não o código do servidor. Mudança aqui =
> versão do servidor + nota neste doc.

---

## 1. Chamada

```
POST {SHVIA_URL}/api/v1/code/chat          # SHVIA_URL default = https://ai.shvia.org
Content-Type: application/json
Accept: text/event-stream
X-API-Key: <chave do ShvIA>                # ou Authorization: Bearer <token>
```

- **HTTPS obrigatório** (SECURITY §2.6). Rate limit = o mesmo do `agent/chat`
  (`SHVIA_API_KEY_RATE_LIMIT`, default 20/min).
- Resposta de sucesso: `200` + `text/event-stream` (SSE). Headers úteis:
  `X-Inference-Server`, `X-Inference-Model`, `X-Inference-Request-Id`.

## 2. Corpo da requisição (JSON)

```jsonc
{
  "messages": [                     // obrigatório, 1..200
    { "role": "system",    "content": "..." },
    { "role": "user",      "content": "...", "images": ["<base64 puro>"] },
    { "role": "assistant", "content": "...", "tool_calls": [ {"id","name","arguments"} ] },
    { "role": "tool",      "tool_call_id": "call_1", "content": "<resultado da ferramenta>" }
  ],
  "tools": [                        // opcional, 0..32 — defs no formato JSON Schema
    { "name": "read_file", "description": "...", "parameters": { "type":"object", ... } }
  ],
  "tool_choice": "auto",            // opcional: auto|none|required|<nome-da-tool>
  "profile_name": "model@server",   // opcional — perfil explícito; senão o default do usuário
  "server": "shvia1",               // opcional (alternativa a profile_name)
  "model":  "qwen-coder",           // opcional
  "effort": "medium"                // opcional: low|medium|high|xhigh|max
}
```

**Limites (recusa 422 acima):** `content` ≤ 200 000 chars; `images` ≤ 8/mensagem,
≤ 8 MB de base64 **cada** e ≤ 24 MB **somados na requisição** (o total é o que bate
no `post_max_size` do PHP-FPM, e estourá-lo faz o corpo chegar VAZIO ao Laravel);
`tools` ≤ 32;
`tool_calls` ≤ 32/mensagem; `arguments` = array OU string JSON (o driver tolera
os dois). **STATELESS:** o servidor não guarda conversa — o **histórico inteiro
vai a cada volta** (as sessões são locais no CLI, D7).

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

## 3. Stream de eventos (SSE)

Cada evento é uma linha `data: {json}` terminada por linha em branco (`\n\n`):

```
data: {"kind":"<tipo>","data":{...}}
```

| `kind` | quando | `data` |
|--------|--------|--------|
| `model` | 1º, sempre | `{name, model, server, server_label, target_source, request_id}` — cabeçalho da sessão |
| `text_delta` | streaming da resposta | `{delta:"<pedaço de texto>"}` — concatenar |
| `reasoning_delta` | modelos com "pensar" | `{delta:"..."}` — pensamento; o CLI pode mostrar esmaecido ou ocultar |
| `tool_call` | modelo pediu ferramenta | `{id, name, arguments:{...}}` — **arguments já é objeto** (decodificado) |
| `usage` | antes do `done` | `{prompt_tokens, completion_tokens, total_tokens, thinking_time_ms, response_time_ms, effort, cost, cost_estimated}` |
| `error` | falha | `{message:"..."}` — seguido de `done` |
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
     - sem tool_calls?  → é a resposta final; encerra o turno
5. teto de voltas por turno (SECURITY §2.7, default 100; `max_voltas` no
   config.toml ou env `SHVIA_MAX_VOLTAS`) — estoura: terminal interativo
   pergunta se continua mais um lote; host NDJSON persistente encerra o
   turno com a sessão viva (reenviar `{"type":"user","text":"continua"}`
   retoma); `-p`/`--json -p` terminam sem persistir nada — o aviso manda
   rodar de novo com teto maior
```

O **servidor nunca executa ferramenta** — só devolve `tool_call`. Execução (e os
gates de aprovação) são 100% do cliente. Ver `SECURITY_GUIDELINES.md` §3.

## 5. Erros

| Situação | Forma |
|----------|-------|
| Perfil mal-formado / servidor inválido | `400 {detail}` (antes do stream) |
| Perfil bloqueado pro usuário | `403 {detail}` |
| Validação de campos | `422 {errors}` |
| Servidor de inferência off/erro | evento SSE `error` + `done` (stream já aberto) |
| Rate limit | `429` (Retry-After) |

O CLI deve tratar tanto o **JSON de erro pré-stream** (4xx antes do `text/event-stream`)
quanto o **evento `error` no meio do stream**. Nunca ecoar a API key em log.

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
