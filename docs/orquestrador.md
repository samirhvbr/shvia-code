# Orquestrador — pipeline de papéis (`/plano`)

> Código: [`src/orquestrador.rs`](../src/orquestrador.rs) ·
> contratos: [`src/plano.rs`](../src/plano.rs) ·
> elenco: [`src/papel.rs`](../src/papel.rs) · decisão: [ADR-003](decisoes.md).

O turno normal da Anna é **um modelo fazendo tudo**: entende, planeja, edita e
confere, no mesmo contexto. O `/plano` quebra isso em três papéis, cada um com
perfil, esforço e ferramentas próprios.

```
pedido → [arquiteto] → plano.json → VOCÊ aprova → [executor] → diff
                                                        ↑          ↓
                                                        │   clippy/testes ──✗──┐
                                                        │          ↓ ✓         │
                                                        └──✗── [revisor] ──────┘
                                                                   ↓ ✓
                                                                 pronto
```

> **Current boundary (2026-10-15, rechecked against 0.15.8 on 2026-11-05):** implemented
> in the terminal only, with local tools and separate role histories. Pipeline state is not
> durably resumable, and a `/plano` run is not written to the session store. Automatic
> verification skips non-allowlisted/unspawnable commands with a warning and still uses a
> subprocess path (`falha_verificacao`) without the shell tool's timeout or turn stop.
> Since 0.14.0 a Ctrl-C that stops an architect, executor or reviewer turn ends the
> pipeline instead of letting the next phase act on a cut-short result. Treat successful
> review as a model verdict, not proof that all configured checks ran. See the
> [source findings](DOCUMENTATION-REVIEW-20261015.md#status-on-2026-11-05).

## Por que papéis, e não um roteador

A leitura fácil de "blend de modelos" é *um roteador escolhe o modelo por
pedido*. Não é isso aqui. O que faz a coisa funcionar não é o roteamento — é o
**contrato entre as fases**: os papéis não compartilham contexto, compartilham
**artefato** (`Plano`, `Veredito`). É isso que deixa trocar o perfil de um papel
sem o resto desmoronar, e é por isso que o schema é validado em
[`plano.rs`](../src/plano.rs) — se o modelo derivar de formato, a fase falha alto
em vez de degradar em silêncio na fase seguinte.

Três consequências de desenho:

1. **A tabela papel → perfil é estática** (`[elenco]` no `config.toml`). Um
   modelo-roteador seria mais uma chamada paga, erraria a classificação e não
   daria pra auditar. Estático é grátis, versionável e visível no `anna config`.
2. **Esforço é uma dimensão mais barata que perfil.** O papel é um par
   `(perfil, esforço)`. Um Sonnet em `xhigh` resolve muita coisa que pareceria
   trabalho de Opus — dobra o espaço de ajuste sem dobrar a conta.
3. **Cache de prompt é escopado por modelo.** Toda troca de papel paga cache
   frio. Esse é o argumento econômico a favor de passar artefato compacto entre
   as fases em vez de repassar histórico: não é elegância, é custo.

## Os papéis

| Papel | Entrega | Ferramentas | Esforço padrão |
|---|---|---|---|
| **arquiteto** | `Plano` (JSON) | só leitura | `xhigh` |
| **executor** | o diff | todas (com os gates de sempre) | `high` |
| **revisor** | `Veredito` (JSON) | só leitura | `high` |

"Só leitura" **não é instrução de prompt**: `runner::SomenteLeitura` filtra as
ferramentas oferecidas *e* recusa a chamada se o modelo insistir. O arquiteto e o
revisor não escrevem porque não têm como. `bash` fica de fora da lista — comando
que lê e comando que grava entram pela mesma porta.

O revisor é o melhor lugar pra usar uma **família diferente** do executor:
revisor da mesma família concorda consigo mesmo, e erro correlacionado é o modo
de falha que o pipeline existe pra pegar.

## Os contratos

`o_que` (plano) e `o_que_fazer` (veredito) seguem a mesma regra dura: **quem vai
executar não viu o raciocínio de quem escreveu**. Instrução que só faz sentido
pra quem leu o thinking do arquiteto é instrução quebrada — daí a validação de
tamanho mínimo, que é o modo de falha nº 1 do pipeline.

`aceite` é a régua do revisor. Critério verificável (um comando que passa, um
comportamento observável) faz o loop fechar; critério vago (`"o código fica
melhor"`) faz o loop nunca terminar.

## Os dois gates de revisão

**Gate 0 — determinístico, e vem primeiro.** `cargo clippy -D warnings` +
`cargo test` (ou o `verificacao` do `config.toml`). Grátis, exato, e pega a maior
fatia do que o executor erra. Rodar um modelo num diff que nem compila é pagar
pra ouvir o que o compilador já disse melhor.

O gate só executa comando que o `bash` já auto-aprovaria (`Policy::Auto`,
[`tools.rs`](../src/tools.rs)) — o gate **não é porta dos fundos** pra rodar
comando sem o dev ver. O que não passa na allowlist é pulado com aviso.

**Gate 1 — revisor-modelo.** Pergunta FECHADA: *o que o plano prometeu que o
diff não entregou?* Não é "revise este código" — crítica aberta sempre acha mais
uma coisa e o loop não converge.

O revisor classifica cada achado, e a classe decide o destino:

| classe | significa | volta pra |
|---|---|---|
| `desvio` | o diff não faz o que o passo mandou | executor |
| `escopo` | o diff mexe no que o plano não pediu | executor |
| `plano_errado` | o diff é fiel ao plano, mas o plano não resolve | **o dev** |

`plano_errado` **para o pipeline**. Replanejar no meio do voo é caro, lento, e o
dev perde o fio do que está acontecendo — e ele está sentado ali. Fechar o loop
de volta no arquiteto fica pro v1, se a prática mostrar que vale.

## Os freios

O modo de falha real não é o revisor ser burro — é **ping-pong**: reprova →
"conserta" → reprova outra coisa → desfaz a anterior → ∞, queimando token a cada
volta. Dois freios:

- **Teto de 2 voltas** por gate.
- **Dedup por achado** (`passo_id | aceite_violado`). O mesmo achado voltando
  depois de uma tentativa de correção significa que ou o executor não consegue,
  ou o achado está errado — nos dois casos, insistir não resolve. Para e devolve.

## Configurar

```toml
# Use the OS-specific path reported by anna config
profile = "seu-perfil-default"

# Comandos do gate 0. Omitido = detecta pela stack (Cargo.toml → cargo
# clippy/test; package.json/composer.json com script "test" → npm/composer test).
verificacao = ["cargo clippy --all-targets -- -D warnings", "cargo test"]

[elenco.arquiteto]
profile = "claude-opus-4-8@anthropic"
effort  = "xhigh"

[elenco.executor]
profile = "claude-sonnet-4-6@anthropic"
effort  = "high"

[elenco.revisor]
profile = "deepseek-reasoner@deepseek"   # família DIFERENTE do executor — ver abaixo
effort  = "high"
```

> **Profile names above are historical examples, not a current catalog.** Replace
> them with profiles currently available on your gateway. `anna config` shows the
> configured routing but does not fetch or validate a model catalog.
>
> **E o revisor mudou de família de propósito.** O exemplo anterior punha
> `opus5` no arquiteto **e** no revisor, com `sonnet5` no executor — Opus e Sonnet
> são a MESMA família, então o comentário *"de preferência família diferente"*
> estava sendo contrariado pelo próprio exemplo que o carregava. O ponto do revisor
> é ter modo de falha independente: um modelo da mesma casa tende a errar junto,
> aceitar a mesma alucinação e concordar com a mesma leitura torta do plano.
> `deepseek-reasoner@deepseek` é uma escolha, não uma prescrição — vale qualquer
> perfil de outra família.
>
> ⚠️ **O elenco é a conta.** Três papéis em perfis de nuvem custam três chamadas
> pagas por volta do pipeline. Para dogfood, `ShvIA:latest@srv1` (on-prem, a V100)
> não entra na fatura — vale começar por ele antes de calibrar com modelo caro.

Papel omitido cai no perfil global. Elenco inteiro vazio: o pipeline roda mesmo
assim (e avisa que não é blend de nada). `anna config` mostra a tabela resolvida.

Precedência do esforço: **elenco do papel > `effort` global > padrão do papel**.
Quem rodou `anna --effort low` de propósito não vê o arquiteto subir pra `xhigh`
sozinho.

## Usar

```
você › /plano extrair o parser de SSE do api.rs pra um módulo próprio, com testes
```

O arquiteto inspeciona e propõe; **o plano aparece inteiro e você aprova ou
não**; o executor aplica passo a passo (cada escrita ainda passa pelo gate de
diff de sempre); a revisão fecha. No fim sai o custo **por papel** — é o número
que responde "quanto custou o arquiteto?" e o que guia o ajuste do elenco.

## Escopo do v0 e o que falta

- **Terminal apenas.** O `--json` (NDJSON) precisa de um evento de gate pro plano
  no protocolo de [embedding](embedding.md) — v1.
- **Sem loop de replanejamento.** `plano_errado` devolve pro dev (acima).
- **Ferramentas locais.** O `--tools host` (ADR-002 modelo B) não passa pelo
  pipeline.
- **O prompt do revisor vai envelhecer bem só depois de uso real.** O schema de
  veredito bom sai dos erros que o executor de fato comete neste código, não dos
  que dava pra imaginar quando isto foi escrito.
