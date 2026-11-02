# User guide

This guide describes the source reviewed on 2026-10-15. Local checks do not replace a
live model/tool round trip or acceptance in a packaged host.

## Project context

Start `anna` in the project to inspect. It walks upward to the first `.git` entry, or
uses the current directory when none exists. The first existing root instruction file
in this order is selected: **ANNA.md → CLAUDE.md → AGENTS.md**. They are not merged;
up to 6,000 characters of the selected file enter the project prompt. Instructions
in nested directories are not discovered as a separate hierarchy.

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

Do not use process exit status alone as proof of a successful answer: some gateway
errors currently return through a successful Rust result. Hosts must handle `error`
and process closure in addition to `turn_done`; see the
[review](DOCUMENTATION-REVIEW-20261015.md).

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
| Profile | `--model` → `SHVIA_PROFILE` → `profile` → gateway's user default |
| Effort | `--effort` → nonempty `SHVIA_EFFORT` → `effort` → gateway default |
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

Writes show a numbered diff. Approvals use arrow keys/Enter, numbers, or letter
shortcuts; Esc and Ctrl-C refuse. Destructive commands default to refusal and cannot
receive session approval. `t` applies to a tool scope, such as `write_file` or `bash`,
until the process exits. Automatic shell commands can include builds and tests;
they run project code and can have side effects.

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
The loop compacts long history: past 150 messages it cuts down to 100, keeping the
session's first request and the latest one. Since 0.15.0, when the gateway reports the
model's context window, the history is also cut when it fills 70% of it (down to 50%), and
a request the provider refuses (usually a full context) is retried once with a harder cut.
Before any message leaves, old large tool outputs, and reads of files edited since, become a
one-line note saying what the tool returned (0.15.1); this is not an immutable full transcript. Since
0.13.8 the history also keeps the assistant's final answers (an empty one is left out), so a
resumed session and the next request carry what the assistant already said. On Unix, the store applies `0700` to its directory and `0600`
to files. Saves are best effort. There is no scheduled expiration or cloud sync.

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
not the local session store and does not retrieve memories into the agent prompt.
