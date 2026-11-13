//! Servidor JSON-lines del daemon de Musa.
//!
//! Un hilo por conexión, un hilo global para el reloj de reproducción y un
//! canal por cliente para los eventos. No hay HTTP ni puertos TCP.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use musa_protocol::{
    Album, Artist, Event, Favorites, Hello, Lyrics, PlayerState, Playlist, ProtocolError,
    RepeatMode, Request, Response, SearchResults, Song, cmd, encode_line, event, parse_request,
    to_json,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::covers::{self, CoverKind};
use crate::data::Library;
use crate::sidecar::Sidecar;

#[derive(Debug)]
struct Player {
    queue: Vec<String>,
    index: usize,
    position_ms: u64,
    playing: bool,
    shuffle: bool,
    repeat: RepeatMode,
    volume: u8,
    /// Orden previo para poder deshacer shuffle.
    base_queue: Option<Vec<String>>,
    last_tick: Instant,
    last_emit: Instant,
    /// Pista que reporta MusicKit cuando el motor es real.
    current_track: Option<Song>,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            queue: Vec::new(),
            index: 0,
            position_ms: 0,
            playing: false,
            shuffle: false,
            repeat: RepeatMode::Off,
            volume: 78,
            base_queue: None,
            last_tick: Instant::now(),
            last_emit: Instant::now(),
            current_track: None,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct PersistedPlayer {
    queue: Vec<String>,
    index: usize,
    position_ms: u64,
    shuffle: bool,
    repeat: RepeatMode,
    volume: u8,
}

struct ClientHandle {
    id: u64,
    tx: mpsc::Sender<String>,
}

struct Shared {
    library: RwLock<Library>,
    player: Mutex<Player>,
    clients: Mutex<Vec<ClientHandle>>,
    covers_dir: PathBuf,
    favorites_path: PathBuf,
    player_path: PathBuf,
    socket_path: PathBuf,
    started: Instant,
    sidecar: Option<Arc<Sidecar>>,
    mode: String,
    had_client: AtomicBool,
}

impl Shared {
    fn player_state(&self) -> PlayerState {
        let player = self.player.lock().unwrap();
        PlayerState {
            queue: player.queue.clone(),
            index: player.index,
            position_ms: player.position_ms,
            playing: player.playing,
            shuffle: player.shuffle,
            repeat: player.repeat,
            volume: player.volume,
            track: player.current_track.clone(),
            engine: "real".into(),
        }
    }

    fn broadcast(&self, event_name: &str, data: Value) {
        let line = encode_line(&Event::new(event_name, data));
        let mut clients = self.clients.lock().unwrap();
        clients.retain(|client| client.tx.send(line.clone()).is_ok());
    }

    fn broadcast_player(&self) {
        let state = self.player_state();
        self.broadcast(event::PLAYER_CHANGED, to_json(&state));
    }

    fn save_favorites(&self, favorites: &Favorites) {
        if let Some(parent) = self.favorites_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(bytes) = serde_json::to_vec_pretty(favorites) {
            let _ = std::fs::write(&self.favorites_path, bytes);
        }
    }

