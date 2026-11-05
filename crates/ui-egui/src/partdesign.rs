//! PartCraft's More menu and the Design features in the app (M13.6): partcraft.js `newFeature` for the menu's
//! reference geometry, sweep, loft, draft, body, surface, rib, thread and sheet-metal tools as 0.5's mesh build runs
//! them (plane / axis / point are made from the selection; draft and thread are picked on the mesh; the rest say what
//! 0.5 says: "not built yet"), design-ui.js's panels (reference plane / axis / point, draft) with its pick slots
//! (`arm` / `onPick`: a Pick button waits for the next click in the view), its extra rows of extrude / hole / chamfer
//! / fillet, thread-ui.js's panel and a tapped hole's modelled thread, bodies-ui.js's "copy bodies" row of a pattern,
//! and the reference geometry drawn in the view (`refsForView`). Every change is one undo step with 0.5's label
//! (`part.set`, `part.slot`, `part.ref`, `part.region`, `part.sketch.on`).

use cc_part::{jv, part as P, readers as RD, threads as TH, vec as V};
use serde_json::Value;

use crate::partview::{Mode, PartView, edge_ref, face_ref, p3, ref_for_name, tr, trv};

/// The More menu (ui.inc `data-features-menu`): a command with its catalog key, or a heading (`None`).
pub const MORE_MENU: &[(Option<&str>, &str)] = &[
    (Some("revolve-cut"), "ui.tb.fmore.revolve_cut"),
    (None, "ui.tb.fmore.h.reference"),
    (Some("plane"), "ui.tb.fmore.plane"),
    (Some("axis"), "ui.tb.fmore.axis"),
    (Some("point"), "ui.tb.fmore.point"),
    (None, "ui.tb.fmore.h.sweep"),
    (Some("sweep"), "ui.tb.fmore.sweep"),
    (Some("loft"), "ui.tb.fmore.loft"),
    (Some("draft"), "ui.tb.fmore.draft"),
    (None, "ui.tb.fmore.h.bodies"),
    (Some("combine"), "ui.tb.fmore.combine"),
    (Some("movebody"), "ui.tb.fmore.movebody"),
    (Some("scalebody"), "ui.tb.fmore.scalebody"),
    (Some("pathpattern"), "ui.tb.fmore.pathpattern"),
    (None, "ui.tb.fmore.h.surfaces"),
    (Some("patch"), "ui.tb.fmore.patch"),
    (Some("surfloft"), "ui.tb.fmore.surfloft"),
    (Some("gordon"), "ui.tb.fmore.gordon"),
    (Some("stitch"), "ui.tb.fmore.stitch"),
    (Some("thicken"), "ui.tb.fmore.thicken"),
    (Some("offsetsurf"), "ui.tb.fmore.offsetsurf"),
    (Some("extendsurf"), "ui.tb.fmore.extendsurf"),
    (Some("trimsurf"), "ui.tb.fmore.trimsurf"),
    (None, "ui.tb.fmore.h.details"),
    (Some("rib"), "rib.menu"),
    (Some("thread"), "thread.menu"),
    (None, "ui.tb.fmore.h.sheet"),
    (Some("sheetbase"), "sheet.menu.base"),
    (Some("sheetflange"), "sheet.menu.flange"),
];

const SURFACES: [&str; 9] = ["form", "patch", "surfloft", "stitch", "thicken", "offsetsurf", "extendsurf", "trimsurf", "gordon"];
const BODIES: [&str; 4] = ["combine", "movebody", "scalebody", "pathpattern"];

/// design-ui.js's option lists.
pub const PLANE_MODES: [&str; 8] = ["offset", "angle", "3pt", "mid", "tangent", "normal", "edgept", "midnormal"];
pub const AXIS_MODES: [&str; 5] = ["2pt", "cyl", "planes", "edge", "normal"];
pub const POINT_MODES: [&str; 6] = ["xyz", "at", "center", "mid", "meet", "planes3"];

/// A Pick button waiting for a click in the view (design-ui.js `slot`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slot {
    pub id: String,
    pub key: String,
    /// plane, point, face, edge, axis, region
    pub kind: String,
    pub index: Option<usize>,
}

/// What a click in the view hit, for a slot (view3d.js `pickAt`'s hit).
#[derive(Clone, Debug)]
pub enum Hit {
    /// a face (index into the rebuild's faces) and the point clicked
    Face(usize, [f64; 3]),
    /// an edge and the segment of it under the pointer (None: the edge's middle stands in)
    Edge(usize, Option<([f64; 3], [f64; 3])>),
    /// a reference feature drawn in the view
    Ref(String),
    /// a point on the sketch plane of a region-picking extrude (sketch coordinates)
    Region([f64; 2]),
}

fn obj(pairs: Vec<(&str, Value)>) -> Value {
    jv::obj(pairs)
}

fn round_to(v: f64, dp: usize) -> f64 {
    cc_cad::js::str_to_number(&cc_part::js::to_fixed(v, dp))
}

fn round3(p: [f64; 3], dp: usize) -> Value {
    jv::pt3([round_to(p[0], dp), round_to(p[1], dp), round_to(p[2], dp)])
}

/// `String(+Number(v).toFixed(3))`.
fn fmt3(v: f64) -> String {
    cc_part::js::num_str(round_to(v, 3))
}

fn label_key(key: &str) -> String {
    tr(key)
}

impl PartView {
    fn stats_size(&self) -> f64 {
        let Some(st) = self.regen.stats.as_ref() else { return 100.0 };
        let (mn, mx) = (
            p3(st.get("bbox").and_then(|b| b.get("min")).unwrap_or(&Value::Null)),
            p3(st.get("bbox").and_then(|b| b.get("max")).unwrap_or(&Value::Null)),
        );
        cc_part::js::max(cc_part::js::max(mx[0] - mn[0], mx[1] - mn[1]), mx[2] - mn[2])
    }

    /// A More-menu command (partcraft.js `run(cmd)` → `newFeature`), as 0.5's mesh build runs it.
    pub fn more_feature(&mut self, cmd: &str) -> Result<(), String> {
        match cmd {
            "plane" | "axis" | "point" => self.new_ref_feature(cmd),
            // design-ui.js / surface-ui.js / bodies-ui.js `create`: exactWhy → "not built yet" (no exact kernel in 0.5)
            "sweep" | "loft" => self.note = trv("kernel.not_built", &[("what", tr(&format!("kernel.what.{cmd}")))]),
            c if SURFACES.contains(&c) || BODIES.contains(&c) => self.note = trv("kernel.not_built", &[("what", P::feature_label(c))]),
            // sheet-ui.js / rib-ui.js `create`
            "sheetbase" | "sheetflange" | "rib" => self.note = tr("sheet.ui.needs_exact"),
            "draft" | "thread" => self.new_feature(cmd, obj(vec![])),
            other => return Err(format!("part.feature: unknown tool {other}")),
        }
        Ok(())
    }

    /// design-ui.js `create(type)` for plane / axis / point, then partcraft.js's commit and status.
    fn new_ref_feature(&mut self, ty: &str) {
        let pick = self.pick.clone();
        let sel_f = self.sel_feature.as_deref().and_then(|id| P::feature_by_id(&self.part, id)).cloned();
        let props = match ty {
            "plane" => {
                let base = if let Some(r) = pick.as_ref().filter(|p| p.face && p.flat).and_then(|p| p.rf.clone()) {
                    r
                } else if let Some(n) = self.sel_plane.clone() {
                    obj(vec![("kind", "base".into()), ("name", n.into())])
                } else if let Some(f) = sel_f.as_ref().filter(|f| jv::s(f, "type") == "plane") {
                    obj(vec![("kind", "ref".into()), ("feature", jv::s(f, "id").into())])
                } else {
                    obj(vec![("kind", "base".into()), ("name", "top".into())])
                };
                let r = cc_part::js::round(self.stats_size() * 0.2);
                let d = if r == 0.0 || r.is_nan() { 10.0 } else { r };
                obj(vec![("mode", "offset".into()), ("planes", Value::Array(vec![base])), ("d", jv::num(d))])
            }
            "axis" => {
                let e = pick.as_ref().filter(|p| !p.face).and_then(|p| self.edges().get(p.index).cloned());
                let fc = pick.as_ref().filter(|p| p.face).and_then(|p| self.face(p.index).cloned());
                if let Some(e) = e.filter(|e| matches!(jv::s(e, "type"), "line" | "circle")) {
                    obj(vec![("mode", "edge".into()), ("edge", edge_ref(&e))])
                } else if let Some(f) = fc.filter(|f| jv::truthy(f.get("name")) && matches!(jv::s(f, "type"), "cylinder" | "cone")) {
                    obj(vec![("mode", "cyl".into()), ("face", face_ref(&f))])
                } else {
                    obj(vec![])
                }
            }
            _ => match pick.as_ref().filter(|p| !p.face).and_then(|p| self.edges().get(p.index).cloned()) {
                Some(e) => obj(vec![
                    ("mode", "at".into()),
                    (
                        "points",
                        Value::Array(vec![obj(vec![
                            ("kind", "vertex".into()),
                            ("edge", edge_ref(&e)),
                            ("p", e.get("a").cloned().unwrap_or(Value::Null)),
                        ])]),
                    ),
                ]),
                None => obj(vec![]),
            },
        };
        let label = P::feature_label(ty);
        let mut id = String::new();
        if let Err(e) = self.commit(&label, |p| {
            let f = P::add_feature(p, ty, &props)?;
            id = jv::s(&f, "id").to_string();
            Ok(())
        }) {
            self.note = e;
            return;
        }
        self.sel_feature = Some(id.clone());
        self.sel_plane = None;
        self.pick = None;
        self.multi.clear();
        self.sel_face = None;
        self.retint();
        let name = P::feature_by_id(&self.part, &id).map(|f| jv::s(f, "name").to_string()).unwrap_or_default();
        self.note = self.error_words(&id).unwrap_or_else(|| trv("app.feature.added_refs", &[("name", name)]));
    }

    // ------------------------------------------------------------------ words for references (design-ui.js)

    fn name_of(&self, id: &str) -> String {
        P::feature_by_id(&self.part, id).map_or_else(|| tr("ui.design.deleted_feature"), |f| jv::s(f, "name").to_string())
    }

