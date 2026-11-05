//! CadCraft L3: WebM video from RGBA frames, in pure Rust (no codec library): VP8 (RFC 6386) key frames and inter
//! frames predicted from the frame before ([`vp8::Encoder`]), the frames in a WebM file ([`webm::mux`]). ModelCraft's scenes recorded as an
//! animation (0.4 M23: 0.5 records the view with the browser's MediaRecorder) are written with it.

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

mod tables;
pub mod vp8;
pub mod webm;

/// Most frames one recording keeps (10 minutes at 30 frames a second).
pub const MAX_FRAMES: usize = 18_000;

/// A key frame every this many frames (two seconds at 30 a second): where a player can start or seek.
pub const KEY_EVERY: usize = 60;

/// A video being recorded: frames of one size, coded as they come.
#[derive(Debug)]
pub struct Recorder {
    pub w: u32,
    pub h: u32,
    pub q: u8,
    /// each frame's VP8 data and whether it is a key frame
    frames: Vec<(Vec<u8>, bool)>,
    enc: Option<vp8::Encoder>,
}

impl Recorder {
    /// A recorder of `w` × `h` frames (even sizes, as the page's canvas: `& ~1`) at quantizer index `q`.
    pub fn new(w: u32, h: u32, q: u8) -> Recorder {
        let (w, h) = ((w & !1).max(2), (h & !1).max(2));
        Recorder { w, h, q, frames: Vec::new(), enc: vp8::Encoder::new(w, h, q, KEY_EVERY) }
    }
    /// One RGBA frame (rows top to bottom; a different size is refused) → whether it was taken.
    pub fn push(&mut self, rgba: &[u8], w: u32, h: u32) -> bool {
        if self.frames.len() >= MAX_FRAMES {
            return false;
        }
        let Some(enc) = self.enc.as_mut() else { return false };
        let frame = if (w, h) == (self.w, self.h) {
            enc.frame(rgba)
        } else if w >= self.w && h >= self.h {
            // the view's odd last row or column left out
            let mut c = Vec::with_capacity((self.w * self.h * 4) as usize);
            for y in 0..self.h as usize {
                let o = y * w as usize * 4;
                c.extend_from_slice(rgba.get(o..o + self.w as usize * 4).unwrap_or(&[]));
            }
            enc.frame(&c)
        } else {
            None
        };
        match frame {
            Some(f) => {
                self.frames.push(f);
                true
            }
            None => false,
        }
    }
    /// How many frames so far.
    pub fn len(&self) -> usize {
        self.frames.len()
    }
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }
    /// The WebM file at `fps` frames a second.
    pub fn finish(&self, fps: f64, app: &str) -> Vec<u8> {
        webm::mux(&self.frames, self.w, self.h, fps, app)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `CC_VIDEO_BENCH=1`: a 1280 × 720 frame's coding time.
    #[test]
    fn bench_a_frame() {
        if std::env::var("CC_VIDEO_BENCH").is_err() {
            return;
        }
        let (w, h) = (1280u32, 720u32);
        let rgba: Vec<u8> = (0..w * h).flat_map(|i| [(i % 251) as u8, (i / w % 255) as u8, 120, 255]).collect();
        let t = std::time::Instant::now();
        let f = vp8::key_frame(&rgba, w, h, 16).unwrap();
        eprintln!("1280x720 key frame: {} ms, {} KB", t.elapsed().as_millis(), f.len() / 1024);
        // inter frames of a moving scene (M6.40): best of 30
        let mut e = vp8::Encoder::new(w, h, 16, 1000).unwrap();
        let mut best = f64::INFINITY;
        let mut bytes = 0;
        for k in 0..31u32 {
            let rgba = scene(w, h, k);
            let t = std::time::Instant::now();
            let (f, _) = e.frame(&rgba).unwrap();
            if k > 0 {
                best = best.min(t.elapsed().as_secs_f64() * 1000.0);
                bytes += f.len();
            }
        }
        eprintln!("1280x720 inter frame: best {best:.1} ms, {} KB a frame", bytes / 30 / 1024);
    }

    /// A frame of a scene that moves: a textured ground panning right and down, a disc crossing it.
    fn scene(w: u32, h: u32, t: u32) -> Vec<u8> {
        (0..w * h)
            .flat_map(|i| {
                let (x, y) = ((i % w) as i32, (i / w) as i32);
                let (u, v) = (x + 2 * t as i32, y + 2 * t as i32);
                let disc = ((x - 30 - 5 * t as i32).pow(2) + (y - 40).pow(2)) < 400;
                if disc {
                    [230, 40, 30, 255]
                } else {
                    // smooth shading with tiles, as a rendered model has, not pixel noise
                    let c = ((u / 16 + v / 16) % 2 * 50 + u % 64 + v % 48) as u8;
                    [c, c.wrapping_add(40), 200 - c / 2, 255]
                }
            })
            .collect()
    }

    /// M6.40: inter frames decode to the encoder's own reconstruction, byte for byte (ffmpeg's VP8 decoder, an oracle
    /// run as a program; `CC_VIDEO_FFMPEG=1`), and a film of a moving scene is several times smaller than key frames.
    #[test]
    fn inter_frames_decode_as_the_encoder_reconstructs_them() {
        let (w, h) = (176u32, 112u32);
        let n = 75u32;
        let mut e = vp8::Encoder::new(w, h, 16, 30).unwrap();
        let mut frames = Vec::new();
        let mut recon = Vec::new();
        let mut keys_only = 0usize;
        for t in 0..n {
            let rgba = scene(w, h, t);
            let (f, key) = e.frame(&rgba).unwrap();
            assert_eq!(key, t % 30 == 0);
            keys_only += vp8::key_frame(&rgba, w, h, 16).unwrap().len();
            frames.push((f, key));
            recon.extend(e.last_i420().unwrap());
        }
        let total: usize = frames.iter().map(|(f, _)| f.len()).sum();
        eprintln!("{n} frames {w} x {h}: {total} bytes, as key frames {keys_only}");
        let file = webm::mux(&frames, w, h, 30.0, "CadCraft");
        if std::env::var("CC_VIDEO_FFMPEG").is_err() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("cc-video-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (src, out) = (dir.join("f.webm"), dir.join("f.yuv"));
        std::fs::write(&src, &file).unwrap();
        let st = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-i"])
            .arg(&src)
            .args(["-f", "rawvideo", "-pix_fmt", "yuv420p"])
            .arg(&out)
            .status()
            .unwrap();
        assert!(st.success());
        let got = std::fs::read(&out).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(got.len(), recon.len());
        let first = got.iter().zip(&recon).position(|(a, b)| a != b);
        assert!(first.is_none(), "differs at byte {first:?} (frame {})", first.unwrap_or(0) / (recon.len() / n as usize));
        assert!(total * 3 < keys_only, "{total} bytes against {keys_only} as key frames");
    }

    /// A moving picture; `CC_VIDEO_OUT=<file>` keeps the WebM for a player or ffmpeg to check.
    #[test]
    fn a_short_video() {
        let (w, h) = (161u32, 97u32);
        let mut r = Recorder::new(w, h, 20);
        for t in 0..20u32 {
            let rgba: Vec<u8> = (0..w * h)
                .flat_map(|i| {
                    let (x, y) = (i % w, i / w);
                    let ring = ((x as i32 - 80 - t as i32 * 3).pow(2) + (y as i32 - 48).pow(2)) < 900;
                    if ring { [230, 40, 30, 255] } else { [(x * 255 / w) as u8, (y * 255 / h) as u8, 160, 255] }
                })
                .collect();
            assert!(r.push(&rgba, w, h));
        }
        assert_eq!((r.w, r.h, r.len()), (160, 96, 20));
        let f = r.finish(30.0, "CadCraft");
        if let Ok(p) = std::env::var("CC_VIDEO_OUT") {
            std::fs::write(p, &f).unwrap();
        }
        assert!(f.len() > 1000);
    }
}
