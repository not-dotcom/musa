//! Búsqueda global contra el daemon (biblioteca y catálogo) con debounce.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::time::Duration;

use musa_protocol::{Album, Artist, CatalogResults, SearchResults, Song};
use relm4::gtk;
use relm4::gtk::glib;
use relm4::gtk::prelude::*;

use super::Actions;
use super::widgets::{
    album_card_button, artist_card_button, catalog_song_result_button, page_header, page_scroll,
    playlist_card_button, song_result_button,
};

/// Lo que se ha recibido de cada fuente. `None` significa “todavía buscando”.
#[derive(Default)]
struct SearchData {
    library: Option<SearchResults>,
    catalog: Option<CatalogResults>,
    catalog_failed: bool,
}

pub fn search_page(actions: &Actions) -> gtk::Widget {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    content.set_margin_top(24);
    content.set_margin_bottom(30);
    content.set_margin_start(30);
    content.set_margin_end(30);
    content.append(&page_header(
        "Buscar",
        "Tu biblioteca y el catálogo de Apple Music.",
    ));

    let entry = gtk::SearchEntry::new();
    entry.set_placeholder_text(Some("Busca en tu biblioteca y en Apple Music"));
    entry.set_hexpand(false);
    entry.set_width_request(560);
    entry.set_halign(gtk::Align::Start);
    entry.add_css_class("search-entry");
    content.append(&entry);
    *actions.search_entry.borrow_mut() = Some(entry.clone());

    let results = gtk::Box::new(gtk::Orientation::Vertical, 24);
    let hint = gtk::Label::new(Some(
        "Escribe para empezar. También puedes buscar por género.",
    ));
    hint.add_css_class("empty-state");
    hint.set_vexpand(true);
    hint.set_valign(gtk::Align::Center);
    results.append(&hint);
    content.append(&results);

    let timeout: Rc<Cell<Option<glib::SourceId>>> = Rc::new(Cell::new(None));
    let current_query = Rc::new(RefCell::new(String::new()));
    let data = Rc::new(RefCell::new(SearchData::default()));
    let actions_entry = actions.clone();
    let results_clone = results.clone();
    let timeout_clone = timeout.clone();
    let current_clone = current_query.clone();
    let data_clone = data.clone();
    entry.connect_search_changed(move |entry| {
        if let Some(source) = timeout_clone.take() {
            // La fuente pudo ejecutarse ya y destruirse sola; en ese caso el
            // slot se vació al disparar (ver abajo) y este `remove()` no se
            // llama. Quitar un id muerto hace panic en glib y aborta la app.
            source.remove();
        }
        let query = entry.text().to_string();
        *current_clone.borrow_mut() = query.clone();
        *data_clone.borrow_mut() = SearchData::default();
        if query.trim().is_empty() {
            clear_results(
                &results_clone,
                Some("Escribe para empezar. También puedes buscar por género."),
            );
            return;
        }
        show_loading(&results_clone);
        let actions = actions_entry.clone();
        let results_box = results_clone.clone();
        let current = current_clone.clone();
        let data = data_clone.clone();
        let fired = timeout_clone.clone();
        let source = glib::timeout_add_local(Duration::from_millis(280), move || {
            // La fuente devuelve `Break` y glib la destruye al terminar. Si el
            // slot siguiera apuntando a este id, el siguiente borrado en el
            // campo llamaría a `SourceId::remove` sobre una fuente inexistente
            // y glib haría panic (abortando la interfaz).
            fired.set(None);
            let query = current.borrow().clone();
            if query.trim().is_empty() {
                return glib::ControlFlow::Break;
            }

            // Biblioteca: respuesta inmediata y local.
            let library_actions = actions.clone();
            let library_box = results_box.clone();
            let library_current = current.clone();
            let library_data = data.clone();
            let library_query = query.clone();
            actions.client.call(
                musa_protocol::cmd::SEARCH,
                serde_json::json!({"query": library_query.clone(), "limit": 50}),
                move |result| {
                    if *library_current.borrow() != library_query {
                        return;
                    }
                    match result {
                        Ok(value) => match serde_json::from_value::<SearchResults>(value) {
                            Ok(results) => {
                                library_data.borrow_mut().library = Some(results);
                            }
                            Err(_) => {
                                library_data.borrow_mut().library =
                                    Some(empty_results(&library_query));
                                library_actions.toast(
                                    "No se pudieron leer los resultados. Inténtalo de nuevo.",
                                );
                            }
                        },
                        Err(_) => {
                            library_data.borrow_mut().library = Some(empty_results(&library_query));
                            library_actions.toast(
                                "No se pudo buscar. Comprueba la conexión e inténtalo de nuevo.",
                            );
                        }
                    }
                    render(&library_actions, &library_box, &library_data);
                },
            );

            // Catálogo: puede tardar más y traer canciones que no tienes.
            let catalog_actions = actions.clone();
            let catalog_box = results_box.clone();
            let catalog_current = current.clone();
            let catalog_data = data.clone();
            let catalog_query = query.clone();
            actions.client.call(
                musa_protocol::cmd::SEARCH_CATALOG,
                serde_json::json!({"query": query, "limit": 20}),
                move |result| {
                    if *catalog_current.borrow() != catalog_query {
                        return;
                    }
                    {
                        let mut state = catalog_data.borrow_mut();
                        match result {
                            Ok(value) => match serde_json::from_value::<CatalogResults>(value) {
                                Ok(results) => state.catalog = Some(results),
                                Err(_) => {
                                    state.catalog = Some(empty_catalog());
                                    state.catalog_failed = true;
                                }
                            },
                            Err(_) => {
                                state.catalog = Some(empty_catalog());
                                state.catalog_failed = true;
                            }
                        }
                    }
                    render(&catalog_actions, &catalog_box, &catalog_data);
                },
            );

            glib::ControlFlow::Break
        });
        timeout_clone.set(Some(source));
    });

    page_scroll(&content).upcast()
}

