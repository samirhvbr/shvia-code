# Changelog

Entradas no formato da mensagem de commit (`versão - comentário`), mais recente
primeiro. É daqui que a skill COMMITTER tira a mensagem (CLAUDE.md §PS).

## 0.15.0 - Budget the history against the target's context window and retry once when the provider refuses the request

The only budget was a count: past 150 messages, cut to 100, sized for the gateway's
200-message validation and blind to tokens. A local model served with `num_ctx` 32k got up to
150 messages; Ollama then either cut the start of the prompt in silence or, with
`truncate:false` (on by default since 08/08/2026), refused with a 400 that ended the turn. The
gateway has classified that refusal since 22/09/2026 (`codigo: provedor_recusou_pedido`, with
a comment saying `anna` could read the error but never handle it), and it knows the target's
window (`ModelMetadata::contextWindow`), but neither reached the client.

- **The `model` event's `messages_max` and `context_window` set the budget** (both optional;
  the matching shvia-web change sends them). The count ceiling becomes `messages_max` (cut past
  3/4, down to 1/2); without a window it stays capped at 200, since a count is then the only
  budget. With a window the history is also cut when its estimate passes 70% of it, down to
  50%, so cuts stay rare and the prefix stays cacheable.
- **The estimate is calibrated by the provider's own count:** chars/4 of the messages and tool
  definitions, scaled by the last `usage.prompt_tokens` over the chars/4 of the request that
  produced it (clamped to 0.3–4), which absorbs the tokenizer, the gateway's tool-output
  compression and its system prompt. The limits and the factor live in `Telemetry`, so the
  next turn starts already budgeted.
- **A token cut keeps what the model is about to act on:** the latest assistant turn and its
  tool results always stay, with the first and the latest requests and the single marker,
  which now says whether the cut was for the gateway's ceiling or the model's window.
- **A provider refusal is retried once in the same round**, with the history cut to half of
  what was just sent, and a warning that says so. Classified by the gateway's code, never by
  the provider's wording. Any other cause fails the same way again and is reported as before.
- An older gateway sends neither field and nothing changes: the count is the 0.13.6 one.

Hermes budgets by window fraction (50–75%) and claw-code recovers from overflow reactively;
this takes both, without a summarizing model (comparison of 02/11/2026). Tests:
`com_a_janela_conhecida_o_corte_e_por_tokens_e_guarda_o_que_importa`,
`o_evento_model_traz_os_limites_do_alvo` and, against the scripted gateway,
`a_provider_refusal_is_retried_once_with_a_shorter_history`. Reversals, each red: no token
budget, no recovery. The existing count tests caught a regression on the way (the latest-turn
guard had leaked into the count cut). Docs: `docs/protocolo.md` §2–§3, `docs/arquitetura-cli.md`
§4, `docs/USER-GUIDE.md`.

## 0.14.0 - Stop a turn with Ctrl-C or the host's cancel without killing anna, and save each round

Nothing handled SIGINT during a turn. Ctrl-C ended `anna` itself; the REPL saved the session
only after the turn, so the whole turn's history was lost; and a running `bash` child kept
going on its own, because it runs in its own process group, out of the terminal's reach. A
host had no way to stop a turn short of killing the sidecar. Hermes and claw-code both stop
the turn and keep the transcript valid (comparison of 02/11/2026).

- **One stop flag** (`src/interrupcao.rs`), raised by Ctrl-C in the terminal (REPL and `-p`)
  or by `{"type":"cancel"}` from an NDJSON host, and checked where the turn waits: the
  request and the stream (dropped, closing the connection), the 0.13.10 retry wait, between
  tool calls, inside `bash` (which kills its process group, even with `bash_timeout = 0`),
  and while a host decides a gate or runs a command.
- **The history stays valid.** A cut-off answer is kept as an assistant message with what was
  already shown, marked `[interrompido pelo usuário]`, and its tool calls are dropped unrun.
  Every tool call not yet run gets `RECUSADO: o usuário interrompeu o turno…`, so the next
  request carries no orphan call. The turn ends with `turn_end`, never an error.
- **A second Ctrl-C ends `anna`** (exit 130), as does Ctrl-C when no turn runs: the way out
  of anything that does not look at the flag. At the REPL prompt Ctrl-C still exits as before.
- **NDJSON (protocol addition, hence 0.14.0):** one engine thread reads stdin, so `cancel`
  reaches a streaming turn; other lines are queued in order. `cancel` also stops a `user` turn
  already queued and is ignored when nothing runs; `exit` also stops the turn in progress. A
  stopped turn emits `{"type":"interrupted"}` before `usage` and `turn_done`. Old hosts that
  never send `cancel` see no change.
- **The REPL saves the session after every round that ran tools**, and again when the turn
  ends. `run_turn` gained `ao_fim_da_volta` for that; the planning pipeline stops when any
  phase is interrupted, instead of acting on a cut-short plan.

