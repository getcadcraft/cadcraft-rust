//! The call-script runner behind `cadcraft-cli part <script.json>` and the parity harness (`forks/tools/pcdiff.mjs`,
//! whose JS driver calls the frozen modules with the same op names): `{ "calls": [{ "op": "part.read", "args": [...] },
//! …] }` → `[{ "ok": <result> } | { "err": "<message>" }, …]`. Ops that change a document return it with the result
//! (`{ res, part }`), so every call is self-contained JSON in and out.

use crate::i18n::t;
use crate::js;
use crate::jv;
use crate::omap::OMap;
use crate::{bom, configs, expr, form, gdt, params, params_part, part, sketch, solver, spline, threads};
use serde_json::{Value, json};

/// A handler: Some(result) when it knows the op.
pub type Handler = fn(&str, &[Value]) -> Option<Result<Value, String>>;

/// Run a script through the handlers (tried in order).
pub fn run_with(script: &Value, handlers: &[Handler]) -> Value {
    let mut out = Vec::new();
    for c in jv::arr(script.get("calls")) {
        let op = jv::as_str(c.get("op")).unwrap_or("");
        let args = jv::arr(c.get("args"));
        let mut res = None;
        for h in handlers {
            res = h(op, args);
            if res.is_some() {
                break;
            }
        }
        out.push(match res {
            Some(Ok(v)) => json!({ "ok": jv::canon_marks(&v) }),
            Some(Err(e)) => json!({ "err": e }),
            None => json!({ "err": format!("unknown op {op}") }),
        });
    }
    Value::Array(out)
}

/// Run a script with this crate's ops.
pub fn run(script: &Value) -> Value {
    run_with(script, &[handle, crate::script2::handle])
}

/// A number argument ("$NaN" / "$Inf" / "$-Inf" carry the values JSON cannot).
pub fn num_arg(v: Option<&Value>) -> f64 {
    if let Some(x) = v.and_then(jv::nonfinite) {
        return x;
    }
    match v {
        Some(Value::String(s)) if s == "$NaN" => f64::NAN,
        Some(Value::String(s)) if s == "$Inf" => f64::INFINITY,
        Some(Value::String(s)) if s == "$-Inf" => f64::NEG_INFINITY,
        Some(v) => v.as_f64().unwrap_or(f64::NAN),
        None => f64::NAN,
    }
}
fn arg(a: &[Value], i: usize) -> &Value {
    a.get(i).unwrap_or(&Value::Null)
}
fn s_arg(a: &[Value], i: usize) -> String {
    js::str_of(a.get(i))
}
fn strs_arg(a: &[Value], i: usize) -> Vec<String> {
    jv::arr(a.get(i)).iter().map(|v| js::str_of(Some(v))).collect()
}
fn omap_num(v: &Value) -> Option<OMap<f64>> {
    let o = v.as_object()?;
    let mut m = OMap::new();
    for (k, x) in o {
        m.set(k, num_arg(Some(x)));
    }
    Some(m)
}
fn omap_pt(v: &Value) -> Option<OMap<[f64; 2]>> {
    let o = v.as_object()?;
    let mut m = OMap::new();
    for (k, x) in o {
        let a = jv::arr(Some(x));
        m.set(k, [num_arg(a.first()), num_arg(a.get(1))]);
    }
    Some(m)
}
fn pairs(v: &Value) -> Vec<(String, f64)> {
    v.as_object().map(|o| o.iter().map(|(k, x)| (k.clone(), num_arg(Some(x)))).collect()).unwrap_or_default()
}
fn p2(v: &Value) -> [f64; 2] {
    let a = jv::arr(Some(v));
    [num_arg(a.first()), num_arg(a.get(1))]
}
fn p2s(v: &Value) -> Vec<[f64; 2]> {
    jv::arr(Some(v)).iter().map(p2).collect()
}
fn pts_json(p: &[[f64; 2]]) -> Value {
    Value::Array(p.iter().map(|q| jv::pt2(*q)).collect())
}
fn expr_err(e: &expr::ExprError) -> Value {
    jv::obj(vec![("ok", false.into()), ("error", e.message.as_str().into()), ("pos", e.pos.map_or(Value::Null, jv::unum))])
}
pub(crate) fn curve_arg(v: &Value) -> spline::Curve {
    let poles = p2s(v.get("poles").unwrap_or(&Value::Null));
    let knots: Vec<f64> = jv::arr(v.get("knots")).iter().map(|x| num_arg(Some(x))).collect();
    let deg = jv::as_num(v.get("deg")).and_then(js::idx).unwrap_or(3);
    let mut c = spline::Curve { deg, knots, poles, ..spline::Curve::default() };
    if jv::truthy(v.get("periodic")) {
        c.periodic = true;
        c.n = jv::as_num(v.get("n")).and_then(js::idx).unwrap_or(0);
        c.pp = p2s(v.get("P").unwrap_or(&Value::Null));
        c.tau = jv::arr(v.get("tau")).iter().map(|x| num_arg(Some(x))).collect();
    }
    c
}

