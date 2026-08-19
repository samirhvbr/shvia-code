# Changelog

Entradas no formato da mensagem de commit (`versão - comentário`), mais recente
primeiro. É daqui que a skill COMMITTER tira a mensagem (CLAUDE.md §PS).

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
