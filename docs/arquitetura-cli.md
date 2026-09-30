# Arquitetura do CLI `anna` (Rust)

> O plano de build do **C1+** (o binário `anna`). Complementa
> [protocolo.md](protocolo.md) (contrato com o servidor) e
> [ferramentas.md](ferramentas.md) (o que a Anna faz). Decisões de estrutura
> aqui; regras de produto no [CLAUDE.md](../CLAUDE.md); segurança no
> [SECURITY_GUIDELINES.md](../SECURITY_GUIDELINES.md).

---

## 1. Princípios

- **Binário único, sem runtime** (por isso Rust — ADR-001). `anna` roda em
  qualquer máquina com a API key, sem Node/PHP instalados.
- **Loop no cliente**: o servidor infere; as mãos (tools) e os gates são aqui.
- **Streaming first**: a resposta aparece token a token; nada de "spinner e espera".
- **Nada roda/escreve sem o dev ver** (gates — ferramentas.md §1).
- **Camadas finas e testáveis**: api / agent / tools / gate / ui separados; o
  loop não sabe de terminal, as tools não sabem de rede.

## 2. Árvore de módulos

> **Esta seção descrevia um plano, não o código** — até 02/09/2026 ela listava
> `api/{mod,sse,types}.rs`, `tools/{mod,fs,edit,shell,git,guard}.rs`, `ui/{mod,repl,tui}.rs`,
> `cli.rs` e `error.rs`, nenhum dos quais chegou a existir; o desenho por subdiretório foi
> abandonado cedo e ninguém voltou aqui. Pior que estar velha, ela estava sendo **mantida
> velha**: as descrições de `shell.rs` chegaram a ganhar as notas dos achados F-04 e F-05,
> escritas dentro de um arquivo inexistente. Agora a árvore é a real, e a régua
> `arvore_da_doc_bate_com_src` (em `src/main.rs`) reprova a próxima divergência.
> Achado F-10 da revisão de setembro/2026.

O `src/` é **plano**: um arquivo por assunto, sem subdiretório. A separação de camadas que
a §1 promete é de responsabilidade, não de pasta — `agent.rs` não sabe de terminal e
`tools.rs` não sabe de rede, estando os dois no mesmo nível.

