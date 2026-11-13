//! Decodificación diferida de portadas con grupo de 4 hilos y caché LRU.
//!
//! El JPEG se decodifica y reduce en un hilo de trabajo; el hilo principal
//! solo convierte los píxeles en un `Pixbuf` y lo entrega al widget. El tope de
//! caché es de 72 MB de píxeles RGBA, suficiente para esta pantalla y muy por
//! debajo del catálogo completo.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};

use gdk_pixbuf::{Colorspace, Pixbuf};
use relm4::gtk::glib;

const MAX_CACHE_BYTES: usize = 16 * 1024 * 1024;
const WORKERS: usize = 4;

type Key = (PathBuf, u32);
type Callback = Box<dyn FnOnce(Option<Pixbuf>) + 'static>;

struct Job {
    key: Key,
    path: PathBuf,
    px: u32,
}

struct Done {
    key: Key,
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    error: Option<String>,
}

struct Entry {
    pixbuf: Pixbuf,
    bytes: usize,
    stamp: u64,
}

#[derive(Default)]
struct Lru {
    entries: HashMap<Key, Entry>,
    bytes: usize,
    stamp: u64,
}

impl Lru {
    fn get(&mut self, key: &Key) -> Option<Pixbuf> {
        self.stamp += 1;
        let stamp = self.stamp;
        let entry = self.entries.get_mut(key)?;
        entry.stamp = stamp;
        Some(entry.pixbuf.clone())
    }

    fn insert(&mut self, key: Key, pixbuf: Pixbuf) {
        let bytes = (pixbuf.width().max(0) as usize) * (pixbuf.height().max(0) as usize) * 4;
        self.stamp += 1;
        if let Some(old) = self.entries.insert(
            key,
            Entry {
                pixbuf,
                bytes,
                stamp: self.stamp,
            },
        ) {
            self.bytes = self.bytes.saturating_sub(old.bytes);
        }
        self.bytes += bytes;
        while self.bytes > MAX_CACHE_BYTES && self.entries.len() > 1 {
            let victim = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.stamp)
                .map(|(key, _)| key.clone());
            let Some(victim) = victim else { break };
            if let Some(entry) = self.entries.remove(&victim) {
                self.bytes = self.bytes.saturating_sub(entry.bytes);
            }
        }
    }
}

struct Inner {
    job_tx: Vec<mpsc::Sender<Job>>,
    next_worker: Cell<usize>,
    cache: RefCell<Lru>,
    pending: RefCell<HashMap<Key, Vec<Callback>>>,
    inbox: Arc<Mutex<VecDeque<Done>>>,
    hits: Cell<u64>,
    misses: Cell<u64>,
}

#[derive(Clone)]
pub struct CoverLoader {
    inner: Arc<Inner>,
}

impl CoverLoader {
    pub fn new() -> Self {
        let inbox: Arc<Mutex<VecDeque<Done>>> = Arc::new(Mutex::new(VecDeque::new()));
        let mut job_tx = Vec::new();
        for index in 0..WORKERS {
            let (tx, rx) = mpsc::channel::<Job>();
            let inbox = inbox.clone();
            std::thread::Builder::new()
                .name(format!("musa-cover-{index}"))
                .spawn(move || {
                    while let Ok(job) = rx.recv() {
                        let result = decode(&job.path, job.px);
                        let done = match result {
                            Ok((rgba, width, height)) => Done {
                                key: job.key,
                                rgba,
                                width,
                                height,
                                error: None,
                            },
                            Err(error) => Done {
                                key: job.key,
                                rgba: Vec::new(),
                                width: 0,
                                height: 0,
                                error: Some(error),
                            },
                        };
                        inbox.lock().unwrap().push_back(done);
                    }
                })
                .expect("no se pudo crear el hilo de portadas");
            job_tx.push(tx);
        }
        Self {
            inner: Arc::new(Inner {
                job_tx,
                next_worker: Cell::new(0),
                cache: RefCell::new(Lru::default()),
                pending: RefCell::new(HashMap::new()),
                inbox,
                hits: Cell::new(0),
                misses: Cell::new(0),
            }),
        }
    }

    pub fn request<F>(&self, path: &Path, px: u32, callback: F)
    where
        F: FnOnce(Option<Pixbuf>) + 'static,
    {
        if px == 0 {
            callback(None);
            return;
        }
        let key = (path.to_path_buf(), px);
        if let Some(pixbuf) = self.inner.cache.borrow_mut().get(&key) {
            self.inner.hits.set(self.inner.hits.get() + 1);
            callback(Some(pixbuf));
            return;
        }
        if let Some(waiters) = self.inner.pending.borrow_mut().get_mut(&key) {
            waiters.push(Box::new(callback));
            return;
        }
        self.inner
            .pending
            .borrow_mut()
            .insert(key.clone(), vec![Box::new(callback)]);
        self.inner.misses.set(self.inner.misses.get() + 1);
        let worker = self.inner.next_worker.get() % self.inner.job_tx.len();
        self.inner.next_worker.set(worker + 1);
        let job = Job {
            key,
            path: path.to_path_buf(),
            px,
        };
        let key = job.key.clone();
        if self.inner.job_tx[worker].send(job).is_err() {
            if let Some(waiters) = self.inner.pending.borrow_mut().remove(&key) {
                for waiter in waiters {
                    waiter(None);
                }
            }
        }
    }

    /// Debe llamarse desde el hilo principal (temporizador de GLib).
    pub fn pump(&self) {
        loop {
            let done = self.inner.inbox.lock().unwrap().pop_front();
            let Some(done) = done else { break };
            let pixbuf = if let Some(error) = done.error {
                eprintln!("musa: portada {:?}: {error}", done.key.0);
                None
            } else {
                let stride = done.width as i32 * 4;
                let bytes = glib::Bytes::from_owned(done.rgba);
                let pixbuf = Pixbuf::from_bytes(
                    &bytes,
                    Colorspace::Rgb,
                    true,
                    8,
                    done.width as i32,
                    done.height as i32,
                    stride,
                );
                self.inner
                    .cache
                    .borrow_mut()
                    .insert(done.key.clone(), pixbuf.clone());
                Some(pixbuf)
            };
            if let Some(waiters) = self.inner.pending.borrow_mut().remove(&done.key) {
                for waiter in waiters {
                    waiter(pixbuf.clone());
                }
            }
        }
    }

    pub fn stats(&self) -> (u64, u64, usize) {
        (
            self.inner.hits.get(),
            self.inner.misses.get(),
            self.inner.cache.borrow().bytes,
        )
    }
}

fn decode(path: &Path, px: u32) -> Result<(Vec<u8>, u32, u32), String> {
    let reader = image::ImageReader::open(path)
        .map_err(|e| format!("abrir: {e}"))?
        .with_guessed_format()
        .map_err(|e| format!("formato: {e}"))?;
    let image = reader.decode().map_err(|e| format!("decodificar: {e}"))?;
    let thumb = image.thumbnail(px, px);
    let (width, height) = (thumb.width(), thumb.height());
    Ok((thumb.to_rgba8().into_raw(), width, height))
}
