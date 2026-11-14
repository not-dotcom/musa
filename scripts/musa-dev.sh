#!/usr/bin/env bash
# Lanza la versión debug de Musa: compila el workspace y abre target/debug/musa.
# Pensado para el lanzador «Musa Dev» de Fuzzel. La versión release no se toca.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PREFIX="$ROOT/.deps/sysroot/usr"
CACHE="${XDG_CACHE_HOME:-$HOME/.cache}/musa"
LOG="$CACHE/musa-dev.log"
mkdir -p "$CACHE"

# GTK reutiliza la ventana de la misma application_id: si ya hay una instancia
# abierta (por ejemplo la release de Fuzzel), el proceso nuevo se cerraría sin
# mostrar nada. Es mejor avisar y no confundir.
if pgrep -x musa >/dev/null 2>&1; then
    notify-send "Musa Dev" "Ya hay una instancia de Musa abierta. Ciérrala y vuelve a intentarlo." 2>/dev/null || true
    exit 0
fi

export PKG_CONFIG_PATH="$PREFIX/lib64/pkgconfig"
cd "$ROOT"

# El lanzador puede heredar un PATH sin ~/.cargo/bin (Fuzzel arrancado desde el
# compositor). Busca cargo en los sitios habituales antes de rendirse.
CARGO="$(command -v cargo 2>/dev/null || true)"
if [[ -z "$CARGO" && -x "$HOME/.cargo/bin/cargo" ]]; then
    CARGO="$HOME/.cargo/bin/cargo"
fi
if [[ -z "$CARGO" ]]; then
    notify-send -u critical "Musa Dev: falta cargo" "Instala Rust o añade ~/.cargo/bin al PATH." 2>/dev/null || true
    exit 1
fi

if ! "$CARGO" build --workspace >"$LOG" 2>&1; then
    notify-send -u critical "Musa Dev: falló la compilación" "Revisa $LOG" 2>/dev/null || true
    exit 1
fi

"$ROOT/target/debug/musa" "$@" >>"$LOG" 2>&1 || {
    status=$?
    notify-send -u critical "Musa Dev: no se pudo abrir" "Revisa $LOG" 2>/dev/null || true
    exit "$status"
}
