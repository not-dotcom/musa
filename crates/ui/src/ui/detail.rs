//! Páginas de detalle: álbum, playlist y artista.

use musa_protocol::{
    Album, AlbumDetail, Artist, ArtistDetail, CatalogResults, Playlist, PlaylistDetail, Song,
};
use relm4::gtk;
use relm4::gtk::prelude::*;

use super::Actions;
use super::widgets::{
    album_card_button, collection_summary, cover_picture, heart_button, icon_pill_button,
    page_scroll, playlist_card_button, set_heart, song_rows,
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
        let library_playlist = self.data.borrow().playlist(id).cloned();
        match library_playlist {
            Some(playlist) => {
                let data = self.data.borrow();
                let songs: Vec<Song> = playlist
                    .song_ids
                    .iter()
                    .filter_map(|id| data.song(id).cloned())
                    .collect();
                drop(data);
                let page = playlist_detail(self, &playlist, &songs, true);
                self.open_detail(&page, &playlist.name);
            }
            // Playlist de Apple Music (perfil de un artista): se pide al
            // catálogo como los álbumes.
            None => self.open_catalog_playlist(id),
        }
    }

    fn open_catalog_playlist(&self, id: &str) {
        let key = format!("playlist:{id}");
        *self.pending_detail.borrow_mut() = Some(key.clone());
        self.open_detail(&loading_page("Cargando playlist…"), "Playlist");
        let actions = self.clone();
        self.client.call(
            musa_protocol::cmd::CATALOG_PLAYLIST,
            serde_json::json!({"id": id}),
            move |result| {
                if actions.pending_detail.borrow().as_deref() != Some(key.as_str()) {
                    return;
                }
                *actions.pending_detail.borrow_mut() = None;
                let detail = result.map_err(|_| ()).and_then(|value| {
                    serde_json::from_value::<PlaylistDetail>(value).map_err(|_| ())
                });
                match detail {
                    Ok(detail) => {
                        let page =
                            playlist_detail(&actions, &detail.playlist, &detail.songs, false);
                        actions.open_detail(&page, &detail.playlist.name);
                    }
                    Err(_) => {
                        actions.back();
                        actions.toast("No se pudo abrir la playlist. Inténtalo de nuevo.");
                    }
                }
            },
        );
    }

    pub fn open_artist(&self, id: &str) {
        // El perfil de la biblioteca solo contiene lo guardado en Musa y puede
        // quedar incompleto: siempre se abre la ficha de Apple Music y la local
        // se reserva como respaldo.
        let local = self.data.borrow().artist(id).cloned();
        match local {
            Some(artist) if is_synthetic_artist_id(&artist.id) => {
                self.open_catalog_artist_by_name(&artist.id, &artist.name);
            }
            Some(artist) => self.load_catalog_artist(&artist.id, Some(&artist.id)),
            None => self.load_catalog_artist(id, None),
        }
    }

    /// Ficha local de un artista: solo sus álbumes y canciones de la
    /// biblioteca. Se usa cuando Apple Music no tiene (o no devuelve) al
    /// artista.
    fn open_local_artist(&self, id: &str) {
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
        // Playlists de la biblioteca que incluyen alguna canción del artista.
        let playlists: Vec<Playlist> = artist
            .as_ref()
            .map(|artist| {
                let ids: std::collections::HashSet<&str> = songs
                    .iter()
                    .filter(|song| song.artist_id == artist.id)
                    .map(|song| song.id.as_str())
                    .collect();
                data.snapshot
                    .playlists
                    .iter()
                    .filter(|playlist| playlist.song_ids.iter().any(|id| ids.contains(id.as_str())))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        drop(data);
        let Some(artist) = artist else {
            self.back();
            self.toast("Artista no encontrado");
            return;
        };
        *self.pending_detail.borrow_mut() = None;
        let page = artist_detail(self, &artist, &albums, &songs, &playlists);
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

    /// Pide al catálogo la ficha del artista. `fallback` es el id local que se
    /// pinta si Apple no devuelve al artista (sin conexión, por ejemplo).
    fn load_catalog_artist(&self, id: &str, fallback: Option<&str>) {
        let key = format!("artist:{id}");
        *self.pending_detail.borrow_mut() = Some(key.clone());
        self.open_detail(&loading_page("Cargando artista…"), "Artista");
        let actions = self.clone();
        let fallback = fallback.map(|id| id.to_string());
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
                        let page = artist_detail(
                            &actions,
                            &detail.artist,
                            &detail.albums,
                            &detail.songs,
                            &detail.playlists,
                        );
                        actions.open_detail(&page, &detail.artist.name);
                    }
                    Err(_) => match fallback {
                        Some(local_id) => actions.open_local_artist(&local_id),
                        None => {
                            actions.back();
                            actions.toast("No se pudo abrir el artista. Inténtalo de nuevo.");
                        }
                    },
                }
            },
        );
    }

    /// Los artistas de la biblioteca llevan ids sintéticos (`artist-lib-…`)
    /// que Apple no reconoce. Se resuelve el id real del catálogo buscando por
    /// nombre y, si no aparece, se queda la ficha local.
    fn open_catalog_artist_by_name(&self, local_id: &str, name: &str) {
        let key = format!("artist-name:{local_id}");
        *self.pending_detail.borrow_mut() = Some(key.clone());
        self.open_detail(
            &loading_page("Buscando al artista en Apple Music…"),
            "Artista",
        );
        let actions = self.clone();
        let name = name.to_string();
        let local_id = local_id.to_string();
        self.client.call(
            musa_protocol::cmd::SEARCH_CATALOG,
            serde_json::json!({"query": name.clone(), "limit": 20}),
            move |result| {
                if actions.pending_detail.borrow().as_deref() != Some(key.as_str()) {
                    return;
                }
                *actions.pending_detail.borrow_mut() = None;
                let found = result
                    .ok()
                    .and_then(|value| serde_json::from_value::<CatalogResults>(value).ok())
                    .and_then(|results| {
                        results
                            .artists
                            .into_iter()
                            .find(|candidate| same_artist_name(&candidate.name, &name))
                    });
                match found {
                    Some(artist) => actions.load_catalog_artist(&artist.id, Some(&local_id)),
                    None => actions.open_local_artist(&local_id),
                }
            },
        );
    }
}

