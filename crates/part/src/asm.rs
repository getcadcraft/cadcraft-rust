//! assembly.js — parts put together with joints (`<name>.partcraft-asm.json`, format 'partcraft-assembly', versions
//! 1–4, every part embedded): instances, joints (rigid, revolute, slider, cylindrical, planar, ball, pin-slot) with
//! limits, placement walked from the grounded instance through the joints, free instances moved by hand,
//! interference filters, exploded views (explode.rs) and the untrusted-file reader. Kernel results ("builds") come in as
//! data: per part the feature results, face candidates, revolve and hole axes and reference geometry, as the mesh
//! kernel's `regenerate()` gives them (kernel.js `planeFrame` is ported here). The interference and motion checks,
//! which need solid booleans, are in kernel_asm.rs (on cc-manifold).

use crate::bom;
use crate::explode;
use crate::i18n::{F, t};
use crate::js;
use crate::jv;
use crate::part;
use crate::readers::{self, Ctx};
use crate::vec::{self as v, Frame, M4, V3};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub const FORMAT: &str = "partcraft-assembly";
pub const VERSION: f64 = 4.0;
pub const JOINT_TYPES: [(&str, &str); 7] = [
    ("rigid", "Rigid"),
    ("revolute", "Revolute"),
    ("slider", "Slider"),
    ("cylindrical", "Cylindrical"),
    ("planar", "Planar"),
    ("ball", "Ball"),
    ("pinslot", "PinSlot"),
];
pub const CLASH_KINDS: [&str; 4] = ["overlap", "thread-depth", "thread", "touch"];
const NEW_TYPES: [&str; 4] = ["cylindrical", "planar", "ball", "pinslot"];
const EXTRA_VALUES: [&str; 6] = ["dx", "dy", "tiltX", "tiltY", "slide", "slotDir"];
const IDENT: M4 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
const FLIP: M4 = [1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

pub fn joint_label(ty: &str) -> Option<&'static str> {
    JOINT_TYPES.iter().find(|(k, _)| *k == ty).map(|(_, l)| *l)
}
/// `JOINT_DOF[type]`.
pub fn joint_dof(ty: &str) -> Option<&'static [&'static str]> {
    Some(match ty {
        "rigid" => &[],
        "revolute" => &["angle"],
        "slider" => &["offset"],
        "cylindrical" => &["angle", "offset"],
        "planar" => &["dx", "dy", "angle"],
        "ball" => &["angle", "tiltX", "tiltY"],
        "pinslot" => &["slide", "angle"],
        _ => return None,
    })
}
fn value_unit_deg(key: &str) -> bool {
    matches!(key, "angle" | "tiltX" | "tiltY" | "slotDir")
}

fn m4(v: Option<&Value>) -> M4 {
    let a = jv::arr(v);
    let mut m = [f64::NAN; 16];
    for (i, x) in m.iter_mut().enumerate() {
        *x = jv::as_num(a.get(i)).unwrap_or(f64::NAN);
    }
    m
}
fn p3(v: Option<&Value>) -> V3 {
    let a = jv::arr(v);
    let g = |i: usize| jv::as_num(a.get(i)).unwrap_or(f64::NAN);
    [g(0), g(1), g(2)]
}
fn insts(asm: &Value) -> &[Value] {
    jv::arr(asm.get("instances"))
}
fn joints(asm: &Value) -> &[Value] {
    jv::arr(asm.get("joints"))
}
fn seq_next(asm: &mut Value) -> f64 {
    let s = jv::as_num(asm.get("seq")).unwrap_or(f64::NAN);
    jv::set(asm, "seq", jv::num(s + 1.0));
    s
}

/// `newAssembly(name)`.
pub fn new_assembly(name: &str) -> Value {
    jv::obj(vec![
        ("format", FORMAT.into()),
        ("version", jv::num(VERSION)),
        ("name", name.into()),
        ("seq", 1.into()),
        ("parts", jv::obj(vec![])),
        ("instances", Value::Array(Vec::new())),
        ("joints", Value::Array(Vec::new())),
        ("views", Value::Array(Vec::new())),
    ])
}

/// `insertPart(asm, part, { place, pid })` → the instance.
pub fn insert_part(asm: &mut Value, pt: Option<&Value>, place: Option<&Value>, pid: Option<&str>) -> Result<Value, String> {
    let pid = match pid.filter(|p| !p.is_empty()) {
        Some(p) => p.to_string(),
        None => {
            let pt = pt.ok_or_else(|| "Cannot read properties of undefined (reading 'name')".to_string())?;
            let id = format!("p{}", js::num_str(seq_next(asm)));
            let copy = jv::parse(&part::to_json(pt)).unwrap_or(Value::Null);
            if let Some(ps) = asm.get_mut("parts") {
                jv::set(ps, &id, jv::obj(vec![("name", pt.get("name").cloned().unwrap_or(Value::Null)), ("part", copy)]));
            }
            id
        }
    };
    let n = insts(asm).iter().filter(|i| jv::is_str(i.get("part"), &pid)).count().saturating_add(1);
    let pname = asm
        .get("parts")
        .and_then(|p| p.get(&pid))
        .map(|p| js::string_of(p.get("name")))
        .ok_or_else(|| "Cannot read properties of undefined (reading 'name')".to_string())?;
    let id = format!("i{}", js::num_str(seq_next(asm)));
    let place = match place.filter(|p| jv::truthy(Some(p))) {
        Some(p) => Value::Array(jv::arr(Some(p)).to_vec()),
        None => jv::nums(&IDENT),
    };
    let inst = jv::obj(vec![("id", id.into()), ("part", pid.into()), ("name", format!("{pname}<{n}>").into()), ("place", place)]);
    if let Some(a) = asm.get_mut("instances").and_then(Value::as_array_mut) {
        a.push(inst.clone());
    }
    Ok(inst)
}

