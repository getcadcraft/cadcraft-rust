//! ModelCraft's scenes as a WebM animation (0.4 M23, modelcraft.js recordScenes): Scenes › Export animation plays the
//! scenes from the first to the last — the first held 1.5 s, then each next one reached in 2 s and held 1.5 s — and
//! saves the film as `<model>.webm`; the button again or Esc stops it ("nothing was saved"). Fewer than two scenes:
//! "Add two scenes or more …".
//!
//! 0.5 draws the view into a canvas and records it in real time with the browser's MediaRecorder (VP9 / VP8). Here each
//! frame of the film is placed exactly (30 a second: the camera on 0.5's path at that instant), rendered by the view,
//! read back from the GPU without blocking (WebGPU allows nothing else) and coded by `cc-video` (VP8: a key frame every
//! 2 s, inter frames between, in a WebM file, pure Rust); the file goes to the host's save (a save dialog on the desktop, a download on the web) as
//! 0.5's download does. The film's length is the scenes' timing, whatever the machine's speed. The view's marks are not
//! in it (0.5's presentation view hides them too).
//!
//! Actions (`model.edit=<action>`): scene-record (starts, or stops one under way).

use std::sync::{Arc, Mutex, PoisonError};

use cc_m3d::ops::Sel;
use cc_m3d_tools::Camera;

use crate::modelcraft::{ModelView, mt};
use crate::view3d::Capture;

/// Frames a second.
pub const FPS: f64 = 30.0;
/// The first scene's hold, each move, each hold after it (recordScenes: hold(1500), goScene ms 2000, hold(1500)).
const HOLD_MS: f64 = 1500.0;
const MOVE_MS: f64 = 2000.0;
/// Quantizer index of the frames (0 best … 127 smallest): sharp lines at a few MB a second.
const QUALITY: u8 = 16;

/// One frame of the film: the scene it belongs to (its style and shown things), whether that scene starts here, the
/// camera.
#[derive(Clone, Debug)]
pub struct FrameSpec {
    pub scene: usize,
    pub start: bool,
    pub cam: ([f64; 3], [f64; 3], f64, bool),
}

/// The film's coder: on the desktop a thread of its own (a frame takes tens of milliseconds), on the web in line.
enum Coder {
    Idle,
    Inline(cc_video::Recorder),
    #[cfg(not(target_arch = "wasm32"))]
    Thread(std::sync::mpsc::Sender<(u32, u32, Vec<u8>)>, std::thread::JoinHandle<Option<cc_video::Recorder>>),
}

impl Coder {
    fn push(&mut self, w: u32, h: u32, px: Vec<u8>) {
        if matches!(self, Coder::Idle) {
            *self = Coder::start(w, h);
        }
        match self {
            Coder::Idle => {}
            Coder::Inline(r) => {
                r.push(&px, w, h);
            }
            #[cfg(not(target_arch = "wasm32"))]
            Coder::Thread(tx, _) => {
                let _ = tx.send((w, h, px));
            }
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn start(w: u32, h: u32) -> Coder {
        let (tx, rx) = std::sync::mpsc::channel::<(u32, u32, Vec<u8>)>();
        let spawned = std::thread::Builder::new().name("cc-video".into()).spawn(move || {
            let mut r = cc_video::Recorder::new(w, h, QUALITY);
            while let Ok((fw, fh, px)) = rx.recv() {
                r.push(&px, fw, fh);
            }
            Some(r)
        });
        match spawned {
            Ok(j) => Coder::Thread(tx, j),
            Err(_) => Coder::Inline(cc_video::Recorder::new(w, h, QUALITY)),
        }
    }
    #[cfg(target_arch = "wasm32")]
    fn start(w: u32, h: u32) -> Coder {
        Coder::Inline(cc_video::Recorder::new(w, h, QUALITY))
    }
    /// Every frame coded: the recorder.
    fn finish(self) -> Option<cc_video::Recorder> {
        match self {
            Coder::Idle => None,
            Coder::Inline(r) => Some(r),
            #[cfg(not(target_arch = "wasm32"))]
            Coder::Thread(tx, j) => {
                drop(tx);
                j.join().ok().flatten()
            }
        }
    }
}

/// A recording under way.
pub struct Recording {
    pub frames: Vec<FrameSpec>,
    pub next: usize,
    pub waiting: bool,
    pub cap: Arc<Mutex<Capture>>,
    coder: Coder,
    pub stop: bool,
    keep_sel: Vec<Sel>,
    n: usize,
}

impl std::fmt::Debug for Recording {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Recording {}/{}", self.next, self.frames.len())
    }
}

/// The film's frames for scenes' cameras (eye, target, fov, persp), 0.5's timing at [`FPS`].
pub fn plan(cams: &[([f64; 3], [f64; 3], f64, bool)], base: &Camera) -> Vec<FrameSpec> {
    let per = |ms: f64| (ms * FPS / 1000.0).round() as usize;
    let mut out = Vec::new();
    let cam_of = |c: &([f64; 3], [f64; 3], f64, bool)| {
        let mut k = base.clone();
        k.set(c.0, c.1, Some(c.2), Some(c.3));
        k
    };
    let Some(first) = cams.first() else { return out };
    for j in 0..per(HOLD_MS) {
        out.push(FrameSpec { scene: 0, start: j == 0, cam: *first });
    }
    for (i, w) in cams.windows(2).enumerate() {
        let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
        let (from, to) = (cam_of(a), cam_of(b));
        let m = per(MOVE_MS).max(1);
        for j in 0..m {
            let k = (j + 1) as f64 / m as f64;
            out.push(FrameSpec { scene: i + 1, start: j == 0, cam: cc_m3d_tools::camera::lerp_cam(&from, &to, k) });
        }
        for _ in 0..per(HOLD_MS) {
            out.push(FrameSpec { scene: i + 1, start: false, cam: *b });
        }
    }
    out
}

impl ModelView {
    /// Scenes › Export animation: start recording, or stop the one under way.
    pub fn record_scenes(&mut self) {
        if let Some(r) = self.recording.as_mut() {
            r.stop = true;
            return;
        }
        let n = self.tools.host.model.scenes.len();
        if n < 2 {
            self.print(mt("app.scene.rec.few", &[]));
            return;
        }
        self.stop_play();
        let _ = self.tools.escape();
        let keep_sel = std::mem::take(&mut self.tools.host.sel);
        let cams: Vec<_> = self.tools.host.model.scenes.iter().map(|s| (s.eye, s.target, s.fov, s.persp)).collect();
        let frames = plan(&cams, &self.tools.host.cam);
        self.anim = None;
        self.recording = Some(Box::new(Recording {
            frames,
            next: 0,
            waiting: false,
            cap: Arc::new(Mutex::new(Capture::default())),
            coder: Coder::Idle,
            stop: false,
            keep_sel,
            n,
        }));
    }

