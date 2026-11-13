//! Páginas principales de la biblioteca.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use musa_protocol::Song;
use relm4::gtk;
use relm4::gtk::prelude::*;

use super::Actions;
use super::widgets::{
    album_grid, artist_grid, content_box, cover_picture, home_sections, icon_pill_button,
    page_header, page_scroll, playlist_grid, song_list, song_list_header,
};
use crate::data::{format_count, plural, total_duration};

pub fn home_page(actions: &Actions) -> gtk::Widget {
    let data = actions.data.borrow();
    let content = content_box();
    content.append(&page_header(
        crate::data::greeting(),
        "Tu biblioteca, tus favoritos y algunas ideas para hoy.",
    ));

    let hero = gtk::Box::new(gtk::Orientation::Horizontal, 18);
    hero.add_css_class("hero");
    let hero_text = gtk::Box::new(gtk::Orientation::Vertical, 8);
    hero_text.set_valign(gtk::Align::Center);
    let kicker = gtk::Label::new(Some("MEZCLA PERSONAL"));
    kicker.set_xalign(0.0);
    kicker.add_css_class("eyebrow");
    let hero_title = gtk::Label::new(Some("Hecha para tus oídos"));
    hero_title.set_xalign(0.0);
    hero_title.add_css_class("hero-title");
    let stats = &data.snapshot.stats;
    let hero_copy = gtk::Label::new(Some(&format!(
        "{} canciones, {} álbumes y {} artistas en tu biblioteca.",
        format_count(stats.songs as usize),
        format_count(stats.albums as usize),
        format_count(stats.artists as usize),
    )));
    hero_copy.set_xalign(0.0);
    hero_copy.set_wrap(true);
    hero_copy.add_css_class("muted");
    let play = icon_pill_button("Reproducir mezcla", "media-playback-start-symbolic");
    play.add_css_class("suggested-action");
    let actions_play = actions.clone();
    let mix: Vec<Song> = data
        .snapshot
        .songs
        .iter()
        .filter(|s| s.favorite && s.playable)
        .take(40)
        .cloned()
        .collect();
    play.connect_clicked(move |_| actions_play.play_songs(&mix, 0));
    play.set_halign(gtk::Align::Start);
    hero_text.append(&kicker);
    hero_text.append(&hero_title);
    hero_text.append(&hero_copy);
    hero_text.append(&play);
    hero_text.set_hexpand(true);
    hero.append(&hero_text);

    let mosaic = gtk::Grid::new();
    mosaic.set_halign(gtk::Align::End);
    mosaic.set_valign(gtk::Align::Center);
    mosaic.set_row_spacing(6);
    mosaic.set_column_spacing(6);
    let mut covers = data
        .snapshot
        .albums
        .iter()
        .filter(|album| album.favorite)
        .take(4)
        .cloned()
        .collect::<Vec<_>>();
    if covers.len() < 4 {
        for album in data.snapshot.albums.iter().take(4 - covers.len()) {
            if !covers.iter().any(|c| c.id == album.id) {
                covers.push(album.clone());
            }
        }
    }
    for (i, album) in covers.iter().enumerate() {
        let picture = cover_picture(actions, &album.cover_path, 78);
        picture.add_css_class("hero-cover");
        mosaic.attach(&picture, i as i32 % 2, i as i32 / 2, 1, 1);
    }
    if !covers.is_empty() {
        hero.append(&mosaic);
    }
    content.append(&hero);

    // Inicio ya abre con la mezcla personal: la sección homónima del daemon
    // se omite aquí para no repetir la misma promesa dos veces.
    let sections: Vec<_> = data
        .home()
        .iter()
        .filter(|section| section.title != "Escuchar ahora" && !section.items.is_empty())
        .cloned()
        .collect();
    drop(data);
    content.append(&home_sections(actions, &sections));
    page_scroll(&content).upcast()
}

