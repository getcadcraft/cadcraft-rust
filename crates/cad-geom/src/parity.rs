//! The parity contract with the frozen JS (`forks/tools/gdiff.mjs`): run a case `{ fn, args }` by the JS
//! function's name with JSON arguments and give its result as JSON, exactly as the harness writes the JS result
//! (`$NaN` / `$Inf` for non-finite numbers, absent keys for undefined). `cadcraft-cli cadgeom <cases.json>` runs a
//! file of them.

use cc_cad::entity::{Color, Entity, Obj, num, pt};
use cc_cad::i18n::Lookup;
use cc_cad::{Block, Doc, js};
use cc_geom::P2;
use serde_json::{Value, json};

use crate::corners::{ChamferSpec, CornerSpec, Current, Target, chamfer_pair, corner_target, explode_entity, fillet_pair, poly_corners};
use crate::dims::*;
use crate::dimstyle::{Base, arrow_parts, dim_style_text, dim_value};
use crate::edit::*;
use crate::geom::*;
use crate::pick::*;
use crate::tx::*;
use crate::walk::{Recorder, WalkOpts, frozen_key, frozen_set, mask_loop, poly_bands, walk};

fn g<'a>(a: &'a Value, k: &str) -> Option<&'a Value> {
    a.get(k).filter(|v| !v.is_null())
}
fn f_(a: &Value, k: &str) -> f64 {
    a.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN)
}
fn b_(a: &Value, k: &str) -> bool {
    js::truthy(a.get(k))
}
fn pt_(a: &Value, k: &str) -> P2 {
    cc_cad::entity::pt_of(a.get(k)).unwrap_or(P2::new(f64::NAN, f64::NAN))
}
fn ent(a: &Value, k: &str) -> Entity {
    Entity::from_value(a.get(k).unwrap_or(&Value::Null))
}
fn ents(a: &Value, k: &str) -> Vec<Entity> {
    a.get(k).and_then(Value::as_array).map(|l| l.iter().map(Entity::from_value).collect()).unwrap_or_default()
}
fn pts(a: &Value, k: &str) -> Vec<P2> {
    a.get(k)
        .and_then(Value::as_array)
        .map(|l| l.iter().map(|q| cc_cad::entity::pt_of(Some(q)).unwrap_or(P2::new(f64::NAN, f64::NAN))).collect())
        .unwrap_or_default()
}
fn blocks(a: &Value) -> Option<Blocks> {
    let l = g(a, "blocks")?.as_array()?;
    Some(
        l.iter()
            .map(|b| {
                let blk = Block::from_value(b);
                (blk.name.clone(), blk)
            })
            .collect(),
    )
}
fn m6(v: Option<&Value>) -> M {
    let mut m = [f64::NAN; 6];
    if let Some(Value::Array(a)) = v {
        for (i, x) in a.iter().take(6).enumerate() {
            if let Some(slot) = m.get_mut(i) {
                *slot = x.as_f64().unwrap_or(f64::NAN);
            }
        }
    }
    m
}
fn mv(m: &M) -> Value {
    Value::Array(m.iter().map(|x| num(*x)).collect())
}
fn prim(a: &Value, k: &str) -> Prim {
    a.get(k).and_then(Prim::from_value).unwrap_or(Prim::l(f64::NAN, f64::NAN, f64::NAN, f64::NAN))
}
fn prims_v(l: &[Prim]) -> Value {
    Value::Array(l.iter().map(Prim::to_value).collect())
}
fn ents_v(l: &[Entity]) -> Value {
    Value::Array(l.iter().map(Entity::to_value).collect())
}
fn opt_ent(e: Option<Entity>) -> Value {
    e.map_or(Value::Null, |e| e.to_value())
}
fn pts_v(l: &[P2]) -> Value {
    Value::Array(l.iter().map(|q| pt(*q)).collect())
}
fn idx(a: &Value, k: &str) -> usize {
    a.get(k).and_then(Value::as_u64).and_then(|v| usize::try_from(v).ok()).unwrap_or(0)
}
fn circ(v: Option<&Value>) -> Circ {
    let f = |k: &str| v.and_then(|c| c.get(k)).and_then(Value::as_f64).unwrap_or(f64::NAN);
    Circ { cx: f("cx"), cy: f("cy"), r: f("r") }
}
fn doc_of(a: &Value) -> Doc {
    let mut d = Doc::new();
    if let Some(input) = a.get("doc") {
        d.load(input);
    }
    d
}
fn color_arg(v: Option<&Value>) -> Option<Color> {
    v.and_then(Color::from_value).filter(|c| !matches!(c, Color::Null))
}
fn ids_v(l: &[Option<u64>]) -> Value {
    Value::Array(l.iter().map(|i| i.map_or(Value::Null, Value::from)).collect())
}
fn current_arg(a: &Value) -> Option<Current> {
    let c = g(a, "current")?;
    Some(Current {
        layer: c.get("layer").and_then(Value::as_str).map(str::to_string),
        color: c.get("color").and_then(Color::from_value).filter(|c| !matches!(c, Color::Null)),
    })
}

