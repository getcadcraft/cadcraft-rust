//! The call-script runner behind `cadcraft-cli detail <script.json>` and the parity harness
//! (`forks/tools/dcdiff.mjs`, whose JS driver mirrors every op here).
//!
//! A script is `{ "calls": [{ "op": "model.change", "args": [...], "as": "M", "self": "M" }, …] }`: `as` keeps the
//! call's object (a model, a build, marks …) under a name, `self` names the object a method runs on. Each call gives
//! `{ "ok": <the result's JSON form> }`, `{}` when the JS returns undefined, or `{ "err": "<message>" }`.
//! The registry and call types are cc-civil's (the same runner shape as `cadcraft-cli civil`).

use crate::build::{Built, build, built_json};
use crate::i18n::t0;
use crate::js::{J, Obj, arr, jnum, num, p3v, s};
use crate::marks::{Marks, bolt_list, bom_tables, mark_footings, mark_parts, marks_json};
use crate::model::Model;
use crate::profiles;
use crate::rebar::{self, Dims, Footing};
use crate::state;
pub use cc_civil::script::{Call, Registry};
use serde_json::{Value, json};

/// The JS `undefined` as a result (the runner writes `{}` for it).
pub const UNDEF: &str = "$undefined";
pub fn undef() -> Value {
    Value::String(UNDEF.into())
}

/// A handler: Some(result) when it knows the op.
pub type Handler = fn(&mut Registry, &Call) -> Option<Result<Value, String>>;
type R = Option<Result<Value, String>>;

/// Run a script through the handlers (tried in order) → the JSON array of results.
pub fn run_with(script: &Value, handlers: &[Handler]) -> Value {
    run_with_timer(script, handlers, None)
}

fn call_of(c: &Value) -> Call<'_> {
    Call {
        op: s(c.get("op")),
        args: arr(c.get("args")),
        as_name: c.get("as").and_then(Value::as_str),
        this: c.get("self").and_then(Value::as_str),
    }
}
fn dispatch(reg: &mut Registry, call: &Call, handlers: &[Handler]) -> Option<Result<Value, String>> {
    for h in handlers {
        let res = h(reg, call);
        if res.is_some() {
            return res;
        }
    }
    None
}

/// `run_with`, plus `bench.time [call, runs]` (best and median ms of `runs` runs of one call after a warm-up) when the
/// host gives a clock in ms (the CLI; the web build has none).
pub fn run_with_timer(script: &Value, handlers: &[Handler], clock: Option<&dyn Fn() -> f64>) -> Value {
    let mut reg = Registry::new();
    let mut out = Vec::new();
    for c in arr(script.get("calls")) {
        let call = call_of(c);
        let res = if call.op == "bench.time" {
            match (clock, call.args.first()) {
                (Some(clock), Some(inner)) => {
                    let ic = call_of(inner);
                    let runs = call.args.get(1).and_then(Value::as_u64).unwrap_or(5).clamp(1, 1000);
                    drop(dispatch(&mut reg, &ic, handlers));
                    let mut ms = Vec::new();
                    for _ in 0..runs {
                        let t0 = clock();
                        let r = dispatch(&mut reg, &ic, handlers);
                        ms.push(clock() - t0);
                        drop(r);
                    }
                    let best = ms.iter().copied().fold(f64::INFINITY, f64::min);
                    let mut sorted = ms.clone();
                    sorted.sort_by(f64::total_cmp);
                    let median = sorted.get(sorted.len() / 2).copied().unwrap_or(best);
                    Some(Ok(json!({ "best": best, "median": median, "runs": ms })))
                }
                _ => Some(Err("bench.time needs a clock and a call".into())),
            }
        } else {
            dispatch(&mut reg, &call, handlers)
        };
        out.push(match res {
            Some(Ok(Value::String(x))) if x == UNDEF => json!({}),
            Some(Ok(v)) => json!({ "ok": v }),
            Some(Err(e)) => json!({ "err": e }),
            None => json!({ "err": format!("unknown op {}", call.op) }),
        });
    }
    Value::Array(out)
}

