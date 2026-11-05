# ANNA.md — project context for the coding agent

This is the root instruction file selected by `anna` before CLAUDE.md/AGENTS.md.
Keep it short: up to 6,000 characters of each selected file, and 9,000 in all, become
project context, resent on every round. Follow [AGENTS.md](AGENTS.md) for full
repository rules.

## Product

This repository builds `anna`, the Rust ShvIA coding-agent engine. The executable name
does not assign the model an Anna chat persona. It calls `POST /api/v1/code/chat` for
inference; tools execute locally or are proposed to an NDJSON host. Terminal `/plano`
runs sequential architect/executor/reviewer roles with human plan approval.

## Work rules

- Fetch/pull before changes; work on a branch/worktree and propose a PR. Never push
  work directly to master/main. Do not merge, release, install or deploy without scope.
- Write documentation, code comments, commits and PRs in English (US); preserve dated
  historical text and generated rule blocks rather than silently rewriting their history.
- Keep secrets out of examples and command arguments. Read the actual controls and gaps
  in [SECURITY_GUIDELINES.md](SECURITY_GUIDELINES.md); gates are not a sandbox.
- Make the smallest relevant change and inspect source before claiming behavior.
  Report unrelated code findings for a separate decision.
- Run `cargo test --locked` and `cargo clippy --all-targets --locked -- -D warnings`.
  Source/build checks do not prove installed UI or a live gateway round trip.

## Versioning

Commit subject: `X.Y.Z - English description`; no Conventional Commits prefix.
Write the matching CHANGELOG entry and align `version.md`, `Cargo.toml` and package
`anna` in `Cargo.lock`, including documentation-only bumps. Z covers CLI/build/docs;
Y covers a new tool or gateway protocol change; X is a stable milestone.
Commit and push only the intended paths, then open a PR for review.

## Source map

- `main`: clap options, terminal REPL, one-shot/JSON modes, config/sessions/stats commands.
- `agent` / `api` / `types`: bounded conversation loop, SSE transport and wire types.
- `tools` / `guard` / `runner`: registry, path helpers, local/host/read-only execution.
- `gate` / `frontend` / `ui` / `json`: approval policy and terminal/NDJSON frontends.
- `orquestrador` / `papel` / `plano`: role pipeline, configuration and artifacts.
- `config` / `project` / `session` / `checkpoint` / `memory`: settings, context and state.
- `term` / `menu` / `view` / `render` / `diff`: terminal capabilities and rendering.
- `stats` / `stats_term` / `stats_html` / `telemetry`: local usage views and accounting.

Start with [README.md](README.md), [documentation index](docs/README.md) and
[development](docs/DEVELOPMENT.md). Original C0–C5 material in `.continue/` is a planning
record, not the current acceptance report; do not delete unimplemented plans as cleanup.
