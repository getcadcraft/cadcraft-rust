//! The script ops of the design modules (grading … project); each mirrors one op of the JS driver.

use crate::alignment::Alignment;
use crate::geo::{read_p2, read_p2s, read_p3s, read_pz};
use crate::js::{J, arr, numf};
use crate::profile::Profile;
use crate::project::{self, Project};
use crate::script::{Call, Registry, keep, obj};
use crate::script_ops::read_pts;
use crate::tin::Surface;
use crate::*;
use serde_json::{Map, Value, json};

type R = Option<Result<Value, String>>;
fn ok<T: J + ?Sized>(v: &T) -> R {
    Some(Ok(v.j()))
}
macro_rules! need {
    ($reg:expr, $name:expr, $t:ty, $what:expr) => {
        match obj::<$t>($reg, $name, $what) {
            Ok(x) => x,
            Err(e) => return Some(Err(e)),
        }
    };
}
fn s_of<'a>(reg: &'a Registry, v: Option<&Value>) -> Option<&'a Surface> {
    v.and_then(Value::as_str).and_then(|n| reg.get::<Surface>(n))
}
fn opt_f(v: Option<&Value>, d: f64) -> f64 {
    v.filter(|x| !x.is_null()).map_or(d, |x| numf(Some(x)))
}
fn o_f(o: Option<&Value>, k: &str, d: f64) -> f64 {
    opt_f(o.and_then(|o| o.get(k)), d)
}
fn str_list(v: Option<&Value>) -> Vec<String> {
    arr(v).iter().map(|x| crate::js::string_or_empty(Some(x))).collect()
}
fn regions_of(v: Option<&Value>) -> Vec<Vec<geo::P3>> {
    arr(v).iter().map(|r| read_p3s(Some(r))).collect()
}
fn pad_of(v: &Value, opts: Option<&Value>) -> grading::Pad {
    grading::Pad {
        pts: arr(v.get("pts")).iter().map(read_pz).collect(),
        z: crate::js::number(v.get("z")),
        cut: crate::js::number(v.get("cut")),
        fill: crate::js::number(v.get("fill")),
        name: crate::js::string_or_empty(v.get("name")),
        spacing: o_f(opts, "spacing", f64::NAN),
        fan_step: o_f(opts, "fanStep", f64::NAN),
        max_dist: o_f(opts, "maxDist", f64::NAN),
    }
}
fn patch_json(p: &grading::Patch) -> Value {
    json!({ "problems": p.problems, "region": p.region.j(), "points": p.points.j(), "breaklines": p.breaklines.j(), "info": p.info })
}

pub fn handle(reg: &mut Registry, c: &Call) -> R {
    let (module, _) = c.op.split_once('.').unwrap_or((c.op, ""));
    match module {
        "grading" => grading_ops(reg, c),
        "corridor" => corridor_ops(reg, c),
        "super" => super_ops(reg, c),
        "pipes" => pipes_ops(reg, c),
        "hydro" => hydro_ops(reg, c),
        "sec" => sec_ops(reg, c),
        "fl" => fl_ops(reg, c),
        "dk" => dk_ops(c),
        "pc" => pc_ops(c),
        "tv" => tv_ops(reg, c),
        "sk" => sk_ops(reg, c),
        "mh" => mh_ops(reg, c),
        "why" => why_ops(c),
        "clip" => clip_ops(c),
        "pe" => pe_ops(c),
        "drive" => drive_ops(reg, c),
        "project" => project_ops(reg, c),
        "s3" => s3_ops(reg, c),
        "agent" => agent_ops(reg, c),
        _ => None,
    }
}

fn grading_ops(reg: &mut Registry, c: &Call) -> R {
    match c.op {
        "grading.castSlope" => {
            let s = need!(reg, c.arg(0).and_then(Value::as_str), Surface, "surface");
            let o = c.arg(7);
            let r = grading::cast_slope(
                s,
                read_p2(c.arg(1).unwrap_or(&Value::Null)),
                c.num(2),
                c.num(3),
                c.num(4),
                c.num(5),
                c.num(6),
                o_f(o, "rate", 1.0),
                o_f(o, "step", 0.25),
                o_f(o, "far", 1e4),
            );
            ok(&r)
        }
        "grading.daylightPad" => {
            let s = need!(reg, c.arg(0).and_then(Value::as_str), Surface, "surface");
            ok(&grading::daylight_pad(s, &pad_of(c.arg(1).unwrap_or(&Value::Null), c.arg(2))))
        }
        "grading.gridVolume" => {
            let a = need!(reg, c.arg(0).and_then(Value::as_str), Surface, "surface");
            let b = need!(reg, c.arg(1).and_then(Value::as_str), Surface, "surface");
            let o = c.arg(2);
            let regions = o.and_then(|o| o.get("region")).filter(|r| !r.is_null()).map(|r| regions_of(Some(r)));
            ok(&grading::grid_volume(a, b, o_f(o, "cell", f64::NAN), regions.as_deref(), None, o_f(o, "maxCells", 4e6)))
        }
        "grading.selfCrosses" => ok(&grading::self_crosses(&read_p2s(c.arg(0)))),
        "grading.gradedSurface" => {
            // [base surface, basePoints, baseBreaklines, items]: an item is a pad, or {name, corridor: {alignment, profile, asm}}
            let base = need!(reg, c.arg(0).and_then(Value::as_str), Surface, "surface");
            let (pts, bls, _, _) = crate::script_ops::read_build_input(Some(
                &json!({ "points": c.arg(1).cloned().unwrap_or(Value::Null), "breaklines": c.arg(2).cloned().unwrap_or(Value::Null) }),
            ));
            let raw = arr(c.arg(3)).to_vec();
            let mut items: Vec<(String, grading::Item)> = Vec::new();
            for it in &raw {
                let name = crate::js::string_or_empty(it.get("name"));
                if let Some(co) = it.get("corridor").filter(|v| v.is_object()) {
                    let al = co.get("alignment").and_then(Value::as_str).and_then(|n| reg.get::<Alignment>(n));
                    let pr = co.get("profile").and_then(Value::as_str).and_then(|n| reg.get::<Profile>(n));
                    let asm = corridor::read_assembly(co.get("asm").unwrap_or(&Value::Null), "ft");
                    items
                        .push((name, grading::Item::Patch(Box::new(move |sf: &Surface| corridor::corridor_patch(sf, al, pr, &asm).patch))));
                } else {
                    items.push((name, grading::Item::Pad(pad_of(it, it.get("options")))));
                }
            }
            let build = |p: &[crate::tin::ZPt], b: &[crate::tin::Breakline]| crate::tin::build_surface(p, b, None, 0.0);
            let g = grading::graded_surface(base, &pts, &bls, &items, &build);
            drop(items);
            let rep_items: Vec<Value> = g
                .items_report
                .iter()
                .zip(raw.iter())
                .map(|((_, probs), it)| {
                    let mut m = serde_json::Map::new();
                    if let Some(n) = it.get("name") {
                        m.insert("name".into(), n.clone());
                    }
                    m.insert("problems".into(), probs.j());
                    Value::Object(m)
                })
                .collect();
            let j = json!({
                "surface": g.surface.as_ref().map(crate::script_ops::surface_view),
                "report": { "notes": g.notes, "errors": g.errors, "items": rep_items },
                "regions": g.regions.j(),
                "items": g.infos,
            });
            if let Some(s) = g.surface {
                keep(reg, c, s);
            }
            Some(Ok(j))
        }
        _ => None,
    }
}

