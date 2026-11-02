# ShvIA Code security guidelines

> Reviewed against the CLI source on 2026-10-15. This document separates implemented
> controls from required server behavior and known gaps. It does not certify live
> gateway policy, Windows ACLs, a packaged host, or an operating-system sandbox.

## 1. Threat model

The agent reads project content, sends conversation/tool results to a model gateway,
and can execute local code. Relevant threats include prompt injection through repository
files, disclosure of source/secrets, destructive commands, exposed credentials or saved
sessions, excessive requests/cost, unsafe dependencies and an impostor gateway.

## 2. Client controls and limits

1. **Approval (A1, A3).** File writes show a diff and require confirmation. Shell commands
   use an automatic allowlist, confirmation for other commands, and always-confirm for
   classified destructive commands. Session approval is per tool scope. Terminal `-p`
   refuses confirmation gates, but automatic builds/tests can run project code and have
   side effects. JSON one-shot can still request/receive host approval. There is no yolo
   flag. These are classification/approval controls, not a sandbox or a guarantee that
   every destructive operation can be recognized.
2. **Untrusted content (A1).** Tool output is conversation data, not permission to relax
   gates. Architectural/reviewer roles filter write/shell tools in code. Models can still
   follow malicious instructions in files; inspect proposed changes and commands.
3. **File paths and secrets (A2).** `read_file`, `grep`, `edit_file` and `write_file` check
   the requested path **and** the real file after every symlink against `guard::denied`;
   canonical path helpers constrain filesystem tools to the project, and a write to a name
   that already exists follows its links and must still land inside. Until 0.13.9 the `grep`
   tool did not apply the list at all, and a symlink inside the project reached denied files
   (external review, 26/10/2026). The denylist includes `.env`/`.env.*` except
   `.env.example`, private-key extensions/names, credential paths and `.git`. There is no
   interactive override in the file tools. **Coverage is still incomplete:** `list_dir` and
   `glob` show the names (not the contents) of denied files; recursive search in `bash`
   (`grep -r X .`, `rg X`) stays automatic and can print a line from a denied file; and an
   unquoted glob in a content-printing `bash` command asks, because the shell expands it
   after the argument check. Do not treat ignore rules or filename filters as secret containment.
   An approved shell command has the user's filesystem/network access.
4. **API key (A4).** Runtime authentication comes from nonempty `SHVIA_API_KEY` or
   `api_key` in the OS configuration directory; `anna config` reports the path and key
   presence without printing the key. Use `anna config set-key` through stdin. Its
   positional key form exists but exposes the value to arguments/history. On Unix the
   setter applies `0600` to a temporary file before rename; it currently writes content
   before applying that mode, so a restrictive `umask` matters. Loaded config containing
   a key triggers a warning if group/other bits are set. There is no keychain or Windows
   ACL enforcement. Never put real keys in project examples, logs or reports.
5. **Local state (A5).** The REPL stores retained messages/tool results in
   `~/.shvia-code/sessions/`, applying directory/file modes `0700`/`0600` on Unix as
   best effort. Saves are not atomic and permissions are applied after file writes.
   Clear/age-based deletion is manual and has no confirmation. Headless/one-shot runs
   do not use the REPL autosave. Local storage does not mean conversation stays local:
   history and tool results are sent to the gateway for inference. Checkpoints and HTML
   exports are separate stores; Git checkpoints may include unignored local files.
6. **Transport (A8).** Gateway URLs require HTTPS except actual loopback HTTP for local
   development. URL comes from `--url`, `SHVIA_URL`, config or the compiled default.
   TLS uses rustls with normal certificate validation. This is not a provider-locality
   policy: selecting a cloud profile sends working content beyond the local machine.
   Optional memory hooks have a separate HTTPS/loopback guard and can export prompts,
   full tool arguments and result summaries to another configured service.
7. **Runtime limits (A6).** `max_voltas`/`SHVIA_MAX_VOLTAS` defaults to 100 model calls
   per turn, minimum 1. The interactive terminal can grant another batch; JSON ends the
   turn with a warning. `bash_timeout`/`SHVIA_BASH_TIMEOUT` defaults to 120 seconds,
   with process-tree termination; 0 disables it. It applies to the shell tool, not every
   subprocess (pipeline verification uses another path). File/search/output limits vary
   by tool and are not a global memory ceiling. HTTP streams and stdin approval/result
   waits have no explicit overall application timeout; since 0.14.0 the person can stop
   them (terminal Ctrl-C, host `cancel`), which also kills a running shell command's
   process tree, even with `bash_timeout = 0`.

## 3. Gateway and external-host boundary

`api.rs` sends a ShvIA user API key in `X-API-Key`, messages, tool definitions and optional
profile/effort. Provider inference, account access, rate limits, quotas, outbound PII
handling, data locality and inference audit are implemented/operated in **ShvIA Web**.
They must be checked there; client compilation cannot certify them. The CLI itself does
not mask arbitrary tool results before sending them, nor guarantee that all code secrets
are PII covered by a gateway masker.

`--json --tools host` delegates shell execution and approval to its host; the CLI's local
file guards are not a remote-host policy. Local NDJSON approval currently consumes the
next decision without enforcing its correlation ID, so hosts must serialize pending
requests. EOF refuses a local gate, but a host that leaves stdin open can leave it waiting.
See [embedding](docs/embedding.md) and the [review](docs/DOCUMENTATION-REVIEW-20261015.md).

## 4. Supply chain and build (A7)

Commit `Cargo.lock`; validate with `cargo test --locked`,
`cargo clippy --all-targets --locked -- -D warnings`, and locked release builds.
CI checks RustSec advisories. The installer requires `cargo-audit` and stops on a finding;
plain `build-local.sh` does not automatically run that audit. SHA-256 files verify
download integrity, not code signing/notarization or the safety of the executed project.
See [development and distribution](docs/DEVELOPMENT.md).

## 5. Data handling

Choose the gateway/profile appropriate to the source and data being processed. An
on-prem profile changes inference locality; it does not disable optional memory hooks,
filesystem persistence, external host logging, or network access by approved shell tools.
Session files, exported dashboards, checkpoint objects and service-side records have
different retention. Removing one does not remove the others. Do not include real
secrets or raw sensitive transcripts in reports or test fixtures.

## 6. Release acceptance

Validate guards, approvals, loop and shell limits, dependency checks, version carriers
and asset checksums. Then obtain installed terminal/host and authenticated gateway
evidence for the target platform. State which checks actually ran. Existing code findings
remain open until a separate implementation PR and appropriate regression tests address
them; documentation alone does not close those findings.

## 7. Vulnerability reporting

Do not publish sensitive reproductions publicly. Report privately to the repository
owner with affected version, safe reproduction and impact. Use synthetic data and keep
credentials out of issue/PR bodies. Source observations and decisions still requiring
reproduction are recorded in the private repository's
[documentation review](docs/DOCUMENTATION-REVIEW-20261015.md).
