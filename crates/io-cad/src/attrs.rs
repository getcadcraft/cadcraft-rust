//! attrs.js: attribute definitions (`attdef`) and turning the `_attribs` the readers hang on references into
//! `attrs` / `attloc` (`resolveAttribs`), plus the two geom.js pieces it needs on dynamic values (`insertMatrix`,
//! `txEntity` for a text).

use cc_cad::js;
use cc_cad_geom::geom::{M, m_apply, m_det, m_inverse, m_mul, m_rotate, m_scale, m_similar, m_translate, norm_angle};
use cc_cad_geom::jsmath::JsMath;
use cc_geom::P2;
use cc_io_dwg::value::{Obj, V};

use crate::jsv::{self, Ov};
use crate::text::one_line;

/// geom.js `normTag(t)` (`String(t ?? '')` first).
pub fn norm_tag(t: Ov) -> String {
    cc_cad_geom::geom::norm_tag(&jsv::str_nn(t))
}

/// Options of `attdef`.
#[derive(Default)]
pub struct AttdefOpts {
    pub tag: V,
    pub prompt: String,
    pub def: String,
    pub invisible: bool,
    pub constant: bool,
    pub preset: bool,
}

/// `attdef(t, opts)`: an attribute definition from a text-shaped entity (block coordinates).
pub fn attdef(t: &Obj, o: AttdefOpts) -> Obj {
    let g = |k: &str| t.get(k);
    let mut d = Obj::new();
    d.set("tag", norm_tag(Some(&o.tag)));
    d.set("prompt", o.prompt);
    d.set("def", one_line(&o.def));
    d.set("x", jsv::pn_or(g("x"), 0.0));
    d.set("y", jsv::pn_or(g("y"), 0.0));
    d.set("h", if jsv::to_num(g("h")) > 0.0 { jsv::to_num(g("h")) } else { 2.5 });
    d.set("rot", jsv::orv(g("rot"), V::Num(0.0)));
    d.set("ha", jsv::orv(g("ha"), V::from("left")));
    d.set("va", jsv::orv(g("va"), V::from("baseline")));
    d.set("wf", jsv::orv(g("wf"), V::Num(1.0)));
    d.set("layer", jsv::nnv(g("layer"), V::from("0")));
    d.set("color", jsv::nnv(g("color"), V::Null));
    if o.invisible {
        d.set("invisible", true);
    }
    if o.constant {
        d.set("constant", true);
    }
    if o.preset {
        d.set("preset", true);
    }
    d
}

/// geom.js `insertMatrix(e, blk)` on dynamic values.
pub fn insert_matrix(e: &Obj, blk: &Obj) -> M {
    let b = blk.get("base").filter(|b| b.truthy());
    let (bx, by) = match b {
        Some(b) => (jsv::to_num(jsv::get(Some(b), "x")), jsv::to_num(jsv::get(Some(b), "y"))),
        None => (0.0, 0.0),
    };
    let x = jsv::to_num(e.get("x"));
    let y = jsv::to_num(e.get("y"));
    let rot = jsv::or(e.get("rot"), 0.0);
    let sx = jsv::nn(e.get("sx"), 1.0);
    let sy = jsv::nn(e.get("sy"), 1.0);
    m_mul(&m_translate(x, y), &m_mul(&m_rotate(rot), &m_mul(&m_scale(sx, sy), &m_translate(-bx, -by))))
}

/// geom.js `txEntity(text, m)` → (x, y, h, rot) of the moved text.
pub fn tx_text(e: &Obj, m: &M) -> (f64, f64, f64, f64) {
    let a = m_apply(m, P2::new(jsv::to_num(e.get("x")), jsv::to_num(e.get("y"))));
    let (s, srot, flip) = match m_similar(m) {
        Some(sim) => (sim.s, sim.rot, sim.flip),
        None => (m_det(m).abs().sqrt(), m[1].jatan2(m[0]), m_det(m) < 0.0),
    };
    let er = jsv::or(e.get("rot"), 0.0);
    let mut rot = er + srot;
    if flip {
        rot = m[1].jatan2(m[0]) - er;
        if rot.jcos() < -1e-9 {
            rot += std::f64::consts::PI;
        }
    }
    (a.x, a.y, jsv::to_num(e.get("h")) * s, norm_angle(rot))
}

