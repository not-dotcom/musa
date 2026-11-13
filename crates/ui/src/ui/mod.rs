//! Raíz de la interfaz: ventana, barra lateral, páginas y bucle de eventos.

mod detail;
mod measure;
mod pages;
mod player;
mod search;
mod widgets;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

use musa_protocol::{Favorites, LibrarySnapshot, PlayerState, RepeatMode, Song, event};
use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;
use relm4::{ComponentParts, RelmApp, SimpleComponent};

use crate::client::Client;
use crate::covers::CoverLoader;
use crate::data::LibraryData;

pub use player::PlayerBar;

pub struct AppInit {
    pub client: Client,
    pub snapshot: LibrarySnapshot,
    pub player: PlayerState,
    pub measure: Option<String>,
    pub page: Option<String>,
    pub query: Option<String>,
    pub open: Option<String>,
}

#[derive(Clone)]
pub struct PlayerUi {
    pub state: PlayerState,
    pub synced_at: Instant,
}

impl PlayerUi {
    pub fn new(state: PlayerState) -> Self {
        Self {
            state,
            synced_at: Instant::now(),
        }
    }

    pub fn set(&mut self, state: PlayerState) {
        self.state = state;
        self.synced_at = Instant::now();
    }

    pub fn position_ms(&self) -> u64 {
        let mut pos = self.state.position_ms;
        if self.state.playing {
            pos = pos.saturating_add(self.synced_at.elapsed().as_millis() as u64);
        }
        if let Some(track) = &self.state.track {
            pos.min(track.duration_ms)
        } else {
            pos
        }
    }
}

#[derive(Clone)]
pub struct Actions {
    pub client: Client,
    pub data: Rc<RefCell<LibraryData>>,
    pub covers: CoverLoader,
    pub player: Rc<RefCell<PlayerUi>>,
    pub stack: gtk::Stack,
    pub split: adw::OverlaySplitView,
    pub toast: adw::ToastOverlay,
    pub window_title: adw::WindowTitle,
    pub window: adw::ApplicationWindow,
    history: Rc<RefCell<Vec<String>>>,
    favorites_dirty: Rc<Cell<bool>>,
    pub search_entry: Rc<RefCell<Option<gtk::SearchEntry>>>,
    pub albums_adjustment: Rc<RefCell<Option<gtk::Adjustment>>>,
    built_pages: Rc<RefCell<HashSet<String>>>,
    nav_list: Rc<RefCell<Option<gtk::ListBox>>>,
    nav_rows: Rc<RefCell<HashMap<String, gtk::ListBoxRow>>>,
    nav_guard: Rc<Cell<bool>>,
    now_rows: Rc<RefCell<Vec<gtk::glib::WeakRef<gtk::Box>>>>,
    grid_syncers: Rc<RefCell<Vec<Rc<dyn Fn(i32)>>>>,
}

const PAGES: &[(&str, &str, &str)] = &[
    ("home", "Inicio", "go-home-symbolic"),
    ("search", "Buscar", "system-search-symbolic"),
    ("listen", "Escuchar ahora", "audio-headphones-symbolic"),
    ("albums", "Álbumes", "media-optical-symbolic"),
    ("artists", "Artistas", "avatar-default-symbolic"),
    ("songs", "Canciones", "audio-x-generic-symbolic"),
    ("playlists", "Playlists", "view-list-symbolic"),
    ("favorites", "Favoritos", "emblem-favorite-symbolic"),
];

fn page_title(name: &str) -> &'static str {
    PAGES
        .iter()
        .find(|(page, _, _)| *page == name)
        .map(|(_, title, _)| *title)
        .unwrap_or("Musa")
}

