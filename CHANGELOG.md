# Changelog

Entradas no formato da mensagem de commit (`versão - comentário`), mais recente
primeiro. É daqui que a skill COMMITTER tira a mensagem (CLAUDE.md §PS).

## 0.11.2 - O gate de aprovação para de perguntar o que ele mesmo classifica como leitura

- **O relato (19/08):** *"no modo Aprovação Auto está demandando muito ainda,
  tem muita coisa como `ls`"*. O `ls` sozinho já era `Auto` — o que pedia
  aprovação era `cd KIDS-THALO && ls`, porque a string tinha `&&` e isso
  derrubava o comando INTEIRO do allowlist. O agente compõe `cd <pasta> && <cmd>`
  o tempo todo, então praticamente toda inspeção virava pergunta.
- **Segmento a segmento.** `&&` deixa de ser motivo de recusa: cada trecho é
  conferido isoladamente e **todos** precisam passar; um fora da lista basta para
  virar `Confirm`. O `is_destructive` continua rodando sobre a string inteira
  ANTES disso, então `ls && git push` segue `Always`.
- **A lista de leitura cresceu** com o que faltava e aparecia toda hora: `grep`,
  `rg`, `awk`, `cut`, `sort`, `uniq`, `diff`, `jq`, `which`, `cd`, `stat`,
  `basename`, `find` (com guarda) e os `git` de leitura (`rev-parse`,
  `ls-files`, `blame`…). Critério: não escreve, não apaga, não sai para a rede.
- **`sed` ficou de fora, e está registrado por quê:** `sed -n` é leitura, mas
  `sed -n 's/a/b/w saida'` grava, e separar isso de um `w` que é só parte do
  padrão (`/warn/p`) exige parsear sed. Guarda incompleta em gate de segurança é
  pior que guarda nenhuma. Quem quer intervalo de linhas usa `head`/`tail`/`awk`.
- **O scanner passou a conhecer aspas**, e isso corrigiu dois erros que a versão
  ingênua tinha — os dois no mesmo comando real, `awk 'NR>=37 && NR<=46' x`: o
  `&&` dentro do script era tratado como encadeamento, e o `>=` era lido como
  redirecionamento. Aspa simples e dupla **não** são equivalentes: dentro de
  `"…"` o shell ainda expande `$(…)` e crase, então substituição de comando é
  recusada mesmo entre aspas duplas. Aspa não fechada também recusa.
- 58 testes (7 novos), clippy limpo. **Nada foi afrouxado**: destrutivo continua
  `Always`, escrita continua `Confirm`, e não existe modo yolo.

## 0.11.1 - O aviso do teto para de vazar JSON de protocolo na tela do usuário

- A casca renderiza a string do `warn` na timeline, então `aviso_teto` do
  `JsonSink` é texto para HUMANO: mandar `{"type":"user","text":"continua"}` ali
  colocou payload NDJSON na frente de quem só quer continuar o trabalho (visto
  em 19/08 no app desktop, um turno depois de a 0.11.0 subir). O payload segue
  documentado no `embedding.md`, que é onde quem implementa host vai olhar.

## 0.11.0 - Teto de voltas deixa de ser constante e o estouro passa a perguntar ao dev (terminal) em vez de jogar o turno fora

- CASO REAL (19/08, projeto KIDS/Godot no app desktop, MiniMax M3): tarefa
  legítima de testes+correção morreu na volta 25 com "teto de voltas atingido —
  refine a pergunta" — conselho errado (a sessão fica viva; "continua" retoma) e
  teto baixo demais para trabalho agêntico de verdade (26 ferramentas no turno).
- O teto sai de `const MAX_ROUNDS = 25` para `max_voltas` no Config: env
  `SHVIA_MAX_VOLTAS` > config.toml > default 100 (`config::DEFAULT_MAX_VOLTAS`),
  mínimo 1. `anna config` mostra o valor resolvido. Continua SEMPRE limitado
  (A6/§2.7) — o que mudou é onde o humano decide.
- Estouro agora é decisão do dev, por casca (`Frontend::continuar_apos_teto` +
  `aviso_teto`): terminal interativo pergunta "continuar mais um lote?" (cada
  "sim" concede exatamente mais um `max_voltas`); host NDJSON persistente
  encerra o turno com a sessão viva e aviso para reenviar
  `{"type":"user","text":"continua"}`; `-p`/`--json -p` avisam que nada é
  persistido e o caminho é rodar de novo com teto maior. A pergunta NUNCA viaja
  pelo `gate_request` — o modo Auto do host aprovaria sozinho (loop sem humano).
- Docs no mesmo commit: protocolo.md §4.5, SECURITY §2.7, embedding.md (campo
  `text`, não `content`), arquitetura-cli.md (pseudocódigo e config.toml),
  ferramentas.md, ANNA.md (que o anna lê como contexto), README (contagem de
  testes). Revisão adversarial (24 agentes) pegou e matou: doc de retomada com
  campo errado, promessa falsa de sessão no `-p`, pseudocódigo meio-atualizado.

## 0.10.0 - O agente deixa de se apresentar como "Anna" e o histórico ganha teto (sessão longa não trava mais no 422)

- IDENTIDADE: o system prompt dizia "Você é a Anna" e modelos obedientes
  defendiam a persona contra o próprio usuário — caso real de 18/08 com o
  MiniMax M3, que tratou "você não é a Anna" como prompt injection e respondeu
  "I should continue being Anna". ANNA é a marca do Chat do ShvIA (outro agente);
  o modelo do Modo Code não deve nem tomar conhecimento dela. Prompt novo:
  "agente de código do ShvIA Code", sem nome próprio nem persona, com instrução
  de não reivindicar nem discutir identidade. Vale para os dois modos (terminal
  e host/SShvTerm). O nome `anna` segue sendo só o binário/CLI.
- HISTÓRICO: o gateway valida `messages` com max:200 (anti-abuso §3.3) e uma
  sessão longa ESTOURAVA o teto — o /code/chat rejeitava a requisição inteira
  ("must not have more than 200 items") e a sessão ficava presa, retry após
  retry (caso real: 26M tokens acumulados no M3). Agora `compactar_historico`
  roda a cada volta do loop: preserva os system iniciais (prompt-semente +
  contexto do projeto) e as mensagens mais recentes (janela nunca abre em
  role=tool órfão — provedor OpenAI-strict rejeita), e insere UM marcador system
  no corte avisando o modelo do trecho ausente (sem isso ele alucina o que não
  está mais lá). Compacta em 150 (folga de 50 para as voltas do turno);
  recompactação substitui o marcador, nunca acumula. O frontend avisa quantas
  mensagens saíram. +4 testes (47 no total), clippy limpo.
