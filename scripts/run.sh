#!/usr/bin/env bash
# Arranca el daemon real y la UI. La UI también sabe arrancar el daemon por su
# cuenta; este script es útil para ver los logs del daemon en primer plano.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
"$ROOT/scripts/build.sh"
SOCKET="${MUSA_SOCKET:-${XDG_CACHE_HOME:-$HOME/.cache}/musa/musad.sock}"
mkdir -p "$(dirname "$SOCKET")"
"$ROOT/target/release/musad" --socket "$SOCKET" &
DAEMON=$!
trap 'kill "$DAEMON" 2>/dev/null || true' EXIT
MUSA_SOCKET="$SOCKET" MUSA_NO_AUTOSTART=1 "$ROOT/target/release/musa" "$@"
