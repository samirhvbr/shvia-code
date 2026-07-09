#!/usr/bin/env bash
# Instala o `anna` no PATH do usuário — SEM sudo. Reusa o build-local.sh
# (preflight de toolchain + build release + checksum) e copia o binário pra
# ~/.local/bin (ou $ANNA_BIN_DIR). Idempotente: rode de novo pra atualizar.
set -euo pipefail
cd "$(dirname "$0")"

./build-local.sh

DEST="${ANNA_BIN_DIR:-$HOME/.local/bin}"
mkdir -p "$DEST"
install -m 0755 target/release/anna "$DEST/anna"

echo
echo "✓ anna instalado em $DEST/anna"
"$DEST/anna" --version

case ":$PATH:" in
  *":$DEST:"*) ;;
  *)
    echo
    echo "⚠  $DEST não está no PATH. Adicione ao seu ~/.bashrc ou ~/.zshrc:"
    echo "     export PATH=\"$DEST:\$PATH\""
    ;;
esac

echo
echo "Próximo: export SHVIA_API_KEY=<sua-chave>  e rode  anna  dentro de um projeto."