/// `addJoint(asm, type, a, b, { angle, offset, …more })`.
pub fn add_joint(asm: &mut Value, ty: &str, a: &Value, b: &Value, more: &Value) -> Result<Value, String> {
    let Some(label) = joint_label(ty) else { return Err(t("model.assembly.unknown_joint", &[])) };
    if jv::strict_eq(a.get("inst"), b.get("inst")) {
        return Err(t("model.assembly.joint_two_parts", &[]));
    }
    let n = joints(asm).iter().filter(|j| jv::is_str(j.get("type"), ty)).count().saturating_add(1);
    let id = format!("j{}", js::num_str(seq_next(asm)));
    let dflt = |k: &str| match more.get(k) {
        None | Some(Value::Null) => Value::from(0),
        Some(x) => x.clone(),
    };
    let mut j = jv::obj(vec![
        ("id", id.into()),
        ("name", format!("{label}{n}").into()),
        ("type", ty.into()),
        ("a", a.clone()),
        ("b", b.clone()),
        ("angle", dflt("angle")),
        ("offset", dflt("offset")),
    ]);
    for k in EXTRA_VALUES {
        if let Some(x) = jv::finite(more.get(k)) {
            jv::set(&mut j, k, jv::num(x));
        }
    }
    if let Some(arr) = asm.get_mut("joints").and_then(Value::as_array_mut) {
        arr.push(j.clone());
    }
    Ok(j)
}

/// `removeInstance(asm, id)`.
pub fn remove_instance(asm: &mut Value, id: &str) {
    if let Some(a) = asm.get_mut("instances").and_then(Value::as_array_mut) {
        a.retain(|i| !jv::is_str(i.get("id"), id));
    }
    if let Some(a) = asm.get_mut("joints").and_then(Value::as_array_mut) {
        a.retain(|j| !jv::is_str(j.get("a").and_then(|e| e.get("inst")), id) && !jv::is_str(j.get("b").and_then(|e| e.get("inst")), id));
    }
    if let Some(vs) = asm.get_mut("views").and_then(Value::as_array_mut) {
        for view in vs.iter_mut() {
            let steps: Vec<Value> = jv::arr(view.get("steps"))
                .iter()
                .map(|s| {
                    let mut s2 = s.clone();
                    let keep: Vec<Value> = jv::arr(s.get("insts")).iter().filter(|i| !jv::is_str(Some(i), id)).cloned().collect();
                    jv::set(&mut s2, "insts", Value::Array(keep));
                    s2
                })
                .filter(|s| !jv::arr(s.get("insts")).is_empty())
                .collect();
            jv::set(view, "steps", Value::Array(steps));
        }
    }
    if let Some(cf) = asm.get_mut("clashFilter").filter(|c| jv::truthy(Some(c))) {
        let pairs: Vec<Value> =
            jv::arr(cf.get("pairs")).iter().filter(|p| !jv::arr(Some(p)).iter().any(|x| jv::is_str(Some(x), id))).cloned().collect();
        jv::set(cf, "pairs", Value::Array(pairs));
        tidy_filter(asm);
    }
    let used: HashSet<String> = insts(asm).iter().map(|i| js::string_of(i.get("part"))).collect();
    if let Some(ps) = asm.get_mut("parts").and_then(Value::as_object_mut) {
        ps.retain(|k, _| used.contains(k));
    }
}

/// `rigidInverse(m)`.
pub fn rigid_inverse(m: &M4) -> M4 {
    let r = [m[0], m[4], m[8], 0.0, m[1], m[5], m[9], 0.0, m[2], m[6], m[10], 0.0];
    let tt = [m[12], m[13], m[14]];
    [
        r[0],
        r[1],
        r[2],
        0.0,
        r[4],
        r[5],
        r[6],
        0.0,
        r[8],
        r[9],
        r[10],
        0.0,
        -(r[0] * tt[0] + r[4] * tt[1] + r[8] * tt[2]),
        -(r[1] * tt[0] + r[5] * tt[1] + r[9] * tt[2]),
        -(r[2] * tt[0] + r[6] * tt[1] + r[10] * tt[2]),
        1.0,
    ]
}

/// A part's kernel result, as data.
#[derive(Clone, Debug, Default)]
pub struct Build {
    pub results: Value,
    pub candidates: Value,
    pub axes: Vec<(String, V3, V3)>,
    pub hole_axes: Vec<(String, Vec<(V3, V3)>)>,
    pub ref_geom: Value,
    pub exact: bool,
    pub planes: Value,
}
impl Build {
    pub fn from_json(b: &Value) -> Build {
        Build {
            results: b.get("results").cloned().unwrap_or(Value::Null),
            candidates: b.get("candidates").cloned().unwrap_or(Value::Null),
            axes: b
                .get("axes")
                .and_then(Value::as_object)
                .map(|o| o.iter().map(|(k, a)| (k.clone(), p3(a.get("o")), p3(a.get("d")))).collect())
                .unwrap_or_default(),
            hole_axes: b
                .get("holeAxes")
                .and_then(Value::as_object)
                .map(|o| {
                    o.iter()
                        .map(|(k, l)| (k.clone(), jv::arr(Some(l)).iter().map(|a| (p3(a.get("o")), p3(a.get("d")))).collect()))
                        .collect()
                })
                .unwrap_or_default(),
            ref_geom: b.get("refGeom").cloned().unwrap_or(Value::Null),
            exact: jv::truthy(b.get("exact")),
            planes: b.get("planes").cloned().unwrap_or(Value::Null),
        }
    }
}

/// kernel.js `planeFrame(ref, regen, part)` (the mesh kernel's resolvePlane; any failure → None).
pub fn plane_frame(r: &Value, build: Option<&Build>, pt: &Value) -> Result<Option<Frame>, String> {
    let kind = jv::s(r, "kind");
    if kind == "ref" {
        let g = build.and_then(|b| b.ref_geom.get(js::str_of(r.get("feature"))));
        return Ok(g.filter(|g| jv::is_str(g.get("kind"), "plane")).map(|g| Frame::from_json(g.get("frame").unwrap_or(&Value::Null))));
    }
    let Some(b) = build else { return Err("Cannot read properties of undefined (reading 'results')".into()) };
    if b.exact {
        // the exact kernel lists every flat face of the body by name (brep.js): not built in the fork
        return Ok(match kind {
            "base" => v::base_plane(jv::s(r, "name")),
            "fixed" => Some(v::frame_for(p3(r.get("n")), p3(r.get("o")))),
            _ => None,
        });
    }
    Ok(match kind {
        "base" => v::base_plane(jv::s(r, "name")),
        "fixed" => Some(v::frame_for(p3(r.get("n")), p3(r.get("o")))),
        "ref" => None,
        _ => {
            let fid = js::str_of(r.get("feature"));
            let res = b.results.get(&fid);
            let ok = res.is_some_and(|x| jv::is_str(x.get("status"), "ok"));
            let face = jv::s(r, "face");
            if !ok || face == "named" {
                None
            } else {
                let _ = pt;
                jv::arr(b.candidates.get(&fid))
                    .iter()
                    .find(|k| {
                        let kr = k.get("ref").unwrap_or(&Value::Null);
                        jv::strict_eq(kr.get("face"), r.get("face")) && (face != "side" || jv::strict_eq(kr.get("entity"), r.get("entity")))
                    })
                    .map(|c| Frame::from_json(c.get("frame").unwrap_or(&Value::Null)))
            }
        }
    })
}

