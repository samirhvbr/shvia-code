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
- **08/07/2026 — C1 CODADO (0.1.0): o `anna` de verdade compila, roda e é testado.**
  Módulos: `main`(REPL + `-p`) · `config`(env/toml, chave nunca em arg) · `types`
  (protocolo) · `api`(POST + parser SSE) · `agent`(o loop) · `tools`(read_file/
  list_dir/grep/glob, respeita .gitignore) · `guard`(denylist+confinamento) ·
  `gate`(Auto no C1; Confirm/Always prontos p/ C2) · `project`(raiz git + ANNA.md
  + system prompt) · `ui`(streaming/tool lines/rodapé) · `telemetry`(custo/turno).
  - ✅ `cargo clippy -D warnings` limpo · **5 testes** (denylist, confinamento,
    parse SSE, read/grep/glob) verdes · `anna config` roda · `-p` sem chave =
    erro amigável.
  - ⏳ **Falta o smoke ao vivo** (precisa da API key + deploy do 2.16.0):
    ```bash
    ./build-local.sh                      # gera target/release/anna
    cd ~/x/QUALQUER-REPO
    SHVIA_API_KEY=<chave> ~/x/SHVIA/SHVIA-CODE/target/release/anna
    > explique este projeto / onde está o bug em X?
    ```
    (o `spikes/c0-gateway.php` já provou o protocolo; isto prova o CLI Rust.)
- **09/07/2026 — C2 CODADO (0.2.0): as mãos que escrevem.**
  - `edit_file` (substituição exata, exige old_string único) e `write_file`
    (cria/sobrescreve) — ambos mostram **diff colorido** (`similar`) e pedem
    **confirmação** (`[s]im/[n]ão/[t]udo`); denylist de segredos vale na escrita.
  - `bash` — classifica: **allowlist** (ls/cat/cargo test/…) roda direto;
    **destrutivo** (rm -rf, git push, curl|sh, drop table…) confirma SEMPRE;
    resto confirma. Metacaractere (`>`,`|`,`;`,`&&`,`$(`) derruba do allowlist.
  - `git_status`/`git_diff` (Auto) + `git_commit` (mostra diff staged, confirma).
  - Gate `Confirm`/`Always` agora vivos; modo `-p` nega escrita por padrão (seguro).
  - ✅ clippy `-D warnings` limpo · **9 testes** verdes (inclui o caminho de
    escrita real via gate aprovado, e a classificação de comandos) · binário release.
