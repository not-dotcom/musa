#!/usr/bin/env bash
# Instala el lanzador de Musa para Fuzzel y otros lanzadores XDG.
# No necesita sudo. Escribe en ~/.local/share (datos de aplicaciones del usuario).
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$ROOT/target/release/musa"
[[ -x "$BIN" ]] || { echo "Falta $BIN; ejecuta antes: make build" >&2; exit 1; }

DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
APPS="$DATA/applications"
ICONS="$DATA/icons/hicolor"
mkdir -p "$APPS" "$ICONS/scalable/apps" "$ICONS/128x128/apps" "$ICONS/256x256/apps"

sed "s|^Exec=musa$|Exec=$BIN|" \
    "$ROOT/data/io.github.juliogarcia.Musa.desktop" \
    > "$APPS/io.github.juliogarcia.Musa.desktop"
cp "$ROOT/data/io.github.juliogarcia.Musa.svg" \
   "$ICONS/scalable/apps/io.github.juliogarcia.Musa.svg"

if command -v magick >/dev/null 2>&1; then
    magick -background none "$ROOT/data/io.github.juliogarcia.Musa.svg" \
        -resize 128x128 "$ICONS/128x128/apps/io.github.juliogarcia.Musa.png"
    magick -background none "$ROOT/data/io.github.juliogarcia.Musa.svg" \
        -resize 256x256 "$ICONS/256x256/apps/io.github.juliogarcia.Musa.png"
elif command -v convert >/dev/null 2>&1; then
    convert -background none "$ROOT/data/io.github.juliogarcia.Musa.svg" \
        -resize 128x128 "$ICONS/128x128/apps/io.github.juliogarcia.Musa.png"
    convert -background none "$ROOT/data/io.github.juliogarcia.Musa.svg" \
        -resize 256x256 "$ICONS/256x256/apps/io.github.juliogarcia.Musa.png"
fi

command -v update-desktop-database >/dev/null 2>&1 && \
    update-desktop-database "$APPS" >/dev/null 2>&1 || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && \
    gtk-update-icon-cache -f -t "$ICONS" >/dev/null 2>&1 || true

echo "Listo: $APPS/io.github.juliogarcia.Musa.desktop"
echo "Abre Fuzzel (Mod+D), escribe «Musa» y pulsa Enter."
