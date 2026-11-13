//! Arranque y localización del daemon.

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub fn cache_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_CACHE_HOME") {
        return PathBuf::from(dir).join("musa");
    }
    std::env::var("HOME")
        .map(|home| PathBuf::from(home).join(".cache/musa"))
        .unwrap_or_else(|_| PathBuf::from("/tmp/musa"))
}

pub fn socket_path() -> PathBuf {
    if let Ok(path) = std::env::var("MUSA_SOCKET") {
        return PathBuf::from(path);
    }
    cache_dir().join("musad.sock")
}

/// Si no hay daemon, arranca `musad` desde el mismo directorio que el
/// ejecutable o desde `PATH`. No arranca nada si MUSA_NO_AUTOSTART=1.
pub fn ensure_daemon(socket: &Path) -> anyhow::Result<()> {
    if UnixStream::connect(socket).is_ok() {
        return Ok(());
    }
    if std::env::var("MUSA_NO_AUTOSTART").as_deref() == Ok("1") {
        anyhow::bail!(
            "no hay daemon en {} y MUSA_NO_AUTOSTART=1",
            socket.display()
        );
    }
    let daemon = find_daemon().ok_or_else(|| {
        anyhow::anyhow!(
            "no se encontró musad. Compila el workspace o define MUSA_DAEMON=/ruta/a/musad"
        )
    })?;
    spawn_daemon(&daemon, socket, 600)
}

fn find_daemon() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("MUSA_DAEMON") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let candidate = parent.join("musad");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    if let Ok(path) = std::env::var("PATH") {
        for dir in path.split(':') {
            let candidate = PathBuf::from(dir).join("musad");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn spawn_daemon(daemon: &Path, socket: &Path, timeout_secs: u64) -> anyhow::Result<()> {
    if let Some(parent) = socket.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let log_path = cache_dir().join("musad.log");
    let _ = std::fs::create_dir_all(cache_dir());
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let log_err = log.try_clone()?;
    let mut child = Command::new(daemon)
        .arg("--socket")
        .arg(socket)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err))
        .spawn()
        .map_err(|e| anyhow::anyhow!("no se pudo arrancar {}: {e}", daemon.display()))?;
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(timeout_secs) {
        if UnixStream::connect(socket).is_ok() {
            return Ok(());
        }
        if let Ok(Some(status)) = child.try_wait() {
            anyhow::bail!(
                "el daemon terminó durante el arranque ({status}); revisa {}",
                log_path.display()
            );
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let _ = child.kill();
    let _ = child.wait();
    anyhow::bail!(
        "el daemon no respondió en {timeout_secs} s; revisa {}",
        log_path.display()
    )
}
