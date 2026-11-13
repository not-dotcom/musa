//! Piezas visuales reutilizables. Todas las listas usan modelos + factories,
//! de modo que GTK solo crea widgets para las filas visibles.

use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

static ALBUM_CARDS: AtomicUsize = AtomicUsize::new(0);

// Corazones propios: el tema del sistema puede resolver
// `emblem-favorite-symbolic` como un emoji a color o como un PNG legacy. Estos
// dos recursos garantizan el mismo trazo monocromo en cualquier escritorio.
const HEART_FILLED: &[u8] = include_bytes!("../../assets/heart-filled.png");
const HEART_OUTLINE: &[u8] = include_bytes!("../../assets/heart-outline.png");

pub fn album_cards_built() -> usize {
    ALBUM_CARDS.load(Ordering::Relaxed)
}

use musa_protocol::{Album, Artist, HomeItem, HomeSection, Playlist, Song};
use relm4::adw;
use relm4::gtk;
use relm4::gtk::glib;
use relm4::gtk::prelude::*;

use super::Actions;
use crate::data::{duration, plural};

fn bounded_scroll(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_vexpand(true);
    scroll.set_hexpand(true);
    scroll.add_css_class("scroll-bound");
    scroll.set_overlay_scrolling(false);
    scroll.set_child(Some(child));
    scroll
}

pub fn page_scroll(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_vexpand(true);
    scroll.set_hexpand(true);
    scroll.add_css_class("scroll-bound");
    scroll.set_overlay_scrolling(false);
    scroll.set_child(Some(child));
    scroll
}

pub fn content_box() -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 26);
    content.set_margin_top(24);
    content.set_margin_bottom(34);
    content.set_margin_start(30);
    content.set_margin_end(30);
    content
}

pub fn page_header(title: &str, subtitle: &str) -> gtk::Box {
    let header = gtk::Box::new(gtk::Orientation::Vertical, 5);
    let h1 = gtk::Label::new(Some(title));
    h1.set_xalign(0.0);
    h1.add_css_class("h1");
    let sub = gtk::Label::new(Some(subtitle));
    sub.set_xalign(0.0);
    sub.add_css_class("muted");
    sub.set_wrap(true);
    header.append(&h1);
    header.append(&sub);
    header
}

pub fn section_heading(actions: &Actions, section: &HomeSection) -> gtk::Widget {
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let titles = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let title = gtk::Label::new(Some(&section.title));
    title.set_xalign(0.0);
    title.add_css_class("h2");
    let subtitle = gtk::Label::new(Some(&section.subtitle));
    subtitle.set_xalign(0.0);
    subtitle.add_css_class("muted-small");
    titles.append(&title);
    titles.append(&subtitle);
    header.append(&titles);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    header.append(&spacer);
    let play = gtk::Button::from_icon_name("media-playback-start-symbolic");
    play.add_css_class("ghost-button");
    play.set_tooltip_text(Some("Reproducir esta sección"));
    let songs: Vec<Song> = section
        .items
        .iter()
        .filter_map(|item| match item {
            HomeItem::Song(song) => actions.data.borrow().song(&song.id).cloned(),
            _ => None,
        })
        .collect();
    if !songs.is_empty() {
        let actions = actions.clone();
        play.connect_clicked(move |_| actions.play_songs(&songs, 0));
        header.append(&play);
    }
    header.upcast()
}

#[allow(deprecated)]
pub fn cover_picture(actions: &Actions, path: &str, logical: i32) -> gtk::Picture {
    cover_picture_at(actions, path, logical, logical)
}

/// Portada con tamaño de pinta y tamaño de decodificación separados. Las
/// tarjetas adaptativas crecen con la columna, así que piden píxeles para su
/// tamaño máximo y no se ven borrosas al ensancharse.
#[allow(deprecated)]
pub fn cover_picture_at(actions: &Actions, path: &str, logical: i32, decode: i32) -> gtk::Picture {
    let picture = gtk::Picture::new();
    picture.set_size_request(logical, logical);
    picture.set_content_fit(gtk::ContentFit::Cover);
    picture.set_can_shrink(false);
    picture.set_hexpand(false);
    picture.set_vexpand(false);
    picture.set_valign(gtk::Align::Start);
    picture.add_css_class("cover");
    picture.add_css_class("cover-placeholder");
    request_cover(&picture, actions, path, decode);
    let picture_weak = picture.downgrade();
    let covers = actions.covers.clone();
    let path = path.to_string();
    picture.connect_notify_local(Some("scale-factor"), move |_, _| {
        if let Some(picture) = picture_weak.upgrade() {
            request_cover(&picture, &covers, &path, decode);
        }
    });
    picture
}