    pub fn plane_words(&self, r: Option<&Value>) -> String {
        let Some(r) = r.filter(|r| jv::truthy(Some(r))) else { return tr("ui.design.none") };
        match jv::s(r, "kind") {
            "base" => crate::partview::plane_name(jv::s(r, "name")),
            "ref" => self.name_of(jv::s(r, "feature")),
            "face" => {
                let name = if jv::truthy(r.get("name")) {
                    jv::s(r, "name").to_string()
                } else {
                    let ent =
                        r.get("entity").and_then(Value::as_str).filter(|s| !s.is_empty()).map(|e| format!(":{e}")).unwrap_or_default();
                    format!("{}.{}{ent}", cc_part::js::str_of(r.get("feature")), cc_part::js::str_of(r.get("face")))
                };
                self.face_words(&name)
            }
            _ => tr("ui.design.fixed_plane"),
        }
    }

    pub fn point_words(&self, r: Option<&Value>) -> String {
        let Some(r) = r.filter(|r| jv::truthy(Some(r))) else { return tr("ui.design.none") };
        match jv::s(r, "kind") {
            "fixed" => format!(
                "({})",
                jv::arr(r.get("p")).iter().map(|x| V::fmt(x.as_f64().unwrap_or(f64::NAN), 3)).collect::<Vec<_>>().join(", ")
            ),
            "ref" => self.name_of(jv::s(r, "feature")),
            "sketch" => {
                let ent = jv::s(r, "entity");
                if ent == "O" {
                    trv("ui.design.origin_of", &[("name", self.name_of(jv::s(r, "sketch")))])
                } else {
                    trv("ui.design.point_of", &[("id", ent.to_string()), ("name", self.name_of(jv::s(r, "sketch")))])
                }
            }
            _ => {
                let fs = r.get("edge").map(|e| jv::arr(e.get("faces")).to_vec()).unwrap_or_default();
                let w = |i: usize| self.face_words(fs.get(i).and_then(Value::as_str).unwrap_or(""));
                trv("ui.design.edge_end", &[("a", w(0)), ("b", w(1))])
            }
        }
    }

    pub fn dir_words(&self, r: Option<&Value>) -> String {
        let Some(r) = r.filter(|r| jv::truthy(Some(r))) else { return tr("ui.design.none") };
        match jv::s(r, "kind") {
            "world" => trv("ui.design.world_axis", &[("axis", jv::s(r, "axis").to_uppercase())]),
            "ref" => self.name_of(jv::s(r, "feature")),
            "axis" => trv("ui.design.axis_of", &[("name", self.name_of(jv::s(r, "feature")))]),
            "line" => trv("ui.design.line_of", &[("name", self.name_of(jv::s(r, "sketch")))]),
            _ => tr("ui.design.picked_edge"),
        }
    }

    pub fn edge_words(&self, r: Option<&Value>) -> String {
        let Some(r) = r.filter(|r| jv::truthy(Some(r))) else { return tr("ui.design.none") };
        let fs = jv::arr(r.get("faces"));
        let w = |i: usize| self.face_words(fs.get(i).and_then(Value::as_str).unwrap_or(""));
        format!("{} × {}", w(0), w(1))
    }

    pub fn face_words_r(&self, r: Option<&Value>) -> String {
        match r.filter(|r| jv::truthy(Some(r))) {
            Some(r) => self.face_words(jv::s(r, "name")),
            None => tr("ui.design.none"),
        }
    }

    // ------------------------------------------------------------------ slots

    /// The undo label of a slot: the row's label, as design-ui.js's `planeSlot` / `dirSlot` pass it to `set`.
    pub fn slot_label(f: &Value, key: &str, index: Option<usize>) -> String {
        let ty = jv::s(f, "type");
        let mode = jv::s(f, "mode");
        let k = match (ty, key, index) {
            ("plane", "planes", Some(0)) => {
                if mode == "mid" {
                    "ui.design.first_plane"
                } else {
                    "ui.design.from_plane"
                }
            }
            ("plane" | "axis", "planes", Some(1)) => "ui.design.second_plane",
            ("axis", "planes", Some(0)) => {
                if mode == "normal" {
                    "ui.design.face"
                } else {
                    "ui.design.first_plane"
                }
            }
            ("point", "planes", Some(0)) => {
                if mode == "meet" {
                    "ui.design.meets_plane"
                } else {
                    "ui.design.first_plane"
                }
            }
            ("point", "planes", Some(1)) => "ui.design.second_plane",
            ("point", "planes", Some(2)) => "ui.design.third_plane",
            ("plane", "axis", _) => {
                if mode == "edgept" {
                    "ui.design.or_axis"
                } else {
                    "ui.design.turn_about"
                }
            }
            ("point", "axis", _) => "ui.design.or_axis",
            ("draft", "neutral", _) => "ui.design.neutral_plane",
            ("draft", "pull", _) => "ui.design.pull_direction",
            ("extrude", "upto", _) => "ui.design.up_to",
            _ => "ui.design.pick",
        };
        label_key(k)
    }

    /// design-ui.js `arm(f, key, kind, index)`: a Pick button (again = off).
    pub fn slot_arm(&mut self, spec: &str) -> Result<(), String> {
        if spec == "cancel" {
            self.slot_cancel();
            return Ok(());
        }
        let parts: Vec<&str> = spec.split(':').collect();
        let (fkey, key, index, kind) = match parts.as_slice() {
            [f, k, kind] => (*f, *k, None, *kind),
            [f, k, i, kind] => (*f, *k, Some(i.parse::<usize>().map_err(|_| format!("part.slot: {i} is not an index"))?), *kind),
            _ => return Err(format!("part.slot: feature:key[:index]:kind or cancel, not {spec}")),
        };
        if !matches!(kind, "plane" | "point" | "face" | "edge" | "axis" | "pathEdge" | "loftFace" | "region") {
            return Err(format!("part.slot: kind is plane, point, face, edge, axis or region, not {kind}"));
        }
        let id = self.feature_id(fkey).ok_or_else(|| format!("part.slot: no feature {fkey}"))?;
        let s = Slot { id: id.clone(), key: key.to_string(), kind: kind.to_string(), index };
        if self.slot.as_ref() == Some(&s) {
            self.slot = None;
            self.note = String::new();
            return Ok(());
        }
        if !matches!(kind, "plane" | "point" | "region" | "axis") && !self.named() {
            self.note = trv("kernel.not_built", &[("what", tr("kernel.what.pick"))]);
            return Ok(());
        }
        let name = P::feature_by_id(&self.part, &id).map(|f| jv::s(f, "name").to_string()).unwrap_or_default();
        let sk = match kind {
            "pathEdge" => "path_edge",
            "loftFace" => "loft_face",
            k => k,
        };
        self.note = trv(&format!("ui.design.slot.{sk}"), &[("name", name)]);
        self.slot = Some(s);
        Ok(())
    }

    /// design-ui.js `cancel()` (Esc).
    pub fn slot_cancel(&mut self) -> bool {
        if self.slot.take().is_none() {
            return false;
        }
        self.note = tr("ui.design.pick_cancelled");
        true
    }

