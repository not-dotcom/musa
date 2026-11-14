# Musa — instrucciones para agentes

## Flujo de desarrollo

- Para revisar o probar cualquier cambio, usar **siempre el build debug**.
  No basta con `cargo check`: no genera ejecutable. Compila y ejecuta con:

  ```bash
  export PKG_CONFIG_PATH="$PWD/.deps/sysroot/usr/lib64/pkgconfig"
  cargo build --workspace   # binarios debug: target/debug/musa y target/debug/musad
  cargo run -p musa-ui      # compila debug y abre la UI
  ./scripts/musa-dev.sh     # compila debug y abre; es el «Musa Dev» de Fuzzel
  ```

- El binario release (`target/release/musa`) es el que lanza Fuzzel desde
  `~/.local/share/applications/io.github.juliogarcia.Musa.desktop`. No lo
  uses para revisar cambios en desarrollo.
- Cierra la instancia abierta antes de lanzar otra: la UI usa el mismo
  `application_id` y GTK reutilizaría la ventana existente en vez de arrancar
  la nueva.
- Al terminar una tarea, **preguntar al usuario si quiere hacer el build
  release** para actualizar la versión de Fuzzel. No ejecutarlo por cuenta
  propia:

  ```bash
  make build   # equivale a ./scripts/build.sh (release)
  ```

- **Antes de cualquier commit, ejecutar siempre el build release** y
  confirmar que termina sin errores:

  ```bash
  make build
  ```

## Referencia rápida

| Objetivo | Comando |
|---|---|
| Revisar cambios (debug) | `cargo run -p musa-ui` |
| Abrir Musa Dev desde Fuzzel | `./scripts/musa-dev.sh` |
| Binarios debug | `cargo build --workspace` |
| Actualizar Fuzzel (release) | `make build` |
| Formato | `cargo fmt --all -- --check` |
| Tests | `make test` |