/// kernel.js `faceRefAt(regen, featureId, normal, point)`.
pub fn face_ref_at(build: &Build, feature: &str, normal: V3, point: V3) -> Value {
    let n = v::norm(normal);
    for c in jv::arr(build.candidates.get(feature)) {
        let f = Frame::from_json(c.get("frame").unwrap_or(&Value::Null));
        if v::dot(f.n, n) > 1.0 - 1e-6 && v::dot(v::sub(point, f.o), f.n).abs() < 1e-4 * js::max(1.0, v::len(point)) {
            let cr = c.get("ref").unwrap_or(&Value::Null);
            let mut o = jv::obj(vec![
                ("kind", "face".into()),
                ("feature", feature.into()),
                ("face", cr.get("face").cloned().unwrap_or(Value::Null)),
            ]);
            if let Some(e) = cr.get("entity").filter(|e| jv::truthy(Some(e))) {
                jv::set(&mut o, "entity", e.clone());
            }
            return o;
        }
    }
    jv::obj(vec![
        ("kind", "fixed".into()),
        ("o", jv::nums(&point.map(|x| js::fixed_num(x, 9)))),
        ("n", jv::nums(&n.map(|x| js::fixed_num(x, 12)))),
    ])
}

fn feature_name(pt: &Value, fid: &str) -> String {
    part::features(pt).iter().find(|f| jv::is_str(f.get("id"), fid)).map_or_else(|| fid.to_string(), |f| js::string_of(f.get("name")))
}

/// One axis a mate can turn about.
#[derive(Clone, Debug)]
pub struct AxisOf {
    pub key: String,
    pub feature: String,
    pub index: Option<usize>,
    pub o: V3,
    pub d: V3,
    pub label: String,
}

/// `axesOf(build, part)`.
pub fn axes_of(build: Option<&Build>, pt: &Value) -> Vec<AxisOf> {
    let mut out = Vec::new();
    let Some(b) = build else { return out };
    for (fid, o, d) in &b.axes {
        out.push(AxisOf {
            key: fid.clone(),
            feature: fid.clone(),
            index: None,
            o: *o,
            d: *d,
            label: t("model.assembly.axis_of", &[("name", F::S(feature_name(pt, fid)))]),
        });
    }
    for (fid, list) in &b.hole_axes {
        for (i, (o, d)) in list.iter().enumerate() {
            out.push(AxisOf {
                key: format!("{fid}:{i}"),
                feature: fid.clone(),
                index: Some(i),
                o: *o,
                d: *d,
                label: t("model.assembly.hole_axis", &[("name", F::S(feature_name(pt, fid))), ("n", F::from(i.saturating_add(1)))]),
            });
        }
    }
    out
}

/// `mateFrame(end, build, part)`.
pub fn mate_frame(end: &Value, build: Option<&Build>, pt: &Value) -> Result<M4, String> {
    let pname = js::string_of(pt.get("name"));
    let Some(f) = plane_frame(end.get("plane").unwrap_or(&Value::Null), build, pt)? else {
        return Err(t("model.assembly.face_gone", &[("part", F::S(pname))]));
    };
    let mut o = f.o;
    if let Some(ax) = end.get("axis").filter(|x| jv::truthy(Some(x))) {
        let want_idx = jv::as_num(ax.get("index"));
        let found = axes_of(build, pt).into_iter().find(|a| jv::is_str(ax.get("feature"), &a.feature) && a.index.map(js::f) == want_idx);
        let Some(a) = found else { return Err(t("model.assembly.axis_gone", &[("part", F::S(pname))])) };
        let den = v::dot(a.d, f.n);
        if den.abs() < 1e-9 {
            return Err(t("model.assembly.axis_along_face", &[]));
        }
        o = v::add(a.o, v::mul(a.d, v::dot(v::sub(f.o, a.o), f.n) / den));
    }
    Ok(v::basis_matrix(f.x, f.y, f.n, o))
}

/// `limitOf(j, key)` → (min, max).
pub fn limit_of(j: &Value, key: Option<&str>) -> Option<(f64, f64)> {
    let ty = jv::s(j, "type");
    let dof = joint_dof(ty)?;
    let key = key.or_else(|| dof.first().copied())?;
    let l = j.get("limits").filter(|l| jv::truthy(Some(l)))?;
    if !dof.contains(&key) {
        return None;
    }
    if l.get("min").is_some_and(Value::is_number) {
        return if Some(&key) == dof.first() {
            Some((jv::as_num(l.get("min")).unwrap_or(f64::NAN), jv::as_num(l.get("max")).unwrap_or(f64::NAN)))
        } else {
            None
        };
    }
    let lk = l.get(key).filter(|x| jv::truthy(Some(x)) && x.get("min").is_some_and(Value::is_number))?;
    Some((jv::as_num(lk.get("min")).unwrap_or(f64::NAN), jv::as_num(lk.get("max")).unwrap_or(f64::NAN)))
}
fn or0(x: Option<&Value>) -> f64 {
    match jv::as_num(x) {
        Some(v) if v != 0.0 && !v.is_nan() => v,
        _ => 0.0,
    }
}
/// `jointValue(j, key)`.
pub fn joint_value(j: &Value, key: Option<&str>) -> f64 {
    let key = key.unwrap_or_else(|| joint_dof(jv::s(j, "type")).and_then(|d| d.first().copied()).unwrap_or("angle"));
    let val = or0(j.get(key));
    match limit_of(j, Some(key)) {
        Some((lo, hi)) => js::max(lo, js::min(hi, val)),
        None => val,
    }
}
/// `jointMotion(j)`.
pub fn joint_motion(j: &Value) -> M4 {
    let ty = jv::s(j, "type");
    let dof = joint_dof(ty).unwrap_or(&[]);
    let val = |k: &str| if dof.contains(&k) { joint_value(j, Some(k)) } else { or0(j.get(k)) };
    let rz = |deg: f64| v::rotation_about([0.0; 3], [0.0, 0.0, 1.0], deg);
    match ty {
        "planar" => v::mat_mul(&v::translation([val("dx"), val("dy"), 0.0]), &rz(val("angle"))),
        "ball" => v::mat_mul(
            &v::rotation_about([0.0; 3], [1.0, 0.0, 0.0], val("tiltX")),
            &v::mat_mul(&v::rotation_about([0.0; 3], [0.0, 1.0, 0.0], val("tiltY")), &rz(val("angle"))),
        ),
        "pinslot" => {
            let d = or0(j.get("slotDir")) * std::f64::consts::PI / 180.0;
            let s = val("slide");
            v::mat_mul(&v::translation([s * js::cos(d), s * js::sin(d), 0.0]), &rz(val("angle")))
        }
        _ => v::mat_mul(&v::translation([0.0, 0.0, val("offset")]), &rz(val("angle"))),
    }
}