fn corridor_ops(reg: &mut Registry, c: &Call) -> R {
    match c.op {
        "corridor.defaultAssembly" => ok(&corridor::default_assembly(c.str(0))),
        "corridor.patch" => {
            let s = need!(reg, c.arg(0).and_then(Value::as_str), Surface, "surface");
            let al = c.arg(1).and_then(Value::as_str).and_then(|n| reg.get::<Alignment>(n));
            let pr = c.arg(2).and_then(Value::as_str).and_then(|n| reg.get::<Profile>(n));
            let asm = corridor::read_assembly(c.arg(3).unwrap_or(&Value::Null), "ft");
            let cp = corridor::corridor_patch(s, al, pr, &asm);
            Some(Ok(patch_json(&cp.patch)))
        }
        _ => None,
    }
}

fn super_ops(reg: &mut Registry, c: &Call) -> R {
    match c.op {
        "super.defaults" => ok(&superelev::super_defaults(c.str(0))),
        "super.clean" => ok(&superelev::clean_super(c.raw(0), c.str(1))),
        "super.rateFor" => {
            let rate: Vec<(f64, Option<f64>)> =
                arr(c.arg(1)).iter().map(|r| (numf(r.get(0)), r.get(1).filter(|e| !e.is_null()).map(|e| numf(Some(e))))).collect();
            ok(&superelev::rate_for(c.num(0), &rate))
        }
        "super.plan" => {
            let al = need!(reg, c.arg(0).and_then(Value::as_str), Alignment, "alignment");
            let o = c.arg(1);
            let sp = o.and_then(|o| o.get("super")).filter(|s| s.is_object()).map(superelev::read_super);
            let plan = superelev::super_plan(al, sp.as_ref(), o_f(o, "nc", 0.02), o_f(o, "lane", 12.0));
            let j = plan.j();
            keep(reg, c, plan);
            Some(Ok(j))
        }
        "super.slopesAt" => {
            let p = need!(reg, c.this, superelev::Plan, "plan");
            let (l, r) = superelev::slopes_at(p, c.num(0), opt_f(c.arg(1), 0.02));
            Some(Ok(json!({ "L": l.j(), "R": r.j() })))
        }
        "super.keyStations" => {
            let p = need!(reg, c.this, superelev::Plan, "plan");
            ok(&superelev::key_stations(p))
        }
        _ => None,
    }
}

fn pipes_ops(reg: &mut Registry, c: &Call) -> R {
    let net = pipes::read_network(c.arg(0).unwrap_or(&Value::Null));
    match c.op {
        "pipes.analyze" => ok(&pipes::analyze_network(&net, s_of(reg, c.arg(1)), c.str(2))),
        "pipes.autoInverts" => {
            let only = c.arg(3).and_then(|o| o.get("only")).filter(|v| v.is_array()).map(|v| str_list(Some(v)));
            let (p, problems) = pipes::auto_inverts(&net, s_of(reg, c.arg(1)), c.str(2), only.as_deref());
            Some(Ok(json!({ "pipes": p.j(), "problems": problems })))
        }
        "pipes.flowOrder" => {
            let o =
                pipes::flow_order(&net).map(|v| v.into_iter().filter_map(|i| net.pipes.get(i).map(|p| p.id.clone())).collect::<Vec<_>>());
            ok(&o)
        }
        "pipes.defaultNetwork" => ok(&pipes::default_network(c.str(0))),
        _ => None,
    }
}

fn hydro_ops(reg: &mut Registry, c: &Call) -> R {
    match c.op {
        "hydro.defaults" => ok(&hydro::hydro_defaults(c.str(0))),
        "hydro.clean" => ok(&hydro::clean_hydro(c.raw(0), c.str(1))),
        "hydro.cleanCatchment" => ok(&hydro::clean_catchment(c.raw(0).unwrap_or(&Value::Null))),
        "hydro.area" => ok(&hydro::catchment_area(&read_p2s(c.arg(0)), c.str(1))),
        "hydro.idf" => ok(&hydro::idf(c.num(0), &hydro::read_hydro(c.arg(1).unwrap_or(&Value::Null)))),
        "hydro.manningFull" => {
            let (q, v, a) = hydro::manning_full(c.num(0), c.num(1), c.num(2), c.str(3));
            Some(Ok(json!({ "Q": q.j(), "V": v.j(), "A": a.j() })))
        }
        "hydro.partFull" => {
            let (a, p, t) = hydro::part_full(c.num(0), c.num(1));
            Some(Ok(json!({ "A": a.j(), "P": p.j(), "T": t.j() })))
        }
        "hydro.normalDepth" => ok(&hydro::normal_depth(c.num(0), c.num(1), c.num(2), c.num(3), c.str(4))),
        "hydro.flows" => {
            let net = pipes::read_network(c.arg(0).unwrap_or(&Value::Null));
            let f = hydro::rational_flows(&net, c.str(1));
            let j = f.j();
            keep(reg, c, f);
            Some(Ok(j))
        }
        "hydro.size" => {
            let net = pipes::read_network(c.arg(0).unwrap_or(&Value::Null));
            let f = need!(reg, c.arg(1).and_then(Value::as_str), hydro::Flows, "flows");
            let sizes = c
                .arg(3)
                .and_then(|o| o.get("sizes"))
                .filter(|v| v.is_array())
                .map(|v| arr(Some(v)).iter().map(|x| numf(Some(x))).collect::<Vec<_>>());
            Some(Ok(hydro::sizes_json(&hydro::size_pipes(&net, f, c.str(2), sizes.as_deref()))))
        }
        "hydro.grade" => {
            let net = pipes::read_network(c.arg(0).unwrap_or(&Value::Null));
            let f = need!(reg, c.arg(1).and_then(Value::as_str), hydro::Flows, "flows");
            ok(&hydro::hydraulic_grade(&net, f, c.str(2), s_of(reg, c.arg(3))))
        }
        _ => None,
    }
}

fn runs_of(v: Option<&Value>) -> Vec<sections::Run> {
    arr(v).iter().map(|r| arr(Some(r)).iter().map(|p| sections::OZ { o: numf(p.get("o")), z: numf(p.get("z")) }).collect()).collect()
}
fn line_of(v: &Value) -> sections::Line {
    sections::Line {
        sta: numf(v.get("sta")),
        c: read_p2(v.get("c").unwrap_or(&Value::Null)),
        a: read_p2(v.get("a").unwrap_or(&Value::Null)),
        b: read_p2(v.get("b").unwrap_or(&Value::Null)),
        dir: numf(v.get("dir")),
        left: numf(v.get("left")),
        right: numf(v.get("right")),
    }
}
fn lines_of(v: Option<&Value>) -> Vec<(String, Vec<geo::P3>)> {
    v.and_then(Value::as_object)
        .map_or_else(Vec::new, |m| m.iter().filter(|(_, p)| p.is_array()).map(|(k, p)| (k.clone(), read_p3s(Some(p)))).collect())
}

