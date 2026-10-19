//! CivilCraft's profile panel (M11.8): existing ground and the design profile along one alignment under the plan, as
//! civilcraft.js `showProfile` / `renderProfile` and profile-view.js draw it — station across, elevation up (exaggerated
//! to fill the panel, the exaggeration shown), the grid, grades on the tangents, each vertical curve's L and K with its
//! BVC / EVC ticks and high or low point, the PVIs with their labels — and its editing: drag a PVI (one undo step),
//! Add PVI mode (I) and a click adds one, a PVI selected from the panel or the table goes with Delete, Start a profile
//! from the ground, the PVI table's station / elevation / length typed. Reading under the pointer: station, ground,
//! design, grade, cut or fill, and the point on the plan.

use cc_civil::i18n::t;
use cc_civil::jo;
use cc_civil::js::{arr, fixed_num, jnum, numf, string_of};
use cc_civil::profile::StaZ;
use cc_civil::tr;
use cc_civil::units::{fixed, format_station, parse_station, pct};
use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke};
use serde_json::Value;

use crate::civilplan::Palette;
use crate::civilview::{CivilView, Sel};
use crate::theme::Tokens;

/// A PVI being dragged: its working copy of the PVIs (civilcraft.js `pvDrag`).
#[derive(Clone, Debug)]
pub struct PviDrag {
    pub id: String,
    pub i: usize,
    pub pvis: Vec<Value>,
    pub press: (f64, f64),
    pub moved: bool,
}

/// The profile panel's state.
#[derive(Clone, Debug, Default)]
pub struct ProfilePanel {
    pub open: bool,
    /// the alignment asked for (civilcraft.js `ui.profAl.dataset.want`)
    pub want: Option<String>,
    /// Add PVI mode
    pub add: bool,
    pub sel_pvi: Option<usize>,
    pub hover: Option<usize>,
    pub drag: Option<PviDrag>,
    /// the elevation range shown (profile-view.js `fitY`), kept while the key stays
    pub z0: f64,
    pub z1: f64,
    pub key: String,
    /// the reading under the pointer
    pub read: String,
    /// the station under the pointer (a dot on the plan)
    pub cursor: Option<(String, f64)>,
    /// a table cell being typed in (key, text)
    pub editing: Option<(String, String)>,
    /// the last frame's plot (headless picks in data units go through it)
    pub plot: Option<Plot>,
    /// 0.5 WL.20: the feature lines in the profile, kept per (alignment, project version, within)
    pub fl_cache: Option<(String, u64, f64, Value)>,
    /// the "Alongside within" field being typed in
    pub within_text: Option<String>,
    /// a feature line's Stepped offset fields (line id, distance, "dz" | "slope", amount), kept while it stays selected
    pub offset_form: Option<(String, String, String, String)>,
}

/// profile-view.js `colors().fl` (--layer-violet).
fn fl_color(pal: &Palette) -> Color32 {
    if pal.dark { Color32::from_rgb(0xa8, 0x92, 0xff) } else { Color32::from_rgb(0x5b, 0x3c, 0xc4) }
}

/// The plot area inside the panel (profile-view.js `pad`): left 58, right 16, top 14, bottom 30.
#[derive(Clone, Copy, Debug)]
pub struct Plot {
    pub x0: f64,
    pub x1: f64,
    pub y0: f64,
    pub y1: f64,
    pub start: f64,
    pub end: f64,
    pub z0: f64,
    pub z1: f64,
}

impl Plot {
    pub fn new(w: f64, h: f64, start: f64, end: f64, z0: f64, z1: f64) -> Plot {
        Plot { x0: 58.0, x1: (w - 16.0).max(59.0), y0: (h - 30.0).max(15.0), y1: 14.0, start, end, z0, z1 }
    }
    pub fn to_screen(&self, sta: f64, z: f64) -> (f64, f64) {
        let sx = self.x0 + (sta - self.start) / (self.end - self.start).max(1e-9) * (self.x1 - self.x0);
        let sy = self.y0 - (z - self.z0) / (self.z1 - self.z0).max(1e-9) * (self.y0 - self.y1);
        (sx, sy)
    }
    pub fn to_data(&self, sx: f64, sy: f64) -> (f64, f64) {
        let w = self.x1 - self.x0;
        let h = self.y0 - self.y1;
        (
            self.start + (sx - self.x0) / if w == 0.0 { 1.0 } else { w } * (self.end - self.start),
            self.z0 + (self.y0 - sy) / if h == 0.0 { 1.0 } else { h } * (self.z1 - self.z0),
        )
    }
}

/// profile-view.js `hitFeature`: the nearest crossing mark within 8 px.
fn hit_feature(f: &Value, p: &Plot, at: (f64, f64)) -> Option<String> {
    let mut best = None;
    let mut bd = 8.0;
    for x in arr(Some(f)) {
        for c in arr(x.get("crossings")) {
            let q = p.to_screen(numf(c.get("sta")), numf(c.get("z")));
            let d = (q.0 - at.0).hypot(q.1 - at.1);
            if d < bd {
                bd = d;
                best = x.get("id").and_then(Value::as_str).map(str::to_string);
            }
        }
    }
    best
}

/// profile-view.js `niceStep`.
fn nice_step(raw: f64, steps: &[f64]) -> f64 {
    for s in steps {
        if *s >= raw {
            return *s;
        }
    }
    let last = steps.last().copied().unwrap_or(1.0);
    last * (raw / last).ceil().max(1.0)
}