/// Run a script with this crate's ops.
pub fn run(script: &Value) -> Value {
    run_with(script, &[handle, crate::script_edit::handle, crate::script_draw::handle])
}

/// The object a call names.
pub fn get<'a, T: 'static>(reg: &'a Registry, name: Option<&str>) -> Result<&'a T, String> {
    let n = name.ok_or_else(|| "no object named".to_string())?;
    reg.get::<T>(n).ok_or_else(|| format!("no object {n}"))
}
/// Keep `v` under the call's `as` name.
pub fn keep<T: 'static>(reg: &mut Registry, c: &Call, v: T) {
    if let Some(n) = c.as_name {
        reg.put(n, v);
    }
}
/// A state argument: a kept model's state (by name), or a state given inline.
pub fn state_arg(reg: &Registry, v: Option<&Value>) -> Result<Value, String> {
    match v {
        Some(Value::String(n)) => reg.get::<Model>(n).map(|m| m.state.clone()).ok_or_else(|| format!("no model {n}")),
        Some(x) => Ok(x.clone()),
        None => Err("no state".into()),
    }
}
/// A built argument: a kept build by name.
pub fn built_arg<'a>(reg: &'a Registry, v: Option<&Value>) -> Result<&'a Built, String> {
    let n = v.and_then(Value::as_str).ok_or_else(|| "no build named".to_string())?;
    reg.get::<Built>(n).ok_or_else(|| format!("no build {n}"))
}
fn ids_opt(v: Option<&Value>) -> Option<Vec<String>> {
    match v {
        None | Some(Value::Null) => None,
        Some(x) => Some(arr(Some(x)).iter().map(|v| crate::js::string_of(Some(v))).collect()),
    }
}
fn p3(v: Option<&Value>) -> [f64; 3] {
    let a = arr(v);
    [num(a.first()), num(a.get(1)), num(a.get(2))]
}
fn loop_of(v: Option<&Value>) -> Vec<[f64; 2]> {
    arr(v).iter().map(|p| [num(p.get(0)), num(p.get(1))]).collect()
}
fn loops_json(l: &[[f64; 2]]) -> Value {
    Value::Array(l.iter().map(|q| json!([jnum(q[0]), jnum(q[1])])).collect())
}
fn model_info(m: &Model) -> Value {
    json!({
        "dirty": m.dirty(), "canUndo": m.can_undo(), "canRedo": m.can_redo(),
        "undo": m.undo_stack.iter().map(|s| s.label.clone()).collect::<Vec<_>>(),
        "redo": m.redo_stack.iter().map(|s| s.label.clone()).collect::<Vec<_>>(),
    })
}
fn dims_of(v: Option<&Value>) -> Dims {
    let g = |k: &str| v.and_then(|d| d.get(k)).and_then(Value::as_f64);
    Dims { a: g("A").unwrap_or(f64::NAN), b: g("B"), c: g("C") }
}

/// This crate's ops.
pub fn handle(reg: &mut Registry, c: &Call) -> R {
    let (module, _) = c.op.split_once('.').unwrap_or((c.op, ""));
    match module {
        "profiles" => profiles_ops(c),
        "geom" => geom_ops(c),
        "model" => model_ops(reg, c),
        "build" | "built" => build_ops(reg, c),
        "marks" => marks_ops(reg, c),
        "conn" => conn_ops(reg, c),
        "rebar" => rebar_ops(c),
        "stair" => stair_ops(c),
        "csv" => csv_ops(c),
        "zip" => zip_ops(c),
        "sample" => sample_ops(reg, c),
        _ => None,
    }
}

