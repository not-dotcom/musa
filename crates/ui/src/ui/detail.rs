//! Páginas de detalle: álbum, playlist y artista.

use musa_protocol::{Album, AlbumDetail, Artist, ArtistDetail, Playlist, Song};
use relm4::gtk;
use relm4::gtk::prelude::*;

use super::Actions;
use super::widgets::{
    album_card_button, collection_summary, cover_picture, heart_button, icon_pill_button,
    page_scroll, set_heart, song_rows,
};

impl Actions {
    pub fn open_album(&self, id: &str) {
        let data = self.data.borrow();
        let album = data.album(id).cloned();
        let songs: Vec<Song> = album
            .as_ref()
            .map(|album| {
                data.snapshot
                    .songs
                    .iter()
                    .filter(|song| song.album_id == album.id)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        drop(data);
        let Some(album) = album else {
            self.open_catalog_album(id);
            return;
        };
        *self.pending_detail.borrow_mut() = None;
        let page = album_detail(self, &album, &songs);
        self.open_detail(&page, &album.title);
    }

    pub fn open_playlist(&self, id: &str) {
        *self.pending_detail.borrow_mut() = None;
        let playlist = self.data.borrow().playlist(id).cloned();
        let Some(playlist) = playlist else {
            self.toast("Playlist no encontrada");
            return;
        };
        let page = playlist_detail(self, &playlist);
        self.open_detail(&page, &playlist.name);
    }

    pub fn open_artist(&self, id: &str) {
        let data = self.data.borrow();
        let artist = data.artist(id).cloned();
        let albums: Vec<Album> = artist
            .as_ref()
            .map(|artist| {
                data.snapshot
                    .albums
                    .iter()
                    .filter(|album| album.artist_id == artist.id)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let songs: Vec<Song> = artist
            .as_ref()
            .map(|artist| {
                data.snapshot
                    .songs
                    .iter()
                    .filter(|song| song.artist_id == artist.id)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        drop(data);
        let Some(artist) = artist else {
            self.open_catalog_artist(id);
            return;
        };
        *self.pending_detail.borrow_mut() = None;
        let page = artist_detail(self, &artist, &albums, &songs);
        self.open_detail(&page, &artist.name);
    }

    /// El álbum no está en la biblioteca: se pide al catálogo y, mientras
    /// tanto, se muestra una página de carga que se sustituye al llegar.
    fn open_catalog_album(&self, id: &str) {
        let key = format!("album:{id}");
        *self.pending_detail.borrow_mut() = Some(key.clone());
        self.open_detail(&loading_page("Cargando álbum…"), "Álbum");
        let actions = self.clone();
        self.client.call(
            musa_protocol::cmd::CATALOG_ALBUM,
            serde_json::json!({"id": id}),
            move |result| {
                if actions.pending_detail.borrow().as_deref() != Some(key.as_str()) {
                    return;
                }
                *actions.pending_detail.borrow_mut() = None;
                let detail = result
                    .map_err(|_| ())
                    .and_then(|value| serde_json::from_value::<AlbumDetail>(value).map_err(|_| ()));
                match detail {
                    Ok(detail) => {
                        let page = album_detail(&actions, &detail.album, &detail.songs);
                        actions.open_detail(&page, &detail.album.title);
                    }
                    Err(_) => {
                        actions.back();
                        actions.toast("No se pudo abrir el álbum. Inténtalo de nuevo.");
                    }
                }
            },
        );
    }

    fn open_catalog_artist(&self, id: &str) {
        let key = format!("artist:{id}");
        *self.pending_detail.borrow_mut() = Some(key.clone());
        self.open_detail(&loading_page("Cargando artista…"), "Artista");
        let actions = self.clone();
        self.client.call(
            musa_protocol::cmd::CATALOG_ARTIST,
            serde_json::json!({"id": id}),
            move |result| {
                if actions.pending_detail.borrow().as_deref() != Some(key.as_str()) {
                    return;
                }
                *actions.pending_detail.borrow_mut() = None;
                let detail = result.map_err(|_| ()).and_then(|value| {
                    serde_json::from_value::<ArtistDetail>(value).map_err(|_| ())
                });
                match detail {
                    Ok(detail) => {
                        let page =
                            artist_detail(&actions, &detail.artist, &detail.albums, &detail.songs);
                        actions.open_detail(&page, &detail.artist.name);
                    }
                    Err(_) => {
                        actions.back();
                        actions.toast("No se pudo abrir el artista. Inténtalo de nuevo.");
                    }
                }
            },
        );
    }
}

fn loading_page(message: &str) -> gtk::Widget {
    let box_ = gtk::Box::new(gtk::Orientation::Vertical, 12);
    box_.set_halign(gtk::Align::Center);
    box_.set_valign(gtk::Align::Center);
    box_.set_vexpand(true);
    box_.set_hexpand(true);
    let spinner = gtk::Spinner::new();
    spinner.start();
    spinner.set_size_request(28, 28);
    let label = gtk::Label::new(Some(message));
    label.add_css_class("muted");
    box_.append(&spinner);
    box_.append(&label);
    box_.upcast()
}

fn detail_top(actions: &Actions) -> gtk::Widget {
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    bar.add_css_class("detail-top");
    let back = gtk::Button::new();
    back.add_css_class("ghost-button");
    back.add_css_class("back-button");
    back.set_tooltip_text(Some("Volver"));
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    content.append(&gtk::Image::from_icon_name("go-previous-symbolic"));
    let label = gtk::Label::new(Some("Volver"));
    content.append(&label);
    back.set_child(Some(&content));
    let actions = actions.clone();
    back.connect_clicked(move |_| actions.back());
    bar.append(&back);
    bar.upcast()
}

fn favorite_button(actions: &Actions, kind: &str, id: &str, favorite: bool) -> gtk::Button {
    let button = heart_button(favorite);
    button.set_tooltip_text(Some(if favorite {
        "Quitar de favoritos"
    } else {
        "Añadir a favoritos"
    }));
    let actions = actions.clone();
    let kind = kind.to_string();
    let id = id.to_string();
    button.connect_clicked(move |button| {
        let now = !button.has_css_class("not-favorite");
        actions.toggle_favorite(&kind, &id, now);
        set_heart(button, now);
        button.set_tooltip_text(Some(if now {
            "Quitar de favoritos"
        } else {
            "Añadir a favoritos"
        }));
    });
    button
}

fn album_detail(actions: &Actions, album: &Album, songs: &[Song]) -> gtk::Widget {
    let is_local = actions.data.borrow().album(&album.id).is_some();
    let total: u64 = songs.iter().map(|s| s.duration_ms).sum();

    let layout = gtk::Box::new(gtk::Orientation::Vertical, 0);
    layout.append(&detail_top(actions));

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 26);
    header.add_css_class("detail-header");
    header.set_margin_start(30);
    header.set_margin_end(30);
    header.set_margin_bottom(18);
    let cover = cover_picture(actions, &album.cover_path, 218);
    cover.set_valign(gtk::Align::Start);
    header.append(&cover);

    let info = gtk::Box::new(gtk::Orientation::Vertical, 8);
    info.set_valign(gtk::Align::Center);
    info.set_hexpand(true);
    let eyebrow = gtk::Label::new(Some("ÁLBUM"));
    eyebrow.set_xalign(0.0);
    eyebrow.add_css_class("eyebrow");
    let title = gtk::Label::new(Some(&album.title));
    title.set_xalign(0.0);
    title.set_wrap(true);
    title.add_css_class("h1");
    let artist = gtk::Button::with_label(&album.artist_name);
    artist.add_css_class("link-button");
    artist.set_halign(gtk::Align::Start);
    let actions_artist = actions.clone();
    let artist_id = album.artist_id.clone();
    artist.connect_clicked(move |_| actions_artist.open_artist(&artist_id));
    let meta = gtk::Label::new(Some(&format!(
        "{} · {} · {}",
        album.year,
        album.genre,
        collection_summary(songs.len(), total)
    )));
    meta.set_xalign(0.0);
    meta.add_css_class("muted");
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    buttons.set_margin_top(8);
    if !songs.is_empty() {
        let play = icon_pill_button("Reproducir", "media-playback-start-symbolic");
        play.add_css_class("suggested-action");
        let songs_play = songs.to_vec();
        let actions_play = actions.clone();
        play.connect_clicked(move |_| actions_play.play_songs(&songs_play, 0));
        let shuffle = icon_pill_button("Aleatorio", "media-playlist-shuffle-symbolic");
        let songs_shuffle = songs.to_vec();
        let actions_shuffle = actions.clone();
        shuffle.connect_clicked(move |_| {
            let mut shuffled = songs_shuffle.clone();
            shuffle_songs(&mut shuffled);
            actions_shuffle.play_songs(&shuffled, 0);
        });
        buttons.append(&play);
        buttons.append(&shuffle);
    }
    // Los favoritos solo existen para la biblioteca local, no para el catálogo.
    if is_local {
        buttons.append(&favorite_button(
            actions,
            "album",
            &album.id,
            album.favorite,
        ));
    }
    info.append(&eyebrow);
    info.append(&title);
    info.append(&artist);
    info.append(&meta);
    info.append(&buttons);
    header.append(&info);
    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.append(&header);

    let separator = gtk::Separator::new(gtk::Orientation::Horizontal);
    separator.set_margin_start(26);
    separator.set_margin_end(26);
    body.append(&separator);
    body.append(&song_rows(actions, songs, false, false, is_local));
    layout.append(&page_scroll(&body));
    layout.upcast()
}

fn playlist_detail(actions: &Actions, playlist: &Playlist) -> gtk::Widget {
    let data = actions.data.borrow();
    let songs: Vec<Song> = playlist
        .song_ids
        .iter()
        .filter_map(|id| data.song(id).cloned())
        .collect();
    let total: u64 = songs.iter().map(|s| s.duration_ms).sum();
    drop(data);

    let layout = gtk::Box::new(gtk::Orientation::Vertical, 0);
    layout.append(&detail_top(actions));

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 26);
    header.add_css_class("detail-header");
    header.set_margin_start(30);
    header.set_margin_end(30);
    header.set_margin_bottom(18);
    let cover = cover_picture(actions, &playlist.cover_path, 218);
    cover.set_valign(gtk::Align::Start);
    header.append(&cover);

    let info = gtk::Box::new(gtk::Orientation::Vertical, 8);
    info.set_valign(gtk::Align::Center);
    info.set_hexpand(true);
    let eyebrow = gtk::Label::new(Some("PLAYLIST"));
    eyebrow.set_xalign(0.0);
    eyebrow.add_css_class("eyebrow");
    let title = gtk::Label::new(Some(&playlist.name));
    title.set_xalign(0.0);
    title.set_wrap(true);
    title.add_css_class("h1");
    let description = gtk::Label::new(Some(&playlist.description));
    description.set_xalign(0.0);
    description.set_wrap(true);
    description.set_max_width_chars(70);
    description.add_css_class("muted");
    let meta = gtk::Label::new(Some(&format!(
        "{} · {} · {}",
        playlist.curator,
        collection_summary(songs.len(), total),
        "selección Musa"
    )));
    meta.set_xalign(0.0);
    meta.add_css_class("muted-small");
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    buttons.set_margin_top(8);
    let play = icon_pill_button("Reproducir", "media-playback-start-symbolic");
    play.add_css_class("suggested-action");
    let songs_play = songs.clone();
    let actions_play = actions.clone();
    play.connect_clicked(move |_| actions_play.play_songs(&songs_play, 0));
    let shuffle = icon_pill_button("Aleatorio", "media-playlist-shuffle-symbolic");
    let songs_shuffle = songs.clone();
    let actions_shuffle = actions.clone();
    shuffle.connect_clicked(move |_| {
        let mut shuffled = songs_shuffle.clone();
        shuffle_songs(&mut shuffled);
        actions_shuffle.play_songs(&shuffled, 0);
    });
    buttons.append(&play);
    buttons.append(&shuffle);
    buttons.append(&favorite_button(
        actions,
        "playlist",
        &playlist.id,
        playlist.favorite,
    ));
    info.append(&eyebrow);
    info.append(&title);
    info.append(&description);
    info.append(&meta);
    info.append(&buttons);
    header.append(&info);
    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.append(&header);

    let separator = gtk::Separator::new(gtk::Orientation::Horizontal);
    separator.set_margin_start(26);
    separator.set_margin_end(26);
    body.append(&separator);
    body.append(&song_rows(actions, &songs, false, true, true));
    layout.append(&page_scroll(&body));
    layout.upcast()
}

fn artist_detail(
    actions: &Actions,
    artist: &Artist,
    albums: &[Album],
    songs: &[Song],
) -> gtk::Widget {
    let is_local = actions.data.borrow().artist(&artist.id).is_some();
    let layout = gtk::Box::new(gtk::Orientation::Vertical, 0);
    layout.append(&detail_top(actions));

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 26);
    header.add_css_class("detail-header");
    header.set_margin_start(30);
    header.set_margin_end(30);
    header.set_margin_bottom(12);
    let avatar = super::widgets::artist_avatar(actions, artist, 218);
    avatar.set_valign(gtk::Align::Start);
    header.append(&avatar);

    let info = gtk::Box::new(gtk::Orientation::Vertical, 8);
    info.set_valign(gtk::Align::Center);
    info.set_hexpand(true);
    let eyebrow = gtk::Label::new(Some("ARTISTA"));
    eyebrow.set_xalign(0.0);
    eyebrow.add_css_class("eyebrow");
    let title = gtk::Label::new(Some(&artist.name));
    title.set_xalign(0.0);
    title.set_wrap(true);
    title.add_css_class("h1");
    let meta = gtk::Label::new(Some(&format!(
        "{} · {} · {}",
        artist.genre,
        if albums.len() == 1 {
            "1 álbum".to_string()
        } else {
            format!("{} álbumes", albums.len())
        },
        if songs.len() == 1 {
            "1 canción".to_string()
        } else {
            format!("{} canciones", songs.len())
        }
    )));
    meta.set_xalign(0.0);
    meta.add_css_class("muted");
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    buttons.set_margin_top(8);
    if !songs.is_empty() {
        let play = icon_pill_button("Reproducir", "media-playback-start-symbolic");
        play.add_css_class("suggested-action");
        let songs_play = songs.to_vec();
        let actions_play = actions.clone();
        play.connect_clicked(move |_| actions_play.play_songs(&songs_play, 0));
        buttons.append(&play);
    }
    // Los favoritos solo existen para la biblioteca local, no para el catálogo.
    if is_local {
        buttons.append(&favorite_button(
            actions,
            "artist",
            &artist.id,
            artist.favorite,
        ));
    }
    info.append(&eyebrow);
    info.append(&title);
    info.append(&meta);
    info.append(&buttons);
    header.append(&info);
    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.append(&header);

    if !albums.is_empty() {
        let albums_title = gtk::Label::new(Some("Álbumes"));
        albums_title.set_xalign(0.0);
        albums_title.add_css_class("h2");
        albums_title.set_margin_start(30);
        albums_title.set_margin_top(10);
        body.append(&albums_title);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        row.set_margin_start(18);
        row.set_margin_end(18);
        row.set_margin_bottom(10);
        for album in albums {
            row.append(&album_card_button(actions, album));
        }
        let albums_scroll = gtk::ScrolledWindow::new();
        albums_scroll.set_policy(gtk::PolicyType::External, gtk::PolicyType::Never);
        albums_scroll.set_propagate_natural_height(true);
        albums_scroll.add_css_class("carousel");
        albums_scroll.set_child(Some(&row));
        body.append(&albums_scroll);
    }

    if !songs.is_empty() {
        let songs_title = gtk::Label::new(Some("Canciones"));
        songs_title.set_xalign(0.0);
        songs_title.add_css_class("h2");
        songs_title.set_margin_start(30);
        songs_title.set_margin_top(6);
        body.append(&songs_title);
        body.append(&song_rows(actions, songs, true, false, is_local));
    }
    layout.append(&page_scroll(&body));
    layout.upcast()
}

fn shuffle_songs(songs: &mut [Song]) {
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9e3779b97f4a7c15)
        | 1;
    for i in (1..songs.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let j = (seed % (i as u64 + 1)) as usize;
        songs.swap(i, j);
    }
}
