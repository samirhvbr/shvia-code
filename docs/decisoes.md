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
  SShvTerm". Ao mapear o `~/x/SSHVTERM/SSHVTERM-DESKTOP` (branch `claude/ai-agent-vm-access-dkzii2`)
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

---

## ADR-003 — Pipeline de papéis (`/plano`): arquiteto → dev aprova → executor → revisão

- **Data:** 31/07/2026 · **Status:** Aceito (v0 codado, 0.9.0)
- **Contexto:** o turno normal da Anna é um modelo fazendo tudo no mesmo
  contexto: entender, planejar, editar e conferir. Funciona bem no pequeno e
  degrada no médio — planejamento raso vira execução errada, e não há ninguém
  conferindo o diff contra o que foi combinado. O `traycer` (clonado em
  `~/x/SHVIA/traycer` como referência) resolve isso quebrando o trabalho em
  fases; o que faz aquilo funcionar não é a escolha de modelo por fase, é o
  **contrato entre as fases**.
- **Decisão 1 — papéis com artefato tipado, não roteador.** Três papéis
  (arquiteto/executor/revisor); o que atravessa a fronteira é `Plano` ou
  `Veredito` validado por schema, nunca o histórico bruto. Contexto não viaja
  entre papéis: cache de prompt é escopado por modelo, então repassar histórico
  pagaria cache frio sem ganhar nada. Schema inválido faz a fase falhar alto
  (com uma segunda tentativa levando o erro de volta pro modelo) em vez de
  degradar em silêncio na fase seguinte.
- **Decisão 2 — a tabela papel → (perfil, esforço) é ESTÁTICA**, no `config.toml`
  (`[elenco.*]`). Um modelo-roteador classificando o pedido seria mais uma
  chamada paga, erraria a classificação e não daria pra auditar. Estático é
  grátis, versionável e visível no `anna config`. Esforço entra como segunda
  dimensão porque é mais barato que trocar de perfil.
- **Decisão 3 — só-leitura por construção.** `runner::SomenteLeitura` filtra as
  ferramentas do arquiteto e do revisor **e** recusa a chamada se o modelo
  insistir. Não é instrução de prompt: eles não escrevem porque não têm como.
  `bash` fica fora da lista (comando que lê e comando que grava entram pela mesma
  porta).
- **Decisão 4 — dois gates de revisão, o barato primeiro.** Gate 0
  determinístico (clippy/testes) antes do gate 1 (revisor-modelo): rodar um
  modelo num diff que nem compila é pagar pra ouvir o que o compilador já disse
  melhor. O gate 0 só executa comando que o `bash` já auto-aprovaria
  (`Policy::Auto`) — não pode virar porta dos fundos pra rodar comando sem o dev
  ver, o que contraria a regra de produto "nada roda sem o dev ver".
- **Decisão 5 — o revisor julga contra o PLANO, não "revisa o código".** Os
  `aceite` de cada passo são a régua. Pergunta aberta sempre acha mais uma coisa
  e o loop não converge. Freios contra ping-pong: teto de 2 voltas por gate e
  dedup por achado (`passo_id|aceite_violado`).
- **Decisão 6 — o gate humano fica no plano, e `plano_errado` para o pipeline.**
  Replanejar no meio do voo é caro, lento e tira o dev do fio — e ele está
  sentado ali. Fechar o loop de volta no arquiteto fica pro v1, se a prática
  mostrar que vale.
- **Consequências:** três módulos novos (`orquestrador`, `plano`, `papel`), um
  decorador em `runner`, e `agent::run_turn` passa a devolver o texto final do
  turno (o REPL descarta; as fases que produzem artefato precisam). Nada muda no
  caminho normal da Anna. Escopo do v0 e o que falta: [orquestrador.md](orquestrador.md).

---

## ADR-004 — The terminal's look: restyle the output, keep `rustyline`, one module owns the escape codes

- **Data:** 30/09/2026 · **Status:** Aceito (em entrega: 0.12.3 a 0.12.6)
- **Context:** the owner asked for `anna` to look more like Claude Code or Codex, "the look
  first, features later". What was there: ANSI escape codes written by hand in four modules
  (`ui.rs`, `render.rs`, `diff.rs`, `stats_term.rs`), no test for a terminal, no `NO_COLOR`, no
  width, no progress while the model works, tool calls shown as raw JSON, approvals answered by
  typing a letter and Enter. Colour leaked into pipes: `anna -p ... | tee log` carried the codes.
- **Decision 1 — Claude Code is the reference.** Owner's answer on 30/09/2026, among Claude
  Code, Codex and a hybrid. The vocabulary of the product already matched (`tools`, diff plus
  confirmation), and the README's promise is "the Claude Code experience". Concretely: a
  welcome box, `●` for an action, `⎿` for its result, a spinner with elapsed time, diffs with
  line numbers and a coloured background, approvals as a list picked with the arrow keys.
- **Decision 2 — restyle what `anna` prints; do not build a full-screen interface.** The output
  stays in the terminal's own scrollback (no alternate screen), so copying and scrolling keep
  working, and the prompt stays `rustyline`. `ratatui` is **not** adopted now: a composer of its
  own (input box, `/command` autocomplete, fixed status bar) would replace `rustyline` and have
  to coexist with streaming and with the approval gates, for a gain the owner can judge only
  after seeing the rest. It is stage 5 and is reopened by a new ADR if the restyle is not enough.
- **Decision 3 — `term.rs` is the only place that writes an escape code.** It decides once
  what the terminal supports: colour is written only when stdout is a terminal and `TERM` is not
  `dumb`, `FORCE_COLOR` forces it, `NO_COLOR` (no-color.org) wins over both. The decision is a
  pure function of the environment, tested without a terminal. Everything else asks `term`.
- **Decision 4 — `--json` does not change, and `-p` keeps its information.** Without colour the
  output carries the same information in the same order, with no escape codes; the marks are
  Unicode (`●`, `⎿`) and blank lines between blocks appear only on a terminal. What `-p | cmd`
  used to print as `ANNA` and `⚙ tool(json)` is now `●` and `● Tool(arg)`: whoever parsed those
  lines was parsing decoration. The `--json` host (SHVIA-DESKTOP, SShvTerm) talks through
  `JsonSink`, a different `Frontend`, which this work does not touch.
- **Dependencies.** `crossterm` (already planned in `arquitetura-cli.md` §3: width, raw mode and
  keys for the approval menu) and `unicode-width` (already in the tree through `rustyline`: the
  visible width of a line, for boxes, tables and the diff background). Each enters in the
  version that uses it, and `cargo audit` gates each one.
- **Consequences:** four deliveries, one subject each — the base (0.12.3), the turn's output
  (0.12.4), diff and approval (0.12.5), markdown (0.12.6). The ruler `arvore_da_doc_bate_com_src`
  requires every new `src/*.rs` to be in `arquitetura-cli.md` §2.
