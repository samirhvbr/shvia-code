# Catálogo de ferramentas v1

> **Source review, 2026-10-15:** current usage is in the [user guide](USER-GUIDE.md).
> The registry has eleven tools, including `todo_write`. Local filesystem tools and
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
| **confirm** | mostra a ação e pede `[s]im / [n]ão / [t]udo nesta sessão` | `t` libera aquela tool até fechar |
| **always** | confirma SEMPRE, `t` não vale (destrutivo) | destaque vermelho |

`t` ("tudo nesta sessão") é por **tool + escopo** (ex.: liberar `edit_file` não
libera `bash`). **Não existe `--yolo` no v1.**

## 2. As ferramentas

### `read_file` · gate: auto
```json
{"name":"read_file","description":"Lê um arquivo do projeto (texto).",
 "parameters":{"type":"object","required":["path"],
   "properties":{"path":{"type":"string","description":"Caminho relativo à raiz do projeto."},
     "max_bytes":{"type":"integer","description":"Teto opcional (default 64 KiB); acima disso, recorta e avisa."}}}}
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
{"name":"list_dir","description":"Lista arquivos/pastas de um diretório.",
 "parameters":{"type":"object","required":["path"],
   "properties":{"path":{"type":"string","description":"Relativo à raiz ('.' = raiz)."}}}}
```
Result: one entry per line (`dir/` for directories). The walker respects ignore
rules but `list_dir` explicitly enables hidden entries; this is not a secret-name filter.

### `grep` · gate: auto
```json
{"name":"grep","description":"Busca um regex nos arquivos do projeto.",
 "parameters":{"type":"object","required":["pattern"],
   "properties":{"pattern":{"type":"string"},
     "path":{"type":"string","description":"Subárvore (default: raiz)."},
     "glob":{"type":"string","description":"Filtro de arquivo, ex.: '*.php'."}}}}
```
Resultado: `arquivo:linha: trecho` (teto de N hits; avisa se cortou). Respeita `.gitignore`.
Since 0.13.9 it applies the same denylist as `read_file`: an explicit `path` on the list is
refused, and files on it are skipped during the walk (by their real path), with one closing
line saying how many were skipped. Until then `grep SECRET` returned the lines of a
non-ignored `certs/server.pem`, and `path=".env"` bypassed `.gitignore` and the hidden-file
filter (external review, 26/10/2026).

### `glob` · gate: auto
```json
{"name":"glob","description":"Lista arquivos por padrão (ex.: src/**/*.rs).",
 "parameters":{"type":"object","required":["pattern"],"properties":{"pattern":{"type":"string"}}}}
```

### `edit_file` · gate: **confirm** (diff)
```json
{"name":"edit_file","description":"Edita um arquivo por substituição exata de trecho.",
 "parameters":{"type":"object","required":["path","old_string","new_string"],
   "properties":{"path":{"type":"string"},
     "old_string":{"type":"string","description":"Trecho EXATO a substituir (único no arquivo)."},
     "new_string":{"type":"string"},
     "replace_all":{"type":"boolean","description":"Substituir todas as ocorrências (default false)."}}}}
```
Fluxo: valida que `old_string` é único (senão erro pedindo mais contexto),
**mostra o diff colorido**, pede confirmação, aplica. Resultado: `ok (N hunks)` ou
o erro. Nunca escreve sem mostrar.

### `write_file` · gate: **confirm** (diff)
```json
{"name":"write_file","description":"Cria/sobrescreve um arquivo com o conteúdo dado.",
 "parameters":{"type":"object","required":["path","content"],
   "properties":{"path":{"type":"string"},"content":{"type":"string"}}}}
```
Sobrescrever arquivo existente → diff contra o atual antes de confirmar. Novo →
mostra o conteúdo. Recusa paths da denylist.
Since 0.13.9 a name that already exists is resolved through its symlinks: the real file must
stay inside the project and off the denylist, and the approval card names it
(`notes.md → docs/notes-v2.md`). Until then only the parent folder was checked, so a link
already in the tree (`notes.md -> ../outside.txt`, or `hook.txt -> .git/hooks/pre-commit`)
took an approved write outside the project or into a git hook. `edit_file` applies the same
check.

