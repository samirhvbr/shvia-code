# SHVIA-CODE — Agente de código no terminal (estilo Claude Code), movido pelo ShvIA

> **Status:** escopo APROVADO (Samir, 08/07/2026) — decisões D1/D2/D6 travadas;
> demais adotam a recomendação (reversíveis até o C1). Próximo passo: **C0**.
> Padrão da casa: `version.md`, commits `versão - comentário pt-BR`,
> docs/ + .continue/, build-local com preflight.

---

## 1. O que é

Um **CLI agêntico de código** — a experiência do Claude Code, mas com a **Anna**
e o **gateway do ShvIA** por trás: o dev abre o terminal na pasta de um projeto,
conversa, e a Anna **lê arquivos, propõe diffs, edita, roda comandos e commita**
— com aprovação — usando **qualquer perfil de modelo que o dev já tem no ShvIA**
(on-prem ou nuvem BYOK, igual ao painel; D6 = livre).

```
~/x/MEU-PROJETO ➜ anna
ANNA · ShvIA Code — projeto MEU-PROJETO (git: master limpo) · modelo: <perfil padrão do dev>

> corrige o warning de deprecated no build e roda os testes

  ⚙ lendo build.log … ✓
  ⚙ lendo src/legacy.php … ✓
  ✎ proposta de edição em src/legacy.php (2 hunks):
      - utf8_decode($nome)
      + mb_convert_encoding($nome, 'ISO-8859-1', 'UTF-8')
  aplicar? [s]im / [n]ão / [t]udo nesta sessão  → s
  $ composer test          (comando não-destrutivo: rodou sem aprovação)
  ✓ 34 testes verdes
  ⏱ 41 s · 12.4k tokens · custo ≈US$ 0.03
```

**Não é** fork do Claude Code nem produto público: é a peça que fecha o
ecossistema pra uso interno Blue3 (e clientes ShvIA). Diferencial: quem quiser
que o código **nunca saia de casa** roda num perfil on-prem — e os guardrails
LGPD do gateway (masking/data_locality) valem sempre, sem fricção no CLI.

## 2. Por que agora — o que JÁ EXISTE pra reusar

| Peça | Onde | Estado |
|------|------|--------|
| **Tool-calling nos 3 drivers** (OpenAI-compat/Anthropic/Ollama): `ToolCall`, `StreamCallbacks::onToolCall`, `tools`/`tool_choice` | `SHVIA-WEB app/Services/AI/Drivers/*` (commit `Drivers`, 06/07) | ✅ pronto — falta só **expor na borda HTTP** |
| Gateway único SSE + auditoria + quotas + custo (real/estimado) | `POST /api/v1/chat`, `inference_requests` (2.15.9) | ✅ herdado de graça |
| **LGPD**: `data_locality` por servidor + `maskOutboundPii()` na saída pra nuvem | `ChatService` | ✅ herdado, automático |
| Auth por **API key** (X-API-Key/Bearer) + rate limit | guard apikey (`agent/chat`) | ✅ é o login do CLI |
| BYOK por usuário (Kilo/OpenAI/Anthropic/xAI/Z.ai) + perfis | catalog/perfis | ✅ o `/model` do CLI = perfis do dev |
| Padrão de **aprovação de comandos** (não-destrutivo roda; destrutivo confirma) | SShvTerm (aba Agente) | ✅ UX provada na casa |
| GPU on-prem p/ modelos de código | host SHVIA-GPU (Ollama) | ✅ qwen-coder/deepseek-coder etc. |
| Skills em markdown lidas por API | `/api/v1/skills` | ✅ candidato a slash-commands (C5) |

O trabalho novo de verdade: **um repo de CLI (Rust)** + **~1 endpoint** no servidor.

## 3. Arquitetura — loop agêntico NO CLIENTE (D3)

O modelo fica no servidor; **as mãos ficam na máquina do dev**:

```
┌────────────────────────── máquina do dev ──────────────────────────┐
│  SHVIA-CODE (CLI `anna`, Rust)                                     │
│  ├─ REPL/TUI (streaming, markdown, diff colorido)                  │
│  ├─ Loop agêntico:                                                 │
│  │    1. monta histórico + defs das ferramentas                    │
│  │    2. POST /api/v1/code/chat (SSE) ───────────────┐             │
│  │    3. recebe deltas + tool_calls                  │             │
│  │    4. EXECUTA a ferramenta LOCALMENTE (c/ gate)   │             │
│  │    5. devolve resultado (role=tool) → repete 2    │             │
│  ├─ Tools locais: read/list/grep/glob · edit/write (diff+ok)       │
│  │    · bash (allowlist/confirmação) · git (status/diff/commit)    │
│  ├─ Gates de aprovação (padrão SShvTerm)                           │
│  └─ Sessões locais (~/.shvia-code/) + contexto do projeto          │
│     (ANNA.md; lê CLAUDE.md/AGENTS.md se existirem) + custo/turno   │
└──────────────────────────────┬─────────────────────────────────────┘
                               │ HTTPS (API key; SSE)
                               ▼
        ShvIA (Laravel) — POST /api/v1/code/chat  ← ÚNICO trabalho server
        (ChatService::callOllama já aceita tools; expor na borda + role=tool
         no histórico; auditoria/quota/LGPD/custo idênticos ao /chat)
                               │
                 perfis do dev: Ollama on-prem · nuvem BYOK
```

