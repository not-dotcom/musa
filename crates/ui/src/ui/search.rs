//! Búsqueda global contra el daemon (con debounce y resultados descartables).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use musa_protocol::SearchResults;
use relm4::gtk;
use relm4::gtk::glib;
use relm4::gtk::prelude::*;

use super::Actions;
use super::widgets::{
    album_card_button, artist_card_button, page_header, page_scroll, playlist_card_button,
};

pub fn search_page(actions: &Actions) -> gtk::Widget {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    content.set_margin_top(24);
    content.set_margin_bottom(30);
    content.set_margin_start(30);
    content.set_margin_end(30);
    content.append(&page_header(
        "Buscar",
        "Artistas, álbumes, canciones y playlists de tu biblioteca.",
    ));

    let entry = gtk::SearchEntry::new();
    entry.set_placeholder_text(Some("Busca en tu biblioteca"));
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
    let actions_entry = actions.clone();
    let results_clone = results.clone();
    let timeout_clone = timeout.clone();
    let current_clone = current_query.clone();
    entry.connect_search_changed(move |entry| {
        if let Some(source) = timeout_clone.take() {
            source.remove();
        }
        let query = entry.text().to_string();
        *current_clone.borrow_mut() = query.clone();
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
        let source = glib::timeout_add_local(Duration::from_millis(280), move || {
            let query = current.borrow().clone();
            if query.trim().is_empty() {
                return glib::ControlFlow::Break;
            }
            let actions = actions.clone();
            let results_box = results_box.clone();
            let current = current.clone();
            let client = actions.client.clone();
            client.call(
                musa_protocol::cmd::SEARCH,
                serde_json::json!({"query": query, "limit": 50}),
                move |result| {
                    if *current.borrow() != query {
                        return;
                    }
                    match result {
                        Ok(value) => match serde_json::from_value::<SearchResults>(value) {
                            Ok(results) => render_results(&actions, &results_box, &results),
                            Err(_) => actions
                                .toast("No se pudieron leer los resultados. Inténtalo de nuevo."),
                        },
                        Err(_) => actions.toast(
                            "No se pudo buscar. Comprueba la conexión e inténtalo de nuevo.",
                        ),
                    }
                },
            );
            glib::ControlFlow::Break
        });
        timeout_clone.set(Some(source));
    });

    page_scroll(&content).upcast()
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

fn render_results(actions: &Actions, container: &gtk::Box, results: &SearchResults) {
    clear_results(container, None);
    let total = results.songs.len()
        + results.albums.len()
        + results.artists.len()
        + results.playlists.len();
    if total == 0 {
        clear_results(
            container,
            Some("Sin resultados. Prueba con otro título, artista o género."),
        );
        return;
    }

    if !results.songs.is_empty() {
        container.append(&section("Canciones"));
        let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
        for song in results.songs.iter().take(25) {
            let button = super::widgets::song_result_button(actions, song);
            list.append(&button);
        }
        container.append(&list);
    }

    if !results.albums.is_empty() {
        container.append(&section("Álbumes"));
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        for album in results.albums.iter().take(10) {
            row.append(&album_card_button(actions, album));
        }
        container.append(&carousel(&row));
    }

    if !results.artists.is_empty() {
        container.append(&section("Artistas"));
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        for artist in results.artists.iter().take(10) {
            row.append(&artist_card_button(actions, artist));
        }
        container.append(&carousel(&row));
    }

    if !results.playlists.is_empty() {
        container.append(&section("Playlists"));
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        for playlist in results.playlists.iter().take(8) {
            row.append(&playlist_card_button(actions, playlist));
        }
        container.append(&carousel(&row));
    }
}

fn section(title: &str) -> gtk::Widget {
    let label = gtk::Label::new(Some(title));
    label.set_xalign(0.0);
    label.add_css_class("h2");
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
