# Architecture of the `anna` CLI

> **Current source map, reviewed 2026-10-15.** Source inspection and local checks do
> not certify live gateway behavior or installed Desktop/Workspace integration.
> [User guide](USER-GUIDE.md) · [tools](ferramentas.md) · [embedding](embedding.md) ·
> [development and distribution](DEVELOPMENT.md) · [security](../SECURITY_GUIDELINES.md).

## 1. Runtime boundaries

The Rust executable owns project detection, conversation, the agent loop and its frontend.
`api.rs` sends messages/tool definitions to the ShvIA gateway and consumes SSE. In local
mode `tools.rs` executes under the user's account; in host mode `runner.rs` delegates
only shell proposals to an external executor. Provider routing, quota and server policy
belong to ShvIA Web. This CLI does not implement an OS sandbox or a model catalog browser.

The terminal is a scrolling `rustyline` REPL with crossterm menus, Markdown, diff previews
and a spinner. A full-screen ratatui UI was considered but is not a dependency or runtime.
The same loop uses a `Frontend` trait for terminal output and NDJSON hosts.

## 2. Module inventory

The modules are flat files under `src/`. A previous directory-based design was
reconciled in September 2026; `arvore_da_doc_bate_com_src` now checks this inventory
against the real files. Responsibilities, rather than mutable line counts, belong here.

```
src/
  main.rs        Arguments, REPL, one-shot/JSON entrypoints and config/session/stats commands.
  agent.rs       Bounded agent loop, history compaction, tool results and usage handling.
  api.rs         Gateway HTTP request and SSE framing/event aggregation.
  types.rs       Request, message, tool and event wire types.
  tools.rs       Eleven-tool registry, file/search/edit/shell/Git/task execution and classification.
  guard.rs       Requested-name denylist and canonical project-path helpers.
  runner.rs      Local/host runners and the read-only role decorator.
  frontend.rs    Frontend trait for rendering and approval decisions.
  gate.rs        Auto/Confirm/Always policy, previews and terminal confirmation helpers.
  ui.rs          Terminal frontend, previews, tool summaries and interactive decisions.
  json.rs        NDJSON events and stdin approval decisions for external hosts.
  config.rs      OS config path, overrides, role routing, URL guard and key setter.
  project.rs     Root/context detection and agent/project system prompts.
  session.rs     Local JSON session save/load/list and manual age-based deletion.
  checkpoint.rs  Best-effort Git snapshots through a temporary index and explicit restore.
  memory.rs      Optional best-effort ai-memory hook export with URL guard.
  orquestrador.rs Terminal architect/executor/reviewer pipeline and verification commands.
  papel.rs       Role identities/default effort and role configuration types.
  plano.rs       Plan/verdict artifact parsing and validation.
  term.rs        Terminal capabilities, color, width, ANSI handling and spinner.
  menu.rs        Arrow/shortcut approval-menu state and terminal interaction.
  view.rs        Pure welcome, action, result-summary and footer formatting.
  render.rs      Streaming Markdown state, tables/lists and line-based code highlighting.
  diff.rs        Terminal numbered/color diff and plain unified host preview.
  telemetry.rs   Token/cost accumulation from gateway usage.
  stats.rs       Pure session aggregation, dates, ranges, models and heatmap data.
  stats_term.rs  Terminal statistics rendering.
  stats_html.rs  Escaped standalone HTML statistics rendering.
```

There is no separate `cli.rs`, `sse.rs` or `error.rs`; those responsibilities live in
`main.rs`, `api.rs` and `anyhow` at call sites. See [security](../SECURITY_GUIDELINES.md)
for the implemented controls and remaining gaps.

## 3. Dependencies actually used

| Dependency | Responsibility |
|---|---|
| `tokio`, `futures-util` | Async runtime and HTTP stream consumption |
| `reqwest` with rustls | Gateway and optional memory-hook HTTPS; no system OpenSSL requirement |
| `serde`, `serde_json`, `toml` | Wire, session and configuration serialization |
| `clap`, `rustyline`, `crossterm`, `unicode-width` | Arguments, line editing, menus and terminal rendering |
| `ignore`, `regex`, `globset`, `similar` | Walking/searching files and diff previews |
| `directories`, `chrono`, `anyhow` | OS paths, local date aggregation and errors |
| Windows-only `windows-sys` | Shell timeout process-tree Job Objects |

There is no eventsource-stream, ratatui or thiserror dependency. SSE framing is in
`api.rs`. The authoritative versions/features are in [Cargo.toml](../Cargo.toml).

