//! Modo clásico: reproductor con estética inspirada en Winamp Classic.
//!
//! Es una vista más sobre el mismo estado que ya mantiene la interfaz
//! (`Actions::player`, `Actions::data`): no habla con el daemon por su cuenta
//! y no añade protocolo. El repintado lo empuja el ticker de `mod.rs`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use relm4::gtk;
use relm4::gtk::prelude::*;

use super::Actions;
use crate::data::duration;

const PLAYER_WIDTH: i32 = 384;
const SHADE_HEIGHT: i32 = 34;

/// Estado del modo clásico dentro de `Actions`.
#[derive(Default)]
pub struct ClassicState {
    open: Option<Classic>,
}

struct Classic {
    window: gtk::Window,
    body: gtk::Box,
    shade_row: gtk::Box,
    shade_icon: gtk::Image,
    time: gtk::Label,
    shade_time: gtk::Label,
    shade_title: gtk::Label,
    marquee: gtk::Label,
    spectrum: gtk::DrawingArea,
    spectrum_level: Rc<Cell<f64>>,
    spectrum_phase: Rc<Cell<f64>>,
    progress: gtk::Scale,
    progress_guard: Rc<Cell<bool>>,
    /// Momento del último cambio hecho por el usuario en la barra de posición.
    /// Mientras esté reciente no se reescribe el slider: si no, el ticker lo
    /// devuelve al valor del motor en mitad del arrastre.
    seeking_at: Rc<Cell<Option<Instant>>>,
    /// Posición pendiente de confirmar mientras el arrastre se asienta. Evita
    /// enviar un `seek` por cada píxel de movimiento (lo que trababa el audio).
    pending_seek: Rc<Cell<Option<u64>>>,
    seek_source: Rc<RefCell<Option<gtk::glib::SourceId>>>,
    play_icon: gtk::Label,
    shuffle_button: gtk::Button,
    repeat_button: gtk::Button,
    volume: gtk::Scale,
    volume_guard: Rc<Cell<bool>>,
    info: gtk::Label,
    shade: Rc<Cell<bool>>,
    /// Panel de lista de reproducción, apilado dentro de la misma ventana para
    /// que se mueva con el reproductor (en Wayland no se puede posicionar una
    /// ventana independiente justo debajo de otra).
    queue_panel: gtk::Box,
    queue_list: gtk::ListBox,
    queue_key: Rc<RefCell<String>>,
}

pub fn install_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(include_str!("classic.css"));
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

impl Actions {
    /// Entra o sale del modo clásico. Son dos modos excluyentes: nunca queda
    /// la aplicación sin ninguna ventana visible.
    pub fn toggle_classic(&self) {
        if let Some(classic) = self.classic.borrow().open.as_ref() {
            if classic.window.is_visible() {
                classic.window.set_visible(false);
                self.window.set_visible(true);
                self.window.present();
            } else {
                classic.window.present();
                self.window.set_visible(false);
            }
            return;
        }
        let classic = Classic::open(self);
        self.classic.borrow_mut().open = Some(classic);
    }

    /// Vuelve a la interfaz principal: oculta el clásico y muestra Musa.
    pub fn back_to_main(&self) {
        if let Some(classic) = self.classic.borrow().open.as_ref() {
            classic.window.set_visible(false);
        }
        self.window.set_visible(true);
        self.window.present();
    }

    /// Ajusta el reproductor ya abierto. Se usa en las pruebas de arranque
    /// (`MUSA_OPEN_CLASSIC=shade|queue`).
    pub fn classic_shade(&self) {
        if let Some(classic) = self.classic.borrow().open.as_ref() {
            classic.shade.set(true);
            classic.set_shade(true);
        }
    }

    pub fn classic_show_queue(&self) {
        if let Some(classic) = self.classic.borrow().open.as_ref() {
            classic.queue_panel.set_visible(true);
        }
    }
}

/// Repinta el modo clásico si está abierto. Lo llama el ticker existente.
pub fn refresh(actions: &Actions) {
    let state = actions.classic.borrow();
    let Some(classic) = state.open.as_ref() else {
        return;
    };
    if classic.window.is_visible() {
        classic.refresh(actions);
    }
}