- **09/07/2026 — C3 CODADO (0.3.0): ergonomia do terminal.**
  - **Sessões locais** (`~/.shvia-code/sessions/<id>.json`, 0600): autosave a cada
    volta (título = 1ª pergunta); `anna sessions list|resume <id>|clear` + `/sessions`
    no REPL. Round-trip validado (cria→list→clear).
  - **`/model [perfil]`** troca o modelo no meio da conversa (+ flag `--model` no
    start); `/tools` lista as ferramentas; `/help`/`/clear` atualizados.
  - **Markdown na resposta** (`render.rs`): headings, listas, negrito/itálico/código
    inline + blocos ``` — estilizado **por linha** (não quebra o streaming).
  - `Message` ganhou `Deserialize` (persistência de sessão).
  - ✅ clippy `-D warnings` limpo · **10 testes** verdes (novo: render markdown) ·
    smoke offline ok (`config`, `sessions`, `--help`, round-trip).
- **09/07/2026 — C4 CODADO (0.4.0): distribuição + preparo do dogfood.**
  - **`install.sh`** põe o `anna` no PATH (~/.local/bin, sem sudo), reusando o
    build-local (preflight+release+checksum); validado (instala + `anna --version`).
  - **`ANNA.md`** = contexto do próprio repo, lido pelo `anna` (prioridade sobre
    CLAUDE.md via project.rs): invariantes, manter-verde, commit, mapa de módulos.
  - **`scripts/dogfood.sh`** = harness do DoD (tarefas leitura/bash/git em `-p`
    com a chave; senão orienta; edição/commit ficam no REPL por causa dos gates).
  - **FIX:** o system prompt (`project.rs`) ainda dizia só-leitura (resquício do
    C1) → agora descreve leitura+escrita+bash+git e o fluxo com gates. README atualizado.
  - ✅ clippy `-D warnings` limpo · 10 testes verdes · install.sh rodado de verdade.
- **09/07/2026 — C5.1 CODADO (0.5.0): o motor virou EMBUTÍVEL.**
  - Trait **`Frontend`** (render de eventos + decisão de aprovação) com 2 impls:
    `Ui` (terminal ANSI+stdin, comportamento intacto) e **`JsonSink`** (`anna --json`,
    **NDJSON** bidirecional). As ferramentas não imprimem mais direto — montam um
    `Preview` (diff/comando/commit) e o frontend renderiza. `diff` ganhou modo sem-cor.
  - `anna --json` (persistente: host manda `{"type":"user",...}`; one-shot com `-p`);
    aprovação por handshake (`gate_request` → host responde `{"decision":...}` no stdin).
  - Protocolo: [docs/embedding.md](../docs/embedding.md). Decisão: [ADR-002](../docs/decisoes.md).
  - ✅ clippy `-D warnings` limpo · **10 testes** verdes (tests de escrita agora usam
    `Ui` como Frontend) · smoke offline do `--json` ok (evento de erro NDJSON válido).
- **Achado que reformata o C5:** a aba Agente do **SShvTerm** (`~/x/SSHVTERM/SSHVTERM-DESKTOP`,
  branch `claude/ai-agent-vm-access-dkzii2`) **já é um agente completo, em Python**
  (sidecar WS-RPC): providers, política allow/ask/deny, redação de segredos, diálogo
  de aprovação, e **execução no terminal visível / host SSH** + auditoria. Seam:
  provider externo `sidecar/agent/providers/external.py` que spawna o `anna` e lê NDJSON.
- **DECISÃO (ADR-002): Samir escolheu o Modelo B** (09/07) — o `anna` PROPÕE, o
  SShvTerm executa no lugar certo (política/PTY/SSH/auditoria dele).
- **09/07/2026 — C5.2 CODADO (0.5.1): o lado `anna` do Modelo B.**
  - Abstração **`ToolRunner`**: `LocalRunner` (o `anna` executa — default, terminal
    e `--json` sem host) e **`HostRunner`** (o `anna` só propõe: emite `tool_call`,
    NÃO executa nem aprova, e espera o host injetar `{"type":"tool_result",...}` no
    stdin, casando por id). `agent::run_turn` recebe `&mut dyn ToolRunner`.
  - Flag **`--tools local|host`** (host só com `--json`). Em `host` não há
    `gate_request` — quem aprova é o host. Doc: embedding.md §Execução.
  - ✅ clippy limpo · 10 testes verdes · `--tools host` parseia e smoke NDJSON ok.
    (O loop host-exec completo só se prova com o gateway + o lado Python.)
- **Próximo: C5.3 — o provider no sidecar Python do SShvTerm** (`~/x/SSHVTERM/SSHVTERM-DESKTOP`,
  `sidecar/agent/providers/external.py`): spawna `anna --json --tools host`, relaia
  os eventos NDJSON pro loop `_run_and_respond` que já tem (aprovação/política/PTY/SSH/
  auditoria). Mexe na branch ativa `claude/ai-agent-vm-access-dkzii2` → trabalho combinado.
- **Falta ainda (bloqueio geral): SMOKE AO VIVO** — `SHVIA_API_KEY` + gateway 2.16.0:
  fecha o C0 (`spikes/c0-gateway.php`) e o dogfood do C4, e valida o stream `--json` real.
- **11/07/2026 — Checkpoint git automático + `/rollback` (0.8.0)** — do pool
  "fáceis do Hermes" (comparativo Hermes×SHVIA):
  - `src/checkpoint.rs`: antes de toda mutação APROVADA (edit/write/bash
    destrutivo), foto do working tree em `refs/anna/checkpoint` via índice
    temporário (`GIT_INDEX_FILE` + write-tree/commit-tree) — índice real, branch
    e `git status` intocados; debounce 30 s; silencioso fora de git.
  - REPL: `/rollback` (diff --stat desde a foto) e `/rollback aplicar` (restaura
    conteúdo RASTREADO; arquivos criados pós-foto permanecem). Doc:
    [docs/ferramentas.md §5](../docs/ferramentas.md). Restore é do DEV, nunca tool.
  - ✅ clippy limpo · **14 testes** (2 novos: roundtrip foto→estrago→restore em
    sandbox git; fora-de-git silencioso) · sincronizado o drift `Cargo.toml`
    (0.6.0) → `version.md` (0.8.0).
- **16/07/2026 — `anna stats`: o dashboard "Wrapped" (0.8.4)** — tela de uso
  **100% local** ("O que vem a seguir, <você>?"), do pool comparativo com o
  Claude Code:
  - `src/stats.rs` — o **cérebro** puro/testável: agrega
    `~/.shvia-code/sessions/` em sessões, mensagens, tokens, dias ativos,
    sequência atual/maior, horário de pico, modelo favorito e um heatmap de
    contribuição; `compute_at(.., hoje)` recebe o "hoje" injetado (os testes
    independem do fuso da máquina).
  - Render em duas peças que só desenham o que o cérebro agregou:
    `src/stats_term.rs` (terminal ANSI — 8 tiles box-drawing + heatmap azul
    256-cores + a frase do Moby-Dick) e `src/stats_html.rs` (export **offline**,
    CSS/JS embutidos: abas Visão Geral/Modelos + filtros Todos/30d/7d ao vivo,
    sem duplicar regra — o Rust pré-computa as 3 faixas, o JS só troca a view).
  - **`anna stats`** (`--tab`/`--range`/`--html`/`--open`/`--out`) + **`/stats`**
    no REPL. Doc: [docs/stats.md](../docs/stats.md).
  - **A sessão passou a PERSISTIR `tokens`/`cost`/`model`** no autosave
    (`session.rs`, retrocompat via `#[serde(default)]`); só conta das próximas
    sessões — nas antigas, tokens/modelo aparecem como "—". Dep nova: `chrono`
    (feature `clock`, datas/hora **local**).
  - ✅ clippy limpo · **24 testes** (10 novos do stats) · terminal + export HTML
    rodados de verdade. Drift `Cargo.toml`↔`version.md` sincronizado (0.8.4).
