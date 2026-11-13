use std::path::PathBuf;
use std::time::Instant;

use musa_daemon::data::{self, Library};
use musa_daemon::server::{self, ServerConfig};
use musa_daemon::sidecar::Sidecar;

fn main() -> anyhow::Result<()> {
    let mut cache_dir = std::env::var_os("MUSA_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(data::default_cache_dir);
    let mut config_dir = std::env::var_os("MUSA_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(data::default_config_dir);
    let mut socket: Option<PathBuf> = std::env::var_os("MUSA_SOCKET").map(PathBuf::from);
    let mut print_summary = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--socket" => {
                socket = Some(PathBuf::from(
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("--socket requiere ruta"))?,
                ));
            }
            "--cache" => {
                cache_dir = PathBuf::from(
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("--cache requiere ruta"))?,
                );
            }
            "--config" => {
                config_dir = PathBuf::from(
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("--config requiere ruta"))?,
                );
            }
            "--print-summary" => print_summary = true,
            "--help" | "-h" => {
                println!(
                    "musad [--socket RUTA] [--cache DIR] [--config DIR] [--print-summary]\n\n\
                     Variables: MUSA_SOCKET, MUSA_CACHE_DIR, MUSA_CONFIG_DIR"
                );
                return Ok(());
            }
            other => anyhow::bail!("argumento desconocido: {other} (usa --help)"),
        }
    }

    std::fs::create_dir_all(&cache_dir)?;
    std::fs::create_dir_all(&config_dir)?;
    let covers_dir = cache_dir.join("covers");
    let favorites_path = config_dir.join("favorites.json");
    let followed_path = config_dir.join("artists.json");
    let added_albums_path = config_dir.join("albums.json");
    let player_path = cache_dir.join("player.json");
    let socket_path = socket.unwrap_or_else(|| cache_dir.join("musad.sock"));

    let started = Instant::now();
    let sidecar = Sidecar::start()?;
    let snapshot = sidecar.library(false)?;
    let mut library = Library::from_snapshot(snapshot);
    // Los artistas seguidos viven en Musa aunque no tengan canciones propias
    // en la biblioteca de Apple.
    library.merge_followed(server::load_followed_artists(&followed_path));
    // Los álbumes añadidos desde Musa también sobreviven a los refrescos (y a
    // la caché de Apple, que tarda en incluirlos).
    library.merge_added_albums(server::load_added_albums(&added_albums_path));
    // Los favoritos son locales: se guardan en favorites.json y se reaplican
    // sobre la biblioteca de Apple al arrancar.
    if let Some(favs) = server::load_favorites(&favorites_path) {
        library.apply_favorites(&favs);
    }
    library.refresh_home();
    eprintln!(
        "musad: biblioteca Apple Music: {} canciones, {} álbumes, {} artistas, {} playlists",
        library.snapshot.stats.songs,
        library.snapshot.stats.albums,
        library.snapshot.stats.artists,
        library.snapshot.stats.playlists
    );
    eprintln!("musad: lista en {:.1}s", started.elapsed().as_secs_f32());

    if print_summary {
        let summary = serde_json::json!({
            "songs": library.snapshot.stats.songs,
            "albums": library.snapshot.stats.albums,
            "artists": library.snapshot.stats.artists,
            "playlists": library.snapshot.stats.playlists,
            "favorites": {
                "songs": library.snapshot.favorites.song_ids.len(),
                "albums": library.snapshot.favorites.album_ids.len(),
                "artists": library.snapshot.favorites.artist_ids.len(),
                "playlists": library.snapshot.favorites.playlist_ids.len(),
            },
            "home_sections": library.snapshot.home.len(),
            "generated_at": library.snapshot.generated_at,
            "cache": cache_dir.display().to_string(),
            "socket": socket_path.display().to_string(),
        });
        println!("{}", serde_json::to_string_pretty(&summary)?);
    }

    let restored = server::load_persisted_player(&player_path);
    let config = ServerConfig {
        socket_path,
        covers_dir,
        favorites_path,
        followed_path,
        added_albums_path,
        player_path,
        name: "Musa (Apple Music)".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        mode: "real".into(),
        sidecar: Some(sidecar),
    };
    server::run(config, library, restored)
}