fn empty_results(query: &str) -> SearchResults {
    SearchResults {
        query: query.to_string(),
        songs: Vec::new(),
        albums: Vec::new(),
        artists: Vec::new(),
        playlists: Vec::new(),
    }
}

fn empty_catalog() -> CatalogResults {
    CatalogResults {
        query: String::new(),
        songs: Vec::new(),
        albums: Vec::new(),
        artists: Vec::new(),
    }
}

fn show_loading(results: &gtk::Box) {
    clear_results(results, None);
    let box_ = gtk::Box::new(gtk::Orientation::Vertical, 10);
    box_.set_halign(gtk::Align::Center);
    box_.set_valign(gtk::Align::Center);
    box_.set_vexpand(true);
    let spinner = gtk::Spinner::new();
    spinner.start();
    spinner.set_size_request(24, 24);
    let label = gtk::Label::new(Some("Buscando…"));
    label.add_css_class("empty-state");
    box_.append(&spinner);
    box_.append(&label);
    results.append(&box_);
}

fn clear_results(results: &gtk::Box, hint: Option<&str>) {
    while let Some(child) = results.first_child() {
        results.remove(&child);
    }
    if let Some(text) = hint {
        let label = gtk::Label::new(Some(text));
        label.add_css_class("empty-state");
        label.set_vexpand(true);
        label.set_valign(gtk::Align::Center);
        results.append(&label);
    }
}

