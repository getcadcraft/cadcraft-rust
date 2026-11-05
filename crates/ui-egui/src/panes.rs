//! Split screen (0.5 panes.js and view.js's pane handling; VPORTS in the model, SPLIT, LINKVIEWS): the drawing area
//! split into panes, like AutoCAD's tiled viewports. Each pane keeps its own view (centre, zoom, turn) and its own
//! space (the model or a sheet); the active one is the live view the window draws and edits with, the others are
//! drawn from their records. The panes go into the file as its VPORT table (`doc.vports`, cc-io-cad `vports`) and come
//! back from it.
//!
//! Pure of egui (paneview.rs draws them): every number is in CSS px / egui points, as view.js keeps them, so the
//! parity harness (forks/tools/panediff-cadpanes.mjs through `cadcraft-shot panecases`) compares this with view.js
//! itself. A view here is view.js's `{ cx, cy, zoom, tw }` (zoom: px per drawing unit; tw: the turn, radians,
//! view.js's sign: the window's `View2D::twist` is −tw).

use serde_json::{Map, Value, json};

/// panes.js `LAYOUTS`
pub const LAYOUTS: [&str; 6] = ["single", "2v", "2h", "3r", "3l", "4"];
/// panes.js `GAP`: the space between panes (px)
pub const GAP: f64 = 4.0;

/// `paneCount(layout)`
pub fn pane_count(layout: &str) -> usize {
    match layout {
        "2v" | "2h" => 2,
        "3r" | "3l" => 3,
        "4" => 4,
        _ => 1,
    }
}

/// `clampSplit(f)`: a divider at least 10 % from either edge (not a number: the middle).
pub fn clamp_split(f: f64) -> f64 {
    if f.is_finite() { f.clamp(0.1, 0.9) } else { 0.5 }
}

/// JS `Math.round` (halves up).
fn js_round(v: f64) -> f64 {
    (v + 0.5).floor()
}

/// A rectangle in px from the drawing area's top left.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// `paneRects(layout, fx, fy, W, H)`: the panes with a GAP between them.
pub fn pane_rects(layout: &str, fx: f64, fy: f64, w: f64, h: f64) -> Vec<Rect> {
    let (fx, fy) = (clamp_split(fx), clamp_split(fy));
    let g = GAP / 2.0;
    let (x, y) = (js_round(w * fx), js_round(h * fy));
    let r = |x: f64, y: f64, w: f64, h: f64| Rect { x, y, w: w.max(1.0), h: h.max(1.0) };
    match layout {
        "2v" => vec![r(0.0, 0.0, x - g, h), r(x + g, 0.0, w - x - g, h)],
        "2h" => vec![r(0.0, 0.0, w, y - g), r(0.0, y + g, w, h - y - g)],
        "3r" => vec![r(0.0, 0.0, x - g, y - g), r(0.0, y + g, x - g, h - y - g), r(x + g, 0.0, w - x - g, h)],
        "3l" => vec![r(0.0, 0.0, x - g, h), r(x + g, 0.0, w - x - g, y - g), r(x + g, y + g, w - x - g, h - y - g)],
        "4" => vec![
            r(0.0, 0.0, x - g, y - g),
            r(x + g, 0.0, w - x - g, y - g),
            r(0.0, y + g, x - g, h - y - g),
            r(x + g, y + g, w - x - g, h - y - g),
        ],
        _ => vec![r(0.0, 0.0, w, h)],
    }
}

/// A divider that can be dragged: which split it moves ("fx" | "fy"), vertical (true) or horizontal, where it starts
/// and how long it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Divider {
    pub key: &'static str,
    pub vertical: bool,
    pub x: f64,
    pub y: f64,
    pub len: f64,
}