#[allow(deprecated)]
fn request_cover(picture: &gtk::Picture, source: &impl CoverSource, path: &str, logical: i32) {
    if path.is_empty() {
        return;
    }
    let covers = source.covers().clone();
    let scale = picture.scale_factor().max(1) as u32;
    let px = (logical.max(1) as u32) * scale;
    let path = path.to_string();
    // Pequeño retardo de visibilidad: durante un scroll rápido, las tarjetas
    // que entran y salen del viewport no llegan a decodificar su portada.
    let weak = picture.downgrade();
    gtk::glib::timeout_add_local_once(Duration::from_millis(90), move || {
        let Some(picture) = weak.upgrade() else {
            return;
        };
        let target = picture.clone();
        covers.request(std::path::Path::new(&path), px, move |pixbuf| {
            if let Some(pixbuf) = pixbuf {
                let texture = gtk::gdk::Texture::for_pixbuf(&pixbuf);
                target.set_paintable(Some(&texture));
                target.remove_css_class("cover-placeholder");
            }
        });
    });
}

pub trait CoverSource {
    fn covers(&self) -> &crate::covers::CoverLoader;
}

impl CoverSource for Actions {
    fn covers(&self) -> &crate::covers::CoverLoader {
        &self.covers
    }
}

impl CoverSource for crate::covers::CoverLoader {
    fn covers(&self) -> &crate::covers::CoverLoader {
        self
    }
}

pub fn album_card(actions: &Actions, album: &Album) -> gtk::Box {
    ALBUM_CARDS.fetch_add(1, Ordering::Relaxed);
    let card = gtk::Box::new(gtk::Orientation::Vertical, 7);
    card.set_width_request(160);
    card.set_margin_start(4);
    card.set_margin_end(4);
    card.set_margin_top(4);
    card.set_margin_bottom(4);
    card.add_css_class("media-card");
    card.set_tooltip_text(Some(&format!("{} — {}", album.title, album.artist_name)));
    let cover = cover_picture(actions, &album.cover_path, 148);
    cover.set_halign(gtk::Align::Start);
    card.append(&cover);
    let title = gtk::Label::new(Some(&album.title));
    title.set_xalign(0.0);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.set_max_width_chars(22);
    title.add_css_class("card-title");
    let artist = gtk::Label::new(Some(&album.artist_name));
    artist.set_xalign(0.0);
    artist.set_ellipsize(gtk::pango::EllipsizeMode::End);
    artist.set_max_width_chars(22);
    artist.add_css_class("card-subtitle");
    card.append(&title);
    card.append(&artist);
    card
}

pub fn album_card_button(actions: &Actions, album: &Album) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("card-button");
    button.set_child(Some(&album_card(actions, album)));
    let actions = actions.clone();
    let id = album.id.clone();
    button.connect_clicked(move |_| actions.open_album(&id));
    button
}

pub fn playlist_card(actions: &Actions, playlist: &Playlist) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 7);
    card.set_width_request(198);
    card.set_margin_start(4);
    card.set_margin_end(4);
    card.set_margin_top(4);
    card.set_margin_bottom(4);
    card.add_css_class("media-card");
    card.set_tooltip_text(Some(&format!(
        "{} — {}",
        playlist.name, playlist.description
    )));
    let cover = cover_picture(actions, &playlist.cover_path, 186);
    cover.set_halign(gtk::Align::Start);
    card.append(&cover);
    let title = gtk::Label::new(Some(&playlist.name));
    title.set_xalign(0.0);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.add_css_class("card-title");
    let description = gtk::Label::new(Some(&playlist.description));
    description.set_xalign(0.0);
    description.set_wrap(true);
    description.set_lines(2);
    description.set_ellipsize(gtk::pango::EllipsizeMode::End);
    description.add_css_class("card-subtitle");
    card.append(&title);
    card.append(&description);
    card
}

