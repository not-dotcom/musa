//! Biblioteca en memoria, agregados y secciones de Inicio del daemon real.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use musa_protocol::{
    Album, AlbumDetail, AlbumSummary, Artist, Favorites, HomeItem, HomeSection, LibrarySnapshot,
    Playlist, PlaylistSummary, Song, SongSummary,
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

    pub fn artist_by_name(&self, name: &str) -> Option<&Artist> {
        self.artists_by_id
            .values()
            .find(|artist| artist.name.eq_ignore_ascii_case(name))
    }

    pub fn album_by_title_artist(&self, title: &str, artist_name: &str) -> Option<&Album> {
        self.albums_by_id.values().find(|album| {
            album.title.eq_ignore_ascii_case(title)
                && album.artist_name.eq_ignore_ascii_case(artist_name)
        })
    }

    /// Añade un álbum de catálogo con sus canciones. Si ya existe por id o por
    /// título + artista (por ejemplo, tras refrescar la biblioteca de Apple),
    /// reutiliza el álbum existente en vez de duplicarlo.
    pub fn add_album(&mut self, album: Album, songs: Vec<Song>) -> Album {
        let existing_id = if self.albums_by_id.contains_key(&album.id) {
            Some(album.id.clone())
        } else {
            self.album_by_title_artist(&album.title, &album.artist_name)
                .map(|existing| existing.id.clone())
        };
        let album_id = existing_id.clone().unwrap_or_else(|| album.id.clone());
        if existing_id.is_some() {
            if let Some(existing) = self.albums_by_id.get_mut(&album_id) {
                if !album.cover_path.is_empty() {
                    existing.cover_path = album.cover_path.clone();
                }
                if existing.genre.is_empty() {
                    existing.genre = album.genre.clone();
                }
                if existing.year == 0 {
                    existing.year = album.year;
                }
            }
        } else {
            let new_album = album.clone();
            self.snapshot.albums.push(new_album.clone());
            self.albums_by_id.insert(new_album.id.clone(), new_album);
        }
        let final_album = self
            .albums_by_id
            .get(&album_id)
            .cloned()
            .unwrap_or_else(|| album.clone());
        if let Some(slot) = self
            .snapshot
            .albums
            .iter_mut()
            .find(|existing| existing.id == final_album.id)
        {
            *slot = final_album.clone();
        }
        for mut song in songs {
            if self.songs_by_id.contains_key(&song.id) {
                continue;
            }
            song.album_id = final_album.id.clone();
            song.album_title = final_album.title.clone();
            song.artist_id = final_album.artist_id.clone();
            song.artist_name = final_album.artist_name.clone();
            self.snapshot.songs.push(song.clone());
            self.songs_by_id.insert(song.id.clone(), song);
        }
        let track_count = self
            .snapshot
            .songs
            .iter()
            .filter(|song| song.album_id == album_id)
            .count() as u32;
        if let Some(existing) = self.albums_by_id.get_mut(&album_id) {
            existing.track_count = track_count;
        }
        if let Some(slot) = self
            .snapshot
            .albums
            .iter_mut()
            .find(|existing| existing.id == album_id)
        {
            slot.track_count = track_count;
        }
        let final_album = self
            .albums_by_id
            .get(&album_id)
            .cloned()
            .unwrap_or(final_album);
        if !self.artists_by_id.contains_key(&final_album.artist_id) {
            let artist = Artist {
                id: final_album.artist_id.clone(),
                name: final_album.artist_name.clone(),
                genre: final_album.genre.clone(),
                photo_path: final_album.cover_path.clone(),
                album_count: 0,
                song_count: 0,
                favorite: false,
                followed: false,
            };
            self.snapshot.artists.push(artist.clone());
            self.artists_by_id.insert(artist.id.clone(), artist);
        }
        self.recompute_artist_counts();
        self.refresh_stats();
        self.refresh_home();
        final_album
    }

    /// Reaplica los álbumes añadidos desde Musa sobre la biblioteca que Apple
    /// devuelve al arrancar.
    pub fn merge_added_albums(&mut self, albums: Vec<AlbumDetail>) {
        for detail in albums {
            self.add_album(detail.album, detail.songs);
        }
    }

    fn recompute_artist_counts(&mut self) {
        let mut album_counts: HashMap<String, u32> = HashMap::new();
        let mut song_counts: HashMap<String, u32> = HashMap::new();
        for album in &self.snapshot.albums {
            *album_counts.entry(album.artist_id.clone()).or_default() += 1;
        }
        for song in &self.snapshot.songs {
            *song_counts.entry(song.artist_id.clone()).or_default() += 1;
        }
        for artist in &mut self.snapshot.artists {
            artist.album_count = album_counts.get(&artist.id).copied().unwrap_or(0);
            artist.song_count = song_counts.get(&artist.id).copied().unwrap_or(0);
        }
        for artist in self.artists_by_id.values_mut() {
            artist.album_count = album_counts.get(&artist.id).copied().unwrap_or(0);
            artist.song_count = song_counts.get(&artist.id).copied().unwrap_or(0);
        }
    }

    /// Artistas seguidos explícitamente. Es lo que se guarda en
    /// `~/.config/musa/artists.json` para sobrevivir a los refrescos de Apple.
    pub fn followed_artists(&self) -> Vec<Artist> {
        self.snapshot
            .artists
            .iter()
            .filter(|artist| artist.followed)
            .cloned()
            .collect()
    }

    /// Añade un artista de catálogo a la biblioteca. Si ya existe por id o por
    /// nombre, solo lo marca como seguido para no duplicarlo.
    pub fn follow_artist(&mut self, mut artist: Artist) -> Artist {
        if let Some(existing) = self.artists_by_id.get_mut(&artist.id) {
            existing.followed = true;
            existing.album_count = existing.album_count.max(artist.album_count);
            existing.song_count = existing.song_count.max(artist.song_count);
            if existing.genre.is_empty() || existing.genre == "Música" {
                existing.genre = artist.genre;
            }
            if !artist.photo_path.is_empty() {
                existing.photo_path = artist.photo_path;
            }
            let updated = existing.clone();
            if let Some(slot) = self
                .snapshot
                .artists
                .iter_mut()
                .find(|a| a.id == updated.id)
            {
                *slot = updated.clone();
            }
            self.refresh_stats();
            self.refresh_home();
            return updated;
        }
        if let Some(existing) = self.artist_by_name(&artist.name).cloned() {
            // Ya está en la biblioteca por sus canciones: no lo duplicamos.
            return existing;
        }
        artist.followed = true;
        self.snapshot.artists.push(artist.clone());
        self.artists_by_id.insert(artist.id.clone(), artist.clone());
        self.refresh_stats();
        self.refresh_home();
        artist
    }

    /// Quita un artista seguido. Si no tiene canciones ni álbumes propios en la
    /// biblioteca, se elimina por completo; si los tiene, solo deja de seguirse.
    /// Devuelve el artista actualizado y si se eliminó de la lista.
    pub fn unfollow_artist(&mut self, id: &str) -> Option<(Artist, bool)> {
        let mut artist = self.artists_by_id.get(id).cloned()?;
        let has_content = self.snapshot.songs.iter().any(|s| s.artist_id == id)
            || self.snapshot.albums.iter().any(|a| a.artist_id == id);
        artist.followed = false;
        if has_content {
            if let Some(existing) = self.artists_by_id.get_mut(id) {
                existing.followed = false;
            }
            if let Some(slot) = self.snapshot.artists.iter_mut().find(|a| a.id == id) {
                slot.followed = false;
            }
        } else {
            self.artists_by_id.remove(id);
            self.snapshot.artists.retain(|a| a.id != id);
            self.snapshot.favorites.artist_ids.retain(|fav| fav != id);
        }
        self.refresh_stats();
        self.refresh_home();
        Some((artist, !has_content))
    }

    /// Reaplica los artistas seguidos guardados en disco sobre una biblioteca
    /// recién descargada de Apple.
    pub fn merge_followed(&mut self, followed: Vec<Artist>) {
        for mut artist in followed {
            artist.followed = true;
            let existing_id = if let Some(existing) = self.artists_by_id.get(&artist.id) {
                Some(existing.id.clone())
            } else {
                self.artists_by_id
                    .values()
                    .find(|existing| existing.name.eq_ignore_ascii_case(&artist.name))
                    .map(|existing| existing.id.clone())
            };
            if let Some(id) = existing_id {
                if let Some(existing) = self.artists_by_id.get_mut(&id) {
                    existing.followed = true;
                }
                if let Some(slot) = self.snapshot.artists.iter_mut().find(|a| a.id == id) {
                    slot.followed = true;
                }
                continue;
            }
            self.snapshot.artists.push(artist.clone());
            self.artists_by_id.insert(artist.id.clone(), artist);
        }
        self.refresh_stats();
    }

    pub fn refresh_stats(&mut self) {
        self.snapshot.stats.songs = self.snapshot.songs.len() as u32;
        self.snapshot.stats.albums = self.snapshot.albums.len() as u32;
        self.snapshot.stats.artists = self.snapshot.artists.len() as u32;
        self.snapshot.stats.playlists = self.snapshot.playlists.len() as u32;
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

    let mut followed: Vec<Artist> = snapshot
        .artists
        .iter()
        .filter(|a| a.followed)
        .cloned()
        .collect();
    followed.extend(
        snapshot
            .artists
            .iter()
            .filter(|a| a.favorite && !a.followed)
            .cloned(),
    );
    sections.push(HomeSection {
        id: "artistas".into(),
        title: "Artistas que sigues".into(),
        subtitle: "Lo que has añadido y tus favoritos".into(),
        items: followed
            .into_iter()
            .take(10)
            .map(HomeItem::Artist)
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn artist(id: &str, name: &str) -> Artist {
        serde_json::from_value(json!({
            "id": id,
            "name": name,
            "genre": "Pop",
            "photo_path": "",
            "album_count": 0,
            "song_count": 0,
            "favorite": false,
        }))
        .unwrap()
    }

    fn library_with(artists: Vec<Artist>) -> Library {
        let snapshot: LibrarySnapshot = serde_json::from_value(json!({
            "artists": artists,
            "albums": [],
            "songs": [],
            "playlists": [],
            "home": [],
            "favorites": {
                "song_ids": [],
                "album_ids": [],
                "artist_ids": [],
                "playlist_ids": [],
            },
            "stats": {"songs": 0, "albums": 0, "artists": 0, "playlists": 0},
            "generated_at": "2026-01-01",
        }))
        .unwrap();
        Library::from_snapshot(snapshot)
    }

    fn album(id: &str, title: &str, artist_id: &str, artist_name: &str) -> Album {
        serde_json::from_value(json!({
            "id": id,
            "title": title,
            "artist_id": artist_id,
            "artist_name": artist_name,
            "year": 2013,
            "genre": "Rock",
            "track_count": 0,
            "cover_path": "",
            "favorite": false,
            "playable": true,
        }))
        .unwrap()
    }

    fn song(id: &str, title: &str, album: &Album, track: u32) -> Song {
        serde_json::from_value(json!({
            "id": id,
            "title": title,
            "artist_id": album.artist_id,
            "artist_name": album.artist_name,
            "album_id": album.id,
            "album_title": album.title,
            "track_number": track,
            "disc_number": 1,
            "duration_ms": 200000,
            "cover_path": "",
            "favorite": false,
            "playable": true,
            "explicit": false,
        }))
        .unwrap()
    }

    #[test]
    fn follow_adds_artist_and_stats() {
        let mut library = library_with(Vec::new());
        let followed = library.follow_artist(artist("cat-1", "Nina Simone"));
        assert!(followed.followed);
        assert_eq!(library.snapshot.stats.artists, 1);
        assert_eq!(library.followed_artists().len(), 1);
        assert!(library.artists_by_id.contains_key("cat-1"));
    }

    #[test]
    fn follow_does_not_duplicate_by_name() {
        let mut library = library_with(vec![artist("lib-1", "Nina Simone")]);
        let followed = library.follow_artist(artist("cat-1", "nina simone"));
        assert!(!followed.followed);
        assert_eq!(library.snapshot.artists.len(), 1);
        assert!(library.followed_artists().is_empty());
    }

    #[test]
    fn unfollow_removes_artist_without_content() {
        let mut library = library_with(Vec::new());
        library.follow_artist(artist("cat-1", "Nina Simone"));
        let (removed_artist, removed) = library.unfollow_artist("cat-1").unwrap();
        assert!(removed);
        assert!(!removed_artist.followed);
        assert!(!library.artists_by_id.contains_key("cat-1"));
        assert_eq!(library.snapshot.stats.artists, 0);
    }

    #[test]
    fn merge_followed_restores_persisted_artists() {
        let mut library = library_with(Vec::new());
        let mut persisted = artist("cat-1", "Nina Simone");
        persisted.followed = true;
        library.merge_followed(vec![persisted]);
        assert_eq!(library.snapshot.stats.artists, 1);
        assert!(library.artists_by_id.get("cat-1").unwrap().followed);
    }

    #[test]
    fn add_album_inserts_songs_and_artist() {
        let mut library = library_with(Vec::new());
        let album = album("cat-album", "Un Tributo", "cat-artist", "Varios Artistas");
        let songs = vec![song("s1", "Uno", &album, 1), song("s2", "Dos", &album, 2)];
        let added = library.add_album(album, songs);
        assert_eq!(added.id, "cat-album");
        assert_eq!(library.snapshot.albums.len(), 1);
        assert_eq!(library.snapshot.songs.len(), 2);
        assert_eq!(library.songs_of_album(&added.id).len(), 2);
        assert_eq!(library.snapshot.stats.albums, 1);
        assert_eq!(library.snapshot.stats.songs, 2);
        let artist = library.artists_by_id.get("cat-artist").unwrap();
        assert_eq!(artist.album_count, 1);
        assert_eq!(artist.song_count, 2);
    }

    #[test]
    fn add_album_does_not_duplicate_by_title_and_artist() {
        let mut library = library_with(Vec::new());
        let first = album("cat-album", "Un Tributo", "cat-artist", "Varios Artistas");
        library.add_album(
            first.clone(),
            vec![song("s1", "Uno", &first, 1), song("s2", "Dos", &first, 2)],
        );
        let other = album("otro-id", "un tributo", "otro-artista", "varios artistas");
        let added = library.add_album(
            other.clone(),
            vec![song("s2", "Dos", &other, 2), song("s3", "Tres", &other, 3)],
        );
        assert_eq!(added.id, "cat-album");
        assert_eq!(library.snapshot.albums.len(), 1);
        assert_eq!(library.snapshot.songs.len(), 3);
        assert_eq!(library.songs_of_album("cat-album").len(), 3);
    }
}
