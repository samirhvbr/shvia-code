# Documentation review — 2026-10-15

## Scope and evidence

This review began from `origin/master` at `b872d8cac1797f6352c488dae44c919e69f056b1`
(0.13.3), in a separate documentation branch/worktree. It inspected all root guidance,
the existing `docs/` guides, the source modules listed in the architecture, configuration,
install/build/release scripts and GitHub workflow definitions. It changes documentation
and the version metadata required by this repository, not Rust/shell/workflow logic.

Implemented means visible in source, not certified against a live provider. Original
scope/queue documents and ADRs remain dated records. The current map is in
[architecture](arquitetura-cli.md), commands/configuration in the
[user guide](USER-GUIDE.md), and distribution in [development](DEVELOPMENT.md).

## Documentation improvements

| Previous gap or contradiction | Correction |
|---|---|
| README still described early C1–C3 versions and an old test count | Replaced it with current surfaces and explicit evidence boundaries |
| No navigable user/configuration/distribution reference | Added index, user guide and build/CI/release guide |
| Architecture still listed proposed crates/flags and durable autonomous resume | Matched actual dependencies, modes and sequential pipeline; removed line-count inventory rot |
| Runtime config path treated Linux's path as universal | Documented OS resolution and `anna config` as the authoritative path |
| `-p` approval/persistence conflated with JSON one-shot | Separated terminal refusal, NDJSON host gates and absence of autosave |
| Uniform denylist/output limits and complete restoration were implied | Documented actual per-tool boundaries and checkpoint fallback effects |
| Local transcripts described as never going to a server | Explained inference-history transmission, separate local files and optional memory exports |
| ANNA.md preferred obsolete Portuguese/direct-master publication instructions | Aligned the short loaded context with current English/PR workflow |
| AGENTS version notes said no build check existed | Described build.rs/locked checks and included Cargo.lock, preserving generated fleet blocks |
| Historical profiles, ADR-002 proposal and C5 labels looked like current state | Added dated boundary notes; host runner/NDJSON are implemented, catalog browsing remains proposed |

## Coverage and remaining verification

The maintained material covers terminal commands/rendering, all eleven tools, gates,
model/profile routing, project context selection, sessions/retention/statistics,
checkpoints, terminal planning roles, NDJSON/local/host execution, images, memory hooks,
build/install, platform requirements and CI/release assets.

Local validation completed in the documentation worktree:

- `cargo test --locked`: 181 passed, including module inventory and AGENTS/CLAUDE mirror.
- `cargo clippy --all-targets --locked -- -D warnings`: passed.
- `cargo build --locked`: passed; the built executable reports `anna 0.13.4`.
- Root, stats and session-clear help matched the documented flags without contacting a gateway.
- 98 local Markdown file targets: no missing targets; version carriers and `git diff --check` passed.

These are local results, not PR CI or installed acceptance. This review does **not** establish:

- An authenticated gateway round trip or current server quota/masking/locality behavior.
- Installed interactive-menu/terminal behavior, macOS distribution signing, or Windows
  shell/ACL acceptance.
- Successful Desktop/SShvTerm/Workspace packaging, installation or remote tool execution.
- Release publication or asset availability for this proposed documentation version.

There is no full closed-loop mock-gateway integration suite in this source. Existing
unit tests and source review leave that runtime boundary open. `.continue/` retains
unimplemented specifications; it was not emptied or promoted as completed work.

## Code findings for a separate decision

The following observations come from source inspection. No real credentials, user
session stores or production gateway were exercised to reproduce them, and no logic
fix is included. Use isolated directories, synthetic content and a mock gateway before
choosing an implementation.

| Candidate | Source evidence | Suggested verification/decision |
|---|---|---|
| Secret filtering differs across tools/canonical targets | `src/tools.rs` file reads check requested names; grep/glob walks do not call `guard::denied`; `src/guard.rs` canonicalization is a separate step | Test ignored/visible synthetic secret fixtures and internal symlink aliases, then define a uniform canonical-target and search policy |
| Sensitive temporary/session files are written before permission hardening | `src/config.rs::gravar_api_key` writes `.toml.tmp` before chmod; `src/session.rs::save` writes before applying file mode | Test a permissive umask and failure/parallel-write paths; decide atomic private creation and cleanup/ACL behavior |
| Commit refusal can leave files staged | `src/tools.rs::git_commit` runs optional `git add -A` before the confirmation gate | Test refusal with an initially clean index and unrelated changes; decide whether staging must be separately approved/deferred |
| Error status/completion can mislead scripts or leave hosts waiting | `src/agent.rs::run_turn` reports a gateway error and returns `Ok` before `turn_end`; `src/main.rs::run_json_turn` reports propagated errors without completion | Mock HTTP/SSE failures and assert CLI exit status plus exactly defined NDJSON terminal events |
| Session resume is not bound to a project | `src/session.rs::Session` has no project root; `src/main.rs` restores stored messages while tools use the current detected root | Resume a synthetic session from another repository; decide validation and how to refresh old system context |
| Session fidelity/identity and resumed usage need review | `Session::new` uses epoch seconds; the final-answer path returns without appending that assistant answer; resumed telemetry starts separately from saved totals | Test two sessions created in one second, multi-turn/resumed transcripts and usage totals before changing the storage contract |
| Pipeline verification can skip checks or wait indefinitely | `src/orquestrador.rs::falha_verificacao` skips non-allowlisted/spawn-error commands and calls `Command::output`, outside the shell timeout executor | Test unavailable, disallowed and hanging configured checks; decide fail/skip semantics and timeout reuse |
| NDJSON local approvals do not enforce request ID | `src/json.rs::approve` reads `decision` without comparing the returned `id`; pending stdin waits have no timeout | Test stale/out-of-order decisions and host shutdown; decide strict correlation/lifecycle compatibility |

**Qualifications from independent review:** grep still respects its walker's default
ignore/hidden rules, while glob returns names rather than file contents; those observations
do not themselves establish a project-root escape. The key setter's temporary-file write
precedes chmod and has no exclusive/no-follow creation, but session files sit in a parent
directory already assigned `0700`; their atomicity/permission behavior must not be equated
with the same exposure. For terminal `-p`, reported HTTP/SSE errors can return success,
whereas transport errors propagate as Rust errors. JSON catches both categories; missing
`turn_done` is an affected-path lifecycle risk, not proof that every host always hangs.

Smaller source limitations already documented include missing branch labels in linked
Git worktrees, permissive `--tools host` without JSON falling back to local mode, and the
legacy checkpoint restore fallback affecting the index. Catalog commands, keychain,
MCP, sandboxing and NDJSON planning are proposals, not implemented features.