fn sec_ops(reg: &mut Registry, c: &Call) -> R {
    match c.op {
        "sec.defaults" => ok(&sections::sample_defaults(c.str(0))),
        "sec.clean" => ok(&sections::clean_sampling(c.raw(0), c.str(1))),
        "sec.stations" => {
            let al = need!(reg, c.arg(0).and_then(Value::as_str), Alignment, "alignment");
            let o = c.arg(1);
            let extra: Vec<f64> = arr(o.and_then(|o| o.get("extra"))).iter().map(|x| numf(Some(x))).collect();
            ok(&sections::sample_stations(al, o_f(o, "step", f64::NAN), &extra, o_f(o, "from", f64::NAN), o_f(o, "to", f64::NAN)))
        }
        "sec.line" => {
            let al = need!(reg, c.arg(0).and_then(Value::as_str), Alignment, "alignment");
            ok(&sections::sample_line(al, c.num(1), c.num(2), c.num(3)))
        }
        "sec.surfaceSection" => {
            let s = s_of(reg, c.arg(0));
            let r = sections::surface_section(
                s,
                read_p2(c.arg(1).unwrap_or(&Value::Null)),
                read_p2(c.arg(2).unwrap_or(&Value::Null)),
                c.num(3),
                c.num(4),
            );
            Some(Ok(Value::Array(r.iter().map(|run| run.j()).collect())))
        }
        "sec.zAt" => ok(&sections::z_at(&runs_of(c.arg(0)), c.num(1))),
        "sec.cutFill" => {
            let (cut, fill, parts) = sections::cut_fill(&runs_of(c.arg(0)), &runs_of(c.arg(1)));
            Some(Ok(json!({ "cut": cut.j(), "fill": fill.j(), "parts": parts.j() })))
        }
        "sec.featureMarks" => ok(&sections::feature_marks(&lines_of(c.arg(0)), &line_of(c.arg(1).unwrap_or(&Value::Null)))),
        "sec.regionSpans" => ok(&sections::region_spans(&line_of(c.arg(0).unwrap_or(&Value::Null)), &regions_of(c.arg(1)))),
        "sec.clipRuns" => {
            let spans: Vec<[f64; 2]> = arr(c.arg(1)).iter().map(|s| [numf(s.get(0)), numf(s.get(1))]).collect();
            let r = sections::clip_runs(&runs_of(c.arg(0)), &spans);
            Some(Ok(Value::Array(r.iter().map(|run| run.j()).collect())))
        }
        "sec.build" => {
            let o = c.arg(0).cloned().unwrap_or(Value::Null);
            let al = need!(reg, o.get("alignment").and_then(Value::as_str), Alignment, "alignment");
            let g = s_of(reg, o.get("ground"));
            let same = o.get("design").and_then(Value::as_str) == o.get("ground").and_then(Value::as_str);
            let d = s_of(reg, o.get("design"));
            let regions = o.get("regions").filter(|r| r.is_array()).map(|r| regions_of(Some(r)));
            let cor = o.get("corridor").filter(|v| v.is_object()).map(|v| lines_of(Some(v)));
            let smp = o.get("sampling").filter(|v| v.is_object()).map(sections::read_sampling);
            let set = sections::build_sections(
                al,
                g,
                d,
                same || d.is_none(),
                regions.as_deref(),
                cor.as_deref(),
                smp.as_ref(),
                opt_f(o.get("from"), f64::NAN),
                opt_f(o.get("to"), f64::NAN),
            );
            let j = set.j();
            keep(reg, c, set);
            Some(Ok(j))
        }
        "sec.project" => {
            // project.sections(alignment), kept for sheets / CSV
            let name = c.arg(0).and_then(Value::as_str).unwrap_or("");
            let Some(p) = reg.get_mut::<Project>(name) else { return Some(Err(format!("no object {name}"))) };
            let Some(a) = al_by_id(p, c.str(1)) else { return Some(Ok(Value::Null)) };
            let Some(set) = p.sections(&a) else { return Some(Ok(Value::Null)) };
            let set = (*set).clone();
            let j = set.j();
            keep(reg, c, set);
            Some(Ok(j))
        }
        "sec.csv" => {
            let set = need!(reg, c.this, sections::SectionSet, "sections");
            let units = c.arg(0).and_then(|o| o.get("units")).and_then(Value::as_str).unwrap_or("ft");
            ok(&sections::sections_csv(set, units, &crate::js::num_to_string))
        }
        _ => None,
    }
}

