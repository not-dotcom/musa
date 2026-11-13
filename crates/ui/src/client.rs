//! Cliente del protocolo. El lector corre en un hilo; las respuestas se
//! entregan en el hilo principal cuando `pump()` se llama desde un temporizador
//! de GLib. No se bloquea el bucle de la interfaz.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use musa_protocol::{Event, ProtocolError, Request, Response, cmd, encode_line};
use serde_json::Value;

type Callback = Box<dyn FnOnce(Result<Value, ProtocolError>) + 'static>;

enum Incoming {
    Response(u64, Result<Value, ProtocolError>),
    Event(Event),
    Closed,
}

#[derive(Clone)]
pub struct Client {
    outgoing: mpsc::Sender<String>,
    pending: Rc<RefCell<HashMap<u64, Callback>>>,
    inbox: Arc<Mutex<VecDeque<Incoming>>>,
    next_id: Rc<Cell<u64>>,
    online: Rc<Cell<bool>>,
}

impl Client {
    pub fn connect(path: &Path) -> std::io::Result<Self> {
        let stream = UnixStream::connect(path)?;
        let mut writer_stream = stream.try_clone()?;
        let reader_stream = stream;
        let (outgoing, outgoing_rx) = mpsc::channel::<String>();
        let inbox = Arc::new(Mutex::new(VecDeque::new()));
        let reader_inbox = inbox.clone();

        std::thread::spawn(move || {
            while let Ok(line) = outgoing_rx.recv() {
                if writer_stream.write_all(line.as_bytes()).is_err() {
                    break;
                }
                let _ = writer_stream.flush();
            }
        });

        std::thread::spawn(move || {
            let reader = BufReader::new(reader_stream);
            for line in reader.lines() {
                let Ok(line) = line else { break };
                if line.trim().is_empty() {
                    continue;
                }
                let Ok(value) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if value.get("event").is_some() {
                    if let Ok(event) = serde_json::from_value::<Event>(value) {
                        reader_inbox
                            .lock()
                            .unwrap()
                            .push_back(Incoming::Event(event));
                    }
                    continue;
                }
                if let Ok(response) = serde_json::from_value::<Response>(value) {
                    let result = response.clone().into_result();
                    reader_inbox
                        .lock()
                        .unwrap()
                        .push_back(Incoming::Response(response.id, result));
                }
            }
            reader_inbox.lock().unwrap().push_back(Incoming::Closed);
        });

        Ok(Self {
            outgoing,
            pending: Rc::new(RefCell::new(HashMap::new())),
            inbox,
            next_id: Rc::new(Cell::new(1)),
            online: Rc::new(Cell::new(true)),
        })
    }

    pub fn call<F>(&self, command: &str, params: Value, callback: F)
    where
        F: FnOnce(Result<Value, ProtocolError>) + 'static,
    {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        self.pending.borrow_mut().insert(id, Box::new(callback));
        let request = Request::new(id, command, params);
        if self.outgoing.send(encode_line(&request)).is_err() {
            self.online.set(false);
            if let Some(cb) = self.pending.borrow_mut().remove(&id) {
                cb(Err(ProtocolError::new("closed", "daemon desconectado")));
            }
        }
    }

    pub fn call_simple<F>(&self, command: &str, callback: F)
    where
        F: FnOnce(Result<Value, ProtocolError>) + 'static,
    {
        self.call(command, Value::Null, callback);
    }

    /// Ejecuta callbacks pendientes y devuelve los eventos recibidos.
    pub fn pump(&self) -> Vec<Event> {
        let mut events = Vec::new();
        loop {
            let next = self.inbox.lock().unwrap().pop_front();
            let Some(incoming) = next else { break };
            match incoming {
                Incoming::Response(id, result) => {
                    if let Some(callback) = self.pending.borrow_mut().remove(&id) {
                        callback(result);
                    }
                }
                Incoming::Event(event) => events.push(event),
                Incoming::Closed => {
                    self.online.set(false);
                    let pending: Vec<_> = self
                        .pending
                        .borrow_mut()
                        .drain()
                        .map(|(_, cb)| cb)
                        .collect();
                    for callback in pending {
                        callback(Err(ProtocolError::new(
                            "closed",
                            "el daemon cerró la conexión",
                        )));
                    }
                }
            }
        }
        events
    }

    pub fn is_online(&self) -> bool {
        self.online.get()
    }
}

/// Llamada bloqueante usada solo durante el arranque (antes de que exista el
/// bucle de GTK). Descarta eventos que lleguen por el camino.
pub fn blocking_call(socket: &Path, command: &str, params: Value) -> anyhow::Result<Value> {
    let stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(Duration::from_secs(20)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    let id = 1u64;
    writer.write_all(encode_line(&Request::new(id, command, params)).as_bytes())?;
    writer.flush()?;
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            anyhow::bail!("el daemon cerró la conexión");
        }
        let value: Value = serde_json::from_str(line.trim())?;
        if value.get("event").is_some() {
            continue;
        }
        if value.get("id").and_then(Value::as_u64) == Some(id) {
            let response: Response = serde_json::from_value(value)?;
            return response.into_result().map_err(|e| anyhow::anyhow!("{e}"));
        }
    }
}

pub fn set_favorite(
    client: &Client,
    kind: &str,
    id: &str,
    favorite: bool,
    on_result: impl FnOnce(bool) + 'static,
) {
    client.call(
        cmd::FAVORITE_SET,
        serde_json::json!({"kind": kind, "id": id, "favorite": favorite}),
        move |result| on_result(result.is_ok()),
    );
}
