# Changelog

Entradas no formato da mensagem de commit (`versão - comentário`), mais recente
primeiro. É daqui que a skill COMMITTER tira a mensagem (CLAUDE.md §PS).

## 0.11.21 - the NDJSON protocol gains stop_request for engines that can hold a turn open

Block B1 of `SHVIA-DESKTOP/docs/code/RUN-20260910.md` (ADR-034 there). `docs/embedding.md`
documents one optional event and its answer: `stop_request`, emitted by an engine that can
**hold a turn open** when the model wants to end it, blocking until the host answers
`continue` (with the message the model reads as its reason to go on) or `stop`. Ceiling of
60 s → `stop`; stdin closed → `stop`; only emitted when the host asked for it at spawn, so an
old host with a new engine keeps working unchanged. The decision is never the engine's.

**`anna` does not change.** Its turn ends when the model returns no tool call — there is
nothing to hold open — so it never emits the event, and the host continues between turns
by sending the next `user` line, as always. The emitter today is the `claude-runner` of
SHVIA-DESKTOP (its SDK has a `Stop` hook); the `codex-runner` has no hook surface.

The three version carriers agree again: 0.11.20 moved `version.md` and this file and left
`Cargo.toml` at 0.11.19 (and the lockfile with it) — the half-bump the CLAUDE.md of this
repository describes, one more time. This commit moves the four together.

## 0.11.20 - the git hooks arrive from repodocs and are enabled here

Both hooks of the standard now run here: `commit-msg`, which checks the shape of
the subject (`X.Y.Z - description`), refuses a Conventional Commits prefix and a
vague message, **and checks that the subject's `X.Y.Z` is the version this commit
carries in `version.md`**; and `pre-push`, which compares the local `version.md`
against the remote default branch for a repeated version and for one that moves
backwards.

The hook does **not** check the language and could not: what it measures is the
shape and the number.

Until now the commit rule lived here only as prose in `CLAUDE.md`, and prose is
what gets forgotten at the end of a long session. On 07/09/2026 the hooks were
enabled in 3 clones out of 58, and two repositories of the fleet were measurably
off the norm with nothing to say so.

Escape hatch, declared in both: `REPODOCS_NO_HOOK=1`. It exists so the hooks stay installed —
a guard with no declared bypass gets bypassed with `--no-verify`, which switches
off every guard at once. In a fresh clone, enable them with
`git config core.hooksPath tools/git-hooks`.

## 0.11.19 - the commit rule names every version carrier, not just version.md

The repair in `0.11.18` put the three numbers back in agreement; it did nothing
to stop them drifting again. The instruction an agent actually reads said only
*"a versão sempre vem de `version.md` (bumpe no mesmo commit)"* — true, and
incomplete: it names the authority and stays silent about the two files that
copy it.

`AGENTS.md` and `CLAUDE.md` now carry a section that names all three carriers,
says why each one holds the version, and points at the shape of commit where
this breaks — the doc-only commit, where `Cargo.toml` is never in the diff to be
noticed. It ends with the one command that answers the question before the
commit exists, because a rule that needs a person to remember it is the rule
that already failed twice here.

This is documentation, not a gate. A `pre-push` hook comparing the three would
make it mechanical, and this repository has no `tools/git-hooks/` yet.

## 0.11.18 - the version carriers catch up with version.md

`version.md` and the commit subjects reached `0.11.18`, but two other carriers of
the same number did not: `Cargo.toml` was still on `0.11.15`, three bumps behind,
and this changelog stopped at `0.11.16`. A clone built from `master` produced a
binary that introduced itself as `0.11.15`.

The comment above the version field in `Cargo.toml` had already recorded this
exact failure once — *"é como o bump da 0.11.2 saiu pela metade sem nada
acusar"*. It went halfway again, three releases in a row, and again nothing
accused it: the two commits that moved `version.md` (`fa7c331`, `7637732`)
touched only `AGENTS.md`, `CLAUDE.md` and `version.md`, so the manifest was never
in the diff to be noticed.

Found on 05/09/2026 while packaging `anna` into the SHVIA-DESKTOP release, where
`stage-anna.mjs` prints the version it is bundling. The binary reported
`0.11.18`, matching the working tree, because the fix had been made locally and
left uncommitted. Nothing shipped wrong — the sources were the committed ones —
but the number was true only on this machine.

The two entries below were written on the same day, after their commits, from
the diffs themselves.

## 0.11.18 - the human gate becomes the refused push, not the commit

Reconstructed entry — the commit (`7637732`) landed without one. It regenerated
the `COMMIT-RULE` echo block in `AGENTS.md` and `CLAUDE.md` from repodocs, adding
the paragraph that makes the push part of the delivery: a clean push needs
nobody's permission and is never held for review, while a refused push —
conflict, non-fast-forward, protected branch — is exactly where a human enters,
and is never answered with a force or an invented merge.

