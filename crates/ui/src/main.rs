mod client;
mod covers;
mod daemon;
mod data;
mod ui;

use musa_protocol::{LibrarySnapshot, PlayerState};

fn main() -> anyhow::Result<()> {
    let mut socket_override: Option<std::path::PathBuf> = None;
    let mut measure: Option<String> = None;
    let mut page: Option<String> = None;
    let mut query: Option<String> = None;
    let mut open: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--measure" => {
                measure = Some(
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("--measure requiere un modo"))?,
                );
            }
            "--socket" => {
                socket_override =
                    Some(std::path::PathBuf::from(args.next().ok_or_else(|| {
                        anyhow::anyhow!("--socket requiere una ruta")
                    })?));
            }
            "--page" => {
                page = Some(
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("--page requiere un nombre"))?,
                );
            }
            "--query" => {
                query = Some(
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("--query requiere texto"))?,
                );
            }
            "--open" => {
                open = Some(
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("--open requiere un id"))?,
                );
            }
            "--help" | "-h" => {
                println!(
                    "musa [--socket RUTA] [--page PÁGINA] [--query TEXTO] [--open ID]\n                     \x20    [--measure library|scroll|playing|paused]\n\n\
                     La ventana vacía se mide con `musa-empty`.\n\
                     Variables: MUSA_SOCKET, MUSA_DAEMON, MUSA_NO_AUTOSTART, MUSA_MEASURE_SECONDS"
                );
                return Ok(());
            }
            "--version" => {
                println!("musa {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            other => anyhow::bail!("argumento desconocido: {other} (usa --help)"),
        }
    }
    if let Some(path) = socket_override {
        // SAFETY: se ejecuta antes de crear cualquier hilo de GTK.
        unsafe { std::env::set_var("MUSA_SOCKET", path) };
    }
    if measure.as_deref() == Some("empty") {
        anyhow::bail!("para medir la ventana vacía usa el binario `musa-empty`");
    }

    let socket = daemon::socket_path();
    daemon::ensure_daemon(&socket)?;
    let hello = client::blocking_call(&socket, musa_protocol::cmd::HELLO, serde_json::Value::Null)?;
    let protocol = hello
        .get("protocol")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    if protocol != musa_protocol::PROTOCOL_VERSION as u64 {
        anyhow::bail!(
            "el daemon habla protocolo {protocol} y la UI espera {}",
            musa_protocol::PROTOCOL_VERSION
        );
    }
    let snapshot: LibrarySnapshot = serde_json::from_value(client::blocking_call(
        &socket,
        musa_protocol::cmd::LIBRARY_GET,
        serde_json::Value::Null,
    )?)?;
    let player: PlayerState = serde_json::from_value(client::blocking_call(
        &socket,
        musa_protocol::cmd::PLAYER_STATE,
        serde_json::Value::Null,
    )?)?;
    let client = client::Client::connect(&socket)?;

    eprintln!(
        "musa: daemon en {} · {} canciones · {} álbumes · {} artistas",
        socket.display(),
        snapshot.stats.songs,
        snapshot.stats.albums,
        snapshot.stats.artists
    );

    ui::run(ui::AppInit {
        client,
        snapshot,
        player,
        measure,
        page,
        query,
        open,
    });
    Ok(())
}
