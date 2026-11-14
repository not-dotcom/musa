//! Barra de reproducción persistente, cola y ventana de letras.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use musa_protocol::{Lyrics, RepeatMode};
use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;

use super::{Actions, repeat_icon};
use crate::data::duration;

#[derive(Clone)]
pub struct PlayerBar {
    pub root: gtk::CenterBox,
    cover: gtk::Picture,
    title: gtk::Label,
    artist: gtk::Label,
    play: gtk::Button,
    play_icon: gtk::Image,
    previous: gtk::Button,
    next: gtk::Button,
    shuffle: gtk::ToggleButton,
    shuffle_icon: gtk::Image,
    repeat_icon: gtk::Image,
    repeat_button: gtk::Button,
    heart: gtk::ToggleButton,
    volume: gtk::Scale,
    volume_button: gtk::Button,
    volume_icon: gtk::Image,
    lyrics: gtk::Button,
    queue: gtk::Button,
    elapsed: gtk::Label,
    total: gtk::Label,
    progress: gtk::Scale,
    last_cover_key: Rc<RefCell<String>>,
    last_progress_track: Rc<RefCell<String>>,
    last_progress_ms: Rc<Cell<u64>>,
    last_duration_ms: Rc<Cell<u64>>,
    last_volume: Rc<Cell<u8>>,
    left: gtk::Box,
    controls: gtk::Box,
    extras: gtk::Box,
    center_width: Rc<Cell<i32>>,
    progress_source: Rc<RefCell<Option<gtk::glib::SourceId>>>,
    shuffle_guard: Rc<Cell<bool>>,
    heart_guard: Rc<Cell<bool>>,
    volume_guard: Rc<Cell<bool>>,
    progress_guard: Rc<Cell<bool>>,
}

impl PlayerBar {
    pub fn new(actions: &Actions) -> Self {
        let initial_volume = actions.player.borrow().state.volume;
        let root = gtk::CenterBox::new();
        root.add_css_class("playerbar");
        root.set_height_request(88);
        root.set_margin_top(8);
        root.set_margin_bottom(8);
        root.set_margin_start(8);
        root.set_margin_end(8);

        // Zona izquierda: portada + texto + favorito, todo seguido.
        let left = gtk::Box::new(gtk::Orientation::Horizontal, 14);
        left.set_valign(gtk::Align::Center);
        left.set_margin_end(24);
        let cover = gtk::Picture::new();
        cover.set_size_request(56, 56);
        cover.set_content_fit(gtk::ContentFit::Cover);
        cover.add_css_class("cover");
        cover.add_css_class("player-cover");
        left.append(&cover);

        let meta_and_heart = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        meta_and_heart.set_valign(gtk::Align::Center);
        let meta = gtk::Box::new(gtk::Orientation::Vertical, 3);
        meta.set_valign(gtk::Align::Center);
        let title = gtk::Label::new(Some("Nada en reproducción"));
        title.set_xalign(0.0);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.set_width_chars(1);
        title.set_max_width_chars(28);
        title.add_css_class("player-title");
        let artist = gtk::Label::new(Some("Elige un álbum o una playlist"));
        artist.set_xalign(0.0);
        artist.set_ellipsize(gtk::pango::EllipsizeMode::End);
        artist.set_width_chars(1);
        artist.set_max_width_chars(28);
        artist.add_css_class("player-artist");
        meta.append(&title);
        meta.append(&artist);
        meta_and_heart.append(&meta);

        let heart = gtk::ToggleButton::new();
        heart.set_child(Some(&super::widgets::heart_image(false, 20)));
        heart.add_css_class("favorite-button");
        heart.set_tooltip_text(Some("Añadir a favoritos"));
        meta_and_heart.append(&heart);
        left.append(&meta_and_heart);

        // Zona central: controles + progreso, ancho fijo.
        let controls = gtk::Box::new(gtk::Orientation::Vertical, 6);
        controls.set_size_request(360, -1);
        controls.set_valign(gtk::Align::Center);
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        buttons.set_halign(gtk::Align::Center);
        let shuffle = gtk::ToggleButton::new();
        let shuffle_icon = gtk::Image::from_icon_name("musa-shuffle");
        shuffle_icon.set_pixel_size(20);
        shuffle_icon.add_css_class("stroke-icon");
        shuffle.set_child(Some(&shuffle_icon));
        shuffle.add_css_class("shuffle-button");
        shuffle.set_tooltip_text(Some("Aleatorio"));
        buttons.append(&shuffle);
        let previous = gtk::Button::new();
        let previous_icon = gtk::Image::from_icon_name("musa-skip-back");
        previous_icon.set_pixel_size(22);
        previous_icon.add_css_class("stroke-icon");
        previous.set_child(Some(&previous_icon));
        previous.set_tooltip_text(Some("Anterior"));
        buttons.append(&previous);
        let play = gtk::Button::new();
        let play_icon = gtk::Image::from_icon_name("musa-play");
        play_icon.set_pixel_size(22);
        play.set_child(Some(&play_icon));
        play.add_css_class("play");
        play.set_tooltip_text(Some("Reproducir o pausar"));
        buttons.append(&play);
        let next = gtk::Button::new();
        let next_icon = gtk::Image::from_icon_name("musa-skip-forward");
        next_icon.set_pixel_size(22);
        next_icon.add_css_class("stroke-icon");
        next.set_child(Some(&next_icon));
        next.set_tooltip_text(Some("Siguiente"));
        buttons.append(&next);
        let repeat_button = gtk::Button::new();
        let repeat_icon = gtk::Image::from_icon_name("musa-repeat");
        repeat_icon.set_pixel_size(20);
        repeat_icon.add_css_class("stroke-icon");
        repeat_button.set_child(Some(&repeat_icon));
        repeat_button.set_tooltip_text(Some("Repetición"));
        buttons.append(&repeat_button);
        controls.append(&buttons);

        let progress_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let elapsed = gtk::Label::new(Some("0:00"));
        elapsed.set_size_request(36, -1);
        elapsed.set_width_chars(4);
        elapsed.set_xalign(1.0);
        elapsed.add_css_class("time-label");
        let progress = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 1.0);
        progress.set_draw_value(false);
        progress.set_hexpand(true);
        progress.add_css_class("progress-scale");
        let total = gtk::Label::new(Some("0:00"));
        total.set_size_request(36, -1);
        total.set_width_chars(4);
        total.set_xalign(0.0);
        total.add_css_class("time-label");
        progress_row.append(&elapsed);
        progress_row.append(&progress);
        progress_row.append(&total);
        controls.append(&progress_row);

