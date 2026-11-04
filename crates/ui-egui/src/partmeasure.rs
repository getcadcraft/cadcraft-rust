//! PartCraft's Measure tool, Mass properties and the live section view in the app (M13.5): measure-ui.js and
//! section-ui.js over cc-part's measure.rs / analysis.rs (measure.js / section.js at parity). Measure (View › Views ›
//! Measure, key M): a click snaps to a vertex, an edge midpoint or a circle centre within 10 px, else an edge within
//! 6 px, else the face under the pointer; Shift+click takes the whole body; two picks give the distance, ΔXYZ, the
//! angle and the gaps; a third starts again; Esc clears, Esc again closes. Mass properties: volume, area, centre of
//! mass and inertia over the mesh (the part's material, or one chosen in the card, gives the mass). Section view (key
//! X): the view cut by the front / top / right plane or a picked flat face, moved along its normal, flipped, the cut
//! filled; display only. Everything is a `part.measure*` / `part.section` command (partview.rs `run`).

use cc_part::{analysis as AN, bom, i18n, jv, measure as ME};
use serde_json::Value;

use crate::partview::{Mode, PartView, tr, trv};

const IDENT4: [f64; 16] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

/// One pick of the Measure tool (measure-ui.js `resolve` → `mkPick`).
#[derive(Clone, Debug)]
pub struct MPick {
    pub kind: String,
    /// a face's or edge's index in the rebuild's records
    pub index: Option<usize>,
    /// the picked point (the part's frame)
    pub local: [f64; 3],
    /// the snap candidate's edge
    pub edge: Option<f64>,
    pub at: Option<Value>,
    /// a body pick's mass properties
    pub mass: Option<ME::Mass>,
}

impl MPick {
    fn json(&self) -> Value {
        let mut o = jv::obj(vec![("kind", self.kind.clone().into()), ("matrix", jv::nums(&IDENT4)), ("local", jv::pt3(self.local))]);
        if let Some(e) = self.edge {
            jv::set(&mut o, "edge", jv::num(e));
        }
        if let Some(i) = self.index {
            jv::set(&mut o, "index", jv::unum(i));
        }
        if let Some(a) = &self.at {
            jv::set(&mut o, "at", a.clone());
        }
        o
    }
    fn is_point(&self) -> bool {
        ME::is_point_kind(&self.kind)
    }
}

/// Mass properties of the part (measure-ui.js `st.mass`).
#[derive(Clone, Debug)]
pub struct MassOf {
    pub props: ME::Mass,
    pub missing: Vec<String>,
    pub count: usize,
}

/// measure-ui.js's state.
#[derive(Default)]
pub struct Measure {
    pub active: bool,
    pub picks: Vec<MPick>,
    pub pair: Option<Value>,
    pub mass: Option<MassOf>,
    /// the density chosen in the card (g/cm³) when the part has no material
    pub density: Option<f64>,
    /// the snap under the pointer
    pub hover: Option<MPick>,
    /// text waiting to go to the clipboard (the window's frame copies it)
    pub clip_out: Option<String>,
    /// the measuring view of the last rebuild and its snap candidates
    build: Option<ME::BuildM>,
    cands: Option<Vec<ME::Cand>>,
    /// physical pixels per point of the view (the tolerances are in points, as the JS's are in CSS pixels)
    pub ppp: f64,
}

/// section-ui.js's state: the cut removes n·p > c (the part's Y-up frame).
#[derive(Clone, Debug)]
pub struct Section {
    pub active: bool,
    pub picking: bool,
    /// front / top / right / face
    pub base: String,
    pub n: [f64; 3],
    pub c: f64,
    pub face: Option<([f64; 3], f64)>,
    pub flip: bool,
    pub caps: bool,
    /// the position field while it is being typed
    pub typing: Option<String>,
}

impl Default for Section {
    fn default() -> Self {
        Section {
            active: false,
            picking: false,
            base: "front".into(),
            n: [0.0, 0.0, 1.0],
            c: 0.0,
            face: None,
            flip: false,
            caps: true,
            typing: None,
        }
    }
}

/// `String(Number(v.toFixed(dp)))` with −0 and tiny values as 0 (section-ui.js `fmt`).
pub fn fmt3(v: f64) -> String {
    let v = if v.abs() < 1e-9 { 0.0 } else { v };
    cc_part::js::num_str(cc_cad::js::str_to_number(&cc_part::js::to_fixed(v, 3)))
}

fn face_type_word(ty: &str) -> &'static str {
    match ty {
        "plane" => "flat",
        "cylinder" => "cylinder",
        "cone" => "cone",
        "sphere" => "sphere",
        "torus" => "torus",
        "spline" => "freeform",
        _ => "curved",
    }
}

impl PartView {
    /// measure-ui.js `ctx.targets()`: the part when it has a solid.
    fn measure_target(&self) -> bool {
        self.regen.mesh.is_some()
    }

    fn part_density(&self) -> Option<f64> {
        self.part.get("material").and_then(|m| jv::as_num(m.get("density")))
    }

