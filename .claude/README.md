# Claude Code profile — ShvIA Code (`anna`)

This project's `.claude/` follows the Blue3/samirhvbr repo standard: permissions
posture and effort, and nothing else. Target stack: **a single Rust CLI (`anna`)
plus Bash build/install/release scripts** — the gateway is remote (ShvIA Web), so
**no database and no secret runs here**; the user's ShvIA key lives in the user's
local configuration, outside the repository.

> Until 2026-11-05 this file's title and stack line named ShvIA Desktop (Tauri 2, npm/Vite,
> a Python sidecar). The allow-list below dropped the entries for that stack (Tauri, npm,
> Node, Python, pytest, bats), which this repository has no use for.

## Files

| File | Role |
|------|------|
| `settings.json` | The **active** profile (versioned). Today = `effortLevel: xhigh`, `defaultMode: plan`, and the security **deny-list** only. It chooses **no model**. |
| `skills/loop-work/` | Vendored copy of the `loop-work` skill (its `VERSION.md` names the source and version). |
| `prompts/continuacao.md`, `prompts/reabastecimento.md` | The Stop-hook prompt templates that skill returns to resume the agent. |

> The **allow-list** (shortcuts that avoid repeated prompts) is **deliberately**
> absent from `settings.json`: granting the agent a permission is **your** act.
> Apply the block below by hand when you want fewer prompts.

## Recommended allow-list (paste into `permissions.allow`)

```jsonc
"allow": [
  "Read", "Edit", "Write",
  "Bash(git status:*)", "Bash(git diff:*)", "Bash(git log:*)",
  "Bash(git show:*)", "Bash(git branch:*)",
  "Bash(git add:*)", "Bash(git commit:*)", "Bash(git push:*)",
  "Bash(cargo check:*)", "Bash(cargo build:*)", "Bash(cargo test:*)",
  "Bash(cargo fmt:*)", "Bash(cargo clippy:*)",
  "Bash(shellcheck:*)"
]
```

## Rules worth remembering

- **The model is the user's choice, and this repository makes none of it**
  (repodocs ADR-027). You pick it with `/model`, per session, and a subagent
  inherits the session's model. `settings.json` carries no `model`, no
  `fallbackModel`, no `availableModels` and no model env var
  (`ANTHROPIC_MODEL`, `ANTHROPIC_DEFAULT_*_MODEL`,
  `CLAUDE_CODE_SUBAGENT_MODEL`). There are no stand-by profiles to copy over
  `settings.json` — `/model` is how the model changes. The context window is
  whatever the model you picked offers; the repository guarantees none.
- **Effort `max` goes through the env** (`CLAUDE_CODE_EFFORT_LEVEL=max`). The
  JSON `effortLevel` field only accepts `low/medium/high/xhigh` — `max` there is
  ignored.
- **`defaultMode: plan`** — the agent plans before acting. It keeps the habit of
  reviewing structural changes before touching code.

## Deny-list (already in `settings.json`)

Blocks reading `.env`/keys (`*.pem`/`*.key`/`*.p8`/`*.p12`/`*.pfx`), `rm -rf`,
`git push --force/-f`, `git reset --hard`, `git clean -fd` and
`curl|sh`/`wget|sh`. **Do not loosen** without a documented reason.
