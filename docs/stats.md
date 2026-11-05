# `anna stats` — dashboard de uso (Wrapped)

> **Current scope (2026-10-15, rechecked 2026-11-05):** this dashboard reads retained REPL session data, not
> every engine call. One-shot and NDJSON modes have no local session autosave, and
> pipeline activity does not create an equivalent complete dashboard record. Message
> history is compacted; per-session model metadata is not per-message accounting.
> [User guide](USER-GUIDE.md) · [open fidelity findings](DOCUMENTATION-REVIEW-20261015.md).

A tela **"O que vem a seguir, &lt;você&gt;?"**: um resumo do seu uso do `anna`,
no estilo _wrapped_. Roda **100% local** — lê só as sessões em
`~/.shvia-code/sessions/`, não fala com o gateway.

```
anna stats                     # Visão Geral no terminal (período: todos)
anna stats --tab models        # aba Modelos (quebra por modelo)
anna stats --range 7d          # filtro: 7d | 30d | all
anna stats --html              # exporta ~/.shvia-code/stats.html (abas/filtros ao vivo)
anna stats --html --open       # exporta e abre no navegador
anna stats --html --out /tmp/x.html
```

No REPL: `/stats`, `/stats models`, `/stats 7d`, `/stats html`.

## O que cada número significa

| Tile | De onde vem |
|------|-------------|
| **Sessões** | nº de arquivos de sessão no período |
| **Mensagens** | mensagens de conversa (papéis `user` + `assistant`; `system`/`tool` não contam) |
| **Total de tokens** | soma dos tokens gravados por sessão (`usage` do gateway) |
| **Dias ativos** | dias distintos com ao menos uma sessão (data **local**) |
| **Seq. atual** | dias consecutivos até hoje/ontem (quebrou → 0) |
| **Maior seq.** | maior corrida de dias consecutivos no histórico |
| **Horário de pico** | hora (local) em que você mais **abre** sessões |
| **Modelo favorito** | modelo mais frequente (o `model` reportado pelo gateway, ou o perfil) |
| **Heatmap** | intensidade = nº de sessões por dia, estilo GitHub |
| **Moby-Dick** | brincadeira: seus tokens ÷ ~268k (o tamanho do romance) |

## De onde vêm os dados (e a pegadinha das sessões antigas)

Cada sessão passou a gravar, no autosave, três campos novos (retrocompatíveis,
`#[serde(default)]` — ver [`session.rs`](../src/session.rs)):

- `tokens` and `cost` — the in-memory telemetry accumulator at autosave time, not a
  per-turn delta. The accumulator starts fresh in a resumed process; saved totals are
  not loaded into it, so the resumed process's first autosave **replaces** them with its
  own count (`main.rs`, `salvar_sessao`). A resumed session therefore under-reports what it
  cost overall. See the [fidelity findings](DOCUMENTATION-REVIEW-20261015.md#status-on-2026-11-05).
- `model` — o `model` do evento SSE do gateway (cai pro perfil escolhido se o
  gateway não informar).

> **Sessões criadas antes desta versão (0.8.4) não têm esses campos** — nunca
> foram registrados. Nelas, **Total de tokens** e **Modelo favorito** aparecem
> como `—`, e a aba Modelos as agrupa em `(sem modelo)`. Tudo passa a contar
> **das próximas sessões em diante**. Sessões/mensagens/dias/streak/heatmap
> funcionam retroativo (derivam do `id`=epoch e das mensagens já salvas).
>
> Granularidade de tempo = **início da sessão** (o `id` é o epoch da criação):
> por isso "horário de pico" e o heatmap são por sessão aberta, não por
> mensagem. Suficiente e honesto; per-mensagem exigiria timestamp por turno.

## Arquitetura (3 peças)

- [`stats.rs`](../src/stats.rs) — o **cérebro**: agrega as sessões e expõe
  funções puras testadas (`current_streak`, `longest_streak`, `peak_hour`,
  `build_heatmap`, formatação). `compute_at(sessions, range, hoje)` recebe o
  "hoje" injetado → testes não dependem do fuso da máquina.
- [`stats_term.rs`](../src/stats_term.rs) — render no **terminal** (ANSI): tiles
  em box-drawing + heatmap em blocos azuis (256-cores) + a frase do Moby-Dick.
- [`stats_html.rs`](../src/stats_html.rs) — **export HTML** offline (CSS/JS
  embutidos, zero rede): o Rust pré-computa as 3 faixas (todos/30d/7d) e o JS só
  troca qual view desenha — abas e filtros funcionam no navegador **sem
  duplicar regra de negócio**.

Datas/hora **local** via `chrono` (feature `clock`) — a única dependência nova.