/// This crate's ops.
pub fn handle(op: &str, a: &[Value]) -> Option<Result<Value, String>> {
    let (module, _) = op.split_once('.').unwrap_or((op, ""));
    match module {
        "expr" => expr_ops(op, a),
        "params" => params_ops(op, a),
        "pp" => pp_ops(op, a),
        "part" => part_ops(op, a),
        "sketch" => sketch_ops(op, a),
        "spline" => spline_ops(op, a),
        "configs" => configs_ops(op, a),
        "gdt" => gdt_ops(op, a),
        "bom" => bom_ops(op, a),
        "threads" => thread_ops(op, a),
        "form" => form_ops(op, a),
        "i18n" => match op {
            "i18n.t" => Some(Ok(Value::String(t(&s_arg(a, 0), &[])))),
            _ => None,
        },
        _ => None,
    }
}

fn expr_ops(op: &str, a: &[Value]) -> Option<Result<Value, String>> {
    Some(Ok(match op {
        "expr.parse" => {
            let names = a.get(1).filter(|v| !v.is_null()).map(|_| strs_arg(a, 1));
            match expr::parse(&s_arg(a, 0), names.as_deref()) {
                Ok(p) => jv::obj(vec![("ok", true.into()), ("ast", p.ast.to_json()), ("uses", jv::strs(&p.uses))]),
                Err(e) => expr_err(&e),
            }
        }
        "expr.compute" => match expr::compute(&s_arg(a, 0), &pairs(arg(a, 1))) {
            Ok((v, uses)) => jv::obj(vec![("ok", true.into()), ("value", jv::num(v)), ("uses", jv::strs(&uses))]),
            Err(e) => expr_err(&e),
        },
        "expr.literal" => expr::literal(&s_arg(a, 0)).map_or(Value::Null, jv::num),
        "expr.formatNumber" => Value::String(expr::format_number(num_arg(a.first()))),
        "expr.solveFor" => {
            let p = match expr::parse(&s_arg(a, 0), None) {
                Ok(p) => p,
                Err(e) => return Some(Ok(expr_err(&e))),
            };
            match expr::solve_for(&p.ast, &pairs(arg(a, 1)), &s_arg(a, 2), num_arg(a.get(3)), 1e-9) {
                Ok(v) => jv::obj(vec![("ok", true.into()), ("value", jv::num(v))]),
                Err(e) => jv::obj(vec![("ok", false.into()), ("error", e.into())]),
            }
        }
        "expr.renameIn" => Value::String(expr::rename_in(&s_arg(a, 0), &s_arg(a, 1), &s_arg(a, 2))),
        _ => return None,
    }))
}