/// Hub editorial de Escuchar ahora. No repite el hero de Inicio: abre con una
/// mezcla destacada y reordena las secciones como descubrimiento.
pub fn listen_page(actions: &Actions) -> gtk::Widget {
    let data = actions.data.borrow();
    let content = content_box();
    content.append(&page_header(
        "Escuchar ahora",
        "Mezclas y novedades pensadas para lo que ya escuchas.",
    ));

    let mut mix: Vec<Song> = data
        .snapshot
        .songs
        .iter()
        .filter(|s| s.favorite && s.playable)
        .take(30)
        .cloned()
        .collect();
    if mix.is_empty() {
        mix = data
            .snapshot
            .songs
            .iter()
            .filter(|s| s.playable)
            .take(30)
            .cloned()
            .collect();
    }
    let total: u64 = mix.iter().map(|s| s.duration_ms).sum();
    let cover_path = mix
        .first()
        .and_then(|s| data.album(&s.album_id))
        .map(|a| a.cover_path.clone())
        .unwrap_or_default();

    let feature = gtk::Box::new(gtk::Orientation::Horizontal, 26);
    feature.add_css_class("listen-feature");
    let cover = cover_picture(actions, &cover_path, 190);
    cover.add_css_class("feature-cover");
    feature.append(&cover);

    let info = gtk::Box::new(gtk::Orientation::Vertical, 8);
    info.set_valign(gtk::Align::Center);
    info.set_hexpand(true);
    let eyebrow = gtk::Label::new(Some("PARA TI"));
    eyebrow.set_xalign(0.0);
    eyebrow.add_css_class("eyebrow");
    let title = gtk::Label::new(Some("Tu mezcla de la semana"));
    title.set_xalign(0.0);
    title.set_wrap(true);
    title.add_css_class("listen-title");
    let copy = gtk::Label::new(Some(&format!(
        "{} · {} · hecha con lo que más escuchas.",
        plural(mix.len(), "canción", "canciones"),
        total_duration(total)
    )));
    copy.set_xalign(0.0);
    copy.set_wrap(true);
    copy.add_css_class("muted");
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    buttons.set_margin_top(8);
    let play = icon_pill_button("Reproducir", "media-playback-start-symbolic");
    play.add_css_class("suggested-action");
    let mix_play = mix.clone();
    let actions_play = actions.clone();
    play.connect_clicked(move |_| actions_play.play_songs(&mix_play, 0));
    let shuffle = icon_pill_button("Aleatorio", "media-playlist-shuffle-symbolic");
    let mix_shuffle = mix.clone();
    let actions_shuffle = actions.clone();
    shuffle.connect_clicked(move |_| {
        let mut shuffled = mix_shuffle.clone();
        shuffle_songs(&mut shuffled);
        actions_shuffle.play_songs(&shuffled, 0);
    });
    buttons.append(&play);
    buttons.append(&shuffle);
    info.append(&eyebrow);
    info.append(&title);
    info.append(&copy);
    info.append(&buttons);
    feature.append(&info);

    let mosaic = gtk::Grid::new();
    mosaic.set_halign(gtk::Align::End);
    mosaic.set_valign(gtk::Align::Center);
    mosaic.set_row_spacing(6);
    mosaic.set_column_spacing(6);
    let mut seen: Vec<String> = Vec::new();
    let mut tiles = 0;
    for song in &mix {
        if tiles >= 4 {
            break;
        }
        if seen.iter().any(|id| id == &song.album_id) {
            continue;
        }
        if let Some(album) = data.album(&song.album_id) {
            seen.push(song.album_id.clone());
            let picture = cover_picture(actions, &album.cover_path, 76);
            picture.add_css_class("hero-cover");
            mosaic.attach(&picture, tiles % 2, tiles / 2, 1, 1);
            tiles += 1;
        }
    }
    if tiles > 0 {
        // El mosaico es un lujo de pantallas anchas: en ventanas estrechas
        // desaparece para no imponer un ancho mínimo a toda la página.
        mosaic.set_visible(false);
        let mosaic_weak = mosaic.downgrade();
        let window = actions.window.clone();
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
            let Some(mosaic) = mosaic_weak.upgrade() else {
                return gtk::glib::ControlFlow::Break;
            };
            mosaic.set_visible(window.width() >= 1180);
            gtk::glib::ControlFlow::Continue
        });
        feature.append(&mosaic);
    }
    content.append(&feature);

    let mut sections: Vec<_> = data
        .home()
        .iter()
        .filter(|section| section.title != "Escuchar ahora" && !section.items.is_empty())
        .cloned()
        .collect();
    for section in &mut sections {
        match section.title.as_str() {
            "Nuevos lanzamientos" => {
                section.title = "Nuevo para tus oídos".into();
                section.subtitle = "Lo último que ha llegado a tu biblioteca".into();
            }
            "Tus canciones favoritas" => {
                section.title = "Vuelve a escuchar".into();
                section.subtitle = "Un atajo a lo que ya te gusta".into();
            }
            "Playlists para ti" => {
                section.title = "Por estado de ánimo".into();
                section.subtitle = "Selecciones para cada momento del día".into();
            }
            _ => {}
        }
    }
    drop(data);
    content.append(&home_sections(actions, &sections));
    page_scroll(&content).upcast()
}