impl Classic {
    fn open(actions: &Actions) -> Self {
        let window = gtk::Window::new();
        window.set_title(Some("Musa Classic"));
        window.set_resizable(false);
        window.add_css_class("classic-window");
        window.set_hide_on_close(true);

        let shade = Rc::new(Cell::new(false));

        // Barra de título propia: GTK la usa como decoración del cliente, así
        // que la ventana se arrastra con ella como cualquier otra.
        let titlebar = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        titlebar.add_css_class("classic-titlebar");
        titlebar.set_valign(gtk::Align::Center);
        let brand = gtk::Label::new(Some("MUSA CLASSIC"));
        brand.add_css_class("classic-titlebar-label");
        brand.set_hexpand(true);
        brand.set_xalign(0.0);
        titlebar.append(&brand);

        let shade_button = gtk::Button::new();
        let shade_icon = gtk::Image::from_icon_name("go-up-symbolic");
        shade_icon.set_pixel_size(10);
        shade_button.set_child(Some(&shade_icon));
        shade_button.set_tooltip_text(Some("Modo ventana"));
        titlebar.append(&shade_button);

        let back_button = gtk::Button::new();
        let back_icon = gtk::Image::from_icon_name("go-previous-symbolic");
        back_icon.set_pixel_size(10);
        back_button.set_child(Some(&back_icon));
        back_button.set_tooltip_text(Some("Volver a Musa"));
        titlebar.append(&back_button);
        window.set_titlebar(Some(&titlebar));

        // Cuerpo completo.
        let body = gtk::Box::new(gtk::Orientation::Vertical, 6);
        body.add_css_class("classic-panel");

        let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        top.set_height_request(46);

        let time = gtk::Label::new(Some("0:00"));
        time.add_css_class("classic-display");
        time.set_valign(gtk::Align::Center);
        top.append(&time);

        let marquee_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        marquee_box.add_css_class("classic-marquee");
        marquee_box.set_hexpand(true);
        marquee_box.set_valign(gtk::Align::Center);
        marquee_box.set_overflow(gtk::Overflow::Hidden);
        let marquee = gtk::Label::new(Some("Nada en reproducción"));
        marquee.set_xalign(0.0);
        marquee.set_single_line_mode(true);
        // Ancho fijo: la marquesina cambia de texto en cada tick y, si su
        // ancho natural variara, la fila entera se recolocaría.
        marquee.set_width_chars(18);
        marquee.set_max_width_chars(18);
        marquee.set_ellipsize(gtk::pango::EllipsizeMode::End);
        marquee_box.append(&marquee);
        top.append(&marquee_box);

        let spectrum_level = Rc::new(Cell::new(0.12));
        let spectrum_phase = Rc::new(Cell::new(0.0));
        let spectrum = gtk::DrawingArea::new();
        spectrum.add_css_class("classic-spectrum");
        spectrum.set_valign(gtk::Align::Center);
        spectrum.set_height_request(46);
        spectrum.set_width_request(96);
        // Sin `hexpand`: si compitiera por el espacio sobrante con la
        // marquesina, el reparto cambiaría en cada tick.
        spectrum.set_hexpand(false);
        {
            let level = spectrum_level.clone();
            let phase = spectrum_phase.clone();
            spectrum.set_draw_func(move |_, cr, width, height| {
                draw_spectrum(cr, width, height, level.get(), phase.get());
            });
        }
        top.append(&spectrum);
        body.append(&top);

        let progress = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 1.0);
        progress.set_draw_value(false);
        progress.add_css_class("classic-progress");
        body.append(&progress);