fn fl_ops(reg: &mut Registry, c: &Call) -> R {
    use featureline as fl;
    let f = || fl::read_feature(c.arg(0).unwrap_or(&Value::Null));
    // `withPts(f, pts)` = `{ ...f, pts }`: the input object with the new points (other fields as they came)
    let with_pts = |ff: &fl::Feature| -> Value {
        match c.arg(0) {
            Some(Value::Object(m)) => {
                let mut m = m.clone();
                m.insert("pts".into(), ff.pts.j());
                Value::Object(m)
            }
            _ => ff.j(),
        }
    };
    match c.op {
        "fl.clean" => ok(&fl::clean_feature(c.raw(0).unwrap_or(&Value::Null), c.str(1))),
        "fl.stations" => ok(&fl::stations(&f())),
        "fl.segments" => ok(&fl::segments(&f())),
        "fl.setElevation" => Some(Ok(with_pts(&fl::set_elevation(&f(), crate::js::usize_of(c.num(1)).unwrap_or(usize::MAX), c.num(2))))),
        "fl.setGrade" => Some(Ok(with_pts(&fl::set_grade(&f(), crate::js::usize_of(c.num(1)).unwrap_or(usize::MAX), c.num(2))))),
        "fl.raise" => Some(Ok(with_pts(&fl::raise(&f(), c.num(1))))),
        "fl.evenGrade" => Some(Ok(with_pts(&fl::even_grade(&f(), c.num(1), c.num(2))))),
        "fl.fromSurface" => {
            let which = c
                .arg(2)
                .filter(|v| v.is_array())
                .map(|v| arr(Some(v)).iter().filter_map(|x| x.as_f64().and_then(crate::js::usize_of)).collect::<Vec<_>>());
            let (ff, missing) = fl::from_surface(&f(), s_of(reg, c.arg(1)), which.as_deref());
            Some(Ok(json!({ "f": with_pts(&ff), "missing": missing })))
        }
        "fl.offset" => {
            let o = c.arg(2);
            let g = |k: &str| o.and_then(|o| o.get(k)).filter(|v| !v.is_null()).map(|v| numf(Some(v)));
            let (ff, problem) = fl::offset_feature(&f(), c.num(1), g("dz"), g("slope"));
            let fj = ff.as_ref().map(|ff| {
                let mut v = with_pts(ff);
                if let Some(m) = v.as_object_mut() {
                    m.insert("id".into(), Value::Null);
                }
                v
            });
            Some(Ok(json!({ "f": fj, "problem": problem })))
        }
        "fl.breaklines" => {
            let fs: Vec<fl::Feature> = arr(c.arg(0)).iter().map(fl::read_feature).collect();
            let v: Vec<Value> = fl::feature_breaklines(&fs, c.str(1))
                .into_iter()
                .map(|(b, id)| json!({ "name": b.name, "pts": b.pts.j(), "feature": id }))
                .collect();
            Some(Ok(Value::Array(v)))
        }
        "fl.item" => {
            let ff = f();
            Some(Ok(match fl::feature_item(&ff) {
                None => Value::Null,
                Some(fl::FeatureItem::Pad(p)) => {
                    json!({ "id": ff.id, "kind": "feature", "name": ff.name, "pts": p.pts.j(), "z": p.z.j(), "cut": p.cut.j(), "fill": p.fill.j() })
                }
                Some(fl::FeatureItem::Outline(pts)) => {
                    json!({ "id": ff.id, "kind": "feature", "name": ff.name, "patch": patch_json(&fl::outline_patch(&ff.name, &pts)) })
                }
            }))
        }
        "fl.gradeLabel" => ok(&fl::grade_label(c.num(0))),
        "fl.crossings" => {
            let al = need!(reg, c.arg(0).and_then(Value::as_str), Alignment, "alignment");
            ok(&fl::alignment_crossings(al, &fl::read_feature(c.arg(1).unwrap_or(&Value::Null))))
        }
        "fl.runs" => {
            let al = need!(reg, c.arg(0).and_then(Value::as_str), Alignment, "alignment");
            ok(&fl::projected_runs(al, &fl::read_feature(c.arg(1).unwrap_or(&Value::Null)), c.num(2)))
        }
        "fl.profile" => {
            let al = need!(reg, c.arg(0).and_then(Value::as_str), Alignment, "alignment");
            let fs: Vec<fl::Feature> = arr(c.arg(1)).iter().map(fl::read_feature).collect();
            Some(Ok(fl::feature_profile(al, &fs, o_f(c.arg(2), "within", 0.0))))
        }
        "fl.overlay" => {
            let fs: Vec<fl::Feature> = arr(c.arg(0)).iter().map(fl::read_feature).collect();
            let t = c.arg(1);
            let (k, ox, oy) = (o_f(t, "k", 1.0), o_f(t, "ox", 0.0), o_f(t, "oy", 0.0));
            let o = c.arg(2);
            let skip = o.and_then(|o| o.get("skip")).and_then(Value::as_str);
            let v = fl::grade_overlay(&fs, &|x, y| ((x - ox) * k, -(y - oy) * k), o_f(o, "minSeg", 60.0), o_f(o, "minGap", 34.0), skip);
            Some(Ok(Value::Array(v)))
        }
        _ => None,
    }
}

fn dk_ops(c: &Call) -> R {
    use desckeys as dk;
    match c.op {
        "dk.cleanKeys" => ok(&dk::clean_desc_keys(c.raw(0))),
        "dk.cleanLineCodes" => ok(&dk::clean_line_codes(c.raw(0))),
        "dk.parseDesc" => {
            let lc = c.arg(1).map_or_else(dk::default_line_codes, dk::read_line_codes);
            let (code, ctrl, rest) = dk::parse_desc(&crate::js::string_or_empty(c.arg(0)), &lc);
            Some(Ok(json!({ "code": code, "ctrl": ctrl, "rest": rest })))
        }
        "dk.groups" => {
            let keys = dk::read_desc_keys(c.arg(0));
            let g = dk::grouper(&keys);
            let v: Vec<String> = read_pts(c.arg(1)).iter().map(&g).collect();
            ok(&v)
        }
        "dk.figures" => {
            let o = c.arg(1);
            let keys = dk::read_desc_keys(o.and_then(|o| o.get("keys")));
            let lc = o.and_then(|o| o.get("lineCodes")).filter(|v| v.is_object()).map_or_else(dk::default_line_codes, dk::read_line_codes);
            let only = o.and_then(|o| o.get("only")).filter(|v| v.is_array()).map(|v| str_list(Some(v)));
            ok(&dk::figures(&read_pts(c.arg(0)), &keys, &lc, only.as_deref()))
        }
        "dk.keyStats" => ok(&dk::key_stats(&read_pts(c.arg(0)), &dk::read_desc_keys(c.arg(1)))),
        _ => None,
    }
}

fn pc_ops(c: &Call) -> R {
    use parcels as pc;
    match c.op {
        "pc.clean" => ok(&pc::clean_parcel(c.raw(0).unwrap_or(&Value::Null))),
        "pc.parseAngle" => ok(&pc::parse_angle(&crate::js::string_or_empty(c.arg(0)))),
        "pc.parseBearing" => ok(&pc::parse_bearing(&crate::js::string_or_empty(c.arg(0)))),
        "pc.parseCalls" => {
            let (calls, errors) = pc::parse_calls(&crate::js::string_or_empty(c.arg(0)));
            Some(Ok(json!({ "calls": calls.j(), "errors": errors.j() })))
        }
        "pc.runText" => {
            let (calls, _) = pc::parse_calls(&crate::js::string_or_empty(c.arg(1)));
            ok(&pc::run_calls(read_p2(c.arg(0).unwrap_or(&Value::Null)), &calls))
        }
        "pc.runCalls" => {
            let calls: Vec<pc::Call> = arr(c.arg(1)).iter().map(pc::read_call).collect();
            ok(&pc::run_calls(read_p2(c.arg(0).unwrap_or(&Value::Null)), &calls))
        }
        "pc.courses" => ok(&pc::courses(&pc::read_corners(c.arg(0)))),
        "pc.area" => ok(&pc::parcel_area(&pc::read_corners(c.arg(0)))),
        "pc.perimeter" => ok(&pc::perimeter(&pc::read_corners(c.arg(0)))),
        "pc.centroid" => ok(&pc::centroid(&pc::read_corners(c.arg(0)))),
        "pc.areaWords" => ok(&pc::area_words(c.num(0), c.str(1))),
        "pc.legal" => {
            let o = c.arg(1);
            let units = o.and_then(|o| o.get("units")).and_then(Value::as_str).unwrap_or("ft");
            let pob = o.and_then(|o| o.get("pob")).and_then(Value::as_str);
            ok(&pc::legal_description(&pc::read_parcel(c.arg(0).unwrap_or(&Value::Null)), units, pob))
        }
        "pc.labels" => {
            let o = c.arg(1);
            let units = o.and_then(|o| o.get("units")).and_then(Value::as_str).unwrap_or("ft");
            Some(Ok(Value::Array(pc::parcel_labels(&pc::read_parcel(c.arg(0).unwrap_or(&Value::Null)), units, o_f(o, "unitPerMm", 1.0)))))
        }
        _ => None,
    }
}

