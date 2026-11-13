//! Portadas JPEG procedurales y deterministas.
//!
//! No son imágenes de relleno de 1×1: se generan a 600×600 por defecto y se
//! pueden pedir variantes (300, 1200…) para que la decodificación en la UI
//! tenga un coste realista. La caché vive en `~/.cache/musa/covers`.

use std::path::{Path, PathBuf};

use image::codecs::jpeg::JpegEncoder;
use image::{ImageBuffer, Rgb};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoverKind {
    Album,
    Artist,
    Playlist,
}

impl CoverKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Album => "album",
            Self::Artist => "artist",
            Self::Playlist => "playlist",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "album" => Some(Self::Album),
            "artist" => Some(Self::Artist),
            "playlist" => Some(Self::Playlist),
            _ => None,
        }
    }
}

/// Asegura que existe la portada pedida y devuelve su ruta absoluta.
pub fn ensure(covers_dir: &Path, kind: CoverKind, id: &str, size: u32) -> std::io::Result<PathBuf> {
    let size = size.clamp(240, 1_600);
    let path = cover_path(covers_dir, kind, id, size);
    if path.is_file() {
        return Ok(path);
    }
    std::fs::create_dir_all(covers_dir)?;
    let image = render(kind, id, size);
    write_jpeg(&path, &image)?;
    Ok(path)
}

pub fn cover_path(covers_dir: &Path, _kind: CoverKind, id: &str, size: u32) -> PathBuf {
    covers_dir.join(format!("{}-{}.jpg", sanitize(id), size))
}

/// Pre-genera las portadas base (600×600) que referencian los modelos.
/// Se ejecuta en paralelo; las que ya existen no se vuelven a codificar.
pub fn generate_all(
    covers_dir: &Path,
    albums: &[musa_protocol::Album],
    artists: &[musa_protocol::Artist],
    playlists: &[musa_protocol::Playlist],
) -> std::io::Result<()> {
    std::fs::create_dir_all(covers_dir)?;
    let mut jobs: Vec<(CoverKind, String)> = Vec::new();
    for album in albums {
        jobs.push((CoverKind::Album, album.id.clone()));
    }
    for artist in artists {
        jobs.push((CoverKind::Artist, artist.id.clone()));
    }
    for playlist in playlists {
        jobs.push((CoverKind::Playlist, playlist.id.clone()));
    }
    let pending: Vec<_> = jobs
        .into_iter()
        .filter(|(kind, id)| !cover_path(covers_dir, *kind, id, 600).is_file())
        .collect();
    if pending.is_empty() {
        return Ok(());
    }
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(2, 8);
    let chunk = pending.len().div_ceil(workers);
    std::thread::scope(|scope| {
        for slice in pending.chunks(chunk.max(1)) {
            scope.spawn(move || {
                for (kind, id) in slice {
                    let path = cover_path(covers_dir, *kind, id, 600);
                    if let Err(err) = write_jpeg(&path, &render(*kind, id, 600)) {
                        eprintln!("musad: no se pudo generar {path:?}: {err}");
                    }
                }
            });
        }
    });
    Ok(())
}

