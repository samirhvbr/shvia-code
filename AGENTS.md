# SHVIA-CODE — Guia do Agente (AGENTS.md)

> **Leia também:** [README.md](README.md) ·
> [.continue/escopo-projeto.md](.continue/escopo-projeto.md) (escopo aprovado,
> fases C0–C5) · [docs/decisoes.md](docs/decisoes.md) (ADRs).
>
> `CLAUDE.md` e `AGENTS.md` são **espelhados** abaixo do H1 — editar os dois.

---

## 🔄 Antes de começar: `git pull`

**SEMPRE** verifique atualizações remotas antes de escrever ou alterar qualquer
coisa neste repositório:

```bash
git pull          # já está pré-autorizado (allow)
```

---

## O que é este repo

**CLI agêntico de código** da Blue3 — comando **`anna`** (Rust): a experiência
do Claude Code movida pelo **gateway do ShvIA** (`ai.shvia.org`). O loop
agêntico roda **no cliente** (tools executam na máquina do dev, com gates de
aprovação); o servidor só infere — auditoria/quotas/LGPD/custo herdados.
Irmão de `SHVIA-WEB` (servidor Laravel), `SHVIA-DESKTOP` e `SHVIA-MOBILE`.

**Escopo aprovado em 08/07/2026** — decisões travadas: Rust · comando `anna` ·
modelos livres (qualquer perfil do dev). Ver ADR-001.

---

## Padrão de Commits (obrigatório)

Formato: `versão - comentário em português`. A versão **sempre** vem de
`version.md` (bumpe no mesmo commit). Critério: **Z** = UX do CLI/ajuste de
build; **Y** = tool nova, mudança de protocolo com o gateway; **X** = estável.
Proibido `feat:`/`fix:`/`chore:` e mensagens vagas.

---

## Regras do produto (não relitigar sem ADR)

- **Loop no cliente**: tools executam localmente; servidor stateless de tools.
- **Gates**: leitura = auto; escrita = diff + confirmação; bash = allowlist /
  confirmação; destrutivo sempre confirma. Sem modo yolo no v1.
- **Nada roda/escreve sem o dev ver.**
- Endpoint do gateway: `POST /api/v1/code/chat` (SSE; tools/role=tool) — o
  trabalho server-side vive no repo `SHVIA-WEB`.
- Auth = API key do ShvIA. Segredos nunca versionados aqui.

---

## Stack & comandos

- **Rust** (binário único `anna`). `cargo check` · `cargo clippy` ·
  `cargo test` · build via `build-local.sh` (com preflight, padrão da casa).

---

## Referências rápidas

- Versão: `version.md` · Escopo/fases: [.continue/escopo-projeto.md](.continue/escopo-projeto.md)
- Decisões: [docs/decisoes.md](docs/decisoes.md)
- Servidor/gateway: `~/x/SHVIA/SHVIA-WEB` (`https://ai.shvia.org`)
- Padrão de aprovação de comandos: SShvTerm (aba Agente)

---

## PS — Commits: a skill COMMITTER cuida disso

**Existe `.committer.yml` na raiz deste repositório** — é o opt-in da skill
**COMMITTER**, que roda em ciclo (cron, via `~/x/GIT/run.sh`). Enquanto esse arquivo
existir com `enabled: true`, **commitar e pushar não é trabalho seu**.

**O que muda para você:**

- **Não commite nem pushe por padrão.** Conclua a entrega bumpando o `version.md`
  **com a entrada de changelog** e deixe a árvore pronta. É dali que a mensagem do
  commit sai — o changelog virou o artefato de handoff entre você e a skill.
- A skill monta `X.Y.Z - descrição`, commita e pusha a branch atual sozinha. Ela
  **nunca bumpa versão** (isso continua sendo julgamento seu) e nunca inventa
  mensagem: sem entrada de changelog ela cai num fallback Sonnet, e sem conseguir
  descrever com honestidade ela aborta e espera.

**Você ainda commita quando:**

- o Samir pedir explicitamente;
- a tarefa exigir o SHA na hora (deploy, abrir PR, referência cruzada);
- o `.committer.yml` sumir ou estiver `enabled: false` — aí vale o fluxo antigo,
  você bumpa, commita e pusha.

**Por que isso existe:** tirar de um modelo caro (Opus/Fable) o trabalho mecânico de
empacotar commit, que um Sonnet — ou, na maioria das vezes, nenhum modelo — resolve.
Economiza token e devolve tempo de desenvolvimento.

---

<!-- RELEASES-RULE:repodocs -->

## Releases — the `version.md` on GitHub is what the Releases show

> Marked echo. The single source is **[samirhvbr/repodocs](https://github.com/samirhvbr/repodocs/blob/master/docs/versioning.md)**
> — change it there, not here. This block is regenerated.

**The `version.md` of the default branch, on GitHub, is what the GitHub Releases
must show.** The local checkout does not enter the calculation: it can be behind,
ahead or mid-work, and none of that is published — GitHub cannot tag a commit it
does not have.

**The bump and the Release are one act.** A commit that bumps `version.md` is not
finished until that version has a tag, a published Release, and the **`Latest`
badge on it** — the same push, not "later". A badge sitting on an older release
tells whoever looks that the project is at a version it is not.

- `.github/workflows/release.yml` does it on any push that touches `version.md`.
- `./tools/release.sh` does it by hand. It is **idempotent and self-healing**:
  it publishes whatever is missing and moves a drifted badge back. Running it is
  always safe, so it is both the check and the fix.

A PR publishes nothing while it is a PR. The moment it merges, the push moves
`version.md` on the default branch and the Release becomes that version.

Tag and Release title are the **bare version — no `v` prefix**.

<!-- /RELEASES-RULE -->