    /// design-ui.js `onPick(h)`: a click in the view while a slot waits. True when taken.
    pub fn slot_pick(&mut self, h: Option<Hit>) -> bool {
        let Some(slot) = self.slot.clone() else { return false };
        let Some(f) = P::feature_by_id(&self.part, &slot.id).cloned() else {
            self.slot = None;
            return false;
        };
        let named = self.named();
        let face = match &h {
            Some(Hit::Face(i, _)) if named => self.face(*i).cloned(),
            _ => None,
        };
        let edge = match &h {
            Some(Hit::Edge(i, _)) if named => self.edges().get(*i).cloned(),
            _ => None,
        };
        let ref_hit = match &h {
            Some(Hit::Ref(id)) => P::feature_by_id(&self.part, id).cloned(),
            _ => None,
        };
        let fname = jv::s(&f, "name").to_string();
        let nope = |pv: &mut PartView, key: &str| {
            pv.note = tr(&format!("ui.design.nope.{key}"));
            true
        };
        let put = |pv: &mut PartView, what: &str, val: Value, keep: bool| {
            pv.slot = None;
            let (key, index) = (slot.key.clone(), slot.index);
            let r = pv.set_feature(&slot.id, &tr(&format!("ui.design.put.{what}")), |g| {
                if matches!(key.as_str(), "path" | "sections" | "rails" | "regions") {
                    let mut l = jv::arr(g.get(&key)).to_vec();
                    l.push(val);
                    jv::set(g, &key, Value::Array(l));
                } else if let Some(i) = index {
                    let mut l = jv::arr(g.get(&key)).to_vec();
                    while l.len() <= i {
                        l.push(Value::Null);
                    }
                    if let Some(x) = l.get_mut(i) {
                        *x = val;
                    }
                    jv::set(g, &key, Value::Array(l));
                } else {
                    jv::set(g, &key, val);
                }
            });
            if let Err(e) = r {
                pv.note = e;
                return true;
            }
            pv.note = trv(&format!("ui.design.put.{what}_set"), &[("name", fname.clone())]);
            if keep {
                pv.slot = Some(slot.clone());
            }
            true
        };
        let own = jv::s(&f, "id").to_string();
        match slot.kind.as_str() {
            "plane" => {
                if let Some(g) = ref_hit.as_ref().filter(|g| jv::s(g, "type") == "plane" && jv::s(g, "id") != own) {
                    return put(self, "plane", obj(vec![("kind", "ref".into()), ("feature", jv::s(g, "id").into())]), false);
                }
                if let Some(fc) = face.as_ref().filter(|fc| jv::s(fc, "type") == "plane") {
                    return if jv::truthy(fc.get("name")) {
                        put(self, "plane", ref_for_name(jv::s(fc, "name")), false)
                    } else {
                        nope(self, "face_unnamed")
                    };
                }
                nope(self, "plane")
            }
            "point" => {
                if let Some(g) = ref_hit.as_ref().filter(|g| jv::s(g, "type") == "point") {
                    return put(self, "point", obj(vec![("kind", "ref".into()), ("feature", jv::s(g, "id").into())]), false);
                }
                if let Some(e) = edge.as_ref() {
                    let seg = match &h {
                        Some(Hit::Edge(_, s)) => *s,
                        _ => None,
                    };
                    let (a, b) = (p3(e.get("a").unwrap_or(&Value::Null)), p3(e.get("b").unwrap_or(&Value::Null)));
                    let m = seg.map_or_else(|| p3(e.get("at").unwrap_or(&Value::Null)), |(s0, s1)| V::mul(V::add(s0, s1), 0.5));
                    let p = if V::len(V::sub(a, m)) <= V::len(V::sub(b, m)) { a } else { b };
                    return put(self, "point", obj(vec![("kind", "vertex".into()), ("edge", edge_ref(e)), ("p", round3(p, 6))]), false);
                }
                if let Some(Hit::Face(_, p)) = &h {
                    return put(self, "point", obj(vec![("kind", "fixed".into()), ("p", round3(*p, 6))]), false);
                }
                nope(self, "point")
            }
            "face" => match face.as_ref().filter(|fc| jv::truthy(fc.get("name"))) {
                Some(fc) => put(self, "face", face_ref(fc), false),
                None => nope(self, "face"),
            },
            "edge" => match edge.as_ref() {
                Some(e) => put(self, "edge", edge_ref(e), false),
                None => nope(self, "edge"),
            },
            "axis" => {
                if let Some(g) = ref_hit.as_ref().filter(|g| matches!(jv::s(g, "type"), "axis" | "plane") && jv::s(g, "id") != own) {
                    return put(self, "direction", obj(vec![("kind", "ref".into()), ("feature", jv::s(g, "id").into())]), false);
                }
                if let Some(Hit::Edge(_, seg)) = &h {
                    let (a, b) = match (edge.as_ref(), seg) {
                        (Some(e), _) => (p3(e.get("a").unwrap_or(&Value::Null)), p3(e.get("b").unwrap_or(&Value::Null))),
                        (None, Some((s0, s1))) => (*s0, *s1),
                        (None, None) => return nope(self, "axis"),
                    };
                    if edge.as_ref().is_some_and(|e| jv::s(e, "type") != "line") {
                        return nope(self, "curved");
                    }
                    let d = V::norm(V::sub(b, a));
                    return put(self, "direction", obj(vec![("kind", "fixed".into()), ("o", round3(a, 9)), ("d", round3(d, 12))]), false);
                }
                nope(self, "axis")
            }
            "pathEdge" => match edge.as_ref() {
                Some(e) => {
                    put(self, "path_edge", obj(vec![("edge", edge_ref(e))]), true);
                    self.note = trv("ui.design.path_edge_added", &[("name", fname.clone())]);
                    true
                }
                None => nope(self, "edge"),
            },
            "loftFace" => match face.as_ref().filter(|fc| jv::truthy(fc.get("name"))) {
                Some(fc) => put(self, "section", obj(vec![("face", face_ref(fc))]), false),
                None => nope(self, "flat_face"),
            },
            "region" => {
                let sk = jv::s(&f, "sketch").to_string();
                let Some(fr) = self.regen.frames.get(&sk).copied() else { return nope(self, "sketch_not_built") };
                let q = match &h {
                    Some(Hit::Region(q)) => *q,
                    _ => return nope(self, "sketch_not_built"),
                };
                let _ = fr;
                let qv = jv::nums(&[round_to(q[0], 6), round_to(q[1], 6)]);
                let at = format!("{}, {}", cc_part::js::num_str(round_to(q[0], 6)), cc_part::js::num_str(round_to(q[1], 6)));
                put(self, "region", qv, true);
                self.note = trv("ui.design.region_picked", &[("name", fname.clone()), ("at", at)]);
                true
            }
            _ => false,
        }
    }

    /// A click in the view for a region slot: the sketch-plane point under pixel (x, y) (design-ui.js `onPick`'s
    /// region branch). None (and the edge-on refusal) when the plane is seen edge-on.
    pub fn region_at(&mut self, x: f64, y: f64) -> Result<[f64; 2], String> {
        let slot = self.slot.clone().ok_or("no region slot")?;
        let f = P::feature_by_id(&self.part, &slot.id).cloned().ok_or("gone")?;
        let fr = self.regen.frames.get(jv::s(&f, "sketch")).copied().ok_or_else(|| tr("ui.design.nope.sketch_not_built"))?;
        let (o, d) = self.view.cam.ray(x, y);
        let (o, d) = ([o[0], o[2], -o[1]], [d[0], d[2], -d[1]]);
        let den = V::dot(fr.n, d);
        if den.abs() < 1e-9 {
            return Err(tr("ui.design.nope.edge_on"));
        }
        let tt = V::dot(V::sub(fr.o, o), fr.n) / den;
        let p = V::add(o, V::mul(d, tt));
        Ok(V::to_plane(&fr, p))
    }

    // ------------------------------------------------------------------ fields (`part.set`)