    /// The measuring view of the rebuild (measure.js's `build`), made once per rebuild from the kernel's result.
    fn measure_build(&mut self) -> Option<&ME::BuildM> {
        if self.measure.build.is_none()
            && let (Some(mesh), Some(raw)) = (self.regen.mesh.as_ref(), self.regen.raw_mesh.as_ref())
        {
            let nums = |k: &str| -> Option<Vec<f64>> {
                mesh.get(k).and_then(Value::as_array).map(|a| a.iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect())
            };
            self.measure.build = Some(ME::BuildM {
                exact: false,
                named: self.regen.faces.is_some(),
                edges: jv::arr(self.regen.edges.as_ref()).to_vec(),
                faces: jv::arr(self.regen.faces.as_ref()).to_vec(),
                positions: raw.positions.clone(),
                indices: raw.indices.clone(),
                tri_face: raw.tri_face.as_ref().map(|tf| tf.iter().map(|&f| f64::from(f)).collect()),
                edge_segs: nums("edgeSegs"),
                seg_edge: nums("segEdge"),
                has_mesh: true,
                stats: self.regen.stats.clone(),
            });
        }
        self.measure.build.as_ref()
    }

    /// measure-ui.js `candsOf(build)`: the snap points, worked out on the first snap.
    fn measure_cands(&mut self) -> Option<&[ME::Cand]> {
        if self.measure.cands.is_none() {
            let c = ME::snap_candidates(self.measure_build()?);
            self.measure.cands = Some(c);
        }
        self.measure.cands.as_deref()
    }

    /// The rebuild changed: the picks were made on the old solid (measure-ui.js `refresh` keeps only picks on the
    /// current build), the section's slider gets the new box.
    pub fn measure_refresh(&mut self) {
        self.measure.build = None;
        self.measure.cands = None;
        if self.measure.active {
            self.measure.picks.clear();
            self.measure.pair = None;
            self.measure.hover = None;
        }
    }

    /// measure-ui.js `start()`.
    pub fn measure_start(&mut self) {
        if self.mode == Mode::Sketch || self.drawing.is_some() {
            self.note = tr("app.measure.model_only");
            return;
        }
        self.measure.active = true;
        self.measure_clear();
        self.note = tr("ui.measure.start");
    }

    /// measure-ui.js `stop()`.
    pub fn measure_stop(&mut self) {
        let m = &mut self.measure;
        m.active = false;
        m.picks.clear();
        m.pair = None;
        m.mass = None;
        m.hover = None;
    }

    pub fn measure_clear(&mut self) {
        let m = &mut self.measure;
        m.picks.clear();
        m.pair = None;
        m.mass = None;
    }

    /// measure-ui.js `resolve(h, x, y, shift)`: the pick under pixel (x, y) of the view (physical pixels).
    pub fn measure_resolve(&mut self, x: f64, y: f64, shift: bool) -> Option<MPick> {
        if !self.measure_target() {
            return None;
        }
        let hit = self.face_hit(x, y).map(|(f, p, _)| (f, p));
        let ppp = if self.measure.ppp > 0.0 { self.measure.ppp } else { 1.0 };
        if shift && let Some((_, p)) = hit {
            return Some(MPick { kind: "body".into(), index: None, local: p, edge: None, at: None, mass: None });
        }
        let named = self.regen.faces.is_some();
        self.measure_cands()?;
        let proj = |p: [f64; 3]| self.screen_of(p).map(|(a, b)| [a / ppp, b / ppp]);
        let (xs, ys) = (x / ppp, y / ppp);
        let mut snap: Option<(ME::Cand, f64)> = None;
        let mut edge: Option<(usize, f64)> = None;
        {
            let (b, cands) = (self.measure.build.as_ref()?, self.measure.cands.as_deref()?);
            if let Some(c) = ME::snap_at(cands, &proj, xs, ys, 10.0)
                && let Some(s) = proj(c.p)
            {
                snap = Some((c.clone(), (s[0] - xs).hypot(s[1] - ys)));
            }
            if named && b.edge_segs.is_some() {
                let (i, d) = ME::nearest_edge_at(b, &proj, xs, ys, 6.0);
                if i >= 0.0 {
                    edge = cc_part::js::idx(i).map(|i| (i, d));
                }
            }
        }
        if let Some((c, d)) = &snap
            && edge.is_none_or(|(_, ed)| *d <= ed + 1.0)
        {
            return Some(MPick { kind: c.kind.into(), index: None, local: c.p, edge: Some(c.edge), at: None, mass: None });
        }
        if let Some((i, _)) = edge {
            let at = self.edges().get(i).and_then(|e| e.get("at")).cloned();
            let local = at.as_ref().map_or([f64::NAN; 3], crate::partview::p3);
            return Some(MPick { kind: "edge".into(), index: Some(i), local, edge: None, at, mass: None });
        }
        let (fi, p) = hit?;
        let at = fi.and_then(|i| self.face(i)).and_then(|f| f.get("at")).cloned();
        // the mesh kernel names its faces: the face record is the pick
        Some(MPick { kind: "face".into(), index: fi, local: p, edge: None, at, mass: None })
    }

