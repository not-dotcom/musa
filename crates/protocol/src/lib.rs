//! Protocolo entre la interfaz (Musa) y el daemon (musad).
//!
//! Transporte: socket Unix, un objeto JSON por línea (`\n`), UTF-8.
//! Versión actual: [`PROTOCOL_VERSION`]. Cada mensaje lleva `v` para poder
//! rechazar clientes incompatibles sin ambigüedad. La especificación completa
//! está en `docs/PROTOCOLO.md`.
//!
//! Tres clases de mensaje:
//! - [`Request`]: `{"v":1,"id":7,"cmd":"albums.list","params":{...}}`
//! - [`Response`]: `{"v":1,"id":7,"ok":true,"data":{...}}`
//! - [`Event`]: `{"v":1,"event":"player.changed","data":{...}}`

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const PROTOCOL_VERSION: u32 = 1;

/// Longitud máxima aceptada para una línea del protocolo (1 MiB).
pub const MAX_LINE_BYTES: usize = 1024 * 1024;

// ---------------------------------------------------------------------------
// Mensajes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub v: u32,
    pub id: u64,
    pub cmd: String,
    #[serde(default)]
    pub params: Value,
}

impl Request {
    pub fn new(id: u64, cmd: impl Into<String>, params: Value) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            id,
            cmd: cmd.into(),
            params,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub v: u32,
    pub id: u64,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ProtocolError>,
}

impl Response {
    pub fn ok(id: u64, data: Value) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            id,
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn err(id: u64, code: &str, message: impl Into<String>) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            id,
            ok: false,
            data: None,
            error: Some(ProtocolError {
                code: code.into(),
                message: message.into(),
            }),
        }
    }

    pub fn into_result(self) -> Result<Value, ProtocolError> {
        if self.ok {
            Ok(self.data.unwrap_or(Value::Null))
        } else {
            Err(self
                .error
                .unwrap_or_else(|| ProtocolError::new("unknown", "respuesta sin detalle")))
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
#[error("{code}: {message}")]
pub struct ProtocolError {
    pub code: String,
    pub message: String,
}

impl ProtocolError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub v: u32,
    pub event: String,
    pub data: Value,
}

impl Event {
    pub fn new(event: impl Into<String>, data: Value) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            event: event.into(),
            data,
        }
    }
}

// ---------------------------------------------------------------------------
// Comandos
// ---------------------------------------------------------------------------

pub mod cmd {
    pub const HELLO: &str = "hello";
    pub const LIBRARY_GET: &str = "library.get";
    pub const ALBUMS_LIST: &str = "albums.list";
    pub const ALBUM_GET: &str = "album.get";
    pub const ARTISTS_LIST: &str = "artists.list";
    pub const ARTIST_GET: &str = "artist.get";
    pub const SONGS_LIST: &str = "songs.list";
    pub const PLAYLISTS_LIST: &str = "playlists.list";
    pub const PLAYLIST_GET: &str = "playlist.get";
    pub const FAVORITES_LIST: &str = "favorites.list";
    pub const FAVORITE_SET: &str = "favorite.set";
    /// Añade a la biblioteca de Musa un artista de catálogo: `{id}`.
    pub const LIBRARY_ADD_ARTIST: &str = "library.add_artist";
    /// Quita de la biblioteca de Musa un artista seguido: `{id}`.
    pub const LIBRARY_REMOVE_ARTIST: &str = "library.remove_artist";
    /// Añade a la biblioteca un álbum de catálogo con sus canciones: `{id}`.
    pub const LIBRARY_ADD_ALBUM: &str = "library.add_album";
    pub const HOME_GET: &str = "home.get";
    pub const SEARCH: &str = "search";
    /// Busca canciones, álbumes y artistas en el catálogo de Apple Music.
    pub const SEARCH_CATALOG: &str = "search.catalog";
    /// Detalle de un álbum del catálogo: `{album, songs}`.
    pub const CATALOG_ALBUM: &str = "catalog.album";
    /// Detalle de un artista del catálogo: `{artist, albums, songs}`.
    pub const CATALOG_ARTIST: &str = "catalog.artist";
    pub const LYRICS_GET: &str = "lyrics.get";
    pub const COVER_ENSURE: &str = "cover.ensure";
    pub const QUEUE_SET: &str = "queue.set";
    pub const QUEUE_GET: &str = "queue.get";
    pub const QUEUE_JUMP: &str = "queue.jump";
    pub const PLAYER_PLAY: &str = "player.play";
    pub const PLAYER_PAUSE: &str = "player.pause";
    pub const PLAYER_NEXT: &str = "player.next";
    pub const PLAYER_PREV: &str = "player.prev";
    pub const PLAYER_SEEK: &str = "player.seek";
    pub const PLAYER_SHUFFLE: &str = "player.shuffle";
    pub const PLAYER_VOLUME: &str = "player.volume";
    pub const PLAYER_REPEAT: &str = "player.repeat";
    pub const PLAYER_STATE: &str = "player.state";
    pub const PING: &str = "ping";
    /// Cierra el daemon y, en modo real, apaga el sidecar (pausa primero).
    pub const SHUTDOWN: &str = "shutdown";
}

