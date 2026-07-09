# Estado atual — SHVIA-CODE

- **08/07/2026** — Nasce o repo com o **escopo aprovado** (0.0.1) + segurança e
  perfil `.claude` (0.0.2). Decisões travadas: `anna` · Rust · modelos livres
  (ADR-001). Remoto privado: `github.com/samirhvbr/SHVIA-CODE`.
- **08/07/2026 — C0 em andamento (código pronto, falta EXECUTAR):**
  1. ✅ Servidor: `POST /api/v1/code/chat` no ar (SHVIA-WEB **2.16.0**) — SSE
     stateless com `tools[]`/`tool_choice`/`role=tool`; eventos `model →
     text_delta/reasoning_delta/tool_call → usage (custo) → done`; auditoria/
     LGPD/quotas herdadas; servidor não executa tool nenhuma.
  2. ✅ Cliente do spike: [`spikes/c0-gateway.php`](../spikes/c0-gateway.php) —
     loop completo com 2 tools read-only confinadas a sandbox em /tmp
     (list_dir/read_file, denylist de .env ativa) e um bug proposital pra Anna
     achar (`config.php: 'scheme' => 'htps'`).
  3. ⏳ **EXECUTAR** (após o deploy do 2.16.0 no servidor):
     ```bash
     SHVIA_API_KEY=<sua chave>  php spikes/c0-gateway.php                     # perfil default
     SHVIA_PROFILE="<model@server>" SHVIA_API_KEY=... php spikes/c0-gateway.php  # on-prem e nuvem
     ```
     **DoD:** transcript do loop fechado (com um modelo on-prem E um de nuvem),
     colado AQUI — é a medida da qualidade de tool-calling antes do C1 (CLI Rust).
- **08/07/2026 — material de produção pronto (0.0.4):** kit pra atacar o C1 sem
  ambiguidade:
  - [`docs/protocolo.md`](../docs/protocolo.md) — contrato CLI↔servidor do
    `/api/v1/code/chat` (eventos SSE, shapes, o loop, erros, exemplo).
  - [`docs/ferramentas.md`](../docs/ferramentas.md) — catálogo v1 (schemas JSON,
    gates auto/confirm/always, denylist, formato do resultado).
  - [`docs/arquitetura-cli.md`](../docs/arquitetura-cli.md) — árvore de módulos
    Rust, tabela de deps c/ rationale, pseudocódigo do loop, config/sessões.
  - **Esqueleto Rust que compila**: `Cargo.toml` + `src/main.rs` (`anna` /
    `anna config`, versão do `version.md`), `build-local.sh` (preflight + timing
    + checksum, padrão 0.5.8). `cargo check` verde.
- **Sequência do C1** (arquitetura §7): api(sse) → agent(loop) → tools(fs+guard,
  read-only) → gate(read) → ui(repl) → telemetry. Fecha quando `anna` conversa e
  explica um repo real. Depende do transcript do C0 pra escolher o modelo.