fn sanitize(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn write_jpeg(path: &Path, image: &RgbImage) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("jpg.tmp");
    let file = std::fs::File::create(&tmp)?;
    let mut encoder = JpegEncoder::new_with_quality(std::io::BufWriter::new(file), 84);
    encoder.encode_image(image).map_err(std::io::Error::other)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

type RgbImage = ImageBuffer<Rgb<u8>, Vec<u8>>;

fn render(kind: CoverKind, id: &str, size: u32) -> RgbImage {
    match kind {
        CoverKind::Album => render_album(id, size),
        CoverKind::Artist => render_artist(id, size),
        CoverKind::Playlist => render_playlist(id, size),
    }
}

fn hash(id: &str, salt: u64) -> u64 {
    let mut h = 0xcbf29ce484222325u64 ^ salt;
    for b in id.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn palette(id: &str) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let h = hash(id, 0);
    let hue = (h % 360) as f32;
    let hue2 = (hue + 38.0 + ((h >> 9) % 90) as f32) % 360.0;
    let accent_hue = (hue + 180.0 + ((h >> 17) % 70) as f32) % 360.0;
    (
        hsl(hue, 0.62, 0.42),
        hsl(hue2, 0.68, 0.20),
        hsl(accent_hue, 0.86, 0.66),
    )
}

fn hsl(h: f32, s: f32, l: f32) -> [f32; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match h {
        h if h < 60.0 => (c, x, 0.0),
        h if h < 120.0 => (x, c, 0.0),
        h if h < 180.0 => (0.0, c, x),
        h if h < 240.0 => (0.0, x, c),
        h if h < 300.0 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [r + m, g + m, b + m]
}

fn render_album(id: &str, size: u32) -> RgbImage {
    let (c1, c2, accent) = palette(id);
    let h = hash(id, 7);
    let mut img = RgbImage::new(size, size);
    let band = ((h >> 5) % 4) as u32;
    for y in 0..size {
        for x in 0..size {
            let fx = x as f32 / size as f32;
            let fy = y as f32 / size as f32;
            let t = (fx * 0.55 + fy * 0.45).clamp(0.0, 1.0);
            let mut col = mix(c2, c1, t);
            // Franjas diagonales sutiles.
            let diag = ((x + y * 2) as f32 / size as f32 * 6.0).fract();
            if diag < 0.08 {
                col = mix(col, c2, 0.18);
            }
            let _ = band;
            img.put_pixel(x, y, to_rgb(col));
        }
    }
    // Formas geométricas grandes, una composición distinta por álbum.
    match h % 6 {
        0 => {
            let cx = size / 2;
            let cy = size / 2;
            let r = size as f32 * 0.29;
            fill_circle(&mut img, cx as f32, cy as f32, r, accent, 0.88);
            fill_circle(&mut img, cx as f32, cy as f32, r * 0.64, c2, 0.72);
            fill_circle(&mut img, cx as f32, cy as f32, r * 0.22, accent, 0.95);
        }
        1 => {
            let y0 = (size as f32 * 0.58) as u32;
            for y in y0..size {
                if (y - y0) % 46 < 23 {
                    fill_rect(&mut img, 0, y, size, 1, accent, 0.83);
                }
            }
            fill_rect(
                &mut img,
                0,
                (size as f32 * 0.18) as u32,
                size,
                (size as f32 * 0.07) as u32,
                c2,
                0.6,
            );
        }
        2 => {
            let cx = (size as f32 * 0.72) as f32;
            let cy = (size as f32 * 0.28) as f32;
            fill_circle(&mut img, cx, cy, size as f32 * 0.25, accent, 0.9);
            fill_rect(
                &mut img,
                0,
                (size as f32 * 0.68) as u32,
                size,
                (size as f32 * 0.16) as u32,
                c2,
                0.75,
            );
        }
        3 => {
            for i in 0..7 {
                let y = (size as f32 * (0.18 + i as f32 * 0.1)) as u32;
                let w = (size as f32 * (0.3 + ((h >> i) % 40) as f32 / 100.0)) as u32;
                fill_rect(&mut img, (size - w) / 2, y, w, 10, accent, 0.6);
            }
        }
        4 => {
            let cx = size as i32 / 2;
            let cy = size as i32 / 2;
            let r = size as i32 / 2 - 4;
            for a in 0..360 {
                if a % 12 < 4 {
                    let rad = a as f32 * std::f32::consts::PI / 180.0;
                    let x = cx as f32 + rad.cos() * r as f32 * 0.82;
                    let y = cy as f32 + rad.sin() * r as f32 * 0.82;
                    fill_circle(&mut img, x, y, size as f32 * 0.02, accent, 0.8);
                }
            }
            fill_circle(&mut img, cx as f32, cy as f32, size as f32 * 0.2, c2, 0.5);
        }
        _ => {
            let half = size / 2;
            fill_rect(&mut img, 0, half, size, half, c2, 0.55);
            fill_circle(
                &mut img,
                size as f32 * 0.28,
                size as f32 * 0.3,
                size as f32 * 0.17,
                accent,
                0.9,
            );
            fill_circle(
                &mut img,
                size as f32 * 0.74,
                size as f32 * 0.7,
                size as f32 * 0.09,
                accent,
                0.65,
            );
        }
    }
    vignette(&mut img, 0.55);
    img
}

fn render_artist(id: &str, size: u32) -> RgbImage {
    let (c1, c2, accent) = palette(id);
    let mut img = RgbImage::new(size, size);
    let cx = size as f32 / 2.0;
    let cy = size as f32 / 2.0;
    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 - cx) / size as f32;
            let dy = (y as f32 - cy) / size as f32;
            let d = (dx * dx + dy * dy).sqrt() * 2.0;
            let col = mix(accent, c2, d.clamp(0.0, 1.0) * 0.85);
            img.put_pixel(x, y, to_rgb(col));
        }
    }
    // Silueta: hombros + cabeza.
    let head_y = cy * 0.78;
    fill_circle(&mut img, cx, head_y, size as f32 * 0.16, c1, 0.92);
    let shoulder_r = size as f32 * 0.36;
    for y in (size as f32 * 0.74) as u32..size {
        let dy = y as f32 - (size as f32 * 0.78);
        if dy <= shoulder_r {
            let half = (shoulder_r * shoulder_r - dy * dy).sqrt();
            fill_rect(
                &mut img,
                (cx - half).max(0.0) as u32,
                y,
                (half * 2.0) as u32,
                1,
                c1,
                0.88,
            );
        }
    }
    // Aro difuso.
    for a in 0..360 {
        if a % 3 == 0 {
            let rad = a as f32 * std::f32::consts::PI / 180.0;
            let x = cx + rad.cos() * size as f32 * 0.46;
            let y = cy + rad.sin() * size as f32 * 0.46;
            fill_circle(&mut img, x, y, size as f32 * 0.005, accent, 0.35);
        }
    }
    vignette(&mut img, 0.4);
    img
}