fn profiles_ops(c: &Call) -> R {
    let custom = arr(c.arg(1));
    Some(Ok(match c.op {
        "profiles.getProfile" => match c.raw(0).and_then(|n| profiles::get_profile(n, custom)) {
            Some(p) => profiles::profile_json(&p, true),
            None => Value::Null,
        },
        "profiles.cleanCustom" => {
            let saved = c.arg(1).and_then(|o| o.get("saved")).is_some_and(|v| crate::js::truthy(Some(v)));
            match profiles::clean_custom(c.raw(0).unwrap_or(&Value::Null), saved) {
                Ok(p) => json!({ "ok": true, "profile": p }),
                Err(e) => json!({ "ok": false, "error": e }),
            }
        }
        "profiles.isLegacyName" => Value::from(profiles::is_legacy_name(&crate::js::string_of(c.raw(0)))),
        "profiles.catalogNames" => Value::Array(profiles::catalog_rows().iter().map(|p| Value::from(p.name.clone())).collect()),
        "profiles.catalogGroups" => Value::Array(
            profiles::catalog_groups(arr(c.arg(0)))
                .into_iter()
                .map(|g| json!({ "label": g.label, "source": g.source, "names": g.names }))
                .collect(),
        ),
        "profiles.describe" => match c.raw(0).and_then(|n| profiles::get_profile(n, custom)) {
            Some(p) => json!(profiles::describe(&p)),
            None => Value::Null,
        },
        "profiles.holeFaces" => json!(profiles::hole_faces(c.raw(0).and_then(|n| profiles::get_profile(n, custom)).as_deref())),
        "profiles.faceThickness" => match c.raw(0).and_then(|n| profiles::get_profile(n, &[])) {
            Some(p) => profiles::face_thickness(&p, c.str(1)).map_or_else(undef, jnum),
            None => Value::Null,
        },
        "profiles.plateName" => Value::from(profiles::plate_name(c.num(0), c.num(1))),
        _ => return None,
    }))
}

fn frame_of(c: &Call, i: usize) -> crate::geom::Frame {
    let f = c.arg(i);
    crate::geom::Frame {
        o: p3(f.and_then(|f| f.get("o"))),
        x: p3(f.and_then(|f| f.get("x"))),
        u: p3(f.and_then(|f| f.get("u"))),
        v: p3(f.and_then(|f| f.get("v"))),
        len: num(f.and_then(|f| f.get("len"))),
    }
}

fn geom_ops(c: &Call) -> R {
    use crate::geom;
    Some(Ok(match c.op {
        "geom.frame" => crate::build::frame_json(&geom::frame(p3(c.arg(0)), p3(c.arg(1)), c.arg(2).map_or(0.0, |v| num(Some(v))))),
        "geom.toWorld" => p3v(geom::to_world(&frame_of(c, 0), c.num(1), c.num(2), c.num(3), c.arg(4).map_or(0.0, |v| num(Some(v))))),
        "geom.toLocal" => {
            let l = geom::to_local(&frame_of(c, 0), p3(c.arg(1)), c.arg(2).map_or(0.0, |v| num(Some(v))));
            Obj::new().set("x", l.x).set("u", l.u).set("v", l.v).done()
        }
        "geom.clipLoopV" => {
            loops_json(&geom::clip_loop_v(&loop_of(c.arg(0)), c.num(1), c.arg(2).is_none_or(|v| crate::js::truthy(Some(v)))))
        }
        "geom.q" => jnum(geom::q(c.num(0), c.arg(1).map_or(1.0, |v| num(Some(v))))),
        "geom.voffFor" => jnum(geom::voff_for(c.str(0), c.num(1))),
        "geom.sectionBoxAt" => {
            Value::Array(geom::section_box_at(&frame_of(c, 0), c.num(1), c.num(2), c.num(3), c.num(4)).iter().map(|p| p3v(*p)).collect())
        }
        "geom.shoelace" => jnum(crate::sections::shoelace(&loop_of(c.arg(0)))),
        "geom.loopLength" => jnum(crate::sections::loop_length(&loop_of(c.arg(0)))),
        _ => return None,
    }))
}