`tokio` gains `signal` (its `signal-hook-registry` was already in the lockfile through
crossterm). Tests, against the scripted gateway: `ctrl_c_stops_the_turn_not_anna_and_each_round_is_saved_before_the_turn_ends`
(a real SIGINT to the REPL) and `ndjson_cancel_kills_the_running_command_and_answers_every_pending_call`
(`sleep 30` killed within the test's seconds). Four reversals, each red: no Ctrl-C handler
(anna dies), no per-round save (no session mid-turn), `bash` ignoring the flag (15 s
timeout), no synthetic result (the second call runs). Docs: `docs/embedding.md`,
`docs/USER-GUIDE.md`, `docs/arquitetura-cli.md`, `docs/ferramentas.md`, `SECURITY_GUIDELINES.md`.

## 0.13.10 - Retry a gateway round that failed before the stream opened, honoring Retry-After

The gateway throttles each key (`/code/chat`, 20 requests per minute by default) and every
round of a turn is one request, while reads run without approval. A fast turn goes over the
limit, and the 429 ended the turn with an error: the work stopped mid-way and the person had to
say "continua". Hermes and claw-code both retry with backoff (comparison of 02/11/2026).

- A round that fails **before the stream opens** with 429, 408, 502, 503 or 504, or that never
  connected, is sent again up to 4 times. The wait is the gateway's `Retry-After` (Laravel's
  throttle sends it with every 429); without it, 2, 4, 8, 16 s capped at 30 s, plus up to 20%
  jitter. Each wait shows a `warn` (terminal and NDJSON) with the reason and the attempt.
- A `Retry-After` above 120 s is reported instead of waited for.
- **Not retried, on purpose:** 400/401/403/422 fail the same way again; a 500 is a gateway
  defect; an SSE `error` comes after the gateway already ran its own failover; and a connection
  that broke after the request was sent may have started a billed inference, which a retry
  could pay for twice. Nothing the person has seen is ever repeated: a retry happens only when
  no event reached the frontend.

`tokio` gains its `time` feature for the wait (no new crate in the lockfile). Tests:
`nova_tentativa_so_para_falha_que_passa_sozinha` (unit) and
`a_rate_limited_round_is_retried_after_the_gateways_retry_after`, which runs the real binary
against a scripted loopback gateway (`tests/common`, shared with the next process-level
tests); with the retry disabled it fails with a single request. Docs: `docs/protocolo.md` §5,
`docs/arquitetura-cli.md` §4.

## 0.13.9 - Apply the secrets denylist to grep and to the real target of every file tool

An external review (26/10/2026) found three ways around the confinement and the secrets denylist
(SECURITY_GUIDELINES §2.3), all confirmed by running the tools against a scratch project:

- **The `grep` tool never applied the denylist.** `read_file certs/server.pem` was refused, but
  `grep SECRET` returned its lines — and those of `aws_credentials.txt`, `deploy/id_rsa` and
  `config/app.key`. An explicit `path=".env"` also bypassed `.gitignore` and the hidden-file
  filter. Now an explicit path on the list is refused, and denied files are skipped during the
  walk by their real path, with one closing line saying how many were skipped.
- **A symlink took file tools past both guards.** `resolve_new` checked only the parent folder, so
  an approved `write_file notes.md` with `notes.md -> ../outside.txt` overwrote the outside file,
  and the denylist saw only the requested name: `read_file readme-env.txt` (a link to `.env`)
  returned the secret with no approval, and `edit_file hook.txt` (a link to
  `.git/hooks/pre-commit`) rewrote a git hook. Now an existing name is resolved through its links
  and must stay inside the project, the denylist is checked on the real file by `read_file`,
  `grep`, `edit_file` and `write_file`, and the approval card names the real file when it differs
  (`notes.md → docs/notes-v2.md`). A symlink already in the tree is what an untrusted repository
  brings with it.
- **`bash` read files through shell globs.** The argument check reads tokens literally, and the
  shell expands globs after it: `cat .e*`, `cat certs/*` and `head -n 50 .e*` were `Auto`. Now an
  unquoted glob (`*`, `?`, `[`, `{a,b}`) in a non-flag argument of a content-printing command asks.
  Flags and quoted patterns do not count, and name-only commands (`ls`, `wc`, `stat`, `tree`,
  `git ls-files`…) stay `Auto`. The price is that `grep TODO src/*.rs` now asks.

Still open, and written down rather than implied: recursive search in `bash` (`grep -r X .`,
`rg X`) stays `Auto` by an earlier decision and can print a line from a denied file; the `grep`
tool is now the safe way to search. No `O_NOFOLLOW` open is used, so a link created between the
check and the write — by a concurrent process, since tools run one at a time — is not covered.

Tests: `grep_respeita_a_denylist`, `link_simbolico_nao_fura_o_confinamento_nem_a_denylist` (Unix)
and `glob_sem_aspas_nao_e_automatico`. Docs: `docs/ferramentas.md`, `SECURITY_GUIDELINES.md`. Opened
as 0.13.6 on 27/10/2026; renumbered to 0.13.9 on 31/10, after master used 0.13.5 to 0.13.7 and #13
took 0.13.8.