/// One case → its result.
pub fn run_case(name: &str, a: &Value, lookup: Lookup) -> Value {
    let bl = blocks(a);
    let blk = bl.as_ref();
    match name {
        "basics" => {
            let (pa, pb, ang, d) = (pt_(a, "a"), pt_(a, "b"), f_(a, "ang"), f_(a, "d"));
            json!({ "dist": num(dist(pa, pb)), "angleOf": num(angle_of(pa, pb)), "polar": pt(polar(pa, ang, d)), "normAngle": num(norm_angle(ang)),
                "deg": num(deg(ang)), "rad": num(rad(ang)), "sweepToBulge": num(sweep_to_bulge(ang)) })
        }
        "onSweep" => Value::Bool(on_sweep(f_(a, "a"), f_(a, "a0"), f_(a, "a1"), a.get("tol").and_then(Value::as_f64).unwrap_or(1e-9))),
        "bulgeArc" => bulge_arc(pt_(a, "p"), pt_(a, "q"), f_(a, "b")).to_value(),
        "arcRange" => {
            let (x, y) = arc_range(f_(a, "a0"), f_(a, "a1"));
            json!([num(x), num(y)])
        }
        "matrix" => {
            let (m, n) = (m6(a.get("m")), m6(a.get("n")));
            let sim = m_similar(&m).map_or(Value::Null, |s| json!({ "s": num(s.s), "rot": num(s.rot), "flip": s.flip }));
            json!({
                "mul": mv(&m_mul(&m, &n)), "apply": pt(m_apply(&m, pt_(a, "p"))), "about": mv(&m_about(pt_(a, "p"), &m)),
                "mirror": mv(&m_mirror(pt_(a, "p"), pt_(a, "q"))), "det": num(m_det(&m)), "similar": sim,
                "inverse": m_inverse(&m).map_or(Value::Null, |x| mv(&x)), "rotate": mv(&m_rotate(f_(a, "r"))),
                "scale": mv(&m_scale(f_(a, "sx"), f_(a, "sy"))), "translate": mv(&m_translate(f_(a, "sx"), f_(a, "sy"))),
            })
        }
        "unitDir" => unit_dir(f_(a, "dx"), f_(a, "dy")).map_or(Value::Null, |(x, y)| json!({ "dx": num(x), "dy": num(y) })),
        "clipInfinite" => {
            let r = Bounds::from_value(a.get("rect").unwrap_or(&Value::Null));
            clip_infinite(pt_(a, "p"), pt_(a, "d"), &r, b_(a, "ray")).map_or(Value::Null, |(x, y)| json!([num(x), num(y)]))
        }
        "viewportMatrix" => {
            let e = ent(a, "e");
            match &e.geo {
                cc_cad::Geo::Viewport(v) => mv(&viewport_matrix(v.x, v.y, v.cx, v.cy, v.s, v.tw)),
                _ => Value::Null,
            }
        }
        "prims" => prims_v(&prims(&ent(a, "e"), blk)),
        "polyPrims" => prims_v(&poly_prims_of(&ent(a, "e"))),
        "ellipsePoints" => pts_v(&ellipse_points(&ent(a, "e"), a.get("steps").and_then(Value::as_f64).unwrap_or(72.0))),
        "text" => {
            let e = ent(a, "e");
            match &e.geo {
                cc_cad::Geo::Text(t) => json!({ "layout": text_layout(t).to_value(), "corners": pts_v(&text_corners(t)), "width": num(text_width(&t.s, t)),
                    "mask": Value::Array(mask_loop(&e, a.get("scale").and_then(Value::as_f64).unwrap_or(1.5)).iter().map(|v| num(*v)).collect()) }),
                _ => Value::Null,
            }
        }
        "insert" => {
            let e = ent(a, "e");
            let cc_cad::Geo::Insert(i) = &e.geo else { return Value::Null };
            let b = blk.and_then(|m| m.get(&i.block));
            json!({
                "matrix": b.map_or(Value::Null, |b| mv(&insert_matrix(i, b))),
                "attrs": ents_v(&attr_texts(i, b, false)),
                "all": ents_v(&attr_texts(i, b, true)),
            })
        }
        "txPrim" => prims_v(&tx_prim(&prim(a, "p"), &m6(a.get("m")))),
        "primOps" => {
            let q = prim(a, "p");
            let x = pt_(a, "q");
            json!({ "point": pt(prim_point(&q, f_(a, "t"))), "param": num(prim_param(&q, x, true)), "paramX": num(prim_param(&q, x, false)),
                "dist": num(dist_to_prim(&q, x)), "length": num(prim_length(&q)) })
        }
        "intersect" => pts_v(&intersect(&prim(a, "p"), &prim(a, "q"), b_(a, "ext1"), b_(a, "ext2"))),
        "normTag" => Value::String(norm_tag(&a.get("s").map(js::to_js_string).unwrap_or_default())),
        "primBounds" => {
            let mut b = empty_bounds();
            prim_bounds(&prim(a, "p"), &mut b);
            b.to_value()
        }
        "entityBounds" => entity_bounds(&ent(a, "e"), blk).to_value(),
        "distToEntity" => num(dist_to_entity(&ent(a, "e"), pt_(a, "q"), blk)),
        "pointInPoly" => Value::Bool(point_in_poly(pt_(a, "q"), &pts(a, "pts"))),
        "inWindow" => {
            let w = Bounds::from_value(a.get("w").unwrap_or(&Value::Null));
            Value::Bool(in_window(&ent(a, "e"), &w, b_(a, "crossing"), blk))
        }
        "fenceHits" => ids_v(&fence_hits(&ents(a, "ents"), &pts(a, "pts"), blk)),
        "snapPoints" => {
            let kinds: Vec<String> = a.get("kinds").and_then(Value::as_array).map(|l| l.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
            let kr: Vec<&str> = kinds.iter().map(String::as_str).collect();
            let base = g(a, "base").and_then(|v| cc_cad::entity::pt_of(Some(v)));
            Value::Array(snap_points(&ent(a, "e"), pt_(a, "q"), f_(a, "r"), &kr, blk, base).iter().map(Snap::to_value).collect())
        }
        "tangents" => {
            let c = circ(a.get("c"));
            let c2 = circ(a.get("c2"));
            let from = pts_v(&tangent_from(pt_(a, "p"), &c));
            let common: Vec<Value> = common_tangents(&c, &c2).iter().map(|[x, y]| json!([pt(*x), pt(*y)])).collect();
            let res = |o: TanOther| match resolve_tangent(&c, pt_(a, "near"), o) {
                None => Value::Null,
                Some(v) if v.len() == 1 => v.first().map_or(Value::Null, |q| pt(*q)),
                Some(v) => pts_v(&v),
            };
            json!({ "from": from, "common": common, "toPoint": res(TanOther::Point(pt_(a, "p"))), "toTan": res(TanOther::Tan(c2, pt_(a, "near2"))) })
        }
        "txEntity" => tx_entity(&ent(a, "e"), &m6(a.get("m"))).to_value(),
        "grips" => {
            let e = ent(a, "e");
            let gs = grips(&e);
            let to = pt_(a, "to");
            let moved: Vec<Value> = gs.iter().map(|gr| grip_move(&e, &gr.key, to).to_value()).collect();
            json!({ "grips": gs.iter().map(Grip::to_value).collect::<Vec<_>>(), "moved": moved })
        }
        "offsetEntity" => opt_ent(offset_entity(&ent(a, "e"), f_(a, "d"), pt_(a, "side"))),
        "trimEntity" => {
            let list = ents(a, "ents");
            let Some(e) = list.get(idx(a, "ei")) else { return Value::Null };
            trim_entity(e, &list, pt_(a, "pick"), blk).map_or(Value::Null, |l| ents_v(&l))
        }
        "subEntity" => opt_ent(sub_entity(&ent(a, "e"), f_(a, "t0"), f_(a, "t1"))),
        "trimSplit" => {
            let list = ents(a, "ents");
            let Some(e) = list.get(idx(a, "ei")) else { return Value::Null };
            let Some(sp) = trim_split(e, &list, blk, b_(a, "edgeExtend")) else { return Value::Null };
            let at: Vec<Value> = pts(a, "picks").iter().map(|q| Value::from(piece_at(&sp, *q))).collect();
            let pcs: Vec<Value> = (0..sp.pieces.len()).map(|i| opt_ent(piece_entity(&sp, i))).collect();
            let gones: Vec<Vec<usize>> = a
                .get("gones")
                .and_then(Value::as_array)
                .map(|l| l.iter().map(|x| x.as_array().map(|y| y.iter().filter_map(Value::as_u64).filter_map(|u| usize::try_from(u).ok()).collect()).unwrap_or_default()).collect())
                .unwrap_or_default();
            let res: Vec<Value> = gones.iter().map(|gs| trim_result(&sp, gs).map_or(Value::Null, |l| ents_v(&l))).collect();
            json!({ "split": sp.to_value(), "at": at, "pieces": pcs, "results": res })
        }
        "pathCrossings" => pts_v(&path_crossings(&ent(a, "e"), &pts(a, "pts"), blk)),
        "extendEntity" => {
            let list = ents(a, "ents");
            let Some(e) = list.get(idx(a, "ei")) else { return Value::Null };
            let r = extend_entity(e, &list, pt_(a, "pick"), blk, b_(a, "edgeExtend"));
            let part = r.as_ref().and_then(|n| extension_part(e, n));
            json!({ "e": opt_ent(r), "part": opt_ent(part) })
        }
        "filletLines" => fillet_lines(&ent(a, "a"), &ent(a, "b"), f_(a, "r"), pt_(a, "pa"), pt_(a, "pb")).map_or(Value::Null, |x| x.to_value()),
        "pathOf" => path_of(&ent(a, "e")).map_or(Value::Null, |(p, c)| json!({ "pts": pts_v(&p), "closed": c })),
        "resamplePath" => pts_v(&resample_path(&pts(a, "pts"), b_(a, "closed"), f_(a, "step"))),
        "revcloud" => revcloud(&pts(a, "pts"), b_(a, "closed"), f_(a, "arcLen"), b_(a, "reverse")).to_value(),
        "paramOn" => num(param_on(&ent(a, "e"), pt_(a, "q"))),
        "breakEntity" => match break_entity(&ent(a, "e"), pt_(a, "p1"), pt_(a, "p2")) {
            Ok(l) => ents_v(&l),
            Err(e) => json!({ "error": e }),
        },
        "curveLength" => curve_length(&ent(a, "e")).map_or(Value::Null, num),
        "lengthenEntity" => {
            let d_angle = g(a, "dAngle").and_then(Value::as_f64);
            match lengthen_entity(&ent(a, "e"), pt_(a, "pick"), a.get("delta").and_then(Value::as_f64).unwrap_or(0.0), d_angle) {
                Ok(e) => e.to_value(),
                Err(e) => json!({ "error": e }),
            }
        }
        "joinEntities" => {
            let (res, left) = join_entities(&ents(a, "ents"), g(a, "tol").and_then(Value::as_f64));
            json!({ "results": res.iter().map(|r| json!({ "from": ids_v(&r.from), "to": r.to.to_value() })).collect::<Vec<_>>(), "left": ids_v(&left) })
        }
        "stretchEntity" => {
            let w = Bounds::from_value(a.get("w").unwrap_or(&Value::Null));
            let inside = |q: P2| q.x >= w.min_x && q.x <= w.max_x && q.y >= w.min_y && q.y <= w.max_y;
            opt_ent(stretch_entity(&ent(a, "e"), &inside, f_(a, "dx"), f_(a, "dy")))
        }
        "polygonMeasure" => {
            let (ar, pe) = polygon_measure(&pts(a, "pts"));
            json!({ "area": num(ar), "perimeter": num(pe) })
        }
        "entityArea" => entity_area(&ent(a, "e")).map_or(Value::Null, |(ar, pe, k)| json!({ "area": num(ar), "perimeter": num(pe), "kind": k })),
        "cornerTarget" => {
            let e = ent(a, "e");
            corner_target(&e, pt_(a, "pick")).map_or(Value::Null, |t| json!({ "seg": t.seg, "prim": t.prim.to_value(), "pick": pt(t.pick) }))
        }
        "filletPair" | "chamferPair" => {
            let list = ents(a, "ents");
            let tgt = |k: &str| -> Option<Target<'_>> {
                let t = g(a, k)?;
                let i = t.get("i").and_then(Value::as_u64).and_then(|v| usize::try_from(v).ok())?;
                corner_target(list.get(i)?, cc_cad::entity::pt_of(t.get("pick"))?)
            };
            let (ta, tb) = (tgt("a"), tgt("b"));
            let trim = a.get("trim").is_none_or(|v| js::truthy(Some(v)));
            let cur = current_arg(a);
            if name == "filletPair" {
                fillet_pair(ta.as_ref(), tb.as_ref(), f_(a, "r"), trim, cur.as_ref()).to_value()
            } else {
                chamfer_pair(ta.as_ref(), tb.as_ref(), &ChamferSpec::from_value(a.get("spec").unwrap_or(&Value::Null)), trim, cur.as_ref()).to_value()
            }
        }
        "polyCorners" => {
            let spec = if a.get("kind").and_then(Value::as_str) == Some("fillet") {
                CornerSpec::Fillet(f_(a, "spec"))
            } else {
                CornerSpec::Chamfer(ChamferSpec::from_value(a.get("spec").unwrap_or(&Value::Null)))
            };
            poly_corners(&ent(a, "e"), &spec).to_value()
        }
        "explodeEntity" => explode_entity(&ent(a, "e"), blk).map_or(Value::Null, |l| ents_v(&l)),
        "dim" => {
            let e = ent(a, "e");
            let m = dim_measure(&e);
            let (main, tol) = dim_value(&e, m);
            let sweep = match &e.geo {
                cc_cad::Geo::Dim(d) if d.kind() == "angular" => {
                    let (s, en, sw) = angular_sweep(d);
                    json!({ "s": num(s), "e": num(en), "sweep": num(sw) })
                }
                _ => Value::Null,
            };
            json!({ "measure": num(m), "format": format_measure(&e, None), "text": dim_text(&e), "geometry": ents_v(&dim_geometry(&e)),
                "textPoint": dim_text_point(&e).map_or(Value::Null, pt), "styleText": dim_style_text(&e, m),
                "value": { "main": main, "tol": tol.map_or(Value::Null, |t| t.to_value()) }, "sweep": sweep })
        }
        "readable" => num(readable(f_(a, "a"))),
        "angularFromLines" => {
            let l = |k: &str| {
                let v = a.get(k);
                let f = |n: &str| v.and_then(|x| x.get(n)).and_then(Value::as_f64).unwrap_or(f64::NAN);
                [f("x1"), f("y1"), f("x2"), f("y2")]
            };
            angular_from_lines(l("l1"), l("l2"), pt_(a, "loc")).map_or(Value::Null, |(c, p1, p2)| json!({ "c": pt(c), "p1": pt(p1), "p2": pt(p2) }))
        }
        "arrowParts" => {
            let b = a.get("base");
            let base = Base {
                layer: b.and_then(|x| x.get("layer")).and_then(Value::as_str).map(str::to_string),
                color: b.and_then(|x| x.get("color")).and_then(Color::from_value),
                role: b.and_then(|x| x.get("role")).and_then(Value::as_str).map(str::to_string),
            };
            ents_v(&arrow_parts(a.get("kind").and_then(Value::as_str).unwrap_or(""), pt_(a, "tip"), f_(a, "ang"), f_(a, "size"), &base))
        }
        "formatLength" => {
            let lu = match a.get("lunits") {
                None => 2.0,
                Some(v) => js::to_number(v),
            };
            Value::String(crate::units::format_length(f_(a, "v"), lu, a.get("prec")))
        }
        "formatArea" => {
            let lu = match a.get("lunits") {
                None => 2.0,
                Some(v) => js::to_number(v),
            };
            Value::String(crate::units::format_area(f_(a, "a"), lu, a.get("prec"), lookup))
        }
        "parseLength" => {
            let lu = match a.get("lunits") {
                None => 2.0,
                Some(v) => js::to_number(v),
            };
            crate::units::parse_length(&a.get("s").map(|v| if v.is_null() { String::new() } else { js::to_js_string(v) }).unwrap_or_default(), lu).map_or(Value::Null, num)
        }
        "unitsOf" => {
            let (l, p) = crate::units::units_of(a.get("meta").and_then(Value::as_object));
            json!({ "lunits": l, "prec": p })
        }
        "advance" => num(crate::font::advance(a.get("s").and_then(Value::as_str).unwrap_or(""))),
        "leader" => {
            let e = ent(a, "e");
            json!({ "geometry": ents_v(&crate::leader::leader_geometry(&e)),
                "landing": crate::leader::landing(&e).map_or(Value::Null, |(s, en, t)| json!({ "start": pt(s), "end": pt(en), "text": pt(t) })) })
        }
        "hatch" => {
            let d = doc_of(a);
            let e = ent(a, "e");
            json!({ "lines": crate::hatch::hatch_lines(&e, &d).map_or(Value::Null, |l| prims_v(&l)),
                "loops": crate::hatch::loops_of(&e).map_or(Value::Null, |l| Value::Array(l.iter().map(|x| Value::Array(x.iter().map(|v| num(*v)).collect())).collect())),
                "metric": crate::hatch::hatch_is_metric_doc(Some(&e), &d) })
        }
        "linetypes" => {
            let d = doc_of(a);
            let e = ent(a, "e");
            let pat = crate::linetypes::pattern_of(&d, g(a, "name").and_then(Value::as_str));
            let byb = g(a, "byBlockLt").and_then(Value::as_str);
            let parent = g(a, "parent").and_then(Value::as_str);
            let ps: Vec<Prim> = a.get("prims").and_then(Value::as_array).map(|l| l.iter().filter_map(Prim::from_value).collect()).unwrap_or_default();
            json!({ "pattern": pat.as_ref().map_or(Value::Null, |p| Value::Array(p.iter().map(|v| num(*v)).collect())),
                "dashes": prims_v(&crate::linetypes::dash_prims(&ps, pat.as_deref(), a.get("scale").and_then(Value::as_f64).unwrap_or(1.0))),
                "lt": crate::linetypes::resolve_lt(&d.layers, &e, byb, parent),
                "lw": num(crate::linetypes::resolve_lw(&d.layers, &e, g(a, "byBlockLw").and_then(Value::as_f64), parent)) })
        }
        "walk" => {
            let mut d = doc_of(a);
            d.set_space(g(a, "space").and_then(Value::as_str));
            let list = d.all();
            let fz = a.get("frozenVp").map(Entity::from_value).and_then(|vp| frozen_set(&vp));
            let opts = WalkOpts { m: g(a, "m").map(|v| m6(Some(v))), plot: b_(a, "plot"), frozen: fz.as_ref() };
            let mut rec = Recorder { calls: Vec::new(), mask: b_(a, "mask"), infinite: b_(a, "infinite") };
            walk(&d, &list, &mut rec, &opts);
            json!({ "calls": rec.calls, "key": frozen_key(fz.as_ref()) })
        }
        "polyBands" => poly_bands(&ent(a, "e")).map_or(Value::Null, |(l, p)| {
            json!({ "loops": l.iter().map(|x| Value::Array(x.iter().map(|v| num(*v)).collect())).collect::<Vec<_>>(), "plain": prims_v(&p) })
        }),
        "color" => {
            let d = doc_of(a);
            let e = ent(a, "e");
            let byb = color_arg(a.get("byBlock"));
            json!({ "aci": cc_cad::aci_hex(f_(a, "i")), "nearest": cc_cad::nearest_aci(a.get("hex").and_then(Value::as_str).unwrap_or("")),
                "resolve": d.resolve_color(&e, byb.as_ref(), g(a, "parent").and_then(Value::as_str)),
                "typeName": cc_cad::type_name(&e, &|k| lookup(k).unwrap_or_else(|| k.to_string())) })
        }
        _ => json!({ "$error": format!("unknown case {name}") }),
    }
}

/// Run a cases file `{ strings?: {key: text}, cases: [{ fn, args }] }` → the results in order.
pub fn run_cases(file: &Value) -> Vec<Value> {
    let strings: Obj = file.get("strings").and_then(Value::as_object).cloned().unwrap_or_default();
    let lookup = |k: &str| strings.get(k).and_then(Value::as_str).map(str::to_string);
    file.get("cases")
        .and_then(Value::as_array)
        .map(|l| {
            l.iter()
                .map(|c| run_case(c.get("fn").and_then(Value::as_str).unwrap_or(""), c.get("args").unwrap_or(&Value::Null), &lookup))
                .collect()
        })
        .unwrap_or_default()
}