/// Builds by part id.
pub type Builds = HashMap<String, Build>;

/// `placeAll(asm, builds)` → (world in instance order of setting, problems).
pub fn place_all(asm: &Value, builds: &Builds) -> (Vec<(String, M4)>, Vec<Value>) {
    let mut world: Vec<(String, M4)> = Vec::new();
    let mut problems = Vec::new();
    let is = insts(asm);
    let Some(ground) = is.first() else { return (world, problems) };
    let wget = |w: &[(String, M4)], id: &str| w.iter().find(|(k, _)| k == id).map(|(_, m)| *m);
    let inst_of = |id: &str| is.iter().find(|i| jv::is_str(i.get("id"), id));
    let frame_of = |end: &Value| -> Result<M4, String> {
        let iid = js::str_of(end.get("inst"));
        let inst = inst_of(&iid);
        let p = inst.and_then(|i| asm.get("parts").and_then(|ps| ps.get(js::str_of(i.get("part")))));
        let Some(p) = p else { return Err(t("model.assembly.joint_part_gone", &[])) };
        let pid = inst.map(|i| js::str_of(i.get("part"))).unwrap_or_default();
        mate_frame(end, builds.get(&pid), p.get("part").unwrap_or(&Value::Null))
    };
    world.push((js::str_of(ground.get("id")), m4(ground.get("place"))));
    let mut done: HashSet<String> = HashSet::new();
    let mut grew = true;
    let mut guard = 0usize;
    while grew && guard < 100_000 {
        guard = guard.saturating_add(1);
        grew = false;
        for j in joints(asm) {
            let jid = js::str_of(j.get("id"));
            if done.contains(&jid) {
                continue;
            }
            let ai = js::str_of(j.get("a").and_then(|e| e.get("inst")));
            let bi = js::str_of(j.get("b").and_then(|e| e.get("inst")));
            let ha = wget(&world, &ai).is_some();
            let hb = wget(&world, &bi).is_some();
            if !ha && !hb {
                continue;
            }
            done.insert(jid.clone());
            let jname = js::string_of(j.get("name"));
            let r: Result<(), String> = (|| {
                let fa = frame_of(j.get("a").unwrap_or(&Value::Null))?;
                let fb = frame_of(j.get("b").unwrap_or(&Value::Null))?;
                let m = joint_motion(j);
                let chain = v::mat_mul(&fa, &v::mat_mul(&m, &v::mat_mul(&FLIP, &rigid_inverse(&fb))));
                if ha && !hb {
                    let wa = wget(&world, &ai).unwrap_or(IDENT);
                    world.push((bi.clone(), v::mat_mul(&wa, &chain)));
                    grew = true;
                } else if hb && !ha {
                    let wb = wget(&world, &bi).unwrap_or(IDENT);
                    world.push((ai.clone(), v::mat_mul(&wb, &rigid_inverse(&chain))));
                    grew = true;
                } else {
                    let want = v::mat_mul(&wget(&world, &ai).unwrap_or(IDENT), &chain);
                    let have = wget(&world, &bi).unwrap_or(IDENT);
                    let mut off = f64::NEG_INFINITY;
                    for k in [12usize, 13, 14] {
                        off = js::max(off, (want.get(k).copied().unwrap_or(f64::NAN) - have.get(k).copied().unwrap_or(f64::NAN)).abs());
                    }
                    for k in [0usize, 1, 2, 4, 5, 6, 8, 9, 10] {
                        off = js::max(
                            off,
                            (want.get(k).copied().unwrap_or(f64::NAN) - have.get(k).copied().unwrap_or(f64::NAN)).abs() * 100.0,
                        );
                    }
                    if off > 1e-6 {
                        problems.push(jv::obj(vec![
                            ("joint", jid.as_str().into()),
                            (
                                "message",
                                t("model.assembly.loop_off", &[("name", F::S(jname.clone())), ("off", F::S(js::to_fixed(off, 3)))]).into(),
                            ),
                        ]));
                    }
                }
                Ok(())
            })();
            if let Err(e) = r {
                problems.push(jv::obj(vec![
                    ("joint", jid.as_str().into()),
                    ("message", t("model.assembly.joint_problem", &[("name", F::S(jname)), ("message", F::S(e))]).into()),
                ]));
            }
        }
    }
    for (k, i) in is.iter().enumerate() {
        let id = js::str_of(i.get("id"));
        if wget(&world, &id).is_none() {
            world.push((id.clone(), m4(i.get("place"))));
            if k != 0 {
                problems.push(jv::obj(vec![
                    ("inst", id.as_str().into()),
                    ("message", t("model.assembly.not_joined", &[("name", F::S(js::string_of(i.get("name"))))]).into()),
                    ("free", true.into()),
                ]));
            }
        }
    }
    (world, problems)
}