fn tv_ops(reg: &mut Registry, c: &Call) -> R {
    use traverse as tv;
    match c.op {
        "tv.parse" => {
            let (legs, errors, ca) =
                tv::parse_traverse(&crate::js::string_or_empty(c.arg(0)), c.arg(1).and_then(Value::as_str).unwrap_or("angles"));
            Some(Ok(json!({ "legs": legs.j(), "errors": errors.j(), "closingAngle": ca.j() })))
        }
        "tv.run" => {
            let o = c.arg(0).cloned().unwrap_or(Value::Null);
            // legsText (harness): the legs as parseTraverse reads them
            let legs: Vec<tv::Leg> = match o.get("legsText").and_then(Value::as_str) {
                Some(t) => tv::parse_traverse(t, o.get("mode").and_then(Value::as_str).unwrap_or("angles")).0,
                None => arr(o.get("legs")).iter().map(tv::read_leg).collect(),
            };
            let close = o.get("close").filter(|v| v.is_object()).map(read_p2);
            let r = tv::run_traverse(
                read_p2(o.get("start").unwrap_or(&Value::Null)),
                o.get("mode").and_then(Value::as_str).unwrap_or("angles"),
                opt_f(o.get("backsight"), 0.0),
                &legs,
                close,
                o.get("closingAngle").filter(|v| !v.is_null()).map(|v| numf(Some(v))),
            );
            let j = r.j();
            keep(reg, c, r);
            Some(Ok(j))
        }
        "tv.report" => {
            let r = need!(reg, c.this, tv::Traverse, "traverse");
            ok(&tv::traverse_report(r, c.arg(0).and_then(|o| o.get("units")).and_then(Value::as_str).unwrap_or("ft")))
        }
        "tv.dir2az" => ok(&tv::dir2az(c.num(0))),
        _ => None,
    }
}

/// A design / ground callback described in JSON: { profile: name } (elevation at the station), { surface: name }
/// (z at the place), { flat: z }.
fn z_source<'a>(reg: &'a Registry, v: Option<&Value>) -> Box<dyn Fn(f64, f64, f64) -> Option<f64> + 'a> {
    let v = v.cloned().unwrap_or(Value::Null);
    if let Some(p) = v.get("profile").and_then(Value::as_str).and_then(|n| reg.get::<Profile>(n)) {
        return Box::new(move |sta, _x, _y| p.elevation_at(sta));
    }
    if let Some(s) = v.get("surface").and_then(Value::as_str).and_then(|n| reg.get::<Surface>(n)) {
        return Box::new(move |_sta, x, y| s.z_at(x, y));
    }
    if let Some(z) = v.get("flat").and_then(Value::as_f64) {
        return Box::new(move |_, _, _| Some(z));
    }
    Box::new(|_, _, _| None)
}

fn sk_ops(reg: &mut Registry, c: &Call) -> R {
    use staking as sk;
    match c.op {
        "sk.parseOffsets" => Some(Ok(sk::offsets_json(&sk::parse_offsets(&crate::js::string_or_empty(c.arg(0)))))),
        "sk.cutFillText" => ok(&sk::cut_fill_text(c.arg(0).filter(|v| !v.is_null()).map(|v| numf(Some(v))))),
        "sk.offsetText" => ok(&sk::offset_text(c.num(0))),
        "sk.csv" => {
            let rows: Vec<sk::Row> = arr(c.arg(0)).iter().map(sk::read_row).collect();
            let o = c.arg(1);
            ok(&sk::staking_csv(
                &rows,
                o.and_then(|o| o.get("units")).and_then(Value::as_str).unwrap_or("ft"),
                o.and_then(|o| o.get("title")).and_then(Value::as_str).unwrap_or(""),
            ))
        }
        _ => {
            let al = need!(reg, c.arg(0).and_then(Value::as_str), Alignment, "alignment");
            match c.op {
                "sk.stations" => {
                    let o = c.arg(1);
                    let keys = o.and_then(|o| o.get("keys")).and_then(Value::as_bool).unwrap_or(true);
                    ok(&sk::stake_stations(al, o_f(o, "from", f64::NAN), o_f(o, "to", f64::NAN), o_f(o, "step", 50.0), keys))
                }
                "sk.rows" => {
                    let o = c.arg(1).cloned().unwrap_or(Value::Null);
                    let stations: Vec<f64> = arr(o.get("stations")).iter().map(|x| numf(Some(x))).collect();
                    let offsets: Vec<f64> = o
                        .get("offsets")
                        .filter(|v| v.is_array())
                        .map_or(vec![0.0], |v| arr(Some(v)).iter().map(|x| numf(Some(x))).collect());
                    let d = z_source(reg, o.get("design"));
                    let g = z_source(reg, o.get("ground"));
                    let rows = sk::stake_rows(
                        al,
                        &stations,
                        &offsets,
                        &|sta, _off, x, y| d(sta, x, y),
                        &|x, y| g(f64::NAN, x, y),
                        opt_f(o.get("first"), 1.0),
                        o.get("prefix").and_then(Value::as_str).unwrap_or(""),
                    );
                    ok(&rows)
                }
                "sk.pointRows" => {
                    let o = c.arg(2).cloned().unwrap_or(Value::Null);
                    let d = z_source(reg, o.get("design"));
                    let has_g = o.get("ground").is_some_and(|v| !v.is_null());
                    let g = z_source(reg, o.get("ground"));
                    let gf = |x: f64, y: f64| g(f64::NAN, x, y);
                    let (rows, outside) =
                        sk::point_rows(al, &read_pts(c.arg(1)), &|sta, _off, x, y| d(sta, x, y), if has_g { Some(&gf) } else { None });
                    Some(Ok(json!({ "rows": rows.j(), "outside": outside })))
                }
                _ => None,
            }
        }
    }
}

fn mh_ops(reg: &mut Registry, c: &Call) -> R {
    match c.op {
        "mh.massHaul" => {
            let rows: Vec<sections::Row> = arr(c.arg(0))
                .iter()
                .map(|r| {
                    let n = |k: &str| numf(r.get(k));
                    sections::Row {
                        sta: n("sta"),
                        cut: n("cut"),
                        fill: n("fill"),
                        cut_vol: n("cutVol"),
                        fill_vol: n("fillVol"),
                        cum_cut: n("cumCut"),
                        cum_fill: n("cumFill"),
                    }
                })
                .collect();
            let m = masshaul::mass_haul(&rows, o_f(c.arg(1), "factor", 1.0));
            let j = m.j();
            keep(reg, c, m);
            Some(Ok(j))
        }
        "mh.project" => {
            // massHaul(project.sections(alignment).rows, opts)
            let name = c.arg(0).and_then(Value::as_str).unwrap_or("");
            let Some(p) = reg.get_mut::<Project>(name) else { return Some(Err(format!("no object {name}"))) };
            let Some(a) = al_by_id(p, c.str(1)) else { return Some(Ok(Value::Null)) };
            let Some(set) = p.sections(&a) else { return Some(Ok(Value::Null)) };
            let m = masshaul::mass_haul(&set.rows, o_f(c.arg(2), "factor", 1.0));
            let j = m.j();
            keep(reg, c, m);
            Some(Ok(j))
        }
        "mh.csv" => {
            let m = need!(reg, c.this, masshaul::MassHaul, "mass haul");
            let units = c.arg(0).and_then(|o| o.get("units")).and_then(Value::as_str).unwrap_or("ft");
            ok(&masshaul::mass_haul_csv(m, units, &crate::js::num_to_string))
        }
        _ => None,
    }
}