/// Pinta lo que ya haya de cada fuente. Se llama dos veces por búsqueda: al
/// llegar la biblioteca y al llegar el catálogo, sin borrar lo anterior.
fn render(actions: &Actions, container: &gtk::Box, data: &Rc<RefCell<SearchData>>) {
    let (library, catalog, catalog_failed) = {
        let state = data.borrow();
        (
            state.library.clone(),
            state.catalog.clone(),
            state.catalog_failed,
        )
    };
    clear_results(container, None);
    let mut shown = 0usize;

    match &library {
        Some(results) => shown += render_library(actions, container, results),
        None => container.append(&pending_label("Buscando en tu biblioteca…")),
    }

    if let Some(results) = &catalog {
        // No repetir canciones que ya salieron en la biblioteca.
        let known_songs: HashSet<&str> = library
            .as_ref()
            .map(|library| library.songs.iter().map(|song| song.id.as_str()).collect())
            .unwrap_or_default();
        let fresh_songs: Vec<Song> = results
            .songs
            .iter()
            .filter(|song| !known_songs.contains(song.id.as_str()))
            .take(20)
            .cloned()
            .collect();
        if !fresh_songs.is_empty() {
            container.append(&section("En Apple Music"));
            let fresh = Rc::new(fresh_songs);
            let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
            for index in 0..fresh.len() {
                list.append(&catalog_song_result_button(actions, &fresh, index));
            }
            container.append(&list);
            shown += fresh.len();
        }

        // Tampoco repetir álbumes y artistas que ya están en la biblioteca.
        let known_albums: HashSet<(String, String)> = library
            .as_ref()
            .map(|library| {
                library
                    .albums
                    .iter()
                    .map(|album| (album.title.to_lowercase(), album.artist_name.to_lowercase()))
                    .collect()
            })
            .unwrap_or_default();
        let fresh_albums: Vec<Album> = results
            .albums
            .iter()
            .filter(|album| {
                !known_albums
                    .contains(&(album.title.to_lowercase(), album.artist_name.to_lowercase()))
            })
            .take(10)
            .cloned()
            .collect();
        if !fresh_albums.is_empty() {
            container.append(&section("Álbumes en Apple Music"));
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 2);
            for album in &fresh_albums {
                row.append(&album_card_button(actions, album));
            }
            container.append(&carousel(&row));
            shown += fresh_albums.len();
        }

        let known_artists: HashSet<String> = library
            .as_ref()
            .map(|library| {
                library
                    .artists
                    .iter()
                    .map(|artist| artist.name.to_lowercase())
                    .collect()
            })
            .unwrap_or_default();
        let fresh_artists: Vec<Artist> = results
            .artists
            .iter()
            .filter(|artist| !known_artists.contains(&artist.name.to_lowercase()))
            .take(10)
            .cloned()
            .collect();
        if !fresh_artists.is_empty() {
            container.append(&section("Artistas en Apple Music"));
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 2);
            for artist in &fresh_artists {
                row.append(&artist_card_button(actions, artist));
            }
            container.append(&carousel(&row));
            shown += fresh_artists.len();
        }
    } else {
        container.append(&pending_label("Buscando en Apple Music…"));
    }

    if catalog_failed {
        let note = gtk::Label::new(Some("No se pudo buscar en Apple Music."));
        note.set_xalign(0.0);
        note.add_css_class("muted-small");
        container.append(&note);
    }

    if shown == 0 && library.is_some() && catalog.is_some() && !catalog_failed {
        clear_results(
            container,
            Some("Sin resultados. Prueba con otro título, artista o género."),
        );
    }
}

fn render_library(actions: &Actions, container: &gtk::Box, results: &SearchResults) -> usize {
    let mut shown = 0usize;

    if !results.songs.is_empty() {
        container.append(&section("Canciones"));
        let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
        for song in results.songs.iter().take(25) {
            list.append(&song_result_button(actions, song));
            shown += 1;
        }
        container.append(&list);
    }

    if !results.albums.is_empty() {
        container.append(&section("Álbumes"));
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        for album in results.albums.iter().take(10) {
            row.append(&album_card_button(actions, album));
            shown += 1;
        }
        container.append(&carousel(&row));
    }

    if !results.artists.is_empty() {
        container.append(&section("Artistas"));
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        for artist in results.artists.iter().take(10) {
            row.append(&artist_card_button(actions, artist));
            shown += 1;
        }
        container.append(&carousel(&row));
    }

    if !results.playlists.is_empty() {
        container.append(&section("Playlists"));
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        for playlist in results.playlists.iter().take(8) {
            row.append(&playlist_card_button(actions, playlist));
            shown += 1;
        }
        container.append(&carousel(&row));
    }

    shown
}

fn section(title: &str) -> gtk::Widget {
    let label = gtk::Label::new(Some(title));
    label.set_xalign(0.0);
    label.add_css_class("h2");
    label.upcast()
}

fn pending_label(text: &str) -> gtk::Widget {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.add_css_class("muted-small");
    label.upcast()
}

fn carousel(row: &gtk::Box) -> gtk::Widget {
    let scroller = gtk::ScrolledWindow::new();
    scroller.set_policy(gtk::PolicyType::External, gtk::PolicyType::Never);
    scroller.set_propagate_natural_height(true);
    scroller.set_child(Some(row));
    scroller.add_css_class("carousel");
    scroller.upcast()
}
