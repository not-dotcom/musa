#!/usr/bin/env bash
# Mediciones reproducibles de PSS. Escribe los informes en docs/mediciones/.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
"$ROOT/scripts/build.sh"
OUT="$ROOT/docs/mediciones"
mkdir -p "$OUT"
SOCKET="${MUSA_SOCKET:-/tmp/musa-medicion-$$.sock}"
export MUSA_SOCKET="$SOCKET"
export MUSA_NO_AUTOSTART=1
"$ROOT/target/release/musad" --socket "$SOCKET" >/tmp/musa-medicion-daemon.log 2>&1 &
DAEMON=$!
trap 'kill "$DAEMON" 2>/dev/null || true; rm -f "$SOCKET"' EXIT
for _ in $(seq 1 100); do [[ -S "$SOCKET" ]] && break; sleep 0.1; done

echo "Piso de ventana vacía (sin biblioteca):"
for renderer in cairo ngl vulkan; do
    GSK_RENDERER="$renderer" "$ROOT/target/release/musa-empty" --seconds 6 \
        | tee "$OUT/empty-$renderer.log"
done

for mode in library playing paused; do
    GSK_RENDERER=ngl "$ROOT/target/release/musa" --measure "$mode" \
        2>&1 | tee "$OUT/$mode-ngl.log" | grep MEDICION || true
done
GSK_RENDERER=ngl "$ROOT/target/release/musa" --measure scroll \
    2>&1 | tee "$OUT/scroll-ngl.log" | grep MEDICION || true
GSK_RENDERER=cairo "$ROOT/target/release/musa" --measure scroll \
    2>&1 | tee "$OUT/scroll-cairo.log" | grep MEDICION || true
