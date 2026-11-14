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

// Peticiones de escritura: Apple responde 202 sin cuerpo, así que no se puede
// reutilizar apiJson (que siempre intenta parsear JSON).
async function apiSend(t, method, url) {
  const full = url.startsWith('http')
    ? url
    : url.startsWith('/v1/')
      ? 'https://api.music.apple.com' + url
      : API + url;
  const res = await fetch(full, {
    method,
    signal: AbortSignal.timeout(20000),
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
  return res.status;
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
    explicit: attrs.contentRating === 'explicit',
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

// ---------------------------------------------------------------------------
// Catálogo de Apple Music (no la biblioteca)
// ---------------------------------------------------------------------------

function cleanSong(song) {
  const { _artwork, _genre, _year, ...rest } = song;
  return rest;
}

function cleanAlbum(album) {
  const { _artwork, ...rest } = album;
  return rest;
}

function cleanArtist(artist) {
  const { _artwork, ...rest } = artist;
  return rest;
}

function cleanPlaylist(playlist) {
  const { _artwork, ...rest } = playlist;
  return rest;
}

function catalogAlbumFrom(item) {
  const attrs = item.attributes || {};
  const relArtist =
    item.relationships &&
    item.relationships.artists &&
    item.relationships.artists.data &&
    item.relationships.artists.data[0];
  return {
    id: String(item.id || (attrs.playParams && attrs.playParams.id) || ''),
    title: attrs.name || 'Álbum',
    artist_id: relArtist
      ? String(relArtist.id)
      : hash(attrs.artistName || '', 'artist-cat'),
    artist_name:
      attrs.artistName ||
      (relArtist && relArtist.attributes && relArtist.attributes.name) ||
      'Artista desconocido',
    year: yearOf(attrs.releaseDate),
    genre: (attrs.genreNames && attrs.genreNames[0]) || 'Música',
    track_count: attrs.trackCount || 0,
    cover_path: '',
    favorite: false,
    playable: true,
    _artwork: coverUrl(attrs),
  };
}

function catalogArtistFrom(item) {
  const attrs = item.attributes || {};
  return {
    id: String(item.id || ''),
    name: attrs.name || 'Artista',
    genre: (attrs.genreNames && attrs.genreNames[0]) || 'Música',
    photo_path: '',
    album_count: 0,
    song_count: 0,
    favorite: false,
    _artwork: coverUrl(attrs),
  };
}

function catalogSongFrom(attrs, rawId, albumId, artistId) {
  const song = songFrom(attrs, rawId);
  if (!song) return null;
  if (albumId) song.album_id = String(albumId);
  if (artistId) song.artist_id = String(artistId);
  return song;
}

// Playlist del catálogo. Apple guarda la descripción como `{standard, short}`;
// en Musa es texto plano.
function catalogPlaylistFrom(item) {
  const attrs = item.attributes || {};
  const tracks =
    (item.relationships && item.relationships.tracks && item.relationships.tracks.data) || [];
  return {
    id: String(item.id || ''),
    name: attrs.name || 'Playlist',
    description:
      (attrs.description && (attrs.description.standard || attrs.description.short)) || '',
    curator: attrs.curatorName || 'Apple Music',
    cover_path: '',
    track_count: attrs.trackCount || tracks.length || 0,
    song_ids: tracks.map((track) => String(track.id)),
    favorite: false,
    _artwork: coverUrl(attrs),
  };
}

function coverIn(covers, item) {
  return (item._artwork && covers.get(item._artwork)) || '';
}

// Busca canciones, álbumes y artistas en el catálogo de Apple Music. Las
// portadas se cachean en disco para que la UI las pinte como las locales.
async function search(win, query, limit = 20, cacheRoot) {
  const term = String(query || '').trim();
  if (!term) return { songs: [], albums: [], artists: [] };
  const t = await tokens(win);
  const store = encodeURIComponent(t.store);
  const size = Math.max(1, Math.min(25, Number(limit) || 20));
  const page = await apiJson(
    t,
    `/catalog/${store}/search?term=${encodeURIComponent(term)}&types=songs,albums,artists&limit=${size}`,
  );
  const results = page.results || {};
  const songs = ((results.songs && results.songs.data) || [])
    .map((item) => songFrom(item.attributes || {}, item.id))
    .filter(Boolean);
  const albums = ((results.albums && results.albums.data) || [])
    .slice(0, 10)
    .map(catalogAlbumFrom);
  const artists = ((results.artists && results.artists.data) || [])
    .slice(0, 10)
    .map(catalogArtistFrom);

  const covers = await downloadCovers(
    [
      ...songs.map((s) => s._artwork),
      ...albums.map((a) => a._artwork),
      ...artists.map((a) => a._artwork),
    ],
    cacheRoot,
  );
  for (const song of songs) song.cover_path = coverIn(covers, song);
  for (const album of albums) album.cover_path = coverIn(covers, album);
  for (const artist of artists) artist.photo_path = coverIn(covers, artist);

  return {
    songs: songs.map(cleanSong),
    albums: albums.map(cleanAlbum),
    artists: artists.map(cleanArtist),
  };
}

// Detalle de un álbum del catálogo con sus pistas y el artista real (para que
// el enlace del artista abra su página de catálogo).
async function catalogAlbum(win, id, cacheRoot) {
  const t = await tokens(win);
  const store = encodeURIComponent(t.store);
  const page = await apiJson(
    t,
    `/catalog/${store}/albums/${encodeURIComponent(id)}?include=tracks,artists&limit%5Btracks%5D=100`,
  );
  const item = page.data && page.data[0];
  if (!item) throw new Error('álbum no encontrado');
  const album = catalogAlbumFrom(item);
  const tracks =
    (item.relationships && item.relationships.tracks && item.relationships.tracks.data) || [];
  const songs = tracks
    .map((track) =>
      catalogSongFrom(track.attributes || {}, track.id, album.id, album.artist_id),
    )
    .filter(Boolean);
  album.track_count = album.track_count || songs.length;

  const covers = await downloadCovers(
    [album._artwork, ...songs.map((s) => s._artwork)],
    cacheRoot,
  );
  album.cover_path = coverIn(covers, album);
  for (const song of songs) {
    song.cover_path = coverIn(covers, song) || album.cover_path;
  }
  return { album: cleanAlbum(album), songs: songs.map(cleanSong) };
}

// Detalle de un artista del catálogo: sus álbumes, una lista de canciones
// populares para poder escucharlo sin tener nada en la biblioteca y las
// playlists de Apple Music en las que aparece.
async function catalogArtist(win, id, cacheRoot) {
  const t = await tokens(win);
  const store = encodeURIComponent(t.store);
  const [page, top, lists] = await Promise.all([
    apiJson(
      t,
      `/catalog/${store}/artists/${encodeURIComponent(id)}?include=albums&limit%5Balbums%5D=50`,
    ),
    apiJson(
      t,
      `/catalog/${store}/artists/${encodeURIComponent(id)}/view/top-songs?limit=20`,
    ).catch(() => ({ data: [] })),
    apiJson(
      t,
      `/catalog/${store}/artists/${encodeURIComponent(id)}/playlists?limit=10`,
    ).catch(() => ({ data: [] })),
  ]);
  const item = page.data && page.data[0];
  if (!item) throw new Error('artista no encontrado');
  const artist = catalogArtistFrom(item);
  const albumItems =
    (item.relationships && item.relationships.albums && item.relationships.albums.data) || [];
  const albums = albumItems.map((albumItem) => {
    const album = catalogAlbumFrom(albumItem);
    album.artist_id = artist.id;
    album.artist_name = artist.name;
    return album;
  });
  const songs = ((top && top.data) || [])
    .map((songItem) =>
      catalogSongFrom(songItem.attributes || {}, songItem.id, null, artist.id),
    )
    .filter(Boolean);
  const playlists = ((lists && lists.data) || []).slice(0, 10).map(catalogPlaylistFrom);

  const covers = await downloadCovers(
    [
      artist._artwork,
      ...albums.map((a) => a._artwork),
      ...songs.map((s) => s._artwork),
      ...playlists.map((p) => p._artwork),
    ],
    cacheRoot,
  );
  const firstAlbumCover =
    albums.map((a) => coverIn(covers, a)).find(Boolean) ||
    songs.map((s) => coverIn(covers, s)).find(Boolean) ||
    '';
  artist.photo_path = coverIn(covers, artist) || firstAlbumCover;
  for (const album of albums) album.cover_path = coverIn(covers, album);
  for (const song of songs) {
    song.cover_path = coverIn(covers, song) || firstAlbumCover;
  }
  for (const playlist of playlists) {
    playlist.cover_path = coverIn(covers, playlist) || firstAlbumCover;
  }
  artist.album_count = albums.length;
  artist.song_count = songs.length;
  return {
    artist: cleanArtist(artist),
    albums: albums.map(cleanAlbum),
    songs: songs.map(cleanSong),
    playlists: playlists.map(cleanPlaylist),
  };
}

// Detalle de una playlist del catálogo con sus canciones, para poder abrirla
// desde el perfil de un artista sin tenerla en la biblioteca.
async function catalogPlaylist(win, id, cacheRoot) {
  const t = await tokens(win);
  const store = encodeURIComponent(t.store);
  const page = await apiJson(
    t,
    `/catalog/${store}/playlists/${encodeURIComponent(id)}?include=tracks&limit%5Btracks%5D=100`,
  );
  const item = page.data && page.data[0];
  if (!item) throw new Error('playlist no encontrada');
  const playlist = catalogPlaylistFrom(item);
  const trackItems =
    (item.relationships && item.relationships.tracks && item.relationships.tracks.data) || [];
  const songs = trackItems
    .map((track) => catalogSongFrom(track.attributes || {}, track.id, null, null))
    .filter(Boolean);
  playlist.track_count = playlist.track_count || songs.length;

  const covers = await downloadCovers(
    [playlist._artwork, ...songs.map((s) => s._artwork)],
    cacheRoot,
  );
  playlist.cover_path = coverIn(covers, playlist);
  for (const song of songs) {
    song.cover_path = coverIn(covers, song) || playlist.cover_path;
  }
  return {
    playlist: cleanPlaylist(playlist),
    songs: songs.map(cleanSong),
  };
}

// Añade un artista de catálogo a los favoritos/biblioteca del usuario en
// Apple Music. La operación es best-effort: Musa guarda el artista en local
// aunque Apple la ignore o la rechace.
async function followArtist(win, id) {
  const t = await tokens(win);
  const params = new URLSearchParams();
  params.set('ids[artists]', String(id));
  const query = params.toString();
  // Apple ha cambiado el endpoint preferido con el tiempo: favoritos es la vía
  // moderna y la biblioteca la clásica. Probamos ambas antes de rendirnos.
  const attempts = [
    `${API}/me/favorites?${query}`,
    `${API}/me/library?${query}`,
  ];
  let firstError = null;
  for (const url of attempts) {
    try {
      await apiSend(t, 'POST', url);
      return { synced: true, id: String(id) };
    } catch (error) {
      if (!firstError) firstError = error;
    }
  }
  throw firstError || new Error('Apple Music no aceptó el artista');
}

// Añade un álbum de catálogo a la biblioteca de Apple Music. Igual que con
// los artistas, Musa guarda el álbum en local aunque Apple no confirme.
async function addAlbum(win, id) {
  const t = await tokens(win);
  const params = new URLSearchParams();
  params.set('ids[albums]', String(id));
  await apiSend(t, 'POST', `${API}/me/library?${params.toString()}`);
  return { synced: true, id: String(id) };
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

module.exports = {
  buildLibrary,
  search,
  catalogAlbum,
  catalogArtist,
  catalogPlaylist,
  followArtist,
  addAlbum,
  lyrics,
  tokens,
  apiJson,
};