/// `jointTree(asm, builds, world)` → [{ joint, parent, child, normal }].
pub fn joint_tree(asm: &Value, builds: &Builds, world: &[(String, M4)]) -> Vec<Value> {
    let mut out = Vec::new();
    let is = insts(asm);
    let Some(g) = is.first() else { return out };
    let mut reached: HashSet<String> = HashSet::from([js::str_of(g.get("id"))]);
    let mut done: HashSet<String> = HashSet::new();
    let mut grew = true;
    let mut guard = 0usize;
    while grew && guard < 100_000 {
        guard = guard.saturating_add(1);
        grew = false;
        for j in joints(asm) {
            let jid = js::str_of(j.get("id"));
            if done.contains(&jid) {
                continue;
            }
            let ae = j.get("a").unwrap_or(&Value::Null);
            let be = j.get("b").unwrap_or(&Value::Null);
            let ha = reached.contains(&js::str_of(ae.get("inst")));
            let hb = reached.contains(&js::str_of(be.get("inst")));
            if ha == hb {
                if ha {
                    done.insert(jid);
                }
                continue;
            }
            done.insert(jid.clone());
            let (pe, ce) = if ha { (ae, be) } else { (be, ae) };
            let pinst = js::str_of(pe.get("inst"));
            reached.insert(js::str_of(ce.get("inst")));
            grew = true;
            let mut normal = [0.0, 1.0, 0.0];
            if let Some(inst) = is.iter().find(|i| jv::is_str(i.get("id"), &pinst)) {
                let pid = js::str_of(inst.get("part"));
                let ptv = asm.get("parts").and_then(|p| p.get(&pid)).and_then(|p| p.get("part"));
                if let Some(ptv) = ptv
                    && let Ok(f) = mate_frame(pe, builds.get(&pid), ptv)
                    && let Some((_, w)) = world.iter().find(|(k, _)| *k == pinst)
                {
                    normal = v::norm(v::apply_dir(w, [f[8], f[9], f[10]]));
                }
            }
            out.push(jv::obj(vec![
                ("joint", jid.into()),
                ("parent", pinst.into()),
                ("child", js::str_of(ce.get("inst")).into()),
                ("normal", jv::pt3(normal)),
            ]));
        }
    }
    out
}

/// `freeIds(asm)` (in instance order).
pub fn free_ids(asm: &Value) -> Vec<String> {
    let is = insts(asm);
    let mut reached: HashSet<String> = is.first().map(|g| HashSet::from([js::str_of(g.get("id"))])).unwrap_or_default();
    let mut grew = true;
    let mut guard = 0usize;
    while grew && guard < 100_000 {
        guard = guard.saturating_add(1);
        grew = false;
        for j in joints(asm) {
            let a = js::str_of(j.get("a").and_then(|e| e.get("inst")));
            let b = js::str_of(j.get("b").and_then(|e| e.get("inst")));
            let (ha, hb) = (reached.contains(&a), reached.contains(&b));
            if ha != hb {
                reached.insert(if ha { b } else { a });
                grew = true;
            }
        }
    }
    is.iter().map(|i| js::str_of(i.get("id"))).filter(|id| !reached.contains(id)).collect()
}

/// `movePlace(place, { move, rotate })`.
pub fn move_place(place: &M4, mv: Option<V3>, rotate: Option<(V3, f64, V3)>) -> Result<M4, String> {
    let mut m = *place;
    if let Some((axis, angle, about)) = rotate {
        if !axis.iter().all(|x| x.is_finite()) || v::len(axis) < 1e-12 || !angle.is_finite() || !about.iter().all(|x| x.is_finite()) {
            return Err(t("model.assembly.rotation_needs", &[]));
        }
        m = v::mat_mul(&v::rotation_about(about, v::norm(axis), angle), &m);
    }
    if let Some(d) = mv {
        if !d.iter().all(|x| x.is_finite()) {
            return Err(t("model.assembly.move_needs", &[]));
        }
        m = v::mat_mul(&v::translation(d), &m);
    }
    Ok(m.map(|x| if x.abs() < 1e-15 { 0.0 } else { x }))
}

/// `moveInstance(asm, id, place)`.
pub fn move_instance(asm: &mut Value, id: &str, place: &Value) -> Result<(), String> {
    let pos = insts(asm).iter().position(|i| jv::is_str(i.get("id"), id));
    let Some(pos) = pos else { return Err(t("model.assembly.no_part", &[])) };
    if pos == 0 {
        return Err(t("model.assembly.grounded_fixed", &[]));
    }
    if !free_ids(asm).iter().any(|x| x == id) {
        let name = insts(asm).get(pos).map(|i| js::string_of(i.get("name"))).unwrap_or_default();
        return Err(t("model.assembly.placed_by_joints", &[("name", F::S(name))]));
    }
    let ok = jv::as_arr(Some(place)).is_some_and(|a| a.len() == 16 && a.iter().all(|x| jv::finite(Some(x)).is_some_and(|v| v.abs() < 1e9)));
    if !ok {
        return Err(t("model.assembly.bad_position", &[]));
    }
    if let Some(i) = asm.get_mut("instances").and_then(|a| a.get_mut(pos)) {
        jv::set(i, "place", Value::Array(jv::arr(Some(place)).to_vec()));
    }
    Ok(())
}

fn pair_key(a: &str, b: &str) -> String {
    if js::cmp16(a, b) == std::cmp::Ordering::Less { format!("{a}|{b}") } else { format!("{b}|{a}") }
}
fn kind_index(k: &str) -> f64 {
    CLASH_KINDS.iter().position(|x| *x == k).map_or(-1.0, js::f)
}

/// `filterClashes(asm, list)` → (shown, hidden).
pub fn filter_clashes(asm: &Value, list: &[Value]) -> (Vec<Value>, Vec<Value>) {
    let f = asm.get("clashFilter").filter(|c| jv::truthy(Some(c)));
    let hide: HashSet<String> = jv::arr(f.and_then(|f| f.get("hide"))).iter().map(|x| js::string_of(Some(x))).collect();
    let pairs: HashSet<String> = jv::arr(f.and_then(|f| f.get("pairs")))
        .iter()
        .map(|p| {
            let a = jv::arr(Some(p));
            pair_key(&js::string_of(a.first()), &js::string_of(a.get(1)))
        })
        .collect();
    let (mut shown, mut hidden) = (Vec::new(), Vec::new());
    for c in list {
        if hide.contains(&js::string_of(c.get("kind"))) || pairs.contains(&pair_key(&js::string_of(c.get("a")), &js::string_of(c.get("b"))))
        {
            hidden.push(c.clone());
        } else {
            shown.push(c.clone());
        }
    }
    (shown, hidden)
}

