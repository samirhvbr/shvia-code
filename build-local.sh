#!/usr/bin/env bash
# build-local.sh — Build LOCAL do `anna` (SHVIA-CODE) no macOS e Linux, sem CI.
# Binário único Rust. Preflight do toolchain + cronômetro por etapa (padrão da
# casa, SHVIA-DESKTOP 0.5.8).
#
# PRÉ-REQUISITOS (instalar uma vez):
#   Rust (rustup default stable). Opcional: cargo-audit
#     (cargo install cargo-audit) — gate de supply chain do SECURITY_GUIDELINES §4.
#
# USO (na raiz do repo):
#   ./build-local.sh            # build release + checksum SHA-256
#   ./build-local.sh --debug    # build debug (compila mais rápido)
#   ./build-local.sh --audit    # roda cargo audit ANTES (falha se houver vuln)
#
# Saída: target/release/anna (ou target/debug/anna) + .sha256 ao lado.
set -euo pipefail
cd "$(dirname "$0")"

# ── Cronômetro do build: total + por etapa (padrão SHVTERM/SHVIA-DESKTOP) ────
SECONDS=0
_BUILD_OS=$(uname -s); [ "$_BUILD_OS" = Darwin ] && _BUILD_OS=macOS
_PH_NAMES=(); _PH_TIMES=(); _PH_CUR=""; _PH_START=0
_fmt() { local t=$1
  if   [ "$t" -ge 3600 ]; then printf '%dh %02dm %02ds' $((t/3600)) $(((t%3600)/60)) $((t%60))
  elif [ "$t" -ge 60 ];   then printf '%dm %02ds' $((t/60)) $((t%60))
  else                         printf '%ds' "$t"; fi
}
step() { local now=$SECONDS
  if [ -n "$_PH_CUR" ]; then _PH_NAMES+=("$_PH_CUR"); _PH_TIMES+=($((now - _PH_START)))
  elif [ "$now" -gt 0 ]; then _PH_NAMES+=("preparação"); _PH_TIMES+=("$now"); fi
  _PH_CUR="$1"; _PH_START=$now
  echo "==> [$(_fmt "$now")] $1"
}
_summary() {
  if [ -n "$_PH_CUR" ]; then _PH_NAMES+=("$_PH_CUR"); _PH_TIMES+=($((SECONDS - _PH_START))); _PH_CUR=""; fi
  echo ""; echo "⏱  tempo por etapa ($_BUILD_OS):"
  local i; for i in "${!_PH_NAMES[@]}"; do printf '     %8s  %s\n' "$(_fmt "${_PH_TIMES[$i]}")" "${_PH_NAMES[$i]}"; done
  echo "     ────────"; printf '     %8s  TOTAL\n' "$(_fmt "$SECONDS")"
}
_on_exit() { local code=$?; if [ "$code" -ne 0 ]; then
  echo "" >&2; echo "❌ build abortou após $(_fmt "$SECONDS")  ($_BUILD_OS, exit $code)" >&2; fi; }
trap _on_exit EXIT

usage() { sed -n '2,18p' "$0" | sed 's/^# \{0,1\}//'; }

# ── Preflight: falha em <1s com mensagem acionável (padrão SHVIA-DESKTOP) ────
preflight() {
  local missing=()
  if ! command -v cargo >/dev/null 2>&1 && [ -x "$HOME/.cargo/bin/cargo" ]; then
    export PATH="$HOME/.cargo/bin:$PATH"
    echo "    (cargo achado em ~/.cargo/bin — adicionado ao PATH; 'source \$HOME/.cargo/env' pra fixar)"
  fi
  if ! command -v cargo >/dev/null 2>&1 || ! command -v rustc >/dev/null 2>&1; then
    missing+=(
"Rust (cargo) não encontrado — instale:
       curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
       source \"\$HOME/.cargo/env\"   (ou reabra o terminal)"
    )
  fi
  # macOS: linker da Apple (o rustup precisa dele).
  if [ "$_BUILD_OS" = macOS ] && ! xcode-select -p >/dev/null 2>&1; then
    missing+=("Command Line Tools do Xcode ausentes. Instale: xcode-select --install")
  fi
  if [ "${#missing[@]}" -gt 0 ]; then
    echo "" >&2; echo "❌ pré-requisitos faltando ($_BUILD_OS) — corrija e rode de novo:" >&2; echo "" >&2
    local m; for m in "${missing[@]}"; do echo "   • $m" >&2; echo "" >&2; done
    exit 1
  fi
  # Coerência de versão: Cargo.toml deve bater com version.md (fonte da verdade).
  local vmd vcargo
  vmd=$(tr -d ' \t\r\n' < version.md)
  vcargo=$(sed -n 's/^version = "\(.*\)".*/\1/p' Cargo.toml | head -1)
  if [ "$vmd" != "$vcargo" ]; then
    echo "⚠  version.md ($vmd) ≠ Cargo.toml ($vcargo) — sincronize antes de release." >&2
  fi
}

PROFILE_FLAG="--release"; OUTDIR="release"; RUN_AUDIT=0
while [ $# -gt 0 ]; do
  case "$1" in
    --debug) PROFILE_FLAG=""; OUTDIR="debug" ;;
    --audit) RUN_AUDIT=1 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "opção desconhecida: $1 (use --help)" >&2; exit 2 ;;
  esac; shift
done

echo "==> anna (SHVIA-CODE) — build local ($_BUILD_OS)"

step "[pré-requisitos] verifica o toolchain (Rust, Xcode) e a versão"
preflight

if [ "$RUN_AUDIT" -eq 1 ]; then
  step "[audit] cargo audit (supply chain — SECURITY §4)"
  if command -v cargo-audit >/dev/null 2>&1; then cargo audit
  else echo "    (cargo-audit não instalado: 'cargo install cargo-audit' — pulando)"; fi
fi

step "[1/1] cargo build $PROFILE_FLAG"
# shellcheck disable=SC2086
cargo build $PROFILE_FLAG

BIN="target/$OUTDIR/anna"
echo ""
if [ -f "$BIN" ]; then
  # Checksum publicável junto do binário (SECURITY §4).
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$BIN" | tee "$BIN.sha256"
  elif command -v shasum >/dev/null 2>&1; then shasum -a 256 "$BIN" | tee "$BIN.sha256"; fi
  echo "[OK] binário: $BIN"
  ls -lh "$BIN"
fi
_summary