    /// A pick made by name (the parity harness and agents): `snap:i` (the i-th snap candidate), `edge:i`,
    /// `face:NAME|x,y,z` (the point on it; face names hold `@` and `~`), `body|x,y,z`.
    pub fn measure_named(&mut self, spec: &str) -> Result<MPick, String> {
        let (what, at_pt) = match spec.split_once('|') {
            Some((w, a)) => (w, Some(a)),
            None => (spec, None),
        };
        let point = |s: Option<&str>| -> Result<[f64; 3], String> {
            let v: Vec<f64> = s.unwrap_or("").split(',').map(|x| x.trim().parse::<f64>().unwrap_or(f64::NAN)).collect();
            match v.as_slice() {
                [a, b, c] if v.iter().all(|x| x.is_finite()) => Ok([*a, *b, *c]),
                _ => Err(format!("part.measure.at: a point x,y,z after |, not {}", s.unwrap_or(""))),
            }
        };
        let (kind, arg) = what.split_once(':').unwrap_or((what, ""));
        match kind {
            "snap" => {
                let i: usize = arg.parse().map_err(|_| format!("part.measure.at: snap:<n>, not {spec}"))?;
                let cands = self.measure_cands().ok_or_else(|| tr("ui.measure.no_solid"))?;
                let c = cands.get(i).ok_or_else(|| format!("part.measure.at: no snap point {i}"))?;
                Ok(MPick { kind: c.kind.into(), index: None, local: c.p, edge: Some(c.edge), at: None, mass: None })
            }
            "edge" => {
                let i: usize = arg.trim_start_matches('#').parse().map_err(|_| format!("part.measure.at: edge:<n>, not {spec}"))?;
                let e = self.edges().get(i).ok_or_else(|| format!("part.measure.at: no edge {i}"))?;
                let at = e.get("at").cloned();
                let local = at.as_ref().map_or([f64::NAN; 3], crate::partview::p3);
                Ok(MPick { kind: "edge".into(), index: Some(i), local, edge: None, at, mass: None })
            }
            "face" => {
                let i = self
                    .faces()
                    .iter()
                    .position(|f| jv::is_str(f.get("name"), arg))
                    .ok_or_else(|| format!("part.measure.at: no face {arg}"))?;
                let at = self.face(i).and_then(|f| f.get("at")).cloned();
                let local = if at_pt.is_some() { point(at_pt)? } else { at.as_ref().map_or([f64::NAN; 3], crate::partview::p3) };
                Ok(MPick { kind: "face".into(), index: Some(i), local, edge: None, at, mass: None })
            }
            "body" => Ok(MPick { kind: "body".into(), index: None, local: point(at_pt)?, edge: None, at: None, mass: None }),
            _ => Err(format!("part.measure.at: snap:<n>, edge:<n>, face:<name>|x,y,z or body|x,y,z, not {spec}")),
        }
    }

    /// measure-ui.js `pick(h, ev)`.
    pub fn measure_pick(&mut self, pk: Option<MPick>) {
        let Some(pk) = pk else {
            self.note = tr("ui.measure.nothing");
            return;
        };
        self.measure.mass = None;
        if self.measure.picks.len() >= 2 {
            self.measure.picks.clear();
        }
        self.measure.picks.push(pk);
        self.measure_compute();
    }

    fn mass_props(&mut self) -> Option<ME::Mass> {
        let b = self.measure_build()?;
        Some(ME::mesh_mass(&b.positions, &b.indices))
    }

    /// measure-ui.js `placed(props, tg, density)`.
    fn placed(props: &ME::Mass, density: Option<f64>) -> ME::Mass {
        match density {
            Some(d) => ME::place_mass(props, Some(&IDENT4), d),
            None => {
                let mut p = ME::place_mass(props, Some(&IDENT4), 1000.0);
                p.mass = None;
                p
            }
        }
    }

    /// measure-ui.js `compute()`.
    fn measure_compute(&mut self) {
        self.measure.pair = None;
        let two = match self.measure.picks.as_slice() {
            [pa, pb] => Some((pa.json(), pb.json(), pa.kind.clone(), pb.kind.clone(), pa.is_point() && pb.is_point())),
            _ => None,
        };
        if let Some((a, b, ak, bk, both_points)) = two
            && let Some(bm) = self.measure_build()
            && let (Ok(ia), Ok(ib)) = (ME::info(bm, &a), ME::info(bm, &b))
        {
            let pair = if both_points {
                ME::pair(&ak, &bk, &ia, &ib, None, None)
            } else {
                let (ga, gb) = (ME::geometry_of(bm, &a), ME::geometry_of(bm, &b));
                ME::pair(&ak, &bk, &ia, &ib, None, Some((&ga, &gb)))
            };
            self.measure.pair = Some(pair);
        }
        let d = self.part_density().or(self.measure.density);
        let props = if self.measure.picks.iter().any(|p| p.kind == "body") { self.mass_props() } else { None };
        if let Some(props) = props {
            for pk in self.measure.picks.iter_mut().filter(|p| p.kind == "body") {
                pk.mass = Some(Self::placed(&props, d));
            }
        }
    }

    /// measure-ui.js `mass()`: mass properties of the part.
    pub fn measure_mass(&mut self) {
        if self.mode == Mode::Sketch || self.drawing.is_some() {
            self.note = tr("app.measure.mass_model_only");
            return;
        }
        if !self.measure.active {
            self.measure_start();
        }
        if !self.measure.active {
            return;
        }
        if !self.measure_target() {
            self.note = tr("ui.measure.no_solid");
            return;
        }
        self.measure.picks.clear();
        self.measure.pair = None;
        let Some(props) = self.mass_props() else { return };
        let d = self.part_density().or(self.measure.density);
        let placed = Self::placed(&props, d);
        let (all, missing) = match d {
            Some(_) => (ME::combine_mass(&[placed]), Vec::new()),
            None => {
                let mut p = placed;
                p.mass = Some(p.volume);
                let mut g = ME::combine_mass(&[p]);
                g.mass = None;
                let name = jv::s(&self.part, "name").to_string();
                (g, vec![name])
            }
        };
        self.measure.mass = Some(MassOf { props: all, missing, count: 1 });
    }

