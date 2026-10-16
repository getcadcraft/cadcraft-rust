//! ModelCraft's tools as state machines (tools.js, one for one). The page feeds pointer events and keys in; the tools
//! edit the model through the kernel's history API exactly as the JS does — `Model::change` for one-step edits,
//! `begin` / `preview` / `commit` / `abort` for live gestures (one undo step each) — and describe what to draw as an
//! [`Overlay`]. A [`Host`] stands for the page: the model, the camera, the open context (`path`), the selection, and
//! what the tools tell it (overlay, tip, value-box label and text, messages).
//!
//! Clicks: press–release in place = a click; press, drag, release = two clicks (both styles draw). Arrow keys lock the
//! red (→), green (←) and blue (↑) axis, ↓ the last edge's direction; Shift holds the current inference. `snapping`
//! false turns off point / edge / axis snapping; `collide` true stops Move and Push/pull where solids touch.
//!
//! Where the JS reads the clock (`ev.time ?? Date.now()` for double clicks) an event without a time counts as a second
//! after the last press (never a double click): commands stay deterministic.

mod draw;
mod edit;
mod measure;
mod misc;
mod select;
mod transform;

use std::rc::Rc;

use cc_m3d::model::{Ctx, Model};
use cc_m3d::ops::Sel;
use cc_m3d::vec::{IDENT, M4, P3, Plane, add, apply, apply_dir, dot, mmul, mul, norm, sub};
use serde_json::Value as Json;

use crate::camera::Camera;
use crate::i18n::{t as tr, t0};
use crate::infer::{Axes3, InferOpts, SkipFn, Snap, infer, pick, pick_dim, pick_section};
use crate::js::{hypot2, jmax, jmin};
use crate::overlay::Overlay;
use crate::scene::{LocalCache, Scene};
use crate::units::{Value, format_length, parse_value};

pub use draw::{ArcCenterTool, ArcTool, CircleTool, LineTool, RectTool};
pub use edit::{EraseTool, FollowTool, OffsetTool, PaintTool, PushTool};
pub use measure::{DimTool, ProtractorTool, TapeTool};
pub use misc::{AxesTool, CamTool, CurveTool, LightTool, PasteTool, PickArg, PickTool, PlaceTool, SculptTool, SectionTool};
pub use select::SelectTool;
pub use transform::{MoveTool, RotateTool, ScaleTool};

/// A kernel or model error, or a message the JS would throw.
pub type Res<T> = cc_m3d::Res<T>;

/// Pixels a press must travel to count as a drag.
pub const DRAG: f64 = 6.0;

/// Tool names, in the order of `TOOL_NAMES`.
pub const TOOL_LIST: [&str; 29] = [
    "select",
    "line",
    "rect",
    "circle",
    "polygon",
    "arc",
    "arcc",
    "curve",
    "follow",
    "offset",
    "paint",
    "push",
    "move",
    "rotate",
    "scale",
    "erase",
    "tape",
    "protractor",
    "dim",
    "section",
    "orbit",
    "pan",
    "zoom",
    "place",
    "paste",
    "pick",
    "axes",
    "sculpt",
    "light",
];
/// `TOOL_NAMES[k]`: a tool's name in the page's language.
pub fn tool_name(k: &str) -> String {
    t0(&format!("m3d.toolname.{k}"))
}
/// `TOOL_KEYS`: single-key shortcuts (lower case; " " = Space).
pub fn tool_key(k: &str) -> Option<&'static str> {
    Some(match k {
        " " => "select",
        "l" => "line",
        "r" => "rect",
        "c" => "circle",
        "n" => "polygon",
        "a" => "arc",
        "f" => "offset",
        "b" => "paint",
        "p" => "push",
        "m" => "move",
        "q" => "rotate",
        "s" => "scale",
        "e" => "erase",
        "t" => "tape",
        "d" => "dim",
        "o" => "orbit",
        "h" => "pan",
        "z" => "zoom",
        _ => return None,
    })
}

/// A pointer event (CSS pixels in the drawing area) with what the Tools add (`count`, `dragging`, `dragged`, `from`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ev {
    pub x: f64,
    pub y: f64,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub time: Option<f64>,
    pub count: f64,
    pub dragging: bool,
    pub dragged: bool,
    pub from: Option<(f64, f64)>,
}
impl Ev {
    pub fn at(x: f64, y: f64) -> Ev {
        Ev { x, y, ..Ev::default() }
    }
}

/// A locked direction: an arrow key or Shift.
#[derive(Clone, Debug, PartialEq)]
pub struct Lock {
    pub kind: String,
    pub dir: P3,
    pub shift: bool,
}

/// What a tool's `value()` returned: false, true, or undefined (which the Tools count as used).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ret {
    False,
    True,
    Undef,
}
impl Ret {
    pub fn of(b: bool) -> Ret {
        if b { Ret::True } else { Ret::False }
    }
}