## 0.13.8 - Preserve final assistant answers in conversation history

The agent displayed final answers but only added assistant messages with tool calls
to its history. A follow-up such as "use your previous answer" therefore reached
the stateless gateway without that answer, in both the persistent NDJSON process
and the terminal. Terminal autosave and resumed sessions also omitted those replies.

Append each successful assistant response once before checking whether the turn
has finished. Tool calls and their results keep their existing order. The
orchestrator's JSON-format retry reuses that recorded answer instead of appending
it again. Integration tests launch the real CLI against a local SSE fixture and
verify the next NDJSON request, terminal autosave, a resumed request, and the
orchestrator's retry. The tests isolate the home directory on Unix; no live model
or Windows session-store behavior is covered.

An empty final answer (no text, no tool calls) stays out of the history: it has
nothing to remember, and the gateway's OpenAI-compatible driver would send it as
`{"role":"assistant","content":""}` in the middle of the conversation, which some
providers reject (the Anthropic driver already drops it). Test
`a_resposta_entra_no_historico_e_a_vazia_fica_de_fora`; without the check it fails
on the empty answer.

## 0.13.7 - Take the patched quinn-proto, which stopped `install.sh` at its strict dependency audit

`./install.sh` runs `build-local.sh --audit-strict`, and on 31/10/2026 it stopped there with "5 vulnerabilities found":
`quinn-proto` 0.11.16 in `Cargo.lock` has RUSTSEC-2026-0338, -0339, -0340, -0341 and -0342 (two of them 7.5 high). The
owner hit it installing 0.13.6 on the Linux build machine, which the Desktop needs since its `anna` floor moved to 0.13.6.

- `cargo update -p quinn-proto`: 0.11.16 → 0.11.19, the only package the lock moves.
- `quinn-proto` is not compiled into `anna`: `cargo tree -i quinn-proto --target all --all-features` prints nothing. It sits
  in the lock as `reqwest`'s optional HTTP/3 dependency, and `cargo audit` reads the lock, not the build graph.
- After the update, `cargo audit` reports no vulnerability (the two yanked warnings, `chacha20` and `clipboard-win`, are
  allowed and do not stop the install) and `cargo deny check advisories` is ok.

## 0.13.6 - Keep the person's requests through history compaction, and compact once every ~25 rounds instead of on every round

The loop compacts the history before each request so it stays under the gateway's 200-message
limit. Until now it slid a 150-message window: past that size, every round dropped the 2–4 oldest
messages, and only the leading `system` messages were protected. Seen on 31/10/2026 in a Desktop
session on MiniMax M3.1 Flash: a "histórico compactado" notice before almost every tool call, the
same three worktrees' `git diff` read twice each, and the 100-round ceiling reached at 153 tools
and 4.47M tokens.

- **The request survives.** The session's first `user` message and the latest one stay in the
  context. A turn resumed with "continua", or continued by the Run's orchestrator, used to keep
  only the continuation; the new test measures the old loop losing the request at round 75.
- **Compaction is rare.** Past 150 messages the history is cut down to 100, so the next cut is
  ~25 rounds away: in the test's 300 rounds of one tool each, nine cuts instead of one per round
  past round 75. The prefix the provider receives stays the same between cuts.
- **The marker stays between cuts.** A history at or below 150 is left alone, including the
  earlier cut's marker; 0.13.5 removed the marker before counting, so a history that shrank to
  150 could lose the note that a stretch was missing.
- **The marker no longer sends the model back over its own work.** It says the requests are still
  there, to continue from the recent messages, and to use tools only for what the next step needs
  and is not in view. The notice to the person says the requests were kept.
- Tests: `o_pedido_sobrevive_a_um_turno_longo_e_ao_continua`, `compacta_de_vez_em_quando_nao_a_cada_volta`
  and `abaixo_do_teto_o_marcador_de_um_corte_anterior_fica`. Reversals, each red: without keeping
  the requests (request lost at round 75), without keeping the latest one ("continua" lost at
  round 155), with a cut down to 150 (cuts again on the next round), and with the marker removed
  before the count (marker lost).
- `docs/protocolo.md`, `docs/USER-GUIDE.md` and `docs/arquitetura-cli.md` state the new numbers.

## 0.13.5 - Document the current CLI and its execution boundaries

- Replace the early-MVP README with the implemented terminal, NDJSON, host-runner,
  planning, task-list, session, checkpoint, statistics and optional-memory surfaces.
- Add a documentation index, command/configuration guide and development/distribution
  guide; reconcile architecture, security, embedding and dated design records with source.
- Correct the short agent context's obsolete direct-master and Portuguese-publication
  instructions; preserve generated fleet rules and unimplemented queue plans.
- Record source-inspection findings for a separate implementation decision. No Rust,
  shell or workflow logic changes. Synchronize version carriers as required for docs.
- Qualify the findings after independent review: ignore/hidden rules versus secret
  filtering, the protected session directory, and transport versus HTTP/SSE failures.

## 0.13.4 - Take the patched rustls, rustls-webpki, tokio and webpki-roots, which turned `cargo deny` red