fn model_ops(reg: &mut Registry, c: &Call) -> R {
    use crate::model as md;
    let r: Result<Value, String> = (|| -> Result<Value, String> {
        Ok(match c.op {
            "model.new" => {
                let st = match c.arg(0) {
                    Some(v) => v.clone(),
                    None => md::starter_state(&t0("model.name.untitled")),
                };
                keep(reg, c, Model::new(st));
                undef()
            }
            "model.starterState" => {
                md::starter_state(&c.arg(0).map_or_else(|| t0("model.name.untitled"), |v| crate::js::string_of(Some(v))))
            }
            "model.emptyState" => md::empty_state(&c.arg(0).map_or_else(|| t0("model.name.untitled"), |v| crate::js::string_of(Some(v)))),
            "model.change" => {
                let label = c.str(0).to_string();
                let ops = arr(c.arg(1)).to_vec();
                // the clips a `$paste` names, taken out of the registry before the model is borrowed
                let mut clips: Vec<(String, Value)> = Vec::new();
                for o in &ops {
                    if s(o.get("op")) == "$paste" {
                        let n = s(arr(o.get("args")).first()).to_string();
                        if let Some(k) = reg.get::<crate::script_edit::Clip>(&n) {
                            clips.push((n, k.0.clone()));
                        }
                    }
                }
                let name = c.this.ok_or("no model named")?;
                let m = reg.get_mut::<Model>(name).ok_or_else(|| format!("no model {name}"))?;
                m.change(&label, |tx| {
                    let mut res = Vec::new();
                    for o in &ops {
                        let op = s(o.get("op"));
                        let r = if op.starts_with('$') {
                            crate::script_edit::tx_op(tx, op, arr(o.get("args")), &clips)?
                        } else {
                            tx.call(op, arr(o.get("args")))?
                        };
                        res.push(r.unwrap_or(Value::Null));
                    }
                    Ok(Value::Array(res))
                })?
            }
            "model.state" => get::<Model>(reg, c.this)?.state.clone(),
            "model.toJSON" => Value::from(get::<Model>(reg, c.this)?.to_json()),
            "model.info" => model_info(get::<Model>(reg, c.this)?),
            "model.undo" => {
                let name = c.this.ok_or("no model named")?;
                let m = reg.get_mut::<Model>(name).ok_or_else(|| format!("no model {name}"))?;
                m.undo().map_or(Value::Null, Value::from)
            }
            "model.redo" => {
                let name = c.this.ok_or("no model named")?;
                let m = reg.get_mut::<Model>(name).ok_or_else(|| format!("no model {name}"))?;
                m.redo().map_or(Value::Null, Value::from)
            }
            "model.load" => {
                let name = c.this.ok_or("no model named")?;
                let m = reg.get_mut::<Model>(name).ok_or_else(|| format!("no model {name}"))?;
                json!({ "warnings": m.load(c.raw(0).unwrap_or(&Value::Null))? })
            }
            "model.readModel" => {
                let (state, warnings) = md::read_model(c.raw(0).unwrap_or(&Value::Null))?;
                json!({ "state": state, "warnings": warnings })
            }
            "model.gridsFromSpacings" => md::grids_from_spacings(c.str(0), c.str(1), c.str(2), c.str(3))?,
            "model.spacingsOf" => Value::from(md::spacings_of(arr(c.arg(0)))),
            "model.nodes" => Value::Array(md::nodes(&state_arg(reg, c.arg(0))?)),
            "model.nodeName" => Value::from(md::node_name(&state_arg(reg, c.arg(0))?, p3(c.arg(1)))),
            "model.pointAt" => {
                let st = state_arg(reg, c.arg(0))?;
                let g = |i: usize| c.raw(i).cloned().unwrap_or(Value::Null);
                md::point_at(&st, &g(1), &g(2), &g(3)).map_or(Value::Null, p3v)
            }
            "model.cleanPhase" => md::clean_phase(c.raw(0)).map_or(Value::Null, Value::from),
            "model.connPhase" => jnum(md::conn_phase(&state_arg(reg, c.arg(0))?, c.raw(1).unwrap_or(&Value::Null))),
            _ => return Err("\u{0}unknown".into()),
        })
    })();
    match r {
        Err(e) if e == "\u{0}unknown" => None,
        r => Some(r),
    }
}