## 0.11.17 - the agent doc stops contradicting the COMMIT-RULE block

Reconstructed entry — the commit (`fa7c331`) landed without one. It regenerated
the same echo block, adding the clause that a repository may override the bump
rule *in writing*, and only in writing: an override nobody wrote down is not an
exception. The changelog entry, the subject format, the language and one subject
per commit hold regardless.

## 0.11.16 - COMMIT-RULE replaces the COMMITTER delegation: the agent commits again

The `PS — Commits: a skill COMMITTER cuida disso` block in this repository's
agent instructions said that committing and pushing were not the agent's job,
because a cron cycle would package the commit from the changelog entry. That
skill was switched off across the fleet on 03/09/2026 — `.committer.yml` here is
`enabled: false` and nothing reads it any more. Switching off the automation did
not unwrite the delegation, so the instruction stayed behind pointing at a cycle
that no longer runs: an agent reading it stops at a dirty working tree that
nothing is watching.

Replaced by the `COMMIT-RULE` echo block, whose single source is
[samirhvbr/repodocs `docs/versioning.md`](https://github.com/samirhvbr/repodocs/blob/master/docs/versioning.md#who-commits-and-when)
(ADR-016 there). It says the opposite and says it in one place for the whole
fleet: the agent commits, nothing is reported as finished until it is committed,
one subject per commit, and a large delivery is split into blocks grouped by
subject. Being a delimited block rather than per-repo prose is the point — the
COMMITTER rollout put bespoke wording into 42 files and left no mechanical way
to find and replace it when the decision behind it was reversed.


## 0.11.15 - `bash` gets a time ceiling, destructive is read by token, and the config's 0600 stops being a promise

Findings **F-04**, **F-05**, **F-06** and **F-07** of the 01/09/2026 technical review. All
four are the same subject — the gate, and the hygiene of whoever holds a secret — and two of
them are documentation promising what the code did not do.

### F-04 · `bash` had no timeout, and the docs said it had

`Command::output()` **waits forever**. A `tail -f`, a `sleep`, an `npm run dev` or a stuck
`cargo test` froze the turn. In the REPL the developer sees it and hits Ctrl-C; in the
**NDJSON host** — the SHVIA-DESKTOP shell — the session **goes mute**, and the only remedy is
killing the sidecar, which loses the session. The cost was not "the command is slow", it was
"the only way out loses the work".

Now: **120 s** by default, `bash_timeout` in the config or `SHVIA_BASH_TIMEOUT`, `0` disables.

- 🔴 **It kills the process GROUP, not just the `sh -c`.** What hangs is almost always the
  grandchild (the `node` of `npm run dev`); killing only the parent would leave the grandchild
  alive holding the pipe — and the read would stay hung exactly as before, with the difference
  of now lying that there was a timeout. It has its own proof: the test records the
  grandchild's PID and confirms with `kill -0` that it died.
- **Two threads drain the pipes** while the command runs. Without that, a command writing more
  than the pipe buffer (64 KB) blocks on the write, and the ceiling would kill it as if it
  were stuck — "fixing" a healthy process. Also proven.
- The timeout message tells the model **what to do** (run it in the background, or raise the
  ceiling). Without that it repeats the same command and burns another whole ceiling.
- It is a number and not an on/off because a legitimate integration suite goes past 120 s: a
  binary ceiling would push whoever has a slow suite into disabling it, which is the worst of
  both worlds.

**Measured:** with the ceiling reverted, the test `comando_que_trava_e_morto_no_teto` takes
**30.00 s** and fails; with it, 0.30 s.

### F-05 · `is_destructive` classified by SPELLING, not by semantics

`c.contains("rm -rf")` looks for the spelling, and the shell has many spellings for the same
thing. These landed in `Confirm` — where **"y/Always" counts**, which is exactly what `Always`
exists to prevent: `rm -r --force build`, `git -C other push`, `sudo rm -rf /`,
`FOO=1 rm -rf x`, `/bin/rm -rf x`, `cat list | xargs rm -rf`.

Now the line is split into segments (`;`, `|`, `&&`, `||`, command substitution), the command
is recognised after discarding transparent prefixes (`sudo`, `env`, `xargs`…) and
`KEY=value` assignments, and flags are read **grouped** (`-rf` = `-r -f` =
`--recursive --force`). `git` skips the global options that take a value (`-C <dir>`,
`-c k=v`), which is how `git -C other push` escaped. It reuses `tokens()` from F-02 — the same
definition, not a second copy.

- ⚠️ **`rm -r` without `-f` became destructive**, and that is a decision: it removes a tree
  just the same. The cost of erring on the strict side is one confirmation; on the other side,
  a deleted tree with "always" memorised.
- ⚠️ **The set of what counts as destructive did NOT grow.** `find … -delete`, `shred`,
  `chown -R` and `git branch -D` stay in `Confirm` — the house already tested them that way,
  and promoting them on the back of this fix would be changing decided behaviour by
  smuggling. It is recorded in the review as a question for the owner. There is a test that
  **fails** if the set grows.

**Measured:** with the substring back, **7 tests** go red.

### F-06 · the config's `0600` was promised on both sides and existed on neither

The README and `arquitetura-cli.md` promised `0600` and "keychain if possible". There was no
command that wrote the config, so the user created it by hand with their own umask —
typically `0644`, readable by any account on the machine — and the read said nothing. A
permission promise that nobody creates and nobody checks is **worse than silence**: whoever
read the docs believes they are protected.

- `anna config set-key [KEY]` writes with `0600`, preserving the rest of the file. It writes
  to a temporary file in the same directory, sets the permission **first** and renames —
  `rename` is atomic, and creating at `0644` to adjust afterwards would open a window with the
  key readable. **With no argument it reads from stdin**, which is the mode that keeps the key
  out of the shell history.
- On load it warns with the mode and the fixing command — **only when there is an `api_key`
  inside**. Warning on a config with no key would train the developer to ignore the warning,
  which is how a guard dies.
- **Keychain left the docs** instead of staying promised.

### F-07 · retention with no deadline, and a memory hook accepting `http://`

The session stores the whole history, including `role=tool` — everything `read_file` read —
and the last image in base64. At `0600`, but **forever**: the only cleanup was all or nothing.
Now `anna sessions clear --older-than N`, by `mtime` (`Session` has no timestamp, and a new
field would only apply to new sessions — precisely the ones that do not matter). A file with
no readable date is **preserved**: in an operation that deletes, doubt votes for not deleting.

And the `ai-memory` hook sends `bearer_auth` + prompts + 400 chars of each tool result,
accepting **any** scheme in `AI_MEMORY_URL` — over `http://` outside the loopback, token and
content in the clear on the network. The **same** rule as `SHVIA_URL` now applies
(`config::url_seguro`, imported and not re-copied: it already has proof, and a second copy
would diverge). An insecure URL **disables memory with a warning** instead of refusing to
boot — memory is optional, and taking the tool down would trade a leak for an outage.

The decision moved out of `init` into a pure function, because `init` writes to a `OnceLock`
and cannot be exercised twice in a test. Same choice as `politica.mjs` in SHVIA-DESKTOP.

### Found by the tests themselves

`clear_older_than` overflowed the arithmetic (`days * 24 * 3600`) and brought the process down
with "attempt to multiply with overflow". In an operation that deletes, an overflow must not
become a panic nor, worse, a cutoff in the future that would delete everything. Fixed with
`checked_mul` before the `checked_sub`.

**Suite: 95/95** (was 66). Clippy clean with `-D warnings` on every target. The two new
commands were exercised for real, not just by reading: `set-key` writes `600` and preserves
`url` and `profile`; the permission warning fires at `0644` and stays quiet at `0600`; an
insecure `AI_MEMORY_URL` prints the warning on the real `anna -p` path.

Docs: `docs/ferramentas.md`, `docs/arquitetura-cli.md`, `README.md`.

## 0.11.14 - the `bash` gate looks at the ARGUMENT, not just the command name

Findings **F-02** and **F-03** of the 01/09/2026 technical review — and F-02 is the one that
hurts most to read, because the lock already existed and was bypassable through the tool next
to it.

🔴 **`cat .env` came out as `Policy::Auto`.** The `ALLOWLIST` was matched by the command
**prefix** only; no argument was checked. `guard::denied` — which makes `read_file` refuse
`.env`, `*.pem`, `id_rsa*`, `credentials`, `/.ssh/`, `/.aws/` — was never consulted on the
`bash` path. The same model that the read tool blocks picked the path and went through the
shell, **with no approval card**, and the content went on to the provider. `cat ~/.ssh/id_rsa`,
`cat ../../.env` and `cat /etc/passwd` also passed.

- **The argument guard** (`argumento_de_caminho_recusado`) downgrades the segment to `Confirm`
  when a token: falls in the secrets denylist; is an **absolute** or **home** path; has a `..`
  that **escapes** the root; or carries **variable expansion** (`$HOME/x`). The check is **per
  token, with quotes resolved** — `cat '.env'` and `cat ".env"` do not dodge it. It does not
  touch the disk: an `ls folder-i-am-about-to-create` does not become a question.
- **`awk` left `Auto`** by the same criterion that already kept `sed` out: `system("…")`
  executes and `print > "file"` writes **inside the program**, which is quoted text and
  therefore invisible to the metacharacter scanner. And telling `print > "f"` from `NR >= 37`
  apart is the same ambiguity this house has already tripped on — the comment on
  `dividir_por_and` records the day a `contains('>')` refused `awk 'NR>=37 && NR<=46'`. A
  partial guard would be wrong on both sides. Cost: one confirmation per `awk`; for ranges and
  columns, `head`, `tail`, `nl`, `cut` and `grep -n` remain.
- **`git branch` is read-only only while it is read-only:** `-D`, `-d`, `-m` and friends ask
  for a card. The `git branch` prefix let `git branch -D main` straight through.
- **`sort -o out` and `uniq in out` write** — both were in `Auto` by name, and the name does
  not say the second positional is a destination.

**What was NOT closed, and is written down instead of masked:** recursive search
(`grep -r X .`) stays in `Auto` and may print a line coming from a project `.env` — closing
that means deciding by **content**, not by path, and is another guard. And `npm run`,
`cargo build/test` and `make test` stay automatic by **recorded decision**: they run what the
project declares, and an injected prompt does not change `package.json` without passing a
write card. Whoever disagrees removes the entries from the list — it is one line.

3 new tests (26 assertions), including the quoting case and the one for commands that write
through the argument. **Reversal checked on both halves:** turning the argument guard off
drops the secrets test; putting `awk` back on the list drops the one for commands that
execute. Suite **66**.

Docs in the same commit: `docs/ferramentas.md` — the `bash` section was listing an allowlist
smaller than the real one (finding F-10) and now describes the whole rule, including what it
does not cover.

## 0.11.13 - `master` compiles again: the two-version bump went out half-done

Finding **F-01** of the 01/09/2026 technical review. Versions **0.11.5** and **0.11.6**
touched only `version.md` — and this repository has **three** version carriers. `build.rs`,
which exists exactly for this, brought the build down with `BUMP PELA METADE`:

```
✗ BUMP PELA METADE — version.md diz 0.11.6, e eles discordam:
      Cargo.toml = 0.11.4
      Cargo.lock = 0.11.4
```

Two versions (29/08 → 02/09) in which `cargo check`, `clippy`, `test`, `install.sh` and the
desktop packaging all stopped at the same `panic!` for anyone cloning `master`.

- **The three carriers aligned at 0.11.7.** The change in that version is the alignment
  itself, so it got its own number instead of rewriting 0.11.6 — the history records the
  defect instead of hiding it.
- 🔴 **And nobody saw it, because CI had no way to see it.** The `build-windows.yml` trigger
  listed `src/**` and `Cargo.*`; a commit touching **only** `version.md` did not wake the
  build. The commit that broke `master` was precisely the one that did not fire CI. Now
  `version.md` and `build.rs` are in the trigger.
- **The guard existed; the other half was missing.** `build.rs` **proved** the three agree;
  nothing **made** them agree. `scripts/sync-version.sh` propagates `version.md` to
  `Cargo.toml` and `Cargo.lock`, and `--verificar` exits 1 when they diverge (same idiom as
  `sync-version.mjs` in SHVIA-DESKTOP).
- **CI now runs `cargo test --locked` and `clippy -D warnings` before the build.** A release
  build takes minutes; a broken test that only shows up after it costs the whole round.
  `--locked` is what really guards the lockfile — `build.rs` itself says so.

## 0.11.12 - CI actions get pinned by SHA, the doc's tree becomes the real one, and July's spike goes

Findings **F-09**, **F-10** and **F-21** of the September 2026 review.

- 🟢 **Every `uses:` in the CI is pinned to a commit SHA** (F-09). A GitHub Actions tag is a
  moving pointer: `actions/checkout@v4` runs whatever the owner publishes under that tag
  tomorrow, with this workflow's permissions and access to the job token — the vector of the
  `tj-actions/changed-files` incident, where a moved tag leaked secrets from thousands of
  repositories. Each SHA carries a comment naming the version, so raising a pin is a decision
  and not a side effect; the header records the `gh api` calls that resolve a tag.
  `dtolnay/rust-toolchain` has no tag — `stable` is a *branch*; the SHA freezes the action's
  code, not the Rust version, which is still whatever is stable that day.

- 🟢 **The `anna.exe` artifact ships with a `.sha256`** (F-09), in the format `sha256sum -c`
  reads — the same one `release-manifest.mjs` already produces in SHVIA-DESKTOP. Whoever
  downloads the artifact to package the desktop could not tell whether what arrived is what
  left here.

- 🟢 **`docs/arquitetura-cli.md` §2 stops describing a plan** (F-10). It listed
  `api/{mod,sse,types}.rs`, `tools/{mod,fs,edit,shell,git,guard}.rs`, `ui/{mod,repl,tui}.rs`,
  `cli.rs` and `error.rs` — a per-directory design abandoned early, none of which ever
  existed. The file was not merely old, it was being **kept old**: the notes for findings
  F-04 and F-05 were written into the description of a `shell.rs` that is not there. It now
  lists the 25 real modules with their line counts.

  Ten of those descriptions were first written from the filename and then **corrected against
  each module's own header** before shipping — `checkpoint.rs` is not a resume point, it
  photographs the working tree into a commit outside the branch before every approved
  mutation; `runner.rs` is the Local/Host executor split of ADR-002, not "run an external
  process"; `frontend.rs` is the trait that makes the engine embeddable. Guessing would have
  reproduced the exact defect being fixed.

- 🟢 **`spikes/c0-gateway.php` removed** (F-10). `docs/protocolo.md` called it "the reference
  proof" of the protocol: a July PHP client, untouched since 08/07, that nothing compiled,
  tested or ran, inside a Rust repository. A reference nobody runs proves nothing and ages
  silently. The reference is now `src/api.rs` — the client the CLI actually uses, which CI
  compiles and tests.

- 🟢 **`.continue/estado-atual.md` is framed for what it is** (F-10). The review read it as
  stale narration; it is a **dated log**, and its 08/07 entry saying C0 is "código pronto,
  falta EXECUTAR" is accurate *for 08/07*. The real defect is that the log stops at
  31/07 (0.9.0) while the repo is at 0.11.12, under a title that promises current state. A
  header now says so and points at `CHANGELOG.md`; the gap is **not** backfilled after the
  fact.

- 🟢 **`AGENTS.md` and `CLAUDE.md` gained a guard** (F-21). They are already identical here,
  which is exactly when a guard is worth adding.

**Three new rulers**, all in `src/main.rs::reguas_do_repo`, all running in `cargo test`:
`toda_action_do_ci_esta_pinada_por_sha` (sweeps the whole workflow directory, so a new
workflow is measured just by existing, and requires the version comment too),
`arvore_da_doc_bate_com_src` (both directions: a module missing from the doc, and a doc entry
missing from `src/`), and `agents_e_claude_sao_espelho`.

**Measured.** 102/102 green, clippy `-D warnings` clean. Five reversions, five reds: an
action back on a moving tag; a correct SHA without the version comment; a new workflow with a
loose action; a new module in `src/` with no line in the doc; a doc entry for a module that
does not exist.

## 0.11.11 - `cargo deny` lands, and passes with an empty exception list

Findings **DEP-3** and **F-31** of the September 2026 review.

None of the three Rust repositories measured RustSec advisories. This one now does, from
`.github/workflows/build-windows.yml`, next to the `cargo test --locked` and `clippy` that
already run before the release build.

**It passes with `ignore = []`, and that is the point.** The two Tauri repositories need
sixteen documented exceptions because of the GTK3 tree they inherit; `anna` has a small
dependency graph and no GUI toolkit, so it needs none. Writing that down is what makes a
future exception visible as a change rather than as the status quo.

The empty list is declared explicitly in `deny.toml`, with the note that an entry there is
debt carrying a written reason — the same doctrine as `scripts/prova-auditoria.mjs` in
SHVIA-WORKSPACE.

⚠️ **The interesting result is in the sibling repositories**, not here: the same check found
**19 advisories** in SHVIA-DESKTOP, including the `time = "=0.3.41"` pin that F-31 predicted
would hide one. See `SHVIA-DESKTOP` 1.4.3.

## 0.11.10 - the repository's only `innerHTML` carrying external data starts escaping

Finding **F-08** of the 01/09/2026 technical review.

`anna stats --html` builds the dashboard by interpolating `DATA` into template literals and
dropping it into `innerHTML`. The model name comes from the gateway (`telemetry.model`), so it
is **server data**: a profile named with HTML would execute in the local dashboard of whoever
ran the command.

Low risk, and it is precisely because it is low that it needed a ruler. Nobody will remember,
adding a field six months from now, that this file is different from all the others.

**What changed.** An `h()` in the template's JS, applied to every `DATA` value entering an
HTML context: `r.model`, the row's four numbers, `day[0]`/`day[1]` (the latter inside a
`title` attribute), the footer's `bits`, and both arguments of `tile()` — the label is always
a literal, but the value includes `v.model`, and escaping only one would leave the next person
to decide again.

