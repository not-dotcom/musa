// Acceso a la API de Apple Music desde el contexto ya autenticado del sidecar.
// No imprime ni persiste tokens: los lee de MusicKit.getInstance() y los usa
// solo en memoria para estas peticiones.

'use strict';

const crypto = require('node:crypto');
const fs = require('node:fs');
const fsp = fs.promises;
const path = require('node:path');

const API = 'https://api.music.apple.com/v1';
const ORIGIN = 'https://music.apple.com';
const REFERER = 'https://music.apple.com/';

function pick(...getters) {
  for (const get of getters) {
    try {
      const v = get();
      if (v !== undefined && v !== null && v !== '') return v;
    } catch { /* probar la siguiente forma */ }
  }
  return null;
}

async function tokens(win) {
  const t = await win.webContents.executeJavaScript(`
    (() => {
      const m = window.MusicKit && window.MusicKit.getInstance();
      return {
        dev: m && (m.developerToken || (m.api && m.api.developerToken)),
        user: m && (m.musicUserToken || (m.api && m.api.userToken)),
        store: (m && (m.storefrontId || (m.api && m.api.storefrontId))) || 'us',
        authorized: !!(m && m.isAuthorized),
      };
    })()
  `, true);
  if (!t || !t.dev) throw new Error('MusicKit no expuso el developer token');
  if (!t.authorized || !t.user) throw new Error('la sesión de Apple Music no está autorizada');
  return t;
}

async function apiJson(t, url) {
  const full = url.startsWith('http')
    ? url
    : url.startsWith('/v1/')
      ? 'https://api.music.apple.com' + url
      : API + url;
  const res = await fetch(full, { signal: AbortSignal.timeout(20000),
    headers: {
      Authorization: `Bearer ${t.dev}`,
      'Music-User-Token': t.user,
      Accept: 'application/json',
      Origin: ORIGIN,
      Referer: REFERER,
    },
  });
  if (!res.ok) {
    const body = await res.text().catch(() => '');
    throw new Error(`Apple Music ${res.status} en ${full}: ${body.slice(0, 300)}`);
  }
  return res.json();
}

async function apiAll(t, url, max = 10000) {
  const out = [];
  let next = url;
  while (next && out.length < max) {
    const page = await apiJson(t, next);
    out.push(...(page.data || []));
    next = page.next || null;
    process.stderr.write(`[apple] pagina ${url}: ${(page.data || []).length}, total ${out.length}, next=${next ? 'si' : 'no'}\n`);
  }
  return out;
}

// La API de biblioteca devuelve páginas de 25 aunque se pidan 100. Pedir seis
// offsets en paralelo convierte una carga de minutos en segundos.
async function apiAllParallel(t, pathWithQuery, max = 10000) {
  const [base, query = ''] = pathWithQuery.split('?');
  const params = new URLSearchParams(query);
  if (!params.has('limit')) params.set('limit', '100');
  const first = await apiJson(t, pathWithQuery);
  const out = [...(first.data || [])];
  const pageSize = Math.max(1, (first.data || []).length);
  if (!first.next || !first.data || first.data.length === 0) return out;
  let offset = pageSize;
  while (offset < max) {
    const offsets = [];
    for (let i = 0; i < 6 && offset + i * pageSize < max; i++) offsets.push(offset + i * pageSize);
    if (!offsets.length) break;
    const pages = await Promise.all(offsets.map((o) => {
      const p = new URLSearchParams(params);
      p.set('offset', String(o));
      return apiJson(t, `${base}?${p.toString()}`).catch(() => ({ data: [] }));
    }));
    let short = false;
    for (const page of pages) {
      const data = page.data || [];
      out.push(...data);
      if (data.length < pageSize) short = true;
    }
    if (short) break;
    offset += offsets.length * pageSize;
    process.stderr.write(`[apple] ${base}: ${out.length} elementos…\n`);
  }
  return out.slice(0, max);
}

async function readCache(file, maxAgeMs) {
  try {
    const st = await fsp.stat(file);
    if (Date.now() - st.mtimeMs < maxAgeMs) return JSON.parse(await fsp.readFile(file, 'utf8'));
  } catch { /* sin caché */ }
  return null;
}

function hash(text, prefix) {
  const h = crypto.createHash('sha1').update(String(text)).digest('hex').slice(0, 16);
  return `${prefix}-${h}`;
}

function yearOf(date) {
  const m = /^(\d{4})/.exec(date || '');
  return m ? Number(m[1]) : 0;
}

function coverUrl(attrs) {
  const url = attrs && attrs.artwork && attrs.artwork.url;
  if (!url) return null;
  return url.replace(/\{w\}/g, '600').replace(/\{h\}/g, '600');
}