    fn save_player(&self) {
        let player = self.player.lock().unwrap();
        let persisted = PersistedPlayer {
            queue: player.queue.clone(),
            index: player.index,
            position_ms: player.position_ms,
            shuffle: player.shuffle,
            repeat: player.repeat,
            volume: player.volume,
        };
        drop(player);
        if let Some(parent) = self.player_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(bytes) = serde_json::to_vec_pretty(&persisted) {
            let _ = std::fs::write(&self.player_path, bytes);
        }
    }
}

#[derive(Debug, Deserialize)]
struct IdParams {
    id: String,
}

#[derive(Debug, Deserialize)]
struct CoverParams {
    kind: String,
    id: String,
    #[serde(default = "default_cover_size")]
    size: u32,
}

fn default_cover_size() -> u32 {
    600
}

#[derive(Debug, Deserialize)]
struct SearchParams {
    query: String,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    60
}

#[derive(Debug, Deserialize)]
struct FavoriteParams {
    kind: String,
    id: String,
    favorite: bool,
}

#[derive(Debug, Deserialize)]
struct QueueParams {
    song_ids: Vec<String>,
    #[serde(default)]
    index: usize,
    #[serde(default)]
    play: bool,
}

#[derive(Debug, Deserialize)]
struct PlayParams {
    #[serde(default)]
    song_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SeekParams {
    position_ms: u64,
}

#[derive(Debug, Deserialize)]
struct VolumeParams {
    volume: u8,
}

#[derive(Debug, Deserialize)]
struct ShuffleParams {
    enabled: bool,
}

#[derive(Debug, Deserialize)]
struct RepeatParams {
    mode: RepeatMode,
}

#[derive(Debug, Deserialize)]
struct JumpParams {
    index: usize,
}

pub struct ServerConfig {
    pub socket_path: PathBuf,
    pub covers_dir: PathBuf,
    pub favorites_path: PathBuf,
    pub player_path: PathBuf,
    pub name: String,
    pub version: String,
    pub mode: String,
    pub sidecar: Option<Arc<Sidecar>>,
}

pub fn run(
    config: ServerConfig,
    library: Library,
    restored_player: Option<PersistedPlayer>,
) -> anyhow::Result<()> {
    if let Some(parent) = config.socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if config.socket_path.exists() {
        // Solo borra el socket si nadie está escuchando.
        if UnixStream::connect(&config.socket_path).is_ok() {
            anyhow::bail!(
                "ya hay un daemon escuchando en {}",
                config.socket_path.display()
            );
        }
        let _ = std::fs::remove_file(&config.socket_path);
    }
    let listener = UnixListener::bind(&config.socket_path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ =
            std::fs::set_permissions(&config.socket_path, std::fs::Permissions::from_mode(0o600));
    }

    let state = Arc::new(Shared {
        library: RwLock::new(library),
        player: Mutex::new(Player::default()),
        clients: Mutex::new(Vec::new()),
        covers_dir: config.covers_dir,
        favorites_path: config.favorites_path,
        player_path: config.player_path,
        socket_path: config.socket_path.clone(),
        started: Instant::now(),
        sidecar: config.sidecar.clone(),
        mode: config.mode.clone(),
        had_client: AtomicBool::new(false),
    });

    if state.sidecar.is_none() {
        if let Some(restored) = restored_player {
            let mut player = state.player.lock().unwrap();
            player.queue = restored.queue;
            player.index = restored.index.min(player.queue.len().saturating_sub(1));
            player.position_ms = restored.position_ms;
            player.shuffle = restored.shuffle;
            player.repeat = restored.repeat;
            player.volume = restored.volume;
            player.playing = false;
        }
    }

    if state.sidecar.is_some() {
        spawn_real_ticker(state.clone());
    } else {
        spawn_ticker(state.clone());
    }
    eprintln!(
        "musad {} [{}] escuchando en {}",
        config.version,
        state.mode,
        state.socket_path.display()
    );

    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                let state = state.clone();
                let name = config.name.clone();
                let version = config.version.clone();
                std::thread::spawn(move || {
                    if let Err(err) = serve_client(stream, state, name, version) {
                        eprintln!("musad: cliente desconectado: {err}");
                    }
                });
            }
            Err(err) => eprintln!("musad: accept: {err}"),
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct RealState {
    #[serde(default)]
    ready: bool,
    playing: bool,
    position_ms: u64,
    #[allow(dead_code)]
    duration_ms: u64,
    queue: Vec<String>,
    index: usize,
    shuffle: bool,
    repeat: String,
    #[serde(default)]
    volume: f64,
    track: Option<RealTrack>,
}

#[derive(Debug, Deserialize)]
struct RealTrack {
    id: String,
    title: String,
    artist_name: String,
    album_title: String,
    duration_ms: u64,
    #[allow(dead_code)]
    cover_url: Option<String>,
}

fn repeat_from_str(value: &str) -> RepeatMode {
    match value {
        "one" => RepeatMode::One,
        "all" => RepeatMode::All,
        _ => RepeatMode::Off,
    }
}

fn apply_real_state(state: &Arc<Shared>, value: Value) -> Result<PlayerState, ProtocolError> {
    let real: RealState = serde_json::from_value(value)
        .map_err(|e| ProtocolError::new("sidecar_json", e.to_string()))?;
    if !real.ready {
        return Err(ProtocolError::new(
            "sidecar_not_ready",
            "MusicKit no está listo",
        ));
    }
    let library = state.library.read().unwrap();
    let track = real.track.as_ref().map(|t| {
        library
            .songs_by_id
            .get(&t.id)
            .cloned()
            .unwrap_or_else(|| Song {
                id: t.id.clone(),
                title: t.title.clone(),
                artist_id: String::new(),
                artist_name: t.artist_name.clone(),
                album_id: String::new(),
                album_title: t.album_title.clone(),
                track_number: 1,
                disc_number: 1,
                duration_ms: t.duration_ms,
                cover_path: String::new(),
                favorite: false,
                playable: true,
                explicit: false,
            })
    });
    {
        let mut player = state.player.lock().unwrap();
        player.queue = real.queue;
        player.index = real.index.min(player.queue.len().saturating_sub(1));
        player.position_ms = real.position_ms;
        player.playing = real.playing;
        player.shuffle = real.shuffle;
        player.repeat = repeat_from_str(&real.repeat);
        player.volume = (real.volume.clamp(0.0, 1.0) * 100.0).round() as u8;
        player.current_track = track;
        player.last_tick = Instant::now();
        player.last_emit = Instant::now();
    }
    Ok(state.player_state())
}

fn spawn_real_ticker(state: Arc<Shared>) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(1000));
            let Some(sidecar) = state.sidecar.clone() else {
                break;
            };
            match sidecar.state() {
                Ok(value) => {
                    if let Ok(player) = apply_real_state(&state, value) {
                        state.broadcast(event::PLAYER_CHANGED, to_json(&player));
                    }
                }
                Err(err) => {
                    eprintln!("musad-real: estado del sidecar: {}", err.message);
                }
            }
        }
    });
}