`cargo deny check advisories` fails on `master` since RustSec published seven advisories against
four crates locked in `Cargo.lock` (measured 23/10/2026, the same command the `test` workflow runs;
the last green run was 13/10/2026, before the advisories existed, so the red is newer than the code):

- `rustls` 0.23.45: RUSTSEC-2026-0346 (`Debug` impls expose TLS secrets), -0347 (data sent to the outer
  server after ECH rejection), -0348 (Brotli certificate decompression can allocate 1 GiB before
  authentication), -0349 (an unauthenticated ServerHello can downgrade later connections).
- `rustls-webpki` 0.103.15: RUSTSEC-2026-0382 (distrusted intermediates may be accepted).
- `webpki-roots` 1.0.8: RUSTSEC-2026-0381 (bundled roots trust distrusted emSign intermediates).
- `tokio` 1.52.3: RUSTSEC-2026-0386 (`AsyncFd::new` violates I/O safety).

`cargo update -p rustls -p rustls-webpki -p tokio -p webpki-roots` moves them to 0.23.46, 0.103.16,
1.54.1 and 1.0.11, all semver-compatible; nothing else in `Cargo.lock` changes and `Cargo.toml` is
untouched except for the version. `deny.toml` keeps its empty ignore list, which is the state it
declares as intended: these are real flaws with a published fix, not exceptions.

Also seen and left alone: `cargo deny check licenses` fails on `anna` itself ("unlicensed", the
crate declares no license). The `test` workflow only runs `check advisories`, and the same crate
in the desktop repository solves it with `[licenses.private] ignore = true`; that is a separate
change.

## 0.13.3 - the Linux binary is built after the Release, so it no longer waits on the runner it holds

**0.13.2 came out without its binary.** `linux-binary.yml` ran on the same push as release.yml,
built anna, and then waited up to ten minutes for the Release to exist. On the self-hosted
runner — one per repository — that wait held the only runner while the Release job sat queued
behind it, so the wait ran out every time (measured on 12/10/2026, the first push that ran it).
The job now starts on `workflow_run`, when the Release workflow completes: the Release exists by
then, and the runner is held only to build. That event is delivered even though release.yml
publishes with `GITHUB_TOKEN`, because the Release run itself was started by the push. A Release
run that failed is skipped; a dispatch with a tag still builds and attaches to that Release, which
is how 0.13.2 got its binary.

## 0.13.2 - every Release carries a static Linux binary, the one the ShvIA Workspace installs

**Master builds again.** 0.13.1 bumped `version.md` and left `Cargo.toml` and `Cargo.lock`
at 0.13.0, so `build.rs` refused to compile master — and the tag `0.13.1` with it (measured on
12/10/2026, building the tag for the Workspace). The three carriers move to 0.13.2 together.
The code is 0.13.0's; 0.13.1 changed only the echo blocks.

**A static Linux binary on every Release.** The new workflow `linux-binary.yml` builds anna for
`x86_64-unknown-linux-musl` on each push that bumps `version.md`, refuses a binary that is not
static or whose `--version` is not `version.md`'s, and attaches `anna-x86_64-unknown-linux-musl`
and its `.sha256` to that version's Release. The ShvIA Workspace runs the engine in each
project's runner container (SHVIA-WORKSPACE 0.33.0), and neither the VM nor the runner image has
a Rust toolchain; until now no Release carried a binary, because the desktop builds anna from
source. The build runs in a Rust image pinned by digest, in Docker, so the self-hosted runner's
host is left as it was. It waits for the Release instead of reacting to it: release.yml creates
it with `GITHUB_TOKEN`, and an event made with that token starts no other workflow.

## 0.13.1 - the echo blocks are regenerated from repodocs