    /// One step of the recording before the view paints: a frame read back is coded, the next one placed.
    pub fn step_record(&mut self, ctx: &egui::Context) {
        let Some(mut r) = self.recording.take() else { return };
        let (frame, failed) = {
            let mut c = r.cap.lock().unwrap_or_else(PoisonError::into_inner);
            (c.frame.take(), c.failed)
        };
        if let Some((w, h, px)) = frame {
            r.coder.push(w, h, px);
            r.next += 1;
            r.waiting = false;
        }
        if failed {
            self.end_record(*r, Some(mt("app.scene.rec.none", &[])));
            return;
        }
        if r.stop {
            self.end_record(*r, Some(mt("app.scene.rec.stopped", &[])));
            return;
        }
        if r.next >= r.frames.len() {
            self.end_record(*r, None);
            return;
        }
        if !r.waiting
            && let Some(f) = r.frames.get(r.next).cloned()
        {
            if f.start {
                // goScene(i, …, { quiet: true }): the scene's style and shown things, then the camera below
                let msg = std::mem::take(&mut self.message);
                self.go_scene_with(f.scene, false);
                self.message = msg;
                self.tools.host.sel.clear();
                self.print(mt("app.scene.rec.progress", &[("i", &(f.scene + 1).to_string()), ("n", &r.n.to_string())]));
            }
            let (w, h) = (self.tools.host.cam.w, self.tools.host.cam.h);
            self.tools.host.cam.set(f.cam.0, f.cam.1, Some(f.cam.2), Some(f.cam.3));
            self.tools.host.cam.set_size(w, h);
            r.cap.lock().unwrap_or_else(PoisonError::into_inner).want = true;
            r.waiting = true;
        }
        self.recording = Some(r);
        ctx.request_repaint();
    }

    /// The recording ends: stopped (or failed) with `msg`, else the film to the host's save.
    fn end_record(&mut self, r: Recording, msg: Option<String>) {
        let Recording { keep_sel, coder, .. } = r;
        self.tools.host.sel = keep_sel;
        let rec = coder.finish();
        if let Some(m) = msg {
            self.print(m);
            return;
        }
        let Some(rec) = rec.filter(|x| !x.is_empty()) else {
            self.print(mt("app.scene.rec.stopped", &[]));
            return;
        };
        let file = format!("{}.webm", self.base_name());
        self.pending_save = Some((file.clone(), rec.finish(FPS, "CadCraft ModelCraft")));
        // the film's length (0.5 records in real time: its seconds on the clock are the film's)
        let s = cc_m3d::js::round(rec.len() as f64 / FPS);
        self.print(mt("app.scene.rec.done", &[("file", &file), ("s", &crate::mcact::ns(s))]));
    }

    /// Whether the scenes are being recorded.
    pub fn recording_now(&self) -> bool {
        self.recording.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_film_follows_the_scenes() {
        let cams = [([10.0, 0.0, 5.0], [0.0; 3], 35.0, true), ([0.0, 10.0, 5.0], [0.0; 3], 35.0, true)];
        let f = plan(&cams, &Camera::new());
        // 1.5 s, then 2 s of moving and 1.5 s held: 5 s at 30 frames a second
        assert_eq!(f.len(), 150);
        assert!(f[0].start && f[45].start && f[45].scene == 1);
        assert_eq!(f[104].cam.0, cams[1].0, "the move ends on the scene's camera");
        assert!(plan(&[], &Camera::new()).is_empty());
    }

    #[test]
    fn two_scenes_are_needed_and_a_second_press_stops() {
        let mut v = ModelView::new(cc_m3d::model::Model::new(), "t", false).unwrap();
        v.act("scene-record").unwrap();
        assert_eq!(v.message, mt("app.scene.rec.few", &[]));
        v.act("scene-add").unwrap();
        v.act("cam:5,5,5,0,0,0").unwrap();
        v.act("scene-add").unwrap();
        v.act("scene-record").unwrap();
        assert!(v.recording_now());
        v.act("scene-record").unwrap();
        v.step_record(&egui::Context::default());
        assert!(!v.recording_now());
        assert_eq!(v.message, mt("app.scene.rec.stopped", &[]));
    }
}