/// Los ids de artista de la biblioteca son hashes locales, no ids de Apple.
fn is_synthetic_artist_id(id: &str) -> bool {
    id.starts_with("artist-")
}

/// Compara nombres de artista ignorando mayúsculas, acentos y puntuación:
/// Apple y la biblioteca no siempre los escriben igual.
fn same_artist_name(left: &str, right: &str) -> bool {
    fn normalize(name: &str) -> String {
        name.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(|c| c.to_lowercase())
            .collect()
    }
    let normalized = normalize(right);
    !normalized.is_empty() && normalize(left) == normalized
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

fn follow_button(
    actions: &Actions,
    artist_id: &str,
    followed: bool,
    add_label: &str,
) -> gtk::Button {
    let button = icon_pill_button("", "list-add-symbolic");
    set_follow_button(&button, followed, add_label);
    let actions = actions.clone();
    let id = artist_id.to_string();
    let add_label = add_label.to_string();
    button.connect_clicked(move |button| {
        let now = !button.has_css_class("following");
        button.set_sensitive(false);
        let button = button.clone();
        let actions_cb = actions.clone();
        let id_cb = id.clone();
        let add_label = add_label.clone();
        actions.set_artist_followed(&id, now, move |ok| {
            button.set_sensitive(true);
            if ok {
                let actual = actions_cb
                    .data
                    .borrow()
                    .artist(&id_cb)
                    .is_some_and(|artist| artist.followed);
                set_follow_button(&button, actual, &add_label);
            }
        });
    });
    button
}

fn set_follow_button(button: &gtk::Button, followed: bool, add_label: &str) {
    if followed {
        button.add_css_class("following");
    } else {
        button.remove_css_class("following");
    }
    let (label, icon) = if followed {
        ("Siguiendo", "emblem-ok-symbolic")
    } else {
        (add_label, "list-add-symbolic")
    };
    button.set_tooltip_text(Some(if followed {
        "Quitar de tu biblioteca"
    } else {
        "Añadir a tu biblioteca"
    }));
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    let image = gtk::Image::from_icon_name(icon);
    image.set_pixel_size(16);
    let text = gtk::Label::new(Some(label));
    content.append(&image);
    content.append(&text);
    button.set_child(Some(&content));
}

/// Apple usa entradas genéricas para recopilatorios que no son artistas
/// seguibles (su ficha de catálogo devuelve 404). No ofrecemos seguirlas.
fn is_placeholder_artist(name: &str) -> bool {
    matches!(
        name.trim().to_lowercase().as_str(),
        "varios artistas"
            | "various artists"
            | "artista desconocido"
            | "unknown artist"
            | "vv. aa."
            | "vvaa"
    )
}

fn add_album_button(actions: &Actions, album_id: &str) -> gtk::Button {
    let button = icon_pill_button("Añadir a la biblioteca", "list-add-symbolic");
    let actions = actions.clone();
    let id = album_id.to_string();
    button.connect_clicked(move |button| {
        button.set_sensitive(false);
        let button = button.clone();
        actions.add_album(&id, move |ok| {
            if ok {
                set_album_added(&button);
            } else {
                button.set_sensitive(true);
            }
        });
    });
    button
}

fn set_album_added(button: &gtk::Button) {
    button.set_sensitive(false);
    button.set_tooltip_text(Some("En tu biblioteca"));
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    let image = gtk::Image::from_icon_name("emblem-ok-symbolic");
    image.set_pixel_size(16);
    let text = gtk::Label::new(Some("En tu biblioteca"));
    content.append(&image);
    content.append(&text);
    button.set_child(Some(&content));
}

fn album_detail(actions: &Actions, album: &Album, songs: &[Song]) -> gtk::Widget {
    let local_album = {
        let data = actions.data.borrow();
        data.album(&album.id).cloned().or_else(|| {
            data.album_by_title_artist(&album.title, &album.artist_name)
                .cloned()
        })
    };
    let is_local = local_album.is_some();
    let favorite_album_id = local_album
        .as_ref()
        .map(|local| local.id.clone())
        .unwrap_or_else(|| album.id.clone());
    let favorite = local_album.as_ref().is_some_and(|local| local.favorite);
    let songs_in_library = songs
        .first()
        .is_some_and(|song| actions.data.borrow().song(&song.id).is_some());
    let album_artist = {
        let data = actions.data.borrow();
        data.artist(&album.artist_id)
            .cloned()
            .or_else(|| data.artist_by_name(&album.artist_name).cloned())
    };
    let artist_followed = album_artist.as_ref().is_some_and(|artist| artist.followed);
    let follow_artist_id = album_artist
        .as_ref()
        .filter(|artist| artist.followed)
        .map(|artist| artist.id.clone())
        .unwrap_or_else(|| album.artist_id.clone());
    // Solo se puede seguir un artista de catálogo. Los ids sintéticos
    // (`artist-…`) son de la biblioteca y no existen en el catálogo de Apple.
    let can_follow_artist = !is_placeholder_artist(&album.artist_name)
        && !follow_artist_id.is_empty()
        && (artist_followed || (!is_local && !follow_artist_id.starts_with("artist-")));
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
            &favorite_album_id,
            favorite,
        ));
    } else {
        buttons.append(&add_album_button(actions, &album.id));
    }
    if can_follow_artist {
        buttons.append(&follow_button(
            actions,
            &follow_artist_id,
            artist_followed,
            "Seguir artista",
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
    body.append(&song_rows(actions, songs, false, false, songs_in_library));
    layout.append(&page_scroll(&body));
    layout.upcast()
}

fn playlist_detail(
    actions: &Actions,
    playlist: &Playlist,
    songs: &[Song],
    is_local: bool,
) -> gtk::Widget {
    let total: u64 = songs.iter().map(|s| s.duration_ms).sum();

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
    description.set_visible(!playlist.description.is_empty());
    let meta = gtk::Label::new(Some(&format!(
        "{} · {}{}",
        playlist.curator,
        collection_summary(songs.len(), total),
        if is_local { " · selección Musa" } else { "" }
    )));
    meta.set_xalign(0.0);
    meta.add_css_class("muted-small");
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    buttons.set_margin_top(8);
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
    // Los favoritos son de la biblioteca: una playlist de Apple Music no se
    // puede marcar hasta que esté en ella.
    if is_local {
        buttons.append(&favorite_button(
            actions,
            "playlist",
            &playlist.id,
            playlist.favorite,
        ));
    }
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
    body.append(&song_rows(actions, songs, false, true, is_local));
    layout.append(&page_scroll(&body));
    layout.upcast()
}

fn artist_detail(
    actions: &Actions,
    artist: &Artist,
    albums: &[Album],
    songs: &[Song],
    playlists: &[Playlist],
) -> gtk::Widget {
    let local_artist = {
        let data = actions.data.borrow();
        data.artist(&artist.id)
            .cloned()
            .or_else(|| data.artist_by_name(&artist.name).cloned())
    };
    let is_local = local_artist.is_some();
    let is_followed = local_artist.as_ref().is_some_and(|local| local.followed);
    let favorite_id = local_artist
        .as_ref()
        .map(|local| local.id.clone())
        .unwrap_or_else(|| artist.id.clone());
    let favorite = local_artist.as_ref().is_some_and(|local| local.favorite);
    let songs_in_library = songs
        .first()
        .is_some_and(|song| actions.data.borrow().song(&song.id).is_some());
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
    // Los artistas de la biblioteca se pueden favoritear; los de catálogo se
    // pueden añadir (o dejar de seguir si ya se añadieron).
    if is_local {
        buttons.append(&favorite_button(actions, "artist", &favorite_id, favorite));
    }
    if !is_placeholder_artist(&artist.name) && (!is_local || is_followed) {
        buttons.append(&follow_button(
            actions,
            &artist.id,
            is_followed,
            "Añadir a la biblioteca",
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
        body.append(&song_rows(actions, songs, true, false, songs_in_library));
    }

    // Las playlists van al final, después de la discografía y las canciones.
    if !playlists.is_empty() {
        let lists_title = gtk::Label::new(Some("Playlists"));
        lists_title.set_xalign(0.0);
        lists_title.add_css_class("h2");
        lists_title.set_margin_start(30);
        lists_title.set_margin_top(10);
        body.append(&lists_title);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        row.set_margin_start(18);
        row.set_margin_end(18);
        row.set_margin_bottom(16);
        for playlist in playlists {
            row.append(&playlist_card_button(actions, playlist));
        }
        let lists_scroll = gtk::ScrolledWindow::new();
        lists_scroll.set_policy(gtk::PolicyType::External, gtk::PolicyType::Never);
        lists_scroll.set_propagate_natural_height(true);
        lists_scroll.add_css_class("carousel");
        lists_scroll.set_child(Some(&row));
        body.append(&lists_scroll);
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