    /// The Design fields of `part.set` (design-ui.js `panel` / `extra`, thread-ui.js, bodies-ui.js `extra`).
    pub fn set_design_field(&mut self, id: &str, f: &Value, field: &str, value: &str) -> Result<(), String> {
        let ty = jv::s(f, "type").to_string();
        let on = matches!(value, "true" | "1" | "on" | "yes");
        let lbl = |k: &str| tr(k);
        // params-panel.js `typed`: a field that is no parameter target (its number not set yet) takes nothing; a bad
        // number is said in the status line
        macro_rules! num {
            ($k:expr, $min:expr, $max:expr) => {{
                if cc_part::params_part::targets(&self.part).get(&format!("{id}.{}", $k)).is_none() {
                    return Ok(());
                }
                match self.typed_number(id, $k, value, $min, $max) {
                    Ok(Some(v)) => v,
                    Ok(None) => return Ok(()),
                    Err(e) => {
                        self.note = e;
                        return Ok(());
                    }
                }
            }};
        }
        let mode_ok = |modes: &[&str]| -> Result<(), String> {
            if modes.contains(&value) { Ok(()) } else { Err(format!("part.set: mode is one of {}, not {value}", modes.join(", "))) }
        };
        match (ty.as_str(), field) {
            ("plane", "mode") => {
                mode_ok(&PLANE_MODES)?;
                self.set_feature(id, &lbl("ui.design.undo.plane_type"), |g| jv::set(g, "mode", value.into()))?;
            }
            ("axis", "mode") => {
                mode_ok(&AXIS_MODES)?;
                self.set_feature(id, &lbl("ui.design.undo.axis_type"), |g| jv::set(g, "mode", value.into()))?;
            }
            ("point", "mode") => {
                mode_ok(&POINT_MODES)?;
                self.set_feature(id, &lbl("ui.design.undo.point_type"), |g| {
                    jv::set(g, "mode", value.into());
                    if (value == "meet" || value == "planes3") && !jv::truthy(g.get("planes")) {
                        jv::set(g, "planes", Value::Array(vec![]));
                    }
                })?;
            }
            ("plane", "d") => {
                let v = num!("d", None, None);
                self.set_feature(id, &lbl("ui.design.undo.distance"), |g| jv::set(g, "d", jv::num(v)))?;
            }
            ("plane", "angle") => {
                let v = num!("angle", Some(-360.0), Some(360.0));
                self.set_feature(id, &lbl("ui.design.undo.angle"), |g| jv::set(g, "angle", jv::num(v)))?;
            }
            ("plane", "flip") => self.set_feature(id, &lbl("ui.design.undo.flip"), |g| jv::set(g, "flip", on.into()))?,
            ("point", "flip") => self.set_feature(id, &lbl("ui.design.undo.other_crossing"), |g| {
                if on {
                    jv::set(g, "flip", true.into());
                } else {
                    jv::del(g, "flip");
                }
            })?,
            ("point", p) if p.starts_with("p:") => {
                let i: usize = p
                    .trim_start_matches("p:")
                    .parse()
                    .ok()
                    .filter(|i| *i < 3)
                    .ok_or_else(|| format!("part.set: {p} is p:0, p:1 or p:2"))?;
                let v = cc_part::js::str_num(value);
                if value.trim().is_empty() || !v.is_finite() {
                    self.note = tr("app.input.number");
                    return Ok(());
                }
                self.set_feature(id, &lbl("ui.design.undo.coordinates"), |g| {
                    let mut l = jv::arr(g.get("p")).to_vec();
                    while l.len() <= i {
                        l.push(Value::Null);
                    }
                    if let Some(x) = l.get_mut(i) {
                        *x = jv::num(v);
                    }
                    jv::set(g, "p", Value::Array(l));
                })?;
            }
            // planeSlot's select: base:<front|top|right> | ref:<id> (the label is the row's)
            ("plane" | "axis" | "point" | "draft" | "extrude", k) if k.starts_with("planes:") || k == "neutral" || k == "upto" => {
                if value == "cur" || value.is_empty() {
                    return Ok(());
                }
                let (key, index) = match k.strip_prefix("planes:") {
                    Some(i) => ("planes", Some(i.parse::<usize>().map_err(|_| format!("part.set: {k}"))?)),
                    None => (k, None),
                };
                let nr = if let Some(r) = value.strip_prefix("ref:") {
                    obj(vec![("kind", "ref".into()), ("feature", r.into())])
                } else {
                    let n = value.strip_prefix("base:").unwrap_or(value);
                    if !matches!(n, "front" | "top" | "right") {
                        return Err(format!("part.set: a plane is base:front|top|right or ref:<id>, not {value}"));
                    }
                    obj(vec![("kind", "base".into()), ("name", n.into())])
                };
                let label = Self::slot_label(f, key, index);
                self.set_feature(id, &label, |g| match index {
                    None => jv::set(g, key, nr),
                    Some(i) => {
                        let mut l = jv::arr(g.get(key)).to_vec();
                        while l.len() <= i {
                            l.push(Value::Null);
                        }
                        if let Some(x) = l.get_mut(i) {
                            *x = nr;
                        }
                        jv::set(g, key, Value::Array(l));
                    }
                })?;
            }
            // dirSlot's select: world:x|y|z | ref:<id> | axis:<id> | "" (none, where allowed)
            ("plane" | "point", "axis") | ("draft", "pull") => {
                if value == "cur" {
                    return Ok(());
                }
                let nr = if value.is_empty() {
                    Value::Null
                } else {
                    let (k, a) = value.split_once(':').unwrap_or((value, ""));
                    match k {
                        "world" if matches!(a, "x" | "y" | "z") => obj(vec![("kind", "world".into()), ("axis", a.into())]),
                        "ref" => obj(vec![("kind", "ref".into()), ("feature", a.into())]),
                        "axis" => obj(vec![("kind", "axis".into()), ("feature", a.into())]),
                        _ => return Err(format!("part.set: a direction is world:x|y|z, ref:<id> or axis:<id>, not {value}")),
                    }
                };
                let label = Self::slot_label(f, field, None);
                self.set_feature(id, &label, |g| jv::set(g, field, nr))?;
            }
            ("draft", "angle") => {
                let v = num!("angle", Some(-89.0), Some(89.0));
                self.set_feature(id, &lbl("ui.design.undo.draft_angle"), |g| jv::set(g, "angle", jv::num(v)))?;
            }
            ("draft" | "thread", d) if d.starts_with("drop:") => {
                let i: usize = d.trim_start_matches("drop:").parse().map_err(|_| format!("part.set: {d} is not drop:<number>"))?;
                self.set_feature(id, &lbl("app.undo.drop_face"), |g| {
                    if let Some(l) = g.get_mut("faces").and_then(Value::as_array_mut)
                        && i < l.len()
                    {
                        l.remove(i);
                    }
                })?;
            }
            // extrude's extra rows
            ("extrude", "offset") => {
                let v = num!("offset", None, None);
                self.set_feature(id, &lbl("ui.design.undo.offset"), |g| jv::set(g, "offset", jv::num(v)))?;
            }
            ("extrude", "two") => self.set_feature(id, &lbl("ui.design.second_direction"), |g| {
                if on {
                    jv::set(g, "two", true.into());
                    let d2 = jv::as_num(g.get("depth2")).filter(|x| *x != 0.0 && !x.is_nan()).unwrap_or(10.0);
                    jv::set(g, "depth2", jv::num(d2));
                } else {
                    jv::del(g, "two");
                }
            })?,
            ("extrude", "depth2") => {
                let v = num!("depth2", Some(0.000001), None);
                self.set_feature(id, &lbl("ui.design.undo.second_depth"), |g| jv::set(g, "depth2", jv::num(v)))?;
            }
            ("extrude", "taper") => {
                let v = num!("taper", Some(-89.0), Some(89.0));
                self.set_feature(id, &lbl("ui.design.undo.taper"), |g| {
                    if v != 0.0 && !v.is_nan() {
                        jv::set(g, "taper", jv::num(v));
                    } else {
                        jv::del(g, "taper");
                    }
                })?;
            }
            ("extrude", "regions") => {
                if value != "all" {
                    return Err("part.set: regions=all (the picks are part.slot … :region)".into());
                }
                self.set_feature(id, &lbl("ui.design.regions"), |g| jv::del(g, "regions"))?;
            }
            // hole's extra rows
            ("hole", "csinkD") => {
                let v = num!("csinkD", Some(0.000001), None);
                self.set_feature(id, &lbl("ui.design.undo.csink_d"), |g| jv::set(g, "csinkD", jv::num(v)))?;
            }
            ("hole", "csinkAngle") => {
                if !matches!(value, "90" | "82") {
                    return Err(format!("part.set: csinkAngle is 90 or 82, not {value}"));
                }
                let n = cc_part::js::str_num(value);
                self.set_feature(id, &lbl("ui.design.csink_angle"), |g| jv::set(g, "csinkAngle", jv::num(n)))?;
            }
            ("hole", "size") => {
                if RD::iso_coarse(value).is_none() {
                    return Err(format!("part.set: size is an ISO coarse size (M2 … M24), not {value}"));
                }
                self.set_feature(id, &lbl("ui.design.undo.thread_size"), |g| jv::set(g, "size", value.into()))?;
            }
            ("hole", "threadDepth") => {
                let v = num!("threadDepth", Some(0.000001), None);
                self.set_feature(id, &lbl("ui.design.undo.thread_depth"), |g| jv::set(g, "threadDepth", jv::num(v)))?;
            }
            ("hole" | "thread", "modelled") => {
                let hole = ty == "hole";
                self.set_feature(id, &lbl("thread.ui.undo.modelled"), |g| {
                    if on {
                        jv::set(g, "modelled", true.into());
                        if jv::nullish(g.get("clearance")) {
                            jv::set(g, "clearance", jv::num(TH::DEFAULT_CLEARANCE));
                        }
                    } else if hole {
                        jv::del(g, "modelled");
                        jv::del(g, "clearance");
                    } else {
                        jv::set(g, "modelled", false.into());
                    }
                })?;
            }
            ("hole" | "thread", "clearance") => {
                let v = num!("clearance", Some(0.0), Some(5.0));
                self.set_feature(id, &lbl("thread.ui.undo.clearance"), |g| jv::set(g, "clearance", jv::num(v)))?;
            }
            // chamfer / fillet extra rows
            ("chamfer", "mode") => {
                if !matches!(value, "equal" | "two" | "angle") {
                    return Err(format!("part.set: mode is equal, two or angle, not {value}"));
                }
                self.set_feature(id, &lbl("ui.design.undo.chamfer_type"), |g| {
                    if value == "equal" {
                        for k in ["mode", "d2", "angle", "flip"] {
                            jv::del(g, k);
                        }
                    } else {
                        jv::set(g, "mode", value.into());
                        if jv::nullish(g.get("d2")) {
                            let d = jv::n(g, "d");
                            jv::set(g, "d2", jv::num(d * 2.0));
                        }
                        if jv::nullish(g.get("angle")) {
                            jv::set(g, "angle", 45.into());
                        }
                        let fl = jv::truthy(g.get("flip"));
                        jv::set(g, "flip", fl.into());
                    }
                })?;
            }
            ("chamfer", "d2") => {
                let v = num!("d2", Some(0.000001), None);
                self.set_feature(id, &lbl("ui.design.second_distance"), |g| jv::set(g, "d2", jv::num(v)))?;
            }
            ("chamfer", "angle") => {
                let v = num!("angle", Some(0.001), Some(89.999));
                self.set_feature(id, &lbl("ui.design.undo.chamfer_angle"), |g| jv::set(g, "angle", jv::num(v)))?;
            }
            ("chamfer", "flip") => self.set_feature(id, &lbl("ui.design.undo.flip"), |g| jv::set(g, "flip", on.into()))?,
            ("fillet", "variable") => self.set_feature(id, &lbl("ui.design.undo.variable_radius"), |g| {
                if on {
                    let r = jv::n(g, "r");
                    jv::set(g, "r2", jv::num(r * 2.0));
                    jv::set(g, "flip", false.into());
                } else {
                    jv::del(g, "r2");
                    jv::del(g, "flip");
                }
            })?,
            ("fillet", "r2") => {
                let v = num!("r2", Some(0.000001), None);
                self.set_feature(id, &lbl("ui.design.undo.end_radius"), |g| jv::set(g, "r2", jv::num(v)))?;
            }
            ("fillet", "flip") => self.set_feature(id, &lbl("ui.design.undo.swap_ends"), |g| jv::set(g, "flip", on.into()))?,
            // thread-ui.js
            ("thread", "form") => {
                if !matches!(value, "iso" | "tr") {
                    return Err(format!("part.set: form is iso or tr, not {value}"));
                }
                self.set_feature(id, &lbl("thread.ui.undo.form"), |g| {
                    jv::set(g, "form", value.into());
                    jv::set(g, "size", "auto".into());
                })?;
            }
            ("thread", "size") => {
                let form = jv::s(f, "form");
                if value != "auto" && TH::table_row(form, value).is_none() {
                    return Err(format!("part.set: size is auto or a {form} size, not {value}"));
                }
                self.set_feature(id, &lbl("thread.ui.undo.size"), |g| jv::set(g, "size", value.into()))?;
            }
            ("thread", "full") => self.set_feature(id, &lbl("thread.ui.undo.length"), |g| {
                jv::set(g, "length", if on { Value::Null } else { 10.into() });
            })?,
            ("thread", "length") => {
                let v = num!("length", Some(1e-3), None);
                self.set_feature(id, &lbl("thread.ui.undo.length"), |g| jv::set(g, "length", jv::num(v)))?;
            }
            ("thread", "flip") => self.set_feature(id, &lbl("thread.ui.undo.flip"), |g| jv::set(g, "flip", on.into()))?,
            ("thread", "hand") => {
                if !matches!(value, "right" | "left") {
                    return Err(format!("part.set: hand is right or left, not {value}"));
                }
                self.set_feature(id, &lbl("thread.ui.undo.hand"), |g| jv::set(g, "hand", value.into()))?;
            }
            // bodies-ui.js `extra`: copy bodies (the mesh kernel keeps no bodies: nothing to copy)
            ("lpattern" | "cpattern", "copybodies") => self.set_feature(id, &lbl("ui.bodies.undo.copy_bodies"), |g| {
                if !on {
                    jv::del(g, "bodies");
                }
            })?,
            _ => return Err(format!("part.set: a {ty} has no field {field} here")),
        }
        Ok(())
    }

    /// `part.ref`: a click on reference geometry drawn in the view (partcraft.js `onPick` with a 'ref' hit).
    pub fn ref_clicked(&mut self, id: &str) {
        if self.mode == Mode::Pick || self.mode == Mode::Sketch {
            return;
        }
        if self.slot_pick(Some(Hit::Ref(id.to_string()))) {
            return;
        }
        let Some(rf) = P::feature_by_id(&self.part, id).cloned() else { return };
        if self.pending_sketch && jv::s(&rf, "type") == "plane" {
            self.start_sketch_on(obj(vec![("kind", "ref".into()), ("feature", id.into())]));
            return;
        }
        self.pick = None;
        self.select_feature(Some(id.to_string()));
    }