        let transport = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let previous = button("◀◀", "Anterior");
        let play = gtk::Button::new();
        let play_icon = gtk::Label::new(Some("▶"));
        play.set_child(Some(&play_icon));
        play.add_css_class("classic-button");
        play.set_tooltip_text(Some("Reproducir o pausar"));
        let next = button("▶▶", "Siguiente");
        let stop = button("■", "Detener");
        let shuffle_button = button("SHUFFLE", "Aleatorio");
        let repeat_button = button("REP", "Repetición");
        let queue_button = button("PL", "Cola de reproducción");
        for button in [
            &previous,
            &play,
            &next,
            &stop,
            &shuffle_button,
            &repeat_button,
            &queue_button,
        ] {
            transport.append(button);
        }
        body.append(&transport);

        let bottom = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let volume = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
        volume.set_draw_value(false);
        volume.set_hexpand(true);
        volume.add_css_class("classic-volume");
        volume.set_value(actions.player.borrow().state.volume as f64);
        bottom.append(&volume);
        let info = gtk::Label::new(Some("—"));
        info.add_css_class("classic-info");
        info.set_xalign(1.0);
        // `—` y `3/12 · 100%` miden distinto; con ancho fijo no arrastran al
        // volumen ni recolocan la fila.
        info.set_width_chars(12);
        info.set_max_width_chars(12);
        bottom.append(&info);
        body.append(&bottom);

        // Franja del windowshade, oculta hasta colapsar.
        let shade_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        shade_row.add_css_class("classic-shade-panel");
        shade_row.set_height_request(SHADE_HEIGHT - 8);
        let shade_time = gtk::Label::new(Some("0:00"));
        shade_time.add_css_class("classic-shade-time");
        shade_row.append(&shade_time);
        let shade_title = gtk::Label::new(Some("Nada en reproducción"));
        shade_title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        shade_title.set_hexpand(true);
        shade_title.set_xalign(0.0);
        shade_title.add_css_class("classic-shade-title");
        shade_row.append(&shade_title);
        shade_row.set_visible(false);

        // Panel de lista, dentro de la misma ventana.
        let queue_panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
        queue_panel.add_css_class("classic-queue-panel");
        queue_panel.set_margin_top(4);

        let queue_titlebar = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        queue_titlebar.add_css_class("classic-titlebar");
        let queue_brand = gtk::Label::new(Some("PLAYLIST"));
        queue_brand.add_css_class("classic-titlebar-label");
        queue_brand.set_xalign(0.0);
        queue_brand.set_hexpand(true);
        queue_titlebar.append(&queue_brand);
        let queue_close = gtk::Button::new();
        let queue_close_icon = gtk::Image::from_icon_name("window-close-symbolic");
        queue_close_icon.set_pixel_size(10);
        queue_close.set_child(Some(&queue_close_icon));
        queue_close.set_tooltip_text(Some("Cerrar lista"));
        queue_titlebar.append(&queue_close);
        queue_panel.append(&queue_titlebar);