pub fn playlist_card_button(actions: &Actions, playlist: &Playlist) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("card-button");
    button.set_child(Some(&playlist_card(actions, playlist)));
    let actions = actions.clone();
    let id = playlist.id.clone();
    button.connect_clicked(move |_| actions.open_playlist(&id));
    button
}

#[allow(deprecated)]
pub fn artist_avatar(actions: &Actions, artist: &Artist, size: i32) -> adw::Avatar {
    artist_avatar_at(actions, artist, size, size)
}

#[allow(deprecated)]
pub fn artist_avatar_at(actions: &Actions, artist: &Artist, size: i32, decode: i32) -> adw::Avatar {
    let avatar = adw::Avatar::new(size, Some(&artist.name), true);
    avatar.add_css_class("artist-avatar");
    let covers = actions.covers.clone();
    let path = artist.photo_path.clone();
    load_avatar(&avatar, &covers, &path, decode);
    let avatar_weak = avatar.downgrade();
    avatar.connect_notify_local(Some("scale-factor"), move |_, _| {
        if let Some(avatar) = avatar_weak.upgrade() {
            load_avatar(&avatar, &covers, &path, decode);
        }
    });
    avatar
}

#[allow(deprecated)]
fn load_avatar(avatar: &adw::Avatar, covers: &crate::covers::CoverLoader, path: &str, size: i32) {
    let scale = avatar.scale_factor().max(1) as u32;
    let target = avatar.clone();
    covers.request(
        std::path::Path::new(path),
        (size as u32) * scale,
        move |pixbuf| {
            if let Some(pixbuf) = pixbuf {
                let texture = gtk::gdk::Texture::for_pixbuf(&pixbuf);
                target.set_custom_image(Some(&texture));
            }
        },
    );
}

pub fn artist_card(actions: &Actions, artist: &Artist) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
    card.set_width_request(150);
    card.set_margin_start(4);
    card.set_margin_end(4);
    card.set_margin_top(4);
    card.set_margin_bottom(4);
    card.add_css_class("media-card");
    card.set_tooltip_text(Some(&format!("{} — {}", artist.name, artist.genre)));
    let avatar = artist_avatar(actions, artist, 112);
    avatar.set_halign(gtk::Align::Center);
    card.append(&avatar);
    let name = gtk::Label::new(Some(&artist.name));
    name.set_xalign(0.5);
    name.set_halign(gtk::Align::Center);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    name.add_css_class("card-title");
    let genre = gtk::Label::new(Some(&artist.genre));
    genre.set_xalign(0.5);
    genre.set_halign(gtk::Align::Center);
    genre.add_css_class("card-subtitle");
    card.append(&name);
    card.append(&genre);
    card
}

pub fn artist_card_button(actions: &Actions, artist: &Artist) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("card-button");
    button.set_child(Some(&artist_card(actions, artist)));
    let actions = actions.clone();
    let id = artist.id.clone();
    button.connect_clicked(move |_| actions.open_artist(&id));
    button
}

pub fn song_tile(actions: &Actions, song: &Song) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("card-button");
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.set_width_request(278);
    row.set_margin_top(7);
    row.set_margin_bottom(7);
    row.set_margin_start(7);
    row.set_margin_end(7);
    let cover = cover_picture(actions, &song.cover_path, 52);
    row.append(&cover);
    let labels = gtk::Box::new(gtk::Orientation::Vertical, 3);
    labels.set_valign(gtk::Align::Center);
    let title = gtk::Label::new(Some(&song.title));
    title.set_xalign(0.0);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.set_max_width_chars(22);
    title.add_css_class("card-title");
    let artist = gtk::Label::new(Some(&format!(
        "{} · {}",
        song.artist_name,
        duration(song.duration_ms)
    )));
    artist.set_xalign(0.0);
    artist.set_ellipsize(gtk::pango::EllipsizeMode::End);
    artist.add_css_class("card-subtitle");
    labels.append(&title);
    labels.append(&artist);
    row.append(&labels);
    button.set_child(Some(&row));
    let actions = actions.clone();
    let song = song.clone();
    button.connect_clicked(move |_| actions.play_song(&song));
    button
}