    /// design-ui.js `refsForView`: (feature, what) of the reference geometry to draw, part frame.
    pub fn refs_for_view(&self) -> Vec<(String, RefDraw)> {
        let s = cc_part::js::max(5.0, self.stats_size() * 0.45);
        let mut out = Vec::new();
        for (id, g) in self.regen.ref_geom.iter() {
            if P::feature_by_id(&self.part, id).is_none() || !self.regen.results.get(id).is_some_and(|r| jv::is_str(r.get("status"), "ok"))
            {
                continue;
            }
            let d = match g {
                cc_part::refgeom::Ref::Plane { frame, center } => RefDraw::Plane(cc_part::refgeom::plane_quad(frame, *center, s)),
                cc_part::refgeom::Ref::Axis { o, d } => RefDraw::Axis(V::add(*o, V::mul(*d, -s * 1.4)), V::add(*o, V::mul(*d, s * 1.4))),
                cc_part::refgeom::Ref::Point { p } => RefDraw::Point(*p),
            };
            out.push((id.to_string(), d));
        }
        out
    }

    /// The reference feature under pixel (x, y) of the view: lines and points within `tol` pixels, else a plane's fill.
    pub fn ref_at(&self, x: f64, y: f64, tol: f64, behind_solid: bool) -> Option<String> {
        let refs = self.refs_for_view();
        let near_seg = |a: (f64, f64), b: (f64, f64)| {
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let l2 = dx * dx + dy * dy;
            let t = if l2 > 0.0 { (((x - a.0) * dx + (y - a.1) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
            (x - a.0 - t * dx).hypot(y - a.1 - t * dy)
        };
        let mut best: Option<(f64, String)> = None;
        for (id, d) in &refs {
            let dist = match d {
                RefDraw::Axis(a, b) => match (self.screen_of(*a), self.screen_of(*b)) {
                    (Some(sa), Some(sb)) => near_seg(sa, sb),
                    _ => f64::INFINITY,
                },
                RefDraw::Point(p) => self.screen_of(*p).map_or(f64::INFINITY, |s| (s.0 - x).hypot(s.1 - y)),
                RefDraw::Plane(q) => {
                    let pts: Vec<(f64, f64)> = q.iter().filter_map(|p| self.screen_of(*p)).collect();
                    let n = pts.len();
                    (0..n).filter_map(|i| Some(near_seg(*pts.get(i)?, *pts.get((i + 1) % n)?))).fold(f64::INFINITY, f64::min)
                }
            };
            if dist <= tol && best.as_ref().is_none_or(|(bd, _)| dist < *bd) {
                best = Some((dist, id.clone()));
            }
        }
        if best.is_some() || behind_solid {
            return best.map(|b| b.1);
        }
        // a plane's fill (behind the solid: only where no face is hit)
        for (id, d) in &refs {
            if let RefDraw::Plane(q) = d {
                let pts: Vec<(f64, f64)> = q.iter().filter_map(|p| self.screen_of(*p)).collect();
                if pts.len() == 4 && inside(&pts, x, y) {
                    return Some(id.clone());
                }
            }
        }
        None
    }

    /// `part.region=u,v`: a region click on the sketch plane (sketch coordinates).
    pub fn region_cmd(&mut self, name: &str) -> Result<(), String> {
        let v: Vec<f64> = name.split(',').map(|x| x.trim().parse::<f64>().unwrap_or(f64::NAN)).collect();
        let [u, w] = v.as_slice() else { return Err(format!("part.region: u,v, not {name}")) };
        if !u.is_finite() || !w.is_finite() {
            return Err(format!("part.region: u,v, not {name}"));
        }
        if self.slot.as_ref().is_none_or(|s| s.kind != "region") {
            return Err("part.region: no region slot is waiting".into());
        }
        self.slot_pick(Some(Hit::Region([*u, *w])));
        Ok(())
    }
}

/// Reference geometry to draw (part frame).
#[derive(Clone, Debug)]
pub enum RefDraw {
    Plane(Vec<[f64; 3]>),
    Axis([f64; 3], [f64; 3]),
    Point([f64; 3]),
}

fn inside(pts: &[(f64, f64)], x: f64, y: f64) -> bool {
    let mut c = false;
    let n = pts.len();
    for i in 0..n {
        let (Some(&a), Some(&b)) = (pts.get(i), pts.get((i + n - 1) % n)) else { continue };
        if (a.1 > y) != (b.1 > y) && x < (b.0 - a.0) * (y - a.1) / (b.1 - a.1) + a.0 {
            c = !c;
        }
    }
    c
}

/// thread-ui.js's size options: (value, words).
pub fn thread_sizes(form: &str) -> Vec<(String, String)> {
    let rows: Vec<&str> =
        if form == "tr" { TH::ISO_TRAPEZOIDAL.iter().map(|r| r.0).collect() } else { TH::ISO_METRIC.iter().map(|r| r.0).collect() };
    let mut out = vec![("auto".to_string(), tr("thread.ui.size_auto"))];
    for k in rows {
        let w = if form == "tr" {
            TH::size_label(form, k)
        } else {
            let p = TH::thread_dims(form, k).map_or(f64::NAN, |d| d.p);
            trv("thread.ui.size_option", &[("size", k.to_string()), ("pitch", fmt3(p))])
        };
        out.push((k.to_string(), w));
    }
    out
}

/// threads.js `THREAD_SOURCES[form]`.
pub fn thread_source(form: &str) -> &'static str {
    if form == "tr" { "ISO 2901 basic profile, ISO 2904 pitches" } else { "ISO 68-1 basic profile, ISO 261 coarse pitches" }
}

/// design-features.js `THREAD_SOURCE`.
pub const HOLE_THREAD_SOURCE: &str = "ISO 261 coarse pitch; tap drill per ISO 2306 (6H)";

/// The hole's thread options (design-ui.js: "M6 × 1 (tap drill 5)").
pub fn hole_sizes() -> Vec<(String, String)> {
    RD::ISO_COARSE
        .iter()
        .map(|(k, _, p, d)| {
            (
                (*k).to_string(),
                trv(
                    "ui.design.thread_option",
                    &[("size", (*k).to_string()), ("pitch", cc_part::js::num_str(*p)), ("drill", cc_part::js::num_str(*d))],
                ),
            )
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------- the panels

use crate::partpanel::{Cmd, cmd, combo, num_row};
use crate::theme::Tokens;
use egui::RichText;

/// A check box in the grid's second column (long labels would widen the first and push the fields away).
fn check_row(ui: &mut egui::Ui, on: bool, label: &str) -> Option<bool> {
    ui.label("");
    let mut v = on;
    let r = ui.checkbox(&mut v, label).changed().then_some(v);
    ui.end_row();
    r
}

/// A note in the grid's second column.
fn note_row(ui: &mut egui::Ui, t: &Tokens, text: String) {
    ui.label("");
    ui.add(egui::Label::new(RichText::new(text).small().color(t.text_dim)).wrap());
    ui.end_row();
}

fn set_cmd(act: &mut Cmd, id: &str, field: &str, v: impl Into<String>) {
    *act = cmd("part.set", format!("{id}:{field}={}", v.into()));
}

/// A Pick button of a slot (design-ui.js `pickBtn`).
#[allow(clippy::too_many_arguments)]
fn pick_btn(ui: &mut egui::Ui, pv: &PartView, id: &str, key: &str, kind: &str, index: Option<usize>, label: Option<String>, act: &mut Cmd) {
    let armed = pv.slot.as_ref().is_some_and(|s| s.id == id && s.key == key && s.index == index);
    let words = if armed { tr("ui.design.picking") } else { label.unwrap_or_else(|| tr("ui.design.pick")) };
    if ui.add(egui::Button::selectable(armed, words).small()).clicked() {
        let i = index.map(|i| format!(":{i}")).unwrap_or_default();
        *act = cmd("part.slot", format!("{id}:{key}{i}:{kind}"));
    }
}

fn earlier(pv: &PartView, id: &str) -> Vec<Value> {
    let at = P::index_of(&pv.part, id).unwrap_or(0);
    P::features(&pv.part).iter().take(at).cloned().collect()
}

/// design-ui.js `planeSlot`.
fn plane_slot(ui: &mut egui::Ui, pv: &PartView, f: &Value, key: &str, index: Option<usize>, label: &str, act: &mut Cmd) {
    let id = jv::s(f, "id");
    let cur = match index {
        None => f.get(key).cloned(),
        Some(i) => f.get(key).and_then(|a| a.get(i)).cloned(),
    }
    .filter(|r| jv::truthy(Some(r)));
    let mut opts: Vec<(String, String)> =
        ["front", "top", "right"].iter().map(|k| (format!("base:{k}"), tr(&format!("ui.design.{k}_plane")))).collect();
    for g in earlier(pv, id).iter().filter(|g| jv::s(g, "type") == "plane") {
        opts.push((format!("ref:{}", jv::s(g, "id")), jv::s(g, "name").to_string()));
    }
    let val = match cur.as_ref() {
        None => String::new(),
        Some(r) if jv::s(r, "kind") == "base" => format!("base:{}", jv::s(r, "name")),
        Some(r) if jv::s(r, "kind") == "ref" => format!("ref:{}", jv::s(r, "feature")),
        Some(_) => "cur".into(),
    };
    if val == "cur" || cur.is_none() {
        opts.push((val.clone(), if cur.is_some() { pv.plane_words(cur.as_ref()) } else { tr("ui.design.pick_one") }));
    }
    let field = match index {
        Some(i) => format!("{key}:{i}"),
        None => key.to_string(),
    };
    let salt = format!("pc-ps-{id}-{field}");
    ui.label(label);
    ui.horizontal(|ui| {
        let shown = opts.iter().find(|(v, _)| *v == val).map_or_else(String::new, |(_, l)| l.clone());
        egui::ComboBox::from_id_salt(salt).selected_text(shown).width(130.0).show_ui(ui, |ui| {
            for (v, l) in &opts {
                if ui.selectable_label(*v == val, l).clicked() && *v != val && !v.is_empty() && v != "cur" {
                    set_cmd(act, id, &field, v.clone());
                }
            }
        });
        pick_btn(ui, pv, id, key, "plane", index, None, act);
    });
    ui.end_row();
}

/// design-ui.js `pointSlot` / `faceSlot` / `edgeSlot`: the words and a Pick button.
#[allow(clippy::too_many_arguments)]
fn words_slot(ui: &mut egui::Ui, pv: &PartView, f: &Value, key: &str, index: Option<usize>, kind: &str, label: &str, act: &mut Cmd) {
    let id = jv::s(f, "id");
    let cur = match index {
        None => f.get(key),
        Some(i) => f.get(key).and_then(|a| a.get(i)),
    };
    let words = match kind {
        "point" => pv.point_words(cur),
        "face" => pv.face_words_r(cur),
        _ => pv.edge_words(cur),
    };
    ui.label(label);
    ui.horizontal(|ui| {
        ui.label(RichText::new(words).small());
        pick_btn(ui, pv, id, key, kind, index, None, act);
    });
    ui.end_row();
}

/// design-ui.js `dirSlot`.
fn dir_slot(ui: &mut egui::Ui, pv: &PartView, f: &Value, key: &str, label: &str, allow_null: Option<String>, act: &mut Cmd) {
    let id = jv::s(f, "id");
    let cur = f.get(key).filter(|r| jv::truthy(Some(r)));
    let mut opts: Vec<(String, String)> = Vec::new();
    if let Some(n) = &allow_null {
        opts.push((String::new(), n.clone()));
    }
    for a in ["x", "y", "z"] {
        opts.push((format!("world:{a}"), trv("ui.design.world_axis", &[("axis", a.to_uppercase())])));
    }
    for g in earlier(pv, id) {
        let (gid, gname) = (jv::s(&g, "id").to_string(), jv::s(&g, "name").to_string());
        match jv::s(&g, "type") {
            "axis" => opts.push((format!("ref:{gid}"), gname)),
            "plane" => opts.push((format!("ref:{gid}"), trv("ui.design.its_normal", &[("name", gname)]))),
            "revolve" => opts.push((format!("axis:{gid}"), trv("ui.design.axis_of_opt", &[("name", gname)]))),
            _ => {}
        }
    }
    let val = match cur {
        None => String::new(),
        Some(r) => match jv::s(r, "kind") {
            "world" => format!("world:{}", jv::s(r, "axis")),
            "ref" => format!("ref:{}", jv::s(r, "feature")),
            "axis" => format!("axis:{}", jv::s(r, "feature")),
            _ => "cur".into(),
        },
    };
    if val == "cur" {
        opts.push(("cur".into(), pv.dir_words(cur)));
    }
    if cur.is_none() && allow_null.is_none() {
        opts.insert(0, (String::new(), tr("ui.design.pick_one")));
    }
    ui.label(label);
    ui.horizontal(|ui| {
        let shown = opts.iter().find(|(v, _)| *v == val).map_or_else(String::new, |(_, l)| l.clone());
        egui::ComboBox::from_id_salt(format!("pc-ds-{id}-{key}")).selected_text(shown).width(130.0).show_ui(ui, |ui| {
            for (v, l) in &opts {
                if ui.selectable_label(*v == val, l).clicked() && *v != val && v != "cur" {
                    set_cmd(act, id, key, v.clone());
                }
            }
        });
        pick_btn(ui, pv, id, key, "axis", None, None, act);
    });
    ui.end_row();
}

fn mode_combo(ui: &mut egui::Ui, f: &Value, modes: &[&str], prefix: &str, act: &mut Cmd) {
    let id = jv::s(f, "id");
    let opts: Vec<(String, String)> = modes.iter().map(|m| ((*m).to_string(), tr(&format!("{prefix}{m}")))).collect();
    if let Some(v) = combo(ui, format!("pc-mode-{id}"), &tr("ui.design.type"), jv::s(f, "mode"), &opts) {
        set_cmd(act, id, "mode", v);
    }
}

fn facts(ui: &mut egui::Ui, t: &Tokens, rows: &[(String, String)]) {
    egui::Grid::new(ui.next_auto_id()).num_columns(2).spacing([8.0, 2.0]).show(ui, |ui| {
        for (k, v) in rows {
            ui.label(RichText::new(k).small().color(t.text_dim));
            ui.label(RichText::new(v).small().color(t.text));
            ui.end_row();
        }
    });
}

fn note(ui: &mut egui::Ui, t: &Tokens, key: &str) {
    crate::shell::note(ui, t, &tr(key));
}

fn fmt_v(p: [f64; 3], dp: usize) -> String {
    p.iter().map(|v| V::fmt(*v, dp)).collect::<Vec<_>>().join(", ")
}

/// design-ui.js `panel(f)` (plane, axis, point, draft) and thread-ui.js `panel(f)`. True when it drew the panel.
pub fn design_panel(ui: &mut egui::Ui, pv: &mut PartView, f: &Value, t: &Tokens, act: &mut Cmd) -> bool {
    let id = jv::s(f, "id").to_string();
    let ty = jv::s(f, "type").to_string();
    let mode = jv::s(f, "mode").to_string();
    let ok = pv.regen.results.get(&id).is_some_and(|r| jv::is_str(r.get("status"), "ok"));
    let g = pv.regen.ref_geom.get(&id).cloned();
    match ty.as_str() {
        "plane" => {
            egui::Grid::new(("pc-dz", id.clone())).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                mode_combo(ui, f, &PLANE_MODES, "ui.design.plane_mode.", act);
                if matches!(mode.as_str(), "offset" | "angle" | "mid") {
                    let l = tr(if mode == "mid" { "ui.design.first_plane" } else { "ui.design.from_plane" });
                    plane_slot(ui, pv, f, "planes", Some(0), &l, act);
                }
                if mode == "mid" {
                    plane_slot(ui, pv, f, "planes", Some(1), &tr("ui.design.second_plane"), act);
                }
                if mode == "offset" {
                    num_row(ui, pv, &id, "d", &tr("ui.design.distance_mm"), act);
                }
                if mode == "angle" {
                    dir_slot(ui, pv, f, "axis", &tr("ui.design.turn_about"), None, act);
                    num_row(ui, pv, &id, "angle", &tr("ui.design.angle_deg"), act);
                }
                if mode == "3pt" {
                    for i in 0..3 {
                        words_slot(ui, pv, f, "points", Some(i), "point", &trv("ui.design.point_n", &[("n", (i + 1).to_string())]), act);
                    }
                }
                if mode == "tangent" {
                    words_slot(ui, pv, f, "face", None, "face", &tr("ui.design.cylinder"), act);
                    words_slot(ui, pv, f, "points", Some(0), "point", &tr("ui.design.near_point"), act);
                }
                if mode == "normal" {
                    words_slot(ui, pv, f, "edge", None, "edge", &tr("ui.design.edge"), act);
                    words_slot(ui, pv, f, "points", Some(0), "point", &tr("ui.design.at_point"), act);
                }
                if mode == "edgept" {
                    words_slot(ui, pv, f, "edge", None, "edge", &tr("ui.design.straight_edge"), act);
                    dir_slot(ui, pv, f, "axis", &tr("ui.design.or_axis"), Some(tr("ui.design.none_use_edge")), act);
                    words_slot(ui, pv, f, "points", Some(0), "point", &tr("ui.design.through_point"), act);
                }
                if mode == "midnormal" {
                    words_slot(ui, pv, f, "edge", None, "edge", &tr("ui.design.edge"), act);
                    note_row(ui, t, tr("ui.design.between_points"));
                    for i in 0..2 {
                        words_slot(ui, pv, f, "points", Some(i), "point", &trv("ui.design.point_n", &[("n", (i + 1).to_string())]), act);
                    }
                }
                if let Some(v) = check_row(ui, jv::truthy(f.get("flip")), &tr("ui.design.flip_normal")) {
                    set_cmd(act, &id, "flip", v.to_string());
                }
            });
            if let Some(cc_part::refgeom::Ref::Plane { frame, center }) = g {
                facts(ui, t, &[(tr("ui.design.normal"), fmt_v(frame.n, 4)), (tr("ui.design.through"), fmt_v(center, 3))]);
            }
            if ui
                .add_enabled(
                    ok,
                    egui::Button::new(RichText::new(tr("ui.design.sketch_on_it")).strong().family(crate::theme::bold_family())),
                )
                .clicked()
            {
                *act = cmd("part.sketch.on", id.clone());
            }
            note(ui, t, "ui.design.plane_note");
            true
        }
        "axis" => {
            egui::Grid::new(("pc-dz", id.clone())).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                mode_combo(ui, f, &AXIS_MODES, "ui.design.axis_mode.", act);
                match mode.as_str() {
                    "2pt" => {
                        for i in 0..2 {
                            words_slot(
                                ui,
                                pv,
                                f,
                                "points",
                                Some(i),
                                "point",
                                &trv("ui.design.point_n", &[("n", (i + 1).to_string())]),
                                act,
                            );
                        }
                    }
                    "cyl" => words_slot(ui, pv, f, "face", None, "face", &tr("ui.design.face"), act),
                    "planes" => {
                        plane_slot(ui, pv, f, "planes", Some(0), &tr("ui.design.first_plane"), act);
                        plane_slot(ui, pv, f, "planes", Some(1), &tr("ui.design.second_plane"), act);
                    }
                    "edge" => words_slot(ui, pv, f, "edge", None, "edge", &tr("ui.design.edge"), act),
                    "normal" => {
                        plane_slot(ui, pv, f, "planes", Some(0), &tr("ui.design.face"), act);
                        words_slot(ui, pv, f, "points", Some(0), "point", &tr("ui.design.through_point"), act);
                    }
                    _ => {}
                }
            });
            if let Some(cc_part::refgeom::Ref::Axis { o, d }) = g {
                facts(ui, t, &[(tr("ui.design.direction"), fmt_v(d, 4)), (tr("ui.design.through"), fmt_v(o, 3))]);
            }
            note(ui, t, "ui.design.axis_note");
            true
        }
        "point" => {
            egui::Grid::new(("pc-dz", id.clone())).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                mode_combo(ui, f, &POINT_MODES, "ui.design.point_mode.", act);
                match mode.as_str() {
                    "xyz" => {
                        for (i, a) in ["X", "Y", "Z"].iter().enumerate() {
                            let v = f.get("p").and_then(|p| p.get(i)).and_then(Value::as_f64).unwrap_or(f64::NAN);
                            ui.label(trv("ui.design.coord_mm", &[("axis", (*a).to_string())]));
                            let key = format!("{id}.p{i}");
                            if let Some(text) = crate::partpanel::field(ui, &mut pv.edits, &key, &V::fmt(v, 6), 90.0, "") {
                                set_cmd(act, &id, &format!("p:{i}"), text);
                            }
                            ui.end_row();
                        }
                    }
                    "at" => words_slot(ui, pv, f, "points", Some(0), "point", &tr("ui.design.at"), act),
                    "center" => words_slot(ui, pv, f, "edge", None, "edge", &tr("ui.design.circle_edge"), act),
                    "mid" => {
                        words_slot(ui, pv, f, "edge", None, "edge", &tr("ui.design.edge"), act);
                        note_row(ui, t, tr("ui.design.between_points"));
                        for i in 0..2 {
                            words_slot(
                                ui,
                                pv,
                                f,
                                "points",
                                Some(i),
                                "point",
                                &trv("ui.design.point_n", &[("n", (i + 1).to_string())]),
                                act,
                            );
                        }
                    }
                    "meet" => {
                        words_slot(ui, pv, f, "edge", None, "edge", &tr("ui.design.edge"), act);
                        dir_slot(ui, pv, f, "axis", &tr("ui.design.or_axis"), Some(tr("ui.design.none_use_edge")), act);
                        plane_slot(ui, pv, f, "planes", Some(0), &tr("ui.design.meets_plane"), act);
                        words_slot(ui, pv, f, "face", None, "face", &tr("ui.design.or_face"), act);
                        if let Some(v) = check_row(ui, jv::truthy(f.get("flip")), &tr("ui.design.other_crossing")) {
                            set_cmd(act, &id, "flip", v.to_string());
                        }
                    }
                    "planes3" => {
                        for (i, k) in ["ui.design.first_plane", "ui.design.second_plane", "ui.design.third_plane"].iter().enumerate() {
                            plane_slot(ui, pv, f, "planes", Some(i), &tr(k), act);
                        }
                    }
                    _ => {}
                }
            });
            if let Some(cc_part::refgeom::Ref::Point { p }) = g {
                facts(ui, t, &[(tr("ui.design.at"), fmt_v(p, 4))]);
            }
            true
        }
        "draft" => {
            egui::Grid::new(("pc-dz", id.clone())).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                num_row(ui, pv, &id, "angle", &tr("ui.design.angle_deg"), act);
                plane_slot(ui, pv, f, "neutral", None, &tr("ui.design.neutral_plane"), act);
                dir_slot(ui, pv, f, "pull", &tr("ui.design.pull_direction"), Some(tr("ui.design.neutral_normal")), act);
            });
            crate::partpanel::ref_list(ui, pv, f, "faces", t, act);
            note(ui, t, "ui.design.draft.note");
            true
        }
        "thread" => {
            let form = jv::s(f, "form").to_string();
            egui::Grid::new(("pc-dz", id.clone())).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                let forms: Vec<(String, String)> =
                    ["iso", "tr"].iter().map(|k| ((*k).to_string(), tr(&format!("thread.ui.form.{k}")))).collect();
                if let Some(v) = combo(ui, format!("pc-tf-{id}"), &tr("thread.ui.form"), &form, &forms) {
                    set_cmd(act, &id, "form", v);
                }
                if let Some(v) = combo(ui, format!("pc-ts-{id}"), &tr("thread.ui.size"), jv::s(f, "size"), &thread_sizes(&form)) {
                    set_cmd(act, &id, "size", v);
                }
                let full = f.get("length").is_some_and(Value::is_null);
                if let Some(v) = check_row(ui, full, &tr("thread.ui.full")) {
                    set_cmd(act, &id, "full", v.to_string());
                }
                if !full {
                    num_row(ui, pv, &id, "length", &tr("thread.ui.length"), act);
                }
                if let Some(v) = check_row(ui, jv::truthy(f.get("flip")), &tr("thread.ui.flip")) {
                    set_cmd(act, &id, "flip", v.to_string());
                }
                let hands: Vec<(String, String)> =
                    ["right", "left"].iter().map(|k| ((*k).to_string(), tr(&format!("thread.ui.hand.{k}")))).collect();
                if let Some(v) = combo(ui, format!("pc-th-{id}"), &tr("thread.ui.hand"), jv::s(f, "hand"), &hands) {
                    set_cmd(act, &id, "hand", v);
                }
                modelled_rows(ui, pv, f, t, act);
                let callouts = pv.regen.threads.get(&id).map(|th| jv::arr(th.get("callouts")).to_vec()).unwrap_or_default();
                for c in callouts {
                    let n = jv::n(&c, "count");
                    ui.label("");
                    ui.label(
                        RichText::new(format!(
                            "{}{}",
                            if n > 1.0 { format!("{}× ", cc_part::js::num_str(n)) } else { String::new() },
                            jv::s(&c, "callout")
                        ))
                        .small(),
                    );
                    ui.end_row();
                }
                note_row(ui, t, thread_source(&form).to_string());
            });
            crate::partpanel::ref_list(ui, pv, f, "faces", t, act);
            true
        }
        _ => false,
    }
}