        // Zona derecha: letras, cola y volumen.
        let extras = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        extras.set_valign(gtk::Align::Center);
        extras.set_halign(gtk::Align::End);
        extras.set_margin_start(24);
        let lyrics_button = gtk::Button::new();
        let lyrics_icon = gtk::Image::from_icon_name("musa-message-square-text");
        lyrics_icon.set_pixel_size(20);
        lyrics_icon.add_css_class("stroke-icon");
        lyrics_button.set_child(Some(&lyrics_icon));
        lyrics_button.set_tooltip_text(Some("Letras"));
        extras.append(&lyrics_button);
        let queue_button = gtk::Button::new();
        let queue_icon = gtk::Image::from_icon_name("musa-list-music");
        queue_icon.set_pixel_size(20);
        queue_icon.add_css_class("stroke-icon");
        queue_button.set_child(Some(&queue_icon));
        queue_button.set_tooltip_text(Some("Cola"));
        extras.append(&queue_button);

        let volume_group = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        volume_group.set_valign(gtk::Align::Center);
        volume_group.set_margin_start(12);
        let volume_button = gtk::Button::new();
        let volume_icon = gtk::Image::from_icon_name("musa-volume-2");
        volume_icon.set_pixel_size(20);
        volume_icon.add_css_class("stroke-icon");
        volume_button.set_child(Some(&volume_icon));
        volume_button.set_tooltip_text(Some("Silenciar"));
        volume_group.append(&volume_button);
        let volume = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
        volume.set_draw_value(false);
        volume.set_size_request(96, -1);
        volume.set_value(initial_volume as f64);
        volume.add_css_class("volume");
        volume_group.append(&volume);
        extras.append(&volume_group);

        root.set_start_widget(Some(&left));
        root.set_center_widget(Some(&controls));
        root.set_end_widget(Some(&extras));