fn params_ops(op: &str, a: &[Value]) -> Option<Result<Value, String>> {
    let p0 = || params::Params::from_json(arg(a, 0));
    Some(match op {
        "params.read" => {
            let keys = a.get(1).filter(|v| !v.is_null()).map(|v| jv::arr(Some(v)).iter().map(|x| js::str_of(Some(x))).collect());
            params::read_params(arg(a, 0), keys.as_ref()).map(|p| p.to_json())
        }
        "params.evaluateVars" => {
            let ov = a.get(1).filter(|v| !v.is_null()).and_then(omap_num);
            Ok(params::evaluate_vars(&p0(), ov.as_ref()).to_json())
        }
        "params.setVar" => {
            let mut p = p0();
            let o = arg(a, 3);
            let note = o.get("note").map(|n| if n.is_null() { None } else { Some(n) });
            let rename = o.get("rename").and_then(Value::as_str);
            let r = params::set_var(&mut p, &s_arg(a, 1), &s_arg(a, 2), note, rename);
            Ok(json!({ "res": r.to_json(), "p": p.to_json() }))
        }
        "params.removeVar" => {
            let mut p = p0();
            let r = params::remove_var(&mut p, &s_arg(a, 1), jv::truthy(a.get(2)));
            Ok(json!({ "res": r.to_json(), "p": p.to_json() }))
        }
        "params.driveVar" => {
            let mut p = p0();
            let prefer = a.get(3).and_then(Value::as_str);
            let r = params::drive_var(&mut p, &s_arg(a, 1), num_arg(a.get(2)), prefer);
            Ok(json!({ "res": r.to_json(), "p": p.to_json() }))
        }
        "params.drivingBehind" => Ok(jv::strs(&params::driving_behind(&p0(), &s_arg(a, 1)))),
        "params.usesOf" => {
            let (v, l) = params::uses_of(&p0(), &s_arg(a, 1));
            Ok(json!({ "vars": v, "links": l }))
        }
        "params.readDesignTable" => match params::read_design_table(&s_arg(a, 0), &params::Params::from_json(arg(a, 1))) {
            Err(e) => Err(e),
            Ok(Err(e)) => Ok(json!({ "ok": false, "error": e })),
            Ok(Ok((rows, cols))) => {
                let rows: Vec<Value> = rows
                    .iter()
                    .map(|r| {
                        let mut o = jv::Obj::new();
                        for (k, v) in r.overrides.iter() {
                            o.insert(k.to_string(), jv::num(*v));
                        }
                        json!({ "name": r.name, "overrides": Value::Object(o) })
                    })
                    .collect();
                Ok(json!({ "ok": true, "rows": rows, "columns": cols }))
            }
        },
        "params.applyOverrides" => {
            let mut p = p0();
            let ov = omap_num(arg(a, 1)).unwrap_or_default();
            params::apply_overrides(&mut p, &ov).map(|()| p.to_json())
        }
        _ => return None,
    })
}