/// The page: model, camera, open context, selection, and what the tools say.
#[derive(Debug)]
pub struct Host {
    pub model: Model,
    pub cam: Camera,
    /// the open context: instance ids from the root
    pub path: Vec<f64>,
    /// the selection in the open context (t: e f i g d s)
    pub sel: Vec<Sel>,
    pub units: String,
    /// the last overlay asked for (None = nothing)
    pub overlay: Option<Overlay>,
    pub tip: String,
    pub vcb: (String, String),
    pub msgs: Vec<String>,
    /// the colour the Paint tool picked up (Alt+click): `paintChanged(c)`
    pub paint_picked: Option<Option<String>>,
    /// the lamp the Light tool placed: `lightPlaced(id)`
    pub light_placed: Option<f64>,
    /// how many times `changed()` was called
    pub changed: u64,
    scene: Option<Rc<Scene>>,
    scene_gen: u64,
    model_gen: u64,
    cache: LocalCache,
}

impl Host {
    pub fn new(model: Model, cam: Camera) -> Host {
        let units = model.units.clone();
        Host {
            model,
            cam,
            path: Vec::new(),
            sel: Vec::new(),
            units,
            overlay: None,
            tip: String::new(),
            vcb: (String::new(), String::new()),
            msgs: Vec::new(),
            paint_picked: None,
            light_placed: None,
            changed: 0,
            scene: None,
            scene_gen: u64::MAX,
            model_gen: 0,
            cache: LocalCache::new(),
        }
    }
    /// A different model (another tab): the scene cache starts over.
    pub fn set_model(&mut self, m: Model) {
        self.model = m;
        self.model_gen = self.model_gen.wrapping_add(1);
        self.scene = None;
        self.cache.clear();
    }
    /// Which model this is (it changes when `set_model` gives the host another one).
    pub fn model_gen(&self) -> u64 {
        self.model_gen
    }
    /// `scene()`: sceneOf(model), cached per model.version.
    pub fn scene(&mut self) -> Rc<Scene> {
        if let Some(s) = &self.scene
            && s.version == self.model.version()
            && self.scene_gen == self.model_gen
        {
            return s.clone();
        }
        let s = Rc::new(Scene::of(&self.model, &mut self.cache));
        self.scene = Some(s.clone());
        self.scene_gen = self.model_gen;
        s
    }
    pub fn set_sel(&mut self, s: Vec<Sel>) {
        self.sel = s;
    }
    pub fn overlay(&mut self, o: Option<Overlay>) {
        self.overlay = o;
    }
    pub fn tip(&mut self, s: String) {
        self.tip = s;
    }
    pub fn vcb(&mut self, label: String, text: String) {
        self.vcb = (label, text);
    }
    pub fn print(&mut self, m: String) {
        self.msgs.push(m);
    }
    pub fn changed(&mut self) {
        self.changed = self.changed.wrapping_add(1);
    }
    pub fn open_context(&mut self, p: Vec<f64>) {
        self.path = p;
        self.sel = Vec::new();
    }
    pub fn close_context(&mut self) {
        self.path.pop();
        self.sel = Vec::new();
    }
    /// `formatLength(m, units)`.
    pub fn fmt(&self, m: f64) -> String {
        format_length(m, &self.units)
    }
}

/// The open context as the tools use it: (definition id, world matrix, inverse).
pub fn ctx_of(model: &Model, path: &[f64]) -> Ctx {
    model.context(path).or_else(|| model.context(&[])).unwrap_or(Ctx { def: "root".into(), m: IDENT, inv: IDENT })
}

/// The drawing axes in the open context, with the origin.
#[derive(Clone, Copy, Debug)]
pub struct AxesO {
    pub x: P3,
    pub y: P3,
    pub z: P3,
    pub o: P3,
}

/// What `T.snap(ev, opts)` takes.
#[derive(Default)]
pub struct SnapOpts<'a> {
    pub scene: Option<Rc<Scene>>,
    pub from: Option<P3>,
    pub plane: Option<Plane>,
    pub skip: Option<&'a SkipFn<'a>>,
}

/// Work queued for after the current event (the JS's queueMicrotask).
#[derive(Clone, Debug)]
enum Micro {
    FollowRun(f64, Vec<f64>),
}

/// The last copy (Ctrl+move, Shift+D): what was copied and how far, for `3x` / `/3`.
#[derive(Clone, Debug)]
pub struct LastCopy {
    pub delta: P3,
    pub src: Vec<Sel>,
}

/// The tool in use.
#[derive(Debug)]
pub enum Tool {
    /// taken out while one of its methods runs
    Detached,
    Select(SelectTool),
    Line(LineTool),
    Rect(RectTool),
    Circle(CircleTool),
    Arc(ArcTool),
    Arcc(ArcCenterTool),
    Curve(CurveTool),
    Follow(FollowTool),
    Offset(OffsetTool),
    Paint(PaintTool),
    Push(PushTool),
    Move(MoveTool),
    Rotate(RotateTool),
    Scale(ScaleTool),
    Erase(EraseTool),
    Tape(TapeTool),
    Protractor(ProtractorTool),
    Dim(DimTool),
    Section(SectionTool),
    Cam(CamTool),
    Place(PlaceTool),
    Paste(PasteTool),
    Pick(PickTool),
    Axes(AxesTool),
    Sculpt(SculptTool),
    Light(LightTool),
}