impl Actions {
    pub fn select(&self, name: &str) {
        self.history.borrow_mut().clear();
        if name == "favorites" && self.favorites_dirty.get() {
            self.refresh_favorites();
            self.favorites_dirty.set(false);
        }
        self.ensure_page(name);
        self.stack.set_visible_child_name(name);
        self.window_title.set_subtitle(page_title(name));
        if self.split.is_collapsed() {
            self.split.set_show_sidebar(false);
        }
        if let (Some(list), Some(row)) = (
            self.nav_list.borrow().as_ref(),
            self.nav_rows.borrow().get(name),
        ) {
            self.nav_guard.set(true);
            list.select_row(Some(row));
            self.nav_guard.set(false);
        }
    }

    /// Registra una fila de canción para poder marcarla como “sonando” sin
    /// reconstruir la lista completa.
    pub fn register_song_row(&self, row: &gtk::Box, song_id: &str) {
        row.set_widget_name(song_id);
        self.now_rows.borrow_mut().push(row.downgrade());
    }

    pub fn refresh_now_playing(&self) {
        let current = self
            .player
            .borrow()
            .state
            .track
            .as_ref()
            .map(|track| track.id.clone());
        let mut rows = self.now_rows.borrow_mut();
        rows.retain(|weak| {
            let Some(row) = weak.upgrade() else {
                return false;
            };
            let playing = current.as_deref() == Some(row.widget_name().as_str());
            if playing {
                row.add_css_class("now-playing");
            } else {
                row.remove_css_class("now-playing");
            }
            true
        });
    }

    /// Las páginas se construyen la primera vez que se visitan. Así el arranque
    /// no crea miles de filas ni decodifica portadas de secciones que nadie ha
    /// abierto, y el pico de memoria se mantiene acotado.
    fn ensure_page(&self, name: &str) {
        if self.built_pages.borrow().contains(name) {
            return;
        }
        let page: gtk::Widget = match name {
            "home" => pages::home_page(self),
            "listen" => pages::listen_page(self),
            "search" => search::search_page(self),
            "albums" => pages::albums_page(self),
            "artists" => pages::artists_page(self),
            "songs" => pages::songs_page(self),
            "playlists" => pages::playlists_page(self),
            "favorites" => pages::favorites_page(self),
            _ => return,
        };
        self.stack.add_named(&page, Some(name));
        self.built_pages.borrow_mut().insert(name.to_string());
    }