```
src/
  tools.rs       TODAS as tools num arquivo: read_file/list_dir/grep, edit_file,
                 write_file, bash e git. Inclui a ALLOWLIST e a classificação por
                 token (F-02/F-03), o teto de tempo e a morte do GRUPO (F-04), e a
                 leitura de destrutivo por token (F-05). É o maior do repo — o
                 cabeçalho do arquivo admite e diz por que ainda não foi partido.  (1471 linhas)
  main.rs        Entrypoint: args (clap, declarados aqui — não há `cli.rs`), config,
                 modo REPL ou `-p/--print` não-interativo. Réguas do repositório.  (761 linhas)
  stats.rs       O cérebro do `anna stats` (a tela "Wrapped"): agrega as sessões
                 locais em números, heatmap e quebra por modelo. Puro e testável.  (549 linhas)
  config.rs      `~/.config/shvia-code/config.toml` (0600) + overrides por env
                 (SHVIA_URL, SHVIA_API_KEY, SHVIA_PROFILE). Chave nunca em arg.  (484 linhas)
  orquestrador.rs Modo autônomo: plano, verificação e retomada entre turnos.  (460 linhas)
  plano.rs       Plano de execução do modo autônomo (passos, estado, retomada).  (302 linhas)
  stats_html.rs  Render das estatísticas em HTML (escapa o que interpola — F-08).  (278 linhas)
  session.rs     `~/.shvia-code/sessions/<id>.json` (0600): salvar, retomar, limpar
                 por idade.  (257 linhas)
  stats_term.rs  Render das estatísticas no terminal.  (252 linhas)
  agent.rs       O LOOP agêntico (§4): monta payload, consome eventos, dispara tools,
                 reenvia. Teto de voltas. Agnóstico de UI.  (221 linhas)
  ui.rs          A interface do terminal: stream, prompts de gate. O `repl.rs`/`tui.rs`
                 do plano não existem; não há TUI (ratatui) neste código.  (218 linhas)
  term.rs        What the terminal supports, decided once (colour: TTY, `NO_COLOR`,
                 `FORCE_COLOR`), and the only module that writes ANSI (ADR-004).  (157 linhas)
  memory.rs      Memória de longo prazo por HTTP — exige URL segura (F-07).  (208 linhas)
  json.rs        `JsonSink` — o `Frontend` que fala NDJSON em vez de ANSI. O host lê
                 os eventos do stdout e responde aprovação pelo stdin.  (167 linhas)
  checkpoint.rs  Checkpoint de segurança: antes de TODA mutação aprovada, fotografa o
                 working tree num commit FORA da branch (índice temporário,
                 `refs/anna/checkpoint`). Não toca o índice do dev.  (150 linhas)
  api.rs         Cliente do gateway (`POST /api/v1/code/chat`) + parser do SSE. Uma
                 chamada = uma "volta". O `sse.rs` separado nunca existiu.  (149 linhas)
  types.rs       Structs do protocolo (docs/protocolo.md). Serde apenas, sem lógica.  (139 linhas)
  runner.rs      QUEM executa as tools (ADR-002): `LocalRunner` roda na máquina do
                 `anna`; `HostRunner` só PROPÕE e espera a casca externa devolver o
                 resultado.  (129 linhas)
  project.rs     Detecta a raiz (git), lê ANNA.md / CLAUDE.md / AGENTS.md, monta o
                 system prompt e o cabeçalho da sessão.  (113 linhas)
  papel.rs       Papéis do pipeline (arquiteto → executor → revisor) e o elenco:
                 a tabela papel → (perfil, esforço) do `config.toml`.  (82 linhas)
  guard.rs       Denylist de leitura + confinamento à raiz do projeto (realpath).  (80 linhas)
  render.rs      Markdown **por linha**, para funcionar com streaming: cada linha é
                 estilizada quando fica completa.  (60 linhas)
  diff.rs        Diff unificado (crate `similar`) — o preview de `edit_file` e
                 `write_file`, colorido no terminal e cru no NDJSON.  (44 linhas)
  gate.rs        Política de aprovação: Allow|Deny; memória do "sempre nesta sessão"
                 por (tool, escopo). Sem yolo.  (43 linhas)
  telemetry.rs   Custo/tokens por turno, a partir do evento `usage`.  (43 linhas)
  frontend.rs    O trait `Frontend`: renderizar os eventos do turno e DECIDIR
                 aprovações. É o que deixa o motor embutível — o mesmo
                 `agent::run_turn` serve o terminal e um host externo.  (38 linhas)
```

Não há `error.rs`: os erros usam `anyhow` no ponto de uso. A regra que aquele módulo
existia para carregar continua valendo e está em `config.rs` — **a API key nunca aparece em
mensagem de erro**.

## 3. Dependências (Cargo)

| Crate | Uso | Por quê |
|-------|-----|---------|
| `tokio` (rt-multi-thread, macros) | async runtime | streaming SSE sem travar a UI |
| `reqwest` (stream, rustls-tls, **sem** default-features) | HTTP/SSE | maduro; **rustls** = TLS sem OpenSSL do SO (SECURITY §4) |
| `eventsource-stream` *(ou parser próprio em `sse.rs`)* | framing SSE | evita reimplementar o `data:`/`\n\n` |
| `serde`, `serde_json` | (de)serialização | payload e eventos |
| `clap` (derive) | args/subcomandos | padrão de CLI em Rust |
| `rustyline` | REPL (C1) | histórico/edição de linha prontos |
| `ratatui` + `crossterm` | TUI (C3) | opcional; só quando o REPL apertar |
| `toml` | config | `~/.config/shvia-code/config.toml` |
| `directories` | caminhos por SO | `~/.config`, `~/.local/share` corretos no mac/Linux |
| `ignore` | walk respeitando `.gitignore` | mesma engine do ripgrep; list/grep/glob |
| `grep-regex`/`regex` | busca | `grep` |
| `similar` | diff | diff unificado colorido pro `edit_file`/`write_file` |
| `anyhow`, `thiserror` | erros | ergonomia + tipos onde precisa |
| `crossterm` (style) | cores | render de markdown/diff |

**Segurança de deps (SECURITY §4):** `Cargo.lock` versionado; `cargo audit` no
build-local; `cargo clippy -D warnings` antes de release; TLS via rustls.