fn filter_mut(asm: &mut Value) -> Option<&mut Value> {
    if !asm.get("clashFilter").is_some_and(|c| jv::truthy(Some(c))) {
        jv::set(asm, "clashFilter", jv::obj(vec![("hide", Value::Array(Vec::new())), ("pairs", Value::Array(Vec::new()))]));
    }
    asm.get_mut("clashFilter")
}
fn tidy_filter(asm: &mut Value) {
    let empty = asm
        .get("clashFilter")
        .filter(|c| jv::truthy(Some(c)))
        .is_some_and(|c| jv::arr(c.get("hide")).is_empty() && jv::arr(c.get("pairs")).is_empty());
    if empty {
        jv::del(asm, "clashFilter");
    }
}

/// `setKindHidden(asm, kind, hidden)`.
pub fn set_kind_hidden(asm: &mut Value, kind: &str, hidden: bool) {
    if !CLASH_KINDS.contains(&kind) {
        return;
    }
    if let Some(f) = filter_mut(asm) {
        let mut h: Vec<Value> = jv::arr(f.get("hide")).iter().filter(|k| !jv::is_str(Some(k), kind)).cloned().collect();
        if hidden {
            h.push(kind.into());
        }
        cc_m3d::sort::timsort(&mut h, |x, y| kind_index(&js::string_of(Some(x))) - kind_index(&js::string_of(Some(y))));
        jv::set(f, "hide", Value::Array(h));
    }
    tidy_filter(asm);
}

/// `setPairHidden(asm, a, b, hidden)`.
pub fn set_pair_hidden(asm: &mut Value, a: &str, b: &str, hidden: bool) {
    let k = pair_key(a, b);
    if let Some(f) = filter_mut(asm) {
        let mut ps: Vec<Value> = jv::arr(f.get("pairs"))
            .iter()
            .filter(|p| {
                let x = jv::arr(Some(p));
                pair_key(&js::string_of(x.first()), &js::string_of(x.get(1))) != k
            })
            .cloned()
            .collect();
        if hidden {
            ps.push(if js::cmp16(a, b) == std::cmp::Ordering::Less { jv::strs(&[a, b]) } else { jv::strs(&[b, a]) });
        }
        jv::set(f, "pairs", Value::Array(ps));
    }
    tidy_filter(asm);
}

/// `readClashFilter(raw, insts)`.
pub fn read_clash_filter(raw: Option<&Value>, inst_ids: &HashSet<String>) -> Result<Option<Value>, String> {
    let bad = || t("asm.clash.err_filter", &[]);
    let Some(raw) = raw.filter(|r| !r.is_null()) else { return Ok(None) };
    let hide_v = raw.get("hide").filter(|x| !x.is_null());
    let pairs_v = raw.get("pairs").filter(|x| !x.is_null());
    if !raw.is_object() || hide_v.is_some_and(|h| !h.is_array()) || pairs_v.is_some_and(|p| !p.is_array()) || jv::arr(pairs_v).len() > 5000
    {
        return Err(bad());
    }
    let mut hide: Vec<Value> = Vec::new();
    for k in jv::arr(hide_v) {
        if !hide.iter().any(|h| jv::strict_eq(Some(h), Some(k))) {
            hide.push(k.clone());
        }
    }
    if !hide.iter().all(|k| k.as_str().is_some_and(|s| CLASH_KINDS.contains(&s))) {
        return Err(bad());
    }
    let mut pairs = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for pr in jv::arr(pairs_v) {
        let a = jv::as_arr(Some(pr));
        let ok = a.is_some_and(|a| {
            a.len() == 2 && a.iter().all(|x| x.as_str().is_some_and(crate::re::inst_id)) && !jv::strict_eq(a.first(), a.get(1))
        });
        if !ok {
            return Err(bad());
        }
        let x = js::string_of(a.and_then(|a| a.first()));
        let y = js::string_of(a.and_then(|a| a.get(1)));
        let k = pair_key(&x, &y);
        if !inst_ids.contains(&x) || !inst_ids.contains(&y) || seen.contains(&k) {
            continue;
        }
        seen.insert(k);
        pairs.push(if js::cmp16(&x, &y) == std::cmp::Ordering::Less { jv::strs(&[x, y]) } else { jv::strs(&[y, x]) });
    }
    cc_m3d::sort::timsort(&mut hide, |x, y| kind_index(&js::string_of(Some(x))) - kind_index(&js::string_of(Some(y))));
    Ok(if hide.is_empty() && pairs.is_empty() {
        None
    } else {
        Some(jv::obj(vec![("hide", Value::Array(hide)), ("pairs", Value::Array(pairs))]))
    })
}

fn num_ok(v: Option<&Value>) -> bool {
    jv::finite(v).is_some_and(|x| x.abs() < 1e9)
}
fn text(v: Option<&Value>, max: usize) -> String {
    js::slice16(&js::strip_ctrl(&js::str_of(v)), max)
}

/// `readLimits(L, type)`.
pub fn read_limits(l: &Value, ty: &str) -> Result<Option<Value>, String> {
    let dof = joint_dof(ty).unwrap_or(&[]);
    let Some(first) = dof.first() else { return Ok(None) };
    let one = |x: Option<&Value>, key: &str| -> Result<Value, String> {
        let x = x.filter(|x| jv::truthy(Some(x)) && jv::is_obj_like(Some(x)));
        let (mn, mx) = (x.and_then(|x| x.get("min")), x.and_then(|x| x.get("max")));
        if x.is_none() || !num_ok(mn) || !num_ok(mx) || !(jv::as_num(mn).unwrap_or(f64::NAN) < jv::as_num(mx).unwrap_or(f64::NAN)) {
            return Err(t("asm.limits.err_bad", &[]));
        }
        let cap = if value_unit_deg(key) { 3600.0 } else { 1e6 };
        let (a, b) = (jv::as_num(mn).unwrap_or(f64::NAN), jv::as_num(mx).unwrap_or(f64::NAN));
        if a.abs() > cap || b.abs() > cap {
            return Err(t("asm.limits.err_range", &[]));
        }
        Ok(jv::obj(vec![("min", jv::num(a)), ("max", jv::num(b))]))
    };
    if !jv::truthy(Some(l)) || !jv::is_obj_like(Some(l)) {
        return Err(t("asm.limits.err_bad", &[]));
    }
    if l.get("min").is_some_and(Value::is_number) || l.get("max").is_some_and(Value::is_number) {
        return one(Some(l), first).map(Some);
    }
    let mut out = jv::obj(vec![]);
    if let Some(o) = l.as_object() {
        for (k, x) in o {
            if !dof.contains(&k.as_str()) {
                return Err(t("asm.limits.err_name", &[]));
            }
            jv::set(&mut out, k, one(Some(x), k)?);
        }
    } else if let Some(a) = l.as_array() {
        // Object.entries of an array: index keys, never a value name
        if !a.is_empty() {
            return Err(t("asm.limits.err_name", &[]));
        }
    }
    Ok(if out.as_object().is_some_and(|o| !o.is_empty()) { Some(out) } else { None })
}