Por quê no cliente: é o desenho do Claude Code (provado); arquivos/segredos só
trafegam no trecho que o modelo precisa ver; servidor stateless de ferramentas
(zero superfície nova server-side); roda em qualquer máquina com binário+chave.

## 4. Ferramentas v1 (poucas e afiadas)

| Tool | Gate | Nota |
|------|------|------|
| `read_file` / `list_dir` / `grep` / `glob` | auto | respeita .gitignore; recorte com limite de bytes |
| `edit_file` (search/replace) / `write_file` | **diff + confirmação** ([s]/[n]/[t]udo) | nunca escreve sem mostrar |
| `bash` | allowlist auto (ls, cat, composer/npm/cargo test|check…) · resto **confirma** · destrutivo (rm -rf, git push -f…) **sempre** confirma | espelha o SShvTerm |
| `git_status` / `git_diff` / `git_commit` | commit confirma; mensagem no padrão da casa | `git push` só explícito |

## 5. Segurança & LGPD

1. **Modelos livres (D6)**: o dev usa qualquer perfil dele (default = o mesmo do
   painel; troca com `/model`). Quem precisa de sigilo total escolhe on-prem.
2. Indo pra nuvem: `maskOutboundPii()` + `data_locality` **continuam valendo**
   server-side — herdados, sem fricção no CLI.
3. Aprovações no cliente: nada escreve/roda sem gate (§4). Sem modo yolo no v1.
4. **Auditoria integral**: cada chamada → `inference_requests` (tokens/custo/
   duração) — o admin vê o uso do code igual vê o chat.
5. API key revogável por usuário; rate limit existente.

## 6. Trabalho no servidor (pequeno e cirúrgico)

- **`POST /api/v1/code/chat`** (D8: dedicado): aceita `messages[]` (com
  `role=tool` + `tool_call_id`), `tools[]`, `tool_choice`, `server/model/effort`
  → repassa pro `ChatService::callOllama(...)` que **já tem os parâmetros**.
  SSE: eventos atuais + `tool_call`.
- System prompt de código (perfil "code") + gate por work group.
- Zero banco novo; auditoria atual serve.

## 7. Fases

- **C0 — Spike do gateway (½ dia)**: provar o roundtrip completo contra o
  servidor: pergunta → `tool_call` → resultado → resposta final, com um modelo
  on-prem E um de nuvem. Mede a qualidade de tool-calling dos modelos ANTES de
  investir. *DoD: transcript do loop fechado.*
- **C1 — MVP leitura (1 sem)**: binário `anna` (Rust) com REPL streaming, auth
  API key, contexto do projeto, tools de leitura (auto). Já é útil: "explica
  esse repo", "onde está o bug?". *DoD: sessão real num repo da Blue3.*
- **C2 — Mãos (1 sem)**: edit/write com diff+aprovação, bash gated, git.
  *DoD: a Anna corrige um bug real e commita, com aprovações.*
- **C3 — Ergonomia (½–1 sem)**: sessões resume, ANNA.md, custo/tokens por
  turno, `/model`, cores/TUI.
- **C4 — Distribuição (½ sem)**: build-local com preflight (padrão 0.5.8),
  binários macOS/Linux (Windows depois), doc de install, **dogfood**: usar o
  SHVIA-CODE pra desenvolver o SHVIA-CODE.
- **C5 — Futuro**: skills do ShvIA como slash-commands, RAG dos repos (KB),
  modo não-interativo (`anna -p "…"` pra CI), mesmo motor na aba Agente do
  SShvTerm.

## 8. Decisões

| # | Decisão | Estado |
|---|---------|--------|
| D1 | Repo `SHVIA-CODE`; comando **`anna`** | ✅ travado (08/07) |
| D2 | Stack **Rust** (binário único, casa domina) | ✅ travado (08/07) |
| D3 | Loop agêntico **no cliente** | adotado (recomendação) |
| D4 | Auth = **API key** existente | adotado (recomendação) |
| D5 | Gates do §4 como default; configurável por projeto depois | adotado (recomendação) |
| D6 | Modelos **livres** (qualquer perfil do dev; sem flag extra) | ✅ travado (08/07) |
| D7 | Sessões **locais** no v1 | adotado (recomendação) |
| D8 | Endpoint **dedicado** `/api/v1/code/chat` | adotado (recomendação) |

## 9. Riscos (com mitigação)

1. **Tool-calling dos modelos on-prem** aquém — C0 mede antes de investir;
   nuvem BYOK como escape (D6 livre já permite).
2. Loop infinito/custo — teto de iterações por turno + quotas existentes.
3. Segurança do bash — gates + allowlist curta no v1 (sem sandbox; uso interno).
4. SSE longo — retomada/timeout tratados no CLI.

## 10. Não-objetivos (v1)

Sem editor/IDE embutido; sem multi-agente; sem sandbox de execução; sem Windows
no C1 (entra no C4); sem publicar fora da Blue3.