/// `dividers(layout, fx, fy, W, H)`
pub fn dividers(layout: &str, fx: f64, fy: f64, w: f64, h: f64) -> Vec<Divider> {
    let (x, y) = (js_round(w * clamp_split(fx)), js_round(h * clamp_split(fy)));
    let v = |len: f64| Divider { key: "fx", vertical: true, x, y: 0.0, len };
    let hz = |x0: f64, len: f64| Divider { key: "fy", vertical: false, x: x0, y, len };
    match layout {
        "2v" => vec![v(h)],
        "2h" => vec![hz(0.0, w)],
        "3r" => vec![v(h), hz(0.0, x)],
        "3l" => vec![v(h), hz(x, w - x)],
        "4" => vec![v(h), hz(0.0, w)],
        _ => Vec::new(),
    }
}

/// `paneAt(rects, x, y)`: the pane holding a point.
pub fn pane_at(rects: &[Rect], x: f64, y: f64) -> Option<usize> {
    rects.iter().position(|r| x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h)
}

/// view.js's `vs`: a view's centre, zoom (px per unit) and turn (its sines as V8's `Math.sin` / `Math.cos`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vs {
    pub cx: f64,
    pub cy: f64,
    pub zoom: f64,
    pub tw: f64,
}
impl Default for Vs {
    fn default() -> Self {
        Vs { cx: 0.0, cy: 0.0, zoom: 1.0, tw: 0.0 }
    }
}
impl Vs {
    /// view.js `zoomAt(sx, sy, f)` in a pane `w` × `h`.
    pub fn zoom_at(&mut self, w: f64, h: f64, sx: f64, sy: f64, f: f64) {
        let before = self.to_world(w, h, sx, sy);
        self.zoom = (self.zoom * f).clamp(1e-9, 1e9);
        let after = self.to_world(w, h, sx, sy);
        self.cx += before.0 - after.0;
        self.cy += before.1 - after.1;
    }
    /// view.js `panBy(dx, dy)`.
    pub fn pan_by(&mut self, dx: f64, dy: f64) {
        let (u, v) = (dx / self.zoom, -dy / self.zoom);
        let (x, y) = if self.tw == 0.0 {
            (u, v)
        } else {
            let (s, c) = (cc_m3d::jsmath::fd_sin(self.tw), cc_m3d::jsmath::fd_cos(self.tw));
            (c * u - s * v, s * u + c * v)
        };
        self.cx -= x;
        self.cy -= y;
    }
    /// view.js `toWorld(sx, sy)` in a pane `w` × `h`.
    pub fn to_world(&self, w: f64, h: f64, sx: f64, sy: f64) -> (f64, f64) {
        let u = (sx - w / 2.0) / self.zoom;
        let v = (h / 2.0 - sy) / self.zoom;
        if self.tw == 0.0 {
            return (u + self.cx, v + self.cy);
        }
        let (s, c) = (cc_m3d::jsmath::fd_sin(self.tw), cc_m3d::jsmath::fd_cos(self.tw));
        (self.cx + c * u - s * v, self.cy + s * u + c * v)
    }
}

/// One pane: the space it shows (0 = the model, then the sheets in tab order), its view and its rectangle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pane {
    pub space: usize,
    pub vs: Vs,
    pub rect: Option<Rect>,
    /// the open drawing it shows (doctabs.js `p.docId`: Tile); None = the window's open one
    pub doc: Option<u32>,
}

/// view.js `panes`.
#[derive(Clone, Debug, PartialEq)]
pub struct Panes {
    pub layout: &'static str,
    pub fx: f64,
    pub fy: f64,
    pub active: usize,
    pub list: Vec<Pane>,
}

/// What the window shows now: the open space, its view, and the active pane's rectangle (view.js's `x0, y0, w, h`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Live {
    pub space: usize,
    pub vs: Vs,
    pub rect: Rect,
}

/// The split screen of one window (view.js's pane state plus `linked`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Split {
    pub panes: Option<Panes>,
    /// LINKVIEWS: panes showing the same kind of space pan and zoom together
    pub linked: bool,
    /// the whole drawing area (view.js `W`, `H`)
    pub w: f64,
    pub h: f64,
}

