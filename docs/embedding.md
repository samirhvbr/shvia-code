# Embutindo o motor do `anna` (protocolo NDJSON)

Como uma casca externa (a aba **Agente do SShvTerm**, uma extensão de editor, um
job de CI) dirige o motor do `anna` sem falar ANSI. O mesmo loop que move o
terminal move o host — via um `Frontend` que fala **NDJSON** (`src/json.rs`).

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
| `{"type":"user","text":"..."}` | inicia um turno (modo persistente) |
| `{"type":"exit"}` | encerra o motor |
| `{"id":"…","decision":"approve"\|"always"\|"reject"}` | **resposta a um `gate_request`** (o `id` correlaciona a proposta; o motor bloqueia por ordem, então o `id` é conferência) |

### `anna` → Host (stdout)

| Evento | Payload | Significado |
|--------|---------|-------------|
| `model` | `model`, `server` | modelo/infra escolhidos no turno |
| `text` | `delta` | pedaço da resposta (streaming) |
| `tool_call` | `id`, `name`, `arguments` | a Anna vai usar uma ferramenta |
| `tool_result` | `id`, `name`, `bytes`, `content` | resultado da ferramenta (já executada) |
| `gate_request` | `id`, `scope`, `policy`, `preview` | **pede aprovação** — o motor BLOQUEIA até a decisão (`id` = o `tool_call.id`, casa com o `tool_result`) |
| `usage` | `tokens`, `cost`, `estimated` | telemetria do turno |
| `turn_done` | — | turno terminou; pode mandar o próximo `user` |
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
- **`--tools host`** (só faz sentido com `--json`): o `anna` **PROPÕE** — emite
  `tool_call` e **não executa nem aprova**. O host roda com a política/PTY/SSH/
  auditoria dele e injeta o resultado no stdin:

  ```
  anna → {"type":"tool_call","id":"c1","name":"bash","arguments":{"command":"ls"}}
  host → (aplica SUA política; executa onde o usuário está)
  host → {"type":"tool_result","id":"c1","content":"a.rs\nb.rs\n"}
  ```

  Neste modo **não há `gate_request`** — quem aprova é o host. É o **Modelo B** do
  ADR-002, feito pra aba Agente do SShvTerm (que já tem política + exec no host).

## Segurança

- Chave só via env (§ SECURITY 2.4); o host repassa no ambiente do filho.
- O motor confina tudo à raiz do projeto (guard) e mantém a denylist de segredos.
- Em `--tools host`, o confinamento e a execução são do host — o `anna` só propõe;
  a política do host (allow/ask/deny) é que manda.