    /// measure-ui.js `pickLabel(pk)`.
    fn pick_label(&self, pk: &MPick) -> String {
        if pk.kind == "face" {
            let fc = pk.index.and_then(|i| self.face(i));
            return format!("Face{}", fc.map(|f| format!(" ({})", face_type_word(jv::s(f, "type")))).unwrap_or_default());
        }
        ME::kind_label(&pk.kind)
    }

    /// measure-ui.js `rows()`.
    pub fn measure_rows(&mut self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        if let Some(m) = &self.measure.mass {
            let of = if m.count > 1 {
                i18n::t("ui.measure.of_parts", &[("n", i18n::F::N(cc_part::js::f(m.count)))])
            } else {
                tr("ui.measure.of_part")
            };
            out.push((tr("ui.measure.of"), of));
            out.extend(ME::mass_rows(&m.props));
            return out;
        }
        let picks = self.measure.picks.clone();
        let many = picks.len() > 1;
        for (i, pk) in picks.iter().enumerate() {
            let k =
                if many { i18n::t("ui.measure.picked_n", &[("n", i18n::F::N(cc_part::js::f(i + 1)))]) } else { tr("ui.measure.picked") };
            out.push((k, self.pick_label(pk)));
            if let Some(bm) = self.measure_build()
                && let Ok(inf) = ME::info(bm, &pk.json())
            {
                out.extend(ME::info_rows(&inf));
            }
            if let Some(m) = &pk.mass {
                out.extend(ME::mass_rows(m));
            }
        }
        if let Some(p) = &self.measure.pair {
            out.extend(ME::pair_rows(p));
        }
        out
    }

    /// measure-ui.js `copy()`.
    pub fn measure_copy(&mut self) {
        let text = ME::as_text(&self.measure_rows());
        self.measure.clip_out = Some(text);
        self.note = tr("ui.measure.copied");
    }

    /// The material picker of the card (measure-ui.js's select: '' = none).
    pub fn measure_density(&mut self, d: Option<f64>) {
        self.measure.density = d.filter(|x| x.is_finite() && *x > 0.0);
        if self.measure.mass.is_some() {
            self.measure_mass();
        } else {
            self.measure_compute();
        }
    }

    /// Esc while measuring: clear, then close (measure-ui.js `keydown`). True when taken.
    pub fn measure_escape(&mut self) -> bool {
        if !self.measure.active {
            return false;
        }
        if !self.measure.picks.is_empty() || self.measure.mass.is_some() {
            self.measure_clear();
        } else {
            self.measure_stop();
        }
        true
    }

    // ------------------------------------------------------------------ the section view

    /// The model's box (view3d.js `box`: the solid's bounding box, else ±50).
    fn section_box(&self) -> ([f64; 3], [f64; 3]) {
        if let Some(m) = self.regen.raw_mesh.as_ref().filter(|m| !m.positions.is_empty()) {
            let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
            for p in m.positions.as_chunks::<3>().0 {
                for ((l, h), v) in lo.iter_mut().zip(hi.iter_mut()).zip(p) {
                    *l = l.min(*v);
                    *h = h.max(*v);
                }
            }
            return (lo, hi);
        }
        ([-50.0; 3], [50.0; 3])
    }

    /// `sectionRange(box, n)`.
    pub fn section_range(&self) -> [f64; 2] {
        let (lo, hi) = self.section_box();
        AN::section_range(lo, hi, self.section.n)
    }

    fn section_middle(&mut self) {
        let [lo, hi] = self.section_range();
        self.section.c = (lo + hi) / 2.0;
    }

    /// section-ui.js `start()`.
    pub fn section_start(&mut self) {
        if self.mode == Mode::Sketch || self.drawing.is_some() {
            self.note = tr("section.blocked");
            return;
        }
        self.section.active = true;
        self.section.picking = false;
        if self.section.base != "face" {
            self.section.n = AN::section_base(&self.section.base).unwrap_or([0.0, 0.0, 1.0]);
            self.section_middle();
        }
        self.apply_section();
    }

    pub fn section_stop(&mut self) {
        if !self.section.active {
            return;
        }
        self.section.active = false;
        self.section.picking = false;
        self.apply_section();
    }

    /// section-ui.js `setBase(k)`.
    pub fn section_base(&mut self, k: &str) -> Result<(), String> {
        if k == "face" {
            if let Some((n, c)) = self.section.face {
                self.section.base = "face".into();
                self.section.n = n;
                self.section.c = c;
            }
        } else {
            let n = AN::section_base(k).ok_or_else(|| tr("section.unknown_plane"))?;
            self.section.base = k.into();
            self.section.n = n;
            self.section_middle();
        }
        self.apply_section();
        Ok(())
    }

    /// section-ui.js `arm()`: Pick a face (again = cancel).
    pub fn section_arm(&mut self) {
        self.section.picking = !self.section.picking;
        self.note = if self.section.picking { tr("section.pick.hint") } else { String::new() };
    }

    /// section-ui.js `pick(hit)` with partcraft.js `facePlane`: a flat face's plane, flush with it.
    pub fn section_pick(&mut self, hit: Option<(Option<usize>, [f64; 3], [f64; 3])>) {
        if !self.section.active || !self.section.picking {
            return;
        }
        let r = match hit {
            None => Err(tr("section.need_flat")),
            Some((face, point, n)) => match face.and_then(|i| self.face(i)).cloned() {
                Some(f) => {
                    let o = f.get("o").map(crate::partview::p3);
                    AN::face_section(jv::s(&f, "type"), f.get("n").map(crate::partview::p3), o, None)
                }
                None => AN::face_section("plane", Some(n), Some(point), None),
            },
        };
        match r {
            Ok((n, c)) => {
                self.section.face = Some((n, c));
                self.section.picking = false;
                self.note = String::new();
                let _ = self.section_base("face");
            }
            Err(e) => self.note = e,
        }
    }