pub fn home_sections(actions: &Actions, sections: &[HomeSection]) -> gtk::Box {
    let box_ = gtk::Box::new(gtk::Orientation::Vertical, 28);
    for section in sections {
        let block = gtk::Box::new(gtk::Orientation::Vertical, 10);
        block.append(&section_heading(actions, section));
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        row.set_valign(gtk::Align::Start);
        for item in &section.items {
            match item {
                HomeItem::Album(album) => {
                    if let Some(full) = actions.data.borrow().album(&album.id).cloned() {
                        row.append(&album_card_button(actions, &full));
                    }
                }
                HomeItem::Playlist(playlist) => {
                    if let Some(full) = actions.data.borrow().playlist(&playlist.id).cloned() {
                        row.append(&playlist_card_button(actions, &full));
                    }
                }
                HomeItem::Song(song) => {
                    if let Some(full) = actions.data.borrow().song(&song.id).cloned() {
                        row.append(&song_tile(actions, &full));
                    }
                }
                HomeItem::Artist(artist) => {
                    if let Some(full) = actions.data.borrow().artist(&artist.id).cloned() {
                        row.append(&artist_card_button(actions, &full));
                    }
                }
            }
        }
        let scroller = gtk::ScrolledWindow::new();
        // External: el carrusel se desplaza, pero no dibuja barra horizontal
        // (el tema la expandía sobre las portadas y los metadatos).
        scroller.set_policy(gtk::PolicyType::External, gtk::PolicyType::Never);
        scroller.set_propagate_natural_height(true);
        scroller.set_child(Some(&row));
        scroller.add_css_class("carousel");
        block.append(&scroller);
        box_.append(&block);
    }
    box_
}

#[derive(Clone, Copy, Debug)]
enum GridKind {
    Album,
    Artist,
    Playlist,
}

fn grid_columns(kind: GridKind, width: i32) -> u32 {
    let (min_card, gap, max_cols) = match kind {
        GridKind::Album => (156, 20, 8),
        GridKind::Artist => (128, 22, 8),
        GridKind::Playlist => (194, 22, 6),
    };
    let available = (width - 12).max(0);
    (((available + gap) / (min_card + gap)) as u32).clamp(2, max_cols)
}

fn apply_grid_columns(
    actions: &Actions,
    view: &gtk::GridView,
    kind: GridKind,
    fallback_width: i32,
) {
    let weak = view.downgrade();
    let sync: Rc<dyn Fn(i32)> = Rc::new(move |content_width: i32| {
        let Some(view) = weak.upgrade() else {
            return;
        };
        let width = if content_width > 0 {
            (content_width - 60).max(320)
        } else {
            fallback_width
        };
        let cols = grid_columns(kind, width);
        view.set_min_columns(cols);
        view.set_max_columns(cols);
    });
    sync(fallback_width + 60);
    actions.grid_syncers.borrow_mut().push(sync);
}

pub fn album_grid(actions: &Actions, albums: &[Album]) -> gtk::Widget {
    let store = gtk::gio::ListStore::new::<glib::BoxedAnyObject>();
    for album in albums {
        store.append(&glib::BoxedAnyObject::new(album.clone()));
    }
    let selection = gtk::SingleSelection::new(Some(store.clone()));
    selection.set_autoselect(false);
    selection.set_can_unselect(true);
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, object| {
        let item = object.downcast_ref::<gtk::ListItem>().unwrap();
        let container = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        container.set_halign(gtk::Align::Center);
        item.set_child(Some(&container));
    });
    let actions_bind = actions.clone();
    factory.connect_bind(move |_, object| {
        let item = object.downcast_ref::<gtk::ListItem>().unwrap();
        let Some(container) = item.child().and_downcast::<gtk::Box>() else {
            return;
        };
        while let Some(child) = container.first_child() {
            container.remove(&child);
        }
        let Some(boxed) = item.item().and_downcast::<glib::BoxedAnyObject>() else {
            return;
        };
        if let Ok(album) = boxed.try_borrow::<Album>() {
            container.append(&album_card(&actions_bind, &album));
        }
    });
    let view = gtk::GridView::new(Some(selection.clone()), Some(factory.clone()));
    view.add_css_class("media-grid");
    view.set_min_columns(2);
    view.set_max_columns(8);
    selection.set_selected(gtk::INVALID_LIST_POSITION);
    apply_grid_columns(
        actions,
        &view,
        GridKind::Album,
        (actions.window.width() - 300).max(600),
    );
    view.set_single_click_activate(true);
    view.set_vexpand(true);
    view.set_hexpand(true);
    view.set_margin_start(0);
    view.set_margin_end(0);
    view.set_margin_bottom(0);
    if let Some(adjustment) = view.vadjustment() {
        *actions.albums_adjustment.borrow_mut() = Some(adjustment);
    }
    let actions_activate = actions.clone();
    let store_activate = store.clone();
    view.connect_activate(move |_, position| {
        if let Some(boxed) = store_activate
            .item(position)
            .and_downcast::<glib::BoxedAnyObject>()
        {
            if let Ok(album) = boxed.try_borrow::<Album>() {
                actions_activate.open_album(&album.id);
            }
        }
    });
    bounded_scroll(&view).upcast()
}