impl Split {
    /// `layoutPanes()`: the pane rectangles for the area's size; the active one's becomes the live rectangle.
    pub fn layout(&mut self, live: &mut Live) {
        let (w, h) = (self.w, self.h);
        let Some(p) = self.panes.as_mut() else {
            live.rect = Rect { x: 0.0, y: 0.0, w, h };
            return;
        };
        let rects = pane_rects(p.layout, p.fx, p.fy, w, h);
        for (q, r) in p.list.iter_mut().zip(&rects) {
            q.rect = Some(*r);
        }
        if let Some(r) = rects.get(p.active) {
            live.rect = *r;
        }
    }
    /// `syncPane()`: the live view back into the active pane's record.
    pub fn sync(&mut self, live: &Live) {
        if let Some(p) = self.panes.as_mut()
            && let Some(q) = p.list.get_mut(p.active)
        {
            q.vs = live.vs;
            q.space = live.space;
        }
    }
    /// `loadPane(p)`: its view (and rectangle) becomes the live one.
    fn load(live: &mut Live, p: &Pane) {
        live.vs = p.vs;
        if let Some(r) = p.rect {
            live.rect = r;
        }
    }
    /// `setPanes(layout, { fx, fy, list, active })`: a layout ("single" ends the split); every new pane starts on the
    /// active pane's view.
    pub fn set(&mut self, layout: &str, fx: f64, fy: f64, list: Option<&[Option<Pane>]>, active: usize, live: &mut Live) {
        let n = pane_count(layout);
        let Some(layout) = LAYOUTS.iter().copied().find(|l| *l == layout) else { return };
        if n < 2 {
            if self.panes.is_none() {
                return;
            }
            self.sync(live);
            let keep = self.panes.as_ref().and_then(|p| p.list.get(p.active).copied());
            self.panes = None;
            if let Some(k) = keep {
                Self::load(live, &k);
            }
            self.layout(live);
            return;
        }
        self.sync(live);
        let cur = match &self.panes {
            Some(p) => p.list.get(p.active).copied(),
            None => None,
        }
        .unwrap_or(Pane { space: live.space, vs: live.vs, rect: None, doc: None });
        let old: Vec<Pane> = self.panes.as_ref().map(|p| p.list.clone()).unwrap_or_default();
        let list: Vec<Pane> = (0..n)
            .map(|i| {
                let q = list.and_then(|l| l.get(i).copied().flatten()).or_else(|| old.get(i).copied()).unwrap_or(cur);
                Pane { rect: None, ..q }
            })
            .collect();
        let active = active.min(n - 1);
        self.panes = Some(Panes { layout, fx: clamp_split(fx), fy: clamp_split(fy), active, list });
        if let Some(p) = self.panes.as_ref().and_then(|p| p.list.get(active).copied()) {
            Self::load(live, &p);
        }
        self.layout(live);
    }
    /// SPLIT / VPORTS: `host.setPanes(layout)` (dividers back in the middle).
    pub fn set_layout(&mut self, layout: &str, live: &mut Live) {
        self.set(layout, 0.5, 0.5, None, 0, live);
    }
    /// `setSplit(key, f)`: move a divider ("fx" | "fy", 0–1).
    pub fn set_split(&mut self, key: &str, f: f64, live: &mut Live) {
        if self.panes.is_none() {
            return;
        }
        self.sync(live);
        if let Some(p) = self.panes.as_mut() {
            match key {
                "fx" => p.fx = clamp_split(f),
                "fy" => p.fy = clamp_split(f),
                _ => return,
            }
        }
        self.layout(live);
    }
    /// `activatePane(i)` → the space the window must open (Some) when pane i became the active one. `sheets` = how many
    /// sheets the drawing has (a pane on a sheet that is gone shows the model).
    pub fn activate(&mut self, i: usize, sheets: usize, live: &mut Live) -> Option<usize> {
        self.activate_with(i, sheets, live, true)
    }
    /// [`Split::activate`]; `sync` false after doctabs.js traded the drawing (it synced the pane before).
    pub fn activate_with(&mut self, i: usize, sheets: usize, live: &mut Live, sync: bool) -> Option<usize> {
        let p = self.panes.as_ref()?;
        if i == p.active || i >= p.list.len() {
            return None;
        }
        if sync {
            self.sync(live);
        }
        let p = self.panes.as_mut()?;
        p.active = i;
        let q = p.list.get_mut(i)?;
        if q.space > sheets {
            q.space = 0;
        }
        let q = *q;
        Self::load(live, &q);
        live.space = q.space;
        Some(q.space)
    }
    /// `followLinked()`: linked, every pane showing the same kind of space (the model, or a sheet) as the active one
    /// takes its centre and zoom.
    pub fn follow_linked(&mut self, live: &Live) {
        if !self.linked || self.panes.is_none() {
            return;
        }
        self.sync(live);
        let Some(p) = self.panes.as_mut() else { return };
        let Some(act) = p.list.get(p.active).copied() else { return };
        let model = |q: &Pane| q.space == 0;
        for (i, q) in p.list.iter_mut().enumerate() {
            if i != p.active && model(q) == model(&act) {
                q.vs.cx = act.vs.cx;
                q.vs.cy = act.vs.cy;
                q.vs.zoom = act.vs.zoom;
            }
        }
    }
    /// `paneState()`: what the panes show, for saving (the file's VPORT table): view heights instead of zooms, sheets
    /// by name (`sheet_name(i)` for space i ≥ 1). `entered`: the active pane works in a sheet's viewport (MSPACE): that
    /// sheet's space and its view.
    pub fn state(&mut self, live: &Live, sheet_name: &dyn Fn(usize) -> Option<String>, entered: Option<(usize, Vs)>) -> Value {
        if self.panes.is_none() {
            return Value::Null;
        }
        self.sync(live);
        let Some(p) = self.panes.as_ref() else { return Value::Null };
        let list: Vec<Value> = p
            .list
            .iter()
            .enumerate()
            .map(|(i, q)| {
                let ph = q.rect.map_or(live.rect.h, |r| r.h);
                let (vs, space) = match entered {
                    Some((s, v)) if i == p.active => (v, s),
                    _ => (q.vs, q.space),
                };
                let mut o = Map::new();
                o.insert("cx".into(), json!(vs.cx));
                o.insert("cy".into(), json!(vs.cy));
                o.insert("h".into(), json!(ph / vs.zoom));
                if vs.tw != 0.0 && !vs.tw.is_nan() {
                    o.insert("tw".into(), json!(vs.tw));
                }
                if space > 0
                    && let Some(n) = sheet_name(space)
                {
                    o.insert("sheet".into(), json!(n));
                }
                Value::Object(o)
            })
            .collect();
        json!({ "layout": p.layout, "fx": p.fx, "fy": p.fy, "active": p.active, "list": list })
    }
    /// `restorePanes(st)`: back from [`Split::state`] or a file's (made safe by [`clean_panes`]); missing panes copy the
    /// view on screen. `sheet_of(name)`: the space of a sheet by its name. → the space the window must open.
    pub fn restore(&mut self, st: Option<&CleanPanes>, sheet_of: &dyn Fn(&str) -> Option<usize>, live: &mut Live) -> usize {
        let Some(st) = st else {
            self.set("single", 0.5, 0.5, None, 0, live);
            return live.space;
        };
        let n = pane_count(st.layout);
        let rects = pane_rects(st.layout, st.fx, st.fy, self.w, self.h);
        let list: Vec<Option<Pane>> = (0..n)
            .map(|i| {
                Some(match st.list.get(i).copied().flatten() {
                    Some(q) => {
                        let space = q.sheet.and_then(|k| st.names.get(k)).and_then(sheet_of).unwrap_or(0);
                        let rh = rects.get(i).map_or(1.0, |r| r.h);
                        Pane { space, vs: Vs { cx: q.cx, cy: q.cy, zoom: rh / q.h, tw: q.tw }, rect: None, doc: None }
                    }
                    None => Pane { space: 0, vs: live.vs, rect: None, doc: None },
                })
            })
            .collect();
        self.panes = None;
        self.set(st.layout, st.fx, st.fy, Some(&list), st.active, live);
        let space = self.panes.as_ref().and_then(|p| p.list.get(p.active)).map_or(0, |q| q.space);
        live.space = space;
        space
    }
    /// The pane records with the live view in the active one (for drawing them).
    pub fn shown(&self, live: &Live) -> Vec<Pane> {
        let Some(p) = self.panes.as_ref() else { return Vec::new() };
        p.list.iter().enumerate().map(|(i, q)| if i == p.active { Pane { space: live.space, vs: live.vs, ..*q } } else { *q }).collect()
    }
}