## 4. O loop agêntico (agent.rs)

```rust
// pseudocódigo
async fn turn(state: &mut Session, ui: &dyn Ui, gate: &mut Gate) -> Result<()> {
    let (lote, mut teto, mut volta) = (cfg.max_voltas, cfg.max_voltas, 0);
    while volta < teto || ui.continuar_apos_teto(teto) { // estouro: só o terminal interativo pergunta
        if volta >= teto { teto += lote; }                // cada "sim" = exatamente mais um lote
        volta += 1;
        let resp = api::code_chat(&state.messages, tools::defs(), &cfg).await?;
        // resp já foi renderizada ao vivo pelo ui (text_delta) e coletou tool_calls/usage
        telemetry::add(resp.usage);
        if resp.tool_calls.is_empty() {
            return Ok(());                              // resposta final → fim do turno
        }
        state.push_assistant(resp.text, &resp.tool_calls);
        for call in resp.tool_calls {
            let out = tools::run(&call, ui, gate).await; // executa LOCAL, com gate
            state.push_tool(call.id, out);               // role=tool
        }
    }
    ui.aviso_teto(teto); // conselho por casca: REPL/host vivo = "continua"; -p = teto maior
    Ok(())
}
```

`tools::run` resolve nome→executor no registry, aplica o gate (auto/confirm/
always), executa e devolve o `content` (sucesso/`ERRO:`/`RECUSADO:`).

## 5. Config & sessões

**`~/.config/shvia-code/config.toml`** (0600):
```toml
url           = "https://ai.shvia.org"
profile       = "qwen-coder@shvia1"  # opcional; default do usuário se ausente
max_voltas    = 100                  # teto de voltas por turno (env SHVIA_MAX_VOLTAS ganha)
bash_timeout  = 120                  # teto por comando, em segundos; 0 desliga (F-04)
api_key       = "shvia_usr_…"        # 0600 — use `anna config set-key`, que grava assim
# Precedência: env SHVIA_API_KEY > config. Nunca em argumento de linha.
```

🔴 **`0600` agora é criado e conferido** (0.11.9, achado F-06). Até a 0.11.8 esta doc e o
README prometiam `0600` e "keychain se der", e **nenhum dos dois existia**: não havia
comando que escrevesse o config, então o usuário o criava à mão com o umask dele —
tipicamente `0644`, legível por qualquer conta da máquina — e a leitura não dizia nada.
Promessa de permissão que ninguém cria nem confere é pior que silêncio, porque quem leu a
doc acredita que está protegido.

- `anna config set-key [CHAVE]` grava com `0600`, preservando o resto do arquivo (escreve
  num temporário no mesmo diretório, põe a permissão **antes** e renomeia — `rename` é
  atômico, e criar em `0644` para ajustar depois abriria uma janela com a chave legível).
  **Sem argumento, lê do stdin** — que é o modo que não deixa a chave no histórico do
  shell nem na lista de processos.
- Ao carregar, se o arquivo **tem `api_key`** e a permissão é frouxa, sai um aviso com o
  modo e o comando para consertar. Só com chave dentro: avisar num config sem chave
  treinaria o dev a ignorar o aviso, que é como uma guarda morre.
- **Keychain continua não existindo.** A menção saiu da doc em vez de continuar prometida.
**Sessões** em `~/.shvia-code/sessions/<id>.json` (0600, 0700 na pasta):
histórico do turno + metadados; `anna sessions {list|resume <id>|clear}`. Nunca
sobem ao servidor (D7). Transcript não guarda a API key.

### Retenção (0.11.9, achado F-07)

O que fica ali é o **histórico inteiro**, inclusive `role=tool` — ou seja, tudo que o
`read_file` leu, e a última imagem em base64 da sessão. Em `0600`, mas **sem prazo**: a
única limpeza era `anna sessions clear`, tudo ou nada. Quem quer manter o trabalho da
semana e não quer o `.env` lido há três meses não tinha o que fazer.

```bash
anna sessions clear --older-than 30   # só as sem uso há mais de 30 dias
anna sessions clear                   # todas, como antes
```