fn build_ops(reg: &mut Registry, c: &Call) -> R {
    match c.op {
        "build" | "build.build" => {
            let st = match state_arg(reg, c.raw(0)) {
                Ok(s) => s,
                Err(e) => return Some(Err(e)),
            };
            Some(build(&st).map(|b| {
                let j = built_json(&b);
                keep(reg, c, b);
                j
            }))
        }
        "build.keep" => {
            let st = match state_arg(reg, c.raw(0)) {
                Ok(s) => s,
                Err(e) => return Some(Err(e)),
            };
            Some(build(&st).map(|b| {
                let n = b.parts.len();
                keep(reg, c, b);
                json!({ "parts": n })
            }))
        }
        _ => None,
    }
}

fn marks_ops(reg: &mut Registry, c: &Call) -> R {
    let r = (|| -> Result<Value, String> {
        Ok(match c.op {
            "marks.markParts" => {
                let b = built_arg(reg, c.raw(0))?;
                let o = c.arg(1);
                let tol = o.and_then(|o| o.get("tol")).map_or(1.0, |v| num(Some(v)));
                let pp = o.and_then(|o| o.get("perPhase")).is_some_and(|v| crate::js::truthy(Some(v)));
                let m = mark_parts(b, tol, pp);
                let j = marks_json(&m);
                keep(reg, c, m);
                j
            }
            "marks.bomTables" => {
                let m = get::<Marks>(reg, c.raw(0).and_then(Value::as_str))?;
                let b = built_arg(reg, c.raw(1))?;
                let t = bom_tables(m, &b.bolts);
                json!({ "parts": t.parts, "assemblies": t.assemblies, "bolts": t.bolts, "totalWeight": jnum(t.total_weight) })
            }
            "marks.boltList" => {
                let b = built_arg(reg, c.raw(0))?;
                Value::Array(
                    bolt_list(&b.bolts)
                        .into_iter()
                        .map(|r| {
                            Obj::new()
                                .set("kind", r.kind.as_str())
                                .set("d", r.d)
                                .set("length", r.length)
                                .set("grade", r.grade.as_str())
                                .set("count", r.count)
                                .done()
                        })
                        .collect(),
                )
            }
            "marks.markFootings" => {
                let fs = match c.raw(0) {
                    Some(Value::String(_)) => state_arg(reg, c.raw(0))?.get("footings").cloned().unwrap_or(Value::Null),
                    Some(v) => v.clone(),
                    None => Value::Null,
                };
                let pp = c.arg(1).and_then(|o| o.get("perPhase")).is_some_and(|v| crate::js::truthy(Some(v)));
                let mut o = serde_json::Map::new();
                for (k, v) in mark_footings(arr(Some(&fs)), pp) {
                    o.insert(k, Value::from(v));
                }
                Value::Object(o)
            }
            _ => return Err("\u{0}unknown".into()),
        })
    })();
    match r {
        Err(e) if e == "\u{0}unknown" => None,
        r => Some(r),
    }
}

