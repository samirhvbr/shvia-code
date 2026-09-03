# SHVIA-CODE

CLI agêntico de código da Blue3 — comando **`anna`** (Rust). A experiência do
Claude Code, movida pelo **gateway do ShvIA** (`ai.shvia.org`): a Anna lê
arquivos, propõe diffs, edita, roda comandos e commita — **com aprovação** — no
projeto em que o dev está, usando qualquer perfil de modelo do ShvIA (on-prem
ou nuvem BYOK).

> Antes de mexer: **`git pull`**. Convenções em [CLAUDE.md](CLAUDE.md) ·
> Escopo aprovado e fases em [.continue/escopo-projeto.md](.continue/escopo-projeto.md) ·
> Decisões em [docs/decisoes.md](docs/decisoes.md).

## Status

**C1–C3 no código (0.3.x)** — o `anna` lê, edita (diff+confirmação), roda `bash`
(com gates), commita, retoma **sessões**, troca de modelo (`/model`) e responde
em markdown. Em 0.9.0 entra o **pipeline de papéis** (`/plano` —
[orquestrador.md](docs/orquestrador.md)): arquiteto → você aprova → executor →
revisão. `cargo clippy -D warnings` limpo, 51 testes verdes. **Falta só o
smoke ao vivo** (precisa de `SHVIA_API_KEY` + gateway 2.16.0) pra provar o loop
de ponta a ponta contra um modelo real — inclusive o `/plano`, que até aqui só
foi exercitado nos testes de contrato (parse/validação/só-leitura), nunca contra
um modelo.

## Instalar

```bash
git clone git@github.com:samirhvbr/SHVIA-CODE.git && cd SHVIA-CODE
./install.sh                    # preflight + build release + copia p/ ~/.local/bin
export SHVIA_API_KEY=<sua-chave-do-ShvIA>
```

Precisa de Rust (`rustup`) — o `install.sh` avisa se faltar algo. A chave vem de
`SHVIA_API_KEY` ou `~/.config/shvia-code/config.toml` — **nunca** por argumento de
CLI. Para gravá-la no arquivo com `0600`, use `anna config set-key` (sem argumento
ele lê do stdin, e assim a chave não fica no histórico do shell). Se o arquivo tiver
chave e permissão frouxa, o `anna` avisa ao carregar. `anna config` mostra o estado
resolvido.

## Usar

```bash
cd ~/meu-projeto
anna                            # REPL: pergunte, edite, rode, commite (com aprovação)
anna -p "onde está o bug no cálculo de boleto?"   # uma pergunta, não-interativo
anna --model "modelo@servidor"  # perfil específico  ·  /model troca no meio
anna sessions list|resume <id>|clear              # retomar de onde parou
anna stats                      # dashboard de uso (Wrapped) — 100% local
anna stats --html --open        # exporta a página HTML e abre no navegador
```

Comandos do REPL: `/help` · `/plano <pedido>` · `/model [perfil]` · `/sessions` · `/stats` · `/tools` · `/clear` · `/exit`.

O **[`/plano`](docs/orquestrador.md)** roda a tarefa como pipeline de papéis em
vez de um modelo fazendo tudo: o **arquiteto** (só leitura) propõe um plano
arquivo-a-arquivo, **você aprova ou não**, o **executor** aplica passo a passo
(com os gates de diff de sempre) e a revisão fecha — primeiro clippy/testes, e só
depois um **revisor** que julga o diff contra o plano. Cada papel tem perfil e
esforço próprios (`[elenco]` no `config.toml`, `anna config` mostra a tabela) e o
custo sai discriminado por papel no fim.

O **[`anna stats`](docs/stats.md)** é a tela _wrapped_ ("O que vem a seguir,
&lt;você&gt;?"): sessões, mensagens, tokens, dias ativos, sequência, horário de
pico, modelo favorito e um heatmap de contribuição — no terminal ou exportado em
HTML (abas Visão Geral/Modelos + filtros 7d/30d ao vivo). Lê só
`~/.shvia-code/sessions/`, sem tocar no gateway.

## Desenho em uma linha

Loop agêntico **no cliente** (tools executam na máquina do dev, com gates), SSE
contra o gateway do ShvIA (auth por API key) — auditoria, quotas, custo e LGPD
herdados do servidor.

## Fases

C0 spike do gateway → **C1 leitura ✓** → **C2 mãos (edit/bash/git) ✓** → **C3
ergonomia (sessões, `/model`, markdown) ✓** → **C4 distribuição (`install.sh` +
dogfood) ▸ em curso** → C5 skills/RAG/CI/SShvTerm.
