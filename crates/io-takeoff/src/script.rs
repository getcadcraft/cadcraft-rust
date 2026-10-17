//! The op-script runner behind `cadcraft-cli takeoff`: a JSON list of model operations (the `Takeoff` methods,
//! clipboard paste, undo / redo) and function calls (`{fn, args}` for the pure functions of every module), each
//! answered with its return value or error and, after a model operation, the project file, the estimate and the
//! quantities. `forks/tools/todiff.mjs` runs the same scripts on the frozen JS and diffs the records.

use std::collections::HashSet;

use serde_json::{Map, Value, json};

use cc_takeoff::bid::{self, bid_summary, bid_used, markup, normal_bid};
use cc_takeoff::clipboard::{self, CopyCtx, PasteOpts};
use cc_takeoff::formula;
use cc_takeoff::geom::{self, Op, P};
use cc_takeoff::jv::{self, Obj, n, s};
use cc_takeoff::library;
use cc_takeoff::matcher::{self, Cand, Img};
use cc_takeoff::model::{self, QOpts, Takeoff};
use cc_takeoff::overlay;
use cc_takeoff::packs;
use cc_takeoff::roomfill;
use cc_takeoff::scaletext;
use cc_takeoff::scene3d;
use cc_takeoff::snap;
use cc_takeoff::typein;
use cc_takeoff::units;

use crate::export;
use crate::xlsx::{self, CellV, Sheet, Stamp, XCell};

/// An integral JS number as an i64 (saturating; NaN → 0), as a deduct index.
#[allow(clippy::cast_possible_truncation)]
fn int_of(v: f64) -> i64 {
    v as i64
}

fn arg(a: &[Value], i: usize) -> Option<&Value> {
    a.get(i).filter(|v| !v.is_null())
}
fn argv(a: &[Value], i: usize) -> Value {
    a.get(i).cloned().unwrap_or(Value::Null)
}
fn num_arg(a: &[Value], i: usize, d: f64) -> f64 {
    arg(a, i).map_or(d, |v| jv::to_number(Some(v)))
}
fn str_arg(a: &[Value], i: usize) -> String {
    jv::to_string_or_empty(a.get(i))
}
fn obj_arg(a: &[Value], i: usize) -> Obj {
    jv::obj(a.get(i)).cloned().unwrap_or_default()
}
fn strs(v: Option<&Value>) -> Vec<String> {
    jv::arr(v).map(|l| l.iter().map(|x| jv::to_string(Some(x))).collect()).unwrap_or_default()
}

/// A point `{x, y, b?}` from JSON.
pub fn p_of(v: Option<&Value>) -> P {
    let g = |k: &str| v.and_then(|p| jv::num(p.get(k))).unwrap_or(f64::NAN);
    let b = v.and_then(|p| jv::num(p.get("b"))).unwrap_or(0.0);
    P::arc(g("x"), g("y"), b)
}
/// Points from JSON.
pub fn pts_of(v: Option<&Value>) -> Vec<P> {
    jv::arr(v).map(|l| l.iter().map(|p| p_of(Some(p))).collect()).unwrap_or_default()
}
fn rings_of(v: Option<&Value>) -> Vec<Vec<P>> {
    jv::arr(v).map(|l| l.iter().map(|r| pts_of(Some(r))).collect()).unwrap_or_default()
}
/// A point as the JS writes it (`{x, y}` plus `b` when it is an arc).
pub fn pj(p: &P) -> Value {
    let mut o = Map::new();
    o.insert("x".into(), n(p.x));
    o.insert("y".into(), n(p.y));
    if p.has_b() {
        o.insert("b".into(), n(p.b));
    }
    Value::Object(o)
}
fn pjs(l: &[P]) -> Value {
    Value::Array(l.iter().map(pj).collect())
}
fn m6(v: Option<&Value>) -> geom::M6 {
    let l = jv::arr(v).cloned().unwrap_or_default();
    let g = |i: usize| jv::num(l.get(i)).unwrap_or(f64::NAN);
    [g(0), g(1), g(2), g(3), g(4), g(5)]
}
fn m6j(m: &geom::M6) -> Value {
    Value::Array(m.iter().map(|v| n(*v)).collect())
}
fn img_of(v: Option<&Value>) -> Img {
    let w = v.and_then(|x| jv::num(x.get("w"))).unwrap_or(0.0);
    let h = v.and_then(|x| jv::num(x.get("h"))).unwrap_or(0.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (wu, hu) = (w.clamp(0.0, 1e5) as usize, h.clamp(0.0, 1e5) as usize);
    let data: Vec<f32> = jv::arr(v.and_then(|x| x.get("data")))
        .map(|l| {
            l.iter()
                .map(|x| {
                    #[allow(clippy::cast_possible_truncation)]
                    let f = jv::to_number(Some(x)) as f32;
                    f
                })
                .collect()
        })
        .unwrap_or_default();
    Img { data, w: wu, h: hu }
}
fn imgj(i: &Img) -> Value {
    json!({ "data": i.data.iter().map(|v| n(f64::from(*v))).collect::<Vec<_>>(), "w": i.w, "h": i.h })
}
fn mask_of(v: Option<&Value>) -> Vec<u8> {
    jv::arr(v)
        .map(|l| {
            l.iter()
                .map(|x| {
                    let f = jv::to_number(Some(x));
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let b = if f.is_finite() { f.rem_euclid(256.0) as u8 } else { 0 };
                    b
                })
                .collect()
        })
        .unwrap_or_default()
}
fn usize_of(v: f64) -> usize {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let u = if v.is_finite() { v.clamp(0.0, 1e9) as usize } else { 0 };
    u
}
fn cand_of(v: &Value) -> Cand {
    let g = |k: &str| jv::num(v.get(k)).unwrap_or(f64::NAN);
    Cand {
        x: g("x"),
        y: g("y"),
        w: g("w"),
        h: g("h"),
        rot: g("rot"),
        score: g("score"),
        sheet: v.get("sheet").filter(|x| !x.is_null()).map(|x| jv::to_string(Some(x))),
    }
}
fn candj(c: &Cand) -> Value {
    let mut o = Map::new();
    o.insert("x".into(), n(c.x));
    o.insert("y".into(), n(c.y));
    o.insert("w".into(), n(c.w));
    o.insert("h".into(), n(c.h));
    o.insert("rot".into(), n(c.rot));
    o.insert("score".into(), n(c.score));
    if let Some(sh) = &c.sheet {
        o.insert("sheet".into(), s(sh.clone()));
    }
    Value::Object(o)
}
fn res_json<T>(r: Result<T, String>, ok: impl FnOnce(T) -> Value) -> Value {
    match r {
        Ok(v) => ok(v),
        Err(e) => json!({ "ok": false, "error": e }),
    }
}

/// `labels` of the script: sheet key → its name in the exports (the key itself when not listed).
fn labeler(labels: &Value) -> impl Fn(&str) -> String + '_ {
    move |k: &str| labels.get(k).map_or_else(|| k.to_string(), |v| jv::to_string(Some(v)))
}