/// profile-view.js `fitY`: the ground's and the PVIs' range, at least 2 units, 15 % margin.
pub fn fit_y(eg: &[StaZ], pvis: &[Value]) -> (f64, f64) {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for p in eg {
        if let Some(z) = p.z {
            lo = lo.min(z);
            hi = hi.max(z);
        }
    }
    for v in pvis {
        let z = numf(v.get("z"));
        lo = lo.min(z);
        hi = hi.max(z);
    }
    if !lo.is_finite() || !hi.is_finite() {
        lo = 0.0;
        hi = 10.0;
    }
    let span = (hi - lo).max(2.0);
    let m = span * 0.15;
    (lo - m, hi + m)
}

fn find_al<'a>(state: &'a Value, id: &str) -> Option<&'a Value> {
    arr(state.get("alignments")).iter().find(|a| a.get("id").and_then(Value::as_str) == Some(id))
}

fn pvis_mut<'a>(s: &'a mut Value, id: &str) -> Option<&'a mut Vec<Value>> {
    s.get_mut("alignments")?
        .as_array_mut()?
        .iter_mut()
        .find(|a| a.get("id").and_then(Value::as_str) == Some(id))?
        .get_mut("pvis")?
        .as_array_mut()
}

/// JS `p.sort((m, n) => m.sta - n.sta)` (stable, by number).
fn sort_by_sta(p: &mut [Value]) {
    cc_civil::js::sort_by(p, |a, b| numf(a.get("sta")) - numf(b.get("sta")));
}

impl CivilView {
    /// civilcraft.js `profileAlignment()`: the selected alignment, else the one asked for, else the first.
    pub fn profile_alignment(&self) -> Option<Value> {
        let st = &self.project.state;
        let want = match &self.sel {
            Some(Sel::Alignment(id)) => Some(id.clone()),
            _ => self.profile.want.clone(),
        };
        want.and_then(|w| find_al(st, &w).cloned()).or_else(|| arr(st.get("alignments")).first().cloned())
    }

    /// civilcraft.js `showProfile(on, alId)`.
    pub fn show_profile(&mut self, on: bool, al: Option<&str>) {
        if on {
            // 0.4 V13: one bottom panel at a time
            self.sections.open = false;
        }
        self.profile.open = on;
        if let Some(a) = al {
            self.profile.want = Some(a.to_string());
        }
    }

    /// 0.5 WL.20 (civilcraft.js `renderProfile`): the feature lines crossing the alignment (and, within the set
    /// distance, running alongside) — featureline.js `featureProfile`, worked out again after every edit; [] when the
    /// profile's Feature lines switch is off.
    pub fn profile_features(&mut self, a: &Value) -> Value {
        let d = self.project.state.get("display").cloned().unwrap_or(Value::Null);
        if !cc_civil::js::truthy(d.get("profFeatures")) {
            return Value::Array(Vec::new());
        }
        let within = numf(d.get("profWithin"));
        let id = string_of(a.get("id"));
        let v = self.project.version;
        if let Some((k, ver, w, f)) = &self.profile.fl_cache
            && *k == id
            && *ver == v
            && (*w == within || (w.is_nan() && within.is_nan()))
        {
            return f.clone();
        }
        let t0 = web_time::Instant::now();
        let al = self.project.alignment(a);
        let feats: Vec<cc_civil::featureline::Feature> =
            arr(self.project.state.get("features")).iter().map(cc_civil::featureline::read_feature).collect();
        let f = cc_civil::featureline::feature_profile(&al, &feats, within);
        self.profile.fl_cache = Some((id, v, within, f.clone()));
        self.timings.insert("flprof", t0.elapsed().as_secs_f64() * 1000.0);
        f
    }

    /// The profile's Feature lines button (civilcraft.js `ui.profFl`): `display.profFeatures` on, off or switched.
    pub fn profile_fl(&mut self, on: Option<bool>) {
        let cur = cc_civil::js::truthy(self.project.state.get("display").and_then(|d| d.get("profFeatures")));
        self.project.set_display("profFeatures", Value::Bool(on.unwrap_or(!cur)));
    }

    /// The "Alongside within" field (civilcraft.js `ui.profWithin` change): empty = 0 (crossings only).
    pub fn profile_within(&mut self, text: &str) -> Result<(), String> {
        if !cc_civil::js::truthy(self.project.state.get("display").and_then(|d| d.get("profFeatures"))) {
            return Err("civil.profile.within: turn the profile's feature lines on first".into());
        }
        let v = cc_civil::js::str_to_number(&text.replacen(',', ".", 1));
        let empty = cc_civil::js::trim(text).is_empty();
        if !empty && (!v.is_finite() || v < 0.0) {
            self.message = t("pv.fl.within.bad", &[]);
            return Ok(());
        }
        self.project.set_display("profWithin", jnum(if empty { 0.0 } else { v }));
        Ok(())
    }

    /// profile-view.js `hitFeature(sx, sy, 8)` at a point in data units (through the last frame's plot): the feature
    /// line whose crossing mark is there is selected (civilcraft.js profile pointerdown).
    pub fn profile_pick_fl(&mut self, sta: f64, z: f64) -> Result<bool, String> {
        let p = self.profile.plot.ok_or("civil.profile.pickfl: open the profile first (civil.profile=on)")?;
        let Some(a) = self.profile_alignment() else { return Ok(false) };
        let f = self.profile_features(&a);
        let at = p.to_screen(sta, z);
        let Some(id) = hit_feature(&f, &p, at) else { return Ok(false) };
        self.select_profile_feature(&a, &id);
        Ok(true)
    }

    fn select_profile_feature(&mut self, a: &Value, id: &str) {
        if arr(self.project.state.get("features")).iter().any(|x| x.get("id").and_then(Value::as_str) == Some(id)) {
            // the profile keeps the alignment it shows (civilcraft.js: ui.profAl keeps its value)
            self.profile.want = Some(string_of(a.get("id")));
            self.sel = Some(Sel::Feature(id.to_string()));
            self.profile.sel_pvi = None;
            self.points.picked.clear();
        }
    }

