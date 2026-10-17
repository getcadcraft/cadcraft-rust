//! Stage-4 ops for the call-script runner (mirrors `forks/tools/pcdiff/ops-stage4.mjs`): assemblies, exploded views,
//! kernel plane frames on kernel results passed as data.

use crate::asm::{self, Build, Builds};
use crate::explode::{self, Box3};
use crate::js;
use crate::jv;
use crate::script::num_arg;
use crate::vec::{M4, V3};
use serde_json::Value;
use std::collections::HashSet;

fn arg(a: &[Value], i: usize) -> &Value {
    a.get(i).unwrap_or(&Value::Null)
}
fn s_arg(a: &[Value], i: usize) -> String {
    js::str_of(a.get(i))
}
fn p3(v: Option<&Value>) -> V3 {
    let a = jv::arr(v);
    [num_arg(a.first()), num_arg(a.get(1)), num_arg(a.get(2))]
}
fn m4(v: Option<&Value>) -> M4 {
    let a = jv::arr(v);
    let mut m = [f64::NAN; 16];
    for (i, x) in m.iter_mut().enumerate() {
        *x = num_arg(a.get(i));
    }
    m
}
fn build_arg(v: &Value) -> Option<Build> {
    (!v.is_null()).then(|| Build::from_json(v))
}
fn builds_arg(v: &Value) -> Builds {
    v.as_object().map(|o| o.iter().map(|(k, b)| (k.clone(), Build::from_json(b))).collect()).unwrap_or_default()
}
fn world_arg(v: &Value) -> Vec<(String, M4)> {
    v.as_object().map(|o| o.iter().map(|(k, m)| (k.clone(), m4(Some(m)))).collect()).unwrap_or_default()
}
fn key_arg(a: &[Value], i: usize) -> Option<String> {
    a.get(i).and_then(Value::as_str).map(str::to_string)
}
fn with_asm(asm: Value, r: Result<Value, String>) -> Result<Value, String> {
    r.map(|x| jv::obj(vec![("res", x), ("asm", asm)]))
}
fn lim_json(l: Option<(f64, f64)>) -> Value {
    l.map_or(Value::Null, |(a, b)| jv::obj(vec![("min", jv::num(a)), ("max", jv::num(b))]))
}