The marked rules in `CLAUDE.md` and `AGENTS.md` are rewritten from the single
source at [samirhvbr/repodocs](https://github.com/samirhvbr/repodocs):
`QUEUE-RULE`, `RELEASES-RULE`, `LANGUAGE-RULE`, `COMMIT-RULE` and `CICD-RULE`.
A block is replaced whole between its markers, heading included — which is what
stops a local edit from surviving a regeneration and confusing the next reader.

`QUEUE-RULE` is new and arrives here for the first time: `.continue/` holds work
that does not exist yet, and a document leaves it when — and only when — the
thing it describes **exists**. Length, language and untidiness are not exit
conditions. **Never empty that folder as tidying.**

## 0.13.0 - the agent keeps a task list with todo_write, which ShvIA's Code mode shows in its Tasks tab

The owner, 30/09/2026, after opencode's sidebar: the list the agent builds and ticks off while it
works. ShvIA's Code mode gained a **Tasks** tab (SHVIA-WEB) that reads Claude Code's `TodoWrite`;
the SHVIA engine had no such tool, so with it the tab stayed empty.

- New tool `todo_write` (gate auto): `todos[{content, status: pending|in_progress|completed,
  activeForm?}]`, the whole list each call — Claude's `TodoWrite` shape, so the screen parses both
  alike. It validates (≤50 tasks, content, status) and answers a one-line summary, warning when
  more than one task is in progress. The list reaches the screen through the `tool_call` event the
  stream already emits.
- It writes nothing on disk: the read-only (plan) runner offers it too.
- The system prompt asks for it on work with 3 or more steps, and not on a simple request.
- Doc: `docs/ferramentas.md`. Tests: 4 new (181 in all).

## 0.12.6 - the answer is rendered as markdown: nested lists, tables, quotes, links and highlighted code

Fourth and last delivery of the restyle (ADR-004). `render.rs` was three regexes over one line;
it is now `Md`, which keeps the little state a line cannot know.

- **Lists and blocks.** Bullets nest (`•`, `◦`, `▪`), checkboxes become `☑`/`☐`, numbered lists
  keep their numbers, a quote gets a bar, `---` a rule. Headings lose their hashes and are styled
  by level.
- **Tables** are held back until the last row and then drawn with box characters, columns aligned
  as the separator row says (left, right, centre). A table wider than the screen falls back to
  plain rows instead of wrapping into a mess; pipes without a separator row stay text.
- **Links** show their text underlined and the address after it; `~~strike~~` works.
- **Code blocks** show their language and colour keywords, strings, numbers and comments for
  Rust, Python, JavaScript/TypeScript, shell, PHP, Go, SQL, JSON, TOML and YAML. One line at a
  time, so a multi-line comment or string colours only its first line. An unknown language stays
  plain.
- **Styles nest.** Each one is switched off by its own code, not by a full reset, so a code span
  inside bold no longer cancels the bold, and a heading keeps its weight around an inline code.
- Without colour the markers are dropped and nothing is added; code spans keep their backticks,
  because they carry meaning. A fence left open at the end of an answer no longer leaks into the
  next one.

## 0.12.5 - an edit shows a numbered diff on a red/green band, and approving is an arrow-key menu

Third delivery of the visual work (ADR-004): what the person sees and answers before anything is
written or run.

- **The diff** (`diff::pretty`) has a summary line ("2 linhas adicionadas, 1 linha removida"), the
  line number of every changed line, its sign, and — on a terminal with colour — a red or green
  background across the whole width (true colour when `COLORTERM` says so, the 256-colour palette
  otherwise). Unchanged context is grey. A new file says "Novo arquivo · N linhas". Nothing is
  left out of the preview: the approval is for the whole change. Without colour the same lines
  come out plain, numbers and signs intact. `diff::unified` stays as it was for the NDJSON host.
- **The approval is a menu** (`src/menu.rs`): arrow keys and Enter, a number (`1`/`2`/`3`) or a
  letter (`s`, `t`, `n`). Esc, Ctrl-C, `q` and a terminal error all refuse. A destructive command
  opens with the cursor on "Não", so a stray Enter refuses; it has no "don't ask again". After
  the answer the menu is erased and one line stays in the scrollback (`✔ Sim`, `✘ Não`).
  The same menu asks whether to run the orchestrator's plan and whether to give the turn one more
  batch of steps.
- **Without a terminal on both ends** (a pipe, `TERM=dumb`) the question is the line on stdin it
  always was, and `anna -p` still refuses writes and commands that need a card.
- **"Don't ask again this session" no longer hides the change.** The diff or command is still
  printed for what it lets through, only the question is skipped.
- A command's preview is the command and its reason; a commit's is the staged stat and the message.

## 0.12.4 - the turn reads like Claude Code: ● for an action, ⎿ for its result, a spinner while the model thinks

Second delivery of the visual work (ADR-004). What `anna` prints while a turn runs.

- **Actions and results.** A tool call is `● Read(src/boleto.rs)` and its result a line under it,
  `⎿  142 linhas` — a summary, never the file (the model still receives everything). Each tool has
  its own summary: lines read, results found, files matched, the first lines of a command's output
  with `… +N linhas`, the exit code in red when it is not zero, a timeout as one sentence. Errors
  and refusals are red. The raw JSON arguments (`⚙ read_file({"path":...})`) are gone.
- **A spinner while the model thinks or a command runs** (`✻ Pensando… (12s)`, `✻ Executando… (3s)`),
  on a terminal only. It owns its line: everything that prints stops it first, and dropping it (an
  error leaving the turn) clears the line.
- **The assistant's text opens with `●`** instead of the `ANNA` label, with a blank line between
  blocks on a terminal.
- **The footer** reads `✻ 14s · 3,6k tokens · US$ 0,0116` (`≈` when the provider did not report the
  cost), with the turn's duration. The orchestrator's per-role lines keep the old footer.
- **A welcome box** replaces the one-line banner: version, model and infra, the folder (`~` for the
  home) and the branch.
- `src/view.rs` holds all of the above as pure functions, so the look is tested without a terminal.
  `crossterm` (the window width) and `unicode-width` (the visible width of a line) enter here; the
  audit stays clean. `Frontend` gains `waiting()`, a no-op for the NDJSON host.

## 0.12.3 - the terminal decides once whether it can take colour, and a pipe gets none

First delivery of the visual work (ADR-004): the base the next three stand on. No look changes
yet, only where the escape codes come from.

- **`src/term.rs`** is now the only module that writes an ANSI code. Colour is written when stdout
  is a terminal and `TERM` is not `dumb`; `FORCE_COLOR=1` forces it (a pipe included), and
  `NO_COLOR` (no-color.org) wins over both. The decision is a pure function of the environment,
  covered by tests that need no terminal.
- **A pipe stops carrying escape codes.** `anna -p "..." | tee log` used to fill the log with
  `ESC[92mANNA` and `ESC[90m...`; it now carries the same words and nothing else. The same goes
  for `anna stats`, which composes the whole screen and lets `term::emit` strip the codes on the
  way out.
- `ui.rs`, `render.rs`, `diff.rs` and `stats_term.rs` lost their hand-written colour constants.
  The REPL prompt goes through `term` too.
- `--json` is untouched: `JsonSink` never printed colour.

## 0.12.2 - the roadmap gains the C5 item to list usable infras and their favorite models

Documentation only. `docs/arquitetura-cli.md` §7 now carries a C5 item: `anna` should list the
infras the user can use (enabled, API configured, online) and, per infra, the favorite models,
in the exact `model@infra` form that `--model` and `/model` take.

Why: nothing in the CLI shows what can be used. On 29/09/2026 a benchmark install failed with
`Modelo "deepseek/deepseek-v3.2" não encontrado no servidor "minimax"`, and finding the server
that does have it (`kilo`) took reading the gateway. The gateway already answers `GET /api/v1/servers`
and `GET /api/v1/profiles` to an API key, with `is_favorite`, `api_key_configured` and `online`,
so the item is client-only.

## 0.12.1 - CI runs on the self-hosted runner cicd while the repository variable CI_RUNNER names it

The account's GitHub Actions budget ran out on 25/09/2026, and since then this private repository's jobs
fail in seconds with zero steps. Every Linux job now reads
`runs-on: ${{ vars.CI_RUNNER || 'ubuntu-latest' }}`. With the repository variable `CI_RUNNER=shvia-ci`
it runs on `cicd`, the self-hosted runner set up on 28/09/2026 (its runbook is SHVIA-WEB
`deploy/servidor-cicd.md`). Deleting the variable moves it back to GitHub-hosted runners.

The Windows build (`build-windows.yml`) stays on `windows-latest`: this runner is Linux.

## 0.12.0 - the installer audits the dependencies, and a finding stops the install

Finding f165, the owner's answer on 24/09/2026: "lock". `install.sh` called `build-local.sh`
without `--audit`, so the dependency audit never ran on an install.

- `install.sh` now runs `build-local.sh --audit-strict`. A vulnerability makes `cargo audit` exit
  non-zero, and `set -e` stops the install before the build.
- `--audit-strict` also stops, with exit 3 and the install command, when `cargo-audit` is missing.
  The plain `--audit` still prints "skipping" in that case: an audit that did not run must not
  read as one that passed, and that is the difference between the two flags.
- No bypass on purpose: the owner chose to block.

Checked by hand, because this repository has no harness for its shell scripts. Without
`cargo-audit` on the `PATH`: exit 3 and no build step. With a `cargo` that answers `audit` with
a vulnerability: exit 1 and no build step. The real audit on today's lockfile passes (one
allowed "yanked" warning), so installs keep working.