/// The argument a tool may be started with.
#[derive(Clone, Debug, Default)]
pub enum ToolArg {
    #[default]
    None,
    /// Curve: edit this curve
    CurveEdit(String),
    /// Place: a definition, aligned to the face under the pointer or not, with an optional hint
    Place { def_id: String, align: bool, hint: Option<String> },
    /// Light: the lamp's settings (`kind` and defaults), as JSON
    Light(Json),
    /// Paste: the clipboard (model.copyClip's JSON)
    Paste(Json),
    /// Pick: what the app asks for
    Pick(PickArg),
}

macro_rules! each {
    ($tool:expr, $t:ident => $body:expr, $none:expr) => {
        match $tool {
            Tool::Detached => $none,
            Tool::Select($t) => $body,
            Tool::Line($t) => $body,
            Tool::Rect($t) => $body,
            Tool::Circle($t) => $body,
            Tool::Arc($t) => $body,
            Tool::Arcc($t) => $body,
            Tool::Curve($t) => $body,
            Tool::Follow($t) => $body,
            Tool::Offset($t) => $body,
            Tool::Paint($t) => $body,
            Tool::Push($t) => $body,
            Tool::Move($t) => $body,
            Tool::Rotate($t) => $body,
            Tool::Scale($t) => $body,
            Tool::Erase($t) => $body,
            Tool::Tape($t) => $body,
            Tool::Protractor($t) => $body,
            Tool::Dim($t) => $body,
            Tool::Section($t) => $body,
            Tool::Cam($t) => $body,
            Tool::Place($t) => $body,
            Tool::Paste($t) => $body,
            Tool::Pick($t) => $body,
            Tool::Axes($t) => $body,
            Tool::Sculpt($t) => $body,
            Tool::Light($t) => $body,
        }
    };
}

/// What every tool shares (the JS base class's fields).
#[derive(Clone, Debug, Default)]
pub struct Base {
    pub name: String,
    pub busy: bool,
    pub modal: bool,
    pub last_ev: Option<Ev>,
    pub cur: Option<Snap>,
    pub last_dir: Option<P3>,
    pub last_kind: Option<String>,
}
impl Base {
    pub fn new(name: &str) -> Base {
        Base { name: name.to_string(), ..Base::default() }
    }
}

/// The behaviour every tool implements (the JS class methods; defaults are the base class's).
pub trait ToolImpl {
    fn base(&self) -> &Base;
    fn base_mut(&mut self) -> &mut Base;
    fn hint(&self, _t: &Tools) -> String {
        String::new()
    }
    fn vcb_label(&self, _t: &Tools) -> String {
        String::new()
    }
    fn lockable(&self) -> bool {
        false
    }
    fn down(&mut self, _t: &mut Tools, _ev: &Ev) -> Res<()> {
        Ok(())
    }
    fn mv(&mut self, _t: &mut Tools, _ev: &Ev) -> Res<()> {
        Ok(())
    }
    fn up(&mut self, _t: &mut Tools, _ev: &Ev) -> Res<()> {
        Ok(())
    }
    fn key(&mut self, _t: &mut Tools, _k: &str) -> Res<bool> {
        Ok(false)
    }
    fn value(&mut self, _t: &mut Tools, _v: &Value, _text: &str) -> Res<Ret> {
        Ok(Ret::False)
    }
    fn cancel(&mut self, _t: &mut Tools) -> Res<()> {
        self.base_mut().busy = false;
        Ok(())
    }
    /// `reset?.()`: false when the tool has none
    fn reset(&mut self, _t: &mut Tools) -> bool {
        false
    }
    /// `relock?.()`
    fn relock(&mut self, _t: &mut Tools) -> Res<()> {
        Ok(())
    }
    /// `escapeStep?.()`
    fn escape_step(&mut self, _t: &mut Tools) -> Res<bool> {
        Ok(false)
    }
    /// A tool method called by name (tests and the app: finish, back, run, begin, apply, start …): None = no such
    /// method.
    fn call(&mut self, _t: &mut Tools, _name: &str, _args: &[Json]) -> Res<Option<Json>> {
        Ok(None)
    }
    /// What the tool keeps that the harness compares (beyond the base fields).
    fn state(&self) -> Json {
        Json::Null
    }
}

/// ClickTool: press = a click, and a drag released away from the press = a second click.
pub trait ClickTool: ToolImpl {
    fn hover(&mut self, t: &mut Tools, ev: &Ev) -> Res<()>;
    fn pick_point(&mut self, t: &mut Tools, ev: &Ev) -> Res<()>;
}

