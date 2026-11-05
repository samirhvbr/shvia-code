# Development and distribution

## Repository workflow

Fetch/pull before changing documentation or code. Work in an isolated branch/worktree,
review the diff, and open a PR against `master`; no direct default-branch publication.
Use English (US) for maintained documentation, commits and GitHub contributions. Do not
rewrite generated rule blocks locally or delete unimplemented plans from `.continue/`.

The repository requires a versioned changelog even for documentation. Keep these aligned:

- `version.md`, the release authority;
- package `anna` in `Cargo.toml` and `Cargo.lock`;
- the first `CHANGELOG.md` heading and commit subject, `X.Y.Z - English description`.

`build.rs` compares version carriers. `cargo --locked` prevents Cargo from silently
rewriting the lockfile first. A documentation metadata bump does not change agent logic
and does not authorize merging or publishing its release.

Helpers in the repository:

- `bash scripts/sync-version.sh` writes the `version.md` number into `Cargo.toml` and
  `Cargo.lock`; `--verificar` only checks and exits 1 when a carrier diverges. It does not
  touch `CHANGELOG.md`.
- `tools/git-hooks/commit-msg` (subject `X.Y.Z - …` must carry the `version.md` version)
  and `tools/git-hooks/pre-push` (checks that version against the remote) are generated
  from repodocs. They run only when the clone points Git at them
  (`git config core.hooksPath tools/git-hooks`); `REPODOCS_NO_HOOK=1` skips them.
- `./tools/release.sh` tags and publishes the `version.md` version as a GitHub Release
  (`--dry-run`, `--backfill`); the Release workflow calls it after a merge.
- `scripts/dogfood.sh` runs a few `-p` prompts against a live gateway with a real
  `SHVIA_API_KEY`; it is a manual smoke test, not part of CI.

## Local build and checks

Rust/Cargo are required; macOS also needs Xcode Command Line Tools. Shell execution needs
`sh`, and Git tools/checkpoints need Git. A Windows executable still needs an available
`sh` for shell tools; the MSVC build workflow is not proof of installed shell acceptance.

```sh
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
cargo build --release --locked
```

The test suite includes filesystem guards, tool classification, shell timeouts/process
trees, checkpoints, rendering, configuration, statistics and repository documentation
guards. The architecture module inventory, the AGENTS/CLAUDE mirror and the SHA pin of
every workflow action are executable checks. The process-level tests in `tests/` run the
built binary against a scripted loopback gateway and are compiled only on Unix (they
isolate `HOME`). Passing these tests does not prove live model tool-calling or installed
UI behavior.

`./build-local.sh` runs a preflight, builds release by default and writes a SHA-256 next
to the executable. `--debug` selects a debug build. `--audit` runs `cargo audit` when
installed but skips it if absent; `--audit-strict` requires it and stops on findings.
The script currently calls `cargo build` without `--locked`, so use the explicit locked
commands above when validating a reproducible checkout. It does not itself run the test
suite or clippy. `./install.sh` uses strict audit and copies the result to the user's
binary directory; it modifies the installed executable and is not a read-only check.

## CI and published artifacts

| Workflow | Trigger and output |
|---|---|
| `test.yml` | PRs, default-branch pushes, manual dispatch; Linux locked tests, clippy and RustSec advisory check |
| `build-windows.yml` | Matching source/version/workflow pushes to master or dispatch; tests/clippy, MSVC release build and `anna-windows-x86_64` Actions artifact with checksum. Runs on GitHub-hosted `windows-latest`, not on `CI_RUNNER`, so it spends hosted minutes |
| `release.yml` | Default-branch push changing `version.md` or manual dispatch; calls `tools/release.sh` to publish the bare version tag/Release |
| `linux-binary.yml` | Successful completion of the Release workflow on master, or dispatch; builds static Linux x86_64 binary and attaches it and SHA-256 to the existing Release |

The Linux build runs in the pinned Docker Rust/Alpine image, checks static linkage and
`anna --version`, then uploads assets. Its completion is separate from creation of the
Release page. A Windows workflow artifact is not automatically a GitHub Release asset.
No macOS release-binary workflow is defined here; local source builds and Desktop's
packaging are different distribution paths.

## Acceptance boundary

Before claiming a delivery usable, identify the evidence actually obtained: source
review, local build/tests, PR CI, release page, matching assets/checksum/version,
installation, terminal approval behavior, and authenticated gateway/tool round trip.
Desktop embedding and Workspace runners require acceptance in those projects too.
This documentation review does not install a binary, publish a release or exercise
real credentials. Investigating code findings in
[the review](DOCUMENTATION-REVIEW-20261015.md) requires a separate implementation decision.