fn spawn_ticker(state: Arc<Shared>) {
    std::thread::spawn(move || {
        let mut last_persist = Instant::now();
        loop {
            std::thread::sleep(Duration::from_millis(200));
            let (changed, emit) = {
                let library = state.library.read().unwrap();
                let mut player = state.player.lock().unwrap();
                if !player.playing {
                    (false, false)
                } else {
                    let now = Instant::now();
                    let elapsed = now.duration_since(player.last_tick);
                    player.last_tick = now;
                    player.position_ms = player
                        .position_ms
                        .saturating_add(elapsed.as_millis() as u64);
                    let duration = library
                        .songs_by_id
                        .get(player.queue.get(player.index).unwrap_or(&String::new()))
                        .map(|s| s.duration_ms)
                        .unwrap_or(0);
                    if duration > 0 && player.position_ms >= duration {
                        if player.repeat == RepeatMode::One {
                            player.position_ms = 0;
                        } else if player.index + 1 < player.queue.len() {
                            player.index += 1;
                            player.position_ms = 0;
                        } else if player.repeat == RepeatMode::All && !player.queue.is_empty() {
                            player.index = 0;
                            player.position_ms = 0;
                        } else {
                            player.position_ms = duration;
                            player.playing = false;
                        }
                    }
                    let emit = now.duration_since(player.last_emit) >= Duration::from_millis(950);
                    if emit {
                        player.last_emit = now;
                    }
                    (true, emit)
                }
            };
            if emit {
                state.broadcast_player();
            }
            if changed && last_persist.elapsed() >= Duration::from_secs(10) {
                state.save_player();
                last_persist = Instant::now();
            }
        }
    });
}

fn serve_client(
    stream: UnixStream,
    state: Arc<Shared>,
    name: String,
    version: String,
) -> anyhow::Result<()> {
    static NEXT_CLIENT: AtomicU64 = AtomicU64::new(1);
    let client_id = NEXT_CLIENT.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = mpsc::channel::<String>();
    state
        .clients
        .lock()
        .unwrap()
        .push(ClientHandle { id: client_id, tx });

    let mut writer = stream.try_clone()?;
    let writer_thread = std::thread::spawn(move || {
        while let Ok(line) = rx.recv() {
            if writer.write_all(line.as_bytes()).is_err() {
                break;
            }
            let _ = writer.flush();
        }
    });

    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        let response = match parse_request(&line) {
            Ok(req) => handle_request(&state, &name, &version, req),
            Err(err) => Response::err(0, &err.code, err.message),
        };
        let encoded = encode_line(&response);
        if state
            .clients
            .lock()
            .unwrap()
            .iter()
            .find(|c| c.id == client_id)
            .is_none_or(|c| c.tx.send(encoded).is_err())
        {
            break;
        }
    }

    state
        .clients
        .lock()
        .unwrap()
        .retain(|client| client.id != client_id);
    let empty = state.clients.lock().unwrap().is_empty();
    if empty && state.had_client.load(Ordering::Relaxed) {
        shutdown_when_idle(state.clone());
    }
    let _ = writer_thread.join();
    Ok(())
}