fn pp_ops(op: &str, a: &[Value]) -> Option<Result<Value, String>> {
    let mut part = arg(a, 0).clone();
    Some(match op {
        "pp.targets" => {
            let tg = params_part::targets(&part);
            let mut o = jv::Obj::new();
            for (k, x) in tg.iter() {
                o.insert(k.to_string(), x.to_json(params_part::target_value(&part, x)));
            }
            Ok(Value::Object(o))
        }
        "pp.apply" => {
            let ov = a.get(1).filter(|v| !v.is_null()).and_then(omap_num);
            let r = params_part::apply_part_params(&mut part, ov.as_ref());
            Ok(json!({ "res": r.to_json(), "part": part }))
        }
        "pp.setLink" => {
            let mut p = params_part::params_of(&part);
            let ad = params_part::PartAdapter::default();
            let r = params::set_link(&mut p, &ad, &part, &s_arg(a, 1), &s_arg(a, 2));
            params_part::put_params(&mut part, &p);
            Ok(json!({ "res": r.to_json(), "part": part }))
        }
        "pp.setVar" => {
            let mut p = params_part::params_of(&part);
            let r = params::set_var(&mut p, &s_arg(a, 1), &s_arg(a, 2), None, None);
            params_part::put_params(&mut part, &p);
            Ok(json!({ "res": r.to_json(), "part": part }))
        }
        "pp.driveBack" => {
            let mut p = params_part::params_of(&part);
            let ad = params_part::PartAdapter::default();
            let prefer = a.get(3).and_then(Value::as_str);
            let r = params::drive_back(&mut p, &ad, &part, &s_arg(a, 1), num_arg(a.get(2)), prefer);
            if part.get("params").is_some() {
                params_part::put_params(&mut part, &p);
            }
            Ok(json!({ "res": r.to_json(), "part": part }))
        }
        "pp.resolveTarget" => Ok(params_part::resolve_target(&part, &s_arg(a, 1)).map_or(Value::Null, Value::String)),
        "pp.targetName" => Ok(Value::String(params_part::target_name(&part, &s_arg(a, 1)))),
        "pp.report" => Ok(params_part::params_report(&part)),
        "pp.pruneDeleted" => {
            let mut after = arg(a, 1).clone();
            let r = params_part::prune_deleted(&part, &mut after);
            Ok(json!({ "res": r, "after": after }))
        }
        "pp.stepRollback" => {
            let r = params_part::step_rollback(&mut part, num_arg(a.get(1)));
            Ok(json!({ "res": r, "part": part }))
        }
        "pp.rollTo" => {
            let id = a.get(1).and_then(Value::as_str);
            let r = params_part::roll_to(&mut part, id);
            Ok(json!({ "res": r, "part": part }))
        }
        "pp.rollbackWords" => Ok(Value::String(params_part::rollback_words(&part))),
        "pp.fieldInUse" => Ok(params_part::field_in_use(&part, &s_arg(a, 1)).into()),
        "pp.readPartParams" => params_part::read_part_params(arg(a, 1), &part).map(|p| p.map_or(Value::Null, |p| p.to_json())),
        _ => return None,
    })
}

fn part_ops(op: &str, a: &[Value]) -> Option<Result<Value, String>> {
    let mut p = arg(a, 0).clone();
    Some(match op {
        "part.read" => part::read_part(arg(a, 0)),
        "part.toJSON" => Ok(Value::String(part::to_json(&p))),
        "part.fileVersion" => Ok(jv::num(part::file_version(&p))),
        "part.new" => Ok(part::new_part(&s_arg(a, 0))),
        "part.addFeature" => part::add_feature(&mut p, &s_arg(a, 1), arg(a, 2)).map(|f| json!({ "f": f, "part": p })),
        "part.dependsOn" => Ok(jv::strs(&part::depends_on(arg(a, 0)))),
        "part.dependents" => Ok(Value::Array(part::dependents(&p, &s_arg(a, 1)))),
        "part.canMove" => Ok(match part::can_move(&p, &s_arg(a, 1), num_arg(a.get(2))) {
            Ok(()) => json!({ "ok": true }),
            Err(r) => json!({ "ok": false, "reason": r }),
        }),
        "part.moveFeature" => {
            let r = match part::move_feature(&mut p, &s_arg(a, 1), num_arg(a.get(2))) {
                Ok(()) => json!({ "ok": true }),
                Err(r) => json!({ "ok": false, "reason": r }),
            };
            Ok(json!({ "res": r, "part": p }))
        }
        "part.deleteFeatures" => {
            let names = part::delete_features(&mut p, &strs_arg(a, 1));
            Ok(json!({ "names": names, "part": p }))
        }
        "part.builtCount" => Ok(jv::num(part::built_count(&p))),
        "part.readDrawing" => part::read_drawing(a.first()),
        _ => return None,
    })
}

fn at_arg(v: &Value) -> sketch::At {
    sketch::At::from_json(v)
}