/// The default ClickTool event methods.
macro_rules! click_tool_events {
    () => {
        fn down(&mut self, t: &mut $crate::tools::Tools, ev: &$crate::tools::Ev) -> $crate::tools::Res<()> {
            $crate::tools::ClickTool::pick_point(self, t, ev)
        }
        fn up(&mut self, t: &mut $crate::tools::Tools, ev: &$crate::tools::Ev) -> $crate::tools::Res<()> {
            if ev.dragged && self.base().busy {
                $crate::tools::ClickTool::pick_point(self, t, ev)?;
            }
            Ok(())
        }
        fn mv(&mut self, t: &mut $crate::tools::Tools, ev: &$crate::tools::Ev) -> $crate::tools::Res<()> {
            $crate::tools::ClickTool::hover(self, t, ev)
        }
        fn relock(&mut self, t: &mut $crate::tools::Tools) -> $crate::tools::Res<()> {
            if let Some(ev) = self.base().last_ev {
                $crate::tools::ClickTool::hover(self, t, &ev)?;
            }
            Ok(())
        }
    };
}
pub(crate) use click_tool_events;

impl Tool {
    fn imp(&self) -> Option<&dyn ToolImpl> {
        each!(self, x => Some(x as &dyn ToolImpl), None)
    }
    fn imp_mut(&mut self) -> Option<&mut dyn ToolImpl> {
        each!(self, x => Some(x as &mut dyn ToolImpl), None)
    }
}

/// The tools.
#[derive(Debug)]
pub struct Tools {
    pub host: Host,
    pub name: String,
    pub tool: Tool,
    pub lock: Option<Lock>,
    /// the last edge hovered (parallel / perpendicular)
    pub ref_edge: Option<(P3, P3)>,
    pub down_at: Option<(f64, f64)>,
    pub last_down: Option<(f64, f64, f64)>,
    pub clicks: f64,
    pub snapping: bool,
    pub collide: bool,
    /// the Paint tool's colour (None = the default)
    pub paint_color: Option<String>,
    /// on a touch screen, snapping and picking reach further
    pub touch: bool,
    pub last_copy: Option<LastCopy>,
    /// the Offset tool's last distance (a double-click repeats it)
    pub last_offset: Option<f64>,
    /// set() ran while the tool was detached: the detached tool is cancelled when its method returns
    replaced: bool,
    micro: Vec<Micro>,
    clock: f64,
}

impl Tools {
    pub fn new(host: Host) -> Res<Tools> {
        let mut t = Tools {
            host,
            name: String::new(),
            tool: Tool::Detached,
            lock: None,
            ref_edge: None,
            down_at: None,
            last_down: None,
            clicks: 0.0,
            snapping: true,
            collide: false,
            paint_color: Some("#c0392b".into()),
            touch: false,
            last_copy: None,
            last_offset: None,
            replaced: false,
            micro: Vec::new(),
            clock: 0.0,
        };
        t.set("select", ToolArg::None)?;
        Ok(t)
    }

    /// Run `f` on the current tool, taken out of `self` (so it can reach the Tools and the host).
    pub fn with_tool<R>(&mut self, f: impl FnOnce(&mut dyn ToolImpl, &mut Tools) -> Res<R>, none: R) -> Res<R> {
        let mut tl = std::mem::replace(&mut self.tool, Tool::Detached);
        let saved = std::mem::replace(&mut self.replaced, false);
        let r = match tl.imp_mut() {
            Some(x) => f(x, self),
            None => Ok(none),
        };
        let replaced = std::mem::replace(&mut self.replaced, saved);
        if replaced {
            // set() ran inside: the old tool was cancelled there in the JS (only its own fields change)
            if let Some(x) = tl.imp_mut() {
                let mut tmp = std::mem::replace(&mut self.tool, Tool::Detached);
                let c = x.cancel(self);
                std::mem::swap(&mut self.tool, &mut tmp);
                c?;
            }
        } else {
            self.tool = tl;
        }
        r
    }

    /// The tool's own hint (its status line).
    pub fn hint(&self) -> String {
        self.tool.imp().map(|x| x.hint(self)).unwrap_or_default()
    }
    pub fn vcb_label(&self) -> String {
        self.tool.imp().map(|x| x.vcb_label(self)).unwrap_or_default()
    }
    pub fn busy(&self) -> bool {
        self.tool.imp().is_some_and(|x| x.base().busy)
    }
    pub fn modal(&self) -> bool {
        self.tool.imp().is_some_and(|x| x.base().modal)
    }
    /// The tool's last snap (`tool.cur`).
    pub fn cur(&self) -> Option<&Snap> {
        self.tool.imp().and_then(|x| x.base().cur.as_ref())
    }
    /// The tool's own state for the harness.
    pub fn tool_state(&self) -> Json {
        self.tool.imp().map_or(Json::Null, |x| x.state())
    }

    /// `set(name, arg)`: cancel the tool in use and start another.
    pub fn set(&mut self, name: &str, arg: ToolArg) -> Res<()> {
        if matches!(self.tool, Tool::Detached) {
            self.replaced = true;
        } else {
            self.with_tool(|x, t| x.cancel(t), ())?;
        }
        self.name = name.to_string();
        self.lock = None;
        let tool = self.make(name, arg)?;
        self.tool = tool;
        self.host.overlay(None);
        let (l, h) = (self.vcb_label(), self.hint());
        self.host.vcb(l, String::new());
        self.host.tip(h);
        Ok(())
    }

