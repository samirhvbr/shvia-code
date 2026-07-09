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
    SHVIA_API_KEY=<chave> ~/x/SHVIA-CODE/target/release/anna
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
- **Próximo: C4 (distribuição/dogfood)** — usar o `anna` pra desenvolver o próprio
  `anna`; empacotar (build-local já gera binário+checksum); ANNA.md do repo. Trava
  só no **smoke ao vivo** (API key + 2.16.0) — o resto do CLI está completo p/ leitura,
  escrita, exec, git, sessões e markdown. Alternativa: **C5** (motor na aba Agente do
  SShvTerm).
