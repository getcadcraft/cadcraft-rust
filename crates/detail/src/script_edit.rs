//! Script ops for the editing modules (clipboard, transform, winsel, lasso-core, nest, clash); see [`crate::script`].
//! Twins of `forks/tools/dcdiff/ops-edit.mjs`.

use crate::build::Built;
use crate::js::{Obj, arr, jnum, num, p3v};
use crate::lasso_core::{self, Pt};
use crate::marks::Marks;
use crate::script::{Call, Registry, built_arg, get, keep, state_arg, undef};
use crate::transform::{self, Screen};
use crate::{clash, clipboard, nest, winsel};
use serde_json::{Value, json};

type R = Option<Result<Value, String>>;

/// A kept clip (copyClip / readClip output).
pub struct Clip(pub Value);

/// The harness's projection: `{ m: [12 numbers] }` → x = row 0 · [p, 1], y = row 1, z = row 2, behind = z < 0.
pub fn projector(v: Option<&Value>) -> impl Fn([f64; 3]) -> Screen {
    let m: Vec<f64> = arr(v.and_then(|o| o.get("m"))).iter().map(|x| num(Some(x))).collect();
    let g = move |i: usize| m.get(i).copied().unwrap_or(0.0);
    move |p: [f64; 3]| {
        let x = g(0) * p[0] + g(1) * p[1] + g(2) * p[2] + g(3);
        let y = g(4) * p[0] + g(5) * p[1] + g(6) * p[2] + g(7);
        let z = g(8) * p[0] + g(9) * p[1] + g(10) * p[2] + g(11);
        Screen { x, y, behind: z < 0.0 }
    }
}
fn p3(v: Option<&Value>) -> [f64; 3] {
    let a = arr(v);
    [num(a.first()), num(a.get(1)), num(a.get(2))]
}
fn pts(v: Option<&Value>) -> Vec<Pt> {
    arr(v).iter().map(|p| Pt { x: num(p.get("x")), y: num(p.get("y")) }).collect()
}
fn segs_json(s: &[[[f64; 3]; 2]]) -> Value {
    Value::Array(s.iter().map(|[a, b]| json!([p3v(*a), p3v(*b)])).collect())
}
fn planes(v: Option<&Value>) -> Vec<winsel::Plane> {
    arr(v).iter().map(|p| winsel::Plane { n: p3(p.get("n")), d: num(p.get("d")) }).collect()
}
fn hidden(v: Option<&Value>) -> Option<Vec<f64>> {
    match v {
        Some(Value::Array(a)) => Some(a.iter().map(|x| num(Some(x))).collect()),
        _ => None,
    }
}
/// A clip without its `source` (a time and a random tag in the JS).
fn clip_view(v: &Value) -> Value {
    let mut c = v.clone();
    if let Some(o) = c.as_object_mut() {
        o.shift_remove("source");
    }
    c
}
fn nest_opts(o: Option<&Value>) -> (nest::Stock, f64, f64, f64) {
    let g = |k: &str, d: f64| o.and_then(|o| o.get(k)).map_or(d, |v| num(Some(v)));
    let stock = match o.and_then(|o| o.get("stock")) {
        Some(Value::Array(a)) => nest::Stock::All(a.iter().map(|x| num(Some(x))).collect()),
        Some(Value::Object(m)) => {
            nest::Stock::Per(m.iter().map(|(k, v)| (k.clone(), arr(Some(v)).iter().map(|x| num(Some(x))).collect())).collect())
        }
        _ => nest::Stock::All(vec![6000.0, 12000.0]),
    };
    (stock, g("kerf", 3.0), g("trim", 0.0), g("keep", 500.0))
}