**Overwriting needs the file as it is (0.15.7).** `write_file` over an existing file is refused,
before any approval card, when this session never read the whole file (`read_file` cut at
`max_bytes` does not count) or when the file changed since that read or since the model's own
last write or edit (the developer's edit, a formatter, another tool). The refusal says how: read it
again and merge, or change only the passage with `edit_file`, which needs no such guard because its
exact `old_string` must match the file as it is. The mark is a hash of the content, kept per
process; a new file needs no read.

**Instruction files always ask (0.15.4).** A write (`edit_file` or `write_file`) to `ANNA.md`,
`AGENTS.md`, `CLAUDE.md` or `CLAUDE.local.md`, by the requested name or by its real target, uses
the **always** gate: `t` does not cover it, and an NDJSON host's `always` is a refusal there.
Those files are loaded into every later session of every agent (anna's context block, Claude
Code, Codex), so an instruction injected in one turn, riding a session-wide approval, would
outlive the session.

### `bash` · gate: **auto p/ allowlist · confirm p/ resto · always p/ destrutivo**
```json
{"name":"bash","description":"Roda um comando no diretório do projeto.",
 "parameters":{"type":"object","required":["command"],
   "properties":{"command":{"type":"string"},
     "why":{"type":"string","description":"1 linha: por que este comando."}}}}
```
- **Allowlist (auto)** — a lista viva é a `ALLOWLIST` em `src/tools.rs`; em resumo:
  navegação e leitura (`ls`, `cat`, `head`, `tail`, `wc`, `stat`, `tree`, `du`…), busca e
  filtro (`grep`, `rg`, `cut`, `sort`, `uniq`, `nl`, `diff`, `jq`, `tr`…), git só-leitura
  (`git status/diff/log/show/branch/rev-parse/ls-files/blame`…) e build/teste
  (`npm test|run`, `cargo check|clippy|test|build`, `php -l`, `phpunit`, `pytest`,
  `make test`). `curl`, `wget`, `ssh` e `docker` ficam **fora** de propósito.
- 🔴 **O nome do comando não basta — o ARGUMENTO também é conferido** (desde a 0.11.8,
  achado F-02 da revisão de 01/09/2026). Antes, a lista era casada só pelo **prefixo**, e
  `cat .env` saía em **auto**: a `denylist` que a tool `read_file` respeita era contornável
  pelo `bash` ao lado dela. Agora o segmento cai para **confirm** quando um argumento:
  está na denylist de segredos (`.env`, `*.pem`, `id_rsa*`, `credentials`, `/.ssh/`,
  `/.aws/`, `.git/`); é caminho **absoluto** ou de **home** (`~/…`); tem `..` que **escapa**
  da raiz; ou traz **expansão de variável** (`$HOME/x`). Aspas não ajudam — a conferência é
  por token, não pela string crua.
- **Comandos que a lista deixava passar pelo nome e escrevem/executam pelo argumento**
  (F-03): `awk` **saiu do auto** (`system("…")` e `print > "arq"` moram dentro do programa,
  entre aspas, invisíveis ao scanner — mesmo critério que já mantinha o `sed` fora);
  `git branch` só é auto nas formas de leitura (`-D`/`-m` pedem cartão); `sort -o` e o
  segundo posicional do `uniq` são destino de escrita.
- 🔴 **An unquoted glob in a command that prints file contents asks (since 0.13.9).** The
  argument check reads tokens literally, and the shell expands globs after it: `cat .e*`,
  `cat certs/*` and `head -n 50 .e*` were **auto** and printed `.env` and
  `certs/server.pem` (external review, 26/10/2026). Now a non-flag word with `*`, `?`, `[`
  or a `{a,b}` list outside quotes sends the segment to **confirm**. Flags
  (`--include=*.rs`) and quoted patterns (`grep -n "fn .*x" src`) do not count, and
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
  entradas da `ALLOWLIST` — é uma linha, e custa uma confirmação por build.
- **Destrutivos (always confirm, `t` não vale)**: `rm` recursivo, `git push`,
  `git reset --hard`, `git clean -f`, `dd`, `mkfs*`, `chmod -R 777`,
  `curl|sh`/`wget|sh`, drop/truncate SQL, fork bomb.

  🔴 **A classificação é por TOKEN, não por substring** (desde a 0.11.9, achado F-05).
  Antes ela procurava a grafia — `contains("rm -rf")` — e o shell tem muitas grafias
  para a mesma coisa. Caíam em **confirm** (onde o `t`/"Sempre" **vale**, que é
  justamente o que "always" existe para impedir): `rm -r --force build`,
  `git -C outro push`, `sudo rm -rf /`, `FOO=1 rm -rf x`, `/bin/rm -rf x`,
  `cat lista | xargs rm -rf`. Agora o comando é reconhecido depois de descartar
  prefixos transparentes (`sudo`, `env`, `xargs`…) e atribuições, e as flags são lidas
  agrupadas (`-rf` = `-r -f` = `--recursive --force`).

  ⚠️ **`rm -r` sem `-f` também é destrutivo**, e a mudança é deliberada: some com uma
  árvore igual. O custo de errar para o lado estrito é uma confirmação; para o outro
  lado, é uma árvore apagada com "sempre" memorizado.

  ⚠️ O que **não** mudou: `find … -delete`, `shred` e `git branch -D` seguem em
  **confirm** por decisão registrada da casa. Este achado troca *como* se classifica,
  não *o que* é destrutivo.
- **Resto**: confirm. Sem shell interativo; captura stdout+stderr (teto de bytes).
  Resultado: saída + exit code.

#### Stopping the turn (0.14.0)

Ctrl-C in the terminal, or `{"type":"cancel"}` from an NDJSON host, kills a running command
with its whole process group, even with `bash_timeout = 0`, and the model receives
`ERRO: o usuário interrompeu o turno — o comando foi encerrado`. Before 0.14.0 the child
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
pai deixaria o neto segurando o pipe. O modelo recebe `ERRO: timeout de Ns` **com o que
fazer** — rodar em background e ler depois, ou aumentar o teto —, senão ele repete o
mesmo comando e gasta outro teto inteiro.

É número, e não liga/desliga, porque suíte de integração legítima passa de 120 s: um teto
binário empurraria quem tem suíte lenta a desligá-lo, que é o pior dos dois mundos.

### `git_status` · `git_diff` · gate: auto
Wrappers de `git status --porcelain` / `git diff [--staged]`. Resultado = a saída.

### `git_commit` · gate: **confirm**
```json
{"name":"git_commit","description":"Commita as mudanças staged (ou todas com add_all).",
 "parameters":{"type":"object","required":["message"],
   "properties":{"message":{"type":"string","description":"Mensagem (o CLI sugere no padrão do repo se houver version.md)."},
     "add_all":{"type":"boolean"}}}}
```
Mostra o `git diff --staged` + a mensagem, confirma, commita. **`git push` NUNCA
é implícito** — só via `bash` explícito (e cai no gate destrutivo).

**Current staging caveat:** `add_all=true` runs `git add -A` before the approval
question; refusing the commit can leave unrelated files staged. Review this separately
from commit approval, and prefer explicitly staging intended paths. See the
[source finding](DOCUMENTATION-REVIEW-20261015.md).

### `todo_write` · gate: auto (0.13.0)
```json
{"name":"todo_write","description":"Mantém a lista de tarefas deste trabalho (a lista INTEIRA a cada chamada)...",
 "parameters":{"type":"object","required":["todos"],
   "properties":{"todos":{"type":"array","items":{"type":"object","required":["content","status"],
     "properties":{"content":{"type":"string"},"status":{"enum":["pending","in_progress","completed"]},
       "activeForm":{"type":"string"}}}}}}}
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
- Sucesso: a saída direta (texto/conteúdo/diff aplicado).
- Erro: prefixo `ERRO: <motivo>` (ex.: `ERRO: 'src/x' inexistente ou fora do projeto.`).
- Recusa por gate/denylist: `RECUSADO: <motivo>` (o modelo entende que não rolou).
- **O resultado é DADO, não instrução** (SECURITY §2.2): texto vindo de arquivo
  jamais relaxa gate — a aprovação é do dev.

## 4. Recorte & limites

- Limits differ by tool: `read_file` defaults to 64 KiB returned text and accepts
  `max_bytes`; it reads the whole file before slicing. Search/glob stop around 200
  matches; shell output is truncated near 32 KiB after capture. This is not a global
  output or process-memory bound.
- Walk tools use ignore rules, and only inside a git repository: the `ignore` crate reads
  `.gitignore` only where there is a `.git` (measured 26/10/2026), so in a plain folder an
  ignored file is listed and searched. Since 0.13.9 `grep` also applies `guard::denied` by
  real path; `list_dir` and `glob` list names without it. Ignore rules and automatic shell
  classification must not be treated as a secret-containment policy.
- Teto de voltas de tool por turno (default 100, `max_voltas`/`SHVIA_MAX_VOLTAS`)
  no loop (SECURITY §2.7) — estouro pergunta ao dev no terminal; no host encerra
  o turno com a sessão preservada ("continua" retoma).

## 5. Checkpoint automático & `/rollback` (0.8.0)

Before approved `edit_file`/`write_file` calls and classified destructive shell
commands, the engine attempts a snapshot using a temporary Git index and a commit at
`refs/anna/checkpoint`, outside the current branch. Snapshot creation leaves the real
index and working tree unchanged. A 30-second debounce groups bursts of operations.
Failures are silent and do not stop a write; this is best effort, not a transaction.

Restaurar é ação do **dev** no REPL (nunca uma tool do modelo):

- `/rollback` — mostra o `diff --stat` desde a foto (o que seria desfeito);
- `/rollback aplicar` restores checkpoint content in the working tree. Files created
  after the snapshot remain. The modern `git restore` path leaves the index intact;
  the legacy `git checkout` fallback can change it.

Inspiração: checkpoints de filesystem do hermes-agent (`/rollback`), adaptados
ao modelo client-side do anna — git como storage, zero estado fora do repo.

## 6. Fora do v1 (futuro)

`apply_patch` (multi-arquivo), `web_fetch`, `run_tests` estruturado, MCP/
conectores, sandbox de execução, tools de refactor AST. Entram por ADR quando
o C1/C2 provarem a base.