fn sketch_ops(op: &str, a: &[Value]) -> Option<Result<Value, String>> {
    let mut sk = arg(a, 0).clone();
    let with = |sk: Value, r: Result<Value, String>| r.map(|res| json!({ "res": res, "sk": sk }));
    Some(match op {
        "sketch.read" => sketch::read_sketch(a.first()),
        "sketch.solve" => {
            let pin = a.get(1).filter(|v| !v.is_null()).and_then(omap_pt);
            let pr = a.get(2).filter(|v| !v.is_null()).and_then(omap_num);
            let r = solver::solve_sketch(&mut sk, pin.as_ref(), pr.as_ref());
            Ok(json!({ "res": r.to_json(), "sk": sk }))
        }
        "sketch.analyze" => {
            let r = solver::analyze(&sk, a.get(1).and_then(Value::as_f64));
            Ok(
                json!({ "dof": r.dof, "rank": r.rank, "vars": r.vars, "redundant": r.redundant, "conflicts": r.conflicts, "determined": r.determined }),
            )
        }
        "sketch.residuals" => Ok(Value::Array(solver::residuals_now(&sk).into_iter().map(|(c, v)| json!([c, jv::num(v)])).collect())),
        "sketch.profile" => Ok(match sketch::profile(&sk, num_arg(a.get(1))) {
            Ok(p) => json!({
                "loops": Value::Array(p.loops.iter().map(|l| pts_json(l)).collect()),
                "curves": Value::Array(p.curves.iter().map(|(id, l)| json!({ "id": id, "loop": l })).collect()),
            }),
            Err(e) => {
                let mut o = json!({ "error": e.error });
                if let Some(at) = e.at {
                    jv::set(&mut o, "at", jv::pt2(at));
                }
                o
            }
        }),
        "sketch.measure" => Ok(sketch::measure(&sk, &s_arg(a, 1), &strs_arg(a, 2), None).to_json()),
        "sketch.matchSig" => sketch::match_sig(&sk, &s_arg(a, 1), &strs_arg(a, 2))
            .map(|m| m.map_or(Value::Null, |(sig, refs)| json!({ "sig": sig, "refs": refs }))),
        "sketch.applicable" => Ok(jv::strs(&sketch::applicable(&sk, &strs_arg(a, 1)))),
        "sketch.addConstraint" => {
            let v = a.get(3).filter(|v| !v.is_null());
            let of = arg(a, 4).get("of").and_then(Value::as_str);
            let r = sketch::add_constraint(&mut sk, &s_arg(a, 1), &strs_arg(a, 2), v, of);
            with(sk, r)
        }
        "sketch.addPoint" => {
            let id = sketch::add_point(&mut sk, num_arg(a.get(1)), num_arg(a.get(2)), jv::truthy(arg(a, 3).get("construction")));
            with(sk, Ok(Value::String(id)))
        }
        "sketch.addLine" => {
            let r = sketch::add_line(&mut sk, &at_arg(arg(a, 1)), &at_arg(arg(a, 2)), jv::truthy(arg(a, 3).get("construction")));
            with(sk, r)
        }
        "sketch.addCircle" => {
            let r = sketch::add_circle(&mut sk, &at_arg(arg(a, 1)), num_arg(a.get(2)), jv::truthy(arg(a, 3).get("construction")));
            with(sk, r)
        }
        "sketch.addArc" => {
            let r = sketch::add_arc(
                &mut sk,
                &at_arg(arg(a, 1)),
                &at_arg(arg(a, 2)),
                &at_arg(arg(a, 3)),
                jv::truthy(arg(a, 4).get("construction")),
            );
            with(sk, r)
        }
        "sketch.addRect" => {
            let r = sketch::add_rect(&mut sk, p2(arg(a, 1)), p2(arg(a, 2)), jv::truthy(arg(a, 3).get("construction")));
            with(sk, r)
        }
        "sketch.addSpline" => {
            let pts: Vec<sketch::At> = jv::arr(a.get(1)).iter().map(at_arg).collect();
            let r = sketch::add_spline(&mut sk, &pts, arg(a, 2));
            with(sk, r)
        }
        "sketch.removeEntities" => {
            let (e, c) = sketch::remove_entities(&mut sk, &strs_arg(a, 1));
            with(sk, Ok(json!({ "entities": e, "constraints": c })))
        }
        "sketch.removeConstraints" => {
            sketch::remove_constraints(&mut sk, &strs_arg(a, 1));
            with(sk, Ok(Value::Null))
        }
        "sketch.filletCorner" => {
            let r = sketch::fillet_corner(&mut sk, &s_arg(a, 1), num_arg(a.get(2)));
            with(sk, r)
        }
        "sketch.describe" => Ok(Value::String(sketch::describe(arg(a, 0)))),
        "sketch.extent" => Ok(jv::num(sketch::extent(&sk))),
        "sketch.bounds" => Ok(sketch::bounds(&sk).map_or(Value::Null, |b| json!({ "x0": b[0], "y0": b[1], "x1": b[2], "y1": b[3] }))),
        "sketch.insideLoops" => {
            let loops: Vec<Vec<[f64; 2]>> = jv::arr(a.first()).iter().map(p2s).collect();
            Ok(sketch::inside_loops(&loops, p2(arg(a, 1))).into())
        }
        "sketch.loopArea" => Ok(jv::num(sketch::loop_area(&p2s(arg(a, 0))))),
        "sketch.loosePoints" => Ok(Value::Array(sketch::loose_points(&sk).into_iter().cloned().collect())),
        "sketch.tessArc" => match sketch::by_id(&sk, &s_arg(a, 1)) {
            Some(e) => Ok(pts_json(&sketch::tess_arc(&sk, &e, num_arg(a.get(2))))),
            None => Err("no arc".into()),
        },
        "sketch.tessSpline" => match sketch::by_id(&sk, &s_arg(a, 1)) {
            Some(e) => sketch::tess_spline(&sk, &e, js::idx(num_arg(a.get(2))).unwrap_or(16)).map(|p| pts_json(&p)),
            None => Err("no spline".into()),
        },
        "sketch.endMeet" => match (sketch::by_id(&sk, &s_arg(a, 1)), sketch::by_id(&sk, &s_arg(a, 2))) {
            (Some(x), Some(y)) => Ok(sketch::end_meet(&sk, &x, &y).map_or(Value::Null, |(p, q)| json!([p, q]))),
            _ => Err("no entity".into()),
        },
        _ => return None,
    })
}