## 0.11.34 - the grandchild test measures on Windows instead of racing the fork

With the Job Object (0.11.33) the Windows run returned in 1.78 s instead of 30 s, the kill
worked, and `o_neto_morre_junto_com_o_pai` then failed on its own premise: *"o neto nem
chegou a nascer"*. Three test defects, none in the product:

- **The PID path.** `display()` gives `C:\Users\...`, and inside the inner `sh -c` those
  backslashes are escapes, so the PID was written somewhere else. It now passes the path
  with forward slashes, in double quotes.
- **The ceiling raced the fork.** Git for Windows' `sh` forks slowly; at 500 ms the job
  killed the tree before the grandchild existed. The ceiling is 3 s on Windows and stays
  500 ms elsewhere.
- **The liveness check could pass without measuring.** It ran a `kill` binary and read
  "could not run" as "dead" (`unwrap_or(false)`). It now uses `sh`'s builtin `kill -0`,
  which resolves the PID `sh` printed, and a shell that cannot run fails the test.

Reversal on Linux: with the tree kill replaced by `filho.kill()` the test fails
(*"não voltou a tempo"*, 30 s).

## 0.11.33 - on Windows the ceiling kills the tree through a Job Object

0.11.32's `taskkill /T /F` was not enough. On its branch the Windows runner passed
`comando_que_trava_e_morto_no_teto` and still failed `o_neto_morre_junto_com_o_pai` with
*"não voltou a tempo"*. `taskkill /T` walks parent PIDs, and under Git for Windows' `sh` a
grandchild whose parent already exited has no parent link left to walk.