pub fn albums_page(actions: &Actions) -> gtk::Widget {
    let data = actions.data.borrow();
    let mut albums = data.snapshot.albums.clone();
    albums.sort_by(|a, b| {
        a.artist_name
            .to_lowercase()
            .cmp(&b.artist_name.to_lowercase())
            .then(a.year.cmp(&b.year))
            .then(a.title.to_lowercase().cmp(&b.title.to_lowercase()))
    });
    let header = page_header(
        "Álbumes",
        &format!(
            "{} en tu biblioteca, ordenados por artista",
            plural(albums.len(), "álbum", "álbumes")
        ),
    );
    drop(data);
    let content = content_box();
    content.set_spacing(18);
    content.append(&header);
    content.append(&album_grid(actions, &albums));
    content.upcast()
}

pub fn artists_page(actions: &Actions) -> gtk::Widget {
    let data = actions.data.borrow();
    let artists = data.snapshot.artists.clone();
    let header = page_header(
        "Artistas",
        &format!(
            "{} en tu biblioteca",
            plural(artists.len(), "artista", "artistas")
        ),
    );
    drop(data);
    let content = content_box();
    content.set_spacing(18);
    content.append(&header);
    content.append(&artist_grid(actions, &artists));
    content.upcast()
}

pub fn playlists_page(actions: &Actions) -> gtk::Widget {
    let data = actions.data.borrow();
    let playlists = data.snapshot.playlists.clone();
    let header = page_header(
        "Playlists",
        "Selecciones de la redacción, la comunidad y tus propios gustos.",
    );
    drop(data);
    let content = content_box();
    content.set_spacing(18);
    content.append(&header);
    content.append(&playlist_grid(actions, &playlists));
    content.upcast()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SongSort {
    Album,
    Artist,
    Title,
    Duration,
}

pub fn songs_page(actions: &Actions) -> gtk::Widget {
    let data = actions.data.borrow();
    let all: Rc<Vec<Song>> = Rc::new(data.snapshot.songs.clone());
    let total: u64 = all.iter().map(|s| s.duration_ms).sum();
    drop(data);

    let content = content_box();
    content.set_spacing(14);
    content.append(&page_header(
        "Canciones",
        &format!(
            "{} · {} de música",
            plural(all.len(), "canción", "canciones"),
            total_duration(total)
        ),
    ));

    let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    toolbar.add_css_class("songs-toolbar");
    let filter = gtk::SearchEntry::new();
    filter.set_placeholder_text(Some("Filtrar en Canciones"));
    filter.set_width_request(300);
    filter.set_hexpand(false);
    filter.add_css_class("search-entry");
    let sort = gtk::DropDown::from_strings(&["Álbum", "Artista", "Título", "Duración"]);
    sort.add_css_class("sort-dropdown");
    sort.set_tooltip_text(Some("Ordenar canciones"));
    sort.set_halign(gtk::Align::Start);
    let count = gtk::Label::new(None);
    count.set_xalign(1.0);
    count.set_halign(gtk::Align::End);
    count.set_hexpand(true);
    count.add_css_class("muted-small");
    toolbar.append(&filter);
    toolbar.append(&sort);
    toolbar.append(&count);
    content.append(&toolbar);

    let list_host = gtk::Box::new(gtk::Orientation::Vertical, 0);
    list_host.set_vexpand(true);
    content.append(&list_host);

    let sort_key = Rc::new(Cell::new(SongSort::Album));
    let query = Rc::new(RefCell::new(String::new()));
    let query_refresh = query.clone();
    let sort_refresh = sort_key.clone();
    let all_refresh = all.clone();
    let actions_refresh = actions.clone();
    let host_refresh = list_host.clone();
    let count_refresh = count.clone();
    let refresh: Rc<dyn Fn()> = Rc::new(move || {
        let needle = query_refresh.borrow().trim().to_lowercase();
        let mut songs: Vec<Song> = all_refresh
            .iter()
            .filter(|song| {
                needle.is_empty()
                    || song.title.to_lowercase().contains(&needle)
                    || song.artist_name.to_lowercase().contains(&needle)
                    || song.album_title.to_lowercase().contains(&needle)
            })
            .cloned()
            .collect();
        match sort_refresh.get() {
            SongSort::Album => songs.sort_by(|a, b| {
                a.album_title
                    .to_lowercase()
                    .cmp(&b.album_title.to_lowercase())
                    .then(a.disc_number.cmp(&b.disc_number))
                    .then(a.track_number.cmp(&b.track_number))
            }),
            SongSort::Artist => songs.sort_by(|a, b| {
                a.artist_name
                    .to_lowercase()
                    .cmp(&b.artist_name.to_lowercase())
                    .then(
                        a.album_title
                            .to_lowercase()
                            .cmp(&b.album_title.to_lowercase()),
                    )
                    .then(a.track_number.cmp(&b.track_number))
            }),
            SongSort::Title => {
                songs.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
            }
            SongSort::Duration => songs.sort_by_key(|song| song.duration_ms),
        }
        count_refresh.set_text(&if songs.len() == all_refresh.len() {
            format!("{} canciones", format_count(songs.len()))
        } else {
            format!(
                "{} de {}",
                format_count(songs.len()),
                format_count(all_refresh.len())
            )
        });
        while let Some(child) = host_refresh.first_child() {
            host_refresh.remove(&child);
        }
        if songs.is_empty() {
            let empty = gtk::Label::new(Some("No hay canciones que coincidan con este filtro."));
            empty.add_css_class("empty-state");
            empty.set_vexpand(true);
            empty.set_valign(gtk::Align::Center);
            host_refresh.append(&empty);
        } else {
            host_refresh.append(&song_list_header());
            host_refresh.append(&song_list(&actions_refresh, &songs, true, false));
        }
    });

    let refresh_filter = refresh.clone();
    let query_entry = query.clone();
    filter.connect_search_changed(move |entry| {
        *query_entry.borrow_mut() = entry.text().to_string();
        refresh_filter();
    });
    let refresh_sort = refresh.clone();
    sort.connect_selected_notify(move |dropdown| {
        sort_key.set(match dropdown.selected() {
            1 => SongSort::Artist,
            2 => SongSort::Title,
            3 => SongSort::Duration,
            _ => SongSort::Album,
        });
        refresh_sort();
    });
    refresh();
    content.upcast()
}

pub fn favorites_page(actions: &Actions) -> gtk::Widget {
    let data = actions.data.borrow();
    let songs = data.favorite_songs();
    let albums = data.favorite_albums();
    let artists = data.favorite_artists();
    let playlists = data.favorite_playlists();
    drop(data);

    let content = content_box();
    content.set_spacing(14);
    let header = page_header(
        "Favoritos",
        &format!(
            "{} · {} · {} · {}",
            plural(songs.len(), "canción", "canciones"),
            plural(albums.len(), "álbum", "álbumes"),
            plural(artists.len(), "artista", "artistas"),
            plural(playlists.len(), "playlist", "playlists")
        ),
    );
    content.append(&header);

    let stack = gtk::Stack::new();
    stack.set_vexpand(true);
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(false);
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);

    let songs_tab = gtk::Box::new(gtk::Orientation::Vertical, 0);
    if songs.is_empty() {
        songs_tab.append(&empty_state(
            actions,
            "folder-music-symbolic",
            "Aún no hay canciones favoritas",
            "Marca canciones con el corazón y aparecerán aquí, listas para sonar.",
            Some(("Ver sugerencias", "listen")),
        ));
    } else {
        songs_tab.append(&song_list(actions, &songs, true, false));
    }
    stack.add_titled(&songs_tab, Some("songs"), "Canciones");

    let albums_tab = gtk::Box::new(gtk::Orientation::Vertical, 0);
    if albums.is_empty() {
        albums_tab.append(&empty_state(
            actions,
            "media-optical-symbolic",
            "Aún no hay álbumes guardados",
            "Guarda un álbum para volver a él sin buscarlo.",
            Some(("Explorar álbumes", "albums")),
        ));
    } else {
        albums_tab.append(&album_grid(actions, &albums));
    }
    stack.add_titled(&albums_tab, Some("albums"), "Álbumes");

    let artists_tab = gtk::Box::new(gtk::Orientation::Vertical, 0);
    if artists.is_empty() {
        artists_tab.append(&empty_state(
            actions,
            "avatar-default-symbolic",
            "Aún no sigues a ningún artista",
            "Sigue a un artista para tener sus novedades a mano.",
            Some(("Descubrir artistas", "artists")),
        ));
    } else {
        artists_tab.append(&artist_grid(actions, &artists));
    }
    stack.add_titled(&artists_tab, Some("artists"), "Artistas");

    let playlists_tab = gtk::Box::new(gtk::Orientation::Vertical, 0);
    if playlists.is_empty() {
        playlists_tab.append(&empty_state(
            actions,
            "view-list-symbolic",
            "Aún no hay playlists guardadas",
            "Guarda una playlist para escucharla cuando quieras.",
            Some(("Ver playlists", "playlists")),
        ));
    } else {
        playlists_tab.append(&playlist_grid(actions, &playlists));
    }
    stack.add_titled(&playlists_tab, Some("playlists"), "Playlists");

    let switcher = gtk::StackSwitcher::new();
    switcher.set_stack(Some(&stack));
    switcher.set_halign(gtk::Align::Start);
    content.append(&switcher);
    content.append(&stack);
    content.upcast()
}