    /// What the profile's feature line labels say (`pv.fl.cross` at each crossing, each run's name), for agents and
    /// tests: "" when there is none.
    pub fn profile_fl_info(&mut self) -> String {
        let Some(a) = self.profile_alignment() else { return String::new() };
        let f = self.profile_features(&a);
        let mut out = Vec::new();
        for x in arr(Some(&f)) {
            let name = string_of(x.get("name"));
            for c in arr(x.get("crossings")) {
                out.push(tr!("pv.fl.cross", name = name.as_str(), z = fixed(numf(c.get("z")), 2)));
            }
            for _ in arr(x.get("runs")) {
                out.push(name.clone());
            }
        }
        out.join(" · ")
    }

    /// Add PVI mode on or off (civilcraft.js `setPviAdd`).
    pub fn set_pvi_add(&mut self, on: bool) {
        self.profile.add = on;
        if on {
            self.message = t("cv.pvi.add.hint", &[]);
        }
    }

    /// The Add PVI click at (sta, z) in data units (civilcraft.js profile pointerdown with Add PVI on).
    pub fn pvi_add(&mut self, sta: f64, z: f64) -> Result<(), String> {
        let a = self.profile_alignment().ok_or("civil.pvi.add: no alignment")?;
        let al = self.project.alignment(&a);
        if !al.ok() {
            return Err("civil.pvi.add: the alignment has no geometry".into());
        }
        let id = string_of(a.get("id"));
        let units = self.project.units().to_string();
        let sta = al.start.max(al.end.min(fixed_num(sta, 2)));
        let z = fixed_num(z, 2);
        if arr(a.get("pvis")).iter().any(|v| (numf(v.get("sta")) - sta).abs() < 0.01) {
            self.message = tr!("cv.pvi.exists", sta = format_station(sta, &units, None));
            return Ok(());
        }
        let v = jo! { "sta" => sta, "z" => z, "L" => 0.0 };
        self.project.change(&t("cv.undo.pvi.add", &[]), &mut |s: &mut Value| {
            if let Some(p) = pvis_mut(s, &id) {
                p.push(v.clone());
                sort_by_sta(p);
            }
        });
        self.profile.sel_pvi =
            find_al(&self.project.state, &id).and_then(|a| arr(a.get("pvis")).iter().position(|v| numf(v.get("sta")) == sta));
        self.message = tr!("cv.pvi.added", sta = format_station(sta, &units, None), z = fixed(z, 2));
        Ok(())
    }

    /// Press on PVI `i` of the profile's alignment: selected, and a drag starts.
    pub fn pvi_press(&mut self, i: usize, press: (f64, f64)) {
        let Some(a) = self.profile_alignment() else { return };
        let id = string_of(a.get("id"));
        self.profile.sel_pvi = Some(i);
        self.sel = Some(Sel::Alignment(id.clone()));
        self.profile.drag = Some(PviDrag { id, i, pvis: arr(a.get("pvis")).to_vec(), press, moved: false });
    }

    /// The dragged PVI to (sta, z) data (2 px of slack first): clamped between its neighbours, to 0.01.
    pub fn pvi_drag_to(&mut self, sta: f64, z: f64, at: (f64, f64)) {
        let Some(a) = self.profile.drag.as_ref().and_then(|d| find_al(&self.project.state, &d.id).cloned()) else { return };
        let al = self.project.alignment(&a);
        let units = self.project.units().to_string();
        let Some(d) = self.profile.drag.as_mut() else { return };
        if (at.0 - d.press.0).hypot(at.1 - d.press.1) > 2.0 {
            d.moved = true;
        }
        if !d.moved {
            return;
        }
        let prev = d.i.checked_sub(1).and_then(|k| d.pvis.get(k)).map(|v| numf(v.get("sta")));
        let next = d.pvis.get(d.i + 1).map(|v| numf(v.get("sta")));
        let lo = prev.map_or(al.start, |p| p + 0.01);
        let hi = next.map_or(al.end, |n| n - 0.01);
        let s = lo.max(hi.min(fixed_num(sta, 2)));
        let zz = fixed_num(z, 2);
        if let Some(v) = d.pvis.get_mut(d.i).and_then(Value::as_object_mut) {
            v.insert("sta".into(), jnum(s));
            v.insert("z".into(), jnum(zz));
        }
        self.profile.read = tr!("cv.prof.read.pvi", sta = format_station(s, &units, None), z = fixed(zz, 2));
    }

    /// Release: a moved PVI becomes one undo step (civilcraft.js profile pointerup).
    pub fn pvi_release(&mut self) {
        let Some(d) = self.profile.drag.take() else { return };
        if !d.moved {
            return;
        }
        let list = Value::Array(d.pvis.clone());
        self.project.change(&t("cv.undo.pvi.move", &[]), &mut |s: &mut Value| {
            if let Some(a) = s
                .get_mut("alignments")
                .and_then(Value::as_array_mut)
                .and_then(|l| l.iter_mut().find(|a| a.get("id").and_then(Value::as_str) == Some(d.id.as_str())))
                && let Some(m) = a.as_object_mut()
            {
                m.insert("pvis".into(), list.clone());
            }
        });
        self.profile.sel_pvi = Some(d.i);
    }

