#!/usr/bin/env bash
# Descarga y extrae libadwaita-devel DENTRO del proyecto, sin sudo, cuando el
# sistema solo tiene la biblioteca en tiempo de ejecución. Es lo que ocurre en
# Fedora 44 con gtk4-devel instalado y libadwaita sin cabeceras.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SYSROOT="$ROOT/.deps/sysroot"
PREFIX="$SYSROOT/usr"
PC="$PREFIX/lib64/pkgconfig/libadwaita-1.pc"

if command -v pkg-config >/dev/null 2>&1 && pkg-config --exists libadwaita-1; then
    echo "libadwaita-1 del sistema disponible: $(pkg-config --modversion libadwaita-1)"
    exit 0
fi
if [[ -f "$PC" ]] && PKG_CONFIG_PATH="$PREFIX/lib64/pkgconfig" pkg-config --exists libadwaita-1; then
    echo "libadwaita-1 local ya preparado en $PREFIX"
    exit 0
fi
if ! command -v dnf >/dev/null 2>&1 || ! command -v rpm2cpio >/dev/null 2>&1; then
    echo "Falta dnf/rpm2cpio. Instala libadwaita-devel con tu gestor o extrae el RPM a mano." >&2
    exit 1
fi

mkdir -p "$ROOT/.deps/rpms" "$PREFIX"
cd "$ROOT/.deps/rpms"
echo "Descargando libadwaita-devel (sin instalarlo en el sistema)…"
dnf download --arch=x86_64 libadwaita-devel
RPM="$(ls -1t libadwaita-devel-*.x86_64.rpm | head -n1)"
rpm2cpio "$RPM" | cpio -idm --quiet -D "$SYSROOT"
# El .pc original apunta a /usr; lo redirigimos al prefijo del proyecto.
sed -i "s|^prefix=.*|prefix=$PREFIX|" "$PC"
echo "Listo. Usa:"
echo "  export PKG_CONFIG_PATH=\"$PREFIX/lib64/pkgconfig\${PKG_CONFIG_PATH:+:\$PKG_CONFIG_PATH}\""
PKG_CONFIG_PATH="$PREFIX/lib64/pkgconfig" pkg-config --modversion libadwaita-1 gtk4