/// One pane of a file's split screen after [`clean_panes`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CleanPane {
    pub cx: f64,
    pub cy: f64,
    pub h: f64,
    pub tw: f64,
    /// index into [`CleanPanes::names`]
    pub sheet: Option<usize>,
}
/// `cleanPanes(v)`'s result.
#[derive(Clone, Debug, PartialEq)]
pub struct CleanPanes {
    pub layout: &'static str,
    pub fx: f64,
    pub fy: f64,
    pub active: usize,
    pub list: Vec<Option<CleanPane>>,
    pub names: Names,
}
/// The sheet names a pane setup gives.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Names(pub Vec<String>);
impl Names {
    pub fn get(&self, i: usize) -> Option<&str> {
        self.0.get(i).map(String::as_str)
    }
}

/// UTF-16 prefix of at most `n` units (JS `slice(0, n)`), never splitting a pair badly: a lone half is dropped.
fn slice16(s: &str, n: usize) -> String {
    let mut out = String::new();
    let mut used = 0usize;
    for c in s.chars() {
        let k = c.len_utf16();
        if used + k > n {
            break;
        }
        used += k;
        out.push(c);
    }
    out
}

/// panes.js `cleanPanes(v)`: a file's (or the clipboard's) pane setup made safe — numbers checked, sizes limited; None
/// for one pane or anything else.
pub fn clean_panes(v: &Value) -> Option<CleanPanes> {
    let o = v.as_object()?;
    let layout = o.get("layout").and_then(Value::as_str).and_then(|l| LAYOUTS.iter().copied().find(|x| *x == l))?;
    if layout == "single" {
        return None;
    }
    let n = pane_count(layout);
    let num = |x: Option<&Value>| x.and_then(Value::as_f64).filter(|f| f.is_finite());
    let mut names = Vec::new();
    let mut list: Vec<Option<CleanPane>> = o
        .get("list")
        .and_then(Value::as_array)
        .map(|a| a.iter().take(n).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .map(|p| {
            let (cx, cy, h) = (num(p.get("cx"))?, num(p.get("cy"))?, num(p.get("h"))?);
            if !(h > 0.0 && cx.abs() < 1e15 && cy.abs() < 1e15 && h < 1e15) {
                return None;
            }
            let tw = num(p.get("tw")).filter(|t| t.abs() <= 100.0).unwrap_or(0.0);
            let sheet = p.get("sheet").and_then(Value::as_str).filter(|s| !s.is_empty()).map(|s| {
                names.push(slice16(s, 255));
                names.len() - 1
            });
            Some(CleanPane { cx, cy, h, tw, sheet })
        })
        .collect();
    list.resize(n, None);
    let active = match o.get("active").and_then(Value::as_f64) {
        Some(a) if a.fract() == 0.0 && a >= 0.0 && a < n as f64 => a as usize,
        _ => 0,
    };
    let split = |k: &str| clamp_split(o.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN));
    Some(CleanPanes { layout, fx: split("fx"), fy: split("fy"), active, list, names: Names(names) })
}

