//! Helpers io.js shares between its formats: names (`cleanName`), paint as exports group it (`xp`), the triangles of
//! a definition (`defTris`) and of everything inside it (`flatTris`).

use cc_m3d::Model;
use cc_m3d::def::{Def, face_normal, face_tris};
use cc_m3d::edges::smooth_normals;
use cc_m3d::js;
use cc_m3d::paint::{is_image_paint, paint_hex as kernel_paint_hex};
use cc_m3d::vec::{M4, P3, apply, cross, det3, len, norm, sub};

/// Most vertices / faces / line pieces a reader brings in (`MAX_VERTS`).
pub const MAX_VERTS: usize = 2_000_000;

/// `cleanName(s)`: control characters → spaces, trimmed, at most 120 UTF-16 units; empty → "Import".
pub fn clean_name(s: &str) -> String {
    let base = if s.is_empty() { "Import" } else { s };
    let c = js::clean_text(base, 120);
    if c.is_empty() { "Import".into() } else { c }
}

/// The material library's colours, as the page registers them (model.js `setMaterialColors`): ModelCraft's app sets
/// this once (its library lives in the UI); the CLI and the tests leave it unset, as io.js runs under Node.
static MATERIAL_COLORS: std::sync::OnceLock<fn(&str) -> Option<&'static str>> = std::sync::OnceLock::new();

/// `setMaterialColors(fn)`: library materials export, print and show as their colour (0.5 RC.7). The first call wins.
pub fn set_material_colors(f: fn(&str) -> Option<&'static str>) {
    let _ = MATERIAL_COLORS.set(f);
}

/// `paintHex(c)`: a library material as its registered colour (none registered: the default), no image colours.
pub fn paint_hex(c: &str) -> String {
    let mat = MATERIAL_COLORS.get().copied();
    kernel_paint_hex(c, &|id| mat.and_then(|f| f(id)).map(str::to_string), &|_| None)
}

/// `xp(c)`: an imported texture exports as its colour, so faces share a material.
pub fn xp(c: &str) -> String {
    if is_image_paint(c) { paint_hex(c) } else { c.to_string() }
}

/// `c || d` for paint strings (an empty string is falsy).
#[inline]
pub fn or_paint<'a>(c: Option<&'a str>, d: Option<&'a str>) -> Option<&'a str> {
    c.filter(|x| !x.is_empty()).or(d.filter(|x| !x.is_empty()))
}

/// `xp(f.color || dflt) || null`.
pub fn face_paint(c: Option<&str>, dflt: Option<&str>) -> Option<String> {
    or_paint(c, dflt).map(xp).filter(|x| !x.is_empty())
}

/// A triangle of `defTris`: corners, flat normal, colour, corner normals where soft edges join faces.
#[derive(Clone, Debug)]
pub struct DefTri {
    pub p: [P3; 3],
    pub n: P3,
    pub color: Option<String>,
    pub ns: Option<[P3; 3]>,
}

/// `defTris(def, dflt)`: the definition's own faces as triangles with flat normals (and smooth corner normals).
pub fn def_tris(def: &Def, dflt: Option<&str>) -> Vec<DefTri> {
    let mut out = Vec::new();
    let smooth = smooth_normals(def);
    for (fid, f) in def.faces.iter() {
        let n = face_normal(def, f);
        if !js::truthy(len(n)) {
            continue;
        }
        let color = face_paint(f.color.as_deref(), dflt);
        let fkey = if smooth.is_empty() {
            String::new()
        } else {
            let mut k = js::num_str(fid);
            k.push('|');
            k
        };
        for t in face_tris(def, f) {
            let mut tri = DefTri { p: [def.v(t[0]), def.v(t[1]), def.v(t[2])], n, color: color.clone(), ns: None };
            if !smooth.is_empty() {
                let look = |v: f64| {
                    let mut k = fkey.clone();
                    js::push_num(&mut k, v);
                    smooth.get(&k).copied()
                };
                let got = [look(t[0]), look(t[1]), look(t[2])];
                if got.iter().any(Option::is_some) {
                    tri.ns = Some([got[0].unwrap_or(n), got[1].unwrap_or(n), got[2].unwrap_or(n)]);
                }
            }
            out.push(tri);
        }
    }
    out
}

/// A triangle of `flatTris`.
#[derive(Clone, Debug)]
pub struct FlatTri {
    pub p: [P3; 3],
    pub n: P3,
    pub color: Option<String>,
}

/// `flatTris(model, def, m0)`: triangles of a definition and everything inside it, in the definition's coordinates.
pub fn flat_tris(model: &Model, def: &str, m0: &M4) -> Vec<FlatTri> {
    let mut out = Vec::new();
    model.walk(def, m0, &mut |d, wm, _path, pc| {
        let mirror = det3(wm) < 0.0;
        for t in def_tris(d, pc) {
            let mut ps = [apply(wm, t.p[0]), apply(wm, t.p[1]), apply(wm, t.p[2])];
            if mirror {
                ps = [ps[0], ps[2], ps[1]];
            }
            out.push(FlatTri { p: ps, n: norm(cross(sub(ps[1], ps[0]), sub(ps[2], ps[0]))), color: t.color });
        }
        true
    });
    out
}

/// `p.slice(0, n)` of a string in UTF-16 units (a pair cut in half is dropped whole: Spec gap, JS-engine quirk).
#[inline]
pub fn slice16(s: &str, n: usize) -> String {
    js::slice16(s, n)
}

/// The methods every JS object inherits: `skipped[k]` reads one of them for a key no entry has yet.
const PROTO: [&str; 11] = [
    "constructor",
    "hasOwnProperty",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toLocaleString",
    "toString",
    "valueOf",
    "__defineGetter__",
    "__defineSetter__",
    "__lookupGetter__",
    "__lookupSetter__",
];

/// Count `k` once more in a report's `skipped` map (`skipped[k] = (skipped[k] || 0) + n`). Keys come from files (a
/// DXF entity type), so the JS object's quirks are kept: `__proto__` is never stored, and an inherited method's name
/// starts from the method (a function + n is text: "function toString() { [native code] }1").
pub fn skip(skipped: &mut serde_json::Map<String, serde_json::Value>, k: &str, n: f64) {
    use serde_json::Value;
    if k == "__proto__" {
        return;
    }
    let next = match skipped.get(k) {
        Some(Value::String(s)) => Value::String(format!("{s}{}", js::num_str(n))),
        Some(v) => Value::from(v.as_f64().filter(|x| *x != 0.0).unwrap_or(0.0) + n),
        None if PROTO.contains(&k) => Value::String(format!("function {k}() {{ [native code] }}{}", js::num_str(n))),
        None => Value::from(n),
    };
    skipped.insert(k.to_string(), next);
}
