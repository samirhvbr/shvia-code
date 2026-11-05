# User guide

This guide describes the source of 0.15.8, rechecked on 2026-11-05 (first written on
2026-10-15). Local checks do not replace a live model/tool round trip or acceptance in a
packaged host.

## Project context

Start `anna` in the project to inspect. It walks upward to the first `.git` entry, or
uses the current directory when none exists. In each folder from the root down to the
one `anna` runs in, the first existing instruction file in this order is selected:
**ANNA.md → CLAUDE.md → AGENTS.md** (0.15.5+; before, the root only). Up to 6,000
characters of each, and 9,000 in all, enter the project prompt, which is resent on every
round. A cut file says so, with its size, so the model can read the rest with `read_file`;
a file that is the same rules under another title (this fleet mirrors CLAUDE.md and
AGENTS.md below the H1) is loaded once; and the other instruction files found are listed
by name and size, not loaded. The first time a tool touches a subfolder whose nearest
instruction file is not in the prompt, its result gets one line pointing at that file and
its size (0.15.6+), once per folder per process; the file itself is read only if the model
asks for it.

Tools use the detected project root. Run session resume from the original project;
the stored session does not bind itself to a project directory. Git worktree roots are
recognized, but branch display currently reads `.git/HEAD` as a file below a directory
and may be absent for a worktree whose `.git` is a pointer file.

## Command line

| Command or option | Current behavior |
|---|---|
| `anna` | Interactive REPL with autosave after ordinary chat turns |
| `-p TEXT`, `--print TEXT` | One turn through the terminal frontend; no session autosave; confirmation gates refuse |
| `--url URL` | Gateway base URL; `/api/v1/code/chat` is appended |
| `--model PROFILE` | Exact gateway profile string, commonly `model@server` |
| `--effort LEVEL` | Passed to the gateway for validation; help lists low/medium/high/xhigh/max |
| `--json` | NDJSON frontend; persistent stdin-driven conversation without `-p` |
| `--tools local` | Default: execute tools in the `anna` process's machine/account |
| `--json --tools host` | Offer only `bash`; host approves, executes and returns results |
| `config` | Resolved URL/profile/effort/round limit/key presence/config path and role routing |
| `config set-key` | Read one key line from stdin and update the configuration file |
| `sessions list` / `sessions resume ID` | List local transcripts or resume into the REPL |
| `sessions clear [--older-than DAYS]` | Delete all session JSON files, or those older by modification time |
| `stats [--tab overview\|models] [--range all\|30d\|7d]` | Aggregate local session data |
| `stats --html [--open] [--out FILE]` | Write an HTML dashboard, optionally open it in a browser |

`--tools host` has effect only with `--json`; without JSON the implementation uses
local execution. `config set-key` also accepts a positional key, but stdin avoids
exposing a secret through shell history or process arguments. There is no runtime
`--api-key`, `--allow-writes`, `models`, `infras`, login or updater subcommand.