fn xcell_of_json(v: &Value) -> XCell {
    match v {
        Value::Null => XCell::Empty,
        Value::Number(x) => XCell::Num(x.as_f64().unwrap_or(f64::NAN)),
        Value::String(t) => XCell::t(t.clone()),
        Value::Object(o) => {
            let cv = match o.get("v") {
                None | Some(Value::Null) => CellV::Undefined,
                Some(Value::Number(x)) => CellV::Num(x.as_f64().unwrap_or(f64::NAN)),
                Some(other) => CellV::Str(jv::to_string(Some(other))),
            };
            let f = o.get("f").filter(|f| jv::truthy(Some(f))).map(|f| jv::to_string(Some(f)));
            let st = jv::str(o.get("s")).and_then(|st| {
                ["none", "head", "qty", "pct", "rate", "money", "bold", "boldMoney", "text", "int"].iter().find(|k| **k == st).copied()
            });
            XCell::Obj { v: cv, f, s: st }
        }
        other => XCell::t(jv::to_string(Some(other))),
    }
}
fn sheets_of(v: Option<&Value>) -> Vec<Sheet> {
    jv::arr(v)
        .map(|l| {
            l.iter()
                .map(|sh| Sheet {
                    name: jv::to_string(sh.get("name")),
                    rows: jv::arr(sh.get("rows"))
                        .map(|rows| {
                            rows.iter()
                                .map(|r| jv::arr(Some(r)).map(|c| c.iter().map(xcell_of_json).collect()).unwrap_or_default())
                                .collect()
                        })
                        .unwrap_or_default(),
                    cols: jv::arr(sh.get("cols")).map(|c| c.iter().map(|x| jv::to_number(Some(x))).collect()).unwrap_or_default(),
                    freeze: usize_of(jv::to_number(sh.get("freeze"))),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn b64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3).saturating_mul(4));
    for c in bytes.chunks(3) {
        let b = [c.first().copied().unwrap_or(0), c.get(1).copied().unwrap_or(0), c.get(2).copied().unwrap_or(0)];
        let v = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (k, sh) in [18u32, 12, 6, 0].iter().enumerate() {
            if k <= c.len() {
                out.push(char::from(T.get(((v >> sh) & 63) as usize).copied().unwrap_or(b'A')));
            } else {
                out.push('=');
            }
        }
    }
    out
}
/// Bytes as the harness compares them.
pub fn bytes_json(b: &[u8]) -> Value {
    json!({ "$base64": b64(b) })
}

fn estimate_json(e: &model::Estimate) -> Value {
    e.to_json()
}

/// A pure function call (`{fn, args}`) → its result as JSON.
pub fn call(f: &str, a: &[Value], labels: &Value) -> Result<Value, String> {
    let lab = labeler(labels);
    let ok = |v: Value| Ok(v);
    match f {
        "numStr" => ok(Value::Array(jv::arr(a.first()).map_or(&[][..], Vec::as_slice).iter().map(|x| s(jv::num_str(jv::to_number(Some(x))))).collect())),
        // ---- units
        "parseLength" => {
            let du = arg(a, 1).and_then(|o| jv::str(o.get("defaultUnit"))).unwrap_or("ft").to_string();
            ok(res_json(units::parse_length(&str_arg(a, 0), &du), |ft| json!({ "ok": true, "ft": n(ft) })))
        }
        "formatFtIn" => ok(s(units::format_ft_in(num_arg(a, 0, f64::NAN), num_arg(a, 1, 16.0)))),
        "formatLength" => ok(s(units::format_length(num_arg(a, 0, f64::NAN), arg(a, 1).map_or("ftin".to_string(), |v| jv::to_string(Some(v))).as_str(), i32_of(num_arg(a, 2, 2.0))))),
        "fixed" => ok(s(units::fixed(num_arg(a, 0, f64::NAN), i32_of(num_arg(a, 1, 2.0))))),
        "roundTo" => ok(n(units::round_to(num_arg(a, 0, f64::NAN), i32_of(num_arg(a, 1, 0.0))))),
        "presetFtPerUnit" => units::preset_ft_per_unit(&str_arg(a, 0), num_arg(a, 1, 1.0)).map(n),
        "toFt" => units::to_ft(num_arg(a, 0, f64::NAN), &str_arg(a, 1)).map(n),
        "calibrate" => {
            let (pa, pb) = (p_of(a.first()), p_of(a.get(1)));
            ok(res_json(units::calibrate(pa.x, pa.y, pb.x, pb.y, num_arg(a, 2, f64::NAN)), |(f, u)| json!({ "ok": true, "ftPerUnit": n(f), "units": n(u) })))
        }
        "matchPreset" => ok(units::match_preset(num_arg(a, 0, f64::NAN), num_arg(a, 1, 1.0), num_arg(a, 2, 0.005))
            .map_or(Value::Null, |(p, e)| json!({ "key": p.key, "label": p.label, "ratio": n(p.ratio), "group": p.group, "err": n(e) }))),
        "describeScale" => ok(s(units::describe_scale(num_arg(a, 0, f64::NAN), num_arg(a, 1, 1.0)))),
        "parseRate" => ok(res_json(units::parse_rate(&str_arg(a, 0)), |r| json!({ "ok": true, "rate4": n(r) }))),
        "parsePercent" => ok(res_json(units::parse_percent(&str_arg(a, 0)), |r| json!({ "ok": true, "bp": n(r) }))),
        "formatCents" => ok(s(units::format_cents(num_arg(a, 0, f64::NAN)))),
        "formatRate" => ok(s(units::format_rate(num_arg(a, 0, f64::NAN)))),
        "rateInput" => ok(s(units::rate_input(num_arg(a, 0, f64::NAN)))),
        "percentInput" => ok(s(units::percent_input(num_arg(a, 0, f64::NAN)))),
        "formatPercent" => ok(s(units::format_percent(num_arg(a, 0, f64::NAN)))),
        "extendCents" => ok(n(units::extend_cents(num_arg(a, 0, f64::NAN), num_arg(a, 1, f64::NAN)))),
        "withWaste" => ok(n(units::with_waste(num_arg(a, 0, f64::NAN), num_arg(a, 1, f64::NAN)))),
        "parsePitch" => ok(match units::parse_pitch(&str_arg(a, 0)) {
            Ok(r) => json!({ "ok": true, "rise": n(r) }),
            Err(e) => json!({ "ok": false, "error": e }),
        }),
        "formatPitch" => ok(s(units::format_pitch(num_arg(a, 0, f64::NAN)))),
        "slopeFactor" => ok(n(units::slope_factor(num_arg(a, 0, f64::NAN)))),
        "hipFactor" => ok(n(units::hip_factor(num_arg(a, 0, f64::NAN)))),
        // ---- geom
        "polygonArea" => ok(n(geom::polygon_area(&pts_of(a.first())))),
        "signedArea" => ok(n(geom::signed_area(&pts_of(a.first())))),
        "perimeter" => ok(n(geom::perimeter(&pts_of(a.first())))),
        "polylineLength" => ok(n(geom::polyline_length(&pts_of(a.first())))),
        "segmentLengths" => ok(Value::Array(geom::segment_lengths(&pts_of(a.first())).into_iter().map(n).collect())),
        "segLength" => ok(n(geom::seg_length(&p_of(a.first()), &p_of(a.get(1)), num_arg(a, 2, 0.0)))),
        "segArea" => ok(n(geom::seg_area(&p_of(a.first()), &p_of(a.get(1)), num_arg(a, 2, 0.0)))),
        "arcOf" => ok(geom::arc_of(&p_of(a.first()), &p_of(a.get(1)), num_arg(a, 2, 0.0))
            .map_or(Value::Null, |c| json!({ "cx": n(c.cx), "cy": n(c.cy), "r": n(c.r), "a0": n(c.a0), "sweep": n(c.sweep) }))),
        "areaWithHoles" => {
            let r = geom::area_with_holes(&pts_of(a.first()), &rings_of(a.get(1)));
            ok(json!({ "gross": n(r.gross), "cut": n(r.cut), "net": n(r.net), "perimeter": n(r.perimeter), "outerPerimeter": n(r.outer_perimeter) }))
        }
        "bulgeThrough" => ok(n(geom::bulge_through(&p_of(a.first()), &p_of(a.get(1)), &p_of(a.get(2))))),
        "flatten" => ok(pjs(&geom::flatten_step(&pts_of(a.first()), jv::truthy(a.get(1)), num_arg(a, 2, std::f64::consts::PI / 36.0)))),
        "pathOps" => ok(Value::Array(
            geom::path_ops(&pts_of(a.first()), jv::truthy(a.get(1)))
                .iter()
                .map(|o| match o {
                    Op::M(p) => json!(["M", pj(p)]),
                    Op::L(p) => json!(["L", pj(p)]),
                    Op::C(c1, c2, p) => json!(["C", pj(c1), pj(c2), pj(p)]),
                    Op::Z => json!(["Z"]),
                })
                .collect(),
        )),
        "pointInPolygon" => ok(Value::Bool(geom::point_in_polygon(&p_of(a.first()), &pts_of(a.get(1))))),
        "selfIntersects" => ok(Value::Bool(geom::self_intersects(&pts_of(a.first())))),
        "outlinesCross" => ok(Value::Bool(geom::outlines_cross(&pts_of(a.first()), &pts_of(a.get(1))))),
        "outlineProblem" => ok(geom::outline_problem(&pts_of(a.first()), arg(a, 1).map_or("area".to_string(), |v| jv::to_string(Some(v))).as_str()).map_or(Value::Null, s)),
        "holeProblem" => ok(geom::hole_problem(&pts_of(a.first()), &rings_of(a.get(1)), &pts_of(a.get(2))).map_or(Value::Null, s)),
        "snap45" => ok(pj(&geom::snap45(&p_of(a.first()), &p_of(a.get(1))))),
        "distToSegment" => ok(n(geom::dist_to_segment(&p_of(a.first()), &p_of(a.get(1)), &p_of(a.get(2))))),
        "distToPath" => ok(n(geom::dist_to_path(&p_of(a.first()), &pts_of(a.get(1)), jv::truthy(a.get(2))))),
        "bounds" => {
            let b = geom::bounds(&pts_of(a.first()));
            ok(json!({ "minX": n(b.min_x), "minY": n(b.min_y), "maxX": n(b.max_x), "maxY": n(b.max_y) }))
        }
        "centroid" => ok(pj(&geom::centroid(&pts_of(a.first())))),
        "isConvex" => ok(Value::Bool(geom::is_convex(&pts_of(a.first())))),
        "clipConvex" => ok(pjs(&geom::clip_convex(&pts_of(a.first()), &pts_of(a.get(1))))),
        "overlapArea" => ok(n(geom::overlap_area(&pts_of(a.first()), &rings_of(a.get(1)), &pts_of(a.get(2))))),
        "spans" => {
            let ded: HashSet<i64> = jv::arr(a.get(1)).map(|l| l.iter().filter_map(|v| jv::int(Some(v))).map(int_of).collect()).unwrap_or_default();
            let f = |i: usize| ded.contains(&i64::try_from(i).unwrap_or(i64::MIN));
            ok(Value::Array(geom::spans(&pts_of(a.first()), &f).iter().map(|sp| json!({ "deduct": sp.deduct, "pts": pjs(&sp.pts) })).collect()))
        }
        "apply" => ok(pj(&geom::apply(&m6(a.first()), &p_of(a.get(1))))),
        "multiply" => ok(m6j(&geom::multiply(&m6(a.first()), &m6(a.get(1))))),
        "invert" => ok(m6j(&geom::invert(&m6(a.first())))),
        // ---- formula
        "parse" => {
            let vars = strs(a.get(1));
            let vr: Vec<&str> = vars.iter().map(String::as_str).collect();
            ok(match formula::parse(&jv::to_string_or_empty(a.first()), &vr) {
                Ok(p) => json!({ "ok": true, "ast": p.ast.to_json(), "uses": p.uses }),
                Err(e) => json!({ "ok": false, "error": e.message, "pos": e.pos.map_or(Value::Null, |p| n(p as f64)) }),
            })
        }
        "compute" => {
            let vals: Vec<(String, f64)> = jv::obj(a.get(1)).map(|o| model::js_entries(o).into_iter().map(|(k, v)| (k.clone(), jv::num(Some(v)).unwrap_or(f64::NAN))).collect()).unwrap_or_default();
            let names: Vec<&str> = vals.iter().map(|(k, _)| k.as_str()).collect();
            ok(match formula::parse(&jv::to_string_or_empty(a.first()), &names) {
                Err(e) => json!({ "ok": false, "error": e.message, "pos": e.pos.map_or(Value::Null, |p| n(p as f64)) }),
                Ok(p) => match formula::evaluate(&p.ast, &|name| vals.iter().rev().find(|(k, _)| k == name).map(|(_, v)| *v)) {
                    Ok(v) => json!({ "ok": true, "value": n(v) }),
                    Err(e) => json!({ "ok": false, "error": e.message }),
                },
            })
        }
        // ---- model
        "normalItem" => ok(Value::Object(model::normal_item(&obj_arg(a, 0)))),
        "normalComponent" => ok(Value::Object(model::normal_component(&obj_arg(a, 0)))),
        "normalNote" => ok(model::normal_note(&obj_arg(a, 0)).unwrap_or(Value::Null)),
        "normalCond" => ok(model::normal_cond(a.first(), &str_arg(a, 1)).unwrap_or(Value::Null)),
        "crewOf" => ok(model::crew_of(a.first()).unwrap_or(Value::Null)),
        "cleanAddonData" => ok(model::clean_addon_data(a.first()).unwrap_or(Value::Null)),
        "csiOk" => ok(Value::Bool(model::csi_ok(a.first()))),
        "readProject" => ok(res_json(model::read_project(&argv(a, 0)), |r| json!({ "ok": true, "state": r.state, "warnings": r.warnings }))),
        "measureMark" => ok(model::measure_mark(&argv(a, 0), num_arg(a, 1, f64::NAN), arg(a, 2)).map_or(Value::Null, |m| m.to_json())),
        "markExtras" => ok(Value::Object(model::mark_extras(&argv(a, 0)))),
        "unitOf" => ok(s(model::unit_of(&argv(a, 0), &str_arg(a, 1)))),
        "kindsOf" => ok(json!(model::kinds_of(arg(a, 0)))),
        "projectJson" => ok(model::project_json(&argv(a, 0), &str_arg(a, 1))),
        // ---- bid
        "normalBid" => ok(normal_bid(a.first())),
        "bidUsed" => ok(Value::Bool(bid_used(a.first()))),
        "markup" => {
            let m = markup(num_arg(a, 0, f64::NAN), num_arg(a, 1, f64::NAN), &argv(a, 2));
            ok(json!({ "burden": n(m.burden), "cost": n(m.cost), "tax": n(m.tax), "overhead": n(m.overhead), "profit": n(m.profit), "bond": n(m.bond), "total": n(m.total) }))
        }
        "bidSummary" => {
            let e = argv(a, 0);
            let alts: Vec<bid::AltIn> = jv::arr(a.get(2))
                .map(|l| {
                    l.iter()
                        .map(|x| bid::AltIn {
                            name: jv::to_string(x.get("area").and_then(|ar| ar.get("name"))),
                            alt: jv::to_string(x.get("alt")),
                            amount: jv::to_number(x.get("amount")),
                            mat_total: jv::to_number(x.get("est").and_then(|e| e.get("matTotal"))),
                            lab_total: jv::to_number(x.get("est").and_then(|e| e.get("labTotal"))),
                        })
                        .collect()
                })
                .unwrap_or_default();
            ok(bid_summary(jv::to_number(e.get("matTotal")), jv::to_number(e.get("labTotal")), jv::truthy(e.get("incomplete")), a.get(1), &alts).to_json())
        }
        // ---- library and packs
        "parseCsv" => ok(json!(library::parse_csv(&str_arg(a, 0)))),
        "readLibraryCsv" => ok(res_json(library::read_library_csv(&str_arg(a, 0)), |(e, w)| json!({ "ok": true, "entries": e, "warnings": w }))),
        "libraryCsv" => ok(s(library::library_csv(jv::arr(a.first()).map_or(&[][..], Vec::as_slice)))),
        "mergeEntries" => {
            let (lib, ad, up) = library::merge_entries(jv::arr(a.first()).map_or(&[][..], Vec::as_slice), jv::arr(a.get(1)).map_or(&[][..], Vec::as_slice));
            ok(json!({ "lib": lib, "added": n(ad), "updated": n(up) }))
        }
        "entryFromItem" => ok(library::entry_from_item(&argv(a, 0))),
        "itemFromEntry" => ok(library::item_from_entry(&argv(a, 0))),
        "readStored" => ok(Value::Array(library::read_stored(&argv(a, 0)))),
        "entryMeta" => ok(Value::Object(library::entry_meta(a.first()))),
        "repricePatches" => ok(Value::Array(
            library::reprice_patches(jv::arr(a.first()).map_or(&[][..], Vec::as_slice), jv::arr(a.get(1)).map_or(&[][..], Vec::as_slice))
                .into_iter()
                .map(|(id, p)| json!({ "id": id, "patch": p }))
                .collect(),
        )),
        "readPack" => ok(res_json(packs::read_pack(&argv(a, 0)), |r| json!({ "ok": true, "pack": r.pack, "warnings": r.warnings }))),
        "packEntries" => {
            let o = argv(a, 1);
            ok(Value::Array(packs::pack_entries(&argv(a, 0), jv::str(o.get("system")).unwrap_or("both"), jv::truthy(o.get("samples")))))
        }
        "installPack" => {
            let o = argv(a, 2);
            let (lib, ad, up, kept, full) = packs::install_pack(jv::arr(a.first()).map_or(&[][..], Vec::as_slice), &argv(a, 1), jv::str(o.get("system")).unwrap_or("both"), jv::truthy(o.get("samples")));
            ok(json!({ "lib": lib, "added": n(ad), "updated": n(up), "kept": n(kept), "full": full }))
        }
        "removePack" => {
            let (lib, r) = packs::remove_pack(jv::arr(a.first()).map_or(&[][..], Vec::as_slice), &str_arg(a, 1));
            ok(json!({ "lib": lib, "removed": n(r) }))
        }
        "installedPacks" => ok(Value::Array(packs::installed_packs(jv::arr(a.first()).map_or(&[][..], Vec::as_slice)))),
        "rateUnset" => ok(Value::Bool(packs::rate_unset(&argv(a, 0)))),
        "libraryPack" => ok(packs::library_pack(jv::arr(a.first()).map_or(&[][..], Vec::as_slice), &obj_arg(a, 1))),
        "packSystemOf" => ok(s(packs::pack_system_of(&argv(a, 0)))),
        // ---- clipboard
        "copyMarks" | "copyItems" => {
            let c = argv(a, 2);
            let ctx = CopyCtx { view: c.get("view").cloned(), user_unit: c.get("userUnit").cloned(), source: jv::to_string_or_empty(c.get("source")), now: jv::to_number(c.get("now")) };
            let ids = strs(a.get(1));
            ok(if f == "copyMarks" { clipboard::copy_marks(&argv(a, 0), &ids, &ctx) } else { clipboard::copy_items(&argv(a, 0), &ids, &ctx) }.unwrap_or(Value::Null))
        }
        "readClip" => ok(res_json(clipboard::read_clip(&argv(a, 0)), |r| json!({ "ok": true, "warnings": r.warnings, "clip": r.clip }))),
        "scaleProblem" => ok(clipboard::scale_problem(&argv(a, 0), arg(a, 1)).map_or(Value::Null, s)),
        "sizeProblem" => ok(clipboard::size_problem(&argv(a, 0), arg(a, 1)).map_or(Value::Null, s)),
        "freeName" => ok(s(clipboard::free_name(&str_arg(a, 0), jv::arr(a.get(1)).map_or(&[][..], Vec::as_slice)))),
        "anchorOf" => ok(clipboard::anchor_of(jv::arr(a.first()).map_or(&[][..], Vec::as_slice))),
        "measured" => ok(Value::Bool(clipboard::measured(&argv(a, 0)))),
        // ---- scales from text, typed entries
        "scalesInText" => ok(Value::Array(
            scaletext::scales_in_text(&str_arg(a, 0)).iter().map(|f| json!({ "preset": f.preset, "label": f.label, "index": f.index, "scaleWord": f.scale_word })).collect(),
        )),
        "sheetScale" => {
            let r = scaletext::sheet_scale(&str_arg(a, 0));
            let mut o = Map::new();
            o.insert("preset".into(), r.preset.map_or(Value::Null, s));
            if let Some(l) = r.label {
                o.insert("label".into(), s(l));
            }
            o.insert("n".into(), json!(r.n));
            o.insert("others".into(), json!(r.others));
            o.insert("nts".into(), Value::Bool(r.nts));
            o.insert("why".into(), s(r.why));
            ok(Value::Object(o))
        }
        "signedLength" => ok(typein::signed_length(&str_arg(a, 0)).map_or(Value::Null, n)),
        "screenDir" => {
            let l = jv::arr(a.first()).cloned().unwrap_or_default();
            let g = |i: usize| jv::num(l.get(i)).unwrap_or(f64::NAN);
            ok(typein::screen_dir(&[g(0), g(1), g(2), g(3)], num_arg(a, 1, f64::NAN), num_arg(a, 2, f64::NAN)).map_or(Value::Null, |p| pj(&p)))
        }
        "parseEntry" => {
            let c = argv(a, 1);
            let opt_p = |k: &str| c.get(k).filter(|v| !v.is_null()).map(|v| p_of(Some(v)));
            let l = jv::arr(c.get("linear")).cloned().unwrap_or_default();
            let g = |i: usize| jv::num(l.get(i)).unwrap_or(f64::NAN);
            let ctx = typein::Ctx { first: opt_p("first"), last: opt_p("last"), cursor: opt_p("cursor"), ft_per_unit: jv::to_number(c.get("ftPerUnit")), linear: [g(0), g(1), g(2), g(3)] };
            ok(match typein::parse_entry(&str_arg(a, 0), &ctx) {
                Ok((p, k)) => json!({ "ok": true, "kind": k, "point": pj(&p) }),
                Err(e) => json!({ "ok": false, "error": e }),
            })
        }
        // ---- snap
        "snap" => {
            let ops = snap_ops(a.first());
            let max = arg(a, 1).map_or(snap::MAX_SEGS, |v| usize_of(jv::to_number(Some(v))));
            let lw = snap::extract_segments(&snap::PDFJS_OPS, &ops, max);
            let idx = snap::SnapIndex::new(&lw);
            let hits: Vec<Value> = jv::arr(a.get(2))
                .map_or(&[][..], Vec::as_slice)
                .iter()
                .map(|q| {
                    let k = q.get("kinds");
                    let kb = |nm: &str| k.and_then(|k| k.get(nm)).is_none_or(|v| jv::truthy(Some(v)));
                    let kinds = snap::Kinds { end: kb("end"), mid: kb("mid"), int: kb("int"), near: kb("near") };
                    idx.snap(jv::to_number(q.get("x")), jv::to_number(q.get("y")), jv::to_number(q.get("tol")), kinds)
                        .map_or(Value::Null, |h| json!({ "x": n(h.x), "y": n(h.y), "kind": h.kind, "d": n(h.d) }))
                })
                .collect();
            ok(json!({ "n": lw.n, "truncated": lw.truncated, "segs": lw.segs.iter().map(|v| n(*v)).collect::<Vec<_>>(), "curve": lw.curve, "hits": hits }))
        }
        "intersect" => {
            let g: Vec<f64> = jv::arr(a.first()).map(|l| l.iter().map(|x| jv::to_number(Some(x))).collect()).unwrap_or_default();
            ok(snap::intersect(&g, usize_of(num_arg(a, 1, 0.0)), usize_of(num_arg(a, 2, 0.0))).map_or(Value::Null, |(x, y)| json!({ "x": n(x), "y": n(y) })))
        }
        // ---- overlay
        "fitTwoPoints" => ok(overlay::fit_two_points(&p_of(a.first()), &p_of(a.get(1)), &p_of(a.get(2)), &p_of(a.get(3))).map_or(Value::Null, |m| m6j(&m))),
        "scaleOf" => ok(n(overlay::scale_of(&m6(a.first())))),
        "pixelMap" => ok(m6j(&overlay::pixel_map(&m6(a.first()), &m6(a.get(1)), &m6(a.get(2)), num_arg(a, 3, f64::NAN)))),
        "warp" => ok(imgj(&overlay::warp(&img_of(a.first()), &m6(a.get(1)), usize_of(num_arg(a, 2, 0.0)), usize_of(num_arg(a, 3, 0.0))))),
        "dilate" => ok(imgj(&overlay::dilate(&img_of(a.first()), usize_of(num_arg(a, 1, 0.0))))),
        "changeMask" => {
            let o = argv(a, 2);
            let cm = overlay::change_mask(&img_of(a.first()), &img_of(a.get(1)), usize_of(o.get("r").map_or(2.0, |v| jv::to_number(Some(v)))), o.get("on").map_or(0.35, |v| jv::to_number(Some(v))), o.get("off").map_or(0.2, |v| jv::to_number(Some(v))));
            ok(json!({ "mask": cm.mask, "w": cm.w, "h": cm.h, "removed": cm.removed, "added": cm.added }))
        }
        "markChange" => {
            let m = argv(a, 0);
            let c = argv(a, 1);
            let o = argv(a, 3);
            let mask = mask_of(c.get("mask"));
            let (w, h) = (usize_of(jv::to_number(c.get("w"))), usize_of(jv::to_number(c.get("h"))));
            let cm = overlay::ChangeMask { sum: overlay::sum_of(&mask, w, h), mask, w, h, removed: 0, added: 0 };
            let mx = m6(a.get(2));
            let to_px = |p: &P| geom::apply(&mx, p);
            let r = overlay::mark_change(&jv::to_string(m.get("kind")), &pts_of(m.get("pts")), &rings_of(m.get("holes")), &cm, &to_px, o.get("buffer").map_or(8.0, |v| jv::to_number(Some(v))));
            ok(json!({ "removed": r.removed, "added": r.added, "total": r.total, "share": n(r.share), "corner": r.corner, "changed": overlay::changed(&r, &jv::to_string(m.get("kind")), 0.15) }))
        }
        "boxSum" => {
            let mask = mask_of(a.first());
            let (w, h) = (usize_of(num_arg(a, 1, 0.0)), usize_of(num_arg(a, 2, 0.0)));
            let sm = overlay::sum_of(&mask, w, h);
            ok(json!(overlay::box_sum(&sm, w, h, num_arg(a, 3, 0.0), num_arg(a, 4, 0.0), num_arg(a, 5, 0.0), num_arg(a, 6, 0.0))))
        }
        "movePts" => ok(pjs(&overlay::move_pts(&pts_of(a.first()), &m6(a.get(1))))),
        // ---- auto count
        "toInk" => {
            let rgba: Vec<u8> = mask_of(a.first());
            ok(imgj(&matcher::to_ink(&rgba, usize_of(num_arg(a, 1, 0.0)), usize_of(num_arg(a, 2, 0.0)))))
        }
        "crop" => ok(imgj(&matcher::crop(&img_of(a.first()), num_arg(a, 1, 0.0), num_arg(a, 2, 0.0), num_arg(a, 3, 0.0), num_arg(a, 4, 0.0)))),
        "rot90" => ok(imgj(&matcher::rot90(&img_of(a.first())))),
        "rotate" => ok(imgj(&matcher::rotate(&img_of(a.first()), num_arg(a, 1, 0.0)))),
        "blur" => ok(imgj(&matcher::blur(&img_of(a.first()), usize_of(num_arg(a, 1, 1.0))))),
        "trim" => {
            let tr = matcher::trim(&img_of(a.first()), num_arg(a, 1, 0.08));
            let mut o = jv::obj(Some(&imgj(&tr.img))).cloned().unwrap_or_default();
            o.insert("ox".into(), json!(tr.ox));
            o.insert("oy".into(), json!(tr.oy));
            ok(Value::Object(o))
        }
        "templateProblem" => ok(matcher::template_problem(&img_of(a.first())).map_or(Value::Null, s)),
        "nccPeaks" => {
            let o = argv(a, 2);
            let mut hooks = matcher::Hooks { tick: None, stop: None };
            let r = matcher::ncc_peaks(&img_of(a.first()), &img_of(a.get(1)), o.get("floor").map_or(0.5, |v| jv::to_number(Some(v))), o.get("minStd").map_or(0.02, |v| jv::to_number(Some(v))), None, &mut hooks);
            ok(r.map_or(Value::Null, |l| {
                Value::Array(l.iter().map(|p| json!({ "x": p.x, "y": p.y, "dx": n(p.dx), "dy": n(p.dy), "score": n(p.score), "raw": n(p.raw) })).collect())
            }))
        }
        "findSymbol" => {
            let o = argv(a, 2);
            let rots: Vec<f64> = jv::arr(o.get("rotations")).map_or_else(|| vec![0.0, 90.0, 180.0, 270.0], |l| l.iter().map(|x| jv::to_number(Some(x))).collect());
            let r = matcher::find_symbol(&img_of(a.first()), &img_of(a.get(1)), &rots, o.get("floor").map_or(0.5, |v| jv::to_number(Some(v))), usize_of(o.get("smooth").map_or(0.0, |v| jv::to_number(Some(v)))), None, None);
            ok(r.map_or(Value::Null, |l| Value::Array(l.iter().map(candj).collect())))
        }
        "suppress" => {
            let c: Vec<Cand> = jv::arr(a.first()).map(|l| l.iter().map(cand_of).collect()).unwrap_or_default();
            ok(Value::Array(matcher::suppress(&c, num_arg(a, 1, 0.0), num_arg(a, 2, 0.25)).iter().map(candj).collect()))
        }
        "workScale" => ok(n(matcher::work_scale(num_arg(a, 0, f64::NAN), num_arg(a, 1, f64::NAN), num_arg(a, 2, 40.0)))),
        // ---- room fill
        "dilateBin" => ok(json!(roomfill::dilate_bin(&mask_of(a.first()), usize_of(num_arg(a, 1, 0.0)), usize_of(num_arg(a, 2, 0.0)), usize_of(num_arg(a, 3, 0.0))))),
        "trace" => ok(pjs(&roomfill::trace(&mask_of(a.first()), usize_of(num_arg(a, 1, 0.0)), usize_of(num_arg(a, 2, 0.0))))),
        "simplify" => ok(pjs(&roomfill::simplify(&pts_of(a.first()), num_arg(a, 1, 1.0)))),
        "roomAt" => {
            let o = argv(a, 2);
            let sp = p_of(a.get(1));
            let r = roomfill::room_at(
                &img_of(a.first()),
                sp.x,
                sp.y,
                o.get("gapPx").map_or(20.0, |v| jv::to_number(Some(v))),
                o.get("on").map_or(0.25, |v| jv::to_number(Some(v))),
                o.get("maxShare").map_or(0.6, |v| jv::to_number(Some(v))),
            );
            ok(match r {
                Ok((ring, px)) => json!({ "ok": true, "ring": pjs(&ring), "px": px }),
                Err(why) => json!({ "ok": false, "why": why }),
            })
        }
        // ---- 3D
        "buildScene" => {
            let o = argv(a, 2);
            ok(match scene3d::build_scene(&argv(a, 0), &str_arg(a, 1), &view_of(&o)) {
                Ok(sc) => sc.to_json(),
                Err(reason) => json!({ "ok": false, "reason": reason }),
            })
        }
        "buildStack" => {
            let levels: Vec<scene3d::Level> = jv::arr(a.get(1))
                .map_or(&[][..], Vec::as_slice)
                .iter()
                .map(|lv| scene3d::Level { key: jv::to_string(lv.get("key")), elev_ft: jv::to_number(lv.get("elevFt")), view: view_of(lv) })
                .collect();
            ok(match scene3d::build_stack(&argv(a, 0), &levels, None) {
                Ok(sc) => sc.to_json(),
                Err(reason) => json!({ "ok": false, "reason": reason }),
            })
        }
        "meshVolume" | "meshArea" => {
            let t: Vec<f64> = jv::arr(a.first()).map(|l| l.iter().map(|x| jv::to_number(Some(x))).collect()).unwrap_or_default();
            ok(n(if f == "meshVolume" { scene3d::mesh_volume(&t) } else { scene3d::mesh_area(&t) }))
        }
        "sceneToGlb" => {
            let o = argv(a, 1);
            let name = o.get("name").filter(|v| !v.is_null()).map_or("TakeoffCraft".to_string(), |v| jv::to_string(Some(v)));
            ok(bytes_json(&crate::glb::scene_to_glb(&argv(a, 0), &name)))
        }
        // ---- exports
        "estimateCsv" => ok(s(export::estimate_csv(&argv(a, 0), &model::base_estimate(&argv(a, 0))))),
        "sheetsCsv" => ok(s(export::sheets_csv(&argv(a, 0), &lab))),
        "estimateSheets" => ok(export::sheets_json(&export::estimate_sheets(&argv(a, 0), &lab, &model::base_estimate(&argv(a, 0))))),
        "materialList" => ok(Value::Array(
            export::material_list(&argv(a, 0), &model::base_estimate(&argv(a, 0)))
                .iter()
                .map(|g| json!({ "name": g.name, "unit": g.unit, "qty": n(g.qty), "qtyW": n(g.qty_w), "matExt": n(g.mat_ext), "csi": g.csi, "items": g.items }))
                .collect(),
        )),
        "materialCsv" => ok(s(export::material_csv(&argv(a, 0), &model::base_estimate(&argv(a, 0))))),
        "takeoffSummary" => ok(Value::Array(
            export::takeoff_summary(&argv(a, 0), &model::base_estimate(&argv(a, 0)))
                .iter()
                .map(|x| json!({ "name": x.name, "csi": x.csi, "type": x.ty, "qty": n(x.qty), "unit": x.unit, "marks": n(x.marks), "sheets": n(x.sheets) }))
                .collect(),
        )),
        "byDivision" => ok(Value::Array(
            export::by_division(&argv(a, 0), &model::base_estimate(&argv(a, 0)))
                .iter()
                .map(|g| json!({ "div": g.div, "title": g.title, "mat": n(g.mat), "lab": n(g.lab), "total": n(g.total), "items": n(g.items) }))
                .collect(),
        )),
        "spotElevationsCsv" => ok(match export::spot_elevations_csv(&argv(a, 0), &str_arg(a, 1), &lab) {
            Ok((csv, nn)) => json!({ "ok": true, "csv": csv, "n": nn }),
            Err(why) => json!({ "ok": false, "why": why }),
        }),
        "estimateXlsx" => {
            let o = argv(a, 1);
            let compress = o.get("compress").is_none_or(|c| jv::truthy(Some(c)));
            let date = Stamp::from_ms(jv::to_number(o.get("now")));
            ok(bytes_json(&export::estimate_xlsx(&argv(a, 0), &lab, compress, &date)))
        }
        "writeXlsx" => {
            let o = argv(a, 1);
            let compress = o.get("compress").is_none_or(|c| jv::truthy(Some(c)));
            let date = Stamp::from_ms(jv::to_number(o.get("now")));
            ok(bytes_json(&xlsx::write_xlsx(&sheets_of(a.first()), &jv::to_string_or_empty(o.get("title")), compress, &date)))
        }
        "crc32" => ok(json!(xlsx::crc32(str_arg(a, 0).as_bytes()))),
        "colName" => ok(s(xlsx::col_name(usize_of(num_arg(a, 0, 0.0))))),
        "sheetName" => {
            let mut used: Vec<String> = strs(a.get(1));
            let nm = xlsx::sheet_name(&str_arg(a, 0), &mut used);
            ok(json!({ "name": nm, "used": used }))
        }
        "xmlText" => ok(s(xlsx::xml_text(&str_arg(a, 0)))),
        // ---- pdf set
        "sheetNumber" => ok(s(crate::pdf::sheet_number(&str_arg(a, 0), num_arg(a, 1, 0.0)))),
        "sheetTitle" => ok(s(crate::pdf::sheet_title(&jv::to_string(a.first().and_then(|f| f.get("name"))), a.get(1).unwrap_or(&Value::Null), jv::truthy(a.get(2))))),
        "matchFiles" => ok(crate::pdf::match_files(jv::arr(a.first()).map_or(&[][..], Vec::as_slice), jv::arr(a.get(1)).map_or(&[][..], Vec::as_slice))),
        _ => Err(format!("unknown function {f}")),
    }
}

fn i32_of(v: f64) -> i32 {
    #[allow(clippy::cast_possible_truncation)]
    let i = if v.is_finite() { v.clamp(-300.0, 300.0) as i32 } else { 0 };
    i
}

fn view_of(o: &Value) -> scene3d::View {
    let tr = jv::arr(o.get("transform")).map(|l| {
        let g = |i: usize| jv::num(l.get(i)).unwrap_or(f64::NAN);
        [g(0), g(1), g(2), g(3), g(4), g(5)]
    });
    scene3d::View {
        transform: tr,
        width: o.get("width").map_or(0.0, |v| jv::to_number(Some(v))),
        height: o.get("height").map_or(0.0, |v| jv::to_number(Some(v))),
        step: o.get("step").filter(|v| jv::truthy(Some(v))).map(|v| jv::to_number(Some(v))),
    }
}

/// pdf.js-style operator list from JSON: `[[fn, args], …]`.
pub fn snap_ops(v: Option<&Value>) -> Vec<snap::Op> {
    jv::arr(v)
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .map(|e| {
            let f = jv::num(e.get(0)).unwrap_or(-1.0);
            #[allow(clippy::cast_possible_truncation)]
            let fi = if f.is_finite() { f as i64 } else { -1 };
            let args = jv::arr(e.get(1)).cloned().unwrap_or_default();
            let parsed: Vec<snap::Arg> = if fi == snap::PDFJS_OPS.construct_path {
                let kind = jv::num(args.first()).unwrap_or(-1.0);
                #[allow(clippy::cast_possible_truncation)]
                let ki = if kind.is_finite() { kind as i64 } else { -1 };
                // pdf.js hands the path data as a Float32Array: the coordinates are rounded to f32
                let d: Vec<f64> = args
                    .get(1)
                    .and_then(|x| jv::arr(Some(x)))
                    .and_then(|x| x.first())
                    .and_then(|x| jv::arr(Some(x)))
                    .map(|l| {
                        l.iter()
                            .map(|x| {
                                #[allow(clippy::cast_possible_truncation)]
                                let f = jv::to_number(Some(x)) as f32;
                                f64::from(f)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                vec![snap::Arg::Path(ki, d)]
            } else {
                args.iter()
                    .map(|x| match x {
                        Value::Number(v) => snap::Arg::Num(v.as_f64().unwrap_or(f64::NAN)),
                        Value::Array(l) => snap::Arg::List(l.iter().map(|y| jv::to_number(Some(y))).collect()),
                        _ => snap::Arg::Other,
                    })
                    .collect()
            };
            snap::Op { f: fi, args: parsed }
        })
        .collect()
}

/// One record of a model operation.
fn record(t: &Takeoff, ret: Result<Value, String>, full: bool) -> Value {
    let mut o = Map::new();
    match ret {
        Ok(v) => {
            o.insert("ret".into(), v);
        }
        Err(e) => {
            o.insert("err".into(), s(e));
        }
    }
    if full {
        o.insert("project".into(), t.to_json("T"));
        let q = model::quantities(&t.state, &QOpts::default());
        o.insert("est".into(), estimate_json(&model::estimate(&t.state, &q)));
        o.insert("q".into(), q.to_json(&t.state));
    }
    o.insert("version".into(), n(t.version));
    o.insert("dirty".into(), Value::Bool(t.dirty()));
    o.insert("undo".into(), json!(t.undo_stack.iter().map(|x| x.label.clone()).collect::<Vec<_>>()));
    o.insert("redo".into(), json!(t.redo_stack.len()));
    Value::Object(o)
}

fn opt_label(a: &Value) -> Option<String> {
    a.get("label").filter(|l| !l.is_null()).map(|l| jv::to_string(Some(l)))
}

/// Run one model operation on the takeoff.
fn model_op(t: &mut Takeoff, st: &Value) -> Result<Value, String> {
    let op = jv::str(st.get("op")).unwrap_or("");
    let g = |k: &str| st.get(k);
    let gs = |k: &str| jv::to_string_or_empty(st.get(k));
    let go = |k: &str| jv::obj(st.get(k)).cloned().unwrap_or_default();
    let label = opt_label(st);
    let lb = label.as_deref();
    let b = Value::Bool;
    let id_or_null = |r: Option<String>| r.map_or(Value::Null, s);
    match op {
        "new" => {
            *t = Takeoff::default();
            Ok(Value::Null)
        }
        "load" => Ok(match t.load(&g("json").cloned().unwrap_or(Value::Null)) {
            Ok(w) => json!({ "ok": true, "warnings": w }),
            Err(e) => json!({ "ok": false, "error": e }),
        }),
        "addFile" => Ok(b(t.add_file(&go("f")))),
        "setLevel" => t.set_level(&gs("sheet"), g("elev")).map(b),
        "setLevels" => t.set_levels(&go("map")).map(b),
        "setScales" => t.set_scales(&go("map"), lb).map(b),
        "setScale" => t.set_scale(&gs("sheet"), g("scale").unwrap_or(&Value::Null), lb).map(b),
        "addItem" => t.add_item(&go("fields")).map(id_or_null),
        "addItems" => t.add_items(jv::arr(g("list")).map_or(&[][..], Vec::as_slice), lb).map(|ids| json!(ids)),
        "updateItems" => {
            let p: Vec<(String, Obj)> = jv::arr(g("patches"))
                .map_or(&[][..], Vec::as_slice)
                .iter()
                .map(|x| (jv::to_string(x.get("id")), jv::obj(x.get("patch")).cloned().unwrap_or_default()))
                .collect();
            t.update_items(&p, lb).map(b)
        }
        "updateItem" => t.update_item(&gs("id"), &go("patch"), lb).map(b),
        "setCond" => t.set_cond(&gs("id"), &gs("kind"), jv::truthy(g("on"))).map(b),
        "updateCond" => t.update_cond(&gs("id"), &gs("kind"), &go("patch"), lb).map(b),
        "removeItem" => t.remove_item(&gs("id")).map(b),
        "moveItem" => t.move_item(&gs("id"), jv::to_number(g("dir"))).map(b),
        "addComponent" => t.add_component(&gs("item"), &go("fields")).map(id_or_null),
        "updateComponent" => t.update_component(&gs("item"), &gs("comp"), &go("patch")).map(b),
        "removeComponent" => t.remove_component(&gs("item"), &gs("comp")).map(b),
        "addMark" => t.add_mark(&go("m")).map(id_or_null),
        "addMarks" => {
            let kind = g("kind").filter(|k| jv::truthy(Some(k))).map(|k| jv::to_string(Some(k)));
            t.add_marks(&gs("item"), jv::arr(g("list")).map_or(&[][..], Vec::as_slice), lb, kind.as_deref()).map(|ids| json!(ids))
        }
        "updateMark" => t.update_mark(&gs("id"), &go("patch"), lb).map(b),
        "removeMarks" => t.remove_marks(&strs(g("ids"))).map(b),
        "setBid" => t.set_bid(&go("patch"), lb).map(b),
        "addNote" => t.add_note(&go("fields")).map(id_or_null),
        "updateNote" => t.update_note(&gs("id"), &go("patch"), lb).map(b),
        "removeNote" => t.remove_note(&gs("id")).map(b),
        "addGroup" => t.add_group(g("name").or(Some(&Value::Null)), g("repeat"), &strs(g("ids"))).map(id_or_null),
        "updateGroup" => t.update_group(&gs("id"), g("name"), g("repeat")).map(b),
        "removeGroup" => t.remove_group(&gs("id")).map(b),
        "setMarksGroup" => {
            let gr = g("group").filter(|v| jv::truthy(Some(v))).map(|v| jv::to_string(Some(v)));
            t.set_marks_group(&strs(g("ids")), gr.as_deref(), lb).map(b)
        }
        "addArea" => t.add_area(g("name").or(Some(&Value::Null))).map(id_or_null),
        "renameArea" => t.rename_area(&gs("id"), g("name").or(Some(&Value::Null))).map(b),
        "setAreaAlt" => {
            let alt = g("alt").filter(|v| !v.is_null()).map(|v| jv::to_string(Some(v)));
            t.set_area_alt(&gs("id"), alt.as_deref()).map(b)
        }
        "removeArea" => t.remove_area(&gs("id")).map(b),
        "moveArea" => t.move_area(&gs("id"), jv::to_number(g("dir"))).map(b),
        "setMarksArea" => {
            let ar = g("area").filter(|v| jv::truthy(Some(v))).map(|v| jv::to_string(Some(v)));
            t.set_marks_area(&strs(g("ids")), ar.as_deref(), lb).map(b)
        }
        "carryMarks" => {
            let mx = m6(g("A"));
            let mv = |pts: &[Value]| -> Vec<Value> {
                let ps: Vec<P> = pts.iter().map(|p| p_of(Some(p))).collect();
                overlay::move_pts(&ps, &mx).iter().map(pj).collect()
            };
            let flagged: HashSet<String> = strs(g("flagged")).into_iter().collect();
            t.carry_marks(&strs(g("ids")), &gs("to"), &mv, &flagged, g("scale").filter(|v| !v.is_null()), lb).map(n)
        }
        "clearFlags" => t.clear_flags(&strs(g("ids"))).map(b),
        "cut" => {
            let ids: HashSet<String> = strs(g("ids")).into_iter().collect();
            t.change(&label.clone().unwrap_or_else(|| "Cut mark".into()), |s| {
                if let Value::Object(o) = s
                    && let Some(Value::Array(l)) = o.get_mut("marks")
                {
                    l.retain(|m| !jv::str(m.get("id")).is_some_and(|i| ids.contains(i)));
                }
                Ok(model::Ch::Commit(()))
            })
            .map(|r| b(r.is_some()))
        }
        "paste" => {
            let rc = clipboard::read_clip(g("wire").unwrap_or(&Value::Null))?;
            let o = g("opts").cloned().unwrap_or(Value::Null);
            let area = match o.get("area") {
                None => None,
                Some(Value::Null) => Some(None),
                Some(v) => Some(Some(jv::to_string(Some(v)))),
            };
            let opts = PasteOpts {
                sheet: o.get("sheet").filter(|v| !v.is_null()).map(|v| jv::to_string(Some(v))),
                dx: o.get("dx").map_or(0.0, |v| jv::to_number(Some(v))),
                dy: o.get("dy").map_or(0.0, |v| jv::to_number(Some(v))),
                label: o.get("label").filter(|v| !v.is_null()).map_or_else(|| "Paste".to_string(), |v| jv::to_string(Some(v))),
                area,
            };
            clipboard::paste_clip(t, &rc.clip, &opts)
                .map(|r| r.map_or(Value::Bool(false), |p| json!({ "marks": p.marks, "items": p.items, "reused": p.reused })))
        }
        "undo" => Ok(t.undo().map_or(Value::Null, s)),
        "redo" => Ok(t.redo().map_or(Value::Null, s)),
        "markSaved" => {
            t.mark_saved();
            Ok(Value::Null)
        }
        _ => Err(format!("unknown op {op}")),
    }
}

/// Queries on the current takeoff (no state change; no project record).
fn query(t: &Takeoff, st: &Value, labels: &Value) -> Option<Result<Value, String>> {
    let op = jv::str(st.get("op")).unwrap_or("");
    let state = &t.state;
    let qo = |v: Option<&Value>| -> QOpts {
        let area = v.and_then(|o| o.as_object()).and_then(|o| {
            if o.contains_key("area") { Some(o.get("area").filter(|a| !a.is_null()).map(|a| jv::to_string(Some(a)))) } else { None }
        });
        let exclude =
            v.and_then(|o| o.get("exclude")).and_then(|e| jv::arr(Some(e))).map(|l| l.iter().map(|x| jv::to_string(Some(x))).collect());
        QOpts { area, exclude }
    };
    let r = match op {
        "quantities" => Ok(model::quantities(state, &qo(st.get("opts"))).to_json(state)),
        "estimate" => Ok(model::estimate(state, &model::quantities(state, &qo(st.get("opts")))).to_json()),
        "baseEstimate" => Ok(model::base_estimate(state).to_json()),
        "areaEstimates" => Ok(Value::Array(
            model::area_estimates(state)
                .into_iter()
                .map(|(a, e)| json!({ "area": a.unwrap_or(Value::Null), "est": e.to_json() }))
                .collect(),
        )),
        "alternates" => Ok(Value::Array(model::alternates(state).iter().map(model::Alternate::to_json).collect())),
        "sheetBreakdown" => Ok(Value::Array(model::sheet_breakdown(state, &model::quantities(state, &QOpts::default())))),
        "backouts" => {
            let bo = model::Backouts::new(state);
            Ok(json!({ "bad": bo.bad, "any": bo.any() }))
        }
        "bidSummary" => {
            let est = model::base_estimate(state);
            let alts: Vec<bid::AltIn> = model::alternates(state).iter().map(model::Alternate::bid_in).collect();
            Ok(bid_summary(est.mat_total, est.lab_total, est.incomplete, state.get("bid"), &alts).to_json())
        }
        "itemVars" => {
            let id = jv::to_string_or_empty(st.get("id"));
            let q = model::quantities(state, &QOpts::default());
            let (Some(it), Some(qq)) = (t.item(&id), q.get(&id)) else { return Some(Ok(Value::Null)) };
            let mut o = Map::new();
            for (k, v) in model::item_vars(it, &qq.tot) {
                o.insert(k, n(v));
            }
            Ok(Value::Object(o))
        }
        "measure" => {
            let id = jv::to_string_or_empty(st.get("id"));
            let Some(m) = t.mark(&id) else { return Some(Ok(Value::Null)) };
            let sheet = jv::to_string(m.get("sheet"));
            let fpu = t.scale_of(&sheet).map_or(f64::NAN, |s| jv::to_number(s.get("ftPerUnit")));
            let it = t.item(&jv::to_string(m.get("item")));
            Ok(model::measure_mark(m, fpu, it).map_or(Value::Null, |x| x.to_json()))
        }
        "state" => Ok(state.clone()),
        "exports" => {
            let lab = labeler(labels);
            let est = model::base_estimate(state);
            Ok(json!({
                "csv": export::estimate_csv(state, &est),
                "sheetsCsv": export::sheets_csv(state, &lab),
                "materialCsv": export::material_csv(state, &est),
                "sheets": export::sheets_json(&export::estimate_sheets(state, &lab, &est)),
            }))
        }
        "xlsx" => {
            let lab = labeler(labels);
            let compress = st.get("compress").is_none_or(|c| jv::truthy(Some(c)));
            let date = Stamp::from_ms(jv::to_number(st.get("now")));
            Ok(bytes_json(&export::estimate_xlsx(state, &lab, compress, &date)))
        }
        _ => return None,
    };
    Some(r)
}

/// Run a script: `{ labels?, steps: [ {op, …} | {fn, args} ] }` → `{ records: [...] }`.
pub fn run(script: &Value) -> Value {
    let labels = script.get("labels").cloned().unwrap_or(Value::Null);
    let mut t = Takeoff::default();
    let mut records = Vec::new();
    for st in jv::arr(script.get("steps")).map_or(&[][..], Vec::as_slice) {
        if let Some(f) = jv::str(st.get("fn")) {
            let args = jv::arr(st.get("args")).cloned().unwrap_or_default();
            records.push(match call(f, &args, &labels) {
                Ok(v) => json!({ "ret": v }),
                Err(e) => json!({ "err": e }),
            });
            continue;
        }
        if let Some(r) = query(&t, st, &labels) {
            records.push(match r {
                Ok(v) => json!({ "ret": v }),
                Err(e) => json!({ "err": e }),
            });
            continue;
        }
        let full = !jv::truthy(st.get("light"));
        let r = model_op(&mut t, st);
        records.push(record(&t, r, full));
    }
    json!({ "records": records })
}