fn conn_ops(reg: &mut Registry, c: &Call) -> R {
    use crate::connections as cn;
    let st = match c.op {
        "conn.dg1Row" | "conn.defaults" => Value::Null,
        _ => match state_arg(reg, c.raw(0)) {
            Ok(s) => s,
            Err(e) => return Some(Err(e)),
        },
    };
    let ms = state::members(&st);
    let cs = state::conns(&st);
    let arg = |k: &str| c.arg(1).and_then(|o| o.get(k)).and_then(Value::as_str);
    let joint = |j: &cn::Joint| json!({ "beam": j.beam, "end": j.end, "column": j.column, "face": j.face, "away": p3v(j.away) });
    let sj = |j: &cn::SpliceJoint| {
        Obj::new().set("a", j.a.as_str()).set("aEnd", j.a_end).set("b", j.b.as_str()).set("bEnd", j.b_end).set("gap", j.gap).done()
    };
    let be = |j: &cn::BraceEnd| json!({ "brace": j.brace, "end": j.end, "support": j.support, "others": j.others });
    Some(Ok(match c.op {
        "conn.findJoints" => Value::Array(cn::find_joints(&ms).iter().map(joint).collect()),
        "conn.similarJoints" => {
            Value::Array(cn::similar_joints(&ms, &cs, arg("beam"), arg("end").unwrap_or("undefined")).iter().map(joint).collect())
        }
        "conn.findSplices" => Value::Array(cn::find_splices(&ms, ids_opt(c.arg(1)).as_deref()).iter().map(sj).collect()),
        "conn.freeSplices" => Value::Array(cn::free_splices(&ms, &cs, ids_opt(c.arg(1)).as_deref()).iter().map(sj).collect()),
        "conn.similarSplices" => Value::Array(cn::similar_splices(&ms, &cs, arg("a")).iter().map(sj).collect()),
        "conn.findBraceEnds" => Value::Array(cn::find_brace_ends(&ms, ids_opt(c.arg(1)).as_deref()).iter().map(be).collect()),
        "conn.freeBraceEnds" => Value::Array(cn::free_brace_ends(&ms, &cs, ids_opt(c.arg(1)).as_deref()).iter().map(be).collect()),
        "conn.similarBraceEnds" => Value::Array(cn::similar_brace_ends(&ms, &cs, arg("brace"), arg("support")).iter().map(be).collect()),
        "conn.similarColumns" => json!(cn::similar_columns(&ms, &cs, arg("column"))),
        "conn.dg1Row" => match cn::dg1_row(c.num(0)) {
            Some(r) => Obj::new()
                .set("rod", r.rod)
                .set("hole", r.hole)
                .set("washer", r.washer)
                .set("washerT", r.washer_t)
                .set("inch", r.inch.to_vec())
                .done(),
            None => Value::Null,
        },
        "conn.defaults" => cn::defaults_of(c.str(0)).unwrap_or(Value::Null),
        _ => return None,
    }))
}

fn footing_arg(v: Option<&Value>) -> Footing {
    Footing::from_value(v.unwrap_or(&Value::Null))
}