O corte usa o **`mtime` do arquivo**, e não um campo de data: `Session` não tem carimbo, e
acrescentar um só valeria para as sessões novas — justamente as que não interessam a uma
limpeza por idade. Arquivo sem `mtime` legível é **preservado**; numa operação que apaga, a
dúvida vota em não apagar. Pelo mesmo motivo, um `--older-than` absurdo devolve zero em vez
de estourar a aritmética (encontrado por teste, não por leitura).

### Memória de longo prazo exige URL segura (0.11.9, achado F-07)

O hook do `ai-memory` manda `bearer_auth` + prompts + 400 chars de cada resultado de tool.
`AI_MEMORY_URL` aceitava **qualquer** esquema: em `http://` para fora do loopback, isso é
token e conteúdo de trabalho em claro na rede.

Agora vale a **mesma** regra do `SHVIA_URL` — `config::url_seguro`, importada e não
recopiada (ela já tem prova, incluindo `http://localhost@atacante.tld` e
`http://127.0.0.1.atacante.tld`; uma segunda cópia divergiria). URL insegura **desliga a
memória com aviso**, em vez de recusar o boot: memória é opcional, e derrubar a ferramenta
por causa de um hook mal configurado trocaria um vazamento por uma indisponibilidade.

## 6. Modos de execução

- **Interativo** (default): `anna` na raiz do projeto → REPL com cabeçalho
  (projeto/git/modelo·infra), streaming, `/model`, `/sessions`, `/help`.
- **Não-interativo**: `anna -p "corrige o lint e roda os testes"` → roda o loop e
  sai (código de saída = sucesso/falha). Base pra CI (C5). Nele, gates de escrita/
  bash **negam por padrão** salvo `--allow-writes`/allowlist explícita.

## 7. Roadmap de implementação (dentro das fases do escopo)

- **C1**: `main/cli/config/api(sse,types)/agent/tools(fs,guard)/gate(read-only)/
  project/ui(repl)/render/telemetry`. Tools só de leitura. Já conversa e explica.
- **C2**: `tools/edit,shell,git` + gates de escrita/confirm/destrutivo + `similar`.
- **C3**: `session` resume, `/model`, `ui/tui` opcional, ANNA.md.
- **C4**: `build-local.sh` (feito) + binários + checksums + dogfood.
- **C5 (future)** — items as they are approved:
  - **Listing infras and models** (proposed 29/09/2026). Nothing in `anna` shows what can be
    used today: `/model` with no argument prints only the current profile, and `/model x`
    accepts any string unchecked, so a stale default or a typo comes back from the gateway as
    a routing error. Seen on 29/09/2026: `deepseek/deepseek-v3.2` was the pinned model, it
    exists only on `kilo`, and the CLI could not say so.
    - **Infras.** List the infras the user can actually use — enabled (`disabled: false`) and
      with an API configured (`api_key_configured`) — with their status (`online`), data
      locality (`data_locality`) and model count. Suggested surface: `anna infras` and
      `/infras`. Disabled or keyless infras stay out of the default view; `--all` shows them
      with the reason.
    - **Models.** List per infra, the user's favorites first (`is_favorite`, the list stored on
      the account through `PUT /api/v1/me/favorite-models`), and `--all` for the full
      catalogue — `kilo` and `openrouter` have 390+ models each. Each line is the exact
      `model@infra` string that `--model` and `/model` take. Suggested surface:
      `anna models [infra]` and `/models [infra]`.
    - **Data.** No server work: `GET /api/v1/servers` and `GET /api/v1/profiles` already answer
      to an API key (`auth:sanctum,apikey`, no ability gate) and carry every field above. Only
      the client is missing. `/profiles` is about 640 KB for roughly 1000 profiles, so fetch it
      once per command and filter client-side; both routes share the 60/min bucket.

## 8. Testes

- Unit: `guard` (denylist/confinamento), `shell` (classificação destrutivo/
  allowlist), `sse` (parse de frames parciais), `edit` (unicidade do old_string).
- Integração: um **mock do gateway** (servidor SSE de teste) exercitando o loop
  fechado — sem depender do servidor real. **Ainda não existe** (02/09/2026): o
  loop fechado só é exercitado à mão, contra o gateway real. Até aqui a frase
  dizia que o `spikes/c0-gateway.php` era "a referência viva do protocolo até
  lá" — era um cliente PHP de 08/07 que nada rodava, e foi removido (F-10). A
  referência do protocolo é `src/api.rs`, que o CI compila e testa.
