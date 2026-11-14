# Musa sidecar (Phase 2)

Hidden Electron window with `music.apple.com`, MusicKit and Widevine. Audio goes
out through PipeWire; the daemon talks to this process using
**newline-delimited JSON over stdin/stdout** and never passes it Apple
credentials.

## Status on this machine

- **Widevine validated** on 2026-11-12 with the castLabs `electron 44.1.0` build
  already installed by another application. This `main.js` was run in read-only
  mode: `navigator.requestMediaKeySystemAccess('com.widevine.alpha')` returned
  `true`, `music.apple.com/us/new` loaded, `state` responded and `shutdown`
  terminated the process.
- The user **signed in** on 2026-11-12. The window opens at
  `https://music.apple.com/mx/home`, with `widevine:true` and no sign-in screen.
  The session is stored in `~/.config/musa/sidecar/` (never in the repo); the
  session cache lives in `~/.cache/musa/sidecar/`. Real audio and hooking up the
  native UI (Phase 3) are still pending.
- The normal Electron distribution does not include Widevine. The castLabs
  **`wvcus`** build is required, as it adds the CDM and VMP.

Validation command used (without installing anything):

```bash
ELECTRON=~/.local/share/slipmat/sidecar/node_modules/electron/dist/electron
( sleep 18
  printf '%s\n' '{"v":1,"id":2,"cmd":"state"}'
  sleep 2
  printf '%s\n' '{"v":1,"id":3,"cmd":"shutdown"}'
) | timeout 45 "$ELECTRON" sidecar/main.js --no-sandbox \
      --ozone-platform=wayland --disable-gpu --show-login-window
```

## Planned installation

```bash
cd sidecar
npm install   # downloads the castLabs build declared in package.json
npm test
npm start
```

If `npm install` can't resolve the castLabs URL, download the
`electron-vX.Y.Z-wvcus-linux-x64.zip` release from
<https://github.com/castlabs/electron-releases/releases> by hand, unpack it into
`sidecar/electron/` and run `sidecar/electron/electron sidecar/`.

## Protocol

Input: `{"v":1,"id":1,"cmd":"play","params":{}}`.
Output: `{"v":1,"id":1,"ok":true,"data":{...}}` or
`{"v":1,"id":1,"ok":false,"error":{"code":"...","message":"..."}}`.
Events: `engine.ready`, `engine.error`, `engine.idle`, `engine.stopped`.

Commands: `hello`, `show-login-window`, `session`, `library`, `search`,
`catalog-album`, `catalog-artist`, `catalog-playlist`, `follow-artist`, `add-album`, `lyrics`,
`set-queue` (`song_ids`, `index`, `play`), `play`, `pause`, `next`, `prev`,
`seek` (`position_ms`), `shuffle`, `repeat`, `volume` (0-100), `state`,
`shutdown`.

`set-queue` is tolerant of removed songs: if MusicKit answers `NOT_FOUND`
because some catalog id no longer exists, the sidecar drops those tracks and
retries with the rest, instead of leaving the whole list unplayable.

`follow-artist` tries to add a catalog artist to the Apple Music library with
`POST /me/favorites?ids[artists]=…` and, if that fails, with
`POST /me/library?ids[artists]=…`. It is best-effort: the daemon stores the
artist locally even if Apple ignores it.

`add-album` adds a record with `POST /me/library?ids[albums]=…`. It is also
best-effort and the daemon keeps the album and its songs locally.

`library` reads the library with the tokens the Apple Music page already uses
(developer token + Music User Token) and returns the Musa protocol snapshot. It
downloads artwork to `~/.cache/musa/real-covers/` and caches the snapshot in
`~/.cache/musa/real-library.json`. It never prints or stores tokens.

`session` returns non-sensitive data only: URL, title, `music_kit`,
`authorized` and storefront. With the user's session it answered:

```json
{"url":"https://music.apple.com/mx/home","music_kit":true,"authorized":true,"storefront":"mx"}
```

The sidecar shuts itself down after **90 s** without commands
(`IDLE_SHUTDOWN_MS`) and emits `engine.idle`. The daemon decides when to resume
it; on resume it restores the queue without playing.

## Real integration

Track selection and playback depend on Apple's DOM/MusicKit, which changes
without warning. The daemon opens the login window if needed and, once the
session is authorized, loads the library and plays through PipeWire. The manual
tests are described in the «First session» section of the main README.