fn rebar_ops(c: &Call) -> R {
    let bar_of = |v: Option<&Value>| rebar::bar(&crate::js::string_of(v));
    Some(Ok(match c.op {
        "rebar.cleanFooting" => match rebar::clean_footing(c.raw(0).unwrap_or(&Value::Null)) {
            Ok(f) => json!({ "ok": true, "footing": Value::Object(f.fields()) }),
            Err(e) => json!({ "ok": false, "error": e }),
        },
        "rebar.footingProblems" => Value::Array(rebar::footing_problems(&footing_arg(c.arg(0))).iter().map(rebar::Problem::j).collect()),
        "rebar.footingBars" => Value::Array(rebar::footing_bars(&footing_arg(c.arg(0))).iter().map(rebar::BarGroup::j).collect()),
        "rebar.barPolylines" => {
            let seg = c.arg(1).and_then(Value::as_u64).map_or(6, |x| usize::try_from(x.min(1000)).unwrap_or(6));
            let groups = rebar::footing_bars(&footing_arg(c.arg(0)));
            Value::Array(
                groups
                    .iter()
                    .map(|g| {
                        Value::Array(
                            g.sets.iter().map(|s| Value::Array(rebar::bar_polyline(g, s, seg).into_iter().map(p3v).collect())).collect(),
                        )
                    })
                    .collect(),
            )
        }
        "rebar.cutLength" => match bar_of(c.arg(2)) {
            Some(b) => match rebar::cut_length(c.str(0), &dims_of(c.arg(1)), b) {
                Ok(x) => jnum(x),
                Err(e) => return Some(Err(e)),
            },
            None => Value::Null,
        },
        "rebar.centerline2d" => match bar_of(c.arg(2)) {
            Some(b) => {
                let segs = rebar::centerline2d(c.str(0), &dims_of(c.arg(1)), b);
                json!({ "segs": segs.iter().map(rebar::Seg::j).collect::<Vec<_>>(), "length": jnum(rebar::centerline_length(&segs)) })
            }
            None => Value::Null,
        },
        "rebar.layout" => {
            let (n, sp) = rebar::layout(c.num(0), c.num(1));
            Obj::new().set("n", n).set("spacing", sp).done()
        }
        "rebar.hookLeg" => bar_of(c.arg(0)).map_or(Value::Null, |b| jnum(rebar::hook_leg(b))),
        "rebar.bars" => Value::Array(rebar::bars().iter().map(rebar::bar_json).collect()),
        "rebar.barSets" => json!({ "astm": rebar::bar_set("astm"), "bs": rebar::bar_set("bs") }),
        "rebar.shapeCode" => Value::from(rebar::shape_code(c.str(0), c.arg(1).and_then(Value::as_str).unwrap_or("dc"))),
        "rebar.barLabel" => Value::from(rebar::bar_label(c.str(0), c.arg(1).and_then(Value::as_str).unwrap_or("imperial"))),
        "rebar.scheduleDims" => {
            let (a, b, cc) = rebar::schedule_dims(c.str(0), &dims_of(c.arg(1)), c.arg(2).and_then(Value::as_str).unwrap_or("dc"));
            Obj::new().opt("A", a).opt("B", b).opt("C", cc).done()
        }
        "rebar.bendingSchedule" | "rebar.scheduleTable" => {
            let fs: Vec<Footing> = arr(c.arg(0)).iter().map(Footing::from_value).collect();
            let sch = rebar::bending_schedule(&fs);
            if c.op == "rebar.scheduleTable" {
                let o = c.arg(1);
                let codes = o.and_then(|o| o.get("codes")).and_then(Value::as_str).unwrap_or("dc");
                let naming = o.and_then(|o| o.get("naming")).and_then(Value::as_str).unwrap_or("imperial");
                json!(rebar::schedule_table(&sch.rows, codes, naming))
            } else {
                let mut by = serde_json::Map::new();
                for (id, list) in &sch.by_footing {
                    by.insert(id.clone(), Value::Array(list.iter().map(|(g, m)| json!({ "group": g.j(), "mark": m })).collect()));
                }
                json!({ "rows": sch.rows.iter().map(rebar::SchedRow::j).collect::<Vec<_>>(), "byFooting": by })
            }
        }
        "rebar.footingVolume" => jnum(rebar::footing_volume(&footing_arg(c.arg(0)))),
        _ => return None,
    }))
}