async function mapLimit(items, limit, worker) {
  const result = new Array(items.length);
  let next = 0;
  const runners = Array.from({ length: Math.min(limit, items.length) }, async () => {
    while (true) {
      const i = next++;
      if (i >= items.length) return;
      result[i] = await worker(items[i], i);
    }
  });
  await Promise.all(runners);
  return result;
}

async function downloadCovers(urls, cacheRoot) {
  const dir = path.join(cacheRoot, 'real-covers');
  await fsp.mkdir(dir, { recursive: true });
  const unique = [...new Set(urls.filter(Boolean))];
  const map = new Map();
  await mapLimit(unique, 8, async (url) => {
    const file = path.join(dir, crypto.createHash('sha1').update(url).digest('hex') + '.jpg');
    if (!fs.existsSync(file)) {
      try {
        const res = await fetch(url, { signal: AbortSignal.timeout(15000) });
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const buf = Buffer.from(await res.arrayBuffer());
        if (buf.length < 1000) throw new Error('imagen vacía');
        const tmp = file + '.tmp';
        await fsp.writeFile(tmp, buf);
        await fsp.rename(tmp, file);
      } catch (error) {
        return null;
      }
    }
    map.set(url, file);
    return file;
  });
  return map;
}

function songFrom(attrs, rawId) {
  const pp = attrs.playParams || {};
  const id = pp.catalogId || pp.id || rawId;
  if (!id) return null;
  const artistName = attrs.artistName || 'Artista desconocido';
  const albumTitle = attrs.albumName || 'Álbum desconocido';
  return {
    id: String(id),
    title: attrs.name || 'Sin título',
    artist_id: hash(artistName, 'artist-lib'),
    artist_name: artistName,
    album_id: hash(`${artistName}\u0000${albumTitle}`, 'album-lib'),
    album_title: albumTitle,
    track_number: attrs.trackNumber || 1,
    disc_number: attrs.discNumber || 1,
    duration_ms: attrs.durationInMillis || 0,
    cover_path: '',
    favorite: attrs.inFavorites === true,
    playable: true,
    explicit: false,
    _artwork: coverUrl(attrs),
    _genre: (attrs.genreNames && attrs.genreNames[0]) || 'Música',
    _year: yearOf(attrs.releaseDate),
  };
}