fn render_playlist(id: &str, size: u32) -> RgbImage {
    let (c1, c2, accent) = palette(id);
    let mut img = RgbImage::new(size, size);
    let quarter = size / 2;
    let colors = [
        (c1, accent),
        (accent, c2),
        (c2, c1),
        (mix(c2, accent, 0.4), c1),
    ];
    for (i, (top, bottom)) in colors.iter().enumerate() {
        let ox = (i as u32 % 2) * quarter;
        let oy = (i as u32 / 2) * quarter;
        for y in 0..quarter {
            for x in 0..quarter {
                let t = (x + y) as f32 / (quarter * 2) as f32;
                let col = mix(*top, *bottom, t);
                img.put_pixel(ox + x, oy + y, to_rgb(col));
            }
        }
        // Un círculo distinto en cada cuadrante.
        let cx = ox as f32 + quarter as f32 * (0.3 + 0.4 * ((i * 37 % 10) as f32 / 10.0));
        let cy = oy as f32 + quarter as f32 * (0.3 + 0.4 * ((i * 53 % 10) as f32 / 10.0));
        let r = quarter as f32 * (0.12 + 0.1 * (i as f32 / 4.0));
        fill_circle(&mut img, cx, cy, r, c1, 0.5);
    }
    // Separadores.
    fill_rect(&mut img, quarter.saturating_sub(1), 0, 2, size, c2, 0.35);
    fill_rect(&mut img, 0, quarter.saturating_sub(1), size, 2, c2, 0.35);
    vignette(&mut img, 0.45);
    img
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn to_rgb(c: [f32; 3]) -> Rgb<u8> {
    Rgb([
        (c[0].clamp(0.0, 1.0) * 255.0) as u8,
        (c[1].clamp(0.0, 1.0) * 255.0) as u8,
        (c[2].clamp(0.0, 1.0) * 255.0) as u8,
    ])
}

fn fill_circle(img: &mut RgbImage, cx: f32, cy: f32, r: f32, color: [f32; 3], alpha: f32) {
    let (w, h) = img.dimensions();
    let x0 = (cx - r).floor().max(0.0) as u32;
    let x1 = (cx + r).ceil().min(w as f32) as u32;
    let y0 = (cy - r).floor().max(0.0) as u32;
    let y1 = (cy + r).ceil().min(h as f32) as u32;
    for y in y0..y1 {
        for x in x0..x1 {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            if dx * dx + dy * dy <= r * r {
                blend_pixel(img, x, y, color, alpha);
            }
        }
    }
}

fn fill_rect(img: &mut RgbImage, x: u32, y: u32, w: u32, h: u32, color: [f32; 3], alpha: f32) {
    let (iw, ih) = img.dimensions();
    for yy in y..(y + h).min(ih) {
        for xx in x..(x + w).min(iw) {
            blend_pixel(img, xx, yy, color, alpha);
        }
    }
}

fn blend_pixel(img: &mut RgbImage, x: u32, y: u32, color: [f32; 3], alpha: f32) {
    let pixel = img.get_pixel_mut(x, y);
    let base = [
        pixel[0] as f32 / 255.0,
        pixel[1] as f32 / 255.0,
        pixel[2] as f32 / 255.0,
    ];
    *pixel = to_rgb(mix(base, color, alpha));
}

fn vignette(img: &mut RgbImage, strength: f32) {
    let (w, h) = img.dimensions();
    let cx = w as f32 / 2.0;
    let cy = h as f32 / 2.0;
    let max_d = (cx * cx + cy * cy).sqrt();
    for y in 0..h {
        for x in 0..w {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let d = (dx * dx + dy * dy).sqrt() / max_d;
            let f = 1.0 - strength * d * d;
            let p = img.get_pixel_mut(x, y);
            p[0] = (p[0] as f32 * f) as u8;
            p[1] = (p[1] as f32 * f) as u8;
            p[2] = (p[2] as f32 * f) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_valid_jpeg_of_requested_size() {
        let dir = std::env::temp_dir().join(format!("musa-covers-{}", std::process::id()));
        let path = ensure(&dir, CoverKind::Album, "album-001", 300).unwrap();
        let meta = std::fs::metadata(&path).unwrap();
        assert!(meta.len() > 2_000, "JPEG demasiado pequeño");
        let img = image::open(&path).unwrap();
        assert_eq!(img.width(), 300);
        assert_eq!(img.height(), 300);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn same_id_same_bytes() {
        let dir = std::env::temp_dir().join(format!("musa-covers-det-{}", std::process::id()));
        let a = ensure(&dir, CoverKind::Playlist, "playlist-003", 240).unwrap();
        let bytes_a = std::fs::read(&a).unwrap();
        let _ = std::fs::remove_file(&a);
        let a = ensure(&dir, CoverKind::Playlist, "playlist-003", 240).unwrap();
        let bytes_b = std::fs::read(&a).unwrap();
        assert_eq!(bytes_a, bytes_b);
        let _ = std::fs::remove_dir_all(dir);
    }
}