fn stair_ops(c: &Call) -> R {
    use crate::stair;
    Some(Ok(match c.op {
        "stair.cleanStair" => match stair::clean_stair(c.raw(0).unwrap_or(&Value::Null)) {
            Ok(s) => json!({ "ok": true, "stair": Value::Object(s.fields()) }),
            Err(e) => json!({ "ok": false, "error": e }),
        },
        "stair.geometry" => {
            let st = stair::Stair::from_value(c.raw(0).unwrap_or(&Value::Null));
            let custom = arr(c.arg(1)).to_vec();
            let g = stair::stair_geometry(&st, &|n: &str| profiles::get_profile_str(n, &custom));
            let parts: Vec<Value> = g
                .parts
                .iter()
                .map(|p| {
                    Obj::new()
                        .set("key", p.key.as_str())
                        .set("type", p.kind)
                        .set("role", p.role.as_str())
                        .set("profileName", p.profile_name.as_str())
                        .set("material", p.material.as_str())
                        .set("start", p3v(p.start))
                        .set("end", p3v(p.end))
                        .set("rotation", p.rotation)
                        .set("position", "middle")
                        .set("attachTo", p.attach_to.clone())
                        .done()
                })
                .collect();
            let welds: Vec<Value> = g
                .welds
                .iter()
                .map(|w| {
                    Obj::new()
                        .set("between", w.between.to_vec())
                        .set("size", w.size)
                        .set("length", w.length)
                        .set("where", w.r#where.as_str())
                        .done()
                })
                .collect();
            let problems: Vec<Value> = g
                .problems
                .iter()
                .map(|p| {
                    let mut f = serde_json::Map::new();
                    for (k, v) in &p.fills {
                        f.insert(
                            (*k).to_string(),
                            match v {
                                cc_civil::i18n::F::N(x) => jnum(*x),
                                cc_civil::i18n::F::S(s) => Value::from(s.clone()),
                            },
                        );
                    }
                    json!({ "code": p.code, "text": p.text, "fills": f })
                })
                .collect();
            Obj::new()
                .set("n", g.n)
                .set("riser", g.riser)
                .set("treads", g.treads)
                .set("going", g.going)
                .set("run", g.run)
                .set("pitch", g.pitch)
                .set("blondel", g.blondel)
                .set("parts", Value::Array(parts))
                .set("welds", Value::Array(welds))
                .set("problems", Value::Array(problems))
                .done()
        }
        _ => return None,
    }))
}

fn rows_of(v: Option<&Value>) -> Vec<Vec<String>> {
    arr(v).iter().map(|r| arr(Some(r)).iter().map(|x| crate::js::string_or_empty(Some(x))).collect()).collect()
}

fn csv_ops(c: &Call) -> R {
    use crate::csv;
    Some(Ok(match c.op {
        "csv.cell" => Value::from(csv::cell(&crate::js::string_or_empty(c.raw(0)))),
        "csv.toCsv" => Value::from(csv::to_csv(&rows_of(c.arg(0)))),
        "csv.sectionsCsv" => {
            let secs: Vec<(String, Vec<Vec<String>>)> =
                arr(c.arg(0)).iter().map(|s| (crate::js::string_of(s.get(0)), rows_of(s.get(1)))).collect();
            Value::from(csv::sections_csv(&secs))
        }
        "csv.parseCsv" => json!(csv::parse_csv(c.str(0))),
        _ => return None,
    }))
}

fn zip_ops(c: &Call) -> R {
    use crate::zip;
    Some(Ok(match c.op {
        "zip.crc32" => Value::from(zip::crc32(c.str(0).as_bytes())),
        "zip.zip" => {
            let files: Vec<(String, Vec<u8>)> =
                arr(c.arg(0)).iter().map(|f| (crate::js::string_of(f.get("name")), s(f.get("data")).as_bytes().to_vec())).collect();
            let d = arr(c.arg(1));
            let g = |i: usize| d.get(i).and_then(Value::as_u64).and_then(|x| u32::try_from(x).ok()).unwrap_or(0);
            let date = zip::DosDate { year: g(0), month: g(1), day: g(2), hours: g(3), minutes: g(4), seconds: g(5) };
            json!({ "$base64": cc_civil::project::b64(&zip::zip(&files, date)) })
        }
        _ => return None,
    }))
}

fn sample_ops(reg: &mut Registry, c: &Call) -> R {
    match c.op {
        "sample.model" => {
            let name = c.arg(0).map_or_else(|| "Sample frame".to_string(), |v| crate::js::string_of(Some(v)));
            Some(crate::sample::sample_model(&name).map(|m| {
                keep(reg, c, m);
                undef()
            }))
        }
        _ => None,
    }
}

/// `J` of a value (re-exported for op modules elsewhere).
pub fn jv<T: J + ?Sized>(v: &T) -> Value {
    v.j()
}