fn spline_ops(op: &str, a: &[Value]) -> Option<Result<Value, String>> {
    let cj = |c: &spline::Curve| spline::curve_json(c);
    let opt = |v: Option<&Value>| v.filter(|x| !x.is_null()).map(|x| num_arg(Some(x)));
    Some(match op {
        "spline.interpFit" => {
            let o = arg(a, 1);
            spline::interp_fit(&p2s(arg(a, 0)), jv::truthy(o.get("closed")), opt(o.get("a0")), opt(o.get("a1"))).map(|c| cj(&c))
        }
        "spline.controlCurve" => {
            let o = arg(a, 1);
            let knots: Option<Vec<f64>> = o.get("knots").filter(|k| jv::truthy(Some(k))).map(|k| jv::arr(Some(k)).iter().map(|x| num_arg(Some(x))).collect());
            Ok(cj(&spline::control_curve(&p2s(arg(a, 0)), jv::truthy(o.get("closed")), knots.as_deref())))
        }
        "spline.tessellate" => Ok(pts_json(&spline::tessellate(&curve_arg(arg(a, 0)), js::idx(num_arg(a.get(1))).unwrap_or(16)))),
        "spline.tessellateTol" => Ok(pts_json(&spline::tessellate_tol(&curve_arg(arg(a, 0)), num_arg(a.get(1))))),
        "spline.areaTerm" => Ok(jv::num(spline::area_term(&curve_arg(arg(a, 0))))),
        "spline.arcLength" => Ok(jv::num(spline::arc_length(&curve_arg(arg(a, 0))))),
        "spline.closestParam" => Ok(jv::num(spline::closest_param(&curve_arg(arg(a, 0)), p2(arg(a, 1)), js::idx(num_arg(a.get(2))).unwrap_or(12)))),
        "spline.curvatureAt" => Ok(jv::num(spline::curvature_at(&curve_arg(arg(a, 0)), num_arg(a.get(1))))),
        "spline.derivsAt" => Ok(pts_json(&spline::derivs_at(&curve_arg(arg(a, 0)), num_arg(a.get(1)), js::idx(num_arg(a.get(2))).unwrap_or(1)))),
        "spline.pointAt" => Ok(jv::pt2(spline::point_at(&curve_arg(arg(a, 0)), num_arg(a.get(1))))),
        "spline.insertKnot" => Ok(cj(&spline::insert_knot(&curve_arg(arg(a, 0)), num_arg(a.get(1))))),
        "spline.segment" => Ok(cj(&spline::segment(&curve_arg(arg(a, 0)), num_arg(a.get(1)), num_arg(a.get(2))))),
        "spline.reverse" => Ok(cj(&spline::reverse(&curve_arg(arg(a, 0))))),
        "spline.normalizeKnots" => Ok(cj(&spline::normalize_knots(&curve_arg(arg(a, 0))))),
        "spline.knotProblem" => Ok(spline::knot_problem(a.first(), js::idx(num_arg(a.get(1))).unwrap_or(0)).map_or(Value::Null, Value::String)),
        "spline.offsetFit" => spline::offset_fit(&curve_arg(arg(a, 0)), num_arg(a.get(1)), num_arg(a.get(2))).map(|r| {
            json!({ "Q": pts_json(&r.q), "a0": r.a0.map_or(Value::Null, jv::num), "a1": r.a1.map_or(Value::Null, jv::num), "closed": r.closed, "err": jv::num(r.err), "curve": cj(&r.curve) })
        }),
        _ => return None,
    })
}

