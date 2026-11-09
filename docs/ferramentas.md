# Catálogo de ferramentas v1

> **Source review, 2026-10-15; rechecked against 0.15.8 on 2026-11-05.** Current usage is
> in the [user guide](USER-GUIDE.md). The registry has eleven tools, including `todo_write`;
> `--json --tools host` offers only `bash`. The schemas below are those of `tools::defs()`
> in [`src/tools.rs`](../src/tools.rs), which is the authority. Local filesystem tools and
> shell approval have different security boundaries; read the actual controls and gaps
> in [SECURITY_GUIDELINES.md](../SECURITY_GUIDELINES.md).

> As ferramentas que o `anna` **envia** ao modelo (`tools[]` do protocolo) e
> **executa localmente** no loop. Poucas e afiadas (escopo §4). Cada uma tem:
> schema JSON (o que o modelo vê), **gate** (política de aprovação) e formato do
> resultado (o `content` da mensagem `role:"tool"`). Segurança: ver
> `SECURITY_GUIDELINES.md` §2 — este doc é a especificação; lá é a lei.

---

## 1. Política de gates (padrão SShvTerm)

| Gate | Significado | UI |
|------|-------------|----|
| **auto** | runs without asking; shell builds/tests can have side effects | mostra o que fez |
| **confirm** | shows the action and asks | arrow-key menu *Sim / Sim, e não perguntar de novo nesta sessão / Não*, starting on *Sim*; without a terminal, the line `aplicar? [s]im / [n]ão / [t]udo nesta sessão` |
| **always** | asks every time, `t` does not apply (destructive commands, instruction files) | two-option menu *Sim, executar / Não*, starting on *Não*; without a terminal, `AÇÃO DESTRUTIVA — confirmar? [s]im / [n]ão` |

Menu keys: arrows (or `j`/`k`, Tab) and Enter, the item number, or its letter (`s`/`y`, `t`,
`n`); Esc, `q`, Ctrl-C and Ctrl-D mean *Não*.

`t` ("tudo nesta sessão") is per **tool name**: granting it on one `edit_file` card covers
every later `edit_file`, and on one `bash` card it covers every later confirm-class shell
command, until `anna` exits. `/clear` does not reset it. Granting `edit_file` does not grant
`bash`. An NDJSON host's `always` works the same way, and later confirm-class calls of that
tool then send no `gate_request` and no preview. **Não existe `--yolo` no v1.**

## 2. As ferramentas

### `read_file` · gate: auto
```json
{"name":"read_file","description":"Lê um arquivo de texto do projeto.",
 "parameters":{"type":"object","required":["path"],
   "properties":{"path":{"type":"string","description":"Caminho relativo à raiz do projeto."},
     "max_bytes":{"type":"integer","description":"Teto opcional (default 65536)."}}}}
```
Result: returned file text, sliced at the requested limit with a truncation notice.
The requested-name denylist refuses `.env`/`.env.*` except `.env.example`, private-key
extensions/names, credential paths and `.git`; there is no interactive file-tool override.
Canonical paths must stay in the project. The requested name and resolved target are
separate checks; search and alias limitations are in the security guide.
Since 0.13.9 the denylist is also checked against the file's **real path, after every
symlink**: `readme.txt -> .env` used to be read with no approval (external review,
26/10/2026).

### `list_dir` · gate: auto
```json
{"name":"list_dir","description":"Lista arquivos e pastas de um diretório (respeita .gitignore).",
 "parameters":{"type":"object","required":["path"],
   "properties":{"path":{"type":"string","description":"Relativo à raiz ('.' = raiz)."}}}}
```
Result: one entry per line (`dir/` for directories). The walker respects ignore
rules but `list_dir` explicitly enables hidden entries; this is not a secret-name filter.

### `grep` · gate: auto
```json
{"name":"grep","description":"Busca um regex nos arquivos do projeto.",
 "parameters":{"type":"object","required":["pattern"],
   "properties":{"pattern":{"type":"string","description":"Regex (sintaxe Rust)."},
     "path":{"type":"string","description":"Subárvore (default: raiz)."},
     "glob":{"type":"string","description":"Filtro de arquivo, ex.: '*.rs'."}}}}
```
Resultado: `arquivo:linha: trecho` (each line trimmed; at most 200 hits, with a closing line
when cut). Respeita `.gitignore` and skips hidden paths; `glob` matches the file name only,
and files that are not valid UTF-8 are skipped silently.
Since 0.13.9 it applies the same denylist as `read_file`: an explicit `path` on the list is
refused, and files on it are skipped during the walk (by their real path), with one closing
line saying how many were skipped. Until then `grep SECRET` returned the lines of a
non-ignored `certs/server.pem`, and `path=".env"` bypassed `.gitignore` and the hidden-file
filter (external review, 26/10/2026).