fn handle_request(state: &Arc<Shared>, name: &str, version: &str, req: Request) -> Response {
    let id = req.id;
    match dispatch(state, name, version, &req) {
        Ok(data) => Response::ok(id, data),
        Err(err) => Response::err(id, &err.code, err.message),
    }
}

/// Si la UI desaparece (cierre normal, crash o kill), no dejamos el motor
/// sonando para siempre: pausa, apaga el sidecar y termina.
fn shutdown_when_idle(state: Arc<Shared>) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1500));
        if !state.clients.lock().unwrap().is_empty() {
            return;
        }
        if let Some(sidecar) = &state.sidecar {
            let _ = sidecar.player("pause", json!({}));
            let _ = sidecar.call("shutdown", json!({}), Duration::from_secs(3));
        }
        std::process::exit(0);
    });
}

fn real_player_dispatch(
    state: &Arc<Shared>,
    req: &Request,
) -> Option<Result<Value, ProtocolError>> {
    let sidecar = state.sidecar.as_ref()?;
    let result = match req.cmd.as_str() {
        cmd::QUEUE_GET | cmd::PLAYER_STATE => Ok(to_json(&state.player_state())),
        cmd::QUEUE_SET => {
            let params: QueueParams = match musa_protocol::params(req) {
                Ok(p) => p,
                Err(e) => return Some(Err(e)),
            };
            sidecar
                .set_queue(&params.song_ids, params.index, params.play)
                .and_then(|value| apply_real_state(state, value))
                .map(|player| to_json(&player))
        }
        cmd::QUEUE_JUMP => {
            let params: JumpParams = match musa_protocol::params(req) {
                Ok(p) => p,
                Err(e) => return Some(Err(e)),
            };
            sidecar
                .player("jump", json!({"index": params.index}))
                .and_then(|value| apply_real_state(state, value))
                .map(|player| to_json(&player))
        }
        cmd::PLAYER_PLAY => {
            let params: PlayParams =
                serde_json::from_value(req.params.clone()).unwrap_or(PlayParams { song_id: None });
            let jump_to = params.song_id.and_then(|id| {
                state
                    .player
                    .lock()
                    .unwrap()
                    .queue
                    .iter()
                    .position(|queued| queued == &id)
            });
            if let Some(index) = jump_to {
                if let Err(err) = sidecar.player("jump", json!({"index": index})) {
                    return Some(Err(err));
                }
            }
            sidecar
                .player("play", json!({}))
                .and_then(|value| apply_real_state(state, value))
                .map(|player| to_json(&player))
        }
        cmd::PLAYER_PAUSE => sidecar
            .player("pause", json!({}))
            .and_then(|value| apply_real_state(state, value))
            .map(|player| to_json(&player)),
        cmd::PLAYER_NEXT => sidecar
            .player("next", json!({}))
            .and_then(|value| apply_real_state(state, value))
            .map(|player| to_json(&player)),
        cmd::PLAYER_PREV => sidecar
            .player("prev", json!({}))
            .and_then(|value| apply_real_state(state, value))
            .map(|player| to_json(&player)),
        cmd::PLAYER_SEEK => {
            let params: SeekParams = match musa_protocol::params(req) {
                Ok(p) => p,
                Err(e) => return Some(Err(e)),
            };
            sidecar
                .player("seek", json!({"position_ms": params.position_ms}))
                .and_then(|value| apply_real_state(state, value))
                .map(|player| to_json(&player))
        }
        cmd::PLAYER_SHUFFLE => {
            let params: ShuffleParams = match musa_protocol::params(req) {
                Ok(p) => p,
                Err(e) => return Some(Err(e)),
            };
            sidecar
                .player("shuffle", json!({"enabled": params.enabled}))
                .and_then(|value| apply_real_state(state, value))
                .map(|player| to_json(&player))
        }
        cmd::PLAYER_REPEAT => {
            let params: RepeatParams = match musa_protocol::params(req) {
                Ok(p) => p,
                Err(e) => return Some(Err(e)),
            };
            sidecar
                .player("repeat", json!({"mode": params.mode}))
                .and_then(|value| apply_real_state(state, value))
                .map(|player| to_json(&player))
        }
        cmd::PLAYER_VOLUME => {
            let params: VolumeParams = match musa_protocol::params(req) {
                Ok(p) => p,
                Err(e) => return Some(Err(e)),
            };
            sidecar
                .player("volume", json!({"volume": params.volume}))
                .and_then(|value| apply_real_state(state, value))
                .map(|player| to_json(&player))
        }
        cmd::LYRICS_GET => {
            let params: IdParams = match musa_protocol::params(req) {
                Ok(p) => p,
                Err(e) => return Some(Err(e)),
            };
            match sidecar.lyrics(&params.id) {
                Ok(value) => Ok(value),
                Err(_) => Ok(to_json(&Lyrics {
                    song_id: params.id,
                    lines: Vec::new(),
                    synced: false,
                    source: "apple (letras no disponibles para este token)".into(),
                })),
            }
        }
        _ => return None,
    };
    if result.is_ok() {
        state.broadcast_player();
    }
    Some(result)
}

