// Sidecar de Musa. Se comunica con musad por líneas JSON en stdin/stdout y
// mantiene una ventana Electron oculta con music.apple.com. Este archivo es
// deliberadamente pequeño: la lógica de la cola y del protocolo está en Rust.
//
// Requiere la distribución de Electron de castLabs (Widevine, VMP) — ver
// README.md. La sesión persistente vive en ~/.config/musa/sidecar/.

'use strict';

const { app, BrowserWindow, session } = require('electron');
const path = require('node:path');
const os = require('node:os');
const readline = require('node:readline');
const apple = require('./apple');

// Rutas permitidas: nada de escribir en ~/.config/Slipmat ni en ~/.config/Electron.
app.setPath('userData', path.join(os.homedir(), '.config/musa/sidecar'));
app.setPath('sessionData', path.join(os.homedir(), '.cache/musa/sidecar'));

const IDLE_SHUTDOWN_MS = 90_000;
const PARTITION = 'persist:musa-apple';

let window = null;
let idleTimer = null;
let lastState = { status: 'stopped', position_ms: 0, duration_ms: 0, track: null };

function send(message) {
  process.stdout.write(JSON.stringify(message) + '\n');
}

function event(name, data) {
  send({ v: 1, event: name, data });
}

function touch() {
  if (idleTimer) clearTimeout(idleTimer);
  idleTimer = setTimeout(() => {
    // Con la ventana de login visible no apagamos: el usuario puede tardar.
    if (window && window.isVisible()) {
      touch();
      return;
    }
    event('engine.idle', { idle_ms: IDLE_SHUTDOWN_MS });
    app.quit();
  }, IDLE_SHUTDOWN_MS);
}

async function createWindow() {
  window = new BrowserWindow({
    show: false,
    width: 1280,
    height: 800,
    webPreferences: {
      partition: PARTITION,
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false,
      autoplayPolicy: 'no-user-gesture-required',
      backgroundThrottling: false,
    },
  });
  window.webContents.setAudioMuted(false);
  window.webContents.on('did-finish-load', async () => {
    let widevine = false;
    try {
      widevine = await window.webContents.executeJavaScript(`
        navigator.requestMediaKeySystemAccess('com.widevine.alpha', [{
          initDataTypes: ['cenc'],
          audioCapabilities: [{ contentType: 'audio/mp4; codecs="mp4a.40.2"' }],
          videoCapabilities: [{ contentType: 'video/mp4; codecs="avc1.42E01E"' }]
        }]).then(() => true).catch(() => false)
      `, true);
    } catch (error) {
      event('engine.widevine_error', { message: String(error) });
    }
    event('engine.ready', {
      url: window.webContents.getURL(),
      widevine,
      electron: process.versions.electron,
    });
  });
  window.webContents.on('did-fail-load', (_event, code, description, url) => {
    event('engine.load_error', { code, description, url });
  });
  window.on('close', (event) => {
    // Cerrar la ventana de login no debe matar el motor: se oculta y puede
    // volver a mostrarse si la sesión sigue sin autorizar.
    event.preventDefault();
    window.hide();
  });
  await window.loadURL('https://music.apple.com/');
  // La ventana queda oculta; el daemon la muestra para el login si hace falta.
}

async function musicState() {
  return window.webContents.executeJavaScript(`
    (() => {
      const m = window.MusicKit && window.MusicKit.getInstance();
      if (!m) return { ready: false };
      const q = m.queue || {};
      const items = (q.items || [])
        .map((i) => (i && i.playParams && i.playParams.catalogId) || (i && i.id) || null)
        .filter(Boolean)
        .map(String);
      const item = m.nowPlayingItem || null;
      const attrs = (item && item.attributes) || {};
      const pp = attrs.playParams || {};
      const names = ['none','loading','playing','paused','stopped','ended','seeking','unknown','waiting','stalled','completed'];
      const num = typeof m.playbackState === 'number' ? m.playbackState : 0;
      const dur = attrs.durationInMillis || Math.round((m.currentPlaybackDuration || 0) * 1000);
      return {
        ready: true,
        playing: num === 2 || m.isPlaying === true,
        status: names[num] || 'unknown',
        position_ms: Math.round((m.currentPlaybackTime || 0) * 1000),
        duration_ms: Math.round((m.currentPlaybackDuration || 0) * 1000),
        queue: items,
        index: Number.isFinite(q.position) && q.position >= 0 ? q.position : 0,
        shuffle: m.shuffleMode === 1,
        repeat: ['off', 'one', 'all'][m.repeatMode] || 'off',
        volume: typeof m.volume === 'number' ? m.volume : 1,
        track: item ? {
          id: String(pp.catalogId || item.id || ''),
          title: attrs.name || item.title || '',
          artist_name: attrs.artistName || item.artistName || '',
          album_title: attrs.albumName || item.albumName || '',
          duration_ms: dur,
          cover_url: (attrs.artwork && attrs.artwork.url) || null,
        } : null,
      };
    })()
  `, true);
}