fn empty_state(
    actions: &Actions,
    icon_name: &str,
    title: &str,
    body: &str,
    cta: Option<(&str, &str)>,
) -> gtk::Widget {
    let box_ = gtk::Box::new(gtk::Orientation::Vertical, 10);
    box_.set_vexpand(true);
    box_.set_valign(gtk::Align::Center);
    box_.set_halign(gtk::Align::Center);
    let icon = gtk::Image::from_icon_name(icon_name);
    icon.set_pixel_size(56);
    icon.add_css_class("empty-icon");
    let title = gtk::Label::new(Some(title));
    title.add_css_class("empty-title");
    title.set_justify(gtk::Justification::Center);
    let label = gtk::Label::new(Some(body));
    label.add_css_class("empty-state");
    label.set_wrap(true);
    label.set_justify(gtk::Justification::Center);
    label.set_max_width_chars(46);
    box_.append(&icon);
    box_.append(&title);
    box_.append(&label);
    if let Some((cta_label, target)) = cta {
        let button = gtk::Button::with_label(cta_label);
        button.add_css_class("pill-button");
        button.add_css_class("empty-cta");
        button.set_halign(gtk::Align::Center);
        button.set_margin_top(6);
        let actions = actions.clone();
        let target = target.to_string();
        button.connect_clicked(move |_| actions.select(&target));
        box_.append(&button);
    }
    box_.upcast()
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
