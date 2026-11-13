# Sidecar de Musa (Fase 2)

Ventana Electron oculta con `music.apple.com`, MusicKit y Widevine. El audio
sale por PipeWire; el daemon habla con este proceso por **JSON por líneas en
stdin/stdout** y nunca le pasa credenciales de Apple.

## Estado en esta máquina

- **Widevine validado** el 2026-11-12 con la build castLabs `electron 44.1.0`
  ya instalada por otra aplicación. Se ejecutó este `main.js` en modo lectura:
  `navigator.requestMediaKeySystemAccess('com.widevine.alpha')` devolvió
  `true`, `music.apple.com/us/new` cargó, `state` respondió y `shutdown` terminó
  el proceso.
- El usuario **inició sesión** el 2026-11-12. La ventana abre en
  `https://music.apple.com/mx/home`, con `widevine:true` y sin pantalla de
  acceso. La sesión se guarda en `~/.config/musa/sidecar/` (nunca en el repo);
  la caché de sesión, en `~/.cache/musa/sidecar/`. Falta probar audio real y
  conectar la UI nativa (Fase 3).
- La distribución normal de Electron no incluye Widevine. Hace falta la build
  **castLabs `wvcus`**, que añade el CDM y VMP.

Comando de validación usado (sin instalar nada):

```bash
ELECTRON=~/.local/share/slipmat/sidecar/node_modules/electron/dist/electron
( sleep 18
  printf '%s\n' '{"v":1,"id":2,"cmd":"state"}'
  sleep 2
  printf '%s\n' '{"v":1,"id":3,"cmd":"shutdown"}'
) | timeout 45 "$ELECTRON" sidecar/main.js --no-sandbox \
      --ozone-platform=wayland --disable-gpu --show-login-window
```

## Instalación prevista

```bash
cd sidecar
npm install   # baja la build de castLabs declarada en package.json
npm test
npm start
```

Si `npm install` no resuelve la URL de castLabs, descarga manualmente el
release `electron-vX.Y.Z-wvcus-linux-x64.zip` desde
<https://github.com/castlabs/electron-releases/releases>, descomprímelo en
`sidecar/electron/` y ejecuta `sidecar/electron/electron sidecar/`.

## Protocolo

Entrada: `{"v":1,"id":1,"cmd":"play","params":{}}`.
Salida: `{"v":1,"id":1,"ok":true,"data":{...}}` o
`{"v":1,"id":1,"ok":false,"error":{"code":"...","message":"..."}}`.
Eventos: `engine.ready`, `engine.error`, `engine.idle`, `engine.stopped`.

Comandos: `hello`, `show-login-window`, `session`, `library`, `lyrics`,
`set-queue` (`song_ids`, `index`, `play`), `play`, `pause`, `next`, `prev`,
`seek` (`position_ms`), `shuffle`, `repeat`, `volume` (0-100), `state`,
`shutdown`.

`library` lee la biblioteca con los tokens que ya usa la página de Apple Music
(developer token + Music User Token) y devuelve el snapshot del protocolo Musa.
Descarga las portadas a `~/.cache/musa/real-covers/` y cachea el snapshot en
`~/.cache/musa/real-library.json`. No imprime ni guarda tokens.

`session` devuelve solo datos no sensibles: URL, título, `music_kit`,
`authorized` y storefront. Con la sesión del usuario respondió:

```json
{"url":"https://music.apple.com/mx/home","music_kit":true,"authorized":true,"storefront":"mx"}
```

El sidecar se apaga solo tras **90 s** sin órdenes (`IDLE_SHUTDOWN_MS`) y emite
`engine.idle`. El daemon es quien decide reanudarlo; al reanudar restaura la
cola sin reproducir.

## Integración real

La selección de pista y la reproducción dependen del DOM/MusicKit de Apple, que
cambia sin aviso. El daemon abre la ventana de login si hace falta y, una vez
autorizada la sesión, carga la biblioteca y reproduce a través de PipeWire. Las
pruebas manuales están descritas en la sección «Primera sesión» del README
principal.