    /// The face under pixel (x, y) of the view (physical pixels): its index (when the mesh names its faces), the
    /// point hit and the facet's normal (the part's frame) — view3d.js `pickAt`.
    pub fn face_hit(&self, x: f64, y: f64) -> Option<(Option<usize>, [f64; 3], [f64; 3])> {
        let m = self.regen.raw_mesh.as_ref()?;
        let (o, d) = self.view.cam.ray(x, y);
        let (o, d) = ([o[0], o[2], -o[1]], [d[0], d[2], -d[1]]);
        let pt = |vi: u32| -> [f64; 3] {
            let i = vi as usize * 3;
            [
                m.positions.get(i).copied().unwrap_or(0.0),
                m.positions.get(i + 1).copied().unwrap_or(0.0),
                m.positions.get(i + 2).copied().unwrap_or(0.0),
            ]
        };
        let mut best: Option<(f64, usize, [f64; 3])> = None;
        for (t, tri) in m.indices.as_chunks::<3>().0.iter().enumerate() {
            let [a, b, c] = tri.map(pt);
            if let Some(hit) = crate::partview::ray_tri(o, d, a, b, c)
                && best.is_none_or(|(bt, _, _)| hit < bt)
            {
                let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
                let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
                best = Some((hit, t, n));
            }
        }
        let (hit, t, n) = best?;
        let face =
            m.tri_face.as_deref().and_then(|tf| tf.get(t)).and_then(|&f| usize::try_from(f).ok()).filter(|_| self.regen.faces.is_some());
        Some((face, [o[0] + d[0] * hit, o[1] + d[1] * hit, o[2] + d[2] * hit], n))
    }

    pub fn section_at(&mut self, v: f64) {
        if v.is_finite() {
            self.section.c = v;
            self.apply_section();
        }
    }

    /// The cut as the view draws it (view3d.js `setSection`: flipped = the other side).
    pub fn apply_section(&mut self) {
        let s = &self.section;
        self.view.style.section = s.active.then(|| {
            let (n, c) = if s.flip { ([-s.n[0], -s.n[1], -s.n[2]], -s.c) } else { (s.n, s.c) };
            ([n[0], -n[2], n[1]], c)
        });
    }

    /// Esc with the section card open: cancel the face pick, else close (section-ui.js `keydown`).
    pub fn section_escape(&mut self) -> bool {
        if !self.section.active {
            return false;
        }
        if self.section.picking {
            self.section_arm();
        } else {
            self.section_stop();
        }
        true
    }

    /// `part.measure` / `part.measure.at` / `part.measure.click` / `part.measure.move` / `part.section`.
    pub fn measure_cmd(&mut self, id: &str, name: &str) -> Result<(), String> {
        match id {
            "part.measure" => match name {
                "" | "toggle" => {
                    if self.measure.active {
                        self.measure_stop();
                    } else {
                        self.measure_start();
                    }
                }
                "on" => {
                    if !self.measure.active {
                        self.measure_start();
                    }
                }
                "off" => self.measure_stop(),
                "mass" => self.measure_mass(),
                "clear" => self.measure_clear(),
                "copy" => self.measure_copy(),
                "escape" => {
                    self.measure_escape();
                }
                // the card's rows in one line (harness): "key = value | …"
                "rows" => {
                    self.note = self.measure_rows().iter().map(|(k, v)| format!("{k} = {v}")).collect::<Vec<_>>().join(" | ");
                }
                other => {
                    let Some(d) = other.strip_prefix("density:") else {
                        return Err(format!(
                            "part.measure: toggle, on, off, mass, clear, copy, escape, rows or density:<g/cm³>, not {other}"
                        ));
                    };
                    let d = if d.is_empty() { None } else { Some(d.parse::<f64>().map_err(|_| format!("part.measure: density:{d}"))?) };
                    self.measure_density(d);
                }
            },
            "part.measure.at" => {
                if !self.measure.active {
                    return Err("part.measure.at: Measure is not on".into());
                }
                let pk = self.measure_named(name)?;
                self.measure_pick(Some(pk));
            }
            "part.measure.click" => {
                if !self.measure.active {
                    return Err("part.measure.click: Measure is not on".into());
                }
                let (x, y, shift) = xy_shift(name)?;
                let pk = self.measure_resolve(x, y, shift);
                self.measure_pick(pk);
            }
            "part.section" => {
                let (k, arg) = name.split_once(':').unwrap_or((name, ""));
                let on = matches!(arg, "on" | "true" | "1" | "");
                match k {
                    "" | "toggle" => {
                        if self.section.active {
                            self.section_stop();
                        } else {
                            self.section_start();
                        }
                    }
                    "off" => self.section_stop(),
                    "front" | "top" | "right" | "face" => self.section_base(k)?,
                    "at" => self.section_at(arg.trim().parse::<f64>().map_err(|_| format!("part.section: at:<mm>, not {name}"))?),
                    "flip" => {
                        self.section.flip = on;
                        self.apply_section();
                    }
                    "caps" => {
                        self.section.caps = on;
                        self.apply_section();
                    }
                    "pick" => self.section_arm(),
                    "click" => {
                        let (x, y, _) = xy_shift(arg)?;
                        let hit = self.face_hit(x, y);
                        self.section_pick(hit);
                    }
                    "facename" => {
                        let i = self
                            .faces()
                            .iter()
                            .position(|f| jv::is_str(f.get("name"), arg))
                            .ok_or_else(|| format!("part.section: no face {arg}"))?;
                        let o = self.face(i).and_then(|f| f.get("o")).map_or([f64::NAN; 3], crate::partview::p3);
                        self.section_pick(Some((Some(i), o, [f64::NAN; 3])));
                    }
                    "escape" => {
                        self.section_escape();
                    }
                    other => {
                        return Err(format!(
                            "part.section: toggle, off, front, top, right, face, at:<mm>, flip:on|off, caps:on|off, pick, click:x,y, facename:<name> or escape, not {other}"
                        ));
                    }
                }
            }
            other => return Err(format!("unknown command {other}")),
        }
        Ok(())
    }
}