pub fn artist_grid(actions: &Actions, artists: &[Artist]) -> gtk::Widget {
    let store = gtk::gio::ListStore::new::<glib::BoxedAnyObject>();
    for artist in artists {
        store.append(&glib::BoxedAnyObject::new(artist.clone()));
    }
    let selection = gtk::SingleSelection::new(Some(store.clone()));
    selection.set_autoselect(false);
    selection.set_can_unselect(true);
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, object| {
        let item = object.downcast_ref::<gtk::ListItem>().unwrap();
        let container = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        container.set_halign(gtk::Align::Center);
        item.set_child(Some(&container));
    });
    let actions_bind = actions.clone();
    factory.connect_bind(move |_, object| {
        let item = object.downcast_ref::<gtk::ListItem>().unwrap();
        let Some(container) = item.child().and_downcast::<gtk::Box>() else {
            return;
        };
        while let Some(child) = container.first_child() {
            container.remove(&child);
        }
        let Some(boxed) = item.item().and_downcast::<glib::BoxedAnyObject>() else {
            return;
        };
        if let Ok(artist) = boxed.try_borrow::<Artist>() {
            container.append(&artist_card(&actions_bind, &artist));
        }
    });
    let view = gtk::GridView::new(Some(selection.clone()), Some(factory.clone()));
    view.add_css_class("media-grid");
    view.set_min_columns(2);
    view.set_max_columns(8);
    selection.set_selected(gtk::INVALID_LIST_POSITION);
    apply_grid_columns(
        actions,
        &view,
        GridKind::Artist,
        (actions.window.width() - 300).max(600),
    );
    view.set_single_click_activate(true);
    view.set_vexpand(true);
    view.set_hexpand(true);
    view.set_margin_start(0);
    view.set_margin_end(0);
    view.set_margin_bottom(0);
    let actions_activate = actions.clone();
    let store_activate = store.clone();
    view.connect_activate(move |_, position| {
        if let Some(boxed) = store_activate
            .item(position)
            .and_downcast::<glib::BoxedAnyObject>()
        {
            if let Ok(artist) = boxed.try_borrow::<Artist>() {
                actions_activate.open_artist(&artist.id);
            }
        }
    });
    bounded_scroll(&view).upcast()
}

pub fn playlist_grid(actions: &Actions, playlists: &[Playlist]) -> gtk::Widget {
    let store = gtk::gio::ListStore::new::<glib::BoxedAnyObject>();
    for playlist in playlists {
        store.append(&glib::BoxedAnyObject::new(playlist.clone()));
    }
    let selection = gtk::SingleSelection::new(Some(store.clone()));
    selection.set_autoselect(false);
    selection.set_can_unselect(true);
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, object| {
        let item = object.downcast_ref::<gtk::ListItem>().unwrap();
        let container = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        container.set_halign(gtk::Align::Center);
        item.set_child(Some(&container));
    });
    let actions_bind = actions.clone();
    factory.connect_bind(move |_, object| {
        let item = object.downcast_ref::<gtk::ListItem>().unwrap();
        let Some(container) = item.child().and_downcast::<gtk::Box>() else {
            return;
        };
        while let Some(child) = container.first_child() {
            container.remove(&child);
        }
        let Some(boxed) = item.item().and_downcast::<glib::BoxedAnyObject>() else {
            return;
        };
        if let Ok(playlist) = boxed.try_borrow::<Playlist>() {
            container.append(&playlist_card(&actions_bind, &playlist));
        }
    });
    let view = gtk::GridView::new(Some(selection.clone()), Some(factory.clone()));
    view.add_css_class("media-grid");
    view.set_min_columns(2);
    view.set_max_columns(6);
    selection.set_selected(gtk::INVALID_LIST_POSITION);
    apply_grid_columns(
        actions,
        &view,
        GridKind::Playlist,
        (actions.window.width() - 300).max(600),
    );
    view.set_single_click_activate(true);
    view.set_vexpand(true);
    view.set_hexpand(true);
    view.set_margin_start(0);
    view.set_margin_end(0);
    view.set_margin_bottom(0);
    let actions_activate = actions.clone();
    let store_activate = store.clone();
    view.connect_activate(move |_, position| {
        if let Some(boxed) = store_activate
            .item(position)
            .and_downcast::<glib::BoxedAnyObject>()
        {
            if let Ok(playlist) = boxed.try_borrow::<Playlist>() {
                actions_activate.open_playlist(&playlist.id);
            }
        }
    });
    bounded_scroll(&view).upcast()
}