        let queue_list = gtk::ListBox::new();
        queue_list.set_selection_mode(gtk::SelectionMode::None);
        queue_list.add_css_class("classic-queue");
        queue_list.set_valign(gtk::Align::Start);
        let queue_scrolled = gtk::ScrolledWindow::new();
        queue_scrolled.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        queue_scrolled.set_min_content_height(320);
        queue_scrolled.set_propagate_natural_height(true);
        queue_scrolled.set_max_content_height(320);
        queue_scrolled.set_child(Some(&queue_list));
        queue_panel.append(&queue_scrolled);
        queue_panel.set_visible(false);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&body);
        content.append(&shade_row);
        content.append(&queue_panel);
        content.set_margin_top(4);
        content.set_margin_bottom(4);
        content.set_margin_start(4);
        content.set_margin_end(4);
        window.set_child(Some(&content));
        window.set_default_width(PLAYER_WIDTH);

        let classic = Self {
            window,
            body,
            shade_row,
            shade_icon,
            time,
            shade_time,
            shade_title,
            marquee,
            spectrum,
            spectrum_level,
            spectrum_phase,
            progress,
            progress_guard: Rc::new(Cell::new(false)),
            seeking_at: Rc::new(Cell::new(None)),
            pending_seek: Rc::new(Cell::new(None)),
            seek_source: Rc::new(RefCell::new(None)),
            play_icon,
            shuffle_button,
            repeat_button,
            volume,
            volume_guard: Rc::new(Cell::new(false)),
            info,
            shade,
            queue_panel,
            queue_list,
            queue_key: Rc::new(RefCell::new(String::new())),
        };
        classic.connect(
            actions,
            &play,
            &previous,
            &next,
            &stop,
            &queue_button,
            &queue_close,
        );
        classic.connect_titlebar(actions, &shade_button, &back_button);
        classic.refresh(actions);
        classic.window.present();
        actions.window.set_visible(false);
        classic
    }

    fn connect(
        &self,
        actions: &Actions,
        play: &gtk::Button,
        previous: &gtk::Button,
        next: &gtk::Button,
        stop: &gtk::Button,
        queue_button: &gtk::Button,
        queue_close: &gtk::Button,
    ) {
        let a = actions.clone();
        play.connect_clicked(move |_| a.toggle_play());

        let a = actions.clone();
        previous.connect_clicked(move |_| a.previous());

        let a = actions.clone();
        next.connect_clicked(move |_| a.next());

        let a = actions.clone();
        stop.connect_clicked(move |_| {
            a.client
                .call_simple(musa_protocol::cmd::PLAYER_PAUSE, |_| {});
            a.seek(0);
        });

        let a = actions.clone();
        self.shuffle_button
            .connect_clicked(move |_| a.toggle_shuffle());

        let a = actions.clone();
        self.repeat_button
            .connect_clicked(move |_| a.cycle_repeat());

        let queue_panel = self.queue_panel.clone();
        queue_button.connect_clicked(move |_| {
            queue_panel.set_visible(!queue_panel.is_visible());
        });

        let queue_panel = self.queue_panel.clone();
        queue_close.connect_clicked(move |_| queue_panel.set_visible(false));

        let a = actions.clone();
        let guard = self.progress_guard.clone();
        let seeking_at = self.seeking_at.clone();
        let pending_seek = self.pending_seek.clone();
        let seek_source = self.seek_source.clone();
        self.progress.connect_change_value(move |_scale, _, value| {
            if guard.get() {
                return gtk::glib::Propagation::Stop;
            }
            // Arrastrar no debe hacer que el ticker devuelva el slider: se
            // recuerda el instante del cambio y `refresh` no lo pisa mientras
            // siga siendo reciente.
            seeking_at.set(Some(Instant::now()));
            pending_seek.set(Some(value.max(0.0) as u64));
            if let Some(source) = seek_source.borrow_mut().take() {
                source.remove();
            }
            let a = a.clone();
            let pending_seek = pending_seek.clone();
            let seek_source_inner = seek_source.clone();
            let source = gtk::glib::timeout_add_local(Duration::from_millis(180), move || {
                *seek_source_inner.borrow_mut() = None;
                if let Some(position_ms) = pending_seek.take() {
                    a.seek(position_ms);
                }
                gtk::glib::ControlFlow::Break
            });
            *seek_source.borrow_mut() = Some(source);
            gtk::glib::Propagation::Proceed
        });

        let a = actions.clone();
        let guard = self.volume_guard.clone();
        self.volume.connect_value_changed(move |scale| {
            if guard.get() {
                return;
            }
            a.set_volume(scale.value().round().clamp(0.0, 100.0) as u8);
        });

        let a = actions.clone();
        self.queue_list
            .connect_row_activated(move |_, row| a.jump_queue(row.index() as usize));
    }

    fn connect_titlebar(
        &self,
        actions: &Actions,
        shade_button: &gtk::Button,
        back_button: &gtk::Button,
    ) {
        let a = actions.clone();
        back_button.connect_clicked(move |_| a.back_to_main());

        // Sin la ventana principal visible, cerrar el clásico dejaría la app
        // sin ventanas. En su lugar se vuelve a la interfaz principal.
        let a = actions.clone();
        self.window.connect_close_request(move |window| {
            window.set_visible(false);
            a.back_to_main();
            gtk::glib::Propagation::Stop
        });

        let shade = self.shade.clone();
        let body = self.body.clone();
        let shade_row = self.shade_row.clone();
        let queue_panel = self.queue_panel.clone();
        let shade_icon = self.shade_icon.clone();
        shade_button.connect_clicked(move |_| {
            let collapsed = !shade.get();
            shade.set(collapsed);
            apply_shade(&body, &shade_row, &queue_panel, &shade_icon, collapsed);
        });
    }

    /// Colapsa o expande el reproductor. Usado por el botón de la barra de
    /// título y por `MUSA_OPEN_CLASSIC=shade` al probar.
    fn set_shade(&self, collapsed: bool) {
        apply_shade(
            &self.body,
            &self.shade_row,
            &self.queue_panel,
            &self.shade_icon,
            collapsed,
        );
    }

    #[allow(deprecated)]
    fn refresh(&self, actions: &Actions) {
        let (track, position, playing, volume, shuffle, repeat, queue_len, index) = {
            let player = actions.player.borrow();
            let state = &player.state;
            (
                state.track.clone(),
                player.position_ms(),
                state.playing,
                state.volume,
                state.shuffle,
                state.repeat,
                state.queue.len(),
                state.index,
            )
        };

        let elapsed = duration(position);
        self.time.set_text(&elapsed);
        self.shade_time.set_text(&elapsed);
        if playing {
            self.time.remove_css_class("classic-display-paused");
        } else {
            self.time.add_css_class("classic-display-paused");
        }

        let label = match &track {
            Some(track) => format!("{} - {}", track.artist_name, track.title),
            None => "MUSA CLASSIC · nada en reproducción".to_string(),
        };
        self.shade_title.set_text(&label);
        self.marquee.set_text(&marquee_slice(&label, position));
        self.shade_title.set_tooltip_text(Some(&label));

        let glyph = if playing { "❚❚" } else { "▶" };
        if self.play_icon.label().as_str() != glyph {
            self.play_icon.set_label(glyph);
        }

        set_active(&self.shuffle_button, shuffle);
        set_active(
            &self.repeat_button,
            repeat != musa_protocol::RepeatMode::Off,
        );
        self.repeat_button.set_tooltip_text(Some(repeat.label()));

        self.volume_guard.set(true);
        self.volume.set_value(volume as f64);
        self.volume_guard.set(false);

        let duration_ms = track.as_ref().map(|t| t.duration_ms).unwrap_or(0);
        let seeking = self
            .seeking_at
            .get()
            .is_some_and(|at| at.elapsed().as_millis() < 1_200);
        if !seeking {
            self.progress_guard.set(true);
            if (self.progress.adjustment().upper() - duration_ms as f64).abs() > 1.0 {
                self.progress.set_range(0.0, duration_ms.max(1) as f64);
            }
            self.progress.set_value(position.min(duration_ms) as f64);
            self.progress_guard.set(false);
        }

        self.info.set_text(&if queue_len > 0 {
            format!("{}/{} · {}%", index + 1, queue_len, volume)
        } else {
            format!("— · {volume}%")
        });

        self.refresh_spectrum(position, playing, volume);
        self.refresh_queue(actions, queue_len, index);
    }

    fn refresh_spectrum(&self, position: u64, playing: bool, volume: u8) {
        // Decorativo: no hay acceso al audio (Widevine lo descifra dentro de
        // Chromium), así que las barras se derivan de la posición. Solo se
        // actualizan los parámetros y se pide un repintado; el dibujo en sí lo
        // hace un único `DrawingArea`, sin invalidar layout por cada barra.
        self.spectrum_level.set(if playing {
            0.35 + (volume as f64 / 100.0) * 0.65
        } else {
            0.12
        });
        self.spectrum_phase.set(position as f64 / 140.0);
        self.spectrum.queue_draw();
    }

    fn refresh_queue(&self, actions: &Actions, queue_len: usize, index: usize) {
        let key = {
            let state = actions.player.borrow();
            let track = state
                .state
                .track
                .as_ref()
                .map(|t| t.id.as_str())
                .unwrap_or("");
            format!("{queue_len}:{index}:{track}")
        };
        if *self.queue_key.borrow() == key {
            return;
        }
        *self.queue_key.borrow_mut() = key;

        while let Some(row) = self.queue_list.first_child() {
            self.queue_list.remove(&row);
        }
        let state = actions.player.borrow().state.clone();
        let data = actions.data.borrow();
        for (i, id) in state.queue.iter().enumerate() {
            let row = gtk::ListBoxRow::new();
            let labels = gtk::Box::new(gtk::Orientation::Vertical, 1);
            labels.set_margin_top(3);
            labels.set_margin_bottom(3);
            labels.set_margin_start(6);
            labels.set_margin_end(6);
            let title = gtk::Label::new(Some(&match data.song(id) {
                Some(song) => format!("{}. {}", i + 1, song.title),
                None => format!("{}. {id}", i + 1),
            }));
            title.set_xalign(0.0);
            title.set_ellipsize(gtk::pango::EllipsizeMode::End);
            title.add_css_class("classic-queue-title");
            labels.append(&title);
            if let Some(song) = data.song(id) {
                let artist = gtk::Label::new(Some(&song.artist_name));
                artist.set_xalign(0.0);
                artist.add_css_class("classic-queue-meta");
                labels.append(&artist);
            }
            row.set_child(Some(&labels));
            if i == state.index {
                row.add_css_class("classic-queue-current");
            }
            self.queue_list.append(&row);
        }
    }
}