fn xy_shift(s: &str) -> Result<(f64, f64, bool), String> {
    let p: Vec<&str> = s.split(',').map(str::trim).collect();
    let n = |i: usize| p.get(i).and_then(|x| x.parse::<f64>().ok()).filter(|x| x.is_finite());
    match (n(0), n(1)) {
        (Some(x), Some(y)) => Ok((x, y, p.contains(&"shift"))),
        _ => Err(format!("x,y[,shift] in view pixels, not {s}")),
    }
}

/// The material list of the card (bom.js `MATERIALS`): words and density.
pub fn materials() -> Vec<(String, f64)> {
    bom::materials()
}

/// The card's material option words.
pub fn material_option(name: &str, d: f64) -> String {
    trv("ui.measure.material_option", &[("name", name.to_string()), ("d", cc_part::js::num_str(d))])
}

// ---------------------------------------------------------------------------------------------- the window

use crate::partpanel::{Cmd, cmd};
use crate::theme::Tokens;
use egui::{Rect, RichText};

/// The Views menu of the tool bar (ui.inc `data-views-menu`): Measure, Mass properties, Section view.
pub fn views_menu(ui: &mut egui::Ui, pv: &PartView, act: &mut Cmd) {
    if ui
        .add(egui::Button::selectable(pv.measure.active, format!("{}   M", tr("ui.tb.views.measure"))))
        .on_hover_text(tr("ui.tb.views.measure.title"))
        .clicked()
    {
        *act = cmd("part.measure", "toggle");
        ui.close();
    }
    if ui.add(crate::shell::Btn::new(tr("ui.tb.views.mass"))).on_hover_text(tr("ui.tb.views.mass.title")).clicked() {
        *act = cmd("part.measure", "mass");
        ui.close();
    }
    if ui
        .add(egui::Button::selectable(pv.section.active, format!("{}   X", tr("section.menu"))))
        .on_hover_text(tr("section.menu.title"))
        .clicked()
    {
        *act = cmd("part.section", "toggle");
        ui.close();
    }
    for (m, key) in [("draft", "draft.menu"), ("zebra", "analysis.menu.zebra"), ("curvature", "analysis.menu.curvature")] {
        let on = pv.analysis.active && pv.analysis.mode == m;
        if ui.add(egui::Button::selectable(on, tr(key))).on_hover_text(tr(&format!("{key}.title"))).clicked() {
            *act = cmd("part.analysis", format!("toggle:{m}"));
            ui.close();
        }
    }
}

/// What the measure overlay draws: picked points, the snap under the pointer, the closest points and their line.
pub fn overlay(ui: &egui::Ui, rect: Rect, pv: &PartView, t: &Tokens, ppp: f64) {
    if !pv.measure.active {
        return;
    }
    let painter = ui.painter_at(rect);
    let to = |p: [f64; 3]| pv.screen_of(p).map(|q| egui::pos2(rect.min.x + (q.0 / ppp) as f32, rect.min.y + (q.1 / ppp) as f32));
    let dot = |p: [f64; 3], c: egui::Color32| {
        if let Some(s) = to(p) {
            painter.circle_filled(s, 4.5, c);
        }
    };
    for pk in &pv.measure.picks {
        dot(pk.local, t.accent);
    }
    if let Some(h) = &pv.measure.hover {
        dot(h.local, egui::Color32::from_rgb(0x1a, 0x9f, 0xb5));
    }
    if let Some(p) = &pv.measure.pair
        && jv::truthy(p.get("p1"))
    {
        let (a, b) = (crate::partview::p3(p.get("p1").unwrap_or(&Value::Null)), crate::partview::p3(p.get("p2").unwrap_or(&Value::Null)));
        dot(a, t.accent);
        dot(b, t.accent);
        if let (Some(sa), Some(sb)) = (to(a), to(b)) {
            painter.line_segment([sa, sb], egui::Stroke::new(1.5, t.accent));
        }
    }
}

/// The hover preview of the snap (measure-ui.js's pointermove).
pub fn hover(pv: &mut PartView, px: Option<(f64, f64)>, shift: bool) {
    if !pv.measure.active {
        return;
    }
    let pk = px.and_then(|(x, y)| pv.measure_resolve(x, y, shift));
    pv.measure.hover = pk.filter(MPick::is_point);
}

fn card_frame(t: &Tokens) -> egui::Frame {
    egui::Frame::new()
        .fill(t.panel)
        .stroke(egui::Stroke::new(1.0, t.divider))
        .corner_radius(f32::from(t.radius))
        .inner_margin(egui::Margin::same(8))
}