/// `fileVersion(asm)`.
pub fn file_version(asm: &Value) -> f64 {
    if asm.get("clashFilter").is_some_and(|c| jv::truthy(Some(c))) {
        return 4.0;
    }
    let three = joints(asm).iter().any(|j| {
        NEW_TYPES.contains(&jv::s(j, "type"))
            || j.get("limits").is_some_and(|l| jv::truthy(Some(l)) && !l.get("min").is_some_and(Value::is_number))
            || EXTRA_VALUES.iter().any(|k| j.get(*k).is_some_and(|x| !jv::strict_eq(Some(x), Some(&Value::from(0)))))
    });
    if three { 3.0 } else { 2.0 }
}

/// `toJSON(asm)`.
pub fn to_json(asm: &Value) -> String {
    let mut o = asm.clone();
    jv::set(&mut o, "version", jv::num(file_version(asm)));
    jv::stringify_pretty(&o, 1)
}

/// `readAssembly(raw)`.
pub fn read_assembly(raw: &Value) -> Result<Value, String> {
    let parsed;
    let j = match raw {
        Value::String(s) => {
            parsed = jv::parse(s).ok_or_else(|| t("model.assembly.file.not_json", &[]))?;
            &parsed
        }
        x => x,
    };
    if !jv::truthy(Some(j)) || !jv::is_str(j.get("format"), FORMAT) {
        return Err(t("model.assembly.file.not_asm", &[]));
    }
    let ver = match jv::int(j.get("version")) {
        Some(x) if x >= 1.0 => x,
        _ => return Err(t("model.assembly.file.no_version", &[])),
    };
    if ver > VERSION {
        return Err(t("model.assembly.file.newer", &[("version", F::N(ver)), ("max", F::N(VERSION))]));
    }
    let name = text(j.get("name"), 120);
    let mut asm = new_assembly(if name.is_empty() { "Assembly1" } else { &name });
    let mut ids: HashSet<String> = HashSet::new();
    let mut max_n: f64 = 0.0;
    let mut take = |id: Option<&Value>, prefix: char| -> Result<String, String> {
        let s = id.and_then(Value::as_str).filter(|s| crate::re::prefixed_id(s, prefix) && !ids.contains(*s));
        let Some(s) = s else { return Err(t("model.assembly.file.bad_id", &[])) };
        ids.insert(s.to_string());
        max_n = js::max(max_n, js::str_num(s.get(1..).unwrap_or("")));
        Ok(s.to_string())
    };
    let parts = j.get("parts").filter(|p| jv::truthy(Some(p)) && jv::is_obj_like(Some(p)));
    let pobj = parts.and_then(Value::as_object);
    let plen = parts.map_or(0, |p| p.as_object().map_or_else(|| jv::arr(Some(p)).len(), serde_json::Map::len));
    if parts.is_none() || plen > 200 {
        return Err(t("model.assembly.file.no_parts", &[]));
    }
    if parts.is_some_and(|p| p.as_array().is_some_and(|a| !a.is_empty())) {
        // Object.entries of an array: index keys, never a part id
        return Err(t("model.assembly.file.bad_id", &[]));
    }
    for (pid, p) in pobj.into_iter().flatten() {
        take(Some(&Value::String(pid.clone())), 'p')?;
        let pname = text(p.get("name"), 120);
        let r = part::read_part(p.get("part").unwrap_or(&Value::Null)).and_then(|pt| bom::read_part_props(Some(p)).map(|pr| (pt, pr)));
        let (pt, props) = r.map_err(|e| {
            t(
                "model.assembly.file.part_problem",
                &[("name", F::S(if pname.is_empty() { pid.clone() } else { pname.clone() })), ("message", F::S(e))],
            )
        })?;
        let mut entry =
            jv::obj(vec![("name", (if pname.is_empty() { js::string_of(pt.get("name")) } else { pname }).into()), ("part", pt)]);
        if let Some(o) = props.as_object() {
            for (k, x) in o {
                jv::set(&mut entry, k, x.clone());
            }
        }
        if let Some(ps) = asm.get_mut("parts") {
            jv::set(ps, pid, entry);
        }
    }
    let ins = j.get("instances");
    if !ins.is_some_and(Value::is_array) || jv::arr(ins).len() > 500 {
        return Err(t("model.assembly.file.no_instances", &[]));
    }
    for i in jv::arr(ins) {
        let pid = js::str_of(i.get("part"));
        if !i.get("part").is_some_and(|p| p.as_str().is_some_and(|s| asm.get("parts").and_then(|ps| ps.get(s)).is_some())) {
            return Err(t("model.assembly.file.inst_missing_part", &[]));
        }
        let place_ok = jv::as_arr(i.get("place")).is_some_and(|a| a.len() == 16 && a.iter().all(|x| num_ok(Some(x))));
        if !place_ok {
            return Err(t("model.assembly.file.inst_bad_place", &[]));
        }
        let id = take(i.get("id"), 'i')?;
        let nm = text(i.get("name"), 80);
        let inst = jv::obj(vec![
            ("id", id.as_str().into()),
            ("part", pid.into()),
            ("name", (if nm.is_empty() { id.clone() } else { nm }).into()),
            ("place", Value::Array(jv::arr(i.get("place")).to_vec())),
        ]);
        if let Some(a) = asm.get_mut("instances").and_then(Value::as_array_mut) {
            a.push(inst);
        }
    }
    let js_ = j.get("joints");
    if !js_.is_some_and(Value::is_array) || jv::arr(js_).len() > 1000 {
        return Err(t("model.assembly.file.no_joints", &[]));
    }
    let inst_ids: HashSet<String> = insts(&asm).iter().map(|i| js::str_of(i.get("id"))).collect();
    let empty_ids = HashSet::new();
    let empty_types = HashMap::new();
    let ctx = Ctx { ids: &empty_ids, types: &empty_types, any_id: true };
    let end = |e: Option<&Value>| -> Result<Value, String> {
        let e = e.filter(|e| jv::truthy(Some(e)) && e.get("inst").and_then(Value::as_str).is_some_and(|s| inst_ids.contains(s)));
        let Some(e) = e else { return Err(t("model.assembly.file.joint_missing_part", &[])) };
        let pl = e.get("plane").filter(|p| jv::truthy(Some(p)) && jv::is_obj_like(Some(p)));
        let Some(pl) = pl else { return Err(t("model.assembly.file.joint_no_face", &[])) };
        let plane = readers::read_plane(Some(pl), &ctx, false).map_err(|_| t("model.assembly.file.joint_bad_face", &[]))?;
        let mut out = jv::obj(vec![("inst", e.get("inst").cloned().unwrap_or(Value::Null)), ("plane", plane)]);
        if let Some(ax) = e.get("axis").filter(|a| jv::truthy(Some(a))) {
            let feat_ok = ax.get("feature").map(|f| crate::re::feature_id(&js::string_of(Some(f)))).unwrap_or(false);
            let idx = ax.get("index");
            let idx_ok = idx.is_none() || jv::int(idx).is_some_and(|n| (0.0..10000.0).contains(&n));
            if !feat_ok || !idx_ok {
                return Err(t("model.assembly.file.joint_bad_axis", &[]));
            }
            let mut a2 = jv::obj(vec![("feature", ax.get("feature").cloned().unwrap_or(Value::Null))]);
            if let Some(ix) = idx {
                jv::set(&mut a2, "index", ix.clone());
            }
            jv::set(&mut out, "axis", a2);
        }
        Ok(out)
    };
    for jt in jv::arr(js_) {
        let ty = js::str_of(jt.get("type"));
        if joint_label(&ty).is_none() || !jt.get("type").is_some_and(Value::is_string) {
            return Err(t("model.assembly.file.joint_unknown", &[]));
        }
        let nz = |k: &str| match jt.get(k) {
            None | Some(Value::Null) => true,
            x => num_ok(x),
        };
        if !nz("angle") || !nz("offset") || EXTRA_VALUES.iter().any(|k| jt.get(*k).is_some() && !num_ok(jt.get(*k))) {
            return Err(t("model.assembly.file.joint_bad_value", &[]));
        }
        let a = end(jt.get("a"))?;
        let b = end(jt.get("b"))?;
        if jv::strict_eq(a.get("inst"), b.get("inst")) {
            return Err(t("model.assembly.file.joint_self", &[]));
        }
        let id = take(jt.get("id"), 'j')?;
        let nm = text(jt.get("name"), 60);
        let dflt = |k: &str| match jt.get(k) {
            None | Some(Value::Null) => Value::from(0),
            Some(x) => x.clone(),
        };
        let mut jj = jv::obj(vec![
            ("id", id.as_str().into()),
            ("name", (if nm.is_empty() { id.clone() } else { nm }).into()),
            ("type", ty.as_str().into()),
            ("a", a),
            ("b", b),
            ("angle", dflt("angle")),
            ("offset", dflt("offset")),
        ]);
        for k in EXTRA_VALUES {
            if let Some(x) = jt.get(k) {
                jv::set(&mut jj, k, x.clone());
            }
        }
        if let Some(l) = jt.get("limits").filter(|l| !l.is_null())
            && let Some(lim) = read_limits(l, &ty)?
        {
            jv::set(&mut jj, "limits", lim);
        }
        if let Some(a) = asm.get_mut("joints").and_then(Value::as_array_mut) {
            a.push(jj);
        }
    }
    let views = explode::read_views(j.get("views"), &inst_ids, &mut take)?;
    jv::set(&mut asm, "views", views);
    if let Some(cf) = read_clash_filter(j.get("clashFilter"), &inst_ids)? {
        jv::set(&mut asm, "clashFilter", cf);
    }
    jv::set(&mut asm, "seq", jv::num(max_n + 1.0));
    let fv = file_version(&asm);
    jv::set(&mut asm, "version", jv::num(fv));
    Ok(asm)
}