    /// civilcraft.js `startProfile(a)`: two PVIs on the ground at the alignment's ends.
    pub fn start_profile(&mut self) -> Result<(), String> {
        let a = self.profile_alignment().ok_or("civil.profile.start: no alignment")?;
        let al = self.project.alignment(&a);
        let egp: Vec<StaZ> = self.project.eg_profile(&a).iter().filter(|p| p.z.is_some()).copied().collect();
        let (Some(z0), Some(z1)) = (egp.first().copied(), egp.last().copied()) else {
            self.message = t("cv.prof.noground", &[]);
            return Ok(());
        };
        if !al.ok() || egp.len() < 2 {
            self.message = t("cv.prof.noground", &[]);
            return Ok(());
        }
        let id = string_of(a.get("id"));
        let list = Value::Array(vec![
            jo! { "sta" => z0.sta, "z" => fixed_num(z0.z.unwrap_or(0.0), 2), "L" => 0.0 },
            jo! { "sta" => z1.sta, "z" => fixed_num(z1.z.unwrap_or(0.0), 2), "L" => 0.0 },
        ]);
        self.project.change(&t("cv.undo.prof.start", &[]), &mut |s: &mut Value| {
            if let Some(a) = s
                .get_mut("alignments")
                .and_then(Value::as_array_mut)
                .and_then(|l| l.iter_mut().find(|a| a.get("id").and_then(Value::as_str) == Some(id.as_str())))
                && let Some(m) = a.as_object_mut()
            {
                m.insert("pvis".into(), list.clone());
            }
        });
        self.show_profile(true, Some(&id));
        self.message = t("cv.prof.started", &[]);
        Ok(())
    }

    /// A PVI table cell typed (civilcraft.js profileProps): "sta" (a station, the PVIs sorted again), "z", "L".
    pub fn pvi_set(&mut self, i: usize, field: &str, text: &str) -> Result<(), String> {
        let a = self.profile_alignment().ok_or("civil.pvi.set: no alignment")?;
        let id = string_of(a.get("id"));
        let n = arr(a.get("pvis")).len();
        if i >= n {
            return Err(format!("civil.pvi.set: no PVI {}", i + 1));
        }
        let units = self.project.units().to_string();
        match field {
            "sta" => {
                let s = parse_station(text, &units).ok_or_else(|| format!("civil.pvi.set: {text} is not a station"))?;
                self.project.change(&t("cv.undo.pvi.sta", &[]), &mut |st: &mut Value| {
                    if let Some(p) = pvis_mut(st, &id) {
                        if let Some(m) = p.get_mut(i).and_then(Value::as_object_mut) {
                            m.insert("sta".into(), jnum(s));
                        }
                        sort_by_sta(p);
                    }
                });
            }
            "z" => {
                let z = cc_civil::js::str_to_number(text);
                if !z.is_finite() || text.is_empty() {
                    return Err(format!("civil.pvi.set: {text} is not an elevation"));
                }
                self.project.change(&t("cv.undo.pvi.z", &[]), &mut |st: &mut Value| {
                    if let Some(m) = pvis_mut(st, &id).and_then(|p| p.get_mut(i)).and_then(Value::as_object_mut) {
                        m.insert("z".into(), jnum(z));
                    }
                });
            }
            "L" => {
                if i == 0 || i + 1 >= n {
                    return Err("civil.pvi.set: only an interior PVI has a curve length".into());
                }
                let l = cc_civil::js::str_to_number(text);
                if l.is_nan() || l < 0.0 {
                    return Err(format!("civil.pvi.set: {text} is not a length"));
                }
                self.project.change(&t("cv.undo.pvi.l", &[]), &mut |st: &mut Value| {
                    if let Some(m) = pvis_mut(st, &id).and_then(|p| p.get_mut(i)).and_then(Value::as_object_mut) {
                        m.insert("L".into(), jnum(l));
                    }
                });
            }
            o => return Err(format!("civil.pvi.set: field sta, z or L (not {o})")),
        }
        Ok(())
    }

    /// A PI table cell typed (civilcraft.js alignmentProps): "x", "y" (one undo step each) or "r" (interior PIs).
    pub fn pi_set(&mut self, i: usize, field: &str, text: &str) -> Result<(), String> {
        let Some(Sel::Alignment(id)) = self.sel.clone() else { return Err("civil.pi.set: select an alignment first".into()) };
        let n = find_al(&self.project.state, &id).map_or(0, |a| arr(a.get("pis")).len());
        if i >= n {
            return Err(format!("civil.pi.set: no PI {}", i + 1));
        }
        let v = cc_civil::js::str_to_number(text);
        let (label, key) = match field {
            "x" | "y" => {
                if !v.is_finite() || text.is_empty() {
                    return Err(format!("civil.pi.set: {text} is not a coordinate"));
                }
                (tr!("cv.undo.pi.move.n", n = i + 1), field)
            }
            "r" => {
                if i == 0 || i + 1 >= n {
                    return Err("civil.pi.set: the end PIs have no curve".into());
                }
                if v.is_nan() || v < 0.0 {
                    return Err(format!("civil.pi.set: {text} is not a radius"));
                }
                (tr!("cv.al.pi.r", n = i + 1), "r")
            }
            o => return Err(format!("civil.pi.set: field x, y or r (not {o})")),
        };
        self.project.change(&label, &mut |s: &mut Value| {
            if let Some(m) = s
                .get_mut("alignments")
                .and_then(Value::as_array_mut)
                .and_then(|l| l.iter_mut().find(|a| a.get("id").and_then(Value::as_str) == Some(id.as_str())))
                .and_then(|a| a.get_mut("pis"))
                .and_then(Value::as_array_mut)
                .and_then(|p| p.get_mut(i))
                .and_then(Value::as_object_mut)
            {
                m.insert(key.to_string(), jnum(v));
            }
        });
        Ok(())
    }

