# ShvIA Code — `anna`

ShvIA Code is the Rust coding agent used from a terminal or embedded in a ShvIA host.
The executable is named **`anna`**; the coding agent has no Anna chat persona. It sends
conversation and tool definitions to the ShvIA gateway at `POST /api/v1/code/chat` and
executes approved tools on the machine running it, or delegates execution to its host.
The gateway selects the configured model and provider; this repository does not run a
model server.

**Current scope:** terminal REPL, one-shot prompts, file/search/edit/shell/Git tools,
task lists, local sessions and statistics, Git checkpoints, a terminal planning pipeline,
and NDJSON integration. [Current architecture](docs/arquitetura-cli.md),
[user guide](docs/USER-GUIDE.md), and [documentation index](docs/README.md) describe
the implemented behavior. The original C0–C5 scope in
[`.continue/escopo-projeto.md`](.continue/escopo-projeto.md) is a planning record, not a
current feature checklist.

This documentation review inspected source and local checks. It does not certify a live
gateway round trip, installed terminal experience, packaged Desktop integration, or
deployment. See [review evidence and open findings](docs/DOCUMENTATION-REVIEW-20261015.md).

## Install

On macOS or Linux, install Rust and `cargo-audit`, then:

```sh
git clone git@github.com:samirhvbr/shvia-code.git
cd shvia-code
./install.sh
```

The installer runs the strict dependency audit and a release build, then copies `anna`
to `~/.local/bin` or `ANNA_BIN_DIR`. Add that directory to `PATH` if needed. Missing
`cargo-audit` or a reported vulnerability stops installation. The script does not update
the checkout for you. [Build and distribution](docs/DEVELOPMENT.md) explains checks,
checksums, platform requirements, and how release publication differs from installation.

The Linux release workflow builds `anna-x86_64-unknown-linux-musl` and its `.sha256`
for x86_64 Linux. Download both from the same [release](https://github.com/samirhvbr/shvia-code/releases)
and verify with `sha256sum -c anna-x86_64-unknown-linux-musl.sha256` before installation.
A Windows workflow produces an `anna.exe` **Actions artifact**; that is a separate path
from GitHub Release assets. Shell tools require `sh`, including on Windows.

## Configure and use

Use a revocable ShvIA user API key, not Laravel's `APP_KEY`. Provide it through
`SHVIA_API_KEY`, or enter it through stdin with:

```sh
anna config set-key
anna config
```

`anna config` reports the resolved settings and configuration path without printing the
key. The configuration directory follows the operating system; `~/.config/shvia-code`
is the usual Linux path, not a portable path for every platform. The setter applies
`0600` on Unix; the [security guide](SECURITY_GUIDELINES.md) states its limits.

Run from the project you intend the tools to use:

```sh
anna
anna -p "Explain the failing calculation and cite the relevant files"
anna --model "model@server" --effort high
anna sessions list
anna sessions resume SESSION_ID
anna stats --tab models --range 30d
anna stats --html --open
```

The profile string must exist on your gateway. There is currently no `anna models` or
`/models` catalog browser. Flags, environment precedence, role configuration and all
commands are in the [user guide](docs/USER-GUIDE.md).

The REPL supports `/help`, `/model`, `/sessions`, `/stats`, `/tools`, `/clear`,
`/plano <request>`, `/rollback`, `/rollback aplicar`, and `/exit`. `/plano` runs an
architect → human approval → executor → verification/reviewer pipeline with separate
role profiles; it is available in the terminal, not as an NDJSON slash-command.

Writes display a diff and require approval. Shell commands are classified as automatic,
confirmation required, or destructive/always confirm. Session approval applies to a
tool scope. **This is an approval system, not an operating-system sandbox:** an approved
shell command runs with the account's permissions. One-shot terminal `-p` refuses
confirmation gates, while automatic tools still run. `--json -p` can ask its host for
approval and must not be treated as a read-only mode.

## Embed and integrate

`anna --json` exposes the same agent loop as newline-delimited JSON over stdin/stdout.
It keeps conversation in the running process, without the REPL's session autosave.
With `--json --tools host`, it proposes only `bash` calls and waits for the host's
execution result; the host owns approval and execution policy.
[Embedding](docs/embedding.md) documents events, decisions, images, and lifecycle.

ShvIA Desktop can package the engine, and ShvIA Workspace can install the static Linux
binary in a runner; in their current source both spawn `anna --json --tools local` with
the key in `SHVIA_API_KEY`. Their routing, installation, UI, and remote execution live in
their own repositories. Optional `AI_MEMORY_URL` enables a separate best-effort hook that
sends prompts and tool information to an ai-memory service; it is disabled when unset.

## Contribute

Work on a branch and propose a PR; do not publish directly to the default branch.
Follow [AGENTS.md](AGENTS.md), [CLAUDE.md](CLAUDE.md) and
[development guidance](docs/DEVELOPMENT.md). Documentation and GitHub contributions
use English (US). Keep `version.md`, the `anna` package in `Cargo.toml`/`Cargo.lock`,
and the first changelog heading aligned, including documentation-only version bumps.
Merging, publishing release assets, installing, and verifying a live gateway remain
separate steps.