## 4. Agent turn

1. Add the user input; clear prior-turn images when a JSON host submits a new turn.
2. Compact long message history: past 150 messages, cut down to 100, preserving the
   initial system context and the session's first and latest requests, and avoiding
   orphan tool results.
3. Call `/api/v1/code/chat` with the current profile/effort and runner's tool definitions.
4. Stream frontend text/tool events, collect usage, and report gateway errors.
5. Append the assistant response to the history once, including a final answer without
   tool calls (0.13.8+; an empty final answer is left out, since some providers reject an
   empty assistant turn). With tool calls, run them through the selected runner, append
   the tool results, and repeat. With a final answer, end the turn. The next request
   therefore carries what the assistant already said, and terminal autosave/resume keep
   it; `tests/assistant_history.rs` checks this against a local SSE fixture.
6. At `max_voltas` (100 by default), the interactive terminal may grant another batch;
   JSON hosts end the turn with a warning and can submit a subsequent user turn.

The runner decorator `SomenteLeitura` restricts architect/reviewer tools on both
advertisement and execution. It offers filesystem/Git reads and `todo_write`, without
shell, writes or commits. The pipeline passes plan/verdict artifacts between separate
role histories. It is sequential and terminal-only, not a parallel-agent scheduler.

## 5. Configuration and state

The [user guide](USER-GUIDE.md#configuration) documents OS-specific paths, all settings,
precedence and illustrative role profiles. The key can come from environment or config;
`set-key` supports stdin and a positional argument, so use stdin for secret hygiene.
The setter applies Unix `0600` before rename but currently writes the temporary file
before chmod; Windows ACL hardening and keychain integration are not implemented here.

The REPL autosaves retained message history to `~/.shvia-code/sessions/`, with best-effort
Unix directory/file permissions. This is not a complete immutable transcript or a
project-bound session registry. One-shot and NDJSON paths do not use this autosave.
Retention is manual (`sessions clear [--older-than DAYS]`), based on file modification
time. Statistics read this local store and cannot represent every external host run.

Checkpoints use a temporary Git index and `refs/anna/checkpoint`, with a 30-second
snapshot debounce; restoring is an explicit REPL action. New post-snapshot files remain,
and the legacy checkout fallback may affect the real index. Checkpoints do not cover
all shell effects or replace backups.

Optional `AI_MEMORY_URL` exports best-effort prompt/tool/session hooks to a separate
service. HTTPS is required except on loopback. The hook sends full tool arguments and
truncated results; it is not retrieval or synchronization of the local session files.

## 6. Execution modes

| Mode | Tools/approval | Conversation lifetime |
|---|---|---|
| Terminal REPL | Local tools with terminal gates | In memory, ordinary-turn local autosave |
| Terminal `-p` | Automatic tools run; confirmation gates refuse | One process, no autosave |
| `--json` / `--json -p` | Local tools; host answers NDJSON gates, including writes | Process memory; host must persist if desired |
| `--json --tools host` | Only shell proposals; host owns execution/security | Process memory; host owns persistence |

There is no `--allow-writes`. `--tools host` alone does not switch execution to a host.
JSON mode does not interpret terminal slash-commands. See [embedding](embedding.md)
for the protocol, stdin waiting behavior and error/completion limitations.

## 7. Roadmap versus implementation

C1–C3's reads, gated edits/shell/Git, session resume, model switching and rendering are
present. Distribution includes source install, Windows Actions artifacts and a static
Linux Release asset workflow. NDJSON embedding, host delegation, task lists and the
planning pipeline also exist; they must not all remain labeled C5 future work.

**Proposed, not implemented:** `anna models`/`anna infras` and matching slash-commands,
favorites/catalog browsing, ShvIA skills/RAG integration, MCP tools, structured test
execution, OS sandboxing, parallel agents and NDJSON planning-pipeline gates. The earlier
catalog proposal used Web endpoints; their current availability/authorization needs
verification in ShvIA Web before implementing a client. Preserve the original scope in
[.continue](../.continue/escopo-projeto.md) as a design record.

## 8. Verification

`cargo test --locked` exercises the Rust tests, including the module inventory in this
section and the AGENTS/CLAUDE mirror. It does not contain a full mock-gateway closed-loop
integration suite. A passing local build is not proof of provider tool-calling quality,
installed terminal acceptance or packaged-host behavior. See the
[review](DOCUMENTATION-REVIEW-20261015.md) for source findings and checks obtained.