/// thread-ui.js `modelledRows`.
fn modelled_rows(ui: &mut egui::Ui, pv: &mut PartView, f: &Value, t: &Tokens, act: &mut Cmd) {
    let id = jv::s(f, "id").to_string();
    let modelled = jv::truthy(f.get("modelled"));
    if let Some(v) = check_row(ui, modelled, &tr("thread.ui.modelled")) {
        set_cmd(act, &id, "modelled", v.to_string());
    }
    if modelled {
        num_row(ui, pv, &id, "clearance", &tr("thread.ui.clearance"), act);
    }
    note_row(ui, t, tr(if modelled { "thread.ui.modelled_note" } else { "thread.ui.cosmetic_note" }));
}

/// design-ui.js `extra(f)` (extrude, hole, chamfer, fillet), thread-ui.js `holeExtra` and bodies-ui.js `extra`, in a
/// grid of their own under the feature's rows.
pub fn design_extra(ui: &mut egui::Ui, pv: &mut PartView, f: &Value, t: &Tokens, act: &mut Cmd) {
    let id = jv::s(f, "id").to_string();
    let ty = jv::s(f, "type").to_string();
    if !matches!(ty.as_str(), "extrude" | "hole" | "chamfer" | "fillet" | "lpattern" | "cpattern") {
        return;
    }
    egui::Grid::new(("pc-dx", id.clone())).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| match ty.as_str() {
        "extrude" => {
            let end = jv::s(f, "end").to_string();
            if end == "upto" || end == "offset" {
                plane_slot(ui, pv, f, "upto", None, &tr("ui.design.up_to"), act);
            }
            if end == "offset" {
                num_row(ui, pv, &id, "offset", &tr("ui.design.offset_mm"), act);
            }
            if end == "vertex" {
                words_slot(ui, pv, f, "vertex", None, "point", &tr("ui.design.up_to_vertex"), act);
            }
            if end == "next" {
                note_row(ui, t, tr("ui.design.next_note"));
            }
            let two = jv::truthy(f.get("two"));
            if end == "blind" {
                if let Some(v) = check_row(ui, two, &tr("ui.design.second_direction")) {
                    set_cmd(act, &id, "two", v.to_string());
                }
                if two {
                    num_row(ui, pv, &id, "depth2", &tr("ui.design.second_depth_mm"), act);
                }
            }
            if end != "mid" && !two {
                num_row(ui, pv, &id, "taper", &tr("ui.design.taper_deg"), act);
            }
            let n = jv::arr(f.get("regions")).len();
            ui.label(tr("ui.design.regions"));
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(if n > 0 {
                        trv("ui.design.regions_picked", &[("n", n.to_string())])
                    } else {
                        tr("ui.design.every_loop")
                    })
                    .small(),
                );
                pick_btn(ui, pv, &id, "regions", "region", None, Some(tr("ui.design.pick_regions")), act);
                if n > 0 && ui.small_button(tr("ui.design.all")).clicked() {
                    set_cmd(act, &id, "regions", "all");
                }
            });
            ui.end_row();
        }
        "hole" => {
            let kind = jv::s(f, "kind").to_string();
            if kind == "csink" {
                num_row(ui, pv, &id, "csinkD", &tr("ui.design.csink_d"), act);
                let opts = vec![("90".to_string(), tr("ui.design.csink_90")), ("82".to_string(), tr("ui.design.csink_82"))];
                let cur = f.get("csinkAngle").and_then(Value::as_f64).map_or_else(|| "90".into(), cc_part::js::num_str);
                if let Some(v) = combo(ui, format!("pc-ca-{id}"), &tr("ui.design.csink_angle"), &cur, &opts) {
                    set_cmd(act, &id, "csinkAngle", v);
                }
            }
            if kind == "tapped" {
                let size = f.get("size").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or("M6").to_string();
                if let Some(v) = combo(ui, format!("pc-hs-{id}"), &tr("ui.design.thread"), &size, &hole_sizes()) {
                    set_cmd(act, &id, "size", v);
                }
                if !jv::truthy(f.get("through")) {
                    num_row(ui, pv, &id, "threadDepth", &tr("ui.design.thread_depth_mm"), act);
                }
                let mut g2 = f.clone();
                jv::set(&mut g2, "size", size.into());
                let callout = RD::hole_callout(&g2, jv::as_num(f.get("depth")));
                note_row(ui, t, trv("ui.design.tapped_note", &[("callout", callout), ("source", HOLE_THREAD_SOURCE.to_string())]));
                modelled_rows(ui, pv, f, t, act);
            }
        }
        "chamfer" => {
            let opts: Vec<(String, String)> =
                ["equal", "two", "angle"].iter().map(|k| ((*k).to_string(), tr(&format!("ui.design.chamfer.{k}")))).collect();
            let mode = f.get("mode").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or("equal").to_string();
            if let Some(v) = combo(ui, format!("pc-cm-{id}"), &tr("ui.design.type"), &mode, &opts) {
                set_cmd(act, &id, "mode", v);
            }
            if mode == "two" {
                num_row(ui, pv, &id, "d2", &tr("ui.design.second_distance"), act);
            }
            if mode == "angle" {
                num_row(ui, pv, &id, "angle", &tr("ui.design.angle_deg"), act);
            }
            if mode != "equal"
                && let Some(v) = check_row(ui, jv::truthy(f.get("flip")), &tr("ui.design.chamfer.flip"))
            {
                set_cmd(act, &id, "flip", v.to_string());
            }
        }
        "fillet" => {
            let var = f.get("r2").is_some_and(Value::is_number);
            if let Some(v) = check_row(ui, var, &tr("ui.design.fillet.variable")) {
                set_cmd(act, &id, "variable", v.to_string());
            }
            if var {
                num_row(ui, pv, &id, "r2", &tr("ui.design.end_radius_mm"), act);
                if let Some(v) = check_row(ui, jv::truthy(f.get("flip")), &tr("ui.design.fillet.swap")) {
                    set_cmd(act, &id, "flip", v.to_string());
                }
            }
        }
        _ => {
            let on = !jv::arr(f.get("bodies")).is_empty();
            if let Some(v) = check_row(ui, on, &tr("ui.bodies.copy.check")) {
                set_cmd(act, &id, "copybodies", v.to_string());
            }
        }
    });
}