Each timed `bash` call now holds its process tree (`Arvore`): on Windows a Job Object the
`sh` is assigned to right after `spawn()`. Job membership is inherited at process creation,
so it survives what breaks the parent links. The ceiling calls `TerminateJobObject`.
`taskkill` stays as the fallback when the job cannot be created or assigned. There is no
KILL_ON_JOB_CLOSE, so a command that ends normally keeps what it started in the background,
as before. On unix `Arvore` is the existing process group and `matar_grupo`, unchanged.

New Windows-only dependency: `windows-sys` 0.59 (features `Win32_Foundation`,
`Win32_Security`, `Win32_System_JobObjects`), already in the tree through tokio/mio; the
lockfile gains one line.

Checked: clippy `-D warnings` for `x86_64-pc-windows-gnu` compiles the Windows branch;
Linux tests 105 passed; `cargo deny check advisories` ok. Whether the job reaches the
grandchild is measured only by `build-windows` on this branch.

## 0.11.32 - the bash time ceiling kills the process tree on Windows too

On Windows `matar_grupo` only called `filho.kill()`, which ends the `sh` and leaves the
grandchild (`sleep`, `node`, a test binary) holding the output pipe. Reading the output
then waited for the grandchild, which is the hang the F-04 ceiling exists to stop. The
Windows CI measured it once 0.11.27 stopped hiding test failures:
`comando_que_trava_e_morto_no_teto` and `o_neto_morre_junto_com_o_pai` both failed there.
(0.11.28 fixed the same two tests on Linux, where the cause was different: `kill` needed `--`.)

It now runs `taskkill /T /F /PID <pid>`, which walks the tree by parent PID, and keeps
`filho.kill()` as the fallback.

Checked: `cargo clippy --target x86_64-pc-windows-gnu --all-targets -D warnings` compiles
the Windows branch cleanly. Whether it kills the tree is measured only on the Windows
runner; this delivery lands after `build-windows` is green on its branch.

## 0.11.31 - the doc-tree ruler reads a CRLF checkout

`arvore_da_doc_bate_com_src` looked for `"\nsrc/\n"` in `docs/arquitetura-cli.md`, and a
Windows checkout with `core.autocrlf=true` has CRLF there, so the ruler panicked with *"a §2
tem o bloco da árvore começando em `src/`"* before measuring anything. It now normalizes
`\r\n` to `\n` after reading the doc. The repository has no `.gitattributes`, and the fix
stays in the ruler rather than forcing LF on every Windows clone.

Measured by converting the doc to CRLF locally: with the normalization the ruler passes;
without it, it fails as on the Windows runner.

## 0.11.30 - a checkpoint restore gives back the exact bytes, line endings included

`/rollback` rewrote line endings. The checkpoint's git calls inherited the user's
`core.autocrlf`, and with `true` (the Windows default) `git restore` converted LF to CRLF on
the way back: a file that was `"estado na foto\n"` in the photo came back as
`"estado na foto\r\n"`. That is what the Windows CI measured once 0.11.27 stopped hiding
test failures (`snapshot_e_restore_devolvem_o_estado_da_foto`).

Every checkpoint git call now runs with `-c core.autocrlf=false`, so the photo and the
rollback are byte-exact. Repository `.gitattributes` still apply and round-trip on their own.

New test `o_restore_devolve_os_bytes_exatos_mesmo_com_autocrlf` sets `core.autocrlf=true` in
its sandbox. autocrlf converts on checkout on every OS, so it reproduces the defect on Linux
too: without the fix it fails with the restored bytes ending in `13, 10`. The two snapshot
tests now share a lock, because the 30 s debounce is process-wide and would make the second
one flaky in parallel.

## 0.11.29 - rustls moves past RUSTSEC-2026-0285

`rustls` 0.23.41 → 0.23.45 (and `rustls-webpki` 0.103.13 → 0.103.15, pulled along), via
`cargo update -p rustls`. RUSTSEC-2026-0285: rustls accepted TLS 1.3 handshake messages
sent at the wrong encryption level. It reaches `anna` through `reqwest` → `hyper-rustls`,
i.e. every call to the gateway.

The advisory check has existed since 0.11.15 and never ran (see 0.11.27), so this is the
first time it could say anything. Its first run on Linux flagged exactly this one advisory.

Checked: `cargo deny check advisories` → `advisories ok`; with the previous lock it fails
with RUSTSEC-2026-0285. `cargo test --locked` 104 passed, clippy clean.

## 0.11.28 - the bash time ceiling kills the whole process group again

`matar_grupo` ran `kill -KILL -<pgid>`, and procps-ng's `kill` does not read a bare
`-<pgid>` after the signal as a PID. Measured: `kill -KILL -999999` exits 0 without a
word, while `kill -KILL -- -999999` says "No such process". Now it runs
`kill -KILL -- -<pgid>`.