fn dispatch(
    state: &Arc<Shared>,
    name: &str,
    version: &str,
    req: &Request,
) -> Result<Value, ProtocolError> {
    if req.cmd == cmd::SHUTDOWN {
        if let Some(sidecar) = &state.sidecar {
            let _ = sidecar.player("pause", json!({}));
            let _ = sidecar.call("shutdown", json!({}), Duration::from_secs(3));
        }
        std::process::exit(0);
    }
    if let Some(result) = real_player_dispatch(state, req) {
        return result;
    }
    match req.cmd.as_str() {
        cmd::HELLO => {
            let library = state.library.read().unwrap();
            Ok(to_json(&Hello {
                name: name.to_string(),
                version: version.to_string(),
                protocol: musa_protocol::PROTOCOL_VERSION,
                mode: state.mode.clone(),
                socket: state.socket_path.display().to_string(),
                library_stats: library.snapshot.stats.clone(),
                features: vec![
                    "apple-music".into(),
                    "biblioteca".into(),
                    "busqueda".into(),
                    "favoritos".into(),
                    "letras".into(),
                    "portadas".into(),
                    "cola".into(),
                    "reproduccion-real".into(),
                ],
            }))
        }
        cmd::PING => Ok(json!({
            "pong": true,
            "now_ms": SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0),
            "uptime_ms": state.started.elapsed().as_millis() as u64,
        })),
        cmd::LIBRARY_GET => {
            let library = state.library.read().unwrap();
            Ok(to_json(&library.snapshot))
        }
        cmd::ALBUMS_LIST => {
            let library = state.library.read().unwrap();
            Ok(to_json(&library.snapshot.albums))
        }
        cmd::ALBUM_GET => {
            let params: IdParams = musa_protocol::params(req)?;
            let library = state.library.read().unwrap();
            let album = library
                .albums_by_id
                .get(&params.id)
                .cloned()
                .ok_or_else(|| ProtocolError::new("not_found", "álbum inexistente"))?;
            let songs = library.songs_of_album(&album.id);
            Ok(json!({"album": album, "songs": songs}))
        }
        cmd::ARTISTS_LIST => {
            let library = state.library.read().unwrap();
            Ok(to_json(&library.snapshot.artists))
        }
        cmd::ARTIST_GET => {
            let params: IdParams = musa_protocol::params(req)?;
            let library = state.library.read().unwrap();
            let artist = library
                .artists_by_id
                .get(&params.id)
                .cloned()
                .ok_or_else(|| ProtocolError::new("not_found", "artista inexistente"))?;
            let albums = library.albums_of_artist(&artist.id);
            let songs: Vec<Song> = library
                .snapshot
                .songs
                .iter()
                .filter(|s| s.artist_id == artist.id)
                .cloned()
                .collect();
            Ok(json!({"artist": artist, "albums": albums, "songs": songs}))
        }
        cmd::SONGS_LIST => {
            let library = state.library.read().unwrap();
            Ok(to_json(&library.snapshot.songs))
        }
        cmd::PLAYLISTS_LIST => {
            let library = state.library.read().unwrap();
            Ok(to_json(&library.snapshot.playlists))
        }
        cmd::PLAYLIST_GET => {
            let params: IdParams = musa_protocol::params(req)?;
            let library = state.library.read().unwrap();
            let playlist = library
                .playlists_by_id
                .get(&params.id)
                .cloned()
                .ok_or_else(|| ProtocolError::new("not_found", "playlist inexistente"))?;
            let songs: Vec<Song> = playlist
                .song_ids
                .iter()
                .filter_map(|id| library.songs_by_id.get(id).cloned())
                .collect();
            Ok(json!({"playlist": playlist, "songs": songs}))
        }
        cmd::FAVORITES_LIST => {
            let library = state.library.read().unwrap();
            let favs = &library.snapshot.favorites;
            let songs: Vec<Song> = favs
                .song_ids
                .iter()
                .filter_map(|id| library.songs_by_id.get(id).cloned())
                .collect();
            let albums: Vec<Album> = favs
                .album_ids
                .iter()
                .filter_map(|id| library.albums_by_id.get(id).cloned())
                .collect();
            let artists: Vec<Artist> = favs
                .artist_ids
                .iter()
                .filter_map(|id| library.artists_by_id.get(id).cloned())
                .collect();
            let playlists: Vec<Playlist> = favs
                .playlist_ids
                .iter()
                .filter_map(|id| library.playlists_by_id.get(id).cloned())
                .collect();
            Ok(json!({
                "songs": songs,
                "albums": albums,
                "artists": artists,
                "playlists": playlists,
            }))
        }
        cmd::FAVORITE_SET => {
            let params: FavoriteParams = musa_protocol::params(req)?;
            let mut library = state.library.write().unwrap();
            let mut favorites = library.snapshot.favorites.clone();
            let (list, exists) = match params.kind.as_str() {
                "song" => (
                    &mut favorites.song_ids,
                    library.songs_by_id.contains_key(&params.id),
                ),
                "album" => (
                    &mut favorites.album_ids,
                    library.albums_by_id.contains_key(&params.id),
                ),
                "artist" => (
                    &mut favorites.artist_ids,
                    library.artists_by_id.contains_key(&params.id),
                ),
                "playlist" => (
                    &mut favorites.playlist_ids,
                    library.playlists_by_id.contains_key(&params.id),
                ),
                other => {
                    return Err(ProtocolError::new(
                        "bad_params",
                        format!("tipo de favorito desconocido: {other}"),
                    ));
                }
            };
            if !exists {
                return Err(ProtocolError::new("not_found", "elemento inexistente"));
            }
            if params.favorite {
                if !list.contains(&params.id) {
                    list.push(params.id.clone());
                }
            } else {
                list.retain(|id| id != &params.id);
            }
            library.apply_favorites(&favorites);
            library.refresh_home();
            let snapshot_favs = library.snapshot.favorites.clone();
            drop(library);
            state.save_favorites(&snapshot_favs);
            state.broadcast(event::LIBRARY_CHANGED, to_json(&snapshot_favs));
            Ok(to_json(&snapshot_favs))
        }
        cmd::HOME_GET => {
            let library = state.library.read().unwrap();
            Ok(to_json(&library.snapshot.home))
        }
        cmd::SEARCH => {
            let params: SearchParams = musa_protocol::params(req)?;
            let query = params.query.trim().to_lowercase();
            let library = state.library.read().unwrap();
            if query.is_empty() {
                return Ok(to_json(&SearchResults {
                    query: params.query,
                    songs: vec![],
                    albums: vec![],
                    artists: vec![],
                    playlists: vec![],
                }));
            }
            let limit = params.limit.clamp(1, 200);
            let matches = |text: &str| text.to_lowercase().contains(&query);
            let songs = library
                .snapshot
                .songs
                .iter()
                .filter(|s| matches(&s.title) || matches(&s.artist_name) || matches(&s.album_title))
                .take(limit)
                .cloned()
                .collect();
            let albums = library
                .snapshot
                .albums
                .iter()
                .filter(|a| matches(&a.title) || matches(&a.artist_name))
                .take(limit)
                .cloned()
                .collect();
            let artists = library
                .snapshot
                .artists
                .iter()
                .filter(|a| matches(&a.name) || matches(&a.genre))
                .take(limit)
                .cloned()
                .collect();
            let playlists = library
                .snapshot
                .playlists
                .iter()
                .filter(|p| matches(&p.name) || matches(&p.description))
                .take(limit)
                .cloned()
                .collect();
            Ok(to_json(&SearchResults {
                query: params.query,
                songs,
                albums,
                artists,
                playlists,
            }))
        }
        cmd::LYRICS_GET => {
            let params: IdParams = musa_protocol::params(req)?;
            Ok(to_json(&Lyrics {
                song_id: params.id,
                lines: Vec::new(),
                synced: false,
                source: "apple".into(),
            }))
        }
        cmd::COVER_ENSURE => {
            let params: CoverParams = musa_protocol::params(req)?;
            let kind = CoverKind::parse(&params.kind).ok_or_else(|| {
                ProtocolError::new("bad_params", "kind debe ser album, artist o playlist")
            })?;
            let library = state.library.read().unwrap();
            let exists = match kind {
                CoverKind::Album => library.albums_by_id.contains_key(&params.id),
                CoverKind::Artist => library.artists_by_id.contains_key(&params.id),
                CoverKind::Playlist => library.playlists_by_id.contains_key(&params.id),
            };
            drop(library);
            if !exists {
                return Err(ProtocolError::new("not_found", "elemento inexistente"));
            }
            let path = covers::ensure(&state.covers_dir, kind, &params.id, params.size)
                .map_err(|e| ProtocolError::new("cover_error", e.to_string()))?;
            Ok(json!({
                "path": path.display().to_string(),
                "width": params.size.clamp(240, 1600),
                "height": params.size.clamp(240, 1600),
            }))
        }
        cmd::QUEUE_SET => {
            let params: QueueParams = musa_protocol::params(req)?;
            let library = state.library.read().unwrap();
            let ids: Vec<String> = params
                .song_ids
                .into_iter()
                .filter(|id| library.songs_by_id.contains_key(id))
                .collect();
            drop(library);
            if ids.is_empty() {
                return Err(ProtocolError::new(
                    "bad_params",
                    "la cola no puede estar vacía",
                ));
            }
            let index = params.index.min(ids.len() - 1);
            {
                let mut player = state.player.lock().unwrap();
                player.queue = ids;
                player.index = index;
                player.position_ms = 0;
                player.playing = params.play;
                player.base_queue = None;
                player.last_tick = Instant::now();
                player.last_emit = Instant::now();
            }
            state.save_player();
            state.broadcast_player();
            Ok(to_json(&state.player_state()))
        }
        cmd::QUEUE_GET | cmd::PLAYER_STATE => Ok(to_json(&state.player_state())),
        cmd::QUEUE_JUMP => {
            let params: JumpParams = musa_protocol::params(req)?;
            {
                let mut player = state.player.lock().unwrap();
                if params.index >= player.queue.len() {
                    return Err(ProtocolError::new("bad_params", "índice fuera de la cola"));
                }
                player.index = params.index;
                player.position_ms = 0;
                player.playing = true;
                player.last_tick = Instant::now();
                player.last_emit = Instant::now();
            }
            state.save_player();
            state.broadcast_player();
            Ok(to_json(&state.player_state()))
        }
        cmd::PLAYER_PLAY => {
            let params: PlayParams =
                serde_json::from_value(req.params.clone()).unwrap_or(PlayParams { song_id: None });
            let library = state.library.read().unwrap();
            let mut player = state.player.lock().unwrap();
            if player.queue.is_empty() {
                if let Some(song_id) = params.song_id {
                    if let Some(song) = library.songs_by_id.get(&song_id) {
                        player.queue = library
                            .songs_of_album(&song.album_id)
                            .iter()
                            .map(|s| s.id.clone())
                            .collect();
                        player.index = player
                            .queue
                            .iter()
                            .position(|id| id == &song_id)
                            .unwrap_or(0);
                        player.position_ms = 0;
                    }
                }
            } else if let Some(song_id) = params.song_id {
                if let Some(index) = player.queue.iter().position(|id| id == &song_id) {
                    player.index = index;
                    player.position_ms = 0;
                }
            }
            if player.queue.is_empty() {
                return Err(ProtocolError::new(
                    "empty_queue",
                    "no hay cola de reproducción",
                ));
            }
            player.playing = true;
            player.last_tick = Instant::now();
            player.last_emit = Instant::now();
            drop(player);
            drop(library);
            state.save_player();
            state.broadcast_player();
            Ok(to_json(&state.player_state()))
        }
        cmd::PLAYER_PAUSE => {
            {
                let mut player = state.player.lock().unwrap();
                player.playing = false;
            }
            state.save_player();
            state.broadcast_player();
            Ok(to_json(&state.player_state()))
        }
        cmd::PLAYER_NEXT => {
            next_track(state, true)?;
            Ok(to_json(&state.player_state()))
        }
        cmd::PLAYER_PREV => {
            {
                let mut player = state.player.lock().unwrap();
                if player.position_ms > 3_000 || player.index == 0 {
                    player.position_ms = 0;
                } else {
                    player.index -= 1;
                    player.position_ms = 0;
                }
                if !player.queue.is_empty() {
                    player.playing = true;
                }
                player.last_tick = Instant::now();
                player.last_emit = Instant::now();
            }
            state.save_player();
            state.broadcast_player();
            Ok(to_json(&state.player_state()))
        }
        cmd::PLAYER_SEEK => {
            let params: SeekParams = musa_protocol::params(req)?;
            let library = state.library.read().unwrap();
            let duration = {
                let player = state.player.lock().unwrap();
                library
                    .songs_by_id
                    .get(player.queue.get(player.index).unwrap_or(&String::new()))
                    .map(|s| s.duration_ms)
                    .unwrap_or(0)
            };
            drop(library);
            {
                let mut player = state.player.lock().unwrap();
                player.position_ms = params.position_ms.min(duration);
                player.last_tick = Instant::now();
                player.last_emit = Instant::now();
            }
            state.broadcast_player();
            Ok(to_json(&state.player_state()))
        }
        cmd::PLAYER_SHUFFLE => {
            let params: ShuffleParams = musa_protocol::params(req)?;
            {
                let mut player = state.player.lock().unwrap();
                player.shuffle = params.enabled;
                if params.enabled && !player.queue.is_empty() {
                    let current = player.queue[player.index].clone();
                    let mut rest: Vec<String> = player
                        .queue
                        .iter()
                        .filter(|id| **id != current)
                        .cloned()
                        .collect();
                    shuffle_vec(&mut rest);
                    let mut new_queue = vec![current];
                    new_queue.extend(rest);
                    player.base_queue = Some(player.queue.clone());
                    player.queue = new_queue;
                    player.index = 0;
                    player.position_ms = player.position_ms.min(1);
                } else if !params.enabled {
                    if let Some(base) = player.base_queue.take() {
                        if let Some(current) = player.queue.get(player.index).cloned() {
                            player.index = base.iter().position(|id| id == &current).unwrap_or(0);
                            player.queue = base;
                        }
                    }
                }
                player.last_emit = Instant::now();
            }
            state.save_player();
            state.broadcast_player();
            Ok(to_json(&state.player_state()))
        }
        cmd::PLAYER_REPEAT => {
            let params: RepeatParams = musa_protocol::params(req)?;
            {
                let mut player = state.player.lock().unwrap();
                player.repeat = params.mode;
                player.last_emit = Instant::now();
            }
            state.save_player();
            state.broadcast_player();
            Ok(to_json(&state.player_state()))
        }
        cmd::PLAYER_VOLUME => {
            let params: VolumeParams = musa_protocol::params(req)?;
            {
                let mut player = state.player.lock().unwrap();
                player.volume = params.volume.min(100);
            }
            state.save_player();
            state.broadcast_player();
            Ok(to_json(&state.player_state()))
        }
        other => Err(ProtocolError::new(
            "unknown_command",
            format!("comando desconocido: {other}"),
        )),
    }
}

fn next_track(state: &Arc<Shared>, manual: bool) -> Result<(), ProtocolError> {
    {
        let mut player = state.player.lock().unwrap();
        if player.queue.is_empty() {
            return Err(ProtocolError::new("empty_queue", "no hay cola"));
        }
        if player.index + 1 < player.queue.len() {
            player.index += 1;
            player.position_ms = 0;
            player.playing = true;
        } else if player.repeat == RepeatMode::All || manual {
            player.index = 0;
            player.position_ms = 0;
            player.playing = true;
        } else {
            player.position_ms = 0;
            player.playing = false;
        }
        player.last_tick = Instant::now();
        player.last_emit = Instant::now();
    }
    state.save_player();
    state.broadcast_player();
    Ok(())
}

fn shuffle_vec(items: &mut [String]) {
    let mut seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9e3779b97f4a7c15)
        | 1;
    for i in (1..items.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let j = (seed % (i as u64 + 1)) as usize;
        items.swap(i, j);
    }
}

pub fn load_persisted_player(path: &Path) -> Option<PersistedPlayer> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn load_favorites(path: &Path) -> Option<Favorites> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}