/// The world map as JSON (insertion order).
pub fn world_json(world: &[(String, M4)]) -> Value {
    let mut o = jv::Obj::new();
    for (k, m) in world {
        o.insert(k.clone(), jv::nums(m));
    }
    Value::Object(o)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_join_place_and_read_back() {
        let pt = part::new_part("Plate");
        let mut a = new_assembly("A");
        let i1 = insert_part(&mut a, Some(&pt), None, None).expect("inserted");
        let pid = js::str_of(i1.get("part"));
        let i2 = insert_part(&mut a, None, Some(&jv::nums(&v::translation([5.0, 0.0, 0.0]))), Some(&pid)).expect("second");
        let plane = jv::obj(vec![("kind", "base".into()), ("name", "top".into())]);
        let ea = jv::obj(vec![("inst", i1.get("id").cloned().unwrap_or(Value::Null)), ("plane", plane.clone())]);
        let eb = jv::obj(vec![("inst", i2.get("id").cloned().unwrap_or(Value::Null)), ("plane", plane)]);
        add_joint(&mut a, "revolute", &ea, &eb, &jv::obj(vec![("angle", 30.into())])).expect("joint");
        assert!(add_joint(&mut a, "weld", &ea, &eb, &Value::Null).is_err());
        let mut builds = Builds::new();
        builds.insert(pid, Build::default());
        let (world, problems) = place_all(&a, &builds);
        assert_eq!(world.len(), 2);
        assert!(problems.is_empty());
        let back = read_assembly(&Value::String(to_json(&a))).expect("reads back");
        assert_eq!(jv::arr(back.get("joints")).len(), 1);
        assert!(read_assembly(&Value::String("{\"format\":\"partcraft-assembly\",\"version\":2,\"parts\":[1]}".into())).is_err());
    }
}
