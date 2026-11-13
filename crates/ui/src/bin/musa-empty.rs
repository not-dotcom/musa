//! Piso de memoria: una ventana GTK4/libadwaita vacía, sin biblioteca.
//! Sirve para comparar los objetivos de las instrucciones con esta máquina.
//!
//! `GSK_RENDERER=cairo musa-empty --seconds 8`

use std::time::{Duration, Instant};

use relm4::adw;
use relm4::gtk;
use relm4::gtk::prelude::*;

fn main() {
    let mut seconds = 8u64;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--seconds" => {
                if let Some(value) = args.next().and_then(|v| v.parse().ok()) {
                    seconds = value;
                }
            }
            "--help" | "-h" => {
                println!("musa-empty [--seconds N]");
                return;
            }
            _ => {}
        }
    }
    let started = Instant::now();
    let app = adw::Application::builder()
        .application_id("io.github.juliogarcia.Musa.Empty")
        .build();
    app.connect_activate(move |app| {
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Musa (medición vacía)")
            .default_width(1280)
            .default_height(820)
            .build();
        window.present();
        let app = app.clone();
        gtk::glib::timeout_add_local(Duration::from_secs(seconds), move || {
            let pss = pss_bytes();
            let report = serde_json::json!({
                "mode": "empty",
                "pss_bytes": pss,
                "pss_mb": (pss as f64 / (1024.0 * 1024.0) * 10.0).round() / 10.0,
                "seconds": started.elapsed().as_secs(),
                "gsk_renderer": std::env::var("GSK_RENDERER").unwrap_or_else(|_| "auto".into()),
                "pid": std::process::id(),
            });
            println!(
                "MEDICION {}",
                serde_json::to_string(&report).unwrap_or_default()
            );
            app.quit();
            gtk::glib::ControlFlow::Break
        });
    });
    app.run_with_args(&[std::env::args()
        .next()
        .unwrap_or_else(|| "musa-empty".into())]);
}

fn pss_bytes() -> u64 {
    let Ok(text) = std::fs::read_to_string("/proc/self/smaps_rollup") else {
        return 0;
    };
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Pss:") {
            if let Some(kb) = rest.trim().split_whitespace().next() {
                if let Ok(kb) = kb.parse::<u64>() {
                    return kb * 1024;
                }
            }
        }
    }
    0
}