async function buildLibrary(win, cacheRoot, refresh = false) {
  const cacheFile = path.join(cacheRoot, 'real-library.json');
  if (!refresh) {
    const cached = await readCache(cacheFile, 6 * 60 * 60 * 1000);
    if (cached) {
      process.stderr.write('[apple] usando biblioteca en caché\n');
      return cached;
    }
  }
  const t = await tokens(win);
  const [songResources, playlistResources] = await Promise.all([
    apiAllParallel(t, '/me/library/songs?limit=100&extend=inFavorites'),
    apiAllParallel(t, '/me/library/playlists?limit=100'),
  ]);

  process.stderr.write(`[apple] canciones: ${songResources.length}, playlists: ${playlistResources.length}\n`);
  const songs = new Map();
  for (const item of songResources) {
    const s = songFrom(item.attributes || {}, item.id);
    if (s) songs.set(s.id, s);
  }

  const playlistTracks = await mapLimit(playlistResources, 4, async (pl) => {
    try {
      const tracks = await apiAllParallel(t, `/me/library/playlists/${encodeURIComponent(pl.id)}/tracks?limit=100`, 500);
      const ids = [];
      for (const item of tracks) {
        const s = songFrom(item.attributes || {}, item.id);
        if (!s) continue;
        if (!songs.has(s.id)) songs.set(s.id, s);
        ids.push(s.id);
      }
      return { pl, ids };
    } catch (error) {
      return { pl, ids: [], error: String(error) };
    }
  });

  process.stderr.write(`[apple] pistas de playlists: ${playlistTracks.reduce((n, p) => n + p.ids.length, 0)}\n`);
  // Portadas: una descarga por URL única.
  const urls = [];
  for (const s of songs.values()) urls.push(s._artwork);
  for (const { pl } of playlistTracks) urls.push(coverUrl(pl.attributes || {}));
  process.stderr.write(`[apple] portadas únicas: ${new Set(urls.filter(Boolean)).size}\n`);
  const covers = await downloadCovers(urls, cacheRoot);
  process.stderr.write(`[apple] biblioteca lista: ${songs.size} canciones\n`);

  const albums = new Map();
  const artists = new Map();
  for (const s of songs.values()) {
    s.cover_path = (s._artwork && covers.get(s._artwork)) || '';
    if (!albums.has(s.album_id)) {
      albums.set(s.album_id, {
        id: s.album_id,
        title: s.album_title,
        artist_id: s.artist_id,
        artist_name: s.artist_name,
        year: s._year || 0,
        genre: s._genre,
        track_count: 0,
        cover_path: s.cover_path,
        favorite: false,
        playable: true,
      });
    }
    const album = albums.get(s.album_id);
    album.track_count += 1;
    album.favorite = album.favorite || s.favorite;
    if (!artists.has(s.artist_id)) {
      artists.set(s.artist_id, {
        id: s.artist_id,
        name: s.artist_name,
        genre: s._genre,
        photo_path: s.cover_path,
        album_count: 0,
        song_count: 0,
        favorite: false,
      });
    }
    const artist = artists.get(s.artist_id);
    artist.song_count += 1;
    artist.favorite = artist.favorite || s.favorite;
    if (!artist.photo_path && s.cover_path) artist.photo_path = s.cover_path;
  }
  for (const album of albums.values()) {
    const artist = artists.get(album.artist_id);
    if (artist) artist.album_count += 1;
  }

  const playlists = playlistTracks.map(({ pl, ids }, index) => {
    const attrs = pl.attributes || {};
    const art = covers.get(coverUrl(attrs)) || '';
    return {
      id: `playlist-${hash(pl.id, 'lib')}-${index}`,
      name: attrs.name || 'Playlist',
      description: (attrs.description && (attrs.description.standard || attrs.description)) || '',
      curator: 'Apple Music',
      cover_path: art,
      track_count: ids.length,
      song_ids: ids,
      favorite: false,
    };
  });
  // La id de playlist anterior no es la de Apple; guardamos la original para
  // futuras operaciones de escritura sin exponerla al cliente.
  for (let i = 0; i < playlists.length; i++) {
    Object.defineProperty(playlists[i], '_apple_id', { value: playlistTracks[i].pl.id, enumerable: false });
  }

  const cleanSongs = [...songs.values()].map(({ _artwork, _genre, _year, ...s }) => s);
  const colors = [...artists.values()];
  const fav = {
    song_ids: cleanSongs.filter((s) => s.favorite).map((s) => s.id),
    album_ids: [...albums.values()].filter((a) => a.favorite).map((a) => a.id),
    artist_ids: colors.filter((a) => a.favorite).map((a) => a.id),
    playlist_ids: [],
  };

  const snapshot = {
    artists: colors,
    albums: [...albums.values()],
    songs: cleanSongs,
    playlists,
    home: [],
    favorites: fav,
    stats: {
      songs: cleanSongs.length,
      albums: albums.size,
      artists: colors.length,
      playlists: playlists.length,
    },
    generated_at: new Date().toISOString().slice(0, 10),
  };
  await fsp.mkdir(cacheRoot, { recursive: true });
  await fsp.writeFile(cacheFile, JSON.stringify(snapshot));
  process.stderr.write(`[apple] caché escrita: ${cacheFile}\n`);
  return snapshot;
}

function stripTtml(text) {
  return text
    .replace(/<br\s*\/?>/gi, ' ')
    .replace(/<[^>]+>/g, '')
    .replace(/&amp;/g, '&')
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/\s+/g, ' ')
    .trim();
}

function parseTime(value) {
  if (!value) return 0;
  if (/^\d+(\.\d+)?$/.test(value)) return Math.round(Number(value) * 1000);
  const parts = value.split(':').map(Number);
  if (parts.length === 3) return Math.round((parts[0] * 3600 + parts[1] * 60 + parts[2]) * 1000);
  if (parts.length === 2) return Math.round((parts[0] * 60 + parts[1]) * 1000);
  return 0;
}

async function lyrics(win, songId) {
  const t = await tokens(win);
  const page = await apiJson(t, `/catalog/${encodeURIComponent(t.store)}/songs/${encodeURIComponent(songId)}/lyrics?extend=ttml`);
  const attrs = page.data && page.data[0] && page.data[0].attributes;
  if (!attrs) return { song_id: songId, lines: [], synced: false, source: 'apple' };
  const ttml = attrs.ttml || '';
  const lines = [];
  if (ttml) {
    const re = /<p[^>]*\bbegin="([^"]+)"[^>]*>([\s\S]*?)<\/p>/gi;
    let m;
    while ((m = re.exec(ttml)) !== null) {
      const text = stripTtml(m[2]);
      if (text) lines.push({ time_ms: parseTime(m[1]), text });
    }
  }
  if (!lines.length && attrs.lyrics) {
    const text = stripTtml(attrs.lyrics);
    for (const [i, line] of text.split(/\n+/).filter(Boolean).entries()) {
      lines.push({ time_ms: i * 5000, text: line.trim() });
    }
  }
  return { song_id: songId, lines, synced: Boolean(ttml), source: 'apple' };
}

module.exports = { buildLibrary, lyrics, tokens, apiJson };
