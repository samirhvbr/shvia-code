# Changelog

Entradas no formato da mensagem de commit (`versão - comentário`), mais recente
primeiro. É daqui que a skill COMMITTER tira a mensagem (CLAUDE.md §PS).

## 0.11.4 - O turno do usuário passa a carregar imagem, e só o último turno a carrega

- **O que entra no protocolo:** `images` em `role:"user"` — lista de **base64
  puro**, sem o prefixo `data:`. É a convenção do Ollama que o gateway já falava
  para o chat (`ChatService` monta `$entry['images']` assim), e os drivers de
  nuvem derivam o mime dos **magic bytes** (`ImageMime::detect`), então o tipo
  **não viaja**. Documentado em `docs/protocolo.md`.
- **Turno sem imagem sai byte a byte como saía.** `skip_serializing_if` mantém a
  chave fora do JSON quando a lista está vazia — e isso é 100% dos turnos de
  hoje. Há régua travando o corpo exato (`turno_sem_imagem_nao_ganha_chave_nenhuma`).
- **`api.rs` não mudou uma linha:** ele serializa `&[Message]` direto, então
  campo novo na struct atravessa sozinho. O dimensionamento previa mexer lá;
  medir encolheu a fatia.
- ⚠️ **A conta que ia crescer sozinha, e o motivo de `empurrar_turno_usuario`
  existir.** O `anna` guarda o histórico em MEMÓRIA e reenvia a lista inteira a
  cada volta do laço agêntico. Se a figura ficasse presa ao turno que a trouxe,
  ela subiria de novo em cada chamada de ferramenta, de todo turno seguinte, até
  a sessão acabar — num turno de código com 30 voltas é a mesma imagem paga 30
  vezes, **com a resposta saindo certa**. Defeito que ninguém audita porque nada
  parece errado. Agora o turno novo apaga a figura do anterior e **mantém o texto
  dele**; dentro do turno ela sobrevive a todas as voltas, de propósito, porque o
  modelo precisa poder olhar a imagem outra vez depois de ler um arquivo.
- **Item que não é string sai fora** em vez de virar `""`: base64 vazio chega ao
  provedor como imagem corrompida, e o erro voltaria falando de outra coisa.
- **Cinco réguas novas** (63 no total), conferidas por reversão: tirar o
  `skip_serializing_if`, tirar a limpeza e fazer a limpeza levar o texto junto —
  cada uma acendeu só a sua.
- **Do outro lado:** `SHVIA-WEB` 2.102.39 (validação, teto do total, guarda de
  visão e o repasse na allowlist do `CodeChatController`) e `SHVIA-DESKTOP`
  1.1.31. Sem eles o campo viaja e o gateway o descarta.

## 0.11.3 - O bump da 0.11.2 saiu pela metade, e o repo ganha a prova que o desktop já tinha

- **O CASO (medido em 20/08/2026):** o commit da 0.11.2 (`3da7875`) levou
  `CHANGELOG.md`, `src/tools.rs` e `version.md` — e **deixou `Cargo.toml` e
  `Cargo.lock` em `0.11.1`**. O commit da 0.11.1 (`5cd3144`) tinha levado os cinco,
  então não era desconhecimento do padrão: foi omissão silenciosa. Nada acusou —
  árvore limpa, `cargo build` exit 0, e **o binário compilado respondendo `anna
  0.11.1` com o código da 0.11.2**.
- **Por que dói além da estética:** o `stage-anna.mjs` do SHVIA-DESKTOP decide o que
  empacotar lendo `--version` do binário, e o *"Sobre → Motor do Code"* (desktop
  1.1.25) mostra esse número **ao usuário** — entregue justamente porque a versão do
  sidecar não aparecia em lugar nenhum. Bump pela metade faz ela aparecer **errada**,
  que é pior que não aparecer: o número existe e mente.
- **E havia um aviso que se dizia guarda.** O `Cargo.toml` trazia, na linha da versão,
  o comentário *"O build-local.sh checa"*. **Não checava:** medido, todas as
  referências a `version.md` naquele script são ao `version.md` do próprio desktop.
  Não é só "aviso perde para gesto" — o aviso **afirmava ser** o gesto.
- **Entra `build.rs` como a prova:** `version.md` (fonte) × `Cargo.toml`, e o build
  **cai** nomeando o divergente. É `build.rs` e não `#[test]` porque o contrato é
  *"os portadores concordam ou o build cai"* e `cargo build` não roda testes — teste
  protege quem roda a suíte, `build.rs` protege quem só compila, que é o caminho do
  `install.sh` e do empacotamento.
- **Conferido por reversão, e o resultado corrigiu o próprio desenho.** Desalinhar o
  `version.md` derruba com a lista dos divergentes (`Cargo.toml = 0.11.3`,
  `Cargo.lock = 0.11.3`). Mas desalinhar **só o `Cargo.lock`** *passou* — porque o
  `cargo build` **reescreve o lock em silêncio** a partir do `Cargo.toml` antes de
  chamar o `build.rs`. **Quem guarda o lock é o `--locked`** (aborta com `cannot
  update the lock file`, exit 101), e isso ficou escrito no `build.rs` em vez de
  virar uma checagem que se anuncia e não age.
- Portadores desta casa, agora explícitos: **`version.md`, `Cargo.toml`,
  `Cargo.lock`** — os dois primeiros pelo `build.rs`, o terceiro por `--locked`.

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