/// The stage-4 ops.
pub fn handle(op: &str, a: &[Value]) -> Option<Result<Value, String>> {
    let r = match op {
        "asm.new" => Ok(asm::new_assembly(a.first().and_then(Value::as_str).unwrap_or("Assembly1"))),
        "asm.insertPart" => {
            let mut x = arg(a, 0).clone();
            let r = asm::insert_part(&mut x, a.get(1).filter(|p| !p.is_null()), a.get(2), a.get(3).and_then(Value::as_str));
            with_asm(x, r)
        }
        "asm.addJoint" => {
            let mut x = arg(a, 0).clone();
            let r = asm::add_joint(&mut x, &s_arg(a, 1), arg(a, 2), arg(a, 3), arg(a, 4));
            with_asm(x, r)
        }
        "asm.removeInstance" => {
            let mut x = arg(a, 0).clone();
            asm::remove_instance(&mut x, &s_arg(a, 1));
            Ok(x)
        }
        "asm.rigidInverse" => Ok(jv::nums(&asm::rigid_inverse(&m4(a.first())))),
        "asm.axesOf" => Ok(Value::Array(
            asm::axes_of(build_arg(arg(a, 0)).as_ref(), arg(a, 1))
                .iter()
                .map(|x| {
                    let mut o = jv::obj(vec![("key", x.key.as_str().into()), ("feature", x.feature.as_str().into())]);
                    if let Some(i) = x.index {
                        jv::set(&mut o, "index", jv::unum(i));
                    }
                    jv::set(&mut o, "o", jv::pt3(x.o));
                    jv::set(&mut o, "d", jv::pt3(x.d));
                    jv::set(&mut o, "label", x.label.as_str().into());
                    o
                })
                .collect(),
        )),
        "asm.mateFrame" => asm::mate_frame(arg(a, 0), build_arg(arg(a, 1)).as_ref(), arg(a, 2)).map(|m| jv::nums(&m)),
        "asm.planeFrame" => {
            asm::plane_frame(arg(a, 0), build_arg(arg(a, 1)).as_ref(), arg(a, 2)).map(|f| f.map_or(Value::Null, |f| f.to_json()))
        }
        "asm.faceRefAt" => match build_arg(arg(a, 0)) {
            Some(b) => Ok(asm::face_ref_at(&b, &s_arg(a, 1), p3(a.get(2)), p3(a.get(3)))),
            None => Err("Cannot read properties of undefined (reading 'candidates')".into()),
        },
        "asm.limitOf" => Ok(lim_json(asm::limit_of(arg(a, 0), key_arg(a, 1).as_deref()))),
        "asm.jointValue" => Ok(jv::num(asm::joint_value(arg(a, 0), key_arg(a, 1).as_deref()))),
        "asm.jointMotion" => Ok(jv::nums(&asm::joint_motion(arg(a, 0)))),
        "asm.placeAll" => {
            let (w, p) = asm::place_all(arg(a, 0), &builds_arg(arg(a, 1)));
            Ok(jv::obj(vec![("world", asm::world_json(&w)), ("problems", Value::Array(p))]))
        }
        "asm.jointTree" => Ok(Value::Array(asm::joint_tree(arg(a, 0), &builds_arg(arg(a, 1)), &world_arg(arg(a, 2))))),
        "asm.freeIds" => Ok(jv::strs(&asm::free_ids(arg(a, 0)))),
        "asm.movePlace" => {
            let o = arg(a, 1);
            let mv = o.get("move").filter(|x| jv::truthy(Some(x))).map(|x| p3(Some(x)));
            let rot =
                o.get("rotate").filter(|x| jv::truthy(Some(x))).map(|r| (p3(r.get("axis")), num_arg(r.get("angle")), p3(r.get("about"))));
            asm::move_place(&m4(a.first()), mv, rot).map(|m| jv::nums(&m))
        }
        "asm.moveInstance" => {
            let mut x = arg(a, 0).clone();
            asm::move_instance(&mut x, &s_arg(a, 1), arg(a, 2)).map(|()| x)
        }
        "asm.filterClashes" => {
            let (s, h) = asm::filter_clashes(arg(a, 0), jv::arr(a.get(1)));
            Ok(jv::obj(vec![("shown", Value::Array(s)), ("hidden", Value::Array(h))]))
        }
        "asm.setKindHidden" => {
            let mut x = arg(a, 0).clone();
            asm::set_kind_hidden(&mut x, &s_arg(a, 1), jv::truthy(a.get(2)));
            Ok(x)
        }
        "asm.setPairHidden" => {
            let mut x = arg(a, 0).clone();
            asm::set_pair_hidden(&mut x, &s_arg(a, 1), &s_arg(a, 2), jv::truthy(a.get(3)));
            Ok(x)
        }
        "asm.readClashFilter" => {
            let ids: HashSet<String> = jv::arr(a.get(1)).iter().map(|x| js::string_of(Some(x))).collect();
            asm::read_clash_filter(a.first(), &ids).map(|x| x.unwrap_or(Value::Null))
        }
        "asm.read" => asm::read_assembly(arg(a, 0)),
        "asm.readLimits" => asm::read_limits(arg(a, 0), &s_arg(a, 1)).map(|x| x.unwrap_or(Value::Null)),
        "asm.fileVersion" => Ok(jv::num(asm::file_version(arg(a, 0)))),
        "asm.toJSON" => Ok(asm::to_json(arg(a, 0)).into()),
        "explode.newView" => {
            let mut x = arg(a, 0).clone();
            let r = explode::new_view(&mut x, a.get(1).and_then(Value::as_str));
            with_asm(x, Ok(r))
        }
        "explode.addStep" => {
            let mut x = arg(a, 0).clone();
            let insts: Vec<String> = jv::arr(a.get(2)).iter().map(|v| js::string_of(Some(v))).collect();
            let r = explode::add_step(&mut x, &s_arg(a, 1), &insts, p3(a.get(3)), num_arg(a.get(4)));
            with_asm(x, r)
        }
        "explode.offsets" => {
            let t = match a.get(1) {
                None | Some(Value::Null) => 1.0,
                v => num_arg(v),
            };
            let mut o = jv::Obj::new();
            for (k, v) in explode::offsets(a.first().filter(|x| !x.is_null()), t) {
                o.insert(k, jv::pt3(v));
            }
            Ok(Value::Object(o))
        }
        "explode.exploded" => {
            let offs: Vec<(String, V3)> =
                arg(a, 1).as_object().map(|o| o.iter().map(|(k, v)| (k.clone(), p3(Some(v)))).collect()).unwrap_or_default();
            Ok(asm::world_json(&explode::exploded(&world_arg(arg(a, 0)), &offs)))
        }
        "explode.trails" => {
            let centres: Vec<(String, V3)> =
                arg(a, 1).as_object().map(|o| o.iter().map(|(k, v)| (k.clone(), p3(Some(v)))).collect()).unwrap_or_default();
            let t = match a.get(2) {
                None | Some(Value::Null) => 1.0,
                v => num_arg(v),
            };
            Ok(Value::Array(
                explode::trails(a.first().filter(|x| !x.is_null()), &centres, t)
                    .into_iter()
                    .map(|(id, pts)| jv::obj(vec![("inst", id.into()), ("pts", Value::Array(pts.iter().map(|p| jv::pt3(*p)).collect()))]))
                    .collect(),
            ))
        }
        "explode.autoSteps" => {
            let items: Vec<(String, Box3)> = jv::arr(a.first())
                .iter()
                .map(|i| {
                    (
                        js::string_of(i.get("id")),
                        Box3 { min: p3(i.get("box").and_then(|b| b.get("min"))), max: p3(i.get("box").and_then(|b| b.get("max"))) },
                    )
                })
                .collect();
            let o = arg(a, 2);
            let tree: Vec<(String, String, V3)> = jv::arr(o.get("tree"))
                .iter()
                .map(|e| (js::string_of(e.get("parent")), js::string_of(e.get("child")), p3(e.get("normal"))))
                .collect();
            let factor = match o.get("factor") {
                None | Some(Value::Null) => 1.25,
                v => num_arg(v),
            };
            let mode = a.get(1).and_then(Value::as_str).unwrap_or("radial");
            explode::auto_steps(&items, mode, &tree, factor).map(|s| {
                Value::Array(
                    s.iter()
                        .map(|x| jv::obj(vec![("insts", jv::strs(&x.insts)), ("dir", jv::pt3(x.dir)), ("dist", jv::num(x.dist))]))
                        .collect(),
                )
            })
        }
        "parts.families" => Ok(crate::shapes_parts::families_json()),
        "parts.sizes" => Ok(Value::Array(crate::shapes_parts::part_sizes(&s_arg(a, 0)))),
        "parts.options" => Ok(crate::shapes_parts::part_options(&s_arg(a, 0), &s_arg(a, 1))),
        "parts.make" => Ok(crate::shapes_parts::make_part(
            &s_arg(a, 0),
            arg(a, 1),
            a.get(2).filter(|x| !x.is_null()).unwrap_or(&Value::Object(jv::Obj::new())),
        )),
        "std.templates" => Ok(crate::stdparts::templates_json()),
        "std.sizes" => crate::stdparts::template_sizes(&s_arg(a, 0)),
        "std.make" => crate::stdparts::make_template(&s_arg(a, 0), a.get(1).and_then(Value::as_str)),
        "clip.copySketch" => {
            let ids: Vec<String> = jv::arr(a.get(1)).iter().map(|v| js::string_of(Some(v))).collect();
            Ok(crate::clipboard::copy_sketch(arg(a, 0), &ids).unwrap_or(Value::Null))
        }
        "clip.pasteSketch" => {
            let mut sk = arg(a, 0).clone();
            let d = jv::arr(a.get(2));
            let dd = if a.get(2).is_none_or(Value::is_null) { [0.0, 0.0] } else { [num_arg(d.first()), num_arg(d.get(1))] };
            let res = crate::clipboard::paste_sketch(&mut sk, arg(a, 1), dd);
            Ok(jv::obj(vec![("res", res), ("sk", sk)]))
        }
        "clip.copyFeature" => crate::clipboard::copy_feature(arg(a, 0), &s_arg(a, 1)),
        "clip.pasteFeatures" => {
            let mut pt = arg(a, 0).clone();
            crate::clipboard::paste_features(&mut pt, arg(a, 1)).map(|res| jv::obj(vec![("res", res), ("part", pt)]))
        }
        "clip.copyInstance" => Ok(crate::clipboard::copy_instance(arg(a, 0), &s_arg(a, 1))),
        "clip.pasteInstance" => {
            let mut x = arg(a, 0).clone();
            let r = crate::clipboard::paste_instance(&mut x, arg(a, 1), arg(a, 2));
            with_asm(x, r)
        }
        "clip.read" => crate::clipboard::read_clip(arg(a, 0)),
        "clip.describe" => Ok(crate::clipboard::describe_clip(arg(a, 0)).into()),
        _ => return None,
    };
    Some(r)
}