    pub fn open_detail(&self, page: &impl IsA<gtk::Widget>, subtitle: &str) {
        let current = self
            .stack
            .visible_child_name()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "home".into());
        if current != "detail" {
            self.history.borrow_mut().push(current);
        }
        if let Some(old) = self.stack.child_by_name("detail") {
            self.stack.remove(&old);
        }
        self.stack.add_named(page, Some("detail"));
        self.stack.set_visible_child_name("detail");
        self.window_title.set_subtitle(subtitle);
    }

    pub fn back(&self) {
        let previous = self
            .history
            .borrow_mut()
            .pop()
            .unwrap_or_else(|| "home".into());
        if let Some(old) = self.stack.child_by_name("detail") {
            self.stack.remove(&old);
        }
        self.stack.set_visible_child_name(&previous);
        self.window_title.set_subtitle(page_title(&previous));
    }

    pub fn toast(&self, message: impl Into<String>) {
        let toast = adw::Toast::new(&message.into());
        toast.set_timeout(3);
        self.toast.add_toast(toast);
    }

    pub fn toast_with_action(
        &self,
        message: impl Into<String>,
        label: &str,
        action: impl Fn() + 'static,
    ) {
        let toast = adw::Toast::new(&message.into());
        toast.set_timeout(4);
        toast.set_button_label(Some(label));
        toast.connect_button_clicked(move |_| action());
        self.toast.add_toast(toast);
    }

    pub fn play_songs(&self, songs: &[Song], start: usize) {
        if songs.is_empty() {
            return;
        }
        let start = start.min(songs.len() - 1);
        let queue: Vec<String> = songs.iter().map(|s| s.id.clone()).collect();
        let previous = self.player.borrow().state.clone();
        let state = PlayerState {
            queue,
            index: start,
            position_ms: 0,
            playing: true,
            shuffle: previous.shuffle,
            repeat: previous.repeat,
            volume: previous.volume,
            track: Some(songs[start].clone()),
            engine: previous.engine.clone(),
        };
        self.player.borrow_mut().set(state);
        let player = self.player.clone();
        let actions = self.clone();
        self.client.call(
            musa_protocol::cmd::QUEUE_SET,
            serde_json::json!({
                "song_ids": songs.iter().map(|s| s.id.clone()).collect::<Vec<_>>(),
                "index": start,
                "play": true,
            }),
            move |result| match result {
                Ok(value) => {
                    if let Ok(state) = serde_json::from_value::<PlayerState>(value) {
                        player.borrow_mut().set(state);
                    }
                }
                Err(_) => {
                    actions.toast("No se pudo iniciar la reproducción. Inténtalo de nuevo.");
                    player.borrow_mut().set(previous);
                }
            },
        );
    }

    pub fn play_song(&self, song: &Song) {
        let data = self.data.borrow();
        let album_songs = data
            .snapshot
            .songs
            .iter()
            .filter(|s| s.album_id == song.album_id)
            .cloned()
            .collect::<Vec<_>>();
        let start = album_songs
            .iter()
            .position(|s| s.id == song.id)
            .unwrap_or(0);
        drop(data);
        self.play_songs(&album_songs, start);
    }

    pub fn toggle_play(&self) {
        let playing = self.player.borrow().state.playing;
        let has_queue = !self.player.borrow().state.queue.is_empty();
        if !has_queue {
            if let Some(track) = self.player.borrow().state.track.clone() {
                self.play_song(&track);
            }
            return;
        }
        self.player.borrow_mut().state.playing = !playing;
        self.player.borrow_mut().synced_at = Instant::now();
        let actions = self.clone();
        let command = if playing {
            musa_protocol::cmd::PLAYER_PAUSE
        } else {
            musa_protocol::cmd::PLAYER_PLAY
        };
        self.client.call_simple(command, move |result| {
            if result.is_err() {
                actions.toast("No se pudo controlar la reproducción. Inténtalo de nuevo.");
            }
        });
    }

    pub fn next(&self) {
        self.client
            .call_simple(musa_protocol::cmd::PLAYER_NEXT, |_| {});
    }

    pub fn previous(&self) {
        self.client
            .call_simple(musa_protocol::cmd::PLAYER_PREV, |_| {});
    }

    pub fn seek(&self, position_ms: u64) {
        if let Some(track) = &self.player.borrow().state.track {
            let position_ms = position_ms.min(track.duration_ms);
            self.player.borrow_mut().state.position_ms = position_ms;
            self.player.borrow_mut().synced_at = Instant::now();
        }
        self.client.call(
            musa_protocol::cmd::PLAYER_SEEK,
            serde_json::json!({"position_ms": position_ms}),
            |_| {},
        );
    }

    pub fn toggle_shuffle(&self) {
        let enabled = !self.player.borrow().state.shuffle;
        self.player.borrow_mut().state.shuffle = enabled;
        let actions = self.clone();
        self.client.call(
            musa_protocol::cmd::PLAYER_SHUFFLE,
            serde_json::json!({"enabled": enabled}),
            move |result| {
                if result.is_err() {
                    actions.toast("No se pudo cambiar el modo aleatorio.");
                }
            },
        );
    }

    pub fn set_volume(&self, volume: u8) {
        let volume = volume.min(100);
        self.player.borrow_mut().state.volume = volume;
        self.client.call(
            musa_protocol::cmd::PLAYER_VOLUME,
            serde_json::json!({"volume": volume}),
            |_| {},
        );
    }

    pub fn cycle_repeat(&self) {
        let mode = self.player.borrow().state.repeat.cycle();
        self.player.borrow_mut().state.repeat = mode;
        self.client.call(
            musa_protocol::cmd::PLAYER_REPEAT,
            serde_json::json!({"mode": mode}),
            |_| {},
        );
    }

    pub fn toggle_favorite_track(&self) {
        let Some(track) = self.player.borrow().state.track.clone() else {
            return;
        };
        let favorite = !track.favorite;
        self.toggle_favorite("song", &track.id, favorite);
        self.player
            .borrow_mut()
            .state
            .track
            .as_mut()
            .unwrap()
            .favorite = favorite;
    }

    pub fn toggle_favorite(&self, kind: &str, id: &str, favorite: bool) {
        self.data.borrow_mut().update_favorite(kind, id, favorite);
        self.favorites_dirty.set(true);
        let actions = self.clone();
        let kind = kind.to_string();
        let id = id.to_string();
        let kind_cb = kind.clone();
        let id_cb = id.clone();
        crate::client::set_favorite(&self.client, &kind, &id, favorite, move |ok| {
            if ok {
                let undo = actions.clone();
                let kind_undo = kind_cb.clone();
                let id_undo = id_cb.clone();
                let message = if favorite {
                    "Añadido a Favoritos"
                } else {
                    "Quitado de Favoritos"
                };
                actions.toast_with_action(message, "Deshacer", move || {
                    undo.toggle_favorite(&kind_undo, &id_undo, !favorite);
                });
            } else {
                actions.toast("No se pudo guardar el favorito. Inténtalo de nuevo.");
                actions
                    .data
                    .borrow_mut()
                    .update_favorite(&kind_cb, &id_cb, !favorite);
            }
        });
    }

    pub fn refresh_favorites(&self) {
        if let Some(old) = self.stack.child_by_name("favorites") {
            self.stack.remove(&old);
        }
        let page = pages::favorites_page(self);
        self.stack.add_named(&page, Some("favorites"));
        self.built_pages.borrow_mut().insert("favorites".into());
        if self.stack.visible_child_name().as_deref() == Some("favorites") {
            self.stack.set_visible_child_name("favorites");
        }
    }

    pub fn current_track(&self) -> Option<Song> {
        self.player.borrow().state.track.clone()
    }
}