/// The Measure card and the Section card over the view (top right), as measure-ui.js / section-ui.js render them.
pub fn cards(ui: &mut egui::Ui, rect: Rect, pv: &mut PartView, t: &Tokens, act: &mut Cmd) {
    let mut y = rect.min.y + 8.0;
    if pv.measure.active {
        let r = egui::Area::new(egui::Id::unique("pc-measure"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(rect.max.x - 300.0, y))
            .show(ui.ctx(), |ui| {
                card_frame(t).show(ui, |ui| {
                    ui.set_width(280.0);
                    measure_card(ui, pv, t, act);
                });
            });
        y = r.response.rect.max.y + 8.0;
    }
    if pv.section.active {
        let r = egui::Area::new(egui::Id::unique("pc-section"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(rect.max.x - 300.0, y))
            .show(ui.ctx(), |ui| {
                card_frame(t).show(ui, |ui| {
                    ui.set_width(280.0);
                    section_card(ui, pv, t, act);
                });
            });
        y = r.response.rect.max.y + 8.0;
    }
    if pv.analysis.active {
        egui::Area::new(egui::Id::unique("pc-analysis")).order(egui::Order::Foreground).fixed_pos(egui::pos2(rect.max.x - 300.0, y)).show(
            ui.ctx(),
            |ui| {
                card_frame(t).show(ui, |ui| {
                    ui.set_width(280.0);
                    crate::partanalysis::card(ui, pv, t, act);
                });
            },
        );
    }
    if let Some(text) = pv.measure.clip_out.take() {
        ui.ctx().copy_text(text);
    }
}

fn measure_card(ui: &mut egui::Ui, pv: &mut PartView, t: &Tokens, act: &mut Cmd) {
    let rows = pv.measure_rows();
    ui.horizontal(|ui| {
        let head = if pv.measure.mass.is_some() { tr("ui.measure.mass") } else { tr("ui.measure.title") };
        ui.label(RichText::new(head).strong().color(t.text_strong));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button(tr("ui.measure.close")).on_hover_text(tr("ui.measure.close.title")).clicked() {
                *act = cmd("part.measure", "off");
            }
        });
    });
    if rows.is_empty() {
        ui.label(RichText::new(tr("ui.measure.empty")).small().color(t.text_dim));
    } else {
        egui::Grid::new("pc-measure-rows").num_columns(2).spacing([8.0, 2.0]).show(ui, |ui| {
            for (k, v) in &rows {
                ui.label(RichText::new(k).small().color(t.text_dim));
                ui.label(RichText::new(v).small().color(t.text));
                ui.end_row();
            }
        });
    }
    if let Some(m) = pv.measure.mass.as_ref().filter(|m| !m.missing.is_empty()) {
        let words = i18n::t(
            "ui.measure.no_mass",
            &[("n", i18n::F::N(cc_part::js::f(m.missing.len()))), ("names", i18n::F::S(m.missing.join(", ")))],
        );
        ui.label(RichText::new(words).small().color(t.text_dim));
    }
    if let Some(d) = pv.part.get("material").and_then(|m| jv::as_num(m.get("density"))) {
        let name = pv.part.get("material").map(|m| jv::s(m, "name").to_string()).unwrap_or_default();
        ui.label(
            RichText::new(trv("measure.material_from_part", &[("name", name), ("d", cc_part::js::num_str(d))])).small().color(t.text_dim),
        );
    } else {
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("ui.measure.material")).small());
            let mats = materials();
            let cur = pv.measure.density;
            let shown = mats.iter().find(|m| Some(m.1) == cur).map_or_else(|| tr("ui.measure.no_material"), |m| material_option(&m.0, m.1));
            egui::ComboBox::new("pc-measure-material", "").selected_text(shown).width(190.0).show_ui(ui, |ui| {
                if ui.selectable_label(cur.is_none(), tr("ui.measure.no_material")).clicked() {
                    *act = cmd("part.measure", "density:");
                }
                for (name, d) in &mats {
                    if ui.selectable_label(cur == Some(*d), material_option(name, *d)).clicked() {
                        *act = cmd("part.measure", format!("density:{}", cc_part::js::num_str(*d)));
                    }
                }
            });
        });
    }
    ui.horizontal(|ui| {
        if ui.add_enabled(!rows.is_empty(), egui::Button::new(tr("ui.measure.copy")).small()).clicked() {
            *act = cmd("part.measure", "copy");
        }
        if ui.add(egui::Button::new(tr("ui.measure.mass")).small()).clicked() {
            *act = cmd("part.measure", "mass");
        }
        if ui.add_enabled(!rows.is_empty(), egui::Button::new(tr("ui.measure.clear")).small()).clicked() {
            *act = cmd("part.measure", "clear");
        }
    });
}