fn wpis(v: Option<&Value>) -> Vec<solve_why::WPi> {
    arr(v).iter().map(|p| solve_why::WPi { p: read_p2(p), r: crate::js::number(p.get("r")) }).collect()
}
fn why_ops(c: &Call) -> R {
    match c.op {
        "why.curve" => Some(Ok(Value::Array(solve_why::curve_why(&wpis(c.arg(0)), o_f(c.arg(1), "minR", 0.0))))),
        "why.maxRadius" => ok(&solve_why::max_radius(&wpis(c.arg(0)), crate::js::usize_of(c.num(1)).unwrap_or(usize::MAX))),
        "why.vertical" => {
            let pv: Vec<profile::Pvi> = arr(c.arg(0)).iter().map(profile::read_pvi).collect();
            Some(Ok(Value::Array(solve_why::vertical_why(&pv))))
        }
        _ => None,
    }
}

fn clip_ops(c: &Call) -> R {
    match c.op {
        "clip.pointsText" => ok(&clip::points_text(&read_pts(c.arg(0)), c.arg(1).and_then(Value::as_str).unwrap_or("PNEZD"))),
        "clip.make" => {
            let o = c.arg(1);
            let g = |k: &str, d: &'static str| o.and_then(|o| o.get(k)).and_then(Value::as_str).map_or(d.to_string(), str::to_string);
            Some(Ok(clip::make_clip(&read_pts(c.arg(0)), &g("units", "ft"), &g("order", "PNEZD"), &g("source", ""), 0.0)))
        }
        "clip.read" => ok(&clip::read_clip(c.raw(0).unwrap_or(&Value::Null))),
        "clip.textPoints" => {
            let (pts, res) = clip::text_points(&crate::js::string_or_empty(c.arg(0)), c.arg(1).and_then(Value::as_str).unwrap_or("PNEZD"));
            Some(Ok(json!({ "points": pts.j(), "res": res.j() })))
        }
        "clip.choose" => {
            let own = c.arg(0).and_then(clip::read_clip);
            let st = c.raw(1).and_then(Value::as_str);
            Some(Ok(clip::choose_paste(own.as_ref(), st, c.arg(2).and_then(Value::as_str).unwrap_or("PNEZD"))))
        }
        "clip.renumber" => {
            let o = c.arg(2);
            let mode = o.and_then(|o| o.get("mode")).and_then(Value::as_str).unwrap_or("clash");
            let start = o.and_then(|o| o.get("start")).filter(|v| !v.is_null()).map(|v| numf(Some(v)));
            // `String(p.n ?? p)`: point objects or plain numbers
            let existing: Vec<String> = arr(c.arg(1))
                .iter()
                .map(|p| match p.get("n").filter(|n| !n.is_null()) {
                    Some(n) => crate::js::to_js_string(n),
                    None if p.is_object() => "[object Object]".to_string(),
                    None => crate::js::to_js_string(p),
                })
                .collect();
            let (pts, changed, kept) = clip::renumber(&read_pts(c.arg(0)), &existing, mode, start);
            let ch: Vec<Value> = changed.iter().map(|(a, b)| json!([a, b])).collect();
            Some(Ok(json!({ "points": pts.j(), "changed": ch, "kept": kept })))
        }
        "clip.renumberText" => {
            let changed: Vec<(String, String)> =
                arr(c.arg(0)).iter().map(|p| (crate::js::string_or_empty(p.get(0)), crate::js::string_or_empty(p.get(1)))).collect();
            ok(&clip::renumber_text(&changed, crate::js::usize_of(c.num(1)).unwrap_or(0)))
        }
        "clip.moveBy" => ok(&clip::move_by(&read_pts(c.arg(0)), c.num(1), c.num(2))),
        "clip.centre" => ok(&clip::centre(&read_pts(c.arg(0)))),
        _ => None,
    }
}

fn pe_ops(c: &Call) -> R {
    match c.op {
        "pe.check" => {
            let empty = Map::new();
            let patch = c.arg(2).and_then(Value::as_object).unwrap_or(&empty);
            ok(&point_edit::check_point_edit(&read_pts(c.arg(0)), &crate::js::string_or_empty(c.arg(1)), patch))
        }
        _ => None,
    }
}

fn drive_ops(reg: &mut Registry, c: &Call) -> R {
    match c.op {
        "drive.speeds" => {
            let v: Vec<Value> = drive::drive_speeds(c.str(0)).into_iter().map(|(l, p)| json!({ "label": l, "perSec": p.j() })).collect();
            Some(Ok(Value::Array(v)))
        }
        "drive.eye" => ok(&drive::eye_height(c.str(0))),
        "drive.sun" => {
            let [az, alt, decl, eot] = drive::sun_position(c.num(0), c.num(1), c.num(2));
            Some(Ok(json!({ "azimuth": az.j(), "altitude": alt.j(), "declination": decl.j(), "eot": eot.j() })))
        }
        "drive.sunVector" => ok(&drive::sun_vector(c.num(0), c.num(1))),
        "drive.frames" => {
            let al = need!(reg, c.arg(0).and_then(Value::as_str), Alignment, "alignment");
            let cfg = c.arg(1).cloned().unwrap_or(Value::Null);
            let prof = cfg.get("profile").and_then(Value::as_str).and_then(|n| reg.get::<Profile>(n));
            let surf = cfg.get("ground").and_then(Value::as_str).and_then(|n| reg.get::<Surface>(n));
            let gf = |x: f64, y: f64| surf.and_then(|s| s.z_at(x, y));
            let ground: Option<&dyn Fn(f64, f64) -> Option<f64>> = if surf.is_some() { Some(&gf) } else { None };
            let mut elev = drive::elevation_along(al, prof, ground);
            let o = c.arg(3);
            let origin = o.and_then(|o| o.get("origin")).map_or([0.0; 3], |v| [numf(v.get("x")), numf(v.get("y")), numf(v.get("z"))]);
            let mut out = Vec::new();
            for s in arr(c.arg(2)) {
                let f = drive::drive_frame(
                    al,
                    &mut elev,
                    numf(Some(s)),
                    o_f(o, "eye", 3.5),
                    o_f(o, "ahead", 100.0),
                    origin,
                    o_f(o, "exag", 1.0),
                );
                out.push(f.j());
            }
            Some(Ok(Value::Array(out)))
        }
        _ => None,
    }
}