fn button(label: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::with_label(label);
    button.add_css_class("classic-button");
    button.set_tooltip_text(Some(tooltip));
    button
}

fn set_active(button: &gtk::Button, active: bool) {
    if active {
        button.add_css_class("classic-active");
    } else {
        button.remove_css_class("classic-active");
    }
}

/// Dibuja el espectro decorativo en un único `DrawingArea`. El número de barras
/// se deriva del ancho disponible, así se adapta sin reencajar widgets por
/// barra en cada frame.
fn draw_spectrum(cr: &gtk::cairo::Context, width: i32, height: i32, level: f64, phase: f64) {
    if width <= 0 || height <= 0 {
        return;
    }
    let bar_w = 3.0;
    let gap = 2.0;
    let stride = bar_w + gap;
    let bars = ((width as f64 + gap) / stride).floor().max(1.0) as usize;
    let max_h = (height as f64 - 2.0).max(1.0);

    cr.set_source_rgb(0.0, 0.69, 0.0); // #00b000
    for i in 0..bars {
        let x = i as f64 * stride;
        let wave = ((phase + i as f64 * 0.9).sin() * 0.5 + 0.5).powf(1.7);
        let h = (3.0 + wave * max_h * level).min(max_h);
        cr.rectangle(x, height as f64 - h, bar_w, h);
    }
    let _ = cr.fill();
}