pub fn run(init: AppInit) {
    let app = RelmApp::new("io.github.juliogarcia.Musa").with_args(vec![
        std::env::args().next().unwrap_or_else(|| "musa".into()),
    ]);
    app.run::<AppModel>(init);
}

struct AppModel;

#[relm4::component]
impl SimpleComponent for AppModel {
    type Init = AppInit;
    type Input = ();
    type Output = ();

    view! {
        main_window = adw::ApplicationWindow {
            set_title: Some("Musa"),
            set_default_width: 1280,
            set_default_height: 820,
            set_width_request: 940,
            set_height_request: 640,
        }
    }

    fn init(
        init: AppInit,
        _root: Self::Root,
        _sender: relm4::ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        install_css();
        adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);
        let widgets = view_output!();
        let (actions, bar) = build_shell(
            &widgets.main_window,
            init.client,
            init.snapshot,
            init.player,
        );
        start_ticker(&actions, &bar);
        if let Some(page) = init.page.as_deref() {
            if page != "home" {
                actions.select(page);
            }
        }
        if let Some(query) = init.query.as_deref() {
            actions.select("search");
            if let Some(entry) = actions.search_entry.borrow().as_ref() {
                entry.set_text(query);
            }
        }
        if let Some(id) = init.open.as_deref() {
            if id.starts_with("playlist-") {
                actions.open_playlist(id);
            } else if id.starts_with("artist-") {
                actions.open_artist(id);
            } else {
                actions.open_album(id);
            }
        }
        if let Some(mode) = init.measure {
            measure::start(&actions, &bar, &mode);
        }
        if std::env::var_os("MUSA_OPEN_LYRICS").is_some() {
            let actions = actions.clone();
            gtk::glib::timeout_add_local_once(Duration::from_millis(900), move || {
                actions.open_lyrics();
            });
        }
        let _ = (&actions, &bar);
        ComponentParts {
            model: AppModel,
            widgets,
        }
    }
}

