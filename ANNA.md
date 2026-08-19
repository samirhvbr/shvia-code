# ANNA.md — contexto pra Anna trabalhar neste repo

> Este arquivo é lido pelo próprio `anna` como contexto do projeto (o
> `SHVIA-CODE`). Mantenha curto e acionável — vira parte do system prompt.

## O que é

Você (`anna`) É este repo: o **CLI agêntico de código da Blue3**, em **Rust**,
sobre o **gateway do ShvIA** (`POST /api/v1/code/chat`). O loop roda no cliente;
o servidor só infere. Binário único `anna`.

## Invariantes (não relitigar sem ADR)

- **Loop no cliente**; servidor é stateless de tools. Nada roda/escreve sem o dev ver.
- **Gates**: leitura = auto · escrita = diff + confirmação · bash = allowlist roda,
  destrutivo confirma sempre, resto confirma. **Sem modo yolo.**
- **API key** vem de `SHVIA_API_KEY`/config 0600 — **nunca** de argumento de CLI, nunca logada.
- **Denylist de segredos** (`.env*`, chaves, credenciais) vale na leitura E na escrita.
- Modelos livres (qualquer perfil); LGPD/custo/quotas herdados do servidor.

## Como mexer (mantenha verde)

- `cargo clippy --all-targets -- -D warnings` **tem que passar limpo**.
- `cargo test` **tem que passar** (hoje 10 testes).
- Build: `./build-local.sh` (preflight + release + checksum). Instalar: `./install.sh`.
- Faça a **menor mudança** que resolve. pt-BR nos comentários e mensagens.

## Commits (obrigatório)

- Formato: `versão - comentário em português`. A versão vem de **`version.md`**
  (bumpe no MESMO commit; espelhe em `Cargo.toml`).
- Critério: **Z** = UX do CLI/build/docs · **Y** = tool nova ou mudança de
  protocolo com o gateway · **X** = estável.
- Push direto na `master` (sem branch/PR). Proibido `feat:`/`fix:`/`chore:`.

## Mapa dos módulos (`src/`)

- `main` — REPL + modo `-p` + subcomandos (`config`, `sessions`) + slash-commands.
- `agent` — o loop (deltas → tool_calls → executa local → repete; teto
  `max_voltas` por turno, default 100 — estouro pergunta ao dev no terminal).
- `api` + `types` — POST e parser do SSE; structs do protocolo.
- `tools` — read/list/grep/glob · edit/write · bash · git. Registry (`defs`+`run`).
- `guard` — denylist + confinamento à raiz (`resolve`/`resolve_new`).
- `gate` — políticas Auto/Confirm/Always (o `[s]im/[n]ão/[t]udo`).
- `diff` (similar) · `render` (markdown por linha) · `session` (persistência) ·
  `project` (raiz git + este arquivo) · `ui` · `telemetry` · `config`.

## Referências

`README.md` · `.continue/escopo-projeto.md` (fases C0–C5) · `docs/` (protocolo,
ferramentas, arquitetura) · `SECURITY_GUIDELINES.md` · `docs/decisoes.md` (ADRs).