pub fn song_list(
    actions: &Actions,
    songs: &[Song],
    show_album: bool,
    show_meta_album: bool,
) -> gtk::Widget {
    let songs: Rc<Vec<Song>> = Rc::new(songs.to_vec());
    let store = gtk::gio::ListStore::new::<glib::BoxedAnyObject>();
    for (index, song) in songs.iter().enumerate() {
        store.append(&glib::BoxedAnyObject::new((index, song.clone())));
    }
    let selection = gtk::SingleSelection::new(Some(store.clone()));
    selection.set_autoselect(false);
    selection.set_can_unselect(true);
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, object| {
        let item = object.downcast_ref::<gtk::ListItem>().unwrap();
        let container = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        item.set_child(Some(&container));
    });
    let actions_bind = actions.clone();
    factory.connect_bind(move |_, object| {
        let item = object.downcast_ref::<gtk::ListItem>().unwrap();
        let Some(container) = item.child().and_downcast::<gtk::Box>() else {
            return;
        };
        while let Some(child) = container.first_child() {
            container.remove(&child);
        }
        let Some(boxed) = item.item().and_downcast::<glib::BoxedAnyObject>() else {
            return;
        };
        if let Ok(entry) = boxed.try_borrow::<(usize, Song)>() {
            let (index, song) = &*entry;
            container.append(&song_row(
                &actions_bind,
                *index,
                song,
                show_album,
                show_meta_album,
                true,
            ));
        }
    });
    let view = gtk::ListView::new(Some(selection.clone()), Some(factory.clone()));
    selection.set_selected(gtk::INVALID_LIST_POSITION);
    view.add_css_class("song-list");
    view.set_single_click_activate(true);
    view.set_vexpand(true);
    view.set_margin_start(16);
    view.set_margin_end(16);
    view.set_margin_bottom(20);
    let actions_activate = actions.clone();
    let songs_activate = songs.clone();
    view.connect_activate(move |_, position| {
        let position = position as usize;
        if let Some(song) = songs_activate.get(position) {
            if song.playable {
                actions_activate.play_songs(&songs_activate, position);
            } else {
                actions_activate.toast("Esta canción no está disponible en tu región.");
            }
        }
    });
    selection.set_selected(gtk::INVALID_LIST_POSITION);
    bounded_scroll(&view).upcast()
}