/// `change` ops: [{ set: [path…], value } | { push: [path…], value } | { del: [path…] }].
pub fn apply_ops(state: &mut Value, ops: &[Value]) {
    for op in ops {
        let (kind, path) = if let Some(p) = op.get("set") {
            ("set", p)
        } else if let Some(p) = op.get("push") {
            ("push", p)
        } else if let Some(p) = op.get("del") {
            ("del", p)
        } else {
            continue;
        };
        let keys: Vec<Value> = arr(Some(path)).to_vec();
        let Some((last, parent_path)) = keys.split_last() else { continue };
        let mut ptr = String::new();
        for k in parent_path {
            ptr.push('/');
            let seg = match k {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                _ => String::new(),
            };
            ptr.push_str(&seg.replace('~', "~0").replace('/', "~1"));
        }
        let Some(cur) = state.pointer_mut(&ptr) else { continue };
        let value = op.get("value").cloned().unwrap_or(Value::Null);
        match (kind, last, cur) {
            ("set", Value::String(s), Value::Object(m)) => {
                m.insert(s.clone(), value);
            }
            ("set", Value::Number(n), Value::Array(a)) => {
                if let Some(slot) = n.as_u64().and_then(|i| usize::try_from(i).ok()).and_then(|i| a.get_mut(i)) {
                    *slot = value;
                }
            }
            ("push", Value::String(s), Value::Object(m)) => {
                if let Some(Value::Array(a)) = m.get_mut(s) {
                    a.push(value);
                }
            }
            ("del", Value::String(s), Value::Object(m)) => {
                m.shift_remove(s);
            }
            ("del", Value::Number(n), Value::Array(a)) => {
                if let Some(i) = n.as_u64().and_then(|i| usize::try_from(i).ok())
                    && i < a.len()
                {
                    a.remove(i);
                }
            }
            _ => {}
        }
    }
}

fn al_by_id(p: &Project, id: &str) -> Option<Value> {
    arr(p.state.get("alignments")).iter().find(|a| a.get("id").and_then(Value::as_str) == Some(id)).cloned()
}

fn project_ops(reg: &mut Registry, c: &Call) -> R {
    if c.op == "project.new" {
        let p = Project::new();
        let j = project::view(&p);
        keep(reg, c, p);
        return Some(Ok(j));
    }
    if c.op == "project.reload" {
        // fromJSON(JSON.parse(JSON.stringify(p.toJSON()))) with the tamper ops applied to the file first
        fn nulls(v: &mut Value) {
            match v {
                Value::String(s) if s == "$NaN" || s == "$Inf" || s == "$-Inf" => *v = Value::Null,
                Value::Array(a) => a.iter_mut().for_each(nulls),
                Value::Object(m) => m.values_mut().for_each(nulls),
                _ => {}
            }
        }
        let name = c.this.unwrap_or("");
        let Some(p) = reg.get::<Project>(name) else { return Some(Err(format!("no object {name}"))) };
        let mut j = p.to_json();
        nulls(&mut j);
        apply_ops(&mut j, arr(c.arg(0)));
        return Some(match Project::from_json(&j) {
            Ok((q, notes)) => {
                let out = json!({ "notes": notes, "view": project::view(&q) });
                keep(reg, c, q);
                Ok(out)
            }
            Err(e) => Err(e),
        });
    }
    if c.op == "project.fromJSON" {
        return Some(match Project::from_json(c.raw(0).unwrap_or(&Value::Null)) {
            Ok((p, notes)) => {
                let j = json!({ "notes": notes, "view": project::view(&p) });
                keep(reg, c, p);
                Ok(j)
            }
            Err(e) => Err(e),
        });
    }
    let name = c.this.unwrap_or("");
    let Some(p) = reg.get_mut::<Project>(name) else { return Some(Err(format!("no object {name}"))) };
    let r: Value = match c.op {
        "project.view" => project::view(p),
        "project.setPoints" => {
            p.set_points(read_pts(c.arg(0)), &crate::js::string_or_empty(c.arg(1)));
            Value::Null
        }
        "project.editPoints" => {
            let e = c.arg(1).cloned().unwrap_or(Value::Null);
            let done = p.edit_points(c.str(0), &mut |mut pts: Vec<points::Pt>, _s: &mut Value| {
                if e.get("none").is_some() {
                    return None;
                }
                if let Some(set) = e.get("set") {
                    pts = read_pts(Some(set));
                }
                if let Some(rm) = e.get("remove") {
                    let rm = str_list(Some(rm));
                    pts.retain(|q| !rm.contains(&q.n));
                }
                if let Some(add) = e.get("add") {
                    pts.extend(read_pts(Some(add)));
                }
                Some(pts)
            });
            Value::Bool(done)
        }
        "project.change" => {
            let ops: Vec<Value> = arr(c.arg(1)).to_vec();
            Value::Bool(p.change(c.str(0), &mut |s: &mut Value| apply_ops(s, &ops)))
        }
        "project.setDisplay" => {
            p.set_display(c.str(0), c.raw(1).cloned().unwrap_or(Value::Null));
            Value::Null
        }
        "project.undo" => p.undo().j(),
        "project.redo" => p.redo().j(),
        "project.nextId" => p.next_id(c.str(0)).j(),
        "project.setTin" => {
            let ops: Vec<Value> = arr(c.arg(2)).to_vec();
            let tin = c.raw(1).cloned().unwrap_or(Value::Null);
            if ops.is_empty() {
                Value::Bool(p.set_tin(c.str(0), tin, None))
            } else {
                Value::Bool(p.set_tin(c.str(0), tin, Some(&mut |s: &mut Value| apply_ops(s, &ops))))
            }
        }
        "project.setCoding" => {
            let label = c.arg(2).and_then(Value::as_str).map_or_else(|| crate::i18n::t("codes.undo", &[]), str::to_string);
            Value::Bool(p.set_coding(c.raw(0), c.raw(1), &label))
        }
        "project.groups" => Value::Array(p.groups().into_iter().map(|(k, n, u)| json!({ "key": k, "count": n, "use": u })).collect()),
        "project.figures" => {
            let only = c.arg(0).filter(|v| v.is_array()).map(|v| str_list(Some(v)));
            p.figures(only.as_deref()).j()
        }
        "project.egBreaklines" => {
            // group lines, then the drawn ones (the state's own objects), then feature lines (with their id)
            let all = p.eg_breaklines();
            let feats = crate::featureline::feature_breaklines(&p.features(), "eg");
            let drawn = arr(p.state.get("surface").and_then(|s| s.get("breaklines"))).to_vec();
            let n_group = all.len().saturating_sub(feats.len()).saturating_sub(drawn.len());
            let mut out: Vec<Value> = all.iter().take(n_group).map(J::j).collect();
            out.extend(drawn);
            for (b, id) in &feats {
                let mut v = b.j();
                if let (Some(m), Some(id)) = (v.as_object_mut(), id) {
                    m.insert("feature".into(), Value::String(id.clone()));
                }
                out.push(v);
            }
            Value::Array(out)
        }
        "project.egPoints" => p.eg_points().j(),
        "project.eg" => crate::script_ops::built_json(&p.eg()),
        "project.contours" => p.contours().j(),
        "project.alignment" => match al_by_id(p, c.str(0)) {
            Some(a) => p.alignment(&a).j(),
            None => Value::Null,
        },
        "project.egProfile" => match al_by_id(p, c.str(0)) {
            Some(a) => p.eg_profile(&a).j(),
            None => Value::Null,
        },
        "project.designProfile" => match al_by_id(p, c.str(0)) {
            Some(a) => p.design_profile(&a).j(),
            None => Value::Null,
        },
        "project.graded" => p.graded().j(),
        "project.sections" => match al_by_id(p, c.str(0)) {
            Some(a) => p.sections(&a).map_or(Value::Null, |s| s.j()),
            None => Value::Null,
        },
        "project.volumes" => p.volumes(opt_f(c.arg(0), f64::NAN)).unwrap_or(Value::Null),
        "project.networkCheck" => p.network_check().map_or(Value::Null, |a| a.j()),
        "project.toJSON" => p.to_json(),
        "project.editPoint" => {
            let empty = Map::new();
            let patch = c.arg(1).and_then(Value::as_object).unwrap_or(&empty);
            p.edit_point(c.str(0), patch, c.arg(2).and_then(Value::as_str))
        }
        _ => return None,
    };
    Some(Ok(r))
}

