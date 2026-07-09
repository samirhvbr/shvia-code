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
do Claude Code movida pelo **gateway do ShvIA** (`ia.blue3.com.br`). O loop
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
- Servidor/gateway: `~/x/SHVIA-WEB` (`https://ia.blue3.com.br`)
- Padrão de aprovação de comandos: SShvTerm (aba Agente)
