# SHVIA-CODE

CLI agêntico de código da Blue3 — comando **`anna`** (Rust). A experiência do
Claude Code, movida pelo **gateway do ShvIA** (`ia.blue3.com.br`): a Anna lê
arquivos, propõe diffs, edita, roda comandos e commita — **com aprovação** — no
projeto em que o dev está, usando qualquer perfil de modelo do ShvIA (on-prem
ou nuvem BYOK).

> Antes de mexer: **`git pull`**. Convenções em [CLAUDE.md](CLAUDE.md) ·
> Escopo aprovado e fases em [.continue/escopo-projeto.md](.continue/escopo-projeto.md) ·
> Decisões em [docs/decisoes.md](docs/decisoes.md).

## Status

**Escopo aprovado (08/07/2026), pré-código.** Próximo passo: **C0** — spike do
gateway (provar pergunta → `tool_call` → resultado → resposta via
`POST /api/v1/code/chat`; o tool-calling nos drivers já existe no SHVIA-WEB).

## Desenho em uma linha

Loop agêntico **no cliente** (tools executam na máquina do dev, com gates), SSE
contra o gateway do ShvIA (auth por API key) — auditoria, quotas, custo e LGPD
herdados do servidor.

## Fases

C0 spike do gateway → C1 MVP leitura → C2 mãos (edit/bash/git com aprovação) →
C3 ergonomia (sessões, ANNA.md, custo/turno) → C4 distribuição (build-local +
binários + dogfood) → C5 skills/RAG/CI/SShvTerm.