fn configs_ops(op: &str, a: &[Value]) -> Option<Result<Value, String>> {
    let params_of = |v: &Value| if v.is_null() { None } else { Some(params::Params::from_json(v)) };
    let cfg_of = |v: &Value| if v.is_null() { None } else { Some(configs::Configs::from_json(v)) };
    let cfg_json = |c: Option<configs::Configs>| c.map_or(Value::Null, |c| c.to_json());
    Some(match op {
        "configs.read" => configs::read_configs(a.first(), params_of(arg(a, 1)).as_ref()).map(cfg_json),
        "configs.rowFromCurrent" => Ok(configs::row_from_current(params_of(arg(a, 0)).as_ref(), a.get(1)).to_json()),
        "configs.saveRow" => configs::save_row(cfg_of(arg(a, 0)).as_ref(), &configs::Row::from_json(arg(a, 1))).map(|c| c.to_json()),
        "configs.applyRow" => {
            let mut p = params::Params::from_json(arg(a, 0));
            configs::apply_row(&mut p, &configs::Row::from_json(arg(a, 1))).map(|()| p.to_json())
        }
        "configs.rowMatches" => Ok(configs::row_matches(params_of(arg(a, 0)).as_ref(), &configs::Row::from_json(arg(a, 1))).into()),
        "configs.renameRow" => configs::rename_row(&configs::Configs::from_json(arg(a, 0)), &s_arg(a, 1), a.get(2)).map(|c| c.to_json()),
        "configs.deleteRow" => Ok(cfg_json(configs::delete_row(&configs::Configs::from_json(arg(a, 0)), &s_arg(a, 1)))),
        "configs.followParams" => {
            Ok(cfg_json(configs::follow_params(cfg_of(arg(a, 0)).as_ref(), params_of(arg(a, 1)).as_ref(), params_of(arg(a, 2)).as_ref())))
        }
        "configs.rowsFromTable" => configs::rows_from_table(&s_arg(a, 0), &params::Params::from_json(arg(a, 1)))
            .map(|rs| Value::Array(rs.iter().map(configs::Row::to_json).collect())),
        "configs.csv" => Ok(Value::String(configs::configs_csv(cfg_of(arg(a, 0)).as_ref()))),
        _ => return None,
    })
}

