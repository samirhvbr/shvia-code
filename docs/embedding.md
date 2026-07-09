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
  argumento (aparece em `ps`). `--model`/`--url` são opcionais.

## Fluxo (stdin ↔ stdout)

Tudo é **uma linha JSON por mensagem** (NDJSON), nos dois sentidos.

### Host → `anna` (stdin)

| Linha | Quando |
|-------|--------|
| `{"type":"user","text":"..."}` | inicia um turno (modo persistente) |
| `{"type":"exit"}` | encerra o motor |
| `{"decision":"approve"\|"always"\|"reject"}` | **resposta a um `gate_request`** (só enquanto um está pendente) |

### `anna` → Host (stdout)

| Evento | Payload | Significado |
|--------|---------|-------------|
| `model` | `model`, `server` | modelo/infra escolhidos no turno |
| `text` | `delta` | pedaço da resposta (streaming) |
| `tool_call` | `id`, `name`, `arguments` | a Anna vai usar uma ferramenta |
| `tool_result` | `id`, `name`, `bytes`, `content` | resultado da ferramenta (já executada) |
| `gate_request` | `scope`, `policy`, `preview` | **pede aprovação** — o motor BLOQUEIA até a decisão |
| `usage` | `tokens`, `cost`, `estimated` | telemetria do turno |
| `turn_done` | — | turno terminou; pode mandar o próximo `user` |
| `error` / `warn` | `message` | erro fatal do turno / aviso |

`policy` ∈ `auto` (nunca pede) · `confirm` (`approve`/`always`/`reject`) ·
`always` (destrutivo — `always` não vale, só `approve`/`reject`).

`preview` (o que aprovar):
- `{"kind":"diff","path":"src/x.rs","diff":"...unificado sem cor..."}`
- `{"kind":"command","command":"cargo test","why":"..."}`
- `{"kind":"commit","stat":"...","message":"..."}`

### O handshake de aprovação

```
anna → {"type":"gate_request","scope":"edit_file","policy":"confirm",
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

## Segurança

- Chave só via env (§ SECURITY 2.4); o host repassa no ambiente do filho.
- O motor confina tudo à raiz do projeto (guard) e mantém a denylist de segredos.
- **Quem executa as ferramentas:** no `--json` atual, o **próprio `anna`** executa
  local (`bash` roda na máquina onde o `anna` está). Para o SShvTerm — que executa
  no terminal visível / host SSH — há um segundo modelo (anna PROPÕE, o host
  executa). Ver [decisoes.md](decisoes.md) **ADR-002**.
