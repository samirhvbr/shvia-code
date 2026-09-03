#!/usr/bin/env bash
# Propaga o `version.md` para os outros dois portadores — e confere, se pedido.
#
#   bash scripts/sync-version.sh              # escreve Cargo.toml e Cargo.lock a partir do version.md
#   bash scripts/sync-version.sh --verificar  # não escreve; sai 1 se algum portador divergir
#
# ## Por que existe (achado F-01 da revisão de 01/09/2026)
#
# O `build.rs` deste repositório **já** provava que os três portadores concordam — e provou:
# ele derrubou o build. O que faltava era o outro lado, o que **faz** concordar. Sem ele, a
# convenção "bumpe o version.md" convida a mexer num arquivo só, que é exatamente o que
# aconteceu nas versões 0.11.5 e 0.11.6: as duas tocaram apenas o `version.md`, e o `master`
# ficou sem compilar por duas versões — `cargo check`, `clippy`, `test`, `install.sh` e o
# empacotamento do desktop, todos parados no mesmo `panic!`.
#
# ⚠️ **E ninguém viu**, porque o CI só dispara em `src/**` e `Cargo.*`: um commit que mexe só
# no `version.md` não acorda o build. Este script fecha o lado de escrever; o gatilho do CI
# foi ampliado no mesmo commit para fechar o lado de medir.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

MD="$(tr -d '[:space:]' < version.md)"
if [[ ! "$MD" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "🔴 version.md não tem um X.Y.Z limpo: '$MD'" >&2
  exit 1
fi

TOML="$(sed -n 's/^version = "\([^"]*\)".*/\1/p' Cargo.toml | head -1)"
LOCK="$(awk '/^name = "anna"$/{getline; sub(/^version = "/,""); sub(/"$/,""); print; exit}' Cargo.lock)"

if [[ "${1:-}" == "--verificar" ]]; then
  falhou=0
  [[ "$TOML" == "$MD" ]] || { echo "🔴 Cargo.toml = $TOML, version.md = $MD" >&2; falhou=1; }
  [[ "$LOCK" == "$MD" ]] || { echo "🔴 Cargo.lock = $LOCK, version.md = $MD" >&2; falhou=1; }
  [[ $falhou -eq 0 ]] && echo "✓ os três portadores em $MD"
  exit $falhou
fi

# Escreve só a PRIMEIRA ocorrência de cada: no Cargo.toml é a do `[package]`, e no
# Cargo.lock é a linha logo abaixo de `name = "anna"` — nunca a de uma dependência.
sed -i "0,/^version = \"[^\"]*\"/s//version = \"$MD\"/" Cargo.toml
awk -v v="$MD" '
  /^name = "anna"$/ { print; getline; print "version = \"" v "\""; next }
  { print }
' Cargo.lock > Cargo.lock.tmp && mv Cargo.lock.tmp Cargo.lock

echo "✓ Cargo.toml e Cargo.lock alinhados em $MD (antes: $TOML e $LOCK)"