**It escapes at RENDER time, not at generation in Rust.** `DATA` is also read as a number
(summing, sorting); escaping at the source would ruin the data for whoever uses it as data.

**The ruler** (`tests_escape_do_dashboard`, 4 cases) sweeps the `TEMPLATE` and fails if any of
those fields shows up raw again — and also if one of the **already assembled** fragments
(`${rows}`, `${cells}`, `${foot}`…) is escaped by mistake, which would show them as text on
screen.

**Measured for real, not by reading.** The binary was rebuilt, the dashboard generated, and
`h()` was extracted from the output HTML and **executed**: `<img src=x onerror=alert(1)>` comes
out as `&lt;img src=x onerror=alert(1)&gt;`. On the first attempt the escaper was **not** in
the generated HTML — the binary predated the change — and only the smoke test showed it.
Suite **99/99**, clippy clean with `-D warnings`.

## 0.11.9 - Agent doc: Releases rule and the English-only language rule

Marked echo of the single source at samirhvbr/repodocs. Two rules land here:

1. The `version.md` of the default branch ON GITHUB is what the GitHub Releases
   show, and a commit that bumps it is not finished until that version has a
   tag, a Release and the `Latest` badge — same push, not "later".
2. Everything in this repository is English (US): documents, commit messages,
   pull requests, issues, code comments. The only carve-out is end-user-facing
   product strings. History is not rewritten.

