#!/usr/bin/env bash
# Dogfood do C4: usar o `anna` pra trabalhar no PRÓPRIO repo — a prova de fogo.
# Precisa: gateway 2.16.0 no ar + SHVIA_API_KEY exportada.
set -euo pipefail
cd "$(dirname "$0")/.."
BIN="./target/release/anna"
[ -x "$BIN" ] || { echo "Build primeiro:  ./build-local.sh   (ou ./install.sh)"; exit 1; }

# Tarefas representativas — uma por capacidade que dá pra rodar em modo -p
# (leitura / bash-allowlist / git-read; escrita e commit são interativos).
TAREFAS=(
  "Explique em 2 frases o que o módulo src/gate.rs faz e quando ele pede confirmação."
  "Onde no código a denylist de segredos é aplicada? Cite arquivo:linha."
  "Rode 'cargo test' e me diga em uma linha se passou."
  "Qual o status do git agora?"
)

if [ -z "${SHVIA_API_KEY:-}" ]; then
  cat <<'EOF'
SHVIA_API_KEY não definida — não dá pra rodar o loop ao vivo.

Quando tiver a chave e o gateway 2.16.0 no ar:

  export SHVIA_API_KEY=<sua-chave>
  ./scripts/dogfood.sh        # roda as tarefas de leitura/bash/git (modo -p)

O ciclo COMPLETO (edição + commit) é interativo, no REPL — porque
edit_file/write_file/git_commit pedem confirmação:

  ./target/release/anna
  você › adicione um comentário de módulo em src/telemetry.rs explicando o rodapé de custo
  você › rode cargo test e, se passar, commite como "0.4.1 - dogfood: comentário em telemetry"
EOF
  exit 0
fi

echo "== Dogfood ao vivo (SHVIA-CODE sobre si mesmo) =="
for t in "${TAREFAS[@]}"; do
  echo
  echo "── $t"
  "$BIN" -p "$t"
done
echo
echo "Leitura/bash/git ✓. Feche o DoD do C4 com uma EDIÇÃO + COMMIT no REPL interativo."