        let bar = Self {
            root,
            cover,
            title,
            artist,
            play,
            play_icon,
            previous,
            next,
            shuffle,
            shuffle_icon,
            repeat_icon,
            repeat_button,
            heart,
            volume,
            volume_button,
            volume_icon,
            lyrics: lyrics_button,
            queue: queue_button,
            elapsed,
            total,
            progress,
            last_cover_key: Rc::new(RefCell::new(String::new())),
            last_progress_track: Rc::new(RefCell::new(String::new())),
            last_progress_ms: Rc::new(Cell::new(u64::MAX)),
            last_duration_ms: Rc::new(Cell::new(u64::MAX)),
            last_volume: Rc::new(Cell::new(initial_volume.max(1))),
            left,
            controls,
            extras,
            center_width: Rc::new(Cell::new(360)),
            progress_source: Rc::new(RefCell::new(None)),
            shuffle_guard: Rc::new(Cell::new(false)),
            heart_guard: Rc::new(Cell::new(false)),
            volume_guard: Rc::new(Cell::new(false)),
            progress_guard: Rc::new(Cell::new(false)),
        };
        bar.connect(actions);
        bar.refresh(actions);
        bar
    }

    fn connect(&self, actions: &Actions) {
        let actions_play = actions.clone();
        self.play
            .connect_clicked(move |_| actions_play.toggle_play());

        let actions_prev = actions.clone();
        self.previous
            .connect_clicked(move |_| actions_prev.previous());

        let actions_next = actions.clone();
        self.next.connect_clicked(move |_| actions_next.next());

        let actions_shuffle = actions.clone();
        let guard = self.shuffle_guard.clone();
        self.shuffle.connect_toggled(move |_| {
            if guard.get() {
                return;
            }
            actions_shuffle.toggle_shuffle();
        });

        let actions_repeat = actions.clone();
        self.repeat_button
            .connect_clicked(move |_| actions_repeat.cycle_repeat());

        let actions_heart = actions.clone();
        let guard = self.heart_guard.clone();
        self.heart.connect_toggled(move |heart| {
            if guard.get() {
                return;
            }
            actions_heart.toggle_favorite_track();
            crate::ui::widgets::apply_favorite_class(
                heart.upcast_ref::<gtk::Button>(),
                heart.is_active(),
            );
            heart.set_tooltip_text(Some(if heart.is_active() {
                "Quitar de favoritos"
            } else {
                "Añadir a favoritos"
            }));
        });

        let actions_volume = actions.clone();
        let volume_guard = self.volume_guard.clone();
        let last_volume = self.last_volume.clone();
        self.volume.connect_value_changed(move |scale| {
            if volume_guard.get() {
                return;
            }
            let volume = scale.value().round().clamp(0.0, 100.0) as u8;
            if volume > 0 {
                last_volume.set(volume);
            }
            actions_volume.set_volume(volume);
        });

        let actions_mute = actions.clone();
        let last_volume = self.last_volume.clone();
        self.volume_button.connect_clicked(move |_| {
            let current = actions_mute.player.borrow().state.volume;
            if current > 0 {
                last_volume.set(current);
                actions_mute.set_volume(0);
            } else {
                actions_mute.set_volume(last_volume.get().clamp(1, 100));
            }
        });

        let actions_seek = actions.clone();
        let guard = self.progress_guard.clone();
        self.progress.connect_change_value(move |_scale, _, value| {
            if guard.get() {
                return gtk::glib::Propagation::Stop;
            }
            actions_seek.seek(value.max(0.0) as u64);
            gtk::glib::Propagation::Proceed
        });

        let actions_lyrics = actions.clone();
        self.lyrics
            .connect_clicked(move |_| actions_lyrics.open_lyrics());

        let actions_queue = actions.clone();
        self.queue
            .connect_clicked(move |button| open_queue_popover(&actions_queue, button));
    }

    #[allow(deprecated)]
    pub fn refresh(&self, actions: &Actions) {
        let player = actions.player.borrow();
        let state = &player.state;
        let position = player.position_ms();
        let track = state.track.clone();

        if let Some(track) = &track {
            self.title.set_text(&track.title);
            self.artist
                .set_text(&format!("{} · {}", track.artist_name, track.album_title));
        } else {
            self.title.set_text("Nada en reproducción");
            self.artist.set_text("Elige un álbum o una playlist");
        }

        // El centro es flexible: 360..560. Se calcula con el ancho disponible
        // y el tamaño natural de las zonas laterales, de modo que el texto no
        // imponga un mínimo a la ventana.
        if self.root.width() > 0 {
            let left_natural = self.left.measure(gtk::Orientation::Horizontal, -1).1;
            let right_natural = self.extras.measure(gtk::Orientation::Horizontal, -1).1;
            let available = self.root.width() - 40;
            let center = (available - 48 - left_natural - right_natural).clamp(360, 560);
            if self.center_width.get() != center {
                self.center_width.set(center);
                self.controls.set_size_request(center, -1);
            }
        }

        let icon = if state.playing {
            "musa-pause"
        } else {
            "musa-play"
        };
        if self.play_icon.icon_name().as_deref() != Some(icon) {
            self.play_icon.set_icon_name(Some(icon));
        }

        self.shuffle_guard.set(true);
        self.shuffle.set_active(state.shuffle);
        self.shuffle_guard.set(false);
        self.shuffle.set_tooltip_text(Some(if state.shuffle {
            "Aleatorio activado"
        } else {
            "Aleatorio"
        }));
        let shuffle_icon = if state.shuffle {
            "musa-shuffle-active"
        } else {
            "musa-shuffle"
        };
        if self.shuffle_icon.icon_name().as_deref() != Some(shuffle_icon) {
            self.shuffle_icon.set_icon_name(Some(shuffle_icon));
        }

        let repeat_icon = repeat_icon(state.repeat);
        let repeat_icon = if state.repeat == RepeatMode::Off {
            repeat_icon.to_string()
        } else {
            format!("{repeat_icon}-active")
        };
        if self.repeat_icon.icon_name().as_deref() != Some(repeat_icon.as_str()) {
            self.repeat_icon.set_icon_name(Some(&repeat_icon));
        }
        self.repeat_button
            .set_tooltip_text(Some(state.repeat.label()));
        if state.repeat == RepeatMode::Off {
            self.repeat_button.remove_css_class("accent-icon");
        } else {
            self.repeat_button.add_css_class("accent-icon");
        }

        let favorite = track.as_ref().is_some_and(|t| t.favorite);
        self.heart_guard.set(true);
        self.heart.set_active(favorite);
        self.heart_guard.set(false);
        self.heart.set_sensitive(track.is_some());
        super::widgets::set_heart_toggle(&self.heart, favorite, 20);
        self.heart.set_tooltip_text(Some(if favorite {
            "Quitar de favoritos"
        } else {
            "Añadir a favoritos"
        }));
        self.volume_guard.set(true);
        self.volume.set_value(state.volume as f64);
        self.volume_guard.set(false);
        let volume_icon = match state.volume {
            0 => "musa-volume-x",
            1..=33 => "musa-volume-1",
            _ => "musa-volume-2",
        };
        if self.volume_icon.icon_name().as_deref() != Some(volume_icon) {
            self.volume_icon.set_icon_name(Some(volume_icon));
        }
        self.volume_button
            .set_tooltip_text(Some(if state.volume == 0 {
                "Activar sonido"
            } else {
                "Silenciar"
            }));

        let playing = state.playing;
        let duration_ms = track.as_ref().map(|t| t.duration_ms).unwrap_or(0);
        let track_key = track
            .as_ref()
            .map(|t| format!("{}:{}", t.id, duration_ms))
            .unwrap_or_default();
        self.progress_guard.set(true);
        if self.last_duration_ms.get() != duration_ms {
            self.last_duration_ms.set(duration_ms);
            self.progress.set_range(0.0, duration_ms.max(1) as f64);
        }
        self.progress_guard.set(false);
        self.total.set_text(&duration(duration_ms));
        let track_changed = *self.last_progress_track.borrow() != track_key;
        if track_changed {
            *self.last_progress_track.borrow_mut() = track_key;
            self.last_progress_ms.set(u64::MAX);
        }
        if track_changed || !playing {
            self.set_progress(position, duration_ms);
        }
        self.sync_progress_loop(actions, playing);

        let cover_key = track
            .as_ref()
            .map(|t| format!("{}:{}", t.id, self.cover.scale_factor()))
            .unwrap_or_default();
        if *self.last_cover_key.borrow() != cover_key {
            *self.last_cover_key.borrow_mut() = cover_key;
            if let Some(track) = &track {
                let cover = self.cover.clone();
                let path = track.cover_path.clone();
                let scale = cover.scale_factor().max(1) as u32;
                actions
                    .covers
                    .request(std::path::Path::new(&path), 56 * scale, move |pixbuf| {
                        if let Some(pixbuf) = pixbuf {
                            let texture = gtk::gdk::Texture::for_pixbuf(&pixbuf);
                            cover.set_paintable(Some(&texture));
                        }
                    });
            } else {
                self.cover.set_paintable(None::<&gtk::gdk::Texture>);
            }
        }
    }

    /// Actualiza solo la posición mientras suena. Se llama desde un timeout de
    /// 500 ms que solo existe durante la reproducción.
    fn refresh_progress(&self, actions: &Actions) {
        let (position, duration_ms) = {
            let player = actions.player.borrow();
            if !player.state.playing {
                return;
            }
            let position = player.position_ms();
            let duration_ms = player
                .state
                .track
                .as_ref()
                .map(|t| t.duration_ms)
                .unwrap_or(0);
            (position, duration_ms)
        };
        self.set_progress(position, duration_ms);
    }

    fn set_progress(&self, position: u64, duration_ms: u64) {
        let position = if duration_ms > 0 {
            position.min(duration_ms)
        } else {
            position
        };
        if self.last_progress_ms.get() == position {
            return;
        }
        self.last_progress_ms.set(position);
        self.progress_guard.set(true);
        self.progress.set_value(if duration_ms > 0 {
            position as f64
        } else {
            0.0
        });
        self.progress_guard.set(false);
        self.elapsed.set_text(&duration(position));
    }

    fn sync_progress_loop(&self, actions: &Actions, playing: bool) {
        if playing {
            if self.progress_source.borrow().is_some() {
                return;
            }
            let bar = self.clone();
            let actions = actions.clone();
            let id = gtk::glib::timeout_add_local(Duration::from_millis(500), move || {
                if actions.player.borrow().state.playing {
                    bar.refresh_progress(&actions);
                }
                gtk::glib::ControlFlow::Continue
            });
            *self.progress_source.borrow_mut() = Some(id);
        } else if let Some(id) = self.progress_source.borrow_mut().take() {
            id.remove();
        }
    }
}