fn gdt_ops(op: &str, a: &[Value]) -> Option<Result<Value, String>> {
    Some(match op {
        "gdt.readNotes" => gdt::read_notes(a.first()).map(Value::Array),
        "gdt.validate" => Ok(gdt::validate(a.first()).map_or(Value::Null, Value::String)),
        "gdt.describe" => Ok(Value::String(gdt::describe(arg(a, 0)))),
        "gdt.newAnnotation" => gdt::new_annotation(&s_arg(a, 0), arg(a, 1)),
        "gdt.geometry" => gdt::annotation_geometry(arg(a, 0), p2(arg(a, 1))).map(|g| g.to_json()),
        "gdt.textW" => Ok(jv::num(gdt::text_w(&s_arg(a, 0), num_arg(a.get(1))))),
        _ => return None,
    })
}

fn bom_ops(op: &str, a: &[Value]) -> Option<Result<Value, String>> {
    Some(match op {
        "bom.readMaterial" => bom::read_material(a.first()).map(|m| m.unwrap_or(Value::Null)),
        "bom.readPartProps" => bom::read_part_props(a.first()),
        "bom.presetOf" => Ok(Value::from(bom::preset_of(a.first()))),
        "bom.materials" => {
            Ok(Value::Array(bom::materials().into_iter().map(|(n, d)| json!({ "name": n, "density": jv::num(d) })).collect()))
        }
        "bom.rows" => {
            let vols = arg(a, 1).clone();
            let f = move |pid: &str| vols.get(pid).filter(|v| !v.is_null()).map(|v| (jv::n(v, "volume"), jv::truthy(v.get("exact"))));
            Ok(Value::Array(bom::bom_rows(arg(a, 0), &f)))
        }
        "bom.csv" => Ok(Value::String(bom::bom_csv(jv::arr(a.first())))),
        "bom.csvCell" => Ok(Value::String(bom::csv_cell(arg(a, 0), !matches!(arg(a, 1).get("text"), Some(Value::Bool(false)))))),
        _ => return None,
    })
}

fn thread_ops(op: &str, a: &[Value]) -> Option<Result<Value, String>> {
    Some(Ok(match op {
        "threads.dims" => match threads::thread_dims_js(&s_arg(a, 0), &s_arg(a, 1)) {
            Ok(d) => d.map_or(Value::Null, |d| d.to_json()),
            Err(e) => return Some(Err(e)),
        },
        "threads.sizeFor" => threads::size_for(&s_arg(a, 0), num_arg(a.get(1)), jv::truthy(a.get(2))).map_or(Value::Null, Value::from),
        "threads.profileCorners" => match threads::profile_corners(&s_arg(a, 0), &s_arg(a, 1), num_arg(a.get(2))) {
            Ok(c) => c.map_or(Value::Null, |c| pts_json(&c)),
            Err(e) => return Some(Err(e)),
        },
        "threads.profileAt" => jv::num(threads::profile_at(&p2s(arg(a, 0)), num_arg(a.get(1)))),
        "threads.boltSection" => {
            let o = arg(a, 1);
            let hand = o.get("hand").and_then(Value::as_str).unwrap_or("right");
            let seg = o.get("segments").map_or(96.0, |v| num_arg(Some(v)));
            pts_json(&threads::bolt_section(&p2s(arg(a, 0)), hand, seg))
        }
        "threads.callout" => {
            let o = arg(a, 2);
            match threads::thread_callout(
                &s_arg(a, 0),
                &s_arg(a, 1),
                jv::truthy(o.get("external")),
                num_arg(o.get("length")),
                jv::truthy(o.get("through")),
                jv::truthy(o.get("modelled")),
            ) {
                Ok(s) => Value::String(s),
                Err(e) => return Some(Err(e)),
            }
        }
        "threads.sizeLabel" => Value::String(threads::size_label(&s_arg(a, 0), &s_arg(a, 1))),
        _ => return None,
    }))
}

fn form_ops(op: &str, a: &[Value]) -> Option<Result<Value, String>> {
    Some(match op {
        "form.read" => form::read_form(a.first()),
        "form.box" => Ok(form::box_form(
            num_arg(a.first()),
            num_arg(a.get(1)),
            num_arg(a.get(2)),
            num_arg(a.get(3)),
            num_arg(a.get(4)),
            num_arg(a.get(5)),
        )),
        _ => return None,
    })
}