fn build_shell(
    window: &adw::ApplicationWindow,
    client: Client,
    snapshot: LibrarySnapshot,
    initial_player: PlayerState,
) -> (Actions, PlayerBar) {
    let data = Rc::new(RefCell::new(LibraryData::new(snapshot)));
    let player = Rc::new(RefCell::new(PlayerUi::new(initial_player)));
    let covers = CoverLoader::new();

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_transition_duration(160);
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(false);
    stack.set_vexpand(true);
    stack.set_hexpand(true);

    let split = adw::OverlaySplitView::new();
    split.set_min_sidebar_width(196.0);
    split.set_max_sidebar_width(238.0);
    split.set_sidebar_width_fraction(0.18);
    split.set_show_sidebar(true);
    // El contenido se centra y no pasa de 1400 px: en pantallas anchas la
    // biblioteca deja de estirarse y los textos no cruzan media pantalla.
    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(1400);
    clamp.set_tightening_threshold(1180);
    clamp.set_child(Some(&stack));
    clamp.set_vexpand(true);
    clamp.set_hexpand(true);
    split.set_content(Some(&clamp));
    split.set_vexpand(true);

    let toast = adw::ToastOverlay::new();

    let window_title = adw::WindowTitle::new("Musa", "Inicio");
    let history = Rc::new(RefCell::new(Vec::new()));
    let favorites_dirty = Rc::new(Cell::new(false));
    let search_entry = Rc::new(RefCell::new(None));
    let albums_adjustment = Rc::new(RefCell::new(None));
    let built_pages = Rc::new(RefCell::new(HashSet::new()));
    let nav_list = Rc::new(RefCell::new(None));
    let nav_rows = Rc::new(RefCell::new(HashMap::new()));
    let nav_guard = Rc::new(Cell::new(false));
    let now_rows = Rc::new(RefCell::new(Vec::new()));
    let grid_syncers: Rc<RefCell<Vec<Rc<dyn Fn(i32)>>>> = Rc::new(RefCell::new(Vec::new()));

    let actions = Actions {
        client: client.clone(),
        data: data.clone(),
        covers: covers.clone(),
        player: player.clone(),
        stack: stack.clone(),
        split: split.clone(),
        toast: toast.clone(),
        window_title: window_title.clone(),
        window: window.clone(),
        history,
        favorites_dirty,
        search_entry,
        albums_adjustment,
        built_pages,
        nav_list,
        nav_rows,
        nav_guard,
        now_rows,
        grid_syncers,
    };

    // Barra superior.
    let header = adw::HeaderBar::new();
    let sidebar_button = gtk::Button::from_icon_name("sidebar-show-symbolic");
    sidebar_button.set_tooltip_text(Some("Mostrar u ocultar la barra lateral"));
    sidebar_button.add_css_class("icon-button");
    let split_for_button = split.clone();
    sidebar_button.connect_clicked(move |_| {
        split_for_button.set_show_sidebar(!split_for_button.shows_sidebar());
    });
    header.pack_start(&sidebar_button);
    // El título de la página vive en el H1 del contenido; el header queda para
    // acciones y no duplica “Musa / página”.

    let search_button = gtk::Button::from_icon_name("system-search-symbolic");
    search_button.set_tooltip_text(Some("Buscar"));
    search_button.add_css_class("icon-button");
    let actions_for_search = actions.clone();
    search_button.connect_clicked(move |_| {
        actions_for_search.select("search");
        if let Some(entry) = actions_for_search.search_entry.borrow().as_ref() {
            entry.grab_focus();
        }
    });
    header.pack_end(&search_button);

    // Barra lateral.
    let sidebar = build_sidebar(&actions);
    split.set_sidebar(Some(&sidebar));

    // En ventanas estrechas la navegación se convierte en un panel superpuesto:
    // recupera el ancho del contenido sin perder el acceso a las secciones.
    let auto_hidden = Rc::new(Cell::new(false));
    let split_resp = split.clone();
    let window_resp = window.clone();
    let auto = auto_hidden.clone();
    let stack_resp = stack.clone();
    let syncers = actions.grid_syncers.clone();
    gtk::glib::timeout_add_local(Duration::from_millis(180), move || {
        let width = window_resp.width();
        if width <= 0 {
            return gtk::glib::ControlFlow::Continue;
        }
        let narrow = width < 1100;
        if narrow && !auto.get() {
            split_resp.set_collapsed(true);
            split_resp.set_show_sidebar(false);
            auto.set(true);
        } else if !narrow && auto.get() {
            split_resp.set_collapsed(false);
            split_resp.set_show_sidebar(true);
            auto.set(false);
        }
        // Las rejillas se recalculan con el ancho real del contenido, que
        // cambia con la sidebar, el clamp y el tamaño de la ventana.
        let content = stack_resp.width();
        if content > 0 {
            for sync in syncers.borrow().iter() {
                sync(content);
            }
        }
        gtk::glib::ControlFlow::Continue
    });

    // Páginas: la primera se crea al mostrarse (ver Actions::ensure_page).
    actions.select("home");

    let bar = PlayerBar::new(&actions);

    let layout = gtk::Box::new(gtk::Orientation::Vertical, 0);
    layout.append(&split);
    layout.append(&bar.root);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&layout));
    toast.set_child(Some(&toolbar));
    window.set_content(Some(&toast));

    // Cerrar la ventana debe parar el audio y apagar el daemon; el motor no
    // queda sonando en segundo plano si el usuario cierra la UI.
    let closing = Rc::new(Cell::new(false));
    let close_client = actions.client.clone();
    window.connect_close_request(move |window| {
        if closing.get() {
            return gtk::glib::Propagation::Proceed;
        }
        closing.set(true);
        close_client.call_simple(musa_protocol::cmd::SHUTDOWN, |_| {});
        let window = window.clone();
        gtk::glib::timeout_add_local_once(Duration::from_millis(450), move || {
            window.destroy();
        });
        gtk::glib::Propagation::Stop
    });
    (actions, bar)
}