/// The reference geometry over the view (view3d.js `setRefs`): plane outlines, axis lines, point marks.
pub fn draw_refs(ui: &egui::Ui, rect: egui::Rect, pv: &PartView, t: &Tokens, ppp: f64) {
    if pv.mode == Mode::Sketch {
        return;
    }
    let painter = ui.painter_at(rect);
    let to = |p: [f64; 3]| pv.screen_of(p).map(|q| egui::pos2(rect.min.x + (q.0 / ppp) as f32, rect.min.y + (q.1 / ppp) as f32));
    // cosmetic threads: the major diameter, dashed, at the top and the thread's end
    for (_, th) in pv.regen.threads.iter() {
        if jv::truthy(th.get("modelled")) {
            continue;
        }
        for x in jv::arr(th.get("list")) {
            let (o, d, r, depth) =
                (p3(x.get("o").unwrap_or(&Value::Null)), p3(x.get("d").unwrap_or(&Value::Null)), jv::n(x, "r"), jv::n(x, "depth"));
            let fr = V::frame_for(d, o);
            for z in [0.0, depth] {
                let c = V::add(o, V::mul(d, z));
                let pts: Vec<egui::Pos2> = (0..=48)
                    .filter_map(|k| {
                        let a = f64::from(k) / 48.0 * std::f64::consts::TAU;
                        to(V::add(c, V::add(V::mul(fr.x, r * a.cos()), V::mul(fr.y, r * a.sin()))))
                    })
                    .collect();
                for w in pts.windows(2).step_by(2) {
                    if let [a, b] = w {
                        painter.line_segment([*a, *b], egui::Stroke::new(1.0, t.text));
                    }
                }
            }
        }
    }
    for (id, d) in pv.refs_for_view() {
        let sel = pv.sel_feature.as_deref() == Some(id.as_str());
        let c = if sel { t.accent } else { egui::Color32::from_rgb(0xd9, 0x82, 0x2b) };
        let stroke = egui::Stroke::new(if sel { 2.0 } else { 1.2 }, c);
        match d {
            RefDraw::Plane(q) => {
                let pts: Vec<egui::Pos2> = q.iter().filter_map(|p| to(*p)).collect();
                if pts.len() == 4 {
                    painter.add(egui::Shape::convex_polygon(pts.clone(), c.gamma_multiply(0.12), egui::Stroke::NONE));
                    painter.add(egui::Shape::closed_line(pts, stroke));
                }
            }
            RefDraw::Axis(a, b) => {
                if let (Some(sa), Some(sb)) = (to(a), to(b)) {
                    painter.add(egui::Shape::dashed_line(&[sa, sb], stroke, 6.0, 4.0));
                }
            }
            RefDraw::Point(p) => {
                if let Some(s) = to(p) {
                    painter.line_segment([s - egui::vec2(5.0, 0.0), s + egui::vec2(5.0, 0.0)], stroke);
                    painter.line_segment([s - egui::vec2(0.0, 5.0), s + egui::vec2(0.0, 5.0)], stroke);
                }
            }
        }
    }
}