fn same_spot(t: (f64, f64, f64, f64), a: &Obj, d: &Obj, invisible: bool) -> bool {
    let (tx, ty, th, trot) = t;
    let dn = |k: &str| jsv::to_num(d.get(k));
    let tol = 1e-6 * js::max_of([1.0, jsv::or(d.get("h"), 1.0).abs(), dn("x").abs(), dn("y").abs()]);
    let da = norm_angle(js::or(Some(trot), 0.0) - jsv::or(d.get("rot"), 0.0)).abs();
    let s_or = |v: Ov, def: &str| jsv::orv(v, V::from(def));
    (tx - dn("x")).abs() <= tol
        && (ty - dn("y")).abs() <= tol
        && (th - dn("h")).abs() <= tol * 10.0
        && js::min(da, 2.0 * std::f64::consts::PI - da) < 1e-6
        && crate::props::strict_eq(Some(&s_or(a.get("ha"), "left")), Some(&s_or(d.get("ha"), "left")))
        && crate::props::strict_eq(Some(&s_or(a.get("va"), "baseline")), Some(&s_or(d.get("va"), "baseline")))
        && invisible == jsv::truthy(d.get("invisible"))
        && crate::props::strict_eq(Some(&jsv::nnv(a.get("layer"), V::from("0"))), Some(&jsv::nnv(d.get("layer"), V::from("0"))))
        && crate::props::strict_eq(Some(&jsv::nnv(a.get("color"), V::Null)), Some(&jsv::nnv(d.get("color"), V::Null)))
}

/// Blocks by name as resolveAttribs reads them (`{ base, attdefs }` only; a later block of the same name wins).
pub type BlockInfo = std::collections::HashMap<String, Obj>;

/// The `{ base, attdefs }` of a block for [`BlockInfo`].
pub fn block_info(b: &Obj) -> Obj {
    let mut o = Obj::new();
    if let Some(x) = b.get("base") {
        o.set("base", x.clone());
    }
    if let Some(x) = b.get("attdefs") {
        o.set("attdefs", x.clone());
    }
    o
}

/// `resolveAttribs(lists, blocks)`: turn each reference's `_attribs` into attrs (+ attloc where an attribute is not
/// where its definition puts it). `block(name)` finds a block.
pub fn resolve_attribs(lists: &mut [&mut Vec<V>], blocks: &BlockInfo) {
    for list in lists.iter_mut() {
        for item in list.iter_mut() {
            let V::Obj(e) = item else { continue };
            if jsv::s(e.get("type")) != Some("insert") || !e.get("_attribs").is_some_and(V::truthy) {
                continue;
            }
            let Some(raw) = e.remove("_attribs") else { continue };
            let raw = match raw {
                V::Arr(a) => a,
                _ => continue,
            };
            if raw.is_empty() {
                continue;
            }
            let blk = jsv::s(e.get("block")).and_then(|n| blocks.get(n));
            let inv = blk.as_ref().and_then(|b| m_inverse(&insert_matrix(e, b)));
            let mut attrs = Obj::new();
            let mut attloc = Obj::new();
            for a in &raw {
                let V::Obj(a) = a else { continue };
                let tag = norm_tag(a.get("tag"));
                if tag.is_empty() || attrs.has(&tag) {
                    continue;
                }
                attrs.set(tag.clone(), one_line(&jsv::to_str(a.get("s"))));
                let local = inv.as_ref().map(|m| tx_text(a, m));
                let d = blk.and_then(|b| {
                    jsv::arr(b.get("attdefs")).iter().find_map(|x| match x {
                        V::Obj(o) if jsv::s(o.get("tag")) == Some(tag.as_str()) => Some(o),
                        _ => None,
                    })
                });
                if let (Some(l), Some(d)) = (local, d)
                    && same_spot(l, a, d, jsv::truthy(a.get("invisible")))
                {
                    continue;
                }
                if let Some((x, y, h, rot)) = local {
                    let mut o = Obj::new();
                    o.set("x", x);
                    o.set("y", y);
                    o.set("h", h);
                    o.set("rot", js::or(Some(rot), 0.0));
                    o.set("ha", jsv::orv(a.get("ha"), V::from("left")));
                    o.set("va", jsv::orv(a.get("va"), V::from("baseline")));
                    o.set("wf", jsv::orv(a.get("wf"), V::Num(1.0)));
                    o.set("layer", jsv::nnv(a.get("layer"), V::from("0")));
                    o.set("color", jsv::nnv(a.get("color"), V::Null));
                    if jsv::truthy(a.get("invisible")) {
                        o.set("invisible", true);
                    }
                    attloc.set(tag, o);
                }
            }
            e.set("attrs", attrs);
            if !attloc.is_empty() {
                e.set("attloc", attloc);
            }
        }
    }
}
