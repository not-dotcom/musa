#!/usr/bin/env bash
# Abre la ventana visible del sidecar para iniciar sesión en Apple Music.
# La sesión se guarda en ~/.config/musa/sidecar/ (nunca en el repo).
# Musa abre esta misma ventana automáticamente si no hay sesión.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

find_electron() {
    if [[ -n "${MUSA_ELECTRON:-}" && -x "$MUSA_ELECTRON" ]]; then
        echo "$MUSA_ELECTRON"; return 0
    fi
    local candidates=(
        "$ROOT/sidecar/node_modules/electron/dist/electron"
        "$HOME/.local/share/slipmat/sidecar/node_modules/electron/dist/electron"
    )
    local c
    for c in "${candidates[@]}"; do
        [[ -x "$c" ]] && { echo "$c"; return 0; }
    done
    command -v electron 2>/dev/null
}

ELECTRON="$(find_electron || true)"
if [[ -z "$ELECTRON" ]]; then
    cat >&2 <<'MSG'
No encontré un Electron castLabs (Widevine).
Instálalo con:
  cd sidecar && npm install
o define MUSA_ELECTRON=/ruta/al/electron
MSG
    exit 1
fi

cd "$ROOT"
echo "Abriendo music.apple.com con Electron: $ELECTRON"
echo "Inicia sesión en la ventana de Apple. La sesión queda en ~/.config/musa/sidecar/"
echo "Cierra esa ventana cuando termines; el proceso se apagará solo."
exec "$ELECTRON" sidecar/main.js --no-sandbox --disable-gpu --show-login-window