pub fn handle(reg: &mut Registry, c: &Call) -> R {
    let (module, _) = c.op.split_once('.').unwrap_or((c.op, ""));
    if !matches!(module, "clip" | "xf" | "winsel" | "lasso" | "nest" | "clash" | "agent") {
        return None;
    }
    let r = (|| -> Result<Value, String> {
        Ok(match c.op {
            "clip.copy" => {
                let st = state_arg(reg, c.raw(0))?;
                let conns = c.arg(2).and_then(|o| o.get("connections")).is_none_or(|v| crate::js::truthy(Some(v)));
                match clipboard::copy_clip(&st, arr(c.arg(1)), conns, "rs-clip") {
                    Some(r) => {
                        let clip = r.get("clip").cloned().unwrap_or(Value::Null);
                        let view = json!({ "clip": clip_view(&clip), "counts": r.get("counts") });
                        keep(reg, c, Clip(clip));
                        view
                    }
                    None => Value::Null,
                }
            }
            "clip.read" | "clip.readKept" => {
                let input = if c.op == "clip.readKept" {
                    let k = get::<Clip>(reg, c.raw(0).and_then(Value::as_str))?;
                    if c.arg(1).is_some_and(|v| crate::js::truthy(Some(v))) {
                        Value::String(crate::js::stringify(&k.0))
                    } else {
                        k.0.clone()
                    }
                } else {
                    c.raw(0).cloned().unwrap_or(Value::Null)
                };
                let clip = clipboard::read_clip(&input)?;
                let view = clip_view(&clip);
                keep(reg, c, Clip(clip));
                view
            }
            "clip.segments" => segs_json(&clipboard::clip_segments(&get::<Clip>(reg, c.raw(0).and_then(Value::as_str))?.0)),
            "clip.overlaps" => {
                let st = state_arg(reg, c.raw(0))?;
                let k = get::<Clip>(reg, c.raw(1).and_then(Value::as_str))?;
                Value::from(clipboard::overlaps(&st, &k.0, c.arg(2)))
            }
            "xf.make" => transform::xf_of(c.raw(0).unwrap_or(&Value::Null))?.j(),
            "xf.member" => {
                let xf = transform::xf_of(c.raw(1).unwrap_or(&Value::Null))?;
                let o = transform::transform_member(c.raw(0).unwrap_or(&Value::Null), &xf, c.arg(2).and_then(Value::as_str));
                Obj::new()
                    .set("start", p3v(o.start))
                    .set("end", p3v(o.end))
                    .set("rotation", o.rotation)
                    .set("position", o.position)
                    .set("holes", Value::Array(o.holes))
                    .set("cuts", Value::Array(o.cuts))
                    .set("note", o.note.map(str::to_string))
                    .done()
            }
            "xf.ghostLines" => {
                let st = state_arg(reg, c.raw(0))?;
                segs_json(&transform::ghost_lines(&st, arr(c.arg(1)), p3(c.arg(2)))?)
            }
            "xf.stairOutline" => segs_json(&transform::stair_outline(c.raw(0).unwrap_or(&Value::Null))),
            "xf.selectionCenter" => {
                let st = state_arg(reg, c.raw(0))?;
                transform::selection_center(&st, arr(c.arg(1)))?.map_or(Value::Null, p3v)
            }
            "xf.snapOffset" => {
                let proj = projector(c.arg(3));
                let o = c.arg(4);
                let mask = o.and_then(|o| o.get("mask")).map_or([1.0, 1.0, 1.0], |m| p3(Some(m)));
                let px = o.and_then(|o| o.get("px")).map_or(18.0, |v| num(Some(v)));
                let step = o.and_then(|o| o.get("step")).map_or(10.0, |v| num(Some(v)));
                let points: Vec<[f64; 3]> = arr(c.arg(1)).iter().map(|p| p3(Some(p))).collect();
                let nodes: Vec<[f64; 3]> = arr(c.arg(2)).iter().map(|p| p3(Some(p))).collect();
                let (off, node, point) = transform::snap_offset(p3(c.arg(0)), &points, &nodes, &proj, mask, px, step);
                json!({ "offset": p3v(off), "node": node.map_or(Value::Null, p3v), "point": point.map_or(Value::Null, p3v) })
            }
            "xf.manyPatchable" => {
                let st = state_arg(reg, c.raw(0))?;
                transform::many_patchable(&st, arr(c.arg(1)))
            }
            "xf.worldSection" => {
                let custom = arr(c.arg(3)).to_vec();
                match crate::profiles::get_profile(c.raw(1).unwrap_or(&Value::Null), &custom) {
                    Some(p) => Value::Array(
                        transform::world_section(c.raw(0).unwrap_or(&Value::Null), &p, c.arg(2).map_or(0.0, |v| num(Some(v))))
                            .into_iter()
                            .map(p3v)
                            .collect(),
                    ),
                    None => Value::Null,
                }
            }
            "winsel.items" => {
                let st = state_arg(reg, c.raw(0))?;
                let b = built_arg(reg, c.raw(1))?;
                let h = hidden(c.arg(2));
                Value::Array(
                    winsel::selection_items(&st, b, h.as_deref())
                        .iter()
                        .map(|it| json!({ "ref": it.r#ref, "segs": segs_json(&it.segs) }))
                        .collect(),
                )
            }
            "winsel.select" => {
                let st = state_arg(reg, c.raw(0))?;
                let b: &Built = built_arg(reg, c.raw(1))?;
                let items = winsel::selection_items(&st, b, hidden(c.arg(6)).as_deref());
                let proj = projector(c.arg(2));
                Value::Array(winsel::window_select(&items, &proj, &pts(c.arg(3)), c.str(4), &planes(c.arg(5))))
            }
            "winsel.clipSeg" => match winsel::clip_seg(p3(c.arg(0)), p3(c.arg(1)), &planes(c.arg(2))) {
                Some([a, b]) => json!([p3v(a), p3v(b)]),
                None => Value::Null,
            },
            "winsel.rectPath" => {
                let p = pts(Some(&Value::Array(vec![c.raw(0).cloned().unwrap_or(Value::Null), c.raw(1).cloned().unwrap_or(Value::Null)])));
                let (a, b) = (
                    p.first().copied().unwrap_or(Pt { x: f64::NAN, y: f64::NAN }),
                    p.get(1).copied().unwrap_or(Pt { x: f64::NAN, y: f64::NAN }),
                );
                Value::Array(winsel::rect_path(a, b).iter().map(|q| json!({ "x": jnum(q.x), "y": jnum(q.y) })).collect())
            }
            "lasso.mode" => {
                let p = pts(Some(&Value::Array(vec![c.raw(0).cloned().unwrap_or(Value::Null), c.raw(1).cloned().unwrap_or(Value::Null)])));
                let (a, b) = (
                    p.first().copied().unwrap_or(Pt { x: f64::NAN, y: f64::NAN }),
                    p.get(1).copied().unwrap_or(Pt { x: f64::NAN, y: f64::NAN }),
                );
                lasso_core::lasso_mode(a, b, c.arg(2).map_or(6.0, |v| num(Some(v)))).map_or(Value::Null, Value::from)
            }
            "lasso.next" => Value::from(lasso_core::next_mode(c.str(0))),
            "lasso.thin" => {
                Value::Array(lasso_core::thin(&pts(c.arg(0)), c.num(1)).iter().map(|q| json!({ "x": jnum(q.x), "y": jnum(q.y) })).collect())
            }
            "lasso.pointInPoly" => Value::from(lasso_core::point_in_poly(
                Pt { x: num(c.arg(0).and_then(|p| p.get("x"))), y: num(c.arg(0).and_then(|p| p.get("y"))) },
                &pts(c.arg(1)),
            )),
            "nest.bars" => {
                let pieces: Vec<nest::Piece> = arr(c.arg(0))
                    .iter()
                    .map(|p| nest::Piece {
                        mark: crate::js::string_of(p.get("mark")),
                        profile: crate::js::string_of(p.get("profile")),
                        material: crate::js::string_of(p.get("material")),
                        length: num(p.get("length")),
                        count: num(p.get("count")),
                    })
                    .collect();
                let (stock, kerf, trim, keep_) = nest_opts(c.arg(1));
                nest::nest_json(&nest::nest_bars(&pieces, &stock, kerf, trim, keep_))
            }
            "nest.fromMarks" | "nest.tables" => {
                let m = get::<Marks>(reg, c.raw(0).and_then(Value::as_str))?;
                let (stock, kerf, trim, keep_) = nest_opts(c.arg(1));
                let r = nest::nest_bars(&nest::pieces_from_marks(m), &stock, kerf, trim, keep_);
                if c.op == "nest.tables" {
                    let (s, b) = nest::cut_list_tables(&r);
                    json!({ "summary": s, "bars": b })
                } else {
                    nest::nest_json(&r)
                }
            }
            "clash.check" => {
                let st = state_arg(reg, c.raw(0))?;
                let b = built_arg(reg, c.raw(1))?;
                let o = c.arg(2);
                let tol = o.and_then(|o| o.get("tol")).map_or(1.0, |v| num(Some(v)));
                let ph = hidden(o.and_then(|o| o.get("phases")));
                clash::clash_json(&clash::clash_check(&st, b, tol, ph.as_deref()))
            }
            "agent.new" => {
                let model = match c.raw(0) {
                    Some(Value::String(x)) if x == "sample" => crate::sample::sample_model("Sample frame")?,
                    None => crate::sample::sample_model("Sample frame")?,
                    Some(st) => crate::model::Model::new(st.clone()),
                };
                keep(reg, c, crate::agents::AgentApp::new(model));
                crate::script::undef()
            }
            "agent.tools" => Value::Array(crate::agents::tool_defs().iter().map(crate::agents::ToolDef::j).collect()),
            "agent.run" | "agent.state" | "agent.change" | "agent.undoModel" => {
                let name = c.this.ok_or("no object named")?;
                let app = reg.get_mut::<crate::agents::AgentApp>(name).ok_or_else(|| format!("no object {name}"))?;
                match c.op {
                    "agent.run" => app.run(c.str(0), c.raw(1).unwrap_or(&json!({})))?,
                    "agent.state" => json!({
                        "state": app.model.state, "selected": app.selected, "saves": app.saves, "exported": app.exported,
                        "undo": app.model.undo_stack.iter().map(|s| s.label.clone()).collect::<Vec<_>>(), "dirty": app.model.dirty(),
                    }),
                    "agent.change" => {
                        let ops = arr(c.arg(1)).to_vec();
                        app.model.change(c.str(0), |tx| {
                            let mut res = Vec::new();
                            for o in &ops {
                                res.push(tx.call(crate::js::s(o.get("op")), arr(o.get("args")))?.unwrap_or(Value::Null));
                            }
                            Ok(Value::Array(res))
                        })?
                    }
                    _ => app.model.undo().map_or(Value::Null, Value::from),
                }
            }
            "clash.obb" => {
                let ob = |v: Option<&Value>| clash::Obb {
                    c: p3(v.and_then(|o| o.get("c"))),
                    ax: {
                        let a = arr(v.and_then(|o| o.get("ax")));
                        [p3(a.first()), p3(a.get(1)), p3(a.get(2))]
                    },
                    h: p3(v.and_then(|o| o.get("h"))),
                };
                Value::from(clash::obb_overlap(&ob(c.arg(0)), &ob(c.arg(1)), c.arg(2).map_or(0.0, |v| num(Some(v)))))
            }
            _ => return Err("\u{0}unknown".into()),
        })
    })();
    match r {
        Err(e) if e == "\u{0}unknown" => None,
        r => Some(r),
    }
}

/// `$` ops inside a model change (`model.change` hands them the transaction and the kept clips).
pub fn tx_op(tx: &mut crate::model::Tx, op: &str, a: &[Value], clips: &[(String, Value)]) -> Result<Option<Value>, String> {
    match op {
        "$paste" => {
            let name = a.first().and_then(Value::as_str).unwrap_or("");
            let clip = clips.iter().find(|(n, _)| n == name).map(|(_, c)| c.clone()).ok_or_else(|| format!("no object {name}"))?;
            clipboard::paste_into(tx, &clip, a.get(1)).map(Some)
        }
        "$transform" => {
            let xf = transform::xf_of(a.get(1).unwrap_or(&Value::Null))?;
            let st = tx.s.clone();
            transform::transform_refs(tx, &st, arr(a.first()), &xf).map(Some)
        }
        "$applyToMany" => {
            let st = tx.s.clone();
            transform::apply_to_many(tx, &st, arr(a.first()), &crate::js::string_of(a.get(1)), a.get(2)).map(Some)
        }
        _ => Ok(Some(undef())),
    }
}