/// Recorta el texto como una marquesina: desplaza la ventana visible sobre la
/// cadena en proporción a la posición, con un hueco entre repeticiones.
/// Se hace sobre el texto y no con márgenes porque un margen cambiante altera
/// el ancho mínimo de la etiqueta y GTK redimensiona la ventana en cada tick.
fn marquee_slice(label: &str, position_ms: u64) -> String {
    const WINDOW: usize = 18;
    let chars: Vec<char> = label.chars().collect();
    if chars.len() <= WINDOW {
        return label.to_string();
    }
    let padded: Vec<char> = chars
        .iter()
        .copied()
        .chain(std::iter::repeat_n(' ', WINDOW))
        .collect();
    let offset = (position_ms / 220) as usize % padded.len();
    padded.iter().cycle().skip(offset).take(WINDOW).collect()
}

fn apply_shade(
    body: &gtk::Box,
    shade_row: &gtk::Box,
    queue_panel: &gtk::Box,
    shade_icon: &gtk::Image,
    collapsed: bool,
) {
    body.set_visible(!collapsed);
    shade_row.set_visible(collapsed);
    // La lista no cabe en la franja colapsada: se oculta y se recuerda.
    queue_panel.set_visible(false);
    shade_icon.set_icon_name(Some(if collapsed {
        "go-down-symbolic"
    } else {
        "go-up-symbolic"
    }));
}
