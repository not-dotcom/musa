//! Índices en memoria de la biblioteca y utilidades de presentación.

use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use musa_protocol::{Album, Artist, Favorites, HomeSection, LibrarySnapshot, Playlist, Song};

pub struct LibraryData {
    pub snapshot: LibrarySnapshot,
    pub songs_by_id: HashMap<String, Song>,
    pub albums_by_id: HashMap<String, Album>,
    pub artists_by_id: HashMap<String, Artist>,
    pub playlists_by_id: HashMap<String, Playlist>,
}

impl LibraryData {
    pub fn new(snapshot: LibrarySnapshot) -> Self {
        let songs_by_id = snapshot
            .songs
            .iter()
            .cloned()
            .map(|s| (s.id.clone(), s))
            .collect();
        let albums_by_id = snapshot
            .albums
            .iter()
            .cloned()
            .map(|a| (a.id.clone(), a))
            .collect();
        let artists_by_id = snapshot
            .artists
            .iter()
            .cloned()
            .map(|a| (a.id.clone(), a))
            .collect();
        let playlists_by_id = snapshot
            .playlists
            .iter()
            .cloned()
            .map(|p| (p.id.clone(), p))
            .collect();
        Self {
            snapshot,
            songs_by_id,
            albums_by_id,
            artists_by_id,
            playlists_by_id,
        }
    }

    pub fn song(&self, id: &str) -> Option<&Song> {
        self.songs_by_id.get(id)
    }

    pub fn album(&self, id: &str) -> Option<&Album> {
        self.albums_by_id.get(id)
    }

    pub fn artist(&self, id: &str) -> Option<&Artist> {
        self.artists_by_id.get(id)
    }

    pub fn playlist(&self, id: &str) -> Option<&Playlist> {
        self.playlists_by_id.get(id)
    }

    pub fn home(&self) -> &[HomeSection] {
        &self.snapshot.home
    }

    pub fn update_favorite(&mut self, kind: &str, id: &str, favorite: bool) {
        let list = match kind {
            "song" => &mut self.snapshot.favorites.song_ids,
            "album" => &mut self.snapshot.favorites.album_ids,
            "artist" => &mut self.snapshot.favorites.artist_ids,
            "playlist" => &mut self.snapshot.favorites.playlist_ids,
            _ => return,
        };
        if favorite {
            if !list.contains(&id.to_string()) {
                list.push(id.to_string());
            }
        } else {
            list.retain(|item| item != id);
        }
        self.sync_favorite_flags();
    }

    /// Reemplaza los favoritos completos (evento `library.changed`).
    pub fn apply_favorites(&mut self, favorites: &Favorites) {
        self.snapshot.favorites = favorites.clone();
        self.sync_favorite_flags();
    }

    fn sync_favorite_flags(&mut self) {
        let favorites = self.snapshot.favorites.clone();
        let songs: HashSet<&str> = favorites.song_ids.iter().map(String::as_str).collect();
        let albums: HashSet<&str> = favorites.album_ids.iter().map(String::as_str).collect();
        let artists: HashSet<&str> = favorites.artist_ids.iter().map(String::as_str).collect();
        let playlists: HashSet<&str> = favorites.playlist_ids.iter().map(String::as_str).collect();
        for song in self.songs_by_id.values_mut() {
            song.favorite = songs.contains(song.id.as_str());
        }
        for album in self.albums_by_id.values_mut() {
            album.favorite = albums.contains(album.id.as_str());
        }
        for artist in self.artists_by_id.values_mut() {
            artist.favorite = artists.contains(artist.id.as_str());
        }
        for playlist in self.playlists_by_id.values_mut() {
            playlist.favorite = playlists.contains(playlist.id.as_str());
        }
        for song in &mut self.snapshot.songs {
            song.favorite = songs.contains(song.id.as_str());
        }
        for album in &mut self.snapshot.albums {
            album.favorite = albums.contains(album.id.as_str());
        }
        for artist in &mut self.snapshot.artists {
            artist.favorite = artists.contains(artist.id.as_str());
        }
        for playlist in &mut self.snapshot.playlists {
            playlist.favorite = playlists.contains(playlist.id.as_str());
        }
    }

    pub fn favorite_songs(&self) -> Vec<Song> {
        self.snapshot
            .favorites
            .song_ids
            .iter()
            .filter_map(|id| self.songs_by_id.get(id).cloned())
            .collect()
    }

    pub fn favorite_albums(&self) -> Vec<Album> {
        self.snapshot
            .favorites
            .album_ids
            .iter()
            .filter_map(|id| self.albums_by_id.get(id).cloned())
            .collect()
    }

    pub fn favorite_artists(&self) -> Vec<Artist> {
        self.snapshot
            .favorites
            .artist_ids
            .iter()
            .filter_map(|id| self.artists_by_id.get(id).cloned())
            .collect()
    }

    pub fn favorite_playlists(&self) -> Vec<Playlist> {
        self.snapshot
            .favorites
            .playlist_ids
            .iter()
            .filter_map(|id| self.playlists_by_id.get(id).cloned())
            .collect()
    }
}

pub fn duration(ms: u64) -> String {
    let total = ms / 1000;
    format!("{}:{:02}", total / 60, total % 60)
}

pub fn total_duration(ms: u64) -> String {
    let minutes = ms / 60_000;
    if minutes >= 60 {
        format!("{} h {} min", minutes / 60, minutes % 60)
    } else {
        format!("{minutes} min")
    }
}

pub fn greeting() -> &'static str {
    let hour = local_hour();
    match hour {
        5..=11 => "Buenos días",
        12..=19 => "Buenas tardes",
        _ => "Buenas noches",
    }
}

fn local_hour() -> u32 {
    // Sin chrono: usamos la hora local del proceso si está disponible.
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // `date` es la fuente más fiable para la zona horaria configurada.
    if let Ok(output) = std::process::Command::new("date").arg("+%H").output() {
        if let Ok(text) = String::from_utf8(output.stdout) {
            if let Ok(hour) = text.trim().parse() {
                return hour;
            }
        }
    }
    ((secs / 3_600) % 24) as u32
}

/// Separador de miles español: 12.345, no 12345.
pub fn format_count(count: usize) -> String {
    let digits = count.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push('.');
        }
        out.push(ch);
    }
    out
}

pub fn plural(count: usize, singular: &str, plural: &str) -> String {
    format!(
        "{} {}",
        format_count(count),
        if count == 1 { singular } else { plural }
    )
}