Do not use process exit status alone as proof of a successful answer: a gateway error
reported in the stream or before it (4xx/5xx, an SSE `error`, or a connection that never
opened, after its retries) ends `-p` with exit status 0; a missing key, an unsafe URL and a
connection that broke after the request was sent exit non-zero. Hosts must handle `error` and process closure in
addition to `turn_done`; see the [review status](DOCUMENTATION-REVIEW-20261015.md#status-on-2026-11-05).

## Configuration

`anna config` prints the file path resolved by `directories::BaseDirs`. Typical defaults
are `$XDG_CONFIG_HOME/shvia-code/config.toml` (or `~/.config/shvia-code/config.toml`)
on Linux, `~/Library/Application Support/shvia-code/config.toml` on macOS, and the
user's roaming AppData configuration directory on Windows. A keychain is not implemented.

This example contains no API key. Replace illustrative profiles with profiles on your
gateway:

```toml
url = "https://ai.shvia.org"
profile = "model@server"
effort = "high"
max_voltas = 100
bash_timeout = 120
verificacao = ["cargo clippy --all-targets -- -D warnings", "cargo test"]

[elenco.arquiteto]
profile = "architect-model@server"
effort = "xhigh"

[elenco.executor]
profile = "executor-model@server"
effort = "high"

[elenco.revisor]
profile = "reviewer-model@server"
effort = "high"
```

| Setting | Precedence and default |
|---|---|
| URL | `--url` → `SHVIA_URL` → `url` → `https://ai.shvia.org` |
| Profile | `--model` → `SHVIA_PROFILE` → `profile` → gateway default (the active Code project's pin, then the user's default; see [protocol](protocolo.md#2-corpo-da-requisição-json)) |
| Effort | `--effort` → nonempty `SHVIA_EFFORT` → `effort` → gateway default (the active Code project's effort pin, then the server's) |
| API key | Nonempty `SHVIA_API_KEY` → `api_key` in the configuration file |
| Round limit | Parseable `SHVIA_MAX_VOLTAS` → `max_voltas` → 100; minimum 1 |
| Shell timeout | Parseable `SHVIA_BASH_TIMEOUT` → `bash_timeout` → 120 seconds; 0 disables it |
| Roles and verification | `[elenco.*]` and `verificacao` from the file |

A role profile overrides the global profile. Role effort overrides explicit global
effort, then the role default applies. `anna config` displays this routing; it does
not fetch the gateway catalog or prove that a profile is routable.

The key setter preserves parsed settings, writes a temporary file and renames it into
place. On Unix it applies `0600` before rename and warns when a loaded key file is
readable by group/others. It currently writes the temporary contents before applying
that mode: use a restrictive `umask` and see the
[security limits](../SECURITY_GUIDELINES.md). POSIX modes are not a Windows ACL guarantee.

## REPL and approval

`/help` lists commands. `/model` shows the current profile; `/model PROFILE` changes it
for subsequent calls. `/tools` lists the registry. `/clear` starts a new conversation
and session; it does not delete saved sessions or reset frontend approvals.
`/exit`, `/quit`, `/q`, Ctrl-C at the prompt or EOF exit the REPL.

**Ctrl-C during a turn stops the turn, not `anna`** (0.14.0+). The answer being streamed is
cut (what was shown stays in the history, marked as interrupted), a running shell command
is killed with its whole process tree, and tool calls not yet run are answered as refused.
Then the prompt comes back with the session intact. A second Ctrl-C in the same turn ends
`anna` (exit 130), the way out of anything that does not stop.

**A model that repeats itself is told, then stopped** (0.15.3+). When the same tool call, with
the same arguments, returns the same result three times in a row (or the same cycle of up to four
calls repeats three times), a note goes to the model with that result and a warning to you. At
five, the REPL asks *"O modelo repetiu … — deixar continuar?"*: yes resets the count, no ends
the turn with the model's summary of what it tried. A different result counts as progress.
At the round ceiling the turn also ends with such a summary (0.15.2+).

Writes show a numbered diff. Approvals use arrow keys/Enter, numbers, or letter
shortcuts; Esc and Ctrl-C refuse. Destructive commands default to refusal and cannot
receive session approval. `t` applies to a tool scope, such as `write_file` or `bash`,
until the process exits. Automatic shell commands can include builds and tests;
they run project code and can have side effects.

Two write rules act before or above the approval card ([tools](ferramentas.md)):
`write_file` over an existing file is refused unless the model has read the whole file in
this process and it has not changed since (0.15.7+; `edit_file` is not affected), and a
write to `ANNA.md`, `AGENTS.md`, `CLAUDE.md` or `CLAUDE.local.md` always asks, even after
`t` (0.15.4+).

Output is a scrolling `rustyline` REPL with Markdown, tool summaries, a spinner and
usage footer, not a full-screen editor. TTY detection controls color; `NO_COLOR`
disables it and `FORCE_COLOR=1` can force it. Non-TTY prompts use line input instead
of the menu. See [tools and gates](ferramentas.md).

`/plano REQUEST` runs the [planning pipeline](orquestrador.md). The architect and
reviewer receive only read tools and `todo_write`; the executor receives the normal
registry. Plan approval does not remove tool gates. Verification commands outside the
automatic shell allowlist are skipped with a warning; a review verdict does not prove
that every desired test executed.

## Sessions, checkpoints and statistics

Normal REPL turns autosave under `~/.shvia-code/sessions/`, after every round that ran
tools and when the turn ends (0.14.0+; before, only when the turn ended). JSON contains the message
history retained by the loop, first-prompt title, usage totals and model metadata.
The loop compacts long history, keeping the session's first request and the latest one.
Without limits from the gateway it cuts past 150 messages down to 100. Since 0.15.0, when
the gateway reports the model's context window, the history is cut when its estimate fills
70% of that window (down to 50%) and the message count follows the gateway's `messages_max`;
the current ShvIA Web source announces 5,000 messages and caps the window at 128k tokens by
default. A request the provider refuses (usually a full context) is retried once with a
harder cut.
Before any message leaves, old large tool outputs, and reads of files edited since, become a
one-line note saying what the tool returned (0.15.1); this is not an immutable full transcript. Since
0.13.8 the history also keeps the assistant's final answers (an empty one is left out), so a
resumed session and the next request carry what the assistant already said. On Unix, the store applies `0700` to its directory and `0600`
to files. Saves are best effort. There is no scheduled expiration or cloud sync.

A session's ID is its creation time in whole seconds. `sessions resume ID` restores the
saved messages, including the system prompt and project instructions as they were when the
session started; it does not rebuild them for the current folder. Usage counters start at
zero in the resumed process, and its next autosave replaces the session's saved token and
cost totals with that process's count (`main.rs`, `salvar_sessao`).

`sessions clear --older-than 30` removes files last modified over 30 days ago. Omitting
the option removes all session JSON files; `--older-than 0` can also remove all existing
files. These commands do not prompt. They do not remove checkpoints, HTML exports or
records already sent to a memory service.

`/rollback` shows changes from the latest best-effort Git checkpoint. Only
`/rollback aplicar` requests restoration. A snapshot is attempted before approved file
writes or classified destructive shell commands, with a 30-second debounce. It uses a
temporary index and `refs/anna/checkpoint`, separate from the current branch. It is not
a transaction or a snapshot for every operation. Files created after the snapshot
remain; the legacy `git checkout` restore fallback can also change the index.

`anna stats` and `/stats [models] [7d|30d]` read local sessions only. `/stats html` exports
HTML. They do not include NDJSON conversations, one-shot runs or all pipeline activity
unless a host separately saves equivalent data. Unknown old usage values remain zero;
session model metadata is not a per-message provider ledger.

## Headless hosts and optional memory

See [embedding](embedding.md) for NDJSON examples. A persistent process accepts
`{"type":"user","text":"...","images":["BASE64"]}`. Images are optional and cleared
from earlier turns when a new user turn starts. There is no image-file CLI flag.
The host must answer local `gate_request` messages or return `tool_result` in host
execution mode; an unanswered request can wait for stdin indefinitely.

The round ceiling ends a headless turn with a warning; a persistent host can submit
another user turn to continue. Terminal one-shot refuses gated writes, but JSON
one-shot still uses host decisions. JSON conversation ends with the process unless
the host persists it.

`AI_MEMORY_URL` enables best-effort `/hook` events with prompts, full tool arguments and
up to 400 characters of each tool result, plus session boundaries. `AI_MEMORY_TOKEN`
optionally supplies a bearer token. HTTPS is required except for loopback HTTP; an unsafe
memory URL disables the integration with a warning. Requests have a 1.5-second timeout,
without retry or failure log. This exports working content to a separate service; it is
not the local session store, and `anna` does not retrieve memories into its prompt. The
gateway can do that on its side: the current ShvIA Web source prefixes a new turn with the
person's own ai-memory block for on-premises targets (cloud only with the operator's
opt-in), whether or not `AI_MEMORY_URL` is set here ([protocol](protocolo.md) §2).
