//! Stage-2 ops for the call-script runner (mirrors `forks/tools/pcdiff/ops-stage2.mjs`): the `.pcscript` reader and
//! writer, parameter uses, the sketch tools, curve intersection and the vector helpers.

use crate::js;
use crate::jv;
use crate::script::num_arg;
use crate::sketch_intersect as si;
use crate::sketch_tools as st;
use crate::spline::P2;
use crate::vec::{self as v3, Frame, M4, V3};
use crate::{params_uses, pcscript, sketch};
use serde_json::Value;

fn arg(a: &[Value], i: usize) -> &Value {
    a.get(i).unwrap_or(&Value::Null)
}
fn s_arg(a: &[Value], i: usize) -> String {
    js::str_of(a.get(i))
}
fn strs_arg(a: &[Value], i: usize) -> Vec<String> {
    jv::arr(a.get(i)).iter().map(|v| js::str_of(Some(v))).collect()
}
fn p2(v: &Value) -> P2 {
    let a = jv::arr(Some(v));
    [num_arg(a.first()), num_arg(a.get(1))]
}
fn v3a(v: &Value) -> V3 {
    let a = jv::arr(Some(v));
    [num_arg(a.first()), num_arg(a.get(1)), num_arg(a.get(2))]
}
fn m4(v: &Value) -> M4 {
    let a = jv::arr(Some(v));
    let mut m = [f64::NAN; 16];
    for (i, x) in m.iter_mut().enumerate() {
        *x = num_arg(a.get(i));
    }
    m
}
fn m4_json(m: &M4) -> Value {
    jv::nums(m)
}
fn with_sk(sk: Value, res: Result<Value, String>) -> Result<Value, String> {
    res.map(|r| jv::obj(vec![("res", r), ("sk", sk)]))
}
fn curve(sk: &Value, id: &str) -> Result<Option<si::Curve>, String> {
    match sketch::by_id(sk, id) {
        Some(e) => si::curve_of(sk, &e),
        None => Err("Cannot read properties of null (reading 'type')".into()),
    }
}