    /// The selected PVI goes (civilcraft.js deleteSelected, first branch). True when one did.
    pub fn delete_pvi(&mut self) -> bool {
        let (Some(Sel::Alignment(id)), Some(i)) = (self.sel.clone(), self.profile.sel_pvi) else { return false };
        let n = find_al(&self.project.state, &id).map_or(0, |a| arr(a.get("pvis")).len());
        if i >= n {
            return false;
        }
        self.project.change(&t("cv.undo.pvi.delete", &[]), &mut |s: &mut Value| {
            if let Some(p) = pvis_mut(s, &id)
                && i < p.len()
            {
                p.remove(i);
            }
        });
        self.profile.sel_pvi = None;
        self.message = t("cv.pvi.deleted", &[]);
        true
    }

    /// The reading at station `sta` (civilcraft.js profile pointermove).
    fn profile_read(&mut self, a: &Value, sta: f64) -> String {
        let units = self.project.units().to_string();
        let eg = self.project.eg_profile(a);
        let egz = interp_eg(&eg, sta);
        let prof = self.project.design_profile(a);
        let fg = if prof.ok() { prof.elevation_at(sta) } else { None };
        let mut read = vec![tr!("cv.read.sta", sta = format_station(sta, &units, None))];
        if let Some(z) = egz {
            read.push(tr!("cv.read.ground", z = fixed(z, 2)));
        }
        if let Some(f) = fg {
            read.push(tr!("cv.read.design", z = fixed(f, 2)));
            read.push(tr!("cv.read.grade", g = pct(prof.grade_at(sta).unwrap_or(f64::NAN), 2)));
            let d = f - egz.unwrap_or(0.0);
            read.push(tr!(if d >= 0.0 { "cv.read.fill" } else { "cv.read.cut" }, d = fixed(d.abs(), 2)));
        }
        read.join(" · ")
    }
}

/// civilcraft.js `interpEg(eg, sta)`.
pub fn interp_eg(eg: &[StaZ], sta: f64) -> Option<f64> {
    for w in eg.windows(2) {
        let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
        if sta <= b.sta {
            let (za, zb) = (a.z?, b.z?);
            return Some(za + (zb - za) * (sta - a.sta) / (b.sta - a.sta).max(1e-12));
        }
    }
    None
}

