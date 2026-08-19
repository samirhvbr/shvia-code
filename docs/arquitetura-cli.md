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

```
src/
  main.rs        Entrypoint: parse args (clap), carrega config, escolhe modo:
                 REPL interativo  ou  -p/--print "…" (não-interativo, p/ CI).
  cli.rs         Definição dos args/subcomandos (clap): [prompt], -p, --model,
                 --url, sessions {list|resume|clear}, config, version.
  config.rs      ~/.config/shvia-code/config.toml + overrides por env
                 (SHVIA_URL, SHVIA_API_KEY, SHVIA_PROFILE). Chave nunca em arg.
  api/
    mod.rs       Cliente do gateway: POST /api/v1/code/chat.
    sse.rs       Parser do SSE (bytes → Event) — enum tipado dos `kind`.
    types.rs     Structs serde: Message, ToolDef, ToolCall, Usage, Event.
  agent.rs       O LOOP agêntico (§4): monta payload, consome eventos, dispara
                 tools via registry, reenvia. Teto de voltas. Agnóstico de UI
                 (recebe um &dyn Ui pra render + &Gate pra aprovar).
  tools/
    mod.rs       Registry: nome → (schema JSON, gate, executor). defs() → tools[].
    fs.rs        read_file, list_dir, grep, glob (respeita .gitignore, denylist).
    edit.rs      edit_file (search/replace único), write_file (diff + confirm).
    shell.rs     bash: classifica allowlist/confirm/destrutivo; executa c/ timeout.
    git.rs       git_status, git_diff, git_commit.
    guard.rs     denylist de leitura + confinamento à raiz do projeto (realpath).
  gate.rs        Política de aprovação: Gate::ask(action) → Allow|Deny; memória
                 do "tudo nesta sessão" por (tool, escopo). Sem yolo no v1.
  session.rs     ~/.shvia-code/sessions/<id>.json (0600): salvar/retomar/limpar.
  project.rs     Detecta raiz (git), lê contexto (ANNA.md; CLAUDE.md/AGENTS.md se
                 houver), monta o system prompt e o cabeçalho da sessão.
  ui/
    mod.rs       trait Ui: stream de texto, markdown, diff colorido, prompts de gate.
    repl.rs      C1: REPL linha-a-linha (rustyline) — simples e sólido.
    tui.rs       C3: ratatui/crossterm (painel, histórico rolável) — opcional.
  render.rs      Markdown → terminal (negrito/código/listas) + diff unificado colorido.
  telemetry.rs   Custo/tokens por turno (do evento usage; US$ vs ≈US$, padrão 2.15.9).
  error.rs       anyhow/thiserror; nunca vaza API key em mensagem.
```

C1 pode nascer com `ui/tui.rs` vazio (só `repl.rs`); tools de escrita/bash entram
no C2. A estrutura já acomoda os dois sem refactor.

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
url        = "https://ai.shvia.org"
profile    = "qwen-coder@shvia1"   # opcional; default do usuário se ausente
max_voltas = 100                   # teto de voltas por turno (env SHVIA_MAX_VOLTAS ganha)
# api_key NÃO fica aqui em texto se der pra usar keychain; fallback: campo 0600.
# Precedência: env SHVIA_API_KEY > config. Nunca em argumento de linha.
```
**Sessões** em `~/.shvia-code/sessions/<id>.json` (0600, 0700 na pasta):
histórico do turno + metadados; `anna sessions {list|resume <id>|clear}`. Nunca
sobem ao servidor (D7). Transcript não guarda a API key.

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

## 8. Testes

- Unit: `guard` (denylist/confinamento), `shell` (classificação destrutivo/
  allowlist), `sse` (parse de frames parciais), `edit` (unicidade do old_string).
- Integração: um **mock do gateway** (servidor SSE de teste) exercitando o loop
  fechado — sem depender do servidor real. O `spikes/c0-gateway.php` é a
  referência viva do protocolo até lá.