/// The stage-2 ops.
pub fn handle(op: &str, a: &[Value]) -> Option<Result<Value, String>> {
    let r = match op {
        "pcscript.read" => {
            let files = arg(a, 1).as_object().cloned();
            let rf = |p: &str| -> Result<String, String> {
                match files.as_ref().and_then(|f| f.get(p)) {
                    Some(v) => Ok(js::string_of(Some(v))),
                    None => Err("no such file".into()),
                }
            };
            let text = js::string_of(a.first());
            let res = if files.is_some() { pcscript::script_to_part(&text, Some(&rf)) } else { pcscript::script_to_part(&text, None) };
            Ok(match res {
                Ok(r) => jv::obj(vec![
                    ("part", r.part),
                    (
                        "warnings",
                        Value::Array(
                            r.warnings
                                .iter()
                                .map(|w| match w.line {
                                    Some(l) => jv::obj(vec![("line", jv::unum(l)), ("message", w.message.as_str().into())]),
                                    None => jv::obj(vec![("message", w.message.as_str().into())]),
                                })
                                .collect(),
                        ),
                    ),
                ]),
                Err(e) => e.to_json(),
            })
        }
        "pcscript.write" => {
            let o = arg(a, 1);
            let (text, files) = pcscript::part_to_script(
                arg(a, 0),
                jv::truthy(o.get("exact")),
                jv::truthy(o.get("inlineStep")),
                o.get("comment").is_none_or(|c| !matches!(c, Value::Bool(false))),
            );
            Ok(jv::obj(vec![
                ("text", text.into()),
                ("files", Value::Array(files.into_iter().map(|(n, t)| jv::obj(vec![("name", n.into()), ("text", t.into())])).collect())),
            ]))
        }
        "pcscript.looks" => Ok(pcscript::looks_like_script(&js::string_of(a.first())).into()),
        "uses.scriptParams" => {
            let (names, all) = params_uses::script_params(&js::string_of(a.first()));
            Ok(jv::obj(vec![("names", jv::strs(&names)), ("all", all.into())]))
        }
        "uses.paramUses" => {
            let u = params_uses::param_uses(arg(a, 0));
            let mut o = jv::Obj::new();
            for (k, x) in u.iter() {
                let mut j = x.to_json();
                jv::set(&mut j, "count", jv::unum(x.count()));
                o.insert(k.to_string(), j);
            }
            Ok(Value::Object(o))
        }
        "tools.crossings" => st::crossings(arg(a, 0), &s_arg(a, 1)).map(|(_, cuts, (lo, hi, per))| {
            jv::obj(vec![
                (
                    "cuts",
                    Value::Array(
                        cuts.iter()
                            .map(|c| jv::obj(vec![("t", jv::num(c.t)), ("p", jv::pt2(c.p)), ("by", c.by.as_str().into())]))
                            .collect(),
                    ),
                ),
                ("range", jv::obj(vec![("lo", jv::num(lo)), ("hi", jv::num(hi)), ("periodic", per.into())])),
            ])
        }),
        "tools.trimPiece" => Ok(st::trim_piece(arg(a, 0), &s_arg(a, 1), p2(arg(a, 2))).map_or(Value::Null, |(x, b, hi, period)| {
            jv::obj(vec![("a", jv::num(x)), ("b", jv::num(b)), ("hi", jv::num(hi)), ("period", jv::num(period))])
        })),
        "tools.trim" | "tools.extend" => {
            let mut sk = arg(a, 0).clone();
            let r = if op == "tools.trim" {
                st::trim(&mut sk, &s_arg(a, 1), p2(arg(a, 2)))
            } else {
                st::extend(&mut sk, &s_arg(a, 1), p2(arg(a, 2)))
            };
            with_sk(sk, r.map(|m| m.to_json()))
        }
        "tools.chainOf" => {
            let ids = arg(a, 2).is_array().then(|| strs_arg(a, 2));
            st::chain_of(arg(a, 0), &s_arg(a, 1), ids.as_deref()).map(|c| c.to_json())
        }
        "tools.offsetSide" => st::offset_side(arg(a, 0), &st::Chain::from_json(arg(a, 1)), p2(arg(a, 2)))
            .map(|b| b.map_or(Value::Null, |(d, side)| jv::obj(vec![("d", jv::num(d)), ("side", jv::num(side))]))),
        "tools.offset" => {
            let mut sk = arg(a, 0).clone();
            let link = a.get(3).and_then(Value::as_str).map(str::to_string);
            let r = st::offset(&mut sk, &st::Chain::from_json(arg(a, 1)), num_arg(a.get(2)), link.as_deref());
            with_sk(sk, r.map(|m| m.to_json()))
        }
        "tools.mirror" => {
            let mut sk = arg(a, 0).clone();
            let r = st::mirror(&mut sk, &strs_arg(a, 1), &s_arg(a, 2));
            with_sk(sk, r.map(|(made, rel)| jv::obj(vec![("made", jv::strs(&made)), ("dropped", 0.into()), ("relations", jv::unum(rel))])))
        }
        "tools.patternLinear" | "tools.patternCircular" => {
            let mut sk = arg(a, 0).clone();
            let r = if op == "tools.patternLinear" {
                st::pattern_linear(&mut sk, &strs_arg(a, 1), p2(arg(a, 2)), num_arg(a.get(3)))
            } else {
                let ang = match a.get(4) {
                    None | Some(Value::Null) => 360.0,
                    v => num_arg(v),
                };
                st::pattern_circular(&mut sk, &strs_arg(a, 1), p2(arg(a, 2)), num_arg(a.get(3)), ang)
            };
            with_sk(sk, r.map(|made| jv::obj(vec![("made", jv::strs(&made)), ("dropped", 0.into())])))
        }
        "tools.addPolygon" => {
            let mut sk = arg(a, 0).clone();
            let r = st::add_polygon(&mut sk, p2(arg(a, 1)), p2(arg(a, 2)), num_arg(a.get(3)));
            with_sk(sk, r)
        }
        "tools.addSlot" => {
            let mut sk = arg(a, 0).clone();
            let r = st::add_slot(&mut sk, p2(arg(a, 1)), p2(arg(a, 2)), num_arg(a.get(3)));
            with_sk(sk, r)
        }
        "tools.projectEdge" => {
            let mut sk = arg(a, 0).clone();
            let r = st::project_edge(&mut sk, arg(a, 1), a.get(2).filter(|v| !v.is_null()));
            with_sk(sk, r)
        }
        "isect.intersect" => match (curve(arg(a, 0), &s_arg(a, 1)), curve(arg(a, 0), &s_arg(a, 2))) {
            (Ok(Some(x)), Ok(Some(y))) => Ok(Value::Array(
                si::intersect(&x, &y, jv::truthy(a.get(3)), jv::truthy(a.get(4)))
                    .iter()
                    .map(|h| jv::obj(vec![("p", jv::pt2(h.p)), ("ta", jv::num(h.ta)), ("tb", jv::num(h.tb))]))
                    .collect(),
            )),
            (Err(e), _) | (_, Err(e)) => Err(e),
            _ => Err("Cannot read properties of null (reading 'kind')".into()),
        },
        "isect.probe" => match curve(arg(a, 0), &s_arg(a, 1)) {
            Err(e) => Err(e),
            Ok(None) => Ok(Value::Null),
            Ok(Some(c)) => {
                let (lo, hi, per) = si::range_of(&c);
                let ts: Vec<f64> = jv::arr(a.get(2)).iter().map(|x| num_arg(Some(x))).collect();
                Ok(jv::obj(vec![
                    ("range", jv::obj(vec![("lo", jv::num(lo)), ("hi", jv::num(hi)), ("periodic", per.into())])),
                    ("size", jv::num(si::size_of(&c))),
                    ("at", Value::Array(ts.iter().map(|t| jv::pt2(si::point_on(&c, *t))).collect())),
                    ("tan", Value::Array(ts.iter().map(|t| jv::pt2(si::tangent_on(&c, *t))).collect())),
                    ("params", Value::Array(jv::arr(a.get(3)).iter().map(|p| jv::num(si::param_of(&c, p2(p)))).collect())),
                ]))
            }
        },
        "isect.intersectCurves" => {
            let cv = |v: &Value| {
                let bs = v.get("bs").map(crate::script::curve_arg);
                si::curve_from_json(v, bs)
            };
            match (cv(arg(a, 0)), cv(arg(a, 1))) {
                (Some(x), Some(y)) => Ok(Value::Array(
                    si::intersect(&x, &y, jv::truthy(a.get(2)), jv::truthy(a.get(3)))
                        .iter()
                        .map(|h| jv::obj(vec![("p", jv::pt2(h.p)), ("ta", jv::num(h.ta)), ("tb", jv::num(h.tb))]))
                        .collect(),
                )),
                _ => Err("not a curve".into()),
            }
        }
        "isect.normAng" => Ok(jv::num(si::norm_ang(num_arg(a.first())))),
        "vec.frameFor" => Ok(v3::frame_for(v3a(arg(a, 0)), v3a(arg(a, 1))).to_json()),
        "vec.basePlanes" => {
            Ok(jv::obj(["front", "top", "right"].iter().filter_map(|n| v3::base_plane(n).map(|f| (*n, f.to_json()))).collect::<Vec<_>>()))
        }
        "vec.rotationAbout" => Ok(m4_json(&v3::rotation_about(v3a(arg(a, 0)), v3a(arg(a, 1)), num_arg(a.get(2))))),
        "vec.reflection" => Ok(m4_json(&v3::reflection(v3a(arg(a, 0)), v3a(arg(a, 1))))),
        "vec.matMul" => Ok(m4_json(&v3::mat_mul(&m4(arg(a, 0)), &m4(arg(a, 1))))),
        "vec.applyMat" => Ok(jv::pt3(v3::apply_mat(&m4(arg(a, 0)), v3a(arg(a, 1))))),
        "vec.applyDir" => Ok(jv::pt3(v3::apply_dir(&m4(arg(a, 0)), v3a(arg(a, 1))))),
        "vec.frameMatrix" => Ok(m4_json(&v3::frame_matrix(&Frame::from_json(arg(a, 0))))),
        "vec.toPlane" => Ok(jv::pt2(v3::to_plane(&Frame::from_json(arg(a, 0)), v3a(arg(a, 1))))),
        "vec.toWorld" => Ok(jv::pt3(v3::to_world(&Frame::from_json(arg(a, 0)), num_arg(a.get(1)), num_arg(a.get(2))))),
        "vec.flipFrame" => Ok(v3::flip_frame(&Frame::from_json(arg(a, 0))).to_json()),
        "vec.solveDense" => {
            let aa: Vec<Vec<f64>> =
                jv::arr(a.first()).iter().map(|r| jv::arr(Some(r)).iter().map(|x| num_arg(Some(x))).collect()).collect();
            let b: Vec<f64> = jv::arr(a.get(1)).iter().map(|x| num_arg(Some(x))).collect();
            Ok(v3::solve_dense(&aa, &b).map_or(Value::Null, |x| jv::nums(&x)))
        }
        "vec.fmt" => {
            let dp = match a.get(1) {
                None | Some(Value::Null) => 4,
                v => js::idx(num_arg(v)).unwrap_or(4),
            };
            Ok(v3::fmt(num_arg(a.first()), dp).into())
        }
        "json.parseError" => Ok(crate::v8json::parse_error(&js::string_of(a.first())).map_or(Value::Null, Value::String)),
        "vec.norm" => Ok(jv::pt3(v3::norm(v3a(arg(a, 0))))),
        _ => return None,
    };
    Some(r)
}