/// What the panel's header asks for.
pub enum ProfAct {
    Run(&'static str, String),
}

/// The profile panel at the bottom of the plan area (lib.rs → civilview.rs `ui`).
pub fn panel(cv: &mut CivilView, ui: &mut egui::Ui, tk: &Tokens, pal: &Palette, acts: &mut Vec<ProfAct>) {
    let a = cv.profile_alignment();
    // header: alignment, Add PVI, the reading, close
    ui.horizontal(|ui| {
        ui.label(RichText::new(t("ui.tb.profile", &[])).strong().color(tk.text_strong));
        let als: Vec<(String, String)> =
            arr(cv.project.state.get("alignments")).iter().map(|x| (string_of(x.get("id")), string_of(x.get("name")))).collect();
        let cur = a.as_ref().map(|x| string_of(x.get("id"))).unwrap_or_default();
        let cur_name = als.iter().find(|x| x.0 == cur).map(|x| x.1.clone()).unwrap_or_default();
        ui.add_enabled_ui(als.len() >= 2, |ui| {
            egui::ComboBox::new("civil_prof_al", "").selected_text(cur_name).show_ui(ui, |ui| {
                for (id, name) in &als {
                    if ui.selectable_label(*id == cur, name).clicked() {
                        acts.push(ProfAct::Run("civil.profile.al", id.clone()));
                    }
                }
            });
        });
        if ui.selectable_label(cv.profile.add, t("ui.pvi.add", &[])).on_hover_text(t("ui.pvi.add.title", &[])).clicked() {
            acts.push(ProfAct::Run("civil.pvi.addmode", String::new()));
        }
        // 0.5 WL.20: feature lines in the profile, and how far alongside
        let d = cv.project.state.get("display").cloned().unwrap_or(Value::Null);
        let fl_on = cc_civil::js::truthy(d.get("profFeatures"));
        if ui.selectable_label(fl_on, t("ui.prof.fl", &[])).on_hover_text(t("ui.prof.fl.title", &[])).clicked() {
            acts.push(ProfAct::Run("civil.profile.fl", String::new()));
        }
        let l = ui.label(RichText::new(t("ui.prof.fl.within", &[])).small().color(tk.text_dim));
        let shown = {
            let w = numf(d.get("profWithin"));
            if w != 0.0 && !w.is_nan() { cc_civil::js::num_to_string(w) } else { String::new() }
        };
        let mut text = cv.profile.within_text.clone().unwrap_or_else(|| shown.clone());
        let r = ui
            .add_enabled(fl_on, egui::TextEdit::singleline(&mut text).desired_width(48.0))
            .labelled_by(l.id)
            .on_hover_text(t("ui.prof.fl.within.title", &[]));
        if r.has_focus() || r.changed() {
            cv.profile.within_text = Some(text.clone());
        }
        if r.lost_focus() {
            let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
            if let Some(v) = cv.profile.within_text.take()
                && !esc
                && v != shown
            {
                acts.push(ProfAct::Run("civil.profile.within", v));
            }
        }
        ui.label(RichText::new(cv.profile.read.clone()).small().color(tk.text_dim));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("×").on_hover_text(t("ui.prof.close", &[])).clicked() {
                acts.push(ProfAct::Run("civil.profile", "off".into()));
            }
        });
    });
    let rect = ui.available_rect_before_wrap();
    let resp = ui.allocate_rect(rect, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let bg = pal.bg;
    painter.rect_filled(rect, 0.0, bg);
    let font = egui::FontId::proportional(12.0);
    let Some(a) = a else {
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, t("pv.empty", &[]), font, pal.ink2);
        return;
    };
    let al = cv.project.alignment(&a);
    if !al.ok() {
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, t("pv.empty", &[]), font, pal.ink2);
        return;
    }
    let id = string_of(a.get("id"));
    let units = cv.project.units().to_string();
    let eg = cv.project.eg_profile(&a);
    let pvis: Vec<Value> = cv.profile.drag.as_ref().map_or_else(|| arr(a.get("pvis")).to_vec(), |d| d.pvis.clone());
    let prof = if cv.profile.drag.is_some() {
        std::sync::Arc::new(cc_civil::profile::build_profile(&pvis.iter().map(cc_civil::profile::read_pvi).collect::<Vec<_>>()))
    } else {
        cv.project.design_profile(&a)
    };
    let key = format!("{id}|{}", cv.project.version);
    if cv.profile.key != key && cv.profile.drag.is_none() {
        let (z0, z1) = fit_y(&eg, &pvis);
        cv.profile.z0 = z0;
        cv.profile.z1 = z1;
        cv.profile.key = key;
    }
    let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
    let p = Plot::new(w, h, al.start, al.end, cv.profile.z0, cv.profile.z1);
    cv.profile.plot = Some(p);
    let feats = cv.profile_features(&a);
    let feat_sel = match &cv.sel {
        Some(Sel::Feature(f)) => Some(f.clone()),
        _ => None,
    };
    let s = |sta: f64, z: f64| {
        let (x, y) = p.to_screen(sta, z);
        Pos2::new(rect.min.x + x as f32, rect.min.y + y as f32)
    };
    // ---- input
    let local = |q: Pos2| (f64::from(q.x - rect.min.x), f64::from(q.y - rect.min.y));
    let hit = |px: (f64, f64)| -> Option<usize> {
        let mut best = None;
        let mut bd = 9.0;
        for (i, v) in pvis.iter().enumerate() {
            let q = p.to_screen(numf(v.get("sta")), numf(v.get("z")));
            let d = (q.0 - px.0).hypot(q.1 - px.1);
            if d < bd {
                bd = d;
                best = Some(i);
            }
        }
        best
    };
    let (pressed, released, press_at) =
        ui.ctx().input(|i| (i.pointer.primary_pressed(), i.pointer.primary_released(), i.pointer.press_origin()));
    if pressed
        && resp.hovered()
        && let Some(q) = press_at
    {
        let px = local(q);
        let fl = if hit(px).is_none() { hit_feature(&feats, &p, px) } else { None };
        match hit(px) {
            Some(i) => cv.pvi_press(i, px),
            None if fl.is_some() => {
                if let Some(f) = &fl {
                    cv.select_profile_feature(&a, f);
                }
            }
            None => {
                if cv.profile.add {
                    let (sta, z) = p.to_data(px.0, px.1);
                    if let Err(e) = cv.pvi_add(sta, z) {
                        cv.message = e;
                    }
                } else {
                    cv.profile.sel_pvi = None;
                }
            }
        }
    }
    if cv.profile.drag.is_some() {
        if let Some(q) = ui.ctx().input(|i| i.pointer.latest_pos()) {
            let px = local(q);
            let (sta, z) = p.to_data(px.0, px.1);
            cv.pvi_drag_to(sta, z, px);
        }
        if released || !ui.ctx().input(|i| i.pointer.primary_down()) {
            cv.pvi_release();
        }
    } else if let Some(q) = resp.hover_pos() {
        let px = local(q);
        cv.profile.hover = hit(px);
        let (sta, _) = p.to_data(px.0, px.1);
        let sta = al.start.max(al.end.min(sta));
        cv.profile.read = cv.profile_read(&a, sta);
        cv.profile.cursor = Some((id.clone(), sta));
    } else {
        cv.profile.hover = None;
        cv.profile.cursor = None;
    }
    // ---- grid
    let len = al.end - al.start;
    let px_per_sta = (p.x1 - p.x0) / len.max(1e-9);
    let s_steps: &[f64] =
        if units == "m" { &[10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0] } else { &[25.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0] };
    let s_step = nice_step(70.0 / px_per_sta, s_steps);
    let z_step = nice_step(28.0 / ((p.y0 - p.y1) / (p.z1 - p.z0)), &[0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0]);
    let grid = Stroke::new(1.0, mix(pal.ink, bg, 0.82));
    let small = egui::FontId::proportional(11.0);
    let mut st = (al.start / s_step).ceil() * s_step;
    let mut guard = 0;
    while st <= al.end + 1e-9 && guard < 400 {
        let x = s(st, p.z0).x;
        painter.line_segment([Pos2::new(x, rect.min.y + p.y1 as f32), Pos2::new(x, rect.min.y + p.y0 as f32)], grid);
        painter.text(
            Pos2::new(x, rect.min.y + p.y0 as f32 + 6.0),
            egui::Align2::CENTER_TOP,
            format_station(st, &units, Some(0)),
            small.clone(),
            pal.ink2,
        );
        st += s_step;
        guard += 1;
    }
    let mut z = (p.z0 / z_step).ceil() * z_step;
    guard = 0;
    while z <= p.z1 + 1e-9 && guard < 400 {
        let y = s(al.start, z).y;
        painter.line_segment([Pos2::new(rect.min.x + p.x0 as f32, y), Pos2::new(rect.min.x + p.x1 as f32, y)], grid);
        painter.text(
            Pos2::new(rect.min.x + p.x0 as f32 - 6.0, y),
            egui::Align2::RIGHT_CENTER,
            fixed(z, if z_step < 1.0 { 1 } else { 0 }),
            small.clone(),
            pal.ink2,
        );
        z += z_step;
        guard += 1;
    }
    let frame = Rect::from_min_max(
        Pos2::new(rect.min.x + p.x0 as f32, rect.min.y + p.y1 as f32),
        Pos2::new(rect.min.x + p.x1 as f32, rect.min.y + p.y0 as f32),
    );
    painter.rect_stroke(frame, 0.0, Stroke::new(1.0, pal.ink), egui::StrokeKind::Middle);
    let ve = ((p.y0 - p.y1) / (p.z1 - p.z0)) / px_per_sta;
    let ve_text = if ve >= 10.0 { cc_civil::js::num_to_string(ve.round()) } else { fixed(ve, 1) };
    painter.text(frame.min + egui::vec2(6.0, 4.0), egui::Align2::LEFT_TOP, tr!("pv.ve", ve = ve_text), small.clone(), pal.ink2);
    let clip = painter.with_clip_rect(frame);
    // ---- existing ground
    let mut run: Vec<Pos2> = Vec::new();
    let egc = mix(pal.ink, bg, 0.25);
    for q in eg.iter() {
        match q.z {
            Some(z) => run.push(s(q.sta, z)),
            None => {
                if run.len() >= 2 {
                    clip.add(egui::Shape::line(std::mem::take(&mut run), Stroke::new(1.5, egc)));
                }
                run.clear();
            }
        }
    }
    if run.len() >= 2 {
        clip.add(egui::Shape::line(run, Stroke::new(1.5, egc)));
    }
    // ---- design: tangents dashed through the PVIs, the profile solid
    if pvis.len() >= 2 {
        let tl: Vec<Pos2> = pvis.iter().map(|v| s(numf(v.get("sta")), numf(v.get("z")))).collect();
        clip.add(egui::Shape::dashed_line(&tl, Stroke::new(1.0, mix(pal.ink, bg, 0.6)), 5.0, 4.0));
    }
    let fgc = pal.red;
    if prof.pvis.len() >= 2 {
        let line: Vec<Pos2> = prof.sample(len / 400.0).iter().filter_map(|q| q.z.map(|z| s(q.sta, z))).collect();
        clip.add(egui::Shape::line(line, Stroke::new(2.2, fgc)));
        let bold = egui::FontId::proportional(11.5);
        for g in &prof.grades {
            let m = (g.from + g.to) / 2.0;
            let (Some(za), Some(zb), Some(zm)) = (prof.elevation_at(g.from), prof.elevation_at(g.to), prof.elevation_at(m)) else {
                continue;
            };
            let (pa, pb) = (s(g.from, za), s(g.to, zb));
            if pb.x - pa.x < 60.0 {
                continue;
            }
            let at = s(m, zm) - egui::vec2(0.0, 4.0);
            let angle = (pb.y - pa.y).atan2(pb.x - pa.x);
            let galley = painter.layout_no_wrap(pct(g.g, 2), bold.clone(), fgc);
            let off = egui::vec2(-galley.size().x / 2.0, -galley.size().y);
            let rot = egui::emath::Rot2::from_angle(angle);
            painter.add(egui::epaint::TextShape::new(at + rot * off, galley, fgc).with_angle(angle));
        }
        for c in &prof.curves {
            let (pa, pb, pv) = (s(c.bvc, c.z_bvc), s(c.evc, c.z_evc), s(c.pvi, c.z_pvi));
            for q in [pa, pb] {
                painter.line_segment([q - egui::vec2(0.0, 8.0), q + egui::vec2(0.0, 8.0)], Stroke::new(1.0, fgc));
            }
            let mut up = c.crest;
            let mut ly = if up { pv.y.min(pa.y).min(pb.y) - 16.0 } else { pv.y.max(pa.y).max(pb.y) + 16.0 };
            if up && ly - 30.0 < frame.min.y {
                up = false;
                ly = pv.y.max(pa.y).max(pb.y) + 26.0;
            } else if !up && ly + 30.0 > frame.max.y {
                up = true;
                ly = pv.y.min(pa.y).min(pb.y) - 26.0;
            }
            let anchor = if up { egui::Align2::CENTER_BOTTOM } else { egui::Align2::CENTER_TOP };
            painter.text(Pos2::new(pv.x, ly), anchor, tr!("pv.lk", len = fixed(c.l, 2), k = fixed(c.k, 1)), bold.clone(), pal.ink);
            if let Some((hs, hz)) = c.hl {
                painter.circle_filled(s(hs, hz), 3.2, pal.green);
                let key = if c.crest { "pv.high" } else { "pv.low" };
                painter.text(
                    Pos2::new(pv.x, if up { ly - 14.0 } else { ly + 14.0 }),
                    anchor,
                    tr!(key, sta = format_station(hs, &units, None), z = fixed(hz, 2)),
                    bold.clone(),
                    pal.green,
                );
            }
        }
    }
    // ---- feature lines alongside (0.5 WL.20): dashed, in the feature colour; the selected one in the action colour
    let flc = fl_color(pal);
    for f in arr(Some(&feats)) {
        let on = f.get("id").and_then(Value::as_str).is_some_and(|i| feat_sel.as_deref() == Some(i));
        let col = if on { pal.action } else { flc };
        for r in arr(f.get("runs")) {
            let line: Vec<Pos2> = arr(Some(r)).iter().map(|q| s(numf(q.get("sta")), numf(q.get("z")))).collect();
            if line.len() >= 2 {
                clip.add(egui::Shape::dashed_line(&line, Stroke::new(1.6, col), 7.0, 4.0));
            }
        }
    }
    draw_features(&painter, &feats, feat_sel.as_deref(), &s, frame, pal, flc);
    // ---- PVIs
    let n = pvis.len();
    for (i, v) in pvis.iter().enumerate() {
        let (sta, z) = (numf(v.get("sta")), numf(v.get("z")));
        let q = s(sta, z);
        let sel = cv.profile.sel_pvi == Some(i);
        let hov = cv.profile.hover == Some(i);
        painter.circle(
            q,
            if sel || hov { 6.0 } else { 4.5 },
            bg,
            Stroke::new(if sel { 2.4 } else { 1.6 }, if sel { pal.action } else { fgc }),
        );
        if sel || n <= 8 {
            painter.text(
                q + egui::vec2(0.0, 9.0),
                egui::Align2::CENTER_TOP,
                tr!("pv.pvi", sta = format_station(sta, &units, None), z = fixed(z, 2)),
                small.clone(),
                pal.ink2,
            );
        }
    }
    if let Some((cid, sta)) = &cv.profile.cursor
        && *cid == id
    {
        let x = s(*sta, p.z0).x;
        painter.line_segment([Pos2::new(x, frame.min.y), Pos2::new(x, frame.max.y)], Stroke::new(1.0, pal.action));
    }
}