/// The parity harness's case runner (`cadcraft-shot panecases`, forks/tools/panediff-cadpanes.mjs): a drawing
/// (`doc`: doc.load's input), the drawing area (`W`, `H`), and `ops` run on one window's split screen as view.js runs
/// them — `["view", cx, cy, zoom, tw?]` the view on screen · `["split", layout]` (SPLIT / VPORTS) · `["divider", "fx" |
/// "fy", f]` · `["act", i]` a click into pane i · `["zoom", sx, sy, f]` · `["pan", dx, dy]` · `["turn", tw]` ·
/// `["link", on]` (LINKVIEWS) · `["frame"]` a redraw (linked panes follow) · `["resize", W, H]` · `["restore", json]`
/// (a file's pane setup) · `["state"]` records the panes and the live view · `["save"]` writes the drawing as DXF R12
/// with the panes in its VPORT table (as cadcraft.js's save), reads it back and restores the panes from it.
pub fn run_case(case: &Value) -> Value {
    let mut doc = cc_cad::Doc::new();
    doc.load(case.get("doc").unwrap_or(&Value::Null));
    let num = |v: Option<&Value>| v.and_then(Value::as_f64).unwrap_or(f64::NAN);
    let mut s = Split { w: num(case.get("W")), h: num(case.get("H")), ..Default::default() };
    let mut live = Live::default();
    s.layout(&mut live);
    let names: Vec<String> = doc.layouts.iter().map(|l| l.name.clone()).collect();
    let sheet_name = |i: usize| names.get(i.wrapping_sub(1)).cloned();
    let sheet_of = |n: &str| names.iter().position(|x| x == n).map(|i| i + 1);
    let rec = |s: &mut Split, live: &Live| {
        let st = s.state(live, &sheet_name, None);
        let r = live.rect;
        json!({ "state": st, "linked": s.linked, "live": { "space": live.space, "cx": live.vs.cx, "cy": live.vs.cy, "zoom": live.vs.zoom,
            "tw": live.vs.tw, "x0": r.x, "y0": r.y, "w": r.w, "h": r.h } })
    };
    let mut out = Vec::new();
    for op in case.get("ops").and_then(Value::as_array).into_iter().flatten() {
        let a = |i: usize| op.get(i);
        let f = |i: usize| num(op.get(i));
        match a(0).and_then(Value::as_str).unwrap_or("") {
            "view" => {
                live.vs = Vs { cx: f(1), cy: f(2), zoom: f(3), tw: a(4).and_then(Value::as_f64).unwrap_or(0.0) };
            }
            "split" => s.set_layout(a(1).and_then(Value::as_str).unwrap_or(""), &mut live),
            "divider" => s.set_split(a(1).and_then(Value::as_str).unwrap_or(""), f(2), &mut live),
            "act" => {
                let i = a(1).and_then(Value::as_u64).map_or(usize::MAX, |i| usize::try_from(i).unwrap_or(usize::MAX));
                s.activate(i, names.len(), &mut live);
            }
            "zoom" => live.vs.zoom_at(live.rect.w, live.rect.h, f(1), f(2), f(3)),
            "pan" => live.vs.pan_by(f(1), f(2)),
            "turn" => live.vs.tw = f(1),
            "link" => s.linked = a(1).and_then(Value::as_bool).unwrap_or(false),
            "frame" => s.follow_linked(&live),
            "resize" => {
                s.sync(&live);
                s.w = f(1);
                s.h = f(2);
                s.layout(&mut live);
            }
            "restore" => {
                let c = a(1).and_then(clean_panes);
                s.restore(c.as_ref(), &sheet_of, &mut live);
            }
            "state" => out.push(rec(&mut s, &live)),
            "save" => {
                let st = s.state(&live, &sheet_name, None);
                doc.view_twist = if s.panes.is_some() || live.space != 0 { 0.0 } else { live.vs.tw };
                doc.vports = (!st.is_null()).then_some(st);
                let text = match cc_io_cad::dxf_write::write_dxf(&doc) {
                    Ok((t, _)) => t,
                    Err(e) => {
                        out.push(json!({ "error": e.0 }));
                        continue;
                    }
                };
                let back = cc_io_cad::dxf_read::read_dxf(&text).ok();
                let vp = back.as_ref().and_then(|b| b.get("vports")).cloned().unwrap_or(Value::Null);
                let c = clean_panes(&vp);
                s.restore(c.as_ref(), &sheet_of, &mut live);
                let mut r = rec(&mut s, &live);
                if let Some(o) = r.as_object_mut() {
                    o.insert("dxf".into(), json!(text));
                    o.insert("viewTwist".into(), back.as_ref().and_then(|b| b.get("viewTwist")).cloned().unwrap_or(Value::Null));
                }
                out.push(r);
            }
            _ => out.push(json!({ "error": "unknown op" })),
        }
    }
    Value::Array(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live() -> Live {
        Live { space: 0, vs: Vs { cx: 10.0, cy: 20.0, zoom: 2.0, tw: 0.0 }, rect: Rect::default() }
    }

    #[test]
    fn rects_and_dividers() {
        let r = pane_rects("4", 0.5, 0.5, 800.0, 600.0);
        assert_eq!(r.len(), 4);
        assert_eq!(r[0], Rect { x: 0.0, y: 0.0, w: 398.0, h: 298.0 });
        assert_eq!(r[3], Rect { x: 402.0, y: 302.0, w: 398.0, h: 298.0 });
        assert_eq!(pane_at(&r, 401.0, 10.0), None);
        assert_eq!(pane_at(&r, 403.0, 10.0), Some(1));
        assert_eq!(dividers("3l", 0.25, 0.5, 800.0, 600.0)[1], Divider { key: "fy", vertical: false, x: 200.0, y: 300.0, len: 600.0 });
        assert_eq!(clamp_split(f64::NAN), 0.5);
        assert_eq!(clamp_split(2.0), 0.9);
        // a tiny area: every pane at least 1 px
        assert!(pane_rects("4", 0.5, 0.5, 1.0, 1.0).iter().all(|r| r.w >= 1.0 && r.h >= 1.0));
    }

    #[test]
    fn split_activate_state_restore() {
        let mut s = Split { w: 800.0, h: 600.0, ..Default::default() };
        let mut l = live();
        s.layout(&mut l);
        s.set_layout("2v", &mut l);
        assert_eq!(l.rect.w, 398.0);
        l.vs.zoom = 4.0;
        assert_eq!(s.activate(1, 0, &mut l), Some(0));
        assert_eq!(l.vs.zoom, 2.0);
        assert_eq!(s.activate(1, 0, &mut l), None);
        let st = s.state(&l, &|_| None, None);
        assert_eq!(st["layout"], "2v");
        assert_eq!(st["active"], 1);
        assert_eq!(st["list"][0]["h"], 150.0);
        assert_eq!(st["list"][1]["h"], 300.0);
        // back from the file
        let c = clean_panes(&st);
        let mut s2 = Split { w: 800.0, h: 600.0, ..Default::default() };
        let mut l2 = live();
        s2.restore(c.as_ref(), &|_| None, &mut l2);
        assert_eq!(s2.state(&l2, &|_| None, None), st);
        // linked: the other model pane follows
        s.linked = true;
        l.vs.cx = 99.0;
        s.follow_linked(&l);
        assert_eq!(s.panes.as_ref().map(|p| p.list[0].vs.cx), Some(99.0));
        s.set_layout("single", &mut l);
        assert!(s.panes.is_none());
        assert_eq!(l.rect.w, 800.0);
    }

    #[test]
    fn hostile_pane_setups() {
        for v in [json!(null), json!({"layout": "single"}), json!({"layout": "5"}), json!({"layout": 4}), json!([1, 2]), json!("4")] {
            assert_eq!(clean_panes(&v), None);
        }
        let c = clean_panes(
            &json!({"layout": "4", "fx": "x", "active": 7, "list": [{"cx": 1, "cy": 2, "h": -1}, {"cx": 1e16, "cy": 0, "h": 1}, null, {"cx": 0, "cy": 0, "h": 5, "tw": 1000, "sheet": "A"}, {"cx": 0}]}),
        );
        let c = c.as_ref();
        assert_eq!(c.map(|c| c.list.iter().filter(|p| p.is_some()).count()), Some(1));
        assert_eq!(c.map(|c| (c.fx, c.active)), Some((0.5, 0)));
        assert_eq!(c.and_then(|c| c.list[3]).map(|p| p.tw), Some(0.0));
    }
}