    fn make(&mut self, name: &str, arg: ToolArg) -> Res<Tool> {
        Ok(match name {
            "line" => Tool::Line(LineTool::new(name)),
            "rect" => Tool::Rect(RectTool::new(name)),
            "circle" | "polygon" => Tool::Circle(CircleTool::new(name)),
            "arc" => Tool::Arc(ArcTool::new(name)),
            "arcc" => Tool::Arcc(ArcCenterTool::new(name)),
            "curve" => Tool::Curve(CurveTool::new(self, name, arg)?),
            "follow" => Tool::Follow(FollowTool::new(self, name)),
            "offset" => Tool::Offset(OffsetTool::new(self, name)?),
            "paint" => Tool::Paint(PaintTool::new(name)),
            "push" => Tool::Push(PushTool::new(name)),
            "move" => Tool::Move(MoveTool::new(name)),
            "rotate" => Tool::Rotate(RotateTool::new(name)),
            "scale" => Tool::Scale(ScaleTool::new(self, name)),
            "erase" => Tool::Erase(EraseTool::new(name)),
            "tape" => Tool::Tape(TapeTool::new(name)),
            "protractor" => Tool::Protractor(ProtractorTool::new(name)),
            "dim" => Tool::Dim(DimTool::new(name)),
            "section" => Tool::Section(SectionTool::new(name)),
            "orbit" | "pan" | "zoom" => Tool::Cam(CamTool::new(name)),
            "place" => Tool::Place(PlaceTool::new(name, arg)),
            "paste" => Tool::Paste(PasteTool::new(name, arg)),
            "pick" => Tool::Pick(PickTool::new(name, arg)),
            "axes" => Tool::Axes(AxesTool::new(name)),
            "sculpt" => Tool::Sculpt(SculptTool::new(name)),
            "light" => Tool::Light(LightTool::new(name, arg)),
            _ => Tool::Select(SelectTool::new(name)),
        })
    }