fn build_sidebar(actions: &Actions) -> gtk::Widget {
    let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 0);
    sidebar.add_css_class("sidebar");
    sidebar.set_width_request(212);

    let brand = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    brand.add_css_class("brand-row");
    brand.set_margin_top(16);
    brand.set_margin_bottom(14);
    brand.set_margin_start(18);
    brand.set_margin_end(18);
    // Marca: ratoncito gris con detalles negros, dibujado por la app para no
    // depender del tema de iconos del escritorio.
    let logo = gtk::Image::new();
    let logo_bytes = gtk::glib::Bytes::from_static(include_bytes!("../../assets/logo-mouse.png"));
    if let Ok(texture) = gtk::gdk::Texture::from_bytes(&logo_bytes) {
        logo.set_paintable(Some(&texture));
    }
    logo.set_pixel_size(28);
    logo.set_valign(gtk::Align::Center);
    logo.add_css_class("brand-logo");
    let brand_label = gtk::Label::new(Some("Musa"));
    brand_label.add_css_class("brand");
    brand_label.set_xalign(0.0);
    brand.append(&logo);
    brand.append(&brand_label);
    sidebar.append(&brand);

    let eyebrow = gtk::Label::new(Some("NAVEGACIÓN"));
    eyebrow.add_css_class("eyebrow");
    eyebrow.set_xalign(0.0);
    eyebrow.set_margin_start(22);
    eyebrow.set_margin_bottom(6);
    sidebar.append(&eyebrow);

    let list = gtk::ListBox::new();
    list.add_css_class("sidebar-list");
    list.set_selection_mode(gtk::SelectionMode::Single);
    *actions.nav_list.borrow_mut() = Some(list.clone());
    for (name, label, icon) in PAGES {
        let row = gtk::ListBoxRow::new();
        row.set_activatable(true);
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        content.add_css_class("nav-row");
        content.set_margin_start(8);
        content.set_margin_end(8);
        content.set_margin_top(2);
        content.set_margin_bottom(2);
        let image = if *name == "favorites" {
            let image = crate::ui::widgets::heart_image(false, 18);
            image.add_css_class("nav-icon");
            image
        } else {
            let image = gtk::Image::from_icon_name(icon);
            image.set_pixel_size(18);
            image.add_css_class("nav-icon");
            image
        };
        let text = gtk::Label::new(Some(label));
        text.set_xalign(0.0);
        text.add_css_class("nav-label");
        content.append(&image);
        content.append(&text);
        row.set_child(Some(&content));
        list.append(&row);
        row.set_widget_name(name);
        actions
            .nav_rows
            .borrow_mut()
            .insert(name.to_string(), row.clone());
    }
    list.set_margin_start(8);
    list.set_margin_end(8);

    let actions_for_list = actions.clone();
    let guard = actions.nav_guard.clone();
    list.connect_row_selected(move |_, row| {
        if guard.get() {
            return;
        }
        let Some(row) = row else { return };
        let name = row.widget_name().to_string();
        actions_for_list.select(&name);
    });
    let actions_for_activate = actions.clone();
    list.connect_row_activated(move |_, row| {
        actions_for_activate.select(&row.widget_name());
    });
    sidebar.append(&list);

    let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    sidebar.append(&spacer);

    let songs = crate::data::format_count(actions.data.borrow().snapshot.stats.songs as usize);
    let footer_text = format!("Apple Music · {songs} canciones");
    let footer = gtk::Label::new(Some(&footer_text));
    footer.add_css_class("sidebar-footer");
    footer.set_xalign(0.0);
    footer.set_margin_start(20);
    footer.set_margin_end(16);
    footer.set_margin_bottom(16);
    sidebar.append(&footer);

    sidebar.upcast()
}

