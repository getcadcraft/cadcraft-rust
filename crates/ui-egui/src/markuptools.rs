//! MarkupCraft's tools on the sheet view, ported from markupcraft.js's pointer handlers (onDown / pointermove /
//! pointerup / dblclick, `KIND`): free (pen, highlight), two clicks or a drag (line, arrow, measure, calibrate), box
//! (rect, ellipse, revision cloud, text, stamp), corner by corner (polyline, polygon, cloud by outline, area), three
//! clicks (arc, angle), callout, note and count, plus Select (pick, Shift-pick, move, grips, window / crossing
//! selection). A finished gesture is markupcraft.js `newMarkup` → `commitNew`: the tool's style (the default with the
//! Properties panel's choices), the author, the page's rotation, the current layer, the type's subject — one
//! `Store::change` step with the 0.5 label. Text, callout and note open an editor first (`startEdit` / `commitEdit`).
//! Points are PDF user space; `s` arguments are screen pixels (the JS measures "moved" and window direction on screen).

use std::collections::BTreeMap;

use cc_markup::geom::{Grip, apply_grip, grips, move_by, pick, pick_window, simplify, snap45, square_from};
use cc_markup::i18n::{Fill, t, t0};
use cc_markup::model::{Markup, Store, make_markup};
use cc_markup::shapes::{NOTE_SIZE, ShapeEnv, frame};
use cc_markup::units::{Cal, calibration_from, parse_length};
use serde_json::{Map, Value, json};

type P = [f64; 2];

/// The tools in toolbar order (ids; the label is the catalog's `tool.<id>`).
pub const TOOLS: &[&str] = &[
    "select",
    "pan",
    "pen",
    "highlight",
    "line",
    "arrow",
    "arc",
    "rect",
    "ellipse",
    "polyline",
    "polygon",
    "cloudrect",
    "cloud",
    "callout",
    "text",
    "note",
    "stamp",
    "calibrate",
    "measure",
    "angle",
    "area",
    "count",
];

/// markupcraft.js `KIND`: how a tool is drawn.
pub fn kind(tool: &str) -> &'static str {
    match tool {
        "pen" | "highlight" => "free",
        "line" | "arrow" | "measure" | "calibrate" => "two",
        "rect" | "ellipse" | "cloudrect" | "text" | "stamp" => "box",
        "arc" | "angle" => "three",
        "polyline" | "polygon" | "cloud" | "area" => "poly",
        "callout" => "callout",
        "note" => "note",
        "count" => "count",
        _ => "",
    }
}

/// `DRAW_TOOLS`.
pub fn draws(tool: &str) -> bool {
    !kind(tool).is_empty() && tool != "calibrate"
}

/// Quick colours on the tool bar (MarkupCraft's red first).
pub const COLORS: &[&str] = &["#d0021b", "#f08c00", "#2da44e", "#1f6feb", "#8250df", "#111111"];

/// A tool's label from MarkupCraft's catalog.
pub fn tool_label(id: &str) -> String {
    t0(&format!("tool.{id}"))
}

/// `TEXT_TYPES`.
pub fn text_type(k: &str) -> bool {
    k == "text" || k == "callout"
}
/// `COLOR_TYPES`.
fn color_type(k: &str) -> bool {
    matches!(k, "pen" | "line" | "arrow" | "rect" | "ellipse" | "polyline" | "polygon" | "cloudrect" | "cloud" | "callout" | "text")
}
/// `tlow(type)`.
fn tlow(k: &str) -> String {
    t0(&format!("type.{k}.lower"))
}

/// Points a pen stroke keeps at most; corners an outline keeps at most (hostile or endless gestures stay bounded).
const MAX_PEN: usize = 20_000;
const MAX_POLY: usize = 5_000;

#[derive(Debug, Clone)]
enum Drag {
    Free { pts: Vec<P> },
    Two { a: P, b: P, s0: P },
    Box { a: P, b: P, s0: P },
    Callout { tip: P, b: P, s0: P },
    Move { ids: Vec<String>, m0s: Vec<Markup>, a: P, d: P, s0: P, moved: bool },
    Grip { id: String, grip: Grip, m0: Box<Markup>, p: P, shift: bool, s0: P, moved: bool },
    Window { a: P, b: P, s0: P, moved: bool },
}

#[derive(Debug, Clone)]
enum Pending {
    Two { a: P, b: P },
    Poly { pts: Vec<P> },
    Three { pts: Vec<P> },
}

/// The text editor over a new or edited markup (`editing`).
#[derive(Debug, Clone)]
pub struct Editing {
    pub m: Markup,
    pub is_new: bool,
    /// "text" (text, callout) or "comment" (note)
    pub field: &'static str,
    pub value: String,
}

/// The tools' state for one open plan set.
#[derive(Debug, Clone)]
pub struct MarkupTools {
    pub tool: String,
    /// settings.styles: each tool's own style choices (the Properties panel with no selection)
    pub styles: BTreeMap<String, Map<String, Value>>,
    /// settings.stamp, settings.countLabel
    pub stamp: String,
    pub count_label: String,
    /// who markups are by, and the layer new ones go on ('' = none)
    pub author: String,
    pub cur_layer: String,
    drag: Option<Drag>,
    pending: Option<Pending>,
    pub editing: Option<Editing>,
    /// the selection, in the order it was made
    pub sel: Vec<String>,
    pub message: String,
    /// PDF units per screen pixel (`1 / view.z`), set by the view each frame
    pub px: f64,
    /// the page's rotation as seen (`view.seenRotate`)
    pub rot: f64,
    /// the measuring line of the calibrate tool: its length in PDF units, waiting for the real distance
    pub cal_len: Option<f64>,
    pub cal_text: String,
    pub cal_unit: String,
    /// the tool to go back to once the sheet is calibrated (`afterCalibrate`)
    after_calibrate: Option<String>,
}