The first run of the new Linux job (0.11.27) is what caught it:
`comando_que_trava_e_morto_no_teto` took the full 30 s ("demorou 30.003s — o teto não
cortou") and `o_neto_morre_junto_com_o_pai` did not return in time. The belt-and-braces
`filho.kill()` killed the `sh`, but the grandchild kept the output pipe open, and reading
the output waited for it. That is the exact hang the F-04 ceiling exists to prevent.

On this machine both tests passed with and without the `--`, so a local run cannot tell the
two apart; the GitHub runner is the measurement.

## 0.11.27 - a Linux job runs the tests on every pull request

New workflow `.github/workflows/test.yml`, on `ubuntu-latest`, on every `pull_request` and
every push to `master`: `cargo test --locked`, `cargo clippy --all-targets --locked -- -D
warnings`, and the RustSec advisory check (`cargo deny check advisories`).

Until now the only workflow that ran the tests was `build-windows.yml`, on push to `master`
only, so a pull request had **zero checks**. That is how 0.11.23-0.11.25 reached review
without building at all (see 0.11.26).

The advisory check **moves** here from the Windows job. It is a container action, and GitHub
runs container actions on Linux only: on `windows-latest` it failed every run since 0.11.15
with *"Container action is only supported on Linux"*. That was the red X on `master`, and
it hid a second defect (next entry).

A failing test now fails the Windows build. The Windows step ran `cargo test` and `cargo
clippy` in one PowerShell script, and a PowerShell step takes the exit code of its **last**
command only. The 12/09 run logged `test result: FAILED. 95 passed; 5 failed` and the step
still went green, because clippy passed after it. One failure was the SHA-pin ruler
(fixed in 0.11.23); the other four were never reported:
`checkpoint::tests::snapshot_e_restore_devolvem_o_estado_da_foto`,
`reguas_do_repo::arvore_da_doc_bate_com_src`,
`tools::tests::comando_que_trava_e_morto_no_teto` and
`tools::tests::o_neto_morre_junto_com_o_pai`. They are now visible, so the Windows build
is red for them until they are fixed. (This entry first called all four Windows-only; the
new Linux job's first run failed the last two as well, see 0.11.28.)

## 0.11.26 - the three version carriers agree again

`version.md` went 0.11.22 → 0.11.25 across three commits (0.11.23, 0.11.24, 0.11.25) and
`Cargo.toml` and `Cargo.lock` stayed at 0.11.22. `build.rs` refuses to compile when the
three disagree, so from 0.11.23 on this branch did not build at all — `cargo test` stopped
at the build script, before a single test ran.

The commit that started it was 0.11.23: it was tested, then bumped, and the bump was the
one change nobody compiled. The next two bumps copied the same shape. This commit only
moves `Cargo.toml` and `Cargo.lock` to the version `version.md` now declares; no code
changes.

Checked: `cargo test --locked` and `cargo clippy --all-targets --locked -- -D warnings`
on this commit.

## 0.11.25 - the repository stops choosing the model

`CLAUDE_CODE_SUBAGENT_MODEL` leaves `.claude/settings.json`. The model is now the user's
choice, made with `/model`, and a subagent inherits it: by default Claude Code gives a
subagent the session's model, and this variable was the only thing making it different —
with the session on Opus 5.5, subagents were measured on Opus 5, because the variable names
the `opus` alias and the alias still resolves through the organization's managed pin.

`.claude/README.md` stop(s) describing a model profile.

Rule and measurement: repodocs ADR-027.

No test: configuration and documents. Checked that the file parses and that repodocs
runbook §7's check is silent here.

## 0.11.24 - the model pin leaves .claude/settings.json

`"model": "opus[1m]"` and the `ANTHROPIC_DEFAULT_OPUS_MODEL` env pin are gone.
The window suffix was a version pin in disguise — the 1M variant existed only for the
previous Opus, so every session was born on it while the catalog already offered the
newer one. The env var is worse than a pin: it redefines what `opus` means for
everything that reads it, the model picker included.

It unblocks nothing on its own: the deciding layer is the account's server-managed
settings, which outrank every local file. Rule, measurement and what to write instead
(`"model": "opus55"`, the version named): repodocs ADR-026.

No test: two JSON keys and a comment. Checked that the file still parses.

## 0.11.22 - the stop_request contract says which end of the last message survives

`docs/embedding.md` said `last` was "cut at 4.000 characters" and left the reader to guess
which end it kept. The obvious reading is the head, and the head is the wrong half.

The emitter measured it first: `parada.mjs` of SHVIA-DESKTOP cut the head until 1.5.4, and
the skeptical panel of 10/09/2026 (SHVIA-WEB #110) found that a closing summary over 4.000
characters flipped **every tier** of the host's rule to `continue` — the model had written
`NEEDS_INPUT:` at the end and the marker was the part thrown away. It now keeps the tail
(`texto.slice(-LAST_MAX)`), and this document now says so.

This matters here and not only there because this file is the **contract**, not one
implementation of it: a second engine written against the old wording would reproduce the
defect that was already measured and fixed, and nothing in the protocol would catch it.
The rule stated: the execution contract asks the model to close with its marker, the host
reads the closing paragraphs, so the head is the report and the tail is the decision.

No change to the event, the handshake, or any code. `anna` still never emits it.

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
