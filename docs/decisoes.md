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

---

## ADR-002 — C5: motor embutível (NDJSON) e como plugar na aba Agente do SShvTerm

- **Data:** 09/07/2026 · **Status:** Parcial — o motor NDJSON está **Aceito e
  codado** (0.5.0); a escolha do **modelo de execução** (A vs B) está **Proposta**,
  aguardando OK antes de tocar no `SSHVTERM-DESKTOP`.
- **Contexto:** o C5 quer "o mesmo motor do `anna` movendo a aba Agente do
  SShvTerm". Ao mapear o `~/x/SSHVTERM-DESKTOP` (branch `claude/ai-agent-vm-access-dkzii2`)
  descobrimos que a aba Agente **já é um agente completo**, e **em Python** (não
  Rust): um **sidecar** (WebSocket JSON-RPC) com providers (Anthropic/OpenAI/SHVIA),
  **engine de política** allow/ask/deny, detecção de destrutivo, **redação de
  segredos** (fail-closed), **diálogo de aprovação** (⚙ "quer rodar um comando" →
  Aprovar/Negar) e — crucial — **execução no terminal VISÍVEL / host SSH** (PTY
  injection + exec remoto), com auditoria. A casca Rust (`gui/src-tauri`) só sobe o
  sidecar. Seam limpo identificado: um **provider externo** no sidecar
  (`sidecar/agent/providers/external.py`) que spawna um binário e consome eventos.
- **Decisão 1 (Aceita):** o motor do `anna` vira **embutível** por um trait
  `Frontend` (render de eventos + decisão de aprovação) com dois impls: `Ui`
  (terminal ANSI+stdin, comportamento intacto) e **`JsonSink`** (`anna --json`,
  NDJSON bidirecional pelo stdio). As ferramentas deixaram de imprimir direto —
  montam um `Preview` e o frontend renderiza. Contrato: [embedding.md](embedding.md).
- **Decisão 2 (Proposta) — quem executa as ferramentas:**
  - **A · `anna` executa (local):** o loop roda as tools na máquina do `anna` e
    emite `gate_request` pra aprovação. É o `--json` atual. Bom pra embutir local
    (GUI simples, CI). **Ruim pro SShvTerm**: o `bash` rodaria na máquina do sidecar,
    não no **terminal visível / host SSH** que é todo o valor do SShvTerm — e
    ignoraria a política/PTY/auditoria que já existem lá.
  - **B · `anna` PROPÕE, o SShvTerm executa:** o `anna` emite `tool_call` como
    proposta (não executa) e recebe o `tool_result` de volta pelo stdin; o sidecar
    roda via a **máquina que já tem** (política allow/ask/deny + PTY/SSH + redação +
    auditoria). O `anna` é o **cérebro**; o SShvTerm são as **mãos, no lugar certo**.
  - **Recomendação:** **B para o SShvTerm** (reusa exec/política/host-remoto/auditoria,
    zero duplicação), mantendo **A** como default pro embed local. B exige um modo
    novo no `anna` — `--tools host` (propõe-e-espera) — sobre uma abstração
    `ToolRunner` (local vs delega-ao-host). Isso é **C5.2**, e o lado Python
    (`external.py` no sidecar) é **C5.3**.
- **Consequências:** C5.1 (motor NDJSON) fecha sem tocar no SShvTerm. A ponte real
  (C5.2 `ToolRunner`/`--tools host` no `anna` + C5.3 provider no sidecar) só começa
  após o OK do modelo B — e mexe na branch ativa do `SSHVTERM-DESKTOP`, então entra
  como trabalho combinado, não silencioso.