async function musicCommand(action, params = {}) {
  const payload = JSON.stringify({ action, params });
  return window.webContents.executeJavaScript(`
    (async () => {
      const m = window.MusicKit && window.MusicKit.getInstance();
      if (!m) throw new Error('MusicKit no está listo');
      const { action, params } = ${payload};
      switch (action) {
        case 'set-queue':
          await m.setQueue({
            songs: params.song_ids,
            startWith: params.index || 0,
            startPosition: params.index || 0,
            startPlaying: params.play !== false,
          });
          break;
        case 'play': await m.play(); break;
        case 'pause': m.pause(); break;
        case 'next': m.skipToNextItem(); break;
        case 'prev': m.skipToPreviousItem(); break;
        case 'jump': m.changeToMediaAtIndex(params.index); break;
        case 'seek': m.seekToTime((params.position_ms || 0) / 1000); await new Promise((r) => setTimeout(r, 350)); break;
        case 'shuffle': m.shuffleMode = params.enabled ? 1 : 0; break;
        case 'repeat': m.repeatMode = params.mode === 'one' ? 1 : params.mode === 'all' ? 2 : 0; break;
        case 'volume': m.volume = Math.max(0, Math.min(1, (params.volume || 0) / 100)); break;
        default: throw new Error('acción desconocida: ' + action);
      }
      return true;
    })()
  `, true);
}

async function execute(command, params) {
  if (!window) throw new Error('ventana no inicializada');
  const js = `
    (async () => {
      const audio = document.querySelector('audio');
      switch (${JSON.stringify(command)}) {
        case 'play': if (audio) await audio.play(); break;
        case 'pause': if (audio) audio.pause(); break;
        case 'seek': if (audio) audio.currentTime = ${Number(params.position_ms || 0)} / 1000; break;
        case 'volume': if (audio) audio.volume = ${Number(params.volume ?? 80)} / 100; break;
        case 'state': {
          if (!audio) return { status: 'stopped', position_ms: 0, duration_ms: 0 };
          return {
            status: audio.paused ? 'paused' : 'playing',
            position_ms: Math.round(audio.currentTime * 1000),
            duration_ms: Math.round((audio.duration || 0) * 1000),
          };
        }
        default: return { ok: true };
      }
      return { ok: true };
    })()
  `;
  return window.webContents.executeJavaScript(js, true);
}

async function handle(request) {
  const { id, cmd, params = {} } = request;
  try {
    let data;
    switch (cmd) {
      case 'hello':
        data = {
          engine: 'electron-wvcus',
          electron: process.versions.electron,
          chrome: process.versions.chrome,
          user_data: app.getPath('userData'),
          idle_shutdown_ms: IDLE_SHUTDOWN_MS,
        };
        break;
      case 'show-login-window':
        window.show();
        window.focus();
        data = { shown: true };
        break;
      case 'hide-login-window':
        window.hide();
        data = { hidden: true };
        break;
      case 'library':
        data = await apple.buildLibrary(
          window,
          path.join(os.homedir(), '.cache/musa'),
          Boolean(params.refresh),
        );
        break;
      case 'lyrics':
        data = await apple.lyrics(window, params.song_id);
        break;
      case 'set-queue':
        await musicCommand('set-queue', params);
        data = await musicState();
        break;
      case 'player':
        await musicCommand(params.action, params);
        data = await musicState();
        break;
      case 'play':
      case 'pause':
      case 'next':
      case 'prev':
      case 'seek':
      case 'shuffle':
      case 'repeat':
      case 'volume':
      case 'jump':
        await musicCommand(cmd, params);
        data = await musicState();
        break;
      case 'state':
        data = await musicState();
        break;
      case 'session': {
        // Diagnóstico sin secretos: confirma que MusicKit existe y si la
        // sesión está autorizada. No devuelve tokens ni cookies.
        data = await window.webContents.executeJavaScript(`
          (() => {
            const m = window.MusicKit && window.MusicKit.getInstance();
            return {
              url: location.href,
              title: document.title,
              music_kit: Boolean(m),
              authorized: Boolean(m && m.isAuthorized),
              storefront: (m && m.storefrontId) || null,
            };
          })()
        `, true);
        break;
      }
      case 'shutdown':
        data = { bye: true };
        send({ v: 1, id, ok: true, data });
        app.quit();
        return;
      default:
        throw new Error(`comando desconocido: ${cmd}`);
    }
    touch();
    lastState = data && data.status ? data : lastState;
    send({ v: 1, id, ok: true, data });
  } catch (error) {
    send({ v: 1, id, ok: false, error: { code: 'sidecar_error', message: String(error) } });
  }
}

app.commandLine.appendSwitch('autoplay-policy', 'no-user-gesture-required');
// Mantener viva la página aunque la ventana esté oculta (audio real).
app.commandLine.appendSwitch('disable-renderer-backgrounding');
app.commandLine.appendSwitch('disable-background-timer-throttling');
app.commandLine.appendSwitch('disable-backgrounding-occluded-windows');
app.whenReady().then(async () => {
  touch();
  try {
    await createWindow();
    if (process.argv.includes('--show-login-window')) {
      window.show();
      window.focus();
    }
  } catch (error) {
    event('engine.error', { message: String(error) });
  }
  const rl = readline.createInterface({ input: process.stdin, terminal: false });
  rl.on('line', (line) => {
    if (!line.trim()) return;
    try {
      handle(JSON.parse(line));
    } catch (error) {
      event('engine.error', { message: `JSON inválido: ${error}` });
    }
  });
});

app.on('before-quit', () => {
  if (idleTimer) clearTimeout(idleTimer);
  event('engine.stopped', { state: lastState });
});
