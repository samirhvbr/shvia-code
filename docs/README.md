# Documentation index

Start with the [repository README](../README.md) and [user guide](USER-GUIDE.md).
This index separates current behavior from design records so that a roadmap does not
become an implementation claim.

| Document | Scope |
|---|---|
| [User guide](USER-GUIDE.md) | Commands, configuration, approval, project context, sessions and statistics |
| [Architecture](arquitetura-cli.md) | Current modules, execution modes, boundaries and remaining design work |
| [Tool catalog](ferramentas.md) | Tool schemas, shell classification, output limits and checkpoints |
| [Gateway protocol](protocolo.md) | Client request/SSE contract and the server-side contract it expects |
| [Embedding](embedding.md) | NDJSON transport, approvals, host execution, images and turn lifecycle |
| [Planning pipeline](orquestrador.md) | Terminal-only architect/executor/reviewer roles, artifacts and verification |
| [Statistics](stats.md) | Local dashboard, source data, aggregation and historical-data limitations |
| [Development and distribution](DEVELOPMENT.md) | Build, tests, version carriers, CI, release assets and acceptance boundaries |
| [Security](../SECURITY_GUIDELINES.md) | Implemented controls, threat model, server boundary and known limitations |
| [Review of 2026-10-15](DOCUMENTATION-REVIEW-20261015.md) | Source coverage, documentation corrections and code findings for a later decision, with their status on 2026-11-05 |
| [Decisions](decisoes.md) | Dated ADRs; preserve historical rationale and read current status with the architecture |

[ANNA.md](../ANNA.md) is the short project context the executable loads before
`CLAUDE.md`/`AGENTS.md`; [AGENTS.md](../AGENTS.md) and [CLAUDE.md](../CLAUDE.md) govern
repository work. The original scope and other files in [`.continue/`](../.continue/)
remain plans or historical queue context. They are not promoted to completed features
by this review. Older Portuguese documents remain available at their established paths;
new documentation and maintained sections use English (US).
