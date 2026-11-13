//! Modo de medición reproducible: PSS (no RSS) en escenarios concretos.
//!
//! Se activa con `musa --measure <library|scroll|playing|paused>`. El
//! resultado se imprime como una línea `MEDICION {json}` y se guarda en
//! `~/.cache/musa/mediciones/` para poder comparar ejecuciones.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use relm4::gtk;
use relm4::gtk::prelude::*;

use super::{Actions, PlayerBar};

pub fn pss_bytes() -> u64 {
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

pub fn start(actions: &Actions, bar: &PlayerBar, mode: &str) {
    let mode = mode.to_string();
    let is_scroll = mode == "scroll";
    let default_seconds = match mode.as_str() {
        "scroll" => 120,
        "playing" => 30,
        "paused" => 30,
        _ => 10,
    };
    let seconds: u64 = std::env::var("MUSA_MEASURE_SECONDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default_seconds);

    if mode == "scroll" {
        actions.select("albums");
    }
    if mode == "playing" || mode == "paused" {
        let songs = actions.data.borrow().favorite_songs();
        if !songs.is_empty() {
            actions.play_songs(&songs, 0);
        }
    }

    let actions_tick = actions.clone();
    let bar_tick = bar.clone();
    let started = Instant::now();
    let last_report = Rc::new(Cell::new(0u64));
    let paused = Rc::new(Cell::new(false));
    let direction = Rc::new(Cell::new(1.0f64));
    let renderer = std::env::var("GSK_RENDERER").unwrap_or_else(|_| "auto".into());

    gtk::glib::timeout_add_local(Duration::from_millis(1000), move || {
        bar_tick.refresh(&actions_tick);
        let elapsed = started.elapsed().as_secs();
        if mode == "paused" && elapsed >= 8 && !paused.get() {
            paused.set(true);
            actions_tick.toggle_play();
        }
        if elapsed > 0 && elapsed % 10 == 0 && last_report.get() != elapsed {
            last_report.set(elapsed);
            eprintln!(
                "musa: medición {} t={}s pss={:.1} MB",
                mode,
                elapsed,
                pss_bytes() as f64 / (1024.0 * 1024.0)
            );
        }
        if elapsed >= seconds {
            let pss = pss_bytes();
            let report = serde_json::json!({
                "mode": mode,
                "pss_bytes": pss,
                "pss_mb": (pss as f64 / (1024.0 * 1024.0) * 10.0).round() / 10.0,
                "seconds": elapsed,
                "gsk_renderer": renderer,
                "songs": actions_tick.data.borrow().snapshot.stats.songs,
                "albums": actions_tick.data.borrow().snapshot.stats.albums,
                "artists": actions_tick.data.borrow().snapshot.stats.artists,
                "queue": actions_tick.player.borrow().state.queue.len(),
                "track": actions_tick.player.borrow().state.track.as_ref().map(|t| t.title.clone()),
                "playing": actions_tick.player.borrow().state.playing,
                "cover_cache_bytes": actions_tick.covers.stats().2,
                "cover_hits": actions_tick.covers.stats().0,
                "cover_misses": actions_tick.covers.stats().1,
                "album_cards_built": super::widgets::album_cards_built(),
                "pid": std::process::id(),
                "when": SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
            });
            println!(
                "MEDICION {}",
                serde_json::to_string(&report).unwrap_or_default()
            );
            save_report(&mode, &report);
            actions_tick.window.close();
            gtk::glib::ControlFlow::Break
        } else {
            gtk::glib::ControlFlow::Continue
        }
    });

    if is_scroll {
        let actions_scroll = actions.clone();
        let direction = direction.clone();
        gtk::glib::timeout_add_local(Duration::from_millis(50), move || {
            let Some(adjustment) = actions_scroll.albums_adjustment.borrow().as_ref().cloned()
            else {
                return gtk::glib::ControlFlow::Continue;
            };
            let lower = adjustment.lower();
            let upper = (adjustment.upper() - adjustment.page_size()).max(lower);
            if upper <= lower {
                return gtk::glib::ControlFlow::Continue;
            }
            let mut value = adjustment.value() + 260.0 * direction.get();
            if value >= upper {
                value = upper;
                direction.set(-1.0);
            } else if value <= lower {
                value = lower;
                direction.set(1.0);
            }
            adjustment.set_value(value);
            gtk::glib::ControlFlow::Continue
        });
    }
}

fn save_report(mode: &str, report: &serde_json::Value) {
    let dir = crate::daemon::cache_dir().join("mediciones");
    let _ = std::fs::create_dir_all(&dir);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path: PathBuf = dir.join(format!("{mode}-{stamp}.json"));
    let _ = std::fs::write(path, serde_json::to_vec_pretty(report).unwrap_or_default());
}