impl Actions {
    #[allow(deprecated)]
    pub fn open_lyrics(&self) {
        let Some(track) = self.current_track() else {
            self.toast("No hay ninguna canción en reproducción");
            return;
        };
        let window = adw::Window::new();
        window.set_title(Some(&format!("Letras · {}", track.title)));
        window.set_transient_for(Some(&self.window));
        window.set_modal(false);
        window.set_default_width(640);
        window.set_default_height(760);
        window.add_css_class("lyrics-window");

        let header = adw::HeaderBar::new();
        let window_title = adw::WindowTitle::new("Letras", &track.title);
        header.set_title_widget(Some(&window_title));

        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        body.set_margin_top(12);
        body.set_margin_bottom(24);
        body.set_margin_start(36);
        body.set_margin_end(36);
        let spinner = gtk::Spinner::new();
        spinner.start();
        spinner.set_size_request(28, 28);
        spinner.set_halign(gtk::Align::Center);
        spinner.set_margin_top(40);
        body.append(&spinner);
        let scrolled = gtk::ScrolledWindow::new();
        scrolled.set_vexpand(true);
        scrolled.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scrolled.set_child(Some(&body));
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&scrolled));
        window.set_content(Some(&toolbar));
        window.present();

        let labels: Rc<RefCell<Vec<(u64, gtk::Label)>>> = Rc::new(RefCell::new(Vec::new()));
        let current: Rc<Cell<usize>> = Rc::new(Cell::new(usize::MAX));
        let body_request = body.clone();
        let actions = self.clone();
        let window_weak = window.downgrade();
        let labels_request = labels.clone();
        let current_request = current.clone();
        let song_id = track.id.clone();
        self.client.call(
            musa_protocol::cmd::LYRICS_GET,
            serde_json::json!({"id": song_id}),
            move |result| {
                if body_request.parent().is_none() {
                    return;
                }
                clear_children(&body_request);
                match result.and_then(|value| {
                    serde_json::from_value::<Lyrics>(value)
                        .map_err(|e| musa_protocol::ProtocolError::new("bad_json", e.to_string()))
                }) {
                    Ok(lyrics) if !lyrics.lines.is_empty() => {
                        let title = gtk::Label::new(Some(&lyrics.source.to_uppercase()));
                        title.add_css_class("eyebrow");
                        title.set_xalign(0.0);
                        title.set_margin_bottom(16);
                        body_request.append(&title);
                        let mut collected = Vec::new();
                        for line in &lyrics.lines {
                            let label = gtk::Label::new(Some(&line.text));
                            label.set_xalign(0.0);
                            label.set_wrap(true);
                            label.set_margin_top(9);
                            label.set_margin_bottom(9);
                            label.add_css_class("lyric-line");
                            body_request.append(&label);
                            collected.push((line.time_ms, label));
                        }
                        *labels_request.borrow_mut() = collected;
                        current_request.set(usize::MAX);
                    }
                    Ok(_) => {
                        let empty =
                            gtk::Label::new(Some("No hay letras disponibles para esta canción."));
                        empty.add_css_class("empty-state");
                        empty.set_wrap(true);
                        body_request.append(&empty);
                    }
                    Err(err) => {
                        let _ = err;
                        let error = gtk::Label::new(Some(
                            "No se pudieron cargar las letras. Inténtalo de nuevo.",
                        ));
                        error.add_css_class("empty-state");
                        error.set_wrap(true);
                        body_request.append(&error);
                    }
                }
            },
        );

        let actions_tick = actions.clone();
        let labels_tick = labels.clone();
        let current_tick = current.clone();
        let scrolled_tick = scrolled.clone();
        gtk::glib::timeout_add_local(Duration::from_millis(250), move || {
            if window_weak.upgrade().is_none() {
                return gtk::glib::ControlFlow::Break;
            }
            let Some(window) = window_weak.upgrade() else {
                return gtk::glib::ControlFlow::Break;
            };
            if !window.is_visible() {
                return gtk::glib::ControlFlow::Break;
            }
            let position = actions_tick.player.borrow().position_ms();
            let labels = labels_tick.borrow();
            if labels.is_empty() {
                return gtk::glib::ControlFlow::Continue;
            }
            let mut index = 0;
            for (i, (time, _)) in labels.iter().enumerate() {
                if *time <= position {
                    index = i;
                }
            }
            if index != current_tick.get() {
                if let Some((_, old)) = labels.get(current_tick.get()) {
                    old.remove_css_class("lyric-current");
                }
                if let Some((_, new)) = labels.get(index) {
                    new.add_css_class("lyric-current");
                    let y = new.allocation().y() as f64;
                    let adjustment = scrolled_tick.vadjustment();
                    adjustment.set_value((y - 120.0).max(0.0));
                }
                current_tick.set(index);
            }
            gtk::glib::ControlFlow::Continue
        });
    }

    pub fn jump_queue(&self, index: usize) {
        self.client.call(
            musa_protocol::cmd::QUEUE_JUMP,
            serde_json::json!({"index": index}),
            |_| {},
        );
    }
}