### `glob` · gate: auto
```json
{"name":"glob","description":"Lista arquivos por padrão glob (ex.: src/**/*.rs).",
 "parameters":{"type":"object","required":["pattern"],"properties":{"pattern":{"type":"string"}}}}
```

### `edit_file` · gate: **confirm** (diff)
```json
{"name":"edit_file","description":"Edita um arquivo por substituição exata de trecho (mostra diff, pede confirmação).",
 "parameters":{"type":"object","required":["path","old_string","new_string"],
   "properties":{"path":{"type":"string"},
     "old_string":{"type":"string","description":"Trecho EXATO a substituir (único, salvo replace_all)."},
     "new_string":{"type":"string"},
     "replace_all":{"type":"boolean","description":"Substituir todas as ocorrências (default false)."}}}}
```
Fluxo: valida que `old_string` é único (senão erro pedindo mais contexto),
**mostra o diff colorido**, pede confirmação, aplica. Resultado:
`ok — <path> editado (N ocorrência(s)).` ou o erro (an empty `old_string`, a missing file or
passage, or a file that is not text are refused). `edit_file` does not create files. Nunca
escreve sem mostrar.

### `write_file` · gate: **confirm** (diff)
```json
{"name":"write_file","description":"Cria ou sobrescreve um arquivo (mostra diff, pede confirmação).",
 "parameters":{"type":"object","required":["path","content"],
   "properties":{"path":{"type":"string"},"content":{"type":"string"}}}}
```
Sobrescrever arquivo existente → diff contra o atual antes de confirmar. Novo →
mostra o conteúdo. Recusa paths da denylist. It does not create missing folders (`ERRO: …
a pasta-pai não existe`). Content identical to the file returns `ok — '<path>' já tem esse
conteúdo (sem mudanças).` without a card; otherwise `ok — '<path>' criado|sobrescrito (N bytes).`
Since 0.13.9 a name that already exists is resolved through its symlinks: the real file must
stay inside the project and off the denylist, and the approval card names it
(`notes.md → docs/notes-v2.md`). Until then only the parent folder was checked, so a link
already in the tree (`notes.md -> ../outside.txt`, or `hook.txt -> .git/hooks/pre-commit`)
took an approved write outside the project or into a git hook. `edit_file` applies the same
check.