/// profile-view.js `drawFeatures`: each run named at its middle; each crossing a diamond with "<name> <elevation>"
/// upright above the mark (or below; flat beside it when neither fits).
fn draw_features(
    painter: &egui::Painter,
    feats: &Value,
    sel: Option<&str>,
    s: &dyn Fn(f64, f64) -> Pos2,
    frame: Rect,
    pal: &Palette,
    flc: Color32,
) {
    let font = egui::FontId::proportional(11.0);
    for f in arr(Some(feats)) {
        let on = f.get("id").and_then(Value::as_str).is_some_and(|i| sel == Some(i));
        let col = if on { pal.action } else { flc };
        let name = string_of(f.get("name"));
        for r in arr(f.get("runs")) {
            let r = arr(Some(r));
            let Some(m) = r.get(r.len() / 2) else { continue };
            let q = s(numf(m.get("sta")), numf(m.get("z")));
            if q.x < frame.min.x || q.x > frame.max.x {
                continue;
            }
            painter.text(q - egui::vec2(0.0, 5.0), egui::Align2::CENTER_BOTTOM, &name, font.clone(), col);
        }
        for c in arr(f.get("crossings")) {
            let z = numf(c.get("z"));
            let q = s(numf(c.get("sta")), z);
            if q.x < frame.min.x - 1.0 || q.x > frame.max.x + 1.0 {
                continue;
            }
            let d = 5.5;
            painter.add(egui::Shape::convex_polygon(
                vec![q - egui::vec2(0.0, d), q + egui::vec2(d, 0.0), q + egui::vec2(0.0, d), q - egui::vec2(d, 0.0)],
                col,
                Stroke::new(1.2, pal.bg),
            ));
            let text = tr!("pv.fl.cross", name = name.as_str(), z = fixed(z, 2));
            let galley = painter.layout_no_wrap(text, font.clone(), col);
            let (w, h) = (galley.size().x, galley.size().y);
            let room_up = q.y - 9.0 - frame.min.y;
            let room_down = frame.max.y - (q.y + 9.0);
            if w <= room_up || w <= room_down {
                let up = w <= room_up;
                // rotated −90°: the text runs upward from its start
                let angle = -std::f32::consts::FRAC_PI_2;
                let start = if up { Pos2::new(q.x - h / 2.0, q.y - 9.0) } else { Pos2::new(q.x - h / 2.0, q.y + 9.0 + w) };
                painter.add(egui::epaint::TextShape::new(start, galley, col).with_angle(angle));
            } else {
                let right = q.x + 9.0 + w <= frame.max.x;
                let y = (frame.min.y + 12.0).max(q.y - 6.0);
                let x = if right { q.x + 8.0 } else { q.x - 8.0 - w };
                painter.galley(Pos2::new(x, y - h), galley, col);
            }
        }
    }
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (f32::from(x) * (1.0 - t) + f32::from(y) * t).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plot_maps_both_ways_at_survey_stations() {
        let p = Plot::new(800.0, 240.0, 10_000.0, 12_500.0, 240.0, 270.0);
        let (x, y) = p.to_screen(11_234.56, 251.25);
        let (s, z) = p.to_data(x, y);
        assert!((s - 11_234.56).abs() < 1e-6 && (z - 251.25).abs() < 1e-9);
        assert_eq!(nice_step(30.0, &[25.0, 50.0]), 50.0);
        assert_eq!(nice_step(130.0, &[25.0, 50.0]), 150.0);
    }

    #[test]
    fn pvis_add_move_and_go_as_single_steps() {
        let mut cv = CivilView::demo().unwrap();
        cv.sel = Some(Sel::Alignment("a1".into()));
        cv.pvi_add(100.004, 253.336).unwrap();
        let pv = &cv.project.state["alignments"][0]["pvis"];
        assert_eq!(arr(Some(pv)).len(), 4);
        assert_eq!(pv[1]["sta"], serde_json::json!(100.0));
        assert_eq!(pv[1]["z"], serde_json::json!(253.34));
        assert_eq!(cv.profile.sel_pvi, Some(1));
        cv.pvi_add(100.001, 250.0).unwrap();
        assert!(cv.message.contains("already"), "{}", cv.message);
        cv.pvi_press(1, (0.0, 0.0));
        cv.pvi_drag_to(5000.0, 255.0, (40.0, 0.0));
        cv.pvi_release();
        let s1 = numf(cv.project.state["alignments"][0]["pvis"][1].get("sta"));
        assert!((s1 - 219.99).abs() < 1e-9, "clamped before the next PVI: {s1}");
        assert!(cv.delete_pvi());
        assert_eq!(arr(cv.project.state["alignments"][0].get("pvis")).len(), 3);
        assert_eq!(cv.project.undo_stack.len(), 3);
        cv.pvi_set(1, "sta", "2+10").unwrap();
        cv.pvi_set(1, "L", "80").unwrap();
        assert!(cv.pvi_set(0, "L", "80").is_err());
        assert!(cv.pvi_set(1, "z", "abc").is_err());
    }
}