fn open_queue_popover(actions: &Actions, button: &gtk::Button) {
    let popover = gtk::Popover::new();
    popover.set_parent(button);
    popover.set_has_arrow(false);
    popover.set_position(gtk::PositionType::Top);
    popover.add_css_class("queue-popover");

    let box_ = gtk::Box::new(gtk::Orientation::Vertical, 8);
    box_.set_margin_top(12);
    box_.set_margin_bottom(12);
    box_.set_margin_start(12);
    box_.set_margin_end(12);
    let title = gtk::Label::new(Some("Cola de reproducción"));
    title.set_xalign(0.0);
    title.add_css_class("h2");
    box_.append(&title);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("queue-list");
    let state = actions.player.borrow().state.clone();
    if state.queue.is_empty() {
        let empty = gtk::Label::new(Some("La cola está vacía"));
        empty.add_css_class("muted");
        box_.append(&empty);
    } else {
        let data = actions.data.borrow();
        for (index, id) in state.queue.iter().enumerate() {
            let Some(song) = data.song(id) else { continue };
            let row = gtk::ListBoxRow::new();
            let labels = gtk::Box::new(gtk::Orientation::Vertical, 2);
            labels.set_margin_top(6);
            labels.set_margin_bottom(6);
            labels.set_margin_start(8);
            labels.set_margin_end(8);
            let name = gtk::Label::new(Some(&format!("{}. {}", index + 1, song.title)));
            name.set_xalign(0.0);
            name.set_ellipsize(gtk::pango::EllipsizeMode::End);
            name.add_css_class("song-title");
            let artist = gtk::Label::new(Some(&song.artist_name));
            artist.set_xalign(0.0);
            artist.add_css_class("song-meta");
            labels.append(&name);
            labels.append(&artist);
            row.set_child(Some(&labels));
            if index == state.index {
                row.add_css_class("queue-current");
            }
            list.append(&row);
        }
        drop(data);
        let actions_jump = actions.clone();
        let popover_close = popover.clone();
        list.connect_row_activated(move |_, row| {
            actions_jump.jump_queue(row.index() as usize);
            popover_close.popdown();
        });
        let scrolled = gtk::ScrolledWindow::new();
        scrolled.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scrolled.set_min_content_height(320);
        scrolled.set_child(Some(&list));
        box_.append(&scrolled);
    }
    popover.set_child(Some(&box_));
    popover.popup();
}

fn clear_children(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}