pub fn song_row(
    actions: &Actions,
    index: usize,
    song: &Song,
    show_album: bool,
    show_meta_album: bool,
    show_favorite: bool,
) -> gtk::Widget {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.add_css_class("song-row");
    row.set_margin_top(2);
    row.set_margin_bottom(2);
    row.set_margin_start(8);
    row.set_margin_end(8);
    if !song.playable {
        row.add_css_class("unplayable");
    }

    let number = gtk::Label::new(Some(&format!("{}", index + 1)));
    number.set_width_chars(3);
    number.set_xalign(1.0);
    number.add_css_class("track-number");
    row.append(&number);

    let labels = gtk::Box::new(gtk::Orientation::Vertical, 2);
    labels.set_valign(gtk::Align::Center);
    if show_album {
        // Columna de título estable: la cabecera y las filas comparten el
        // mismo ancho, y el álbum queda en una posición predecible.
        labels.set_width_request(240);
    } else {
        labels.set_hexpand(true);
    }
    let title = gtk::Label::new(Some(&song.title));
    title.set_xalign(0.0);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.add_css_class("song-title");
    let meta_text = if show_meta_album {
        format!("{} · {}", song.artist_name, song.album_title)
    } else {
        song.artist_name.clone()
    };
    let meta = gtk::Label::new(Some(&meta_text));
    meta.set_xalign(0.0);
    meta.set_ellipsize(gtk::pango::EllipsizeMode::End);
    meta.add_css_class("song-meta");
    labels.append(&title);
    labels.append(&meta);
    row.append(&labels);

    if show_album {
        let album = gtk::Label::new(Some(&song.album_title));
        album.set_xalign(0.0);
        album.set_ellipsize(gtk::pango::EllipsizeMode::End);
        album.set_width_chars(22);
        album.add_css_class("song-album");
        row.append(&album);
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        row.append(&spacer);
    }

    // Ranura fija para el badge E: sin ella, el corazón y la duración se
    // desplazan en las filas que no son explícitas.
    let explicit_slot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    explicit_slot.set_width_request(18);
    explicit_slot.set_valign(gtk::Align::Center);
    if song.explicit {
        let explicit = gtk::Label::new(Some("E"));
        explicit.add_css_class("explicit");
        explicit.set_valign(gtk::Align::Center);
        explicit.set_tooltip_text(Some("Contenido explícito"));
        explicit.set_halign(gtk::Align::Center);
        explicit_slot.append(&explicit);
    }
    row.append(&explicit_slot);

    if show_favorite {
        let favorite = heart_button(song.favorite);
        favorite.set_tooltip_text(Some(if song.favorite {
            "Quitar de favoritos"
        } else {
            "Añadir a favoritos"
        }));
        let actions_fav = actions.clone();
        let song_fav = song.clone();
        favorite.connect_clicked(move |button| {
            let now = !song_fav.favorite;
            actions_fav.toggle_favorite("song", &song_fav.id, now);
            set_heart(button, now);
            button.set_tooltip_text(Some(if now {
                "Quitar de favoritos"
            } else {
                "Añadir a favoritos"
            }));
        });
        row.append(&favorite);
    }

    let length = gtk::Label::new(Some(&duration(song.duration_ms)));
    length.set_width_chars(5);
    length.set_xalign(1.0);
    length.add_css_class("song-duration");
    row.append(&length);
    actions.register_song_row(&row, &song.id);

    row.upcast()
}

/// Cabecera de columnas de la lista de canciones. Repite la geometría de
/// `song_row` para que los rótulos queden sobre su columna.

/// Filas de detalle sin lista virtual: el detalle entero comparte un único
/// scroll, de modo que portada y cabecera se desplazan con las canciones.
pub fn song_rows(
    actions: &Actions,
    songs: &[Song],
    show_album: bool,
    show_meta_album: bool,
    show_favorite: bool,
) -> gtk::Widget {
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("song-list");
    list.add_css_class("song-list-rows");
    list.set_margin_start(18);
    list.set_margin_end(18);
    list.set_margin_bottom(24);
    for (index, song) in songs.iter().enumerate() {
        let row = gtk::ListBoxRow::new();
        row.set_activatable(true);
        row.set_child(Some(&song_row(
            actions,
            index,
            song,
            show_album,
            show_meta_album,
            show_favorite,
        )));
        list.append(&row);
    }
    let songs = Rc::new(songs.to_vec());
    let actions_activate = actions.clone();
    list.connect_row_activated(move |_, row| {
        let index = row.index() as usize;
        if let Some(song) = songs.get(index) {
            if song.playable {
                actions_activate.play_songs(songs.as_slice(), index);
            } else {
                actions_activate.toast("Esta canción no está disponible en tu región.");
            }
        }
    });
    list.upcast()
}

pub fn song_list_header() -> gtk::Widget {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.add_css_class("song-list-header");
    row.set_margin_start(24);
    row.set_margin_end(42);
    row.set_margin_top(6);
    row.set_margin_bottom(4);

    let number = gtk::Label::new(Some("#"));
    number.set_width_chars(3);
    number.set_xalign(1.0);
    number.add_css_class("list-header");
    row.append(&number);

    let title = gtk::Label::new(Some("TÍTULO"));
    title.set_xalign(0.0);
    title.set_width_request(240);
    title.add_css_class("list-header");
    row.append(&title);

    let album = gtk::Label::new(Some("ÁLBUM"));
    album.set_width_chars(22);
    album.set_xalign(0.0);
    album.add_css_class("list-header");
    row.append(&album);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    row.append(&spacer);

    let explicit = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    explicit.set_width_request(18);
    row.append(&explicit);

    let favorite = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    favorite.set_width_request(34);
    row.append(&favorite);

    let length = gtk::Label::new(Some("DURACIÓN"));
    length.set_width_chars(5);
    length.set_xalign(1.0);
    length.add_css_class("list-header");
    row.append(&length);

    row.upcast()
}

