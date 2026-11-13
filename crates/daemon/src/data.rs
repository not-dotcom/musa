//! Biblioteca en memoria, agregados y secciones de Inicio del daemon real.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use musa_protocol::{
    Album, AlbumSummary, Artist, Favorites, HomeItem, HomeSection, LibrarySnapshot, Playlist,
    PlaylistSummary, Song, SongSummary,
};

#[derive(Debug, Clone)]
pub struct Library {
    pub snapshot: LibrarySnapshot,
    pub songs_by_id: HashMap<String, Song>,
    pub albums_by_id: HashMap<String, Album>,
    pub artists_by_id: HashMap<String, Artist>,
    pub playlists_by_id: HashMap<String, Playlist>,
}

impl Library {
    pub fn from_snapshot(snapshot: LibrarySnapshot) -> Self {
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

    pub fn songs_of_album(&self, album_id: &str) -> Vec<Song> {
        let mut songs: Vec<Song> = self
            .snapshot
            .songs
            .iter()
            .filter(|s| s.album_id == album_id)
            .cloned()
            .collect();
        songs.sort_by_key(|s| (s.disc_number, s.track_number));
        songs
    }

    pub fn albums_of_artist(&self, artist_id: &str) -> Vec<Album> {
        self.snapshot
            .albums
            .iter()
            .filter(|a| a.artist_id == artist_id)
            .cloned()
            .collect()
    }

    /// Vuelve a calcular los agregados que dependen de favoritos.
    pub fn apply_favorites(&mut self, favorites: &Favorites) {
        let song_set: HashSet<&str> = favorites.song_ids.iter().map(String::as_str).collect();
        let album_set: HashSet<&str> = favorites.album_ids.iter().map(String::as_str).collect();
        let artist_set: HashSet<&str> = favorites.artist_ids.iter().map(String::as_str).collect();
        let playlist_set: HashSet<&str> =
            favorites.playlist_ids.iter().map(String::as_str).collect();
        for song in &mut self.snapshot.songs {
            song.favorite = song_set.contains(song.id.as_str());
        }
        for album in &mut self.snapshot.albums {
            album.favorite = album_set.contains(album.id.as_str());
        }
        for artist in &mut self.snapshot.artists {
            artist.favorite = artist_set.contains(artist.id.as_str());
        }
        for playlist in &mut self.snapshot.playlists {
            playlist.favorite = playlist_set.contains(playlist.id.as_str());
        }
        self.snapshot.favorites = favorites.clone();
        *self = Self::from_snapshot(self.snapshot.clone());
    }

    pub fn refresh_home(&mut self) {
        self.snapshot.home = build_home(&self.snapshot);
    }
}

pub fn build_home(snapshot: &LibrarySnapshot) -> Vec<HomeSection> {
    let mut sections = Vec::new();

    let mut recommended: Vec<_> = snapshot
        .albums
        .iter()
        .filter(|a| a.favorite)
        .take(8)
        .cloned()
        .chain(snapshot.albums.iter().take(4).cloned())
        .collect();
    recommended.dedup_by(|a, b| a.id == b.id);
    sections.push(HomeSection {
        id: "escuchar-ahora".into(),
        title: "Escuchar ahora".into(),
        subtitle: "Mezclado para ti a partir de tus favoritos".into(),
        items: recommended
            .into_iter()
            .map(|a| HomeItem::Album(summary_album(&a)))
            .collect(),
    });

    let mut new_albums = snapshot.albums.clone();
    new_albums.sort_by(|a, b| b.year.cmp(&a.year).then(a.title.cmp(&b.title)));
    sections.push(HomeSection {
        id: "nuevos".into(),
        title: "Nuevos lanzamientos".into(),
        subtitle: "Lo último de tu biblioteca".into(),
        items: new_albums
            .iter()
            .take(12)
            .map(|a| HomeItem::Album(summary_album(a)))
            .collect(),
    });

    let fav_songs: Vec<_> = snapshot
        .songs
        .iter()
        .filter(|s| s.favorite)
        .take(12)
        .collect();
    sections.push(HomeSection {
        id: "favoritas".into(),
        title: "Tus canciones favoritas".into(),
        subtitle: "Un atajo a lo que ya te gusta".into(),
        items: fav_songs
            .into_iter()
            .map(|s| HomeItem::Song(summary_song(s)))
            .collect(),
    });

    sections.push(HomeSection {
        id: "playlists".into(),
        title: "Playlists para ti".into(),
        subtitle: "Selecciones de tu biblioteca".into(),
        items: snapshot
            .playlists
            .iter()
            .take(10)
            .map(|p| HomeItem::Playlist(summary_playlist(p)))
            .collect(),
    });

    sections.push(HomeSection {
        id: "artistas".into(),
        title: "Artistas que sigues".into(),
        subtitle: "Nuevo material de tus favoritos".into(),
        items: snapshot
            .artists
            .iter()
            .filter(|a| a.favorite)
            .take(10)
            .map(|a| HomeItem::Artist(a.clone()))
            .collect(),
    });

    sections
}

pub fn summary_album(a: &Album) -> AlbumSummary {
    AlbumSummary {
        id: a.id.clone(),
        title: a.title.clone(),
        artist_name: a.artist_name.clone(),
        year: a.year,
        cover_path: a.cover_path.clone(),
        track_count: a.track_count,
    }
}

pub fn summary_song(s: &Song) -> SongSummary {
    SongSummary {
        id: s.id.clone(),
        title: s.title.clone(),
        artist_name: s.artist_name.clone(),
        album_title: s.album_title.clone(),
        duration_ms: s.duration_ms,
        cover_path: s.cover_path.clone(),
    }
}

pub fn summary_playlist(p: &Playlist) -> PlaylistSummary {
    PlaylistSummary {
        id: p.id.clone(),
        name: p.name.clone(),
        description: p.description.clone(),
        curator: p.curator.clone(),
        cover_path: p.cover_path.clone(),
        track_count: p.track_count,
    }
}

pub fn default_cache_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_CACHE_HOME") {
        return PathBuf::from(dir).join("musa");
    }
    std::env::var("HOME")
        .map(|h| PathBuf::from(h).join(".cache/musa"))
        .unwrap_or_else(|_| PathBuf::from("/tmp/musa"))
}

pub fn default_config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(dir).join("musa");
    }
    std::env::var("HOME")
        .map(|h| PathBuf::from(h).join(".config/musa"))
        .unwrap_or_else(|_| PathBuf::from("/tmp/musa-config"))
}
