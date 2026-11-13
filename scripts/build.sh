#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PREFIX="$ROOT/.deps/sysroot/usr"
if ! PKG_CONFIG_PATH="$PREFIX/lib64/pkgconfig" pkg-config --exists libadwaita-1; then
    "$ROOT/scripts/setup-libadwaita.sh"
fi
export PKG_CONFIG_PATH="$PREFIX/lib64/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
exec cargo build --release "$@"