pub mod event {
    pub const PLAYER_CHANGED: &str = "player.changed";
    pub const LIBRARY_CHANGED: &str = "library.changed";
    /// Un artista se ha añadido o quitado de la biblioteca.
    pub const ARTIST_CHANGED: &str = "library.artist_changed";
    /// Un álbum se ha añadido a la biblioteca.
    pub const ALBUM_CHANGED: &str = "library.album_changed";
    pub const ENGINE_CHANGED: &str = "engine.changed";
}

// ---------------------------------------------------------------------------
// Modelo de datos
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Artist {
    pub id: String,
    pub name: String,
    pub genre: String,
    /// Ruta absoluta a un JPEG cuadrado de la portada.
    pub photo_path: String,
    pub album_count: u32,
    pub song_count: u32,
    pub favorite: bool,
    /// `true` cuando el artista se ha añadido explícitamente a la biblioteca
    /// de Musa sin tener canciones propias en ella.
    #[serde(default)]
    pub followed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Album {
    pub id: String,
    pub title: String,
    pub artist_id: String,
    pub artist_name: String,
    pub year: u16,
    pub genre: String,
    pub track_count: u32,
    pub cover_path: String,
    pub favorite: bool,
    /// `true` cuando Apple Music marca la obra como no disponible.
    pub playable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Song {
    pub id: String,
    pub title: String,
    pub artist_id: String,
    pub artist_name: String,
    pub album_id: String,
    pub album_title: String,
    pub track_number: u32,
    pub disc_number: u32,
    pub duration_ms: u64,
    pub cover_path: String,
    pub favorite: bool,
    pub playable: bool,
    pub explicit: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub description: String,
    pub curator: String,
    pub cover_path: String,
    pub track_count: u32,
    pub song_ids: Vec<String>,
    pub favorite: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LyricsLine {
    pub time_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Lyrics {
    pub song_id: String,
    pub lines: Vec<LyricsLine>,
    /// `true` si los tiempos de las letras son reales.
    pub synced: bool,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlaylistSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub curator: String,
    pub cover_path: String,
    pub track_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AlbumSummary {
    pub id: String,
    pub title: String,
    pub artist_name: String,
    pub year: u16,
    pub cover_path: String,
    pub track_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SongSummary {
    pub id: String,
    pub title: String,
    pub artist_name: String,
    pub album_title: String,
    pub duration_ms: u64,
    pub cover_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HomeItem {
    Album(AlbumSummary),
    Playlist(PlaylistSummary),
    Song(SongSummary),
    Artist(Artist),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HomeSection {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub items: Vec<HomeItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LibraryStats {
    pub songs: u32,
    pub albums: u32,
    pub artists: u32,
    pub playlists: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LibrarySnapshot {
    pub artists: Vec<Artist>,
    pub albums: Vec<Album>,
    pub songs: Vec<Song>,
    pub playlists: Vec<Playlist>,
    pub home: Vec<HomeSection>,
    pub favorites: Favorites,
    pub stats: LibraryStats,
    pub generated_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Favorites {
    pub song_ids: Vec<String>,
    pub album_ids: Vec<String>,
    pub artist_ids: Vec<String>,
    pub playlist_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchResults {
    pub query: String,
    pub songs: Vec<Song>,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
    pub playlists: Vec<Playlist>,
}

/// Resultados del catálogo de Apple Music. Las canciones, álbumes y artistas
/// pueden no estar en la biblioteca y aun así abrirse y reproducirse por su id
/// de catálogo.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CatalogResults {
    pub query: String,
    pub songs: Vec<Song>,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
}

/// Detalle de un álbum del catálogo. Misma forma que `album.get`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AlbumDetail {
    pub album: Album,
    pub songs: Vec<Song>,
}

/// Detalle de un artista del catálogo. Misma forma que `artist.get`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtistDetail {
    pub artist: Artist,
    pub albums: Vec<Album>,
    pub songs: Vec<Song>,
}

/// Resultado de añadir o quitar un artista de la biblioteca.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtistChange {
    pub artist: Artist,
    /// Si Apple Music confirmó la operación. El guardado local es independiente.
    #[serde(default)]
    pub synced: bool,
    /// Detalle del fallo de sincronización, si lo hubo.
    #[serde(default)]
    pub sync_error: Option<String>,
    /// `true` cuando el artista se ha eliminado por completo de la biblioteca.
    #[serde(default)]
    pub removed: bool,
}

/// Resultado de añadir un álbum de catálogo a la biblioteca.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlbumChange {
    pub album: Album,
    pub songs: Vec<Song>,
    /// Si Apple Music confirmó la operación. El guardado local es independiente.
    #[serde(default)]
    pub synced: bool,
    /// Detalle del fallo de sincronización, si lo hubo.
    #[serde(default)]
    pub sync_error: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

impl RepeatMode {
    pub fn cycle(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Repetición desactivada",
            Self::All => "Repetir cola",
            Self::One => "Repetir canción",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlayerState {
    pub queue: Vec<String>,
    pub index: usize,
    pub position_ms: u64,
    pub playing: bool,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    /// 0..=100; 0 = silencio, 100 = volumen máximo.
    pub volume: u8,
    pub track: Option<Song>,
    /// Identificador del motor de audio. Con el sidecar es `"real"`.
    pub engine: String,
}

impl PlayerState {
    pub fn normalized_index(&self) -> Option<usize> {
        (!self.queue.is_empty()).then(|| self.index.min(self.queue.len() - 1))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hello {
    pub name: String,
    pub version: String,
    pub protocol: u32,
    pub mode: String,
    pub socket: String,
    pub library_stats: LibraryStats,
    pub features: Vec<String>,
}

// ---------------------------------------------------------------------------
// Helpers de validación
// ---------------------------------------------------------------------------

pub fn parse_request(line: &str) -> Result<Request, ProtocolError> {
    if line.len() > MAX_LINE_BYTES {
        return Err(ProtocolError::new("line_too_long", "línea mayor de 1 MiB"));
    }
    let req: Request = serde_json::from_str(line.trim())
        .map_err(|e| ProtocolError::new("bad_json", format!("JSON inválido: {e}")))?;
    if req.v != PROTOCOL_VERSION {
        return Err(ProtocolError::new(
            "bad_version",
            format!(
                "versión de protocolo {} no soportada (se espera {})",
                req.v, PROTOCOL_VERSION
            ),
        ));
    }
    Ok(req)
}

pub fn params<T: serde::de::DeserializeOwned>(req: &Request) -> Result<T, ProtocolError> {
    serde_json::from_value(req.params.clone())
        .map_err(|e| ProtocolError::new("bad_params", format!("parámetros inválidos: {e}")))
}

pub fn to_json<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

pub fn encode_line<T: Serialize>(value: &T) -> String {
    let mut s = serde_json::to_string(value).unwrap_or_else(|_| "{\"v\":1}".into());
    s.push('\n');
    s
}

pub fn ping_response(id: u64, now_ms: u64) -> Response {
    Response::ok(id, json!({ "pong": true, "now_ms": now_ms }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_request() {
        let req = Request::new(3, cmd::SEARCH, json!({"query":"blue"}));
        let line = encode_line(&req);
        let parsed = parse_request(&line).unwrap();
        assert_eq!(parsed.id, 3);
        assert_eq!(parsed.cmd, "search");
    }

    #[test]
    fn rejects_other_version() {
        let line = r#"{"v":99,"id":1,"cmd":"ping"}"#;
        let err = parse_request(line).unwrap_err();
        assert_eq!(err.code, "bad_version");
    }

    #[test]
    fn error_response_roundtrip() {
        let resp = Response::err(5, "not_found", "álbum inexistente");
        let decoded: Response = serde_json::from_str(&encode_line(&resp)).unwrap();
        assert!(decoded.clone().into_result().is_err());
        assert_eq!(decoded.error.unwrap().code, "not_found");
    }

    #[test]
    fn repeat_cycles() {
        assert_eq!(RepeatMode::Off.cycle(), RepeatMode::All);
        assert_eq!(RepeatMode::All.cycle(), RepeatMode::One);
        assert_eq!(RepeatMode::One.cycle(), RepeatMode::Off);
    }

    #[test]
    fn catalog_results_roundtrip() {
        let value = to_json(&CatalogResults {
            query: "nina simone".into(),
            songs: vec![],
            albums: vec![],
            artists: vec![],
        });
        let decoded: CatalogResults = serde_json::from_value(value).unwrap();
        assert_eq!(decoded.query, "nina simone");
        assert!(decoded.songs.is_empty());
        assert!(decoded.albums.is_empty());
        assert!(decoded.artists.is_empty());
    }

    #[test]
    fn artist_without_followed_field_is_compatible() {
        let artist: Artist = serde_json::from_value(json!({
            "id": "r.123",
            "name": "Nina Simone",
            "genre": "Jazz",
            "photo_path": "",
            "album_count": 1,
            "song_count": 2,
            "favorite": false,
        }))
        .unwrap();
        assert!(!artist.followed);
    }

    #[test]
    fn artist_change_roundtrip() {
        let artist: Artist = serde_json::from_value(json!({
            "id": "cat-1",
            "name": "Nina Simone",
            "genre": "Jazz",
            "photo_path": "",
            "album_count": 2,
            "song_count": 3,
            "favorite": false,
            "followed": true,
        }))
        .unwrap();
        let change = ArtistChange {
            artist,
            synced: true,
            sync_error: None,
            removed: false,
        };
        let decoded: ArtistChange = serde_json::from_value(to_json(&change)).unwrap();
        assert!(decoded.artist.followed);
        assert!(decoded.synced);
    }
}