    /// Run the work queued during an event (the JS's microtasks).
    pub fn flush(&mut self) -> Res<()> {
        while !self.micro.is_empty() {
            let list = std::mem::take(&mut self.micro);
            for m in list {
                match m {
                    Micro::FollowRun(fid, path) => {
                        self.with_tool(
                            |x, t| {
                                x.call(
                                    t,
                                    "run",
                                    &[cc_m3d::file::jn(fid), Json::Array(path.iter().map(|v| cc_m3d::file::jn(*v)).collect())],
                                )?;
                                Ok(())
                            },
                            (),
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    // ---- events ----

    pub fn down(&mut self, ev: &Ev) -> Res<()> {
        let now = match ev.time {
            Some(x) => x,
            None => self.last_down.map_or(self.clock, |l| l.2) + 1000.0,
        };
        self.clock = now;
        match self.last_down {
            Some((lx, ly, lt)) if now - lt < 450.0 && hypot2(ev.x - lx, ev.y - ly) < DRAG => self.clicks += 1.0,
            _ => self.clicks = 1.0,
        }
        self.last_down = Some((ev.x, ev.y, now));
        self.down_at = Some((ev.x, ev.y));
        let e = Ev { count: self.clicks, ..*ev };
        self.with_tool(|x, t| x.down(t, &e), ())
    }
    pub fn mv(&mut self, ev: &Ev) -> Res<()> {
        let dragging = self.down_at.is_some_and(|(x, y)| hypot2(ev.x - x, ev.y - y) >= DRAG);
        let e = Ev { dragging, from: self.down_at, ..*ev };
        self.with_tool(|x, t| x.mv(t, &e), ())
    }
    pub fn up(&mut self, ev: &Ev) -> Res<()> {
        let dragged = self.down_at.is_some_and(|(x, y)| hypot2(ev.x - x, ev.y - y) >= DRAG);
        let from = self.down_at.take();
        let e = Ev { dragged, from, count: self.clicks, ..*ev };
        self.with_tool(|x, t| x.up(t, &e), ())
    }
    /// A click (down + up in place).
    pub fn click(&mut self, ev: &Ev) -> Res<()> {
        self.mv(ev)?;
        self.down(ev)?;
        self.up(ev)
    }
    pub fn key(&mut self, k: &str) -> Res<bool> {
        let ax = self.axes();
        if matches!(k, "ArrowRight" | "ArrowLeft" | "ArrowUp" | "ArrowDown") {
            let want: Option<(&str, P3)> = match k {
                "ArrowRight" => Some(("axis-x", ax.x)),
                "ArrowLeft" => Some(("axis-y", ax.y)),
                "ArrowUp" => Some(("axis-z", ax.z)),
                _ => self.ref_edge.map(|(a, b)| ("parallel", norm(sub(b, a)))),
            };
            let lockable = self.tool.imp().is_some_and(ToolImpl::lockable);
            let Some((kind, dir)) = want.filter(|_| lockable) else { return Ok(false) };
            self.lock = if self.lock.as_ref().is_some_and(|l| l.kind == kind) {
                None
            } else {
                Some(Lock { kind: kind.to_string(), dir, shift: false })
            };
            self.with_tool(|x, t| x.relock(t), ())?;
            return Ok(true);
        }
        if k == "Shift" && self.lock.is_none() {
            let (lockable, dir, kind) =
                self.tool.imp().map_or((false, None, None), |x| (x.lockable(), x.base().last_dir, x.base().last_kind.clone()));
            if lockable && let Some(dir) = dir {
                self.lock = Some(Lock { kind: kind.unwrap_or_default(), dir, shift: true });
                return Ok(true);
            }
        }
        self.with_tool(|x, t| x.key(t, k), false)
    }
    pub fn key_up(&mut self, k: &str) -> Res<()> {
        if k == "Shift" && self.lock.as_ref().is_some_and(|l| l.shift) {
            self.lock = None;
            self.with_tool(|x, t| x.relock(t), ())?;
        }
        Ok(())
    }
    /// The value box: a typed length, list, point, offset, count or factor.
    pub fn value(&mut self, text: &str) -> Res<bool> {
        let Some(v) = parse_value(text, &self.host.units.clone()) else {
            self.host.print(t0("m3d.tool.type_a_length"));
            return Ok(false);
        };
        let r = self.with_tool(|x, t| x.value(t, &v, text), Ret::False)?;
        Ok(r != Ret::False)
    }
    pub fn escape(&mut self) -> Res<bool> {
        if self.with_tool(|x, t| x.escape_step(t), false)? {
            return Ok(true);
        }
        if self.busy() {
            let back = self.modal();
            self.with_tool(
                |x, t| {
                    x.cancel(t)?;
                    x.reset(t);
                    Ok(())
                },
                (),
            )?;
            self.host.overlay(None);
            let (l, h) = (self.vcb_label(), self.hint());
            self.host.vcb(l, String::new());
            self.host.tip(h);
            if back {
                self.set("select", ToolArg::None)?;
            }
            return Ok(true);
        }
        Ok(false)
    }
    /// A tool method by name (None: the tool has none).
    pub fn call(&mut self, name: &str, args: &[Json]) -> Res<Option<Json>> {
        self.with_tool(|x, t| x.call(t, name, args), None)
    }

    // ---- helpers for tools ----

    /// The origin to snap to and the ground to draw on: the model's moved axes at the top level.
    pub fn ground_opts(&self) -> (Option<P3>, Option<Plane>) {
        if self.host.model.axes_m.is_none() || !self.host.path.is_empty() {
            return (None, None);
        }
        let a = self.axes();
        (Some(a.o), Some(Plane { n: a.z, d: dot(a.z, a.o) }))
    }
    pub fn ctx(&self) -> Ctx {
        ctx_of(&self.host.model, &self.host.path)
    }
    /// The drawing axes in the open context.
    pub fn axes(&self) -> AxesO {
        let m = self.host.model.axes_frame(&self.host.path);
        AxesO {
            x: norm(apply_dir(&m, [1.0, 0.0, 0.0])),
            y: norm(apply_dir(&m, [0.0, 1.0, 0.0])),
            z: norm(apply_dir(&m, [0.0, 0.0, 1.0])),
            o: apply(&m, [0.0, 0.0, 0.0]),
        }
    }
    pub fn snap(&mut self, ev: &Ev, opts: SnapOpts<'_>) -> Snap {
        let sc = opts.scene.clone().unwrap_or_else(|| self.host.scene());
        let ax = self.axes();
        let (origin, ground) = self.ground_opts();
        let o = InferOpts {
            radius: Some(if self.touch { 22.0 } else { 12.0 }),
            axes: Some(Axes3 { x: ax.x, y: ax.y, z: ax.z }),
            skip: opts.skip,
            snap: self.snapping,
            from: opts.from,
            lock_dir: if opts.from.is_some() { self.lock.as_ref().map(|l| l.dir) } else { None },
            lock_kind: self.lock.as_ref().map(|l| l.kind.clone()),
            ref_edge: self.ref_edge,
            plane: opts.plane,
            origin,
            ground,
        };
        let r = infer(&sc, &self.host.cam, ev.x, ev.y, &o);
        if let Some(e) = &r.edge
            && opts.from.is_none()
        {
            self.ref_edge = Some((e.a, e.b));
        }
        r
    }
    fn has_geometry_sel(&self) -> bool {
        self.host.sel.iter().any(|s| s.t != "g" && s.t != "d" && s.t != "s")
    }
    /// Start moving (or copying) the selection from the cursor at once.
    pub fn grab(&mut self, ev: &Ev, copy: bool) -> Res<bool> {
        if !self.has_geometry_sel() {
            self.host.print(t0("m3d.tool.select_something_first"));
            return Ok(false);
        }
        self.set("move", ToolArg::None)?;
        self.with_tool(|x, t| x.call(t, "start", &[ev_json(ev), Json::Bool(copy)]).map(|_| ()), ())?;
        Ok(true)
    }
    /// Rotate the selection about its centre, from the cursor.
    pub fn turn(&mut self, ev: &Ev) -> Res<bool> {
        let c = if self.has_geometry_sel() { self.sel_center() } else { None };
        let Some(c) = c else {
            self.host.print(t0("m3d.tool.select_something_first"));
            return Ok(false);
        };
        self.set("rotate", ToolArg::None)?;
        self.with_tool(|x, t| x.call(t, "startAt", &[crate::outj::p3(&c), ev_json(ev)]).map(|_| ()), ())?;
        Ok(true)
    }
    /// Scale the selection about its centre; the cursor's distance from it is 1×.
    pub fn resize(&mut self, ev: &Ev) -> Res<bool> {
        let c = if self.has_geometry_sel() { self.sel_center() } else { None };
        let Some(c) = c else {
            self.host.print(t0("m3d.tool.select_something_first"));
            return Ok(false);
        };
        self.set("scale", ToolArg::None)?;
        self.with_tool(|x, t| x.call(t, "startAt", &[crate::outj::p3(&c), ev_json(ev)]).map(|_| ()), ())?;
        Ok(true)
    }
    pub fn sel_center(&self) -> Option<P3> {
        self.sel_box().map(|(lo, hi)| mul(add(lo, hi), 0.5))
    }
    /// The selection's bounding box in world coordinates.
    pub fn sel_box(&self) -> Option<(P3, P3)> {
        let m = &self.host.model;
        let c = self.ctx();
        let def = m.def(&c.def)?;
        let mut bb: Option<(P3, P3)> = None;
        let mut put = |p: P3| {
            let b = bb.get_or_insert((p, p));
            b.0 = [jmin(b.0[0], p[0]), jmin(b.0[1], p[1]), jmin(b.0[2], p[2])];
            b.1 = [jmax(b.1[0], p[0]), jmax(b.1[1], p[1]), jmax(b.1[2], p[2])];
        };
        for s in &self.host.sel {
            match s.t.as_str() {
                "e" => {
                    if let Some(e) = def.edges.get(s.id) {
                        put(apply(&c.m, def.v(e.a)));
                        put(apply(&c.m, def.v(e.b)));
                    }
                }
                "f" => {
                    if let Some(f) = def.faces.get(s.id) {
                        for l in &f.loops {
                            for v in l {
                                put(apply(&c.m, def.v(*v)));
                            }
                        }
                    }
                }
                "i" => {
                    if let Some(inst) = def.insts.get(s.id)
                        && m.def(&inst.def).is_some()
                        && let Some(ib) = m.bbox(&inst.def, &mmul(&c.m, &inst.m))
                    {
                        put(ib.min);
                        put(ib.max);
                    }
                }
                _ => {}
            }
        }
        for s in &self.host.sel {
            if s.t == "g"
                && let Some(g) = m.guides.get(s.id)
            {
                put(match g {
                    cc_m3d::model::Guide::Line { p, .. } | cc_m3d::model::Guide::Point { p } => *p,
                });
            }
        }
        bb
    }
    /// What a click picks: a dimension first, then a section plane, then geometry and guides.
    pub fn pick_at(&mut self, ev: &Ev) -> Option<Sel> {
        let units = self.host.units.clone();
        let fmt = move |m: f64| format_length(m, &units);
        let cam = self.host.cam.clone();
        let r = if self.touch { 14.0 } else { 6.0 };
        if let Some(s) = pick_dim(&mut self.host.model, &cam, ev.x, ev.y, &fmt, r) {
            return Some(s);
        }
        if let Some(s) = pick_section(&self.host.model, &cam, ev.x, ev.y, r) {
            return Some(s);
        }
        let sc = self.host.scene();
        pick(&sc, &cam, ev.x, ev.y, &self.host.path, if self.touch { 14.0 } else { 7.0 })
    }
    /// `pick(scene, cam, x, y, {path})` with the default radius.
    pub fn pick7(&mut self, ev: &Ev) -> Option<Sel> {
        let sc = self.host.scene();
        pick(&sc, &self.host.cam, ev.x, ev.y, &self.host.path, 7.0)
    }
    /// A typed length for a lone selected dimension or edge: resize the model to it. True when used.
    pub fn resize_sel(&mut self, v: &Value) -> Res<bool> {
        let sel = self.host.sel.clone();
        let (Some(s), Value::Len { v: vv, .. }) = (sel.first(), v) else { return Ok(false) };
        if sel.len() != 1 || (s.t != "d" && s.t != "e") {
            return Ok(false);
        }
        let vv = *vv;
        if !(vv > 0.0) {
            self.host.print(t0("m3d.tool.type_a_length_above_zero"));
            return Ok(true);
        }
        let kind: Option<&str> = if s.t == "d" {
            self.host.model.dims.get(s.id).map(|d| match d {
                cc_m3d::model::Dim::Lin { .. } => "len",
                cc_m3d::model::Dim::Ang { .. } => "ang",
                cc_m3d::model::Dim::Rad { kind, .. } => {
                    if kind == "dia" {
                        "dia"
                    } else {
                        "rad"
                    }
                }
            })
        } else {
            None
        };
        if kind == Some("ang") {
            self.host.print(t0("m3d.model.dim_angle_only"));
            return Ok(true);
        }
        let label = if s.t == "d" { t0("m3d.tool.dimension_length") } else { t0("m3d.tool.edge_length") };
        let path = self.host.path.clone();
        let (st, sid) = (s.t.clone(), s.id);
        let r = self.host.model.change(&label, |mm| {
            if st == "d" { mm.set_dim_length(sid, vv).map(|_| ()) } else { mm.set_edge_length(&path, sid, vv).map(|_| ()) }
        });
        if let Err(e) = r {
            self.host.print(err_message(&e));
            return Ok(true);
        }
        let key = if st != "d" {
            "m3d.tool.edge_set"
        } else if kind == Some("rad") {
            "m3d.tool.rad_set"
        } else if kind == Some("dia") {
            "m3d.tool.dia_set"
        } else {
            "m3d.tool.dim_set"
        };
        let len = self.host.fmt(vv);
        self.host.print(tr(key, &[("len", &len)]));
        let c = self.ctx();
        let keep: Vec<Sel> = sel
            .into_iter()
            .filter(|x| {
                if x.t == "d" { self.host.model.dims.has(x.id) } else { self.host.model.def(&c.def).is_some_and(|d| d.edges.has(x.id)) }
            })
            .collect();
        self.host.set_sel(keep);
        self.host.changed();
        Ok(true)
    }
    pub fn local(&self, p: P3) -> P3 {
        apply(&self.ctx().inv, p)
    }
    pub fn local_dir(&self, d: P3) -> P3 {
        apply_dir(&self.ctx().inv, d)
    }
    pub fn world(&self, p: P3) -> P3 {
        apply(&self.ctx().m, p)
    }
    pub fn toward_eye(&self, p: P3) -> P3 {
        norm(sub(self.host.cam.eye, p))
    }
    /// What a live preview changes: only the open context's own definition.
    pub fn scope(&self) -> Option<Vec<String>> {
        self.host.model.context(&self.host.path).map(|c| vec![c.def])
    }
    /// The model's matrix of the open context.
    pub fn ctx_m(&self) -> M4 {
        self.ctx().m
    }
}

/// An error's message as the JS shows it (`err.message`).
pub fn err_message(e: &cc_m3d::Error) -> String {
    e.to_string()
}

/// An event as JSON (for tool methods called by name).
pub fn ev_json(ev: &Ev) -> Json {
    let mut o = serde_json::Map::new();
    o.insert("x".into(), crate::outj::num(ev.x));
    o.insert("y".into(), crate::outj::num(ev.y));
    if ev.shift {
        o.insert("shift".into(), Json::Bool(true));
    }
    if ev.ctrl {
        o.insert("ctrl".into(), Json::Bool(true));
    }
    if ev.alt {
        o.insert("alt".into(), Json::Bool(true));
    }
    if let Some(tm) = ev.time {
        o.insert("time".into(), crate::outj::num(tm));
    }
    Json::Object(o)
}
/// An event from JSON ({x, y, shift?, ctrl?, alt?, time?}).
pub fn ev_of(j: &Json) -> Ev {
    let num = |k: &str| j.get(k).and_then(Json::as_f64);
    let b = |k: &str| j.get(k).is_some_and(|v| cc_m3d::json::truthy(Some(v)));
    Ev {
        x: num("x").unwrap_or(f64::NAN),
        y: num("y").unwrap_or(f64::NAN),
        shift: b("shift"),
        ctrl: b("ctrl"),
        alt: b("alt"),
        time: num("time"),
        ..Ev::default()
    }
}
/// A point from JSON.
pub fn p3_of(j: Option<&Json>) -> Option<P3> {
    let a = j?.as_array()?;
    let g = |i: usize| a.get(i).and_then(Json::as_f64).unwrap_or(f64::NAN);
    Some([g(0), g(1), g(2)])
}

/// `marker(r)`.
pub fn marker(r: &Snap) -> Option<crate::overlay::Marker> {
    Some(crate::overlay::Marker { p: r.p, kind: r.kind.clone() })
}

/// `planeAxes(n, ax)` (tools.js): red/green on level planes, level/up on others.
pub fn plane_axes(n: P3, ax: &AxesO) -> (P3, P3) {
    use cc_m3d::vec::cross;
    if dot(n, ax.z).abs() > 0.999 {
        let u = ax.x;
        return (u, norm(cross(n, u)));
    }
    let u = norm(cross(ax.z, n));
    (u, cross(n, u))
}

/// `same(a, b)` for paths.
pub fn same_path(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x == y)
}
