//! CadCraft L3: WebM video from RGBA frames, in pure Rust (no codec library): each frame a VP8 key frame
//! ([`vp8::key_frame`], RFC 6386), the frames in a WebM file ([`webm::mux`]). ModelCraft's scenes recorded as an
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

/// A video being recorded: frames of one size, coded as they come.
#[derive(Debug)]
pub struct Recorder {
    pub w: u32,
    pub h: u32,
    pub q: u8,
    frames: Vec<Vec<u8>>,
}

impl Recorder {
    /// A recorder of `w` × `h` frames (even sizes, as the page's canvas: `& ~1`) at quantizer index `q`.
    pub fn new(w: u32, h: u32, q: u8) -> Recorder {
        Recorder { w: (w & !1).max(2), h: (h & !1).max(2), q, frames: Vec::new() }
    }
    /// One RGBA frame (rows top to bottom; a different size is refused) → whether it was taken.
    pub fn push(&mut self, rgba: &[u8], w: u32, h: u32) -> bool {
        if self.frames.len() >= MAX_FRAMES {
            return false;
        }
        let frame = if (w, h) == (self.w, self.h) {
            vp8::key_frame(rgba, w, h, self.q)
        } else if w >= self.w && h >= self.h {
            // the view's odd last row or column left out
            let mut c = Vec::with_capacity((self.w * self.h * 4) as usize);
            for y in 0..self.h as usize {
                let o = y * w as usize * 4;
                c.extend_from_slice(rgba.get(o..o + self.w as usize * 4).unwrap_or(&[]));
            }
            vp8::key_frame(&c, self.w, self.h, self.q)
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
        eprintln!("1280x720: {} ms, {} KB", t.elapsed().as_millis(), f.len() / 1024);
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