fn start_ticker(actions: &Actions, bar: &PlayerBar) {
    let actions = actions.clone();
    let bar = bar.clone();
    let mut warned_offline = false;
    gtk::glib::timeout_add_local(Duration::from_millis(120), move || {
        actions.covers.pump();
        let events = actions.client.pump();
        for event in events {
            match event.event.as_str() {
                event::PLAYER_CHANGED => {
                    if let Ok(state) = serde_json::from_value::<PlayerState>(event.data) {
                        actions.player.borrow_mut().set(state);
                    }
                }
                event::LIBRARY_CHANGED => {
                    if let Ok(favorites) = serde_json::from_value::<Favorites>(event.data) {
                        actions.data.borrow_mut().apply_favorites(&favorites);
                        actions.favorites_dirty.set(true);
                    }
                }
                event::ENGINE_CHANGED => {
                    actions.toast("El reproductor se ha reiniciado. Vuelve a darle a reproducir.");
                }
                _ => {}
            }
        }
        bar.refresh(&actions);
        actions.refresh_now_playing();
        if !actions.client.is_online() && !warned_offline {
            warned_offline = true;
            actions.toast("Se perdió la conexión con la biblioteca. Reintentando…");
        }
        gtk::glib::ControlFlow::Continue
    });
}

pub fn repeat_icon(mode: RepeatMode) -> &'static str {
    match mode {
        RepeatMode::One => "media-playlist-repeat-song-symbolic",
        _ => "media-playlist-repeat-symbolic",
    }
}

fn install_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(include_str!("style.css"));
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}