**Overwriting needs the file as it is (0.15.7).** `write_file` over an existing file is refused,
before any approval card, when the model has not read the whole file since `anna` started
(`read_file` cut at `max_bytes` does not count) or when the file changed since that read or since the model's own
last write or edit (the developer's edit, a formatter, another tool). The refusal says how: read it
again and merge, or change only the passage with `edit_file`, which needs no such guard because its
exact `old_string` must match the file as it is. The mark is a hash of the content, kept per
process (it survives `/clear` and is shared with the `/plano` phases, and a restart loses
it); a new file needs no read.

**Instruction files always ask (0.15.4).** A write (`edit_file` or `write_file`) to `ANNA.md`,
`AGENTS.md`, `CLAUDE.md` or `CLAUDE.local.md`, by the requested name or by its real target, uses
the **always** gate: `t` does not cover it, and an NDJSON host's `always` is a refusal there.
Those files are loaded into every later session of every agent (anna's context block, Claude
Code, Codex), so an instruction injected in one turn, riding a session-wide approval, would
outlive the session. **The rule covers the two file tools only:** a shell command that writes
one of these files is classified like any other command (confirm, which `t` covers, or even
auto for an allowlisted command with an output flag; see `bash` below). The terminal reuses
the destructive menu for these cards, so it reads *Sim, executar*.

### `bash` · gate: **auto p/ allowlist · confirm p/ resto · always p/ destrutivo**
```json
{"name":"bash","description":"Roda um comando no diretório do projeto (allowlist roda direto; destrutivo confirma sempre).",
 "parameters":{"type":"object","required":["command"],
   "properties":{"command":{"type":"string"},
     "why":{"type":"string","description":"1 linha: por que este comando."}}}}
```
- **Allowlist (auto)** — a lista viva é a `ALLOWLIST` e suas guardas em `src/tools.rs`; em resumo:
  navegação e leitura (`ls`, `cat`, `pwd`, `echo`, `cd`, `head`, `tail`, `wc`, `file`,
  `stat`, `tree`, `du`, `df`, `realpath`, `readlink`…), busca e filtro (`grep`, `egrep`,
  `fgrep`, `rg`, `ag`, `cut`, `sort`, `uniq`, `nl`, `diff`, `jq`, `column`, `tr`, `which`,
  `type`, `command -v`), git (`git status/diff/log/show/branch/rev-parse/ls-files/blame/
  describe`, `git remote -v`, `git stash list`, `git worktree list`, `git config --get`) e
  build/teste (`composer test`, `npm test|run`, `cargo check|clippy|test|build`, `php -l`,
  `phpunit`, `pytest`, `make test`). `find`, `git branch`, `sort` and `uniq` are auto only in
  their read forms. `curl`, `wget`, `ssh` e `docker` ficam **fora** de propósito.
- **Only `&&` composes automatic segments.** Unquoted newlines, shell comments,
  pipes, `;`, redirections, a lone `&`, `$(…)` and backticks require confirmation.
  A newline inside a quoted argument stays literal.
- **Options that select executables or write output require confirmation.** Examples
  include `rg --pre` and `--hostname-bin`, `git diff/log --output`, `tree -o`, and
  grouped or attached `sort -o` options. Automatic Git reads switch off external diff
  drivers, text conversion, explicit signature display and filesystem monitoring where
  applicable. Git can still run a configured clean filter when it reads the working tree.
  Build and test entry points trust project scripts and configuration. Direct overrides
  such as `npm test --script-shell=…`, `cargo test --config …` or `make test --eval=…`
  require confirmation.
- 🔴 **O nome do comando não basta — o ARGUMENTO também é conferido** (desde a 0.11.8,
  achado F-02 da revisão de 01/09/2026). Antes, a lista era casada só pelo **prefixo**, e
  `cat .env` saía em **auto**: a `denylist` que a tool `read_file` respeita era contornável
  pelo `bash` ao lado dela. Agora o segmento cai para **confirm** quando um argumento:
  está na denylist de segredos (`.env`, `*.pem`, `id_rsa*`, `credentials`, `/.ssh/`,
  `/.aws/`, `.git/`); é caminho **absoluto** ou de **home** (`~/…`); tem `..` que **escapa**
  da raiz; ou traz **expansão de variável** (`$HOME/x`). Aspas não ajudam — a conferência é
  por token, não pela string crua. **Its limits, measured on 2026-11-05:** the check is
  lexical — it does not resolve symlinks; it skips every token that starts with `-`, so a
  path glued to an option (`--from-file=…`) is not checked; `.ssh/` and `.aws/` are matched only after a `/`,
  and private keys by name only as `id_rsa*`. Automatic `cd` requires one explicit
  relative path. The path check still allows a symlink to point outside the project.
- **Shell escapes require confirmation (0.15.11).** A backslash outside single quotes sends
  the whole command to **confirm**, including inside double quotes and in `&&` chains.
  ANSI-C quoted arguments (`$'...'`) also ask: their backslashes are decoded by the shell.
  The token checks do not interpret shell escapes, so `cat .e\nv` previously passed as
  a harmless name before `sh` removed the backslash and opened `.env`. Rather than approve
  an argument the shell will change, the scanner asks. This also asks for harmless escaped
  spaces; single-quoted literal patterns such as `grep '\d+\s' src/main.rs` remain auto.
- **Comandos que a lista deixava passar pelo nome e escrevem/executam pelo argumento**
  (F-03): `awk` **saiu do auto** (`system("…")` e `print > "arq"` moram dentro do programa,
  entre aspas, invisíveis ao scanner — mesmo critério que já mantinha o `sed` fora);
  `git branch` só é auto nas formas de leitura. Para filtrar por nome, use
  `git branch --list PADRÃO`. `sort -o` e o segundo posicional do `uniq` escrevem
  no destino e pedem confirmação.
- 🔴 **An unquoted glob in a command that prints file contents asks (since 0.13.9).** The
  argument check reads tokens literally, and the shell expands globs after it: `cat .e*`,
  `cat certs/*` and `head -n 50 .e*` were **auto** and printed `.env` and
  `certs/server.pem` (external review, 26/10/2026). Now a non-flag word with `*`, `?`, `[`
  or a `{a,b}` list outside quotes sends the segment to **confirm**. A glob in an
  option name also asks. Attached values such as `--include=*.rs` and quoted
  patterns such as `grep -n "fn .*x" src` stay automatic, and
  commands that only print names or metadata (`ls`, `wc`, `file`, `stat`, `du`, `tree`,
  `git ls-files`…) stay auto. The price: `grep TODO src/*.rs` now asks; `grep -rn TODO src/`
  does not.
- **O que a guarda de argumento NÃO fecha, e está escrito para não ser confundido:** busca
  recursiva (`grep -r X .`) segue em auto e pode imprimir uma linha vinda de um `.env` do
  projeto. Decidir por conteúdo é outra guarda, com outro custo. The `grep` **tool** does not
  have this gap since 0.13.9 — it skips denied files — so it is the safe way to search.
- **`npm run <script>`, `cargo build/test` e `make test` seguem em auto por DECISÃO:** eles
  executam o que o projeto declara (`package.json`, `Cargo.toml`, `Makefile`), e um prompt
  injetado não muda esses arquivos sem passar por `write_file`/`edit_file`, que têm cartão.
  Em uma frase: o dev confia no `package.json` do próprio projeto. Para mudar, tire as
  entradas da `ALLOWLIST` — é uma linha, e custa uma confirmação por build. **The premise
  has a gap:** options after those prefixes are not checked, so an option that makes `make`
  or `npm` run something other than the project's declaration (for example `make test
  --eval=…`, `npm run x --prefix=<other folder>`) stays auto. `composer test`,
  `cargo check|clippy`, `php -l`, `phpunit` and `pytest` are auto too.
- **Destrutivos (always confirm, `t` não vale)**: `rm` recursivo, `git push`,
  `git reset --hard`, `git clean -f`, `dd`, `mkfs*`, `chmod -R 777`,
  a pipe into `sh`/`bash`, drop/truncate SQL, fork bomb. The last three are substring tests
  on the whole line (`| sh`, `|sh`, `| bash`, `|bash`, `drop table`, `truncate table`,
  `drop database`, `:(){`), so they are not specific to `curl`/`wget`: `ls | sha256sum` or a
  commit message containing `drop table` asks as destructive, while `| sudo sh`, `| zsh` or
  `sh <(curl …)` fall to **confirm**.

  🔴 **A classificação é por TOKEN, não por substring** (desde a 0.11.9, achado F-05).
  Antes ela procurava a grafia — `contains("rm -rf")` — e o shell tem muitas grafias
  para a mesma coisa. Caíam em **confirm** (onde o `t`/"Sempre" **vale**, que é
  justamente o que "always" existe para impedir): `rm -r --force build`,
  `git -C outro push`, `sudo rm -rf /`, `FOO=1 rm -rf x`, `/bin/rm -rf x`,
  `cat lista | xargs rm -rf`. Agora o comando é reconhecido depois de descartar
  prefixos transparentes (`sudo`, `doas`, `env`, `command`, `nohup`, `time`, `exec`, `xargs`)
  e atribuições, e as flags são lidas agrupadas (`-rf` = `-r -f` = `--recursive --force`).
  **Still classified confirm (measured on 2026-11-05), so `t` covers them:** a destructive
  command inside `sh -c`/`bash -c`, after `timeout` or `nice`, or after a prefix option the
  table wrongly treats as taking a value (`sudo -n`, `sudo -s`, `env -i`, `xargs -r`,
  `xargs -I{}`).

  ⚠️ **`rm -r` sem `-f` também é destrutivo**, e a mudança é deliberada: some com uma
  árvore igual. O custo de errar para o lado estrito é uma confirmação; para o outro
  lado, é uma árvore apagada com "sempre" memorizado.

  ⚠️ O que **não** mudou: `find … -delete`, `shred` e `git branch -D` seguem em
  **confirm** por decisão registrada da casa. Este achado troca *como* se classifica,
  não *o que* é destrutivo.
- **Resto**: confirm. Sem shell interativo. Resultado: stdout, then `[stderr]` and stderr
  when there is any, cut at 32 KiB together (a noisy stdout can push stderr out), then
  `[exit N]`. Approved destructive commands take a checkpoint first (§5); other approved
  shell writes do not.

#### Stopping the turn (0.14.0)

Ctrl-C in the terminal, or `{"type":"cancel"}` from an NDJSON host, kills a running command
with its whole process group, even with `bash_timeout = 0`, and the model receives the
captured output followed by `ERRO: o usuário interrompeu o turno — o comando foi encerrado
(grupo de processos morto).` Before 0.14.0 the child
kept running: it lives in its own process group, so the terminal's SIGINT never reached it,
and only `anna` died.

#### Teto de tempo (0.11.9, achado F-04)

Cada `bash` tem **120 s** por padrão. `bash_timeout` no `config.toml`, ou
`SHVIA_BASH_TIMEOUT`; **`0` desliga**.

A doc prometia timeout em dois lugares e o código chamava `output()`, que espera para
sempre. No REPL o dev vê e faz Ctrl-C; no **host NDJSON** (a casca do SHVIA-DESKTOP) a
sessão simplesmente emudece, e o único remédio é matar o sidecar — perdendo a sessão.
Era esse o custo, e não "o comando demora".

Ao estourar, o **grupo de processos inteiro** é morto, não só o `sh -c`: quem trava é
quase sempre o neto (o `node` do `npm run dev`, o binário do `cargo test`), e matar só o
pai deixaria o neto segurando o pipe. O modelo recebe a saída capturada seguida de
`ERRO: timeout de Ns` **com o que fazer** — rodar em background e ler depois, ou aumentar
o teto —, senão ele repete o mesmo comando e gasta outro teto inteiro.

É número, e não liga/desliga, porque suíte de integração legítima passa de 120 s: um teto
binário empurraria quem tem suíte lenta a desligá-lo, que é o pior dos dois mundos.

### `git_status` · `git_diff` · gate: auto
```json
{"name":"git_status","description":"git status (branch + arquivos alterados).",
 "parameters":{"type":"object","properties":{},"required":[]}}
{"name":"git_diff","description":"git diff das mudanças (staged com staged=true).",
 "parameters":{"type":"object","properties":{"staged":{"type":"boolean"}},"required":[]}}
```
Wrappers de `git status --short --branch` / `git diff [--staged]`. Resultado = stdout plus
stderr, cut at 32 KiB, `(sem saída)` when empty. Git's exit status is not reported, and
these calls have no timeout.

### `git_commit` · gate: **confirm**
```json
{"name":"git_commit","description":"Commita as mudanças (mostra o diff staged, pede confirmação).",
 "parameters":{"type":"object","required":["message"],
   "properties":{"message":{"type":"string","description":"Mensagem do commit."},
     "add_all":{"type":"boolean","description":"git add -A antes (default false)."}}}}
```
Mostra o `git diff --staged --stat` (file names and counts, not the patch) + a mensagem,
confirma, commita. The message is the model's; `anna` suggests none. The repository's Git
hooks run with no timeout. **`git push` NUNCA é implícito** — só via `bash` explícito (e cai
no gate destrutivo).

**Current staging caveat:** `add_all=true` runs `git add -A` before the approval
question; refusing the commit can leave unrelated files staged, including a non-ignored
file on the secrets denylist. Review this separately from commit approval, and prefer
explicitly staging intended paths. Still open on 2026-11-05; see the
[source finding](DOCUMENTATION-REVIEW-20261015.md#status-on-2026-11-05).

### `todo_write` · gate: auto (0.13.0)
```json
{"name":"todo_write","description":"Mantém a lista de tarefas deste trabalho (a lista INTEIRA a cada chamada). Use em trabalho com 3+ etapas; …",
 "parameters":{"type":"object","required":["todos"],
   "properties":{"todos":{"type":"array","description":"A lista completa, na ordem.","items":{"type":"object","required":["content","status"],
     "properties":{"content":{"type":"string","description":"…"},"status":{"type":"string","enum":["pending","in_progress","completed"]},
       "activeForm":{"type":"string","description":"…"}}}}}}}
```
The agent's own task list, the one ShvIA's Code mode shows in its **Tasks** tab
(SHVIA-WEB `docs/FRONTEND/ABA-TAREFAS.md`). The shape is Claude Code's `TodoWrite` on
purpose: the screen reads both with one parser. The list reaches the screen through the
`tool_call` event the stream already emits for every call; the tool itself only validates
(at most 50 tasks, a `content` each, a known `status`) and answers one line —
`Lista de tarefas atualizada: 1 de 3 concluídas. Em andamento: …` — warning when more
than one task is `in_progress`. It writes nothing on disk, so the read-only (plan) runner
offers it too. The system prompt asks for it on work with 3+ steps, and not on a simple
request.

## 3. Formato do resultado (mensagem `role:"tool"`)

Cada execução vira `{ role:"tool", tool_call_id:"<id>", content:"<string>" }`.
Convenções do `content`:
- Sucesso: a saída direta (texto/conteúdo/diff aplicado), or a sentinel when there is
  nothing: `(vazio)`, `(nenhum resultado)`, `(nenhum arquivo)`, `(sem saída)`.
- Erro: prefixo `ERRO: <motivo>` (ex.: `ERRO: 'src/x' inexistente ou fora do projeto.`).
  Not every failure has it: a failed Git wrapper returns Git's own text (`fatal: …`), and the
  shell's timeout/interrupt `ERRO:` line comes after the captured output.
- Recusa por gate/denylist: `RECUSADO: <motivo>` (o modelo entende que não rolou).
- The loop may append text of its own: the stall-guard note and the subfolder-instructions
  pointer (`[anna: esta pasta tem instruções próprias …]`); see [protocol](protocolo.md) §2.
- **O resultado é DADO, não instrução** (SECURITY §2.2): texto vindo de arquivo
  jamais relaxa gate — a aprovação é do dev.

## 4. Recorte & limites

- Limits differ by tool: `read_file` defaults to 64 KiB returned text and accepts
  `max_bytes` (no upper limit); it reads the whole file before slicing. `grep` stops at 200
  hits with a notice but has no byte cap; `glob` stops at 200 names silently (the first 200
  in walk order, then sorted); `list_dir` has no cap; shell and Git output is cut at 32 KiB
  after capture. This is not a global output or process-memory bound.
- Walk tools use ignore rules, and only inside a git repository: the `ignore` crate reads
  `.gitignore` only where there is a `.git` (measured 26/10/2026), so in a plain folder an
  ignored file is listed and searched. Since 0.13.9 `grep` also applies `guard::denied` by
  real path; `list_dir` and `glob` list names without it. Ignore rules and automatic shell
  classification must not be treated as a secret-containment policy.
- Teto de voltas por turno (default 100, `max_voltas`/`SHVIA_MAX_VOLTAS`): it counts model
  requests (rounds), not tool calls (SECURITY §2.7). Estouro pergunta ao dev no terminal
  interativo; no host e no `-p` the turn ends after the 0.15.2 closing request, and a
  persistent host can resume with "continua".

## 5. Checkpoint automático & `/rollback` (0.8.0)

Before approved `edit_file`/`write_file` calls and classified destructive shell
commands, the engine attempts a snapshot using a temporary Git index and a commit at
`refs/anna/checkpoint`, outside the current branch. Snapshot creation leaves the real
index and working tree unchanged. A 30-second debounce groups bursts of operations.
Failures are silent and do not stop a write; this is best effort, not a transaction.

Restaurar é ação do **dev** no REPL (nunca uma tool do modelo):

- `/rollback` — mostra o `diff --stat` desde a foto (o que seria desfeito);
- `/rollback aplicar` restores checkpoint content in the working tree. Untracked files
  created after the snapshot remain, but `git restore --source <snapshot> --worktree -- .`
  runs without overlay, so a file **staged** after the snapshot is deleted from disk
  (measured on 2026-11-05; the REPL's own note says new files remain). The modern
  `git restore` path leaves the index intact; the legacy `git checkout` fallback can change
  it. Only the latest snapshot is reachable: each one replaces `refs/anna/checkpoint`.

Inspiração: checkpoints de filesystem do hermes-agent (`/rollback`), adaptados
ao modelo client-side do anna — git como storage, zero estado fora do repo.

## 6. Fora do v1 (futuro)

`apply_patch` (multi-arquivo), `web_fetch`, `run_tests` estruturado, MCP/
conectores, sandbox de execução, tools de refactor AST. Entram por ADR quando
o C1/C2 provarem a base.