// ---------------------------------------------------------------------------- site3d

fn s3_summary(i: &crate::site3d::SiteInput) -> Value {
    use crate::site3d::Owner;
    // undefined (None) fields are dropped like the JS's
    let idn = |id: &Option<Value>, name: &Option<Value>, rest: Value| {
        let mut m = serde_json::Map::new();
        if let Some(v) = id {
            m.insert("id".into(), v.clone());
        }
        if let Some(v) = name {
            m.insert("name".into(), v.clone());
        }
        if let Value::Object(r) = rest {
            m.extend(r);
        }
        Value::Object(m)
    };
    json!({
        "units": i.units,
        "eg": i.eg().map(|s| s.n_t),
        "fg": i.fg().map(|s| s.n_t),
        "contours": { "eg": i.contours_eg.len(), "fg": i.contours_fg.len() },
        "interval": crate::js::jnum(i.interval), "major": crate::js::jnum(i.major), "bandCount": crate::js::jnum(i.band_count),
        "breaklines": i.breaklines.iter().map(|b| json!({ "owner": Owner::j(&b.owner), "n": b.pts.len() })).collect::<Vec<_>>(),
        "features": i.features.iter().map(|f| idn(&f.id, &f.name, json!({ "n": f.pts.len() }))).collect::<Vec<_>>(),
        "alignments": i.alignments.iter().map(|a| idn(&a.id, &a.name, json!({ "ok": a.alignment.as_ref().is_some_and(|x| x.ok()), "pok": a.profile.as_ref().is_some_and(|x| x.ok()) }))).collect::<Vec<_>>(),
        "corridors": i.corridors.iter().map(|c| c.iter().map(|(k, v)| json!([k, v.len()])).collect::<Vec<_>>()).collect::<Vec<_>>(),
        "pads": i.pads.iter().map(|p| idn(&p.id, &p.name, json!({ "z": crate::js::jnum(p.z), "n": p.pts.len(), "day": p.daylight.len() }))).collect::<Vec<_>>(),
        "check": i.check.as_ref().map(|c| c.problems.len()),
        "points": i.points.len(),
        "gis": i.gis.iter().map(|g| json!([g.id, g.features.len()])).collect::<Vec<_>>(),
    })
}

fn s3_ops(reg: &mut Registry, c: &Call) -> R {
    use crate::site3d as s3;
    match c.op {
        "s3.input" => {
            let name = c.arg(0).and_then(Value::as_str).unwrap_or("");
            let o = c.arg(1);
            let Some(p) = reg.get_mut::<Project>(name) else { return Some(Err(format!("no object {name}"))) };
            let mut i = s3::site_input(p);
            if o.and_then(|o| o.get("noProfile")).is_some_and(|v| crate::js::truthy(Some(v))) {
                for a in &mut i.alignments {
                    a.profile = None;
                }
            }
            if let Some(n) = o.and_then(|o| o.get("network")).filter(|v| !v.is_null()) {
                let net = crate::pipes::read_network(n);
                let g = p.graded();
                let eg = p.eg();
                let surf = g.surface.as_deref().or(eg.surface.as_ref());
                i.check = Some(crate::pipes::analyze_network(&net, surf, &i.units));
                i.network = Some(net);
            }
            let j = s3_summary(&i);
            keep(reg, c, i);
            Some(Ok(j))
        }
        "s3.inputRaw" => {
            let o = c.arg(0).cloned().unwrap_or(Value::Null);
            let units = crate::js::string_or_empty(o.get("units"));
            let mut i = s3::SiteInput::empty(&units);
            if let Some(n) = o.get("network").filter(|v| !v.is_null()) {
                let net = crate::pipes::read_network(n);
                i.check = Some(crate::pipes::analyze_network(&net, None, &units));
                i.network = Some(net);
            }
            i.points = read_pts(o.get("points"));
            let j = s3_summary(&i);
            keep(reg, c, i);
            Some(Ok(j))
        }
        "s3.build" => {
            let name = c.arg(0).and_then(Value::as_str).unwrap_or("");
            let Some(i) = reg.get::<s3::SiteInput>(name) else { return Some(Err(format!("no object {name}"))) };
            let sc = s3::build_site3d(i, &s3::Opts::read(c.arg(1)));
            let j = sc.j();
            keep(reg, c, sc);
            Some(Ok(j))
        }
        "s3.bandLevels" => ok(&s3::band_levels(c.num(0), c.num(1), c.num(2))),
        "s3.bandColor" => Some(Ok(Value::String(s3::band_color(c.num(0), crate::js::truthy(c.arg(1)))))),
        "s3.rgb" => ok(&s3::rgb(&crate::js::string_or_empty(c.arg(0))).to_vec()),
        _ => None,
    }
}

/// The agent tools: `agent.tools`; `agent.run [project, tool, args]` → `{result, selected}` (the selection lives in the
/// registry as `$agentSel`, like the test page's).
fn agent_ops(reg: &mut Registry, c: &Call) -> R {
    use crate::civil_agent as ag;
    match c.op {
        "agent.tools" => Some(Ok(ag::tools_json())),
        "agent.run" => {
            let mut sel: Vec<String> = reg.take::<Vec<String>>("$agentSel").unwrap_or_default();
            let name = c.arg(0).and_then(Value::as_str).unwrap_or("");
            let tool = crate::js::string_or_empty(c.arg(1));
            let a = c.arg(2).cloned().unwrap_or_else(|| json!({}));
            let r = match reg.get_mut::<Project>(name) {
                None => Err(format!("no object {name}")),
                Some(p) => {
                    let mut select = |nums: &[String], mode: &str| {
                        if mode == "replace" {
                            sel.clear();
                            for n in nums {
                                if !sel.contains(n) {
                                    sel.push(n.clone());
                                }
                            }
                        } else {
                            for n in nums {
                                if mode == "add" {
                                    if !sel.contains(n) {
                                        sel.push(n.clone());
                                    }
                                } else {
                                    sel.retain(|x| x != n);
                                }
                            }
                        }
                        sel.len()
                    };
                    ag::run(&tool, p, &a, &mut select)
                }
            };
            let n = sel.len();
            reg.put("$agentSel", sel);
            Some(r.map(|v| json!({ "result": v, "selected": n })))
        }
        _ => None,
    }
}