pub fn apply_favorite_class(button: &gtk::Button, favorite: bool) {
    if favorite {
        button.remove_css_class("not-favorite");
    } else {
        button.add_css_class("not-favorite");
    }
}

pub fn heart_image(favorite: bool, size: i32) -> gtk::Image {
    let bytes = glib::Bytes::from_static(if favorite {
        HEART_FILLED
    } else {
        HEART_OUTLINE
    });
    let image = gtk::Image::new();
    if let Ok(texture) = gtk::gdk::Texture::from_bytes(&bytes) {
        image.set_paintable(Some(&texture));
    }
    image.set_pixel_size(size);
    image
}

pub fn heart_button(favorite: bool) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("ghost-button");
    button.add_css_class("favorite-button");
    button.set_child(Some(&heart_image(favorite, 16)));
    apply_favorite_class(&button, favorite);
    button
}

pub fn set_heart(button: &gtk::Button, favorite: bool) {
    button.set_child(Some(&heart_image(favorite, 16)));
    apply_favorite_class(button, favorite);
}

pub fn set_heart_toggle(button: &gtk::ToggleButton, favorite: bool) {
    button.set_child(Some(&heart_image(favorite, 16)));
}

/// Botón principal con icono y etiqueta. El glifo ayuda a reconocer la acción
/// antes de leerla, sobre todo en las cabeceras de detalle.
pub fn icon_pill_button(label: &str, icon: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("pill-button");
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    let image = gtk::Image::from_icon_name(icon);
    image.set_pixel_size(16);
    let text = gtk::Label::new(Some(label));
    content.append(&image);
    content.append(&text);
    button.set_child(Some(&content));
    button
}

pub fn collection_summary(count: usize, total_ms: u64) -> String {
    format!(
        "{} · {}",
        plural(count, "canción", "canciones"),
        crate::data::total_duration(total_ms)
    )
}

/// Fila de resultado de búsqueda sin controles anidados: se puede pulsar
/// entera y reproduce la canción.
pub fn song_result_button(actions: &Actions, song: &Song) -> gtk::Button {
    let button = song_result_view(actions, song);
    let actions = actions.clone();
    let song = song.clone();
    button.connect_clicked(move |_| actions.play_song(&song));
    button
}

/// Resultado de una canción del catálogo que puede no estar en la biblioteca.
/// Al pulsarla se encola la lista completa de resultados, de modo que
/// siguiente y anterior funcionan también fuera de la biblioteca.
pub fn catalog_song_result_button(
    actions: &Actions,
    songs: &Rc<Vec<Song>>,
    index: usize,
) -> gtk::Button {
    let button = song_result_view(actions, &songs[index]);
    let actions = actions.clone();
    let songs = songs.clone();
    button.connect_clicked(move |_| actions.play_songs(&songs, index));
    button
}

fn song_result_view(actions: &Actions, song: &Song) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("song-result");
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.set_margin_top(6);
    row.set_margin_bottom(6);
    row.set_margin_start(8);
    row.set_margin_end(8);
    let cover = cover_picture(actions, &song.cover_path, 42);
    row.append(&cover);
    let labels = gtk::Box::new(gtk::Orientation::Vertical, 2);
    labels.set_hexpand(true);
    labels.set_valign(gtk::Align::Center);
    let title = gtk::Label::new(Some(&song.title));
    title.set_xalign(0.0);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.add_css_class("song-title");
    let meta = gtk::Label::new(Some(&format!(
        "{} · {}",
        song.artist_name, song.album_title
    )));
    meta.set_xalign(0.0);
    meta.set_ellipsize(gtk::pango::EllipsizeMode::End);
    meta.add_css_class("song-meta");
    labels.append(&title);
    labels.append(&meta);
    row.append(&labels);
    let time = gtk::Label::new(Some(&duration(song.duration_ms)));
    time.add_css_class("song-duration");
    row.append(&time);
    button.set_child(Some(&row));
    button
}
