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