/// "x,y,z" → a finite point.
pub fn parse_p3(s: &str) -> Option<[f64; 3]> {
    let v: Vec<f64> = s.split(',').map(|x| x.trim().parse::<f64>().unwrap_or(f64::NAN)).collect();
    match v.as_slice() {
        [a, b, c] if a.is_finite() && b.is_finite() && c.is_finite() => Some([*a, *b, *c]),
        _ => None,
    }
}

impl PartView {
    /// The k-th drawn segment of edge `e` (meshfaces.js `edgeSegs` / `segEdge`), part frame.
    pub fn edge_segment(&self, e: usize, k: usize) -> Option<([f64; 3], [f64; 3])> {
        let mesh = self.regen.mesh.as_ref()?;
        let segs = jv::arr(mesh.get("edgeSegs"));
        let own = jv::arr(mesh.get("segEdge"));
        let j = own.iter().enumerate().filter(|(_, x)| x.as_u64().and_then(|n| usize::try_from(n).ok()) == Some(e)).nth(k)?.0;
        let g = |i: usize| {
            j.checked_mul(6).and_then(|b| b.checked_add(i)).and_then(|q| segs.get(q)).and_then(Value::as_f64).unwrap_or(f64::NAN)
        };
        Some(([g(0), g(1), g(2)], [g(3), g(4), g(5)]))
    }

    /// The edge under pixel (x, y) (physical) within `tol` and which of its segments: (edge, k).
    pub fn edge_seg_at(&self, x: f64, y: f64, tol: f64) -> Option<(usize, usize)> {
        let mesh = self.regen.mesh.as_ref()?;
        let segs: Vec<f64> = jv::arr(mesh.get("edgeSegs")).iter().map(|v| v.as_f64().unwrap_or(f64::NAN)).collect();
        let own = jv::arr(mesh.get("segEdge"));
        let mut best: Option<(f64, usize, usize)> = None;
        let mut count: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
        for (j, s) in segs.as_chunks::<6>().0.iter().enumerate() {
            let Some(e) = own.get(j).and_then(Value::as_u64).and_then(|e| usize::try_from(e).ok()) else { continue };
            let k = *count.entry(e).and_modify(|c| *c += 1).or_insert(0);
            let (Some(a), Some(b)) = (self.screen_of([s[0], s[1], s[2]]), self.screen_of([s[3], s[4], s[5]])) else { continue };
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let l2 = dx * dx + dy * dy;
            let t = if l2 > 0.0 { (((x - a.0) * dx + (y - a.1) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
            let d = (x - a.0 - t * dx).hypot(y - a.1 - t * dy);
            if d <= tol && best.is_none_or(|(bd, _, _)| d < bd) {
                best = Some((d, e, k));
            }
        }
        best.map(|(_, e, k)| (e, k))
    }
}

#[cfg(test)]
mod tests {
    use crate::{Params, UiApp};
    use cc_part::{jv, part as P};

    fn run(app: &mut UiApp, id: &str, name: &str) {
        app.run(id, &Params { name: Some(name.into()), ..Default::default() }).unwrap_or_else(|e| panic!("{id}={name}: {e}"));
    }

    #[test]
    fn more_menu_reference_geometry_slots_and_threads() {
        let mut app = UiApp::default();
        run(&mut app, "part.sample", "");
        let n0 = P::features(&app.part.as_ref().unwrap().part).len();
        for c in ["sweep", "patch", "combine", "rib", "sheetbase"] {
            run(&mut app, "part.feature", c);
        }
        assert_eq!(P::features(&app.part.as_ref().unwrap().part).len(), n0, "the exact kernel's tools add nothing");
        run(&mut app, "part.face", "f2.end");
        run(&mut app, "part.feature", "plane");
        let pv = app.part.as_ref().unwrap();
        let f = P::features(&pv.part).last().unwrap().clone();
        assert_eq!(jv::s(&f, "type"), "plane");
        assert_eq!(jv::s(&f, "mode"), "offset");
        assert!(pv.regen.ref_geom.get(jv::s(&f, "id")).is_some());
        run(&mut app, "part.set", "Plane1:d=-20");
        run(&mut app, "part.set", "Plane1:d=abc");
        assert!(app.part.as_ref().unwrap().note.contains("abc"));
        // a slot takes the next click
        run(&mut app, "part.set", "Plane1:mode=mid");
        run(&mut app, "part.slot", "Plane1:planes:1:plane");
        assert!(app.part.as_ref().unwrap().slot.is_some());
        run(&mut app, "part.face", "f2.side:seat");
        let pv = app.part.as_ref().unwrap();
        assert!(pv.slot.is_none());
        let f = P::feature_by_id(&pv.part, "f10").unwrap();
        assert_eq!(jv::s(&f["planes"][1], "kind"), "face");
        // a sketch on the plane
        run(&mut app, "part.set", "Plane1:mode=offset");
        run(&mut app, "part.sketch.on", "Plane1");
        assert_eq!(app.part.as_ref().unwrap().mode, crate::partview::Mode::Sketch);
        run(&mut app, "part.sketch.exit", "");
        // a thread on a bolt hole, its faces dropped by the Remove button's command
        run(&mut app, "part.face", "f6.wall:h1");
        run(&mut app, "part.feature", "thread");
        run(&mut app, "part.face", "f6.wall:h1");
        run(&mut app, "part.pick.done", "");
        run(&mut app, "part.set", "Thread1:form=tr");
        let pv = app.part.as_ref().unwrap();
        let th = P::features(&pv.part).iter().find(|f| jv::s(f, "name") == "Thread1").unwrap().clone();
        assert_eq!(jv::s(&th, "size"), "auto");
        assert_eq!(jv::arr(th.get("faces")).len(), 1);
        run(&mut app, "part.set", "Thread1:drop:0");
        let pv = app.part.as_ref().unwrap();
        assert!(jv::arr(P::features(&pv.part).iter().find(|f| jv::s(f, "name") == "Thread1").unwrap().get("faces")).is_empty());
        // hostile specs are errors, not panics
        for bad in ["Plane1:planes:99999999999999999999=base:top", "Plane1:mode=zz", "Thread1:size=M999"] {
            assert!(app.run("part.set", &Params { name: Some(bad.into()), ..Default::default() }).is_err(), "{bad}");
        }
        for bad in ["x", "Plane1:planes:a:plane", "Plane1:planes:0:nope", "nofeature:k:plane"] {
            assert!(app.run("part.slot", &Params { name: Some(bad.into()), ..Default::default() }).is_err(), "{bad}");
        }
        assert!(app.run("part.region", &Params { name: Some("1,2".into()), ..Default::default() }).is_err());
    }
}