Delimited by a marker, so re-running replaces instead of duplicating.

## 0.11.8 - Regra de Releases no doc de agente: bump e Release sao um ato so

Eco marcado da norma unica em samirhvbr/repodocs (docs/versioning.md). O
`version.md` da branch padrao NO GITHUB e o que as Releases no GitHub mostram, e
um commit que bumpa o `version.md` nao esta terminado ate aquela versao ter tag,
Release e o badge `Latest`.

Bloco delimitado por marcador: rodar de novo substitui, nao duplica.

## 0.11.7 - Releases automaticas: o version.md da master vira tag e Release

O GitHub nao deduz versao de mensagem de commit: sem tag, o numero e string no
`git log` e `git diff` entre versoes nao existe. Entram o
`.github/workflows/release.yml` e o `tools/release.sh`.

**A regra:** o `version.md` da branch padrao **no GitHub** e o que as Releases
**no GitHub** refletem. Checkout local nao entra na conta. Um PR nao publica
nada; no merge, o push do `version.md` dispara o workflow e a Release vira
aquela versao.

Tag e titulo = a versao pura, sem prefixo `v`. Norma:
[samirhvbr/repodocs](https://github.com/samirhvbr/repodocs/blob/master/docs/versioning.md).

## 0.11.6 - Nota do desligamento do COMMITTER passa a citar o T5

- Correção de revisão do dono: a nota do `.committer.yml` cita o T5 pelo nome e o defeito
  medido (scan de segredo des-stageando por assunto, 5/6 falso-positivo na 2.92.0). Desligar
  tira o gatilho, não conserta o ADR-005 — quem religar precisa saber disso.

## 0.11.5 - Desliga o COMMITTER (kill-switch, a pedido do dono)

- `enabled: false` no `.committer.yml` — o kill-switch da SPEC §1.2, sem apagar o marcador.
- Desligado em toda a casa `x/SHVIA/*` em 29/08/2026: passou a haver agente mandando PR, e
  commit automático concorrendo com PR embaralha a história. Commit e push voltam a ser do
  agente da sessão.

## 0.11.4 - O turno do usuário passa a carregar imagem, e só o último turno a carrega

- **O que entra no protocolo:** `images` em `role:"user"` — lista de **base64
  puro**, sem o prefixo `data:`. É a convenção do Ollama que o gateway já falava
  para o chat (`ChatService` monta `$entry['images']` assim), e os drivers de
  nuvem derivam o mime dos **magic bytes** (`ImageMime::detect`), então o tipo
  **não viaja**. Documentado em `docs/protocolo.md`.
- **Turno sem imagem sai byte a byte como saía.** `skip_serializing_if` mantém a
  chave fora do JSON quando a lista está vazia — e isso é 100% dos turnos de
  hoje. Há régua travando o corpo exato (`turno_sem_imagem_nao_ganha_chave_nenhuma`).
- **`api.rs` não mudou uma linha:** ele serializa `&[Message]` direto, então
  campo novo na struct atravessa sozinho. O dimensionamento previa mexer lá;
  medir encolheu a fatia.
- ⚠️ **A conta que ia crescer sozinha, e o motivo de `empurrar_turno_usuario`
  existir.** O `anna` guarda o histórico em MEMÓRIA e reenvia a lista inteira a
  cada volta do laço agêntico. Se a figura ficasse presa ao turno que a trouxe,
  ela subiria de novo em cada chamada de ferramenta, de todo turno seguinte, até
  a sessão acabar — num turno de código com 30 voltas é a mesma imagem paga 30
  vezes, **com a resposta saindo certa**. Defeito que ninguém audita porque nada
  parece errado. Agora o turno novo apaga a figura do anterior e **mantém o texto
  dele**; dentro do turno ela sobrevive a todas as voltas, de propósito, porque o
  modelo precisa poder olhar a imagem outra vez depois de ler um arquivo.
- **Item que não é string sai fora** em vez de virar `""`: base64 vazio chega ao
  provedor como imagem corrompida, e o erro voltaria falando de outra coisa.
- **Cinco réguas novas** (63 no total), conferidas por reversão: tirar o
  `skip_serializing_if`, tirar a limpeza e fazer a limpeza levar o texto junto —
  cada uma acendeu só a sua.
- **Do outro lado:** `SHVIA-WEB` 2.102.39 (validação, teto do total, guarda de
  visão e o repasse na allowlist do `CodeChatController`) e `SHVIA-DESKTOP`
  1.1.31. Sem eles o campo viaja e o gateway o descarta.

## 0.11.3 - O bump da 0.11.2 saiu pela metade, e o repo ganha a prova que o desktop já tinha

- **O CASO (medido em 20/08/2026):** o commit da 0.11.2 (`3da7875`) levou
  `CHANGELOG.md`, `src/tools.rs` e `version.md` — e **deixou `Cargo.toml` e
  `Cargo.lock` em `0.11.1`**. O commit da 0.11.1 (`5cd3144`) tinha levado os cinco,
  então não era desconhecimento do padrão: foi omissão silenciosa. Nada acusou —
  árvore limpa, `cargo build` exit 0, e **o binário compilado respondendo `anna
  0.11.1` com o código da 0.11.2**.
- **Por que dói além da estética:** o `stage-anna.mjs` do SHVIA-DESKTOP decide o que
  empacotar lendo `--version` do binário, e o *"Sobre → Motor do Code"* (desktop
  1.1.25) mostra esse número **ao usuário** — entregue justamente porque a versão do
  sidecar não aparecia em lugar nenhum. Bump pela metade faz ela aparecer **errada**,
  que é pior que não aparecer: o número existe e mente.
- **E havia um aviso que se dizia guarda.** O `Cargo.toml` trazia, na linha da versão,
  o comentário *"O build-local.sh checa"*. **Não checava:** medido, todas as
  referências a `version.md` naquele script são ao `version.md` do próprio desktop.
  Não é só "aviso perde para gesto" — o aviso **afirmava ser** o gesto.
- **Entra `build.rs` como a prova:** `version.md` (fonte) × `Cargo.toml`, e o build
  **cai** nomeando o divergente. É `build.rs` e não `#[test]` porque o contrato é
  *"os portadores concordam ou o build cai"* e `cargo build` não roda testes — teste
  protege quem roda a suíte, `build.rs` protege quem só compila, que é o caminho do
  `install.sh` e do empacotamento.
- **Conferido por reversão, e o resultado corrigiu o próprio desenho.** Desalinhar o
  `version.md` derruba com a lista dos divergentes (`Cargo.toml = 0.11.3`,
  `Cargo.lock = 0.11.3`). Mas desalinhar **só o `Cargo.lock`** *passou* — porque o
  `cargo build` **reescreve o lock em silêncio** a partir do `Cargo.toml` antes de
  chamar o `build.rs`. **Quem guarda o lock é o `--locked`** (aborta com `cannot
  update the lock file`, exit 101), e isso ficou escrito no `build.rs` em vez de
  virar uma checagem que se anuncia e não age.
- Portadores desta casa, agora explícitos: **`version.md`, `Cargo.toml`,
  `Cargo.lock`** — os dois primeiros pelo `build.rs`, o terceiro por `--locked`.

## 0.11.2 - O gate de aprovação para de perguntar o que ele mesmo classifica como leitura

- **O relato (19/08):** *"no modo Aprovação Auto está demandando muito ainda,
  tem muita coisa como `ls`"*. O `ls` sozinho já era `Auto` — o que pedia
  aprovação era `cd KIDS-THALO && ls`, porque a string tinha `&&` e isso
  derrubava o comando INTEIRO do allowlist. O agente compõe `cd <pasta> && <cmd>`
  o tempo todo, então praticamente toda inspeção virava pergunta.
- **Segmento a segmento.** `&&` deixa de ser motivo de recusa: cada trecho é
  conferido isoladamente e **todos** precisam passar; um fora da lista basta para
  virar `Confirm`. O `is_destructive` continua rodando sobre a string inteira
  ANTES disso, então `ls && git push` segue `Always`.
- **A lista de leitura cresceu** com o que faltava e aparecia toda hora: `grep`,
  `rg`, `awk`, `cut`, `sort`, `uniq`, `diff`, `jq`, `which`, `cd`, `stat`,
  `basename`, `find` (com guarda) e os `git` de leitura (`rev-parse`,
  `ls-files`, `blame`…). Critério: não escreve, não apaga, não sai para a rede.
- **`sed` ficou de fora, e está registrado por quê:** `sed -n` é leitura, mas
  `sed -n 's/a/b/w saida'` grava, e separar isso de um `w` que é só parte do
  padrão (`/warn/p`) exige parsear sed. Guarda incompleta em gate de segurança é
  pior que guarda nenhuma. Quem quer intervalo de linhas usa `head`/`tail`/`awk`.
- **O scanner passou a conhecer aspas**, e isso corrigiu dois erros que a versão
  ingênua tinha — os dois no mesmo comando real, `awk 'NR>=37 && NR<=46' x`: o
  `&&` dentro do script era tratado como encadeamento, e o `>=` era lido como
  redirecionamento. Aspa simples e dupla **não** são equivalentes: dentro de
  `"…"` o shell ainda expande `$(…)` e crase, então substituição de comando é
  recusada mesmo entre aspas duplas. Aspa não fechada também recusa.
- 58 testes (7 novos), clippy limpo. **Nada foi afrouxado**: destrutivo continua
  `Always`, escrita continua `Confirm`, e não existe modo yolo.

## 0.11.1 - O aviso do teto para de vazar JSON de protocolo na tela do usuário

- A casca renderiza a string do `warn` na timeline, então `aviso_teto` do
  `JsonSink` é texto para HUMANO: mandar `{"type":"user","text":"continua"}` ali
  colocou payload NDJSON na frente de quem só quer continuar o trabalho (visto
  em 19/08 no app desktop, um turno depois de a 0.11.0 subir). O payload segue
  documentado no `embedding.md`, que é onde quem implementa host vai olhar.

## 0.11.0 - Teto de voltas deixa de ser constante e o estouro passa a perguntar ao dev (terminal) em vez de jogar o turno fora

- CASO REAL (19/08, projeto KIDS/Godot no app desktop, MiniMax M3): tarefa
  legítima de testes+correção morreu na volta 25 com "teto de voltas atingido —
  refine a pergunta" — conselho errado (a sessão fica viva; "continua" retoma) e
  teto baixo demais para trabalho agêntico de verdade (26 ferramentas no turno).
- O teto sai de `const MAX_ROUNDS = 25` para `max_voltas` no Config: env
  `SHVIA_MAX_VOLTAS` > config.toml > default 100 (`config::DEFAULT_MAX_VOLTAS`),
  mínimo 1. `anna config` mostra o valor resolvido. Continua SEMPRE limitado
  (A6/§2.7) — o que mudou é onde o humano decide.
- Estouro agora é decisão do dev, por casca (`Frontend::continuar_apos_teto` +
  `aviso_teto`): terminal interativo pergunta "continuar mais um lote?" (cada
  "sim" concede exatamente mais um `max_voltas`); host NDJSON persistente
  encerra o turno com a sessão viva e aviso para reenviar
  `{"type":"user","text":"continua"}`; `-p`/`--json -p` avisam que nada é
  persistido e o caminho é rodar de novo com teto maior. A pergunta NUNCA viaja
  pelo `gate_request` — o modo Auto do host aprovaria sozinho (loop sem humano).
- Docs no mesmo commit: protocolo.md §4.5, SECURITY §2.7, embedding.md (campo
  `text`, não `content`), arquitetura-cli.md (pseudocódigo e config.toml),
  ferramentas.md, ANNA.md (que o anna lê como contexto), README (contagem de
  testes). Revisão adversarial (24 agentes) pegou e matou: doc de retomada com
  campo errado, promessa falsa de sessão no `-p`, pseudocódigo meio-atualizado.

## 0.10.0 - O agente deixa de se apresentar como "Anna" e o histórico ganha teto (sessão longa não trava mais no 422)

- IDENTIDADE: o system prompt dizia "Você é a Anna" e modelos obedientes
  defendiam a persona contra o próprio usuário — caso real de 18/08 com o
  MiniMax M3, que tratou "você não é a Anna" como prompt injection e respondeu
  "I should continue being Anna". ANNA é a marca do Chat do ShvIA (outro agente);
  o modelo do Modo Code não deve nem tomar conhecimento dela. Prompt novo:
  "agente de código do ShvIA Code", sem nome próprio nem persona, com instrução
  de não reivindicar nem discutir identidade. Vale para os dois modos (terminal
  e host/SShvTerm). O nome `anna` segue sendo só o binário/CLI.
- HISTÓRICO: o gateway valida `messages` com max:200 (anti-abuso §3.3) e uma
  sessão longa ESTOURAVA o teto — o /code/chat rejeitava a requisição inteira
  ("must not have more than 200 items") e a sessão ficava presa, retry após
  retry (caso real: 26M tokens acumulados no M3). Agora `compactar_historico`
  roda a cada volta do loop: preserva os system iniciais (prompt-semente +
  contexto do projeto) e as mensagens mais recentes (janela nunca abre em
  role=tool órfão — provedor OpenAI-strict rejeita), e insere UM marcador system
  no corte avisando o modelo do trecho ausente (sem isso ele alucina o que não
  está mais lá). Compacta em 150 (folga de 50 para as voltas do turno);
  recompactação substitui o marcador, nunca acumula. O frontend avisa quantas
  mensagens saíram. +4 testes (47 no total), clippy limpo.
