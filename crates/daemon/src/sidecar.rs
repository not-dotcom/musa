//! Cliente del sidecar Electron para el modo real.
//!
//! El sidecar es el único proceso que habla con Apple Music (MusicKit +
//! Widevine). Este cliente le envía JSON por stdin y correlaciona las
//! respuestas por `id`. Nunca recibe ni registra tokens.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use musa_protocol::{ProtocolError, Response, encode_line};
use serde_json::{Value, json};

pub struct Sidecar {
    child: Mutex<Child>,
    writer: Mutex<ChildStdin>,
    pending: Arc<Mutex<HashMap<u64, mpsc::Sender<Result<Value, ProtocolError>>>>>,
    next_id: AtomicU64,
}

impl Sidecar {
    pub fn start() -> anyhow::Result<Arc<Self>> {
        let electron = find_electron().ok_or_else(|| {
            anyhow::anyhow!(
                "no se encontró Electron castLabs. Define MUSA_ELECTRON o instala sidecar/node_modules"
            )
        })?;
        let script = sidecar_script()?;
        let log_path = crate::data::default_cache_dir().join("sidecar.log");
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;
        let log_err = log.try_clone()?;
        let mut child = Command::new(&electron)
            .arg(&script)
            .arg("--no-sandbox")
            .arg("--disable-gpu")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(log_err))
            .spawn()
            .map_err(|e| anyhow::anyhow!("no se pudo arrancar {}: {e}", electron.display()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("sidecar sin stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("sidecar sin stdout"))?;
        let pending: Arc<Mutex<HashMap<u64, mpsc::Sender<Result<Value, ProtocolError>>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let pending_reader = pending.clone();
        std::thread::Builder::new()
            .name("musa-sidecar-reader".into())
            .spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    let Ok(line) = line else { break };
                    let Ok(value) = serde_json::from_str::<Value>(&line) else {
                        continue;
                    };
                    let Some(id) = value.get("id").and_then(Value::as_u64) else {
                        continue;
                    };
                    let Some(tx) = pending_reader.lock().unwrap().remove(&id) else {
                        continue;
                    };
                    let response: Response = match serde_json::from_value(value) {
                        Ok(r) => r,
                        Err(err) => {
                            let _ =
                                tx.send(Err(ProtocolError::new("sidecar_json", err.to_string())));
                            continue;
                        }
                    };
                    let _ = tx.send(response.into_result());
                }
            })?;
        let sidecar = Arc::new(Self {
            child: Mutex::new(child),
            writer: Mutex::new(stdin),
            pending,
            next_id: AtomicU64::new(1),
        });
        sidecar.wait_ready()?;
        sidecar.ensure_authorized()?;
        Ok(sidecar)
    }

    /// Si la sesión persistente de Apple Music no está autorizada, abre la
    /// ventana de login del sidecar y espera a que el usuario termine. La
    /// ventana se oculta en cuanto MusicKit confirma la autorización.
    fn ensure_authorized(&self) -> anyhow::Result<()> {
        let started = Instant::now();
        let mut last_shown: Option<Instant> = None;
        while started.elapsed() < Duration::from_secs(300) {
            let authorized = self
                .call("session", json!({}), Duration::from_secs(10))
                .ok()
                .and_then(|value| value.get("authorized").and_then(serde_json::Value::as_bool))
                .unwrap_or(false);
            if authorized {
                if last_shown.is_some() {
                    let _ = self.call("hide-login-window", json!({}), Duration::from_secs(3));
                }
                return Ok(());
            }
            let should_show = last_shown
                .map(|shown| shown.elapsed() >= Duration::from_secs(15))
                .unwrap_or(true);
            if should_show {
                last_shown = Some(Instant::now());
                let _ = self.call("show-login-window", json!({}), Duration::from_secs(5));
            }
            std::thread::sleep(Duration::from_secs(2));
        }
        anyhow::bail!(
            "Apple Music no autorizó la sesión en 5 minutos; vuelve a abrir Musa e inicia sesión en la ventana de Apple"
        )
    }

    fn wait_ready(&self) -> anyhow::Result<()> {
        let started = std::time::Instant::now();
        let mut last_error = String::new();
        while started.elapsed() < Duration::from_secs(90) {
            match self.call("session", json!({}), Duration::from_secs(5)) {
                Ok(_) => return Ok(()),
                Err(err) => last_error = err.message,
            }
            std::thread::sleep(Duration::from_millis(1000));
        }
        anyhow::bail!(
            "el sidecar no estuvo listo en 90 s (revisa ~/.cache/musa/sidecar.log): {last_error}"
        )
    }

    pub fn library(&self, refresh: bool) -> anyhow::Result<musa_protocol::LibrarySnapshot> {
        let value = self
            .call(
                "library",
                json!({"refresh": refresh}),
                Duration::from_secs(240),
            )
            .map_err(|e| anyhow::anyhow!("biblioteca Apple: {e}"))?;
        Ok(serde_json::from_value(value)?)
    }

    pub fn state(&self) -> Result<Value, ProtocolError> {
        self.call("state", json!({}), Duration::from_secs(10))
    }

    pub fn lyrics(&self, song_id: &str) -> Result<Value, ProtocolError> {
        self.call(
            "lyrics",
            json!({"song_id": song_id}),
            Duration::from_secs(30),
        )
    }

    pub fn set_queue(
        &self,
        song_ids: &[String],
        index: usize,
        play: bool,
    ) -> Result<Value, ProtocolError> {
        self.call(
            "set-queue",
            json!({"song_ids": song_ids, "index": index, "play": play}),
            Duration::from_secs(30),
        )
    }

    pub fn player(&self, action: &str, params: Value) -> Result<Value, ProtocolError> {
        let mut params = params;
        params["action"] = json!(action);
        self.call("player", params, Duration::from_secs(20))
    }

    pub fn call(
        &self,
        command: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, ProtocolError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        self.pending.lock().unwrap().insert(id, tx);
        let request = json!({"v": 1, "id": id, "cmd": command, "params": params});
        {
            let mut writer = self.writer.lock().unwrap();
            writer
                .write_all(encode_line(&request).as_bytes())
                .map_err(|e| ProtocolError::new("sidecar_io", e.to_string()))?;
            writer
                .flush()
                .map_err(|e| ProtocolError::new("sidecar_io", e.to_string()))?;
        }
        match rx.recv_timeout(timeout) {
            Ok(result) => result,
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                Err(ProtocolError::new(
                    "sidecar_timeout",
                    format!("el sidecar no respondió a {command}"),
                ))
            }
        }
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn sidecar_script() -> anyhow::Result<PathBuf> {
    if let Ok(path) = std::env::var("MUSA_SIDECAR_JS") {
        return Ok(PathBuf::from(path));
    }
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("../../sidecar/main.js"));
            candidates.push(dir.join("sidecar/main.js"));
        }
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../sidecar/main.js"));
    for candidate in candidates {
        if candidate.is_file() {
            return Ok(candidate.canonicalize()?);
        }
    }
    anyhow::bail!("no se encontró sidecar/main.js (define MUSA_SIDECAR_JS)")
}

fn find_electron() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("MUSA_ELECTRON") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let home = std::env::var("HOME").ok().map(PathBuf::from);
    let mut candidates = Vec::new();
    if let Some(home) = &home {
        candidates
            .push(home.join(".local/share/slipmat/sidecar/node_modules/electron/dist/electron"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("../../sidecar/node_modules/electron/dist/electron"));
        }
    }
    for candidate in candidates {
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    if let Ok(path) = std::env::var("PATH") {
        for dir in path.split(':') {
            let candidate = PathBuf::from(dir).join("electron");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}