impl Default for MarkupTools {
    fn default() -> Self {
        MarkupTools {
            tool: "select".into(),
            styles: BTreeMap::new(),
            stamp: cc_markup::model::STAMPS.first().copied().unwrap_or("Approved").into(),
            count_label: String::new(),
            author: String::new(),
            cur_layer: String::new(),
            drag: None,
            pending: None,
            editing: None,
            sel: Vec::new(),
            message: String::new(),
            px: 1.0,
            rot: 0.0,
            cal_len: None,
            cal_text: String::new(),
            cal_unit: "ft-in".into(),
            after_calibrate: None,
        }
    }
}

fn dist(a: P, b: P) -> f64 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}
fn clamp_pt(v: f64, lo: f64, hi: f64) -> f64 {
    v.min(hi).max(lo)
}

impl MarkupTools {
    fn z(&self) -> f64 {
        if self.px.is_finite() && self.px > 0.0 { 1.0 / self.px } else { 1.0 }
    }

    /// The one selected markup, if exactly one is.
    pub fn selected(&self) -> Option<&str> {
        match self.sel.as_slice() {
            [one] => Some(one.as_str()),
            _ => None,
        }
    }

    /// `styleFor(type)`: the default (Settings › New markups at their defaults) with the tool's own choices.
    pub fn style_for(&self, kind: &str) -> cc_markup::model::Style {
        let mut st = cc_markup::model::default_style(kind);
        if color_type(kind) && st.width != 0.0 {
            st.width = (st.width * 4.0).round() / 4.0;
        }
        if let Some(own) = self.styles.get(kind) {
            st.merge(own);
        }
        st
    }

    /// `newMarkup(type, geom)`.
    pub fn new_markup(&self, store: &mut Store, sheet: &str, kind: &str, geom: Map<String, Value>) -> Markup {
        let mut style = self.style_for(kind);
        let label = if kind == "stamp" { cc_markup::model::stamp_label(&self.stamp) } else { String::new() };
        if kind == "stamp" && !self.styles.get("stamp").is_some_and(|s| s.contains_key("color")) {
            style.color = cc_markup::model::stamp_color(&label).to_string();
        }
        let layer =
            if !self.cur_layer.is_empty() && !store.layer_state(&self.cur_layer).locked { self.cur_layer.clone() } else { String::new() };
        let mut f = Map::new();
        f.insert("sheet".into(), json!(sheet));
        f.insert("type".into(), json!(kind));
        f.insert("rot".into(), json!(self.rot));
        f.insert("author".into(), json!(self.author));
        f.insert("label".into(), json!(label));
        f.insert("layer".into(), json!(layer));
        f.insert("subject".into(), json!(cc_markup::model::type_subject(kind)));
        f.insert("text".into(), json!(""));
        for (k, v) in geom {
            f.insert(k, v);
        }
        let mut m = make_markup(&mut store.env, &f);
        m.style = style;
        m
    }

    /// `commitNew(m, label)`: one undo step; the new markup is selected.
    fn commit_new(&mut self, store: &mut Store, m: Markup, label: Option<String>) -> Option<String> {
        let label = label.unwrap_or_else(|| t("undo.add", &[("type", Fill::S(tlow(&m.kind)))]));
        match store.change(&label, |tx| tx.add(m)) {
            Ok(mk) => {
                self.sel = vec![mk.id.clone()];
                Some(mk.id)
            }
            Err(e) => {
                self.message = e.to_string();
                None
            }
        }
    }

