# SHVIA-CODE — Decisões (ADRs)

Formato ADR. Não relitigar direção já decidida dentro de um how-to — linkar o ADR.

---

## ADR-001 — Nasce o SHVIA-CODE: CLI `anna` em Rust, loop no cliente, sobre o gateway ShvIA

- **Data:** 08/07/2026 · **Status:** Aceito
- **Contexto:** A Blue3 quer "um Claude Code do ShvIA": agente de código no
  terminal usando a infra própria. O commit `Drivers` do SHVIA-WEB (06/07)
  colocou **tool-calling nos 3 drivers** (`ToolCall`, `onToolCall`,
  `tools`/`tool_choice` no `ChatService`) — falta só expor na borda HTTP. O
  resto (auditoria, quotas, custo real/estimado, LGPD `maskOutboundPii` +
  `data_locality`, API key, BYOK) já existe e é herdado.
- **Decisões (travadas com o Samir em 08/07):**
  1. **Repo próprio `SHVIA-CODE`**; comando **`anna`** (persona do ShvIA).
  2. **Stack Rust** — binário único sem runtime, distribuição trivial, casa já
     domina (Tauri/SSHVTERM). Alternativas TS/PHP descartadas (runtime/
     distribuição).
  3. **Modelos livres (D6)** — o dev usa qualquer perfil que já tem no ShvIA
     (default = o do painel; troca via `/model`), sem flag/fricção extra no
     CLI. Guardrails LGPD do gateway continuam valendo server-side,
     automáticos. Sigilo total = escolher perfil on-prem.
  4. **Loop agêntico no cliente** (como o Claude Code): o CLI orquestra; tools
     executam **localmente** com gates (leitura auto; escrita diff+confirmação;
     bash allowlist/confirmação; destrutivo sempre confirma — padrão provado na
     aba Agente do SShvTerm). Servidor stateless de ferramentas.
  5. **Endpoint dedicado** `POST /api/v1/code/chat` no SHVIA-WEB (SSE; aceita
     `messages[]` com `role=tool`, `tools[]`, `tool_choice`) — system prompt e
     gate de acesso próprios, sem arriscar o `/chat` do painel.
  6. **Auth = API key** existente do ShvIA; sessões **locais** no v1.
- **Consequências:** o trabalho novo = CLI Rust + 1 endpoint. Fases C0–C5 no
  [escopo](../.continue/escopo-projeto.md); o **C0 (spike) mede a qualidade de
  tool-calling dos modelos** antes de investir no resto. Riscos e não-objetivos
  idem no escopo.