- **31/07/2026 — pipeline de papéis `/plano` (0.9.0):** a tarefa deixa de ser um
  modelo fazendo tudo e vira **arquiteto → o dev aprova o plano → executor →
  revisão**. Decisão e rationale: [ADR-003](../docs/decisoes.md) · desenho e
  configuração: [docs/orquestrador.md](../docs/orquestrador.md).
  - **`src/plano.rs`** — os contratos (`Plano`/`Veredito`) com schema validado e
    extrator de JSON tolerante a cerca/prosa (contador de profundidade ciente de
    string: `"troque o { por ["` quebrava a busca ingênua pelo último `}`). É a
    peça que sustenta o resto — os papéis não compartilham contexto, compartilham
    artefato.
  - **`src/papel.rs`** — o **elenco**: tabela `papel → (perfil, esforço)` no
    `config.toml`, **estática de propósito** (roteador-modelo seria mais uma
    chamada paga, erraria e não daria pra auditar). Precedência do esforço:
    elenco > `effort` global > padrão do papel.
  - **`src/orquestrador.rs`** — o pipeline e os dois gates de revisão: gate 0
    determinístico (clippy/testes, **grátis, vem primeiro** — rodar modelo em
    diff que não compila é pagar pra ouvir o compilador) e gate 1 revisor-modelo,
    que julga **contra o plano** (os `aceite` são a régua; pergunta aberta não
    converge). Freios contra ping-pong: teto de 2 voltas + dedup por achado.
  - **`runner::SomenteLeitura`** — arquiteto e revisor não escrevem **por
    construção**: filtra as defs e recusa a chamada. Não é instrução de prompt.
  - `agent::run_turn` passa a devolver o texto final do turno (o REPL descarta;
    as fases que produzem artefato precisam). `Project::bloco_contexto()`
    extraído do system prompt pros papéis reusarem.
  - ✅ clippy limpo · **43 testes** (9 novos: contratos, só-leitura, precedência
    do elenco, e um alarme travando o gate 0 na allowlist do `bash`). Drift
    `Cargo.toml` (0.8.8) ↔ `version.md` (0.8.10) sincronizado em 0.9.0.
  - ⚠️ **Nunca rodou contra um modelo** — só contra os testes de contrato.

## 📌 Pendente de produção/distribuição (11/07/2026)

O anna é CLI **client-side** — "produção" = binário novo nos lugares que o usam:

- [ ] **Rebuild/reinstalar** o `anna` 0.8.0 nos hosts de uso (`build-local.sh` /
      `install.sh`).
- [ ] **SHVIA-DESKTOP (Modo Code)**: o desktop embarca o anna como sidecar —
      re-empacotar o desktop com o binário 0.8.0 para o checkpoint/rollback valer lá.
- [ ] **Gate manual**: num repo git de teste, mandar a Anna editar um arquivo →
      aprovar → conferir `git log refs/anna/checkpoint` (foto criada, `git status`
      limpo) → `/rollback` → `/rollback aplicar` devolve o conteúdo.
- [ ] (pré-existente, segue aberto) **SMOKE AO VIVO** com `SHVIA_API_KEY` — fecha
      C0 + dogfood C4; e **C5.3** (provider Python no SShvTerm).
- [ ] **`/plano` ao vivo (0.9.0)**: configurar o `[elenco]` com perfis reais e
      rodar numa tarefa pequena de verdade. O que só se mede rodando: se o
      arquiteto escreve `o_que` autossuficiente, se os `aceite` são régua o
      bastante pro loop fechar, e se o revisor acha o que deve sem virar crítica
      aberta. **O prompt do revisor deve ser reescrito depois de ~10–15 planos
      reais** — o veredito bom sai dos erros que o executor comete NESTE código,
      não dos que dava pra imaginar antes.