    fn env_of(store: &Store, badge: f64) -> impl Fn(&Markup) -> ShapeEnv + '_ {
        move |m: &Markup| ShapeEnv { cal: store.calibrations.get(m.sheet()).cloned(), badge, legend: None }
    }

    /// `setTool(name)`: measuring tools on a sheet with no scale go to Calibrate first; a drawing tool clears the
    /// selection (Count keeps a selected count group).
    pub fn set_tool(&mut self, store: &mut Store, sheet: &str, name: &str) {
        self.commit_edit(store, sheet);
        self.cancel();
        let mut name = name.to_string();
        if matches!(name.as_str(), "measure" | "area") && !store.calibrations.contains_key(sheet) {
            self.message = t0("hint.calibrate_first");
            self.after_calibrate = Some(name);
            name = "calibrate".into();
        } else if matches!(name.as_str(), "calibrate" | "callout" | "area") {
            self.message = t0(&format!("hint.{name}"));
        }
        let keep = name == "count" && self.selected().and_then(|id| store.get(id)).is_some_and(|m| m.kind == "count");
        if draws(&name) && !keep {
            self.sel.clear();
        }
        self.tool = name;
    }

    /// `cancelOps()` (Esc).
    pub fn cancel(&mut self) {
        self.drag = None;
        self.pending = None;
    }

    /// `constrain(from, p, e)`.
    fn constrain(from: P, p: P, shift: bool) -> P {
        if shift { snap45(from, p) } else { p }
    }

    /// The primary button went down at `p` (screen `s`); `dbl` = the second press of a double-click (`e.detail >= 2`).
    #[allow(clippy::too_many_arguments)] // the pointer event as the JS handler gets it (point, screen point, Shift, detail)
    pub fn down(&mut self, store: &mut Store, sheet: &str, badge: f64, p: P, s: P, shift: bool, dbl: bool) {
        if self.editing.is_some() {
            self.commit_edit(store, sheet);
            return;
        }
        let k = if self.tool == "select" { "select" } else { kind(&self.tool) };
        match k {
            "select" => self.select_down(store, sheet, badge, p, s, shift),
            "free" => self.drag = Some(Drag::Free { pts: vec![p] }),
            "two" => {
                if let Some(Pending::Two { a, .. }) = self.pending.clone() {
                    self.pending = None;
                    self.finish_two(store, sheet, a, Self::constrain(a, p, shift));
                } else {
                    self.drag = Some(Drag::Two { a: p, b: p, s0: s });
                }
            }
            "box" => self.drag = Some(Drag::Box { a: p, b: p, s0: s }),
            "poly" => {
                if let Some(Pending::Poly { pts }) = self.pending.as_mut() {
                    let last = pts.len().checked_sub(2).and_then(|i| pts.get(i)).copied().unwrap_or(p);
                    let q = Self::constrain(last, p, shift);
                    if dbl {
                        self.finish_poly(store, sheet);
                        return;
                    }
                    if let Some(l) = pts.last_mut() {
                        *l = q;
                    }
                    if pts.len() < MAX_POLY {
                        pts.push(q);
                    }
                } else {
                    self.pending = Some(Pending::Poly { pts: vec![p, p] });
                }
            }
            "three" => {
                if let Some(Pending::Three { pts }) = self.pending.as_mut() {
                    if let Some(l) = pts.last_mut() {
                        *l = p;
                    }
                    if pts.len() == 3 {
                        let pts = pts.clone();
                        self.pending = None;
                        self.finish_three(store, sheet, pts);
                        return;
                    }
                    pts.push(p);
                    self.message = t0(&format!("three.last.{}", self.tool));
                } else {
                    self.pending = Some(Pending::Three { pts: vec![p, p] });
                    self.message = t0(&format!("three.next.{}", self.tool));
                }
            }
            "callout" => self.drag = Some(Drag::Callout { tip: p, b: p, s0: s }),
            "note" => {
                let mut g = Map::new();
                g.insert("pts".into(), json!([[p[0] - NOTE_SIZE / 2.0, p[1] + NOTE_SIZE / 2.0]]));
                let m = self.new_markup(store, sheet, "note", g);
                self.start_edit(m, true);
            }
            "count" => self.count_click(store, sheet, p, shift),
            _ => {}
        }
    }

    /// The pointer moved (button down or not).
    pub fn mv(&mut self, store: &Store, p: P, s: P, shift: bool) {
        let z = self.z();
        match self.drag.as_mut() {
            None => match self.pending.as_mut() {
                Some(Pending::Two { a, b }) => *b = Self::constrain(*a, p, shift),
                Some(Pending::Poly { pts }) => {
                    let last = pts.len().checked_sub(2).and_then(|i| pts.get(i)).copied().unwrap_or(p);
                    if let Some(l) = pts.last_mut() {
                        *l = Self::constrain(last, p, shift);
                    }
                }
                Some(Pending::Three { pts }) => {
                    if let Some(l) = pts.last_mut() {
                        *l = p;
                    }
                }
                None => {}
            },
            Some(d) => {
                let moved = |s0: &P| dist(*s0, s) > 3.0;
                match d {
                    Drag::Free { pts, .. } => {
                        let last = pts.last().copied().unwrap_or(p);
                        if shift {
                            let first = pts.first().copied().unwrap_or(p);
                            *pts = vec![first, p];
                        } else if dist(p, last) > 0.75 / z && pts.len() < MAX_PEN {
                            pts.push(p);
                        }
                    }
                    Drag::Two { a, b, .. } => *b = Self::constrain(*a, p, shift),
                    Drag::Box { a, b, .. } => *b = if shift { square_from(*a, p) } else { p },
                    Drag::Callout { b, .. } => *b = p,
                    Drag::Move { a, d, s0, moved: mvd, .. } => {
                        if moved(s0) || *mvd {
                            *mvd = true;
                            let q = if shift { snap45(*a, p) } else { p };
                            *d = [q[0] - a[0], q[1] - a[1]];
                        }
                    }
                    Drag::Grip { p: gp, shift: gs, s0, moved: mvd, .. } => {
                        if moved(s0) || *mvd {
                            *mvd = true;
                            *gp = p;
                            *gs = shift;
                        }
                    }
                    Drag::Window { b, s0, moved: mvd, .. } => {
                        *b = s;
                        *mvd = *mvd || moved(s0);
                    }
                }
            }
        }
        let _ = store;
    }

    /// The primary button came up at `p` (screen `s`).
    #[allow(clippy::too_many_arguments)] // the pointer event, and screen → user space for a window selection
    pub fn up(&mut self, store: &mut Store, sheet: &str, badge: f64, p: P, s: P, shift: bool, to_user: &dyn Fn(P) -> P) {
        let Some(d) = self.drag.take() else { return };
        let moved = |s0: P| dist(s0, s) > 4.0;
        match d {
            Drag::Free { pts, .. } => self.finish_free(store, sheet, pts),
            Drag::Two { a, s0, .. } => {
                if moved(s0) {
                    self.finish_two(store, sheet, a, Self::constrain(a, p, shift));
                } else {
                    self.pending = Some(Pending::Two { a, b: a });
                }
            }
            Drag::Box { a, s0, .. } => {
                let b = moved(s0).then(|| if shift { square_from(a, p) } else { p });
                self.finish_box(store, sheet, a, b);
            }
            Drag::Callout { tip, s0, .. } => self.finish_callout(store, sheet, tip, moved(s0).then_some(p)),
            Drag::Move { ids, m0s, d, moved: mvd, .. } => {
                if !mvd || ids.is_empty() || (d[0] == 0.0 && d[1] == 0.0) {
                    return;
                }
                let n = ids.len();
                let _ = store.change(&t("undo.move", &[("n", Fill::N(n as f64))]), |tx| {
                    for m in &m0s {
                        tx.update(&m.id, &move_by(m, d[0], d[1]), true);
                    }
                    Ok(())
                });
            }
            Drag::Grip { id, grip, m0, p: gp, shift: gs, moved: mvd, .. } => {
                if !mvd {
                    return;
                }
                let patch = apply_grip(&m0, &grip, gp, gs).to_map();
                let _ = store.change(&t("undo.resize", &[("type", Fill::S(tlow(&m0.kind)))]), |tx| {
                    tx.update(&id, &patch, true);
                    Ok(())
                });
            }
            Drag::Window { a, b, moved: mvd, .. } => {
                if !mvd {
                    return;
                }
                let (ua, ub) = (to_user(a), to_user(b));
                let r = [ua[0].min(ub[0]), ua[1].min(ub[1]), ua[0].max(ub[0]), ua[1].max(ub[1])];
                let crossing = b[0] < a[0];
                let list: Vec<&Markup> = store.on_sheet(sheet).into_iter().filter(|m| store.open(m)).collect();
                let env = Self::env_of(store, badge);
                let hits: Vec<String> = pick_window(&list, r, crossing, &env).iter().map(|m| m.id.clone()).collect();
                if shift {
                    for h in &hits {
                        if !self.sel.contains(h) {
                            self.sel.push(h.clone());
                        }
                    }
                } else {
                    self.sel = hits.clone();
                }
                self.message = if hits.is_empty() {
                    t0(if crossing { "select.window.none_touch" } else { "select.window.none_inside" })
                } else {
                    t("select.window", &[("n", Fill::N(hits.len() as f64))])
                };
            }
        }
    }

    /// A double-click (the `dblclick` event): ends an outline.
    pub fn dblclick(&mut self, store: &mut Store, sheet: &str) {
        if matches!(self.pending, Some(Pending::Poly { .. })) {
            self.finish_poly(store, sheet);
        }
    }

    /// Enter: ends an outline; in the editor (Ctrl+Enter) commits the text.
    pub fn enter(&mut self, store: &mut Store, sheet: &str) {
        if self.editing.is_some() {
            self.commit_edit(store, sheet);
        } else {
            self.finish_poly(store, sheet);
        }
    }

    fn select_down(&mut self, store: &Store, sheet: &str, badge: f64, p: P, s: P, shift: bool) {
        let z = self.z();
        if let Some(id) = self.selected().map(str::to_string)
            && let Some(m) = store.get(&id).filter(|m| m.sheet() == sheet)
        {
            let tol = 6.0 / z;
            let mut gs = grips(m);
            gs.sort_by_key(|g| g.kind == "box");
            if let Some(g) = gs.into_iter().find(|g| (g.x - p[0]).abs() <= tol && (g.y - p[1]).abs() <= tol) {
                self.drag = Some(Drag::Grip { id, grip: g, m0: Box::new(m.clone()), p, shift: false, s0: s, moved: false });
                return;
            }
        }
        let list: Vec<&Markup> = store.on_sheet(sheet).into_iter().filter(|m| store.open(m)).collect();
        let env = Self::env_of(store, badge);
        let hit = pick(&list, p, 5.0 / z, &env).map(|m| m.id.clone());
        if let Some(id) = hit {
            if shift {
                if let Some(i) = self.sel.iter().position(|x| *x == id) {
                    self.sel.remove(i);
                } else {
                    self.sel.push(id);
                }
                return;
            }
            if !self.sel.contains(&id) {
                self.sel = vec![id];
            }
            let ids: Vec<String> = self.sel.iter().filter(|i| store.get(i).is_some_and(|m| !m.locked)).cloned().collect();
            let m0s = ids.iter().filter_map(|i| store.get(i).cloned()).collect();
            self.drag = Some(Drag::Move { ids, m0s, a: p, d: [0.0, 0.0], s0: s, moved: false });
            return;
        }
        if !shift {
            self.sel.clear();
        }
        self.drag = Some(Drag::Window { a: s, b: s, s0: s, moved: false });
    }

    /// `finishFree(pts)`.
    fn finish_free(&mut self, store: &mut Store, sheet: &str, pts: Vec<P>) {
        let mut clean = if pts.len() > 2 { simplify(&pts, 0.6 / self.z()) } else { pts };
        if let [only] = clean.as_slice() {
            let q = [only[0] + 0.01, only[1]];
            clean.push(q);
        }
        let mut g = Map::new();
        g.insert("pts".into(), json!(clean));
        let m = self.new_markup(store, sheet, &self.tool.clone(), g);
        self.commit_new(store, m, None);
    }

    /// `finishTwo(a, b)`.
    fn finish_two(&mut self, store: &mut Store, sheet: &str, a: P, b: P) {
        if dist(a, b) < 2.0 / self.z() {
            self.message = t0("two.same");
            return;
        }
        if self.tool == "calibrate" {
            let len = dist(a, b);
            self.cal_len = Some(len);
            self.message = t("cal.two.text", &[("d", Fill::S(cc_markup::js::to_fixed(len / 72.0, 3)))]);
            return;
        }
        let mut g = Map::new();
        g.insert("pts".into(), json!([a, b]));
        if self.tool == "measure" {
            let Some(cal) = store.calibrations.get(sheet).cloned() else {
                self.message = t0("need.scale");
                return;
            };
            let m = self.new_markup(store, sheet, "measure", g);
            self.commit_new(store, m, None);
            let v = cc_markup::units::measure(dist(a, b), Some(&cal));
            self.message = t("measure.done", &[("value", Fill::S(v)), ("scale", Fill::S(cal.label.clone()))]);
            return;
        }
        let m = self.new_markup(store, sheet, &self.tool.clone(), g);
        self.commit_new(store, m, None);
    }

    /// `textBoxAt(p, w, h)`.
    fn text_box_at(&self, p: P, w: f64, h: f64) -> [f64; 4] {
        let f = frame(self.rot, [0.0, 0.0, 1.0, 1.0]);
        let q = [p[0] + f.u[0] * w + f.v[0] * h, p[1] + f.u[1] * w + f.v[1] * h];
        [p[0].min(q[0]), p[1].min(q[1]), p[0].max(q[0]), p[1].max(q[1])]
    }

    /// `stampBox(p, label)`.
    fn stamp_box(&self, store: &mut Store, p: P, label: &str) -> [f64; 4] {
        use cc_markup::font::text_width;
        let hh = clamp_pt(54.0 / self.z(), 36.0, 90.0);
        let probe = Markup {
            author: self.author.clone(),
            created: store.env.now_iso(),
            ..make_markup(&mut cc_markup::model::Env::default(), &Map::new())
        };
        let sub = cc_markup::shapes::stamp_sub(&probe);
        let upper = label.to_uppercase();
        let ww = (hh * 2.2).max(text_width(&upper, hh * 0.38, true) + hh * 0.6).max(text_width(&sub, hh * 0.17, false) + hh * 0.6);
        let f = frame(self.rot, [0.0, 0.0, 1.0, 1.0]);
        let (w, h) = if f.u[0] != 0.0 { (ww, hh) } else { (hh, ww) };
        [p[0] - w / 2.0, p[1] - h / 2.0, p[0] + w / 2.0, p[1] + h / 2.0]
    }

    /// `finishBox(a, b)`: b = None for a click.
    fn finish_box(&mut self, store: &mut Store, sheet: &str, a: P, b: Option<P>) {
        let z = self.z();
        let tool = self.tool.clone();
        let mut g = Map::new();
        if tool == "stamp" {
            let label = cc_markup::model::stamp_label(&self.stamp);
            let rect = match b {
                Some(b) => {
                    if (b[0] - a[0]).abs() < 8.0 / z || (b[1] - a[1]).abs() < 8.0 / z {
                        return;
                    }
                    [a[0], a[1], b[0], b[1]]
                }
                None => self.stamp_box(store, a, &label),
            };
            g.insert("rect".into(), json!(rect));
            let m = self.new_markup(store, sheet, "stamp", g);
            self.commit_new(store, m, None);
            return;
        }
        if tool == "text" {
            let st = self.style_for("text");
            let rect = match b {
                Some(b) => [a[0], a[1], b[0], b[1]],
                None => self.text_box_at(a, clamp_pt(200.0 / z, 120.0, 360.0), st.font_size * 1.2 + 6.0),
            };
            g.insert("rect".into(), json!(rect));
            let m = self.new_markup(store, sheet, "text", g);
            self.start_edit(m, true);
            return;
        }
        let Some(b) = b else {
            self.message = t("box.drag", &[("type", Fill::S(tlow(&tool)))]);
            return;
        };
        if (b[0] - a[0]).abs() < 2.0 / z && (b[1] - a[1]).abs() < 2.0 / z {
            return;
        }
        g.insert("rect".into(), json!([a[0], a[1], b[0], b[1]]));
        let m = self.new_markup(store, sheet, &tool, g);
        self.commit_new(store, m, None);
    }

    /// `finishCallout(tip, at)`.
    fn finish_callout(&mut self, store: &mut Store, sheet: &str, tip: P, at: Option<P>) {
        let z = self.z();
        let st = self.style_for("callout");
        let f = frame(self.rot, [0.0, 0.0, 1.0, 1.0]);
        let knee = at.unwrap_or([tip[0] + (f.u[0] * 50.0 - f.v[0] * 40.0) / z, tip[1] + (f.u[1] * 50.0 - f.v[1] * 40.0) / z]);
        let w = clamp_pt(190.0 / z, 120.0, 300.0);
        let h = st.font_size * 1.2 + 10.0;
        let along = (knee[0] - tip[0]) * f.u[0] + (knee[1] - tip[1]) * f.u[1];
        let gap = 6.0 / z;
        let start = if along >= 0.0 {
            [knee[0] + f.u[0] * gap, knee[1] + f.u[1] * gap]
        } else {
            [knee[0] - f.u[0] * (gap + w), knee[1] - f.u[1] * (gap + w)]
        };
        let top_left = [start[0] - f.v[0] * h / 2.0, start[1] - f.v[1] * h / 2.0];
        let rect = self.text_box_at(top_left, w, h);
        let mut g = Map::new();
        g.insert("rect".into(), json!(rect));
        g.insert("pts".into(), json!([tip, knee]));
        let m = self.new_markup(store, sheet, "callout", g);
        self.start_edit(m, true);
    }

    /// `finishPoly()`.
    fn finish_poly(&mut self, store: &mut Store, sheet: &str) {
        let Some(Pending::Poly { pts }) = self.pending.take() else {
            if let Some(p) = self.pending.take() {
                self.pending = Some(p);
            }
            return;
        };
        let pts: Vec<P> = pts.get(..pts.len().saturating_sub(1)).map(<[P]>::to_vec).unwrap_or_default();
        let tol = 1.0 / self.z();
        let clean: Vec<P> = pts
            .iter()
            .enumerate()
            .filter(|(i, q)| *i == 0 || pts.get(i - 1).is_some_and(|r| dist(**q, *r) > tol))
            .map(|(_, q)| *q)
            .collect();
        let tool = self.tool.clone();
        let need = if tool == "polyline" { 2 } else { 3 };
        if tool == "area" && !store.calibrations.contains_key(sheet) {
            self.message = t0("need.scale");
            return;
        }
        if clean.len() < need {
            self.message = t("poly.need", &[("type", Fill::S(tlow(&tool))), ("n", Fill::N(need as f64))]);
            return;
        }
        let mut g = Map::new();
        g.insert("pts".into(), json!(clean));
        let m = self.new_markup(store, sheet, &tool, g);
        self.commit_new(store, m, None);
    }

    /// `finishThree(pts)`.
    fn finish_three(&mut self, store: &mut Store, sheet: &str, pts: Vec<P>) {
        let (Some(a), Some(b), Some(c)) = (pts.first(), pts.get(1), pts.get(2)) else { return };
        let span = dist(*a, *b).max(dist(*b, *c));
        let tool = self.tool.clone();
        if span < 2.0 / self.z() {
            self.message = t("poly.need", &[("type", Fill::S(tlow(&tool))), ("n", Fill::N(3.0))]);
            return;
        }
        let mut g = Map::new();
        g.insert("pts".into(), json!(pts));
        let m = self.new_markup(store, sheet, &tool, g);
        self.commit_new(store, m, None);
    }

    /// `countClick(p, e)`.
    fn count_click(&mut self, store: &mut Store, sheet: &str, p: P, shift: bool) {
        let group = self.selected().and_then(|id| store.get(id)).filter(|m| m.kind == "count" && m.sheet() == sheet && !m.locked).cloned();
        let default = t0("count.default");
        if shift {
            let Some(g) = group else {
                self.message = t0("count.remove.need");
                return;
            };
            let tol = 8.0 / self.z();
            let (mut best, mut bd) = (None, f64::INFINITY);
            for (i, q) in g.pts().iter().enumerate() {
                let d = dist(*q, p);
                if d < bd {
                    bd = d;
                    best = Some(i);
                }
            }
            let Some(best) = best.filter(|_| bd <= tol + g.style.font_size) else {
                self.message = if g.label.is_empty() {
                    t0("count.remove.miss.unnamed")
                } else {
                    t("count.remove.miss", &[("group", Fill::S(g.label.clone()))])
                };
                return;
            };
            if g.pts().len() == 1 {
                let _ = store.change(&t0("undo.count.remove"), |tx| {
                    tx.remove(&g.id);
                    Ok(())
                });
                self.sel.clear();
                self.message = t0("count.removed_last");
                return;
            }
            let rest: Vec<P> = g.pts().iter().enumerate().filter(|(i, _)| *i != best).map(|(_, q)| *q).collect();
            let mut patch = Map::new();
            patch.insert("pts".into(), json!(rest));
            let _ = store.change(&t0("undo.count.remove_mark"), |tx| {
                tx.update(&g.id, &patch, true);
                Ok(())
            });
            let name = if g.label.is_empty() { default } else { g.label.clone() };
            self.message = t("count.total", &[("group", Fill::S(name)), ("n", Fill::N(rest.len() as f64))]);
            return;
        }
        if let Some(g) = group {
            let mut pts = g.pts().to_vec();
            if pts.len() < MAX_POLY {
                pts.push(p);
            }
            let mut patch = Map::new();
            patch.insert("pts".into(), json!(pts));
            let r = store.change(&t0("undo.count"), |tx| Ok(tx.update(&g.id, &patch, true)));
            if let Ok(Some(m)) = r {
                let name = if m.label.is_empty() { default } else { m.label.clone() };
                self.message = t("count.total", &[("group", Fill::S(name)), ("n", Fill::N(m.pts().len() as f64))]);
            }
            return;
        }
        let label: String = if self.count_label.is_empty() { default } else { self.count_label.clone() }.chars().take(60).collect();
        let mut g = Map::new();
        g.insert("pts".into(), json!([p]));
        g.insert("label".into(), json!(label));
        let m = self.new_markup(store, sheet, "count", g);
        if let Some(id) = self.commit_new(store, m, Some(t0("undo.count.start"))) {
            let lab = store.get(&id).map(|m| m.label.clone()).unwrap_or_default();
            self.message = t("count.started", &[("group", Fill::S(lab))]);
        }
    }

    /// `startEdit(m, isNew)`: the editor opens on its text (a note's comment); a stamp has none.
    pub fn start_edit(&mut self, m: Markup, is_new: bool) {
        let field = match m.kind.as_str() {
            "note" => "comment",
            "stamp" => return,
            _ => "text",
        };
        let value = if field == "comment" { m.comment.clone() } else { m.text.clone() };
        self.message = t0(if m.kind == "note" { "edit.note" } else { "edit.text" });
        self.editing = Some(Editing { m, is_new, field, value });
    }

    /// What is typed in the editor changed: the text box grows downward as the text wraps (the editor's `input`).
    pub fn edit_input(&mut self, value: &str) {
        let Some(ed) = self.editing.as_mut() else { return };
        ed.value = value.to_string();
        if ed.m.kind == "note" {
            return;
        }
        let Some(r) = ed.m.rect else { return };
        let fs = ed.m.style.font_size;
        let f = frame(ed.m.rot, r);
        let pad = if ed.m.kind == "text" { 3.0 } else { 4.0 };
        let lines = cc_markup::font::wrap_text(value, fs, f.w - 2.0 * pad, false);
        let need = fs + (lines.len().max(1) as f64 - 1.0) * fs * 1.2 + 2.0 * pad + fs * 0.3;
        if need > f.h {
            let dv = need - f.h;
            let [x0, y0, x1, y1] = r;
            ed.m.rect = Some(if f.v[1] == -1.0 {
                [x0, y0 - dv, x1, y1]
            } else if f.v[1] == 1.0 {
                [x0, y0, x1, y1 + dv]
            } else if f.v[0] == 1.0 {
                [x0, y0, x1 + dv, y1]
            } else {
                [x0 - dv, y0, x1, y1]
            });
        }
    }

    /// `commitEdit()`: a new markup is added with its text (nothing typed: none, except a note); an edited one updated.
    pub fn commit_edit(&mut self, store: &mut Store, sheet: &str) {
        let Some(ed) = self.editing.take() else { return };
        let value = ed.value.trim_end().to_string();
        let _ = sheet;
        if ed.is_new {
            if value.is_empty() && ed.m.kind != "note" {
                self.message = t("edit.empty", &[("type", Fill::S(tlow(&ed.m.kind)))]);
                return;
            }
            let mut m = ed.m;
            if ed.field == "comment" {
                m.comment = value;
            } else {
                m.text = value;
            }
            self.commit_new(store, m, None);
        } else {
            let cur = store.get(&ed.m.id).cloned();
            let old = cur.as_ref().map(|c| if ed.field == "comment" { c.comment.clone() } else { c.text.clone() }).unwrap_or_default();
            if value != old || cur.as_ref().map(|c| c.rect) != Some(ed.m.rect) {
                let mut patch = Map::new();
                patch.insert(ed.field.into(), json!(value));
                if let Some(r) = ed.m.rect {
                    patch.insert("rect".into(), json!(r));
                }
                let _ = store.change(&t0("undo.edit_text"), |tx| {
                    tx.update(&ed.m.id, &patch, true);
                    Ok(())
                });
            }
        }
        self.message.clear();
    }

    /// Esc in the editor.
    pub fn cancel_edit(&mut self) {
        self.editing = None;
        self.message.clear();
    }

    /// The gesture under way as markups to draw over the sheet (not in the store).
    pub fn preview(&self, store: &Store, sheet: &str, hover: Option<P>) -> Vec<Markup> {
        let mut env = cc_markup::model::Env::default();
        let mut mk = |kind: &str, g: Value| -> Option<Markup> {
            let mut f = Map::new();
            f.insert("sheet".into(), json!(sheet));
            f.insert("type".into(), json!(kind));
            f.insert("rot".into(), json!(self.rot));
            if let Value::Object(o) = g {
                for (k, v) in o {
                    f.insert(k, v);
                }
            }
            let mut m = make_markup(&mut env, &f);
            m.style = self.style_for(kind);
            cc_markup::model::geometry_ok(&m).then_some(m)
        };
        let tool = self.tool.as_str();
        let as_kind = if tool == "calibrate" { "measure" } else { tool };
        let mut out = Vec::new();
        match (&self.drag, &self.pending) {
            (Some(Drag::Free { pts, .. }), _) if pts.len() >= 2 => out.extend(mk(as_kind, json!({ "pts": pts }))),
            (Some(Drag::Two { a, b, .. }), _) | (_, Some(Pending::Two { a, b })) => out.extend(mk(as_kind, json!({ "pts": [a, b] }))),
            (Some(Drag::Box { a, b, .. }), _) => {
                let k = if tool == "stamp" || tool == "text" { "rect" } else { as_kind };
                out.extend(mk(k, json!({ "rect": [a[0], a[1], b[0], b[1]] })));
            }
            (Some(Drag::Callout { tip, b, .. }), _) => out.extend(mk("line", json!({ "pts": [tip, b] }))),
            (_, Some(Pending::Poly { pts })) => {
                let k = if pts.len() >= 3 { as_kind } else { "polyline" };
                out.extend(mk(k, json!({ "pts": pts })));
            }
            (_, Some(Pending::Three { pts })) => {
                let k = if pts.len() == 3 { as_kind } else { "line" };
                out.extend(mk(k, json!({ "pts": pts })));
            }
            (Some(Drag::Move { m0s, d, moved: true, .. }), _) => {
                for m in m0s {
                    let mut c = m.clone();
                    let patch = move_by(m, d[0], d[1]);
                    for (k, v) in &patch {
                        cc_markup::json::set_key(&mut c, k, v);
                    }
                    out.push(c);
                }
            }
            (Some(Drag::Grip { m0, grip, p, shift, moved: true, .. }), _) => {
                let mut c = (**m0).clone();
                for (k, v) in &apply_grip(m0, grip, *p, *shift).to_map() {
                    cc_markup::json::set_key(&mut c, k, v);
                }
                out.push(c);
            }
            _ => {}
        }
        if let Some(ed) = &self.editing {
            out.push(ed.m.clone());
        }
        let _ = (store, hover);
        out
    }

    /// Markups hidden while their moved copies are drawn.
    pub fn hidden(&self) -> Vec<String> {
        match &self.drag {
            Some(Drag::Move { ids, moved: true, .. }) => ids.clone(),
            Some(Drag::Grip { id, moved: true, .. }) => vec![id.clone()],
            _ => self.editing.as_ref().filter(|e| !e.is_new).map(|e| vec![e.m.id.clone()]).unwrap_or_default(),
        }
    }

    /// The window being dragged (screen corners).
    pub fn window(&self) -> Option<(P, P)> {
        match &self.drag {
            Some(Drag::Window { a, b, moved: true, .. }) => Some((*a, *b)),
            _ => None,
        }
    }

    /// Whether a gesture is under way (the view then does not pan with the primary button).
    pub fn busy(&self) -> bool {
        self.drag.is_some() || self.pending.is_some()
    }

    /// Set the sheet's scale from the calibrate line and the typed distance (cal.set, one undo step); then the
    /// measuring tool that asked for it (or Length) is next.
    pub fn apply_calibration(&mut self, store: &mut Store, sheet: &str) -> bool {
        let Some(len) = self.cal_len else { return false };
        let Some(mm) = parse_length(&self.cal_text, &self.cal_unit).filter(|v| *v > 0.0) else {
            self.message = t0("cal.bad_length");
            return false;
        };
        let Some(cal) = calibration_from(len, mm, &self.cal_unit) else {
            self.message = t0("cal.bad_length");
            return false;
        };
        self.set_scale(store, sheet, Some(cal));
        self.cal_len = None;
        self.cal_text.clear();
        if self.tool == "calibrate" {
            let next = self.after_calibrate.take().unwrap_or_else(|| "measure".into());
            self.set_tool(store, sheet, &next);
        }
        true
    }

    /// Set (or with `None` remove) the sheet's scale: one undo step labelled as the JS labels it.
    pub fn set_scale(&mut self, store: &mut Store, sheet: &str, cal: Option<Cal>) {
        let label = if cal.is_some() { t0("cal.set") } else { t0("cal.remove") };
        let done = cal.as_ref().map(|c| t("cal.done", &[("scale", Fill::S(c.label.clone()))]));
        let r = store.change(&label, |tx| {
            tx.set_calibration(sheet, cal);
            Ok(())
        });
        self.message = match r {
            Ok(()) => done.unwrap_or_else(|| t0("cal.removed")),
            Err(e) => e.to_string(),
        };
    }

    /// `deleteSelection()`: the selected markups that are not read-only, one undo step.
    pub fn delete_selected(&mut self, store: &mut Store) {
        let del: Vec<String> = self.sel.iter().filter(|i| store.get(i).is_some_and(|m| !m.locked)).cloned().collect();
        if self.sel.is_empty() {
            self.message = t0("delete.need");
            return;
        }
        if !del.is_empty() {
            let n = del.len() as f64;
            let _ = store.change(&t("undo.delete", &[("n", Fill::N(n))]), |tx| {
                for id in &del {
                    tx.remove(id);
                }
                Ok(())
            });
            self.message = t("delete.done", &[("n", Fill::N(n))]);
        }
        self.sel.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> (Store, MarkupTools) {
        (Store::new(), MarkupTools { px: 1.0, ..MarkupTools::default() })
    }
    fn tap(m: &mut MarkupTools, s: &mut Store, p: P) {
        m.down(s, "f1:0", 9.0, p, p, false, false);
        m.up(s, "f1:0", 9.0, p, p, false, &|q| q);
    }
    fn drag(m: &mut MarkupTools, s: &mut Store, a: P, b: P) {
        m.down(s, "f1:0", 9.0, a, a, false, false);
        m.mv(s, b, b, false);
        m.up(s, "f1:0", 9.0, b, b, false, &|q| q);
    }

    #[test]
    fn a_drag_adds_one_rectangle_with_the_tool_style_and_undo_removes_it() {
        let (mut s, mut m) = ctx();
        m.set_tool(&mut s, "f1:0", "rect");
        drag(&mut m, &mut s, [100.0, 100.0], [200.0, 160.0]);
        let id = m.selected().unwrap().to_string();
        let mk = s.get(&id).unwrap();
        assert_eq!(mk.rect, Some([100.0, 100.0, 200.0, 160.0]));
        assert_eq!(mk.style.color, "#d0021b");
        assert_eq!(mk.subject, cc_markup::model::type_subject("rect"));
        // a click with a box tool only says how to draw it
        tap(&mut m, &mut s, [300.0, 300.0]);
        assert_eq!(s.on_sheet("f1:0").len(), 1);
        assert!(s.undo().is_some());
        assert!(s.get(&id).is_none());
    }

    #[test]
    fn two_point_tools_take_a_drag_or_two_clicks() {
        let (mut s, mut m) = ctx();
        m.set_tool(&mut s, "f1:0", "line");
        tap(&mut m, &mut s, [0.0, 0.0]);
        assert!(s.on_sheet("f1:0").is_empty());
        tap(&mut m, &mut s, [50.0, 0.0]);
        assert_eq!(s.on_sheet("f1:0").len(), 1);
    }

    #[test]
    fn outlines_end_with_a_double_click_and_need_enough_corners() {
        let (mut s, mut m) = ctx();
        m.set_tool(&mut s, "f1:0", "polygon");
        for p in [[0.0, 0.0], [100.0, 0.0], [100.0, 50.0]] {
            tap(&mut m, &mut s, p);
        }
        m.down(&mut s, "f1:0", 9.0, [100.0, 50.0], [100.0, 50.0], false, true);
        let id = m.selected().unwrap().to_string();
        assert_eq!(s.get(&id).unwrap().pts().len(), 3);
        m.set_tool(&mut s, "f1:0", "polyline");
        tap(&mut m, &mut s, [0.0, 0.0]);
        m.enter(&mut s, "f1:0");
        assert_eq!(s.on_sheet("f1:0").len(), 1, "one corner is not a polyline");
    }

    #[test]
    fn counts_grow_the_selected_group_and_shift_takes_one_away() {
        let (mut s, mut m) = ctx();
        m.set_tool(&mut s, "f1:0", "count");
        tap(&mut m, &mut s, [10.0, 10.0]);
        tap(&mut m, &mut s, [40.0, 10.0]);
        tap(&mut m, &mut s, [70.0, 10.0]);
        let id = m.selected().unwrap().to_string();
        assert_eq!(s.get(&id).unwrap().pts().len(), 3);
        m.down(&mut s, "f1:0", 9.0, [41.0, 10.0], [41.0, 10.0], true, false);
        assert_eq!(s.get(&id).unwrap().pts().len(), 2);
        assert_eq!(s.on_sheet("f1:0").len(), 1);
    }

    #[test]
    fn text_waits_for_its_words_and_a_note_does_not() {
        let (mut s, mut m) = ctx();
        m.set_tool(&mut s, "f1:0", "text");
        tap(&mut m, &mut s, [100.0, 100.0]);
        assert!(m.editing.is_some());
        m.commit_edit(&mut s, "f1:0");
        assert!(s.on_sheet("f1:0").is_empty(), "nothing typed, no text");
        tap(&mut m, &mut s, [100.0, 100.0]);
        m.edit_input("A long line of words that wraps onto several lines in the box");
        m.commit_edit(&mut s, "f1:0");
        let mk = s.get(m.selected().unwrap()).unwrap().clone();
        let r = mk.rect.unwrap();
        assert!(r[3] - r[1] > 12.0 * 1.2 + 6.0, "the box grew: {r:?}");
        m.set_tool(&mut s, "f1:0", "note");
        tap(&mut m, &mut s, [300.0, 300.0]);
        m.commit_edit(&mut s, "f1:0");
        assert_eq!(s.on_sheet("f1:0").len(), 2);
    }

    #[test]
    fn measuring_without_a_scale_calibrates_first() {
        let (mut s, mut m) = ctx();
        m.set_tool(&mut s, "f1:0", "measure");
        assert_eq!(m.tool, "calibrate");
        drag(&mut m, &mut s, [0.0, 0.0], [144.0, 0.0]);
        assert_eq!(m.cal_len, Some(144.0));
        m.cal_text = "10'".into();
        assert!(m.apply_calibration(&mut s, "f1:0"));
        assert_eq!(m.tool, "measure");
        drag(&mut m, &mut s, [0.0, 10.0], [72.0, 10.0]);
        assert_eq!(s.on_sheet("f1:0").len(), 1);
    }

    #[test]
    fn select_moves_and_windows() {
        let (mut s, mut m) = ctx();
        m.set_tool(&mut s, "f1:0", "rect");
        drag(&mut m, &mut s, [100.0, 100.0], [200.0, 160.0]);
        // Select keeps the selection (only drawing tools clear it)
        m.set_tool(&mut s, "f1:0", "select");
        assert_eq!(m.sel.len(), 1);
        drag(&mut m, &mut s, [120.0, 100.0], [130.0, 110.0]);
        let id = m.selected().unwrap().to_string();
        assert_eq!(s.get(&id).unwrap().rect, Some([110.0, 110.0, 210.0, 170.0]));
        drag(&mut m, &mut s, [0.0, 0.0], [20.0, 20.0]);
        assert!(m.sel.is_empty());
        drag(&mut m, &mut s, [50.0, 50.0], [400.0, 400.0]);
        assert_eq!(m.sel, vec![id]);
    }
}