fn section_card(ui: &mut egui::Ui, pv: &mut PartView, t: &Tokens, act: &mut Cmd) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(tr("section.title")).strong().color(t.text_strong));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button(tr("section.close")).on_hover_text(tr("section.close.title")).clicked() {
                *act = cmd("part.section", "off");
            }
        });
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new(tr("section.plane")).small());
        let shown = tr(&format!("section.base.{}", pv.section.base));
        egui::ComboBox::new("pc-section-plane", "").selected_text(shown).width(130.0).show_ui(ui, |ui| {
            for k in ["front", "top", "right"] {
                if ui.selectable_label(pv.section.base == k, tr(&format!("section.base.{k}"))).clicked() {
                    *act = cmd("part.section", k);
                }
            }
            if ui
                .add_enabled(pv.section.face.is_some(), egui::Button::selectable(pv.section.base == "face", tr("section.base.face")))
                .clicked()
            {
                *act = cmd("part.section", "face");
            }
        });
        let words = if pv.section.picking { tr("section.picking") } else { tr("section.pick") };
        if ui.add(egui::Button::selectable(pv.section.picking, words).small()).clicked() {
            *act = cmd("part.section", "pick");
        }
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new(tr("section.position")).small());
        let [lo, hi] = pv.section_range();
        let mut c = pv.section.c;
        let span = (hi - lo).max(1e-9);
        let sr = ui.add(egui::Slider::new(&mut c, lo..=hi).step_by(span / 1000.0).show_value(false)).on_hover_text(tr("section.position"));
        // only a drag or a click moves the cut (the slider's own step rounding must not)
        if sr.changed() && (sr.dragged() || sr.clicked()) {
            *act = cmd("part.section", format!("at:{c}"));
        }
        let mut text = pv.section.typing.clone().unwrap_or_else(|| fmt3(pv.section.c));
        let resp = ui.add_sized([60.0, 18.0], egui::TextEdit::singleline(&mut text)).on_hover_text(tr("section.position_mm"));
        if resp.has_focus() {
            pv.section.typing = Some(text.clone());
        }
        if resp.lost_focus() {
            pv.section.typing = None;
            if let Ok(v) = text.trim().parse::<f64>()
                && v.is_finite()
            {
                *act = cmd("part.section", format!("at:{v}"));
            }
        }
        ui.label(RichText::new("mm").small().color(t.text_dim));
    });
    let mut flip = pv.section.flip;
    if ui.checkbox(&mut flip, tr("section.flip")).changed() {
        *act = cmd("part.section", if flip { "flip:on" } else { "flip:off" });
    }
    let mut caps = pv.section.caps;
    if ui.checkbox(&mut caps, tr("section.caps")).changed() {
        *act = cmd("part.section", if caps { "caps:on" } else { "caps:off" });
    }
    let note = if pv.section.picking { tr("section.pick.hint") } else { tr("section.note") };
    ui.label(RichText::new(note).small().color(t.text_dim));
}

#[cfg(test)]
mod tests {
    use crate::{Params, UiApp};

    fn run(app: &mut UiApp, id: &str, name: &str) {
        app.run(id, &Params { name: Some(name.into()), ..Default::default() }).unwrap_or_else(|e| panic!("{id}={name}: {e}"));
    }

    #[test]
    fn mass_properties_picks_and_the_section() {
        let mut app = UiApp::default();
        run(&mut app, "part.sample", "");
        run(&mut app, "part.measure", "mass");
        let pv = app.part.as_mut().unwrap();
        assert!(pv.measure.active);
        let rows = pv.measure_rows();
        assert!(rows.iter().any(|(_, v)| v == "101189.679 mm³"), "{rows:?}");
        run(&mut app, "part.measure", "density:7.85");
        let pv = app.part.as_mut().unwrap();
        assert!(pv.measure_rows().iter().any(|(_, v)| v == "794.339 g"));
        // two vertices: their distance; a third pick starts again; a rebuild drops the picks
        run(&mut app, "part.measure.at", "snap:0");
        run(&mut app, "part.measure.at", "snap:5");
        let pv = app.part.as_mut().unwrap();
        assert!(pv.measure.pair.is_some());
        run(&mut app, "part.measure.at", "edge:3");
        assert_eq!(app.part.as_ref().unwrap().measure.picks.len(), 1);
        run(&mut app, "part.param", "length=150");
        assert!(app.part.as_ref().unwrap().measure.picks.is_empty());
        // a click in the view resolves to something on the solid (the default camera looks at the part)
        let pv = app.part.as_mut().unwrap();
        pv.view.cam.width = 800.0;
        pv.view.cam.height = 600.0;
        pv.standard("iso");
        let on = (0..40)
            .flat_map(|i| (0..30).map(move |j| (f64::from(i) * 20.0, f64::from(j) * 20.0)))
            .find(|&(x, y)| pv.face_hit(x, y).is_some());
        let (x, y) = on.expect("the solid is in view");
        assert!(pv.measure_resolve(x, y, false).is_some());
        assert!(pv.measure_resolve(x, y, true).is_some_and(|p| p.kind == "body"));
        assert!(pv.measure_resolve(-50.0, -50.0, false).is_none());
        // hostile names are errors, not panics
        for bad in ["snap:999999", "edge:-1", "face:nope", "body|1,2", "body|nan,1,1", "x"] {
            assert!(app.run("part.measure.at", &Params { name: Some(bad.into()), ..Default::default() }).is_err(), "{bad}");
        }
        run(&mut app, "part.measure", "escape");
        run(&mut app, "part.measure", "escape");
        assert!(!app.part.as_ref().unwrap().measure.active);
        // the section cuts the view only
        run(&mut app, "part.section", "toggle");
        run(&mut app, "part.section", "top");
        run(&mut app, "part.section", "at:20");
        let pv = app.part.as_ref().unwrap();
        assert_eq!(pv.view.style.section, Some(([0.0, 0.0, 1.0], 20.0)));
        run(&mut app, "part.section", "flip:on");
        assert_eq!(app.part.as_ref().unwrap().view.style.section, Some(([-0.0, -0.0, -1.0], -20.0)));
        run(&mut app, "part.section", "pick");
        run(&mut app, "part.section", "facename:f2.end");
        let pv = app.part.as_ref().unwrap();
        assert_eq!(pv.section.base, "face");
        assert!(!pv.section.picking);
        run(&mut app, "part.section", "toggle");
        assert_eq!(app.part.as_ref().unwrap().view.style.section, None);
    }
}
