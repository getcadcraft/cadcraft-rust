//! Stage-5 ops for the call-script runner (mirrors `forks/tools/pcdiff/ops-stage5.mjs`): files PartCraft writes,
//! drawings and the rest of stage 5. Byte outputs go back as `{ "$base64": … }` (the parity kit hashes them).

use crate::drawing::{self as dr, DMesh, View};
use crate::export;
use crate::faces::Mesh;
use crate::js;
use crate::jv;
use crate::script::num_arg;
use serde_json::Value;

fn arg(a: &[Value], i: usize) -> &Value {
    a.get(i).unwrap_or(&Value::Null)
}
#[allow(clippy::cast_possible_truncation)]
fn fround(x: f64) -> f64 {
    f64::from(x as f32)
}
pub(crate) fn mesh_arg(v: &Value) -> Mesh {
    let indices = jv::arr(v.get("indices")).iter().map(|x| x.as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(u32::MAX)).collect();
    let tri_face = v
        .get("triFace")
        .filter(|x| x.is_array())
        .map(|t| jv::arr(Some(t)).iter().map(|x| x.as_i64().and_then(|n| i32::try_from(n).ok()).unwrap_or(-1)).collect());
    Mesh { positions: jv::arr(v.get("positions")).iter().map(|x| fround(num_arg(Some(x)))).collect(), indices, tri_face }
}
/// Standard base64 (the harness's byte carrier).
pub fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((bytes.len().saturating_mul(4) / 3).saturating_add(4));
    for ch in bytes.chunks(3) {
        let b0 = ch.first().copied().unwrap_or(0);
        let b1 = ch.get(1).copied().unwrap_or(0);
        let b2 = ch.get(2).copied().unwrap_or(0);
        let n = (u32::from(b0) << 16) | (u32::from(b1) << 8) | u32::from(b2);
        let c = |s: u32| char::from(T.get(usize::try_from((n >> s) & 63).unwrap_or(0)).copied().unwrap_or(b'A'));
        out.push(c(18));
        out.push(c(12));
        out.push(if ch.len() > 1 { c(6) } else { '=' });
        out.push(if ch.len() > 2 { c(0) } else { '=' });
    }
    out
}
fn bytes_json(b: &[u8]) -> Value {
    jv::obj(vec![("$base64", base64(b).into())])
}
fn text_counts(t: String, c: Value) -> Value {
    jv::obj(vec![("text", t.into()), ("counts", c)])
}

fn p3(v: Option<&Value>) -> [f64; 3] {
    let a = jv::arr(v);
    [num_arg(a.first()), num_arg(a.get(1)), num_arg(a.get(2))]
}
fn m4(v: Option<&Value>) -> [f64; 16] {
    let a = jv::arr(v);
    let mut out = [f64::NAN; 16];
    for (i, o) in out.iter_mut().enumerate() {
        *o = num_arg(a.get(i));
    }
    out
}
fn cn_arg(v: Option<&Value>) -> Option<Vec<f64>> {
    v.filter(|x| x.is_array()).map(|x| jv::arr(Some(x)).iter().map(|y| fround(num_arg(Some(y)))).collect())
}
fn cand_json(c: &crate::measure::Cand) -> Value {
    jv::obj(vec![("kind", c.kind.into()), ("p", jv::pt3(c.p)), ("edge", jv::num(c.edge))])
}
fn rows_json(rows: &[(String, String)]) -> Value {
    Value::Array(rows.iter().map(|(k, v)| Value::Array(vec![k.as_str().into(), v.as_str().into()])).collect())
}
fn geo_json(g: &crate::measure::Geo) -> Value {
    jv::obj(vec![
        ("pts", Value::Array(g.pts.iter().map(|p| jv::pt3(*p)).collect())),
        ("segs", Value::Array(g.segs.iter().map(|s| Value::Array(s.iter().map(|p| jv::pt3(*p)).collect())).collect())),
        ("tris", Value::Array(g.tris.iter().map(|s| Value::Array(s.iter().map(|p| jv::pt3(*p)).collect())).collect())),
    ])
}
/// The harness's projection: a row-major 3×4 matrix; behind the eye (third coordinate < 0) → None.
fn proj(m: &Value) -> impl Fn([f64; 3]) -> Option<[f64; 2]> {
    let a: Vec<f64> = jv::arr(Some(m)).iter().map(|x| num_arg(Some(x))).collect();
    move |p: [f64; 3]| {
        let g = |i: usize| a.get(i).copied().unwrap_or(f64::NAN);
        let row = |r: usize| {
            let b = r.saturating_mul(4);
            g(b) * p[0] + g(b.saturating_add(1)) * p[1] + g(b.saturating_add(2)) * p[2] + g(b.saturating_add(3))
        };
        if row(2) < 0.0 { None } else { Some([row(0), row(1)]) }
    }
}
fn dmesh(v: &Value) -> DMesh {
    let m = mesh_arg(v);
    DMesh { positions: m.positions, indices: m.indices }
}
fn segs(v: Option<&Value>) -> Vec<dr::Seg> {
    jv::arr(v)
        .iter()
        .map(|s| {
            let a = jv::arr(Some(s));
            [num_arg(a.first()), num_arg(a.get(1)), num_arg(a.get(2)), num_arg(a.get(3))]
        })
        .collect()
}
fn segs_json(s: &[dr::Seg]) -> Value {
    Value::Array(s.iter().map(|g| jv::nums(g)).collect())
}
/// composeSheet's options from the op's second argument (date = an ISO string).
fn compose(a: &[Value], views: &[(String, View)], exact: bool, threads: &[Value]) -> Result<Value, String> {
    let o = arg(a, 1);
    let holes: Option<Vec<dr::Hole>> =
        o.get("holes").filter(|h| h.is_array()).map(|h| jv::arr(Some(h)).iter().map(dr::Hole::from_json).collect());
    let opts = dr::SheetOpts {
        name: match o.get("name") {
            None | Some(Value::Null) => crate::i18n::t("model.drawing.default_name", &[]),
            v => js::string_of(v),
        },
        date_ymd: js::slice16(&js::string_of(o.get("date")), 10),
        paper: o.get("paper").and_then(Value::as_str).map(str::to_string),
        drawing: o.get("drawing").filter(|d| jv::truthy(Some(d))).cloned().unwrap_or(Value::Object(jv::Obj::new())),
        annotate: jv::truthy(o.get("annotate")),
        material: o.get("material").filter(|m| jv::truthy(Some(m))).map(|m| js::string_of(Some(m))).unwrap_or_default(),
        holes: holes.as_deref(),
    };
    dr::compose_sheet(views, exact, threads, &opts)
}

/// The stage-5 ops.
pub fn handle(op: &str, a: &[Value]) -> Option<Result<Value, String>> {
    let name = |i: usize| match a.get(i) {
        None | Some(Value::Null) => "part".to_string(),
        v => js::string_of(v),
    };
    let r = match op {
        "export.stl" => Ok(bytes_json(&export::stl_binary(&mesh_arg(arg(a, 0)), &name(1)))),
        "export.3mf" => Ok(bytes_json(&export::three_mf(&mesh_arg(arg(a, 0)), &name(1)))),
        "export.crc32" => Ok(jv::num(f64::from(export::crc32(js::string_of(a.first()).as_bytes())))),
        "export.zip" => {
            let files: Vec<(String, Vec<u8>)> =
                jv::arr(a.first()).iter().map(|f| (js::string_of(f.get("name")), js::string_of(f.get("data")).into_bytes())).collect();
            Ok(bytes_json(&export::zip_stored(&files)))
        }
        "export.dxfSketch" => {
            let (t, c) = export::dxf_sketch(arg(a, 0));
            Ok(text_counts(t, c))
        }
        "export.dxfLoops" => {
            let loops: Vec<Vec<[f64; 2]>> = jv::arr(a.first())
                .iter()
                .map(|l| {
                    jv::arr(Some(l))
                        .iter()
                        .map(|p| {
                            let q = jv::arr(Some(p));
                            [num_arg(q.first()), num_arg(q.get(1))]
                        })
                        .collect()
                })
                .collect();
            let (t, c) = export::dxf_loops(&loops);
            Ok(text_counts(t, c))
        }
        "export.dxfFlat" => {
            let (t, c) = export::dxf_flat(arg(a, 0));
            Ok(text_counts(t, c))
        }
        "export.step" => {
            let iso = js::slice16(&js::string_of(a.get(2)), 19);
            let (t, c) = export::step_faceted(&mesh_arg(arg(a, 0)), &name(1), &iso, jv::is_str(a.get(3), "AP214"));
            Ok(text_counts(t, c))
        }
        "export.stepReal" => Ok(export::step_real(num_arg(a.first())).into()),
        "draw.sheetLabel" => Ok(dr::sheet_label(&js::string_of(a.first())).into()),
        "draw.nextRevision" => Ok(dr::next_revision(jv::arr(a.first())).into()),
        "draw.scaleLabel" => Ok(dr::scale_label(num_arg(a.first())).into()),
        "draw.meshEdges" => {
            let m = dmesh(arg(a, 0));
            let (edges, normals) = dr::mesh_edges(&m);
            Ok(jv::obj(vec![
                (
                    "edges",
                    Value::Array(
                        edges
                            .iter()
                            .map(|e| {
                                jv::obj(vec![
                                    ("a", jv::unum(e.a)),
                                    ("b", jv::unum(e.b)),
                                    ("t1", jv::unum(e.t1)),
                                    ("t2", e.t2.map_or(jv::num(-1.0), jv::unum)),
                                    ("sharp", e.sharp.into()),
                                ])
                            })
                            .collect(),
                    ),
                ),
                ("normals", jv::nums(&normals.iter().flatten().copied().collect::<Vec<_>>())),
            ]))
        }
        "draw.meshViews" => {
            let m = dmesh(arg(a, 0));
            let iso = !matches!(a.get(1), Some(Value::Bool(false)));
            let mut o = jv::Obj::new();
            for (k, vw) in dr::mesh_views(&m, iso) {
                o.insert(k, vw.to_json());
            }
            Ok(jv::obj(vec![("views", Value::Object(o)), ("exact", false.into())]))
        }
        "draw.mergeSegments" => Ok(segs_json(&dr::merge_segments(&segs(a.first()), num_arg(a.get(1)), &segs(a.get(2))))),
        "draw.hatch" => {
            let ang = match a.get(2) {
                None | Some(Value::Null) => 45.0,
                v => num_arg(v),
            };
            Ok(segs_json(&dr::hatch(&segs(a.first()), num_arg(a.get(1)), ang)))
        }
        "draw.clipToCircle" => {
            let c = jv::arr(a.get(1));
            Ok(segs_json(&dr::clip_to_circle(&segs(a.first()), [num_arg(c.first()), num_arg(c.get(1))], num_arg(a.get(2)))))
        }
        "draw.partHoles" => {
            let g = arg(a, 1);
            let named = jv::truthy(g.get("exact")) || jv::truthy(g.get("named"));
            let faces = g.get("faces").and_then(Value::as_array).map(Vec::as_slice);
            let th = g.get("threads").cloned().unwrap_or(Value::Null);
            let threads =
                |fid: &str| th.get(fid).and_then(|x| x.get("callout")).filter(|c| jv::truthy(Some(c))).map(|c| js::string_of(Some(c)));
            Ok(dr::part_holes(arg(a, 0), named, faces, &threads)
                .map_or(Value::Null, |hs| Value::Array(hs.iter().map(dr::Hole::to_json).collect())))
        }
        "draw.holeTableRows" => {
            let holes: Vec<dr::Hole> = jv::arr(a.first()).iter().map(dr::Hole::from_json).collect();
            let b = arg(a, 2);
            let g = |k: &str| jv::as_num(b.get(k)).unwrap_or(f64::NAN);
            let (rows, skipped) = dr::hole_table_rows(&holes, &js::string_of(a.get(1)), [g("u0"), g("v0"), g("u1"), g("v1")]);
            Ok(jv::obj(vec![
                (
                    "rows",
                    Value::Array(
                        rows.iter()
                            .map(|r| {
                                jv::obj(vec![
                                    ("u", jv::num(r.u)),
                                    ("v", jv::num(r.v)),
                                    ("x", jv::num(r.x)),
                                    ("y", jv::num(r.y)),
                                    ("dia", jv::num(r.dia)),
                                    ("size", r.size.as_str().into()),
                                    ("depth", r.depth.map_or(Value::Null, jv::num)),
                                    ("feature", r.feature.as_str().into()),
                                    ("tag", r.tag.as_str().into()),
                                ])
                            })
                            .collect(),
                    ),
                ),
                ("skipped", jv::unum(skipped)),
            ]))
        }
        "draw.compose" => {
            let data = arg(a, 0);
            let views: Vec<(String, View)> = data
                .get("views")
                .and_then(Value::as_object)
                .map(|o| o.iter().map(|(k, v)| (k.clone(), View::from_json(v))).collect())
                .unwrap_or_default();
            compose(a, &views, jv::truthy(data.get("exact")), jv::arr(data.get("threads")))
        }
        "draw.makeDrawing" => {
            let m = dmesh(arg(a, 0));
            let o = arg(a, 1);
            let views = dr::mesh_views(&m, !matches!(o.get("iso"), Some(Value::Bool(false))));
            compose(a, &views, false, &[])
        }
        "draw.winAnsi" => {
            let (s, w, m) = dr::win_ansi(&js::string_of(a.first()));
            Ok(jv::obj(vec![("str", s.into()), ("width", jv::num(w)), ("missing", jv::unum(m))]))
        }
        "draw.pdfContent" => {
            let (c, m) = dr::pdf_content(arg(a, 0));
            Ok(jv::obj(vec![("content", c.into()), ("missing", jv::unum(m))]))
        }
        "an.draft" => {
            let m = mesh_arg(arg(a, 0));
            let cn = cn_arg(a.get(1));
            let c = arg(a, 4);
            let (cols, ar) = crate::analysis::draft_analysis(
                &m,
                cn.as_deref(),
                p3(a.get(2)),
                num_arg(a.get(3)),
                [p3(c.get("positive")), p3(c.get("needs")), p3(c.get("negative"))],
            );
            Ok(jv::obj(vec![
                ("colors", jv::nums(&cols)),
                (
                    "areas",
                    jv::obj(vec![
                        ("positive", jv::num(ar[0])),
                        ("needs", jv::num(ar[1])),
                        ("negative", jv::num(ar[2])),
                        ("total", jv::num(ar[3])),
                    ]),
                ),
            ]))
        }
        "an.zebra" => {
            let m = mesh_arg(arg(a, 0));
            let cn = cn_arg(a.get(1));
            let c = arg(a, 4);
            Ok(jv::nums(&crate::analysis::zebra_colors(
                &m,
                cn.as_deref(),
                p3(a.get(2)),
                num_arg(a.get(3)),
                p3(c.get("dark")),
                p3(c.get("light")),
            )))
        }
        "an.curvature" => {
            let m = mesh_arg(arg(a, 0));
            let c = arg(a, 1);
            let (cols, lim) = crate::analysis::curvature_colors(&m, p3(c.get("concave")), p3(c.get("flat")), p3(c.get("convex")));
            Ok(jv::obj(vec![("colors", jv::nums(&cols)), ("lim", jv::num(lim))]))
        }
        "an.curvatureRaw" => {
            let (g, mn, b) = crate::analysis::curvature(&mesh_arg(arg(a, 0)));
            Ok(jv::obj(vec![
                ("gauss", jv::nums(&g)),
                ("mean", jv::nums(&mn)),
                ("boundary", bytes_json(&b.iter().map(|x| u8::from(*x)).collect::<Vec<u8>>())),
            ]))
        }
        "an.draftAngle" => Ok(jv::num(crate::analysis::draft_angle(p3(a.first()), p3(a.get(1))))),
        "sec.range" => {
            let b = arg(a, 0);
            let r = crate::analysis::section_range(p3(b.get("min")), p3(b.get("max")), p3(a.get(1)));
            Ok(jv::nums(&r))
        }
        "sec.face" => {
            let f = arg(a, 0);
            let mm: Option<[f64; 16]> = a.get(1).filter(|x| x.is_array()).map(|x| {
                let v = jv::arr(Some(x));
                let mut out = [f64::NAN; 16];
                for (i, o) in out.iter_mut().enumerate() {
                    *o = num_arg(v.get(i));
                }
                out
            });
            let n = f.get("n").filter(|x| jv::truthy(Some(x))).map(|x| p3(Some(x)));
            let o = f.get("o").filter(|x| jv::truthy(Some(x))).map(|x| p3(Some(x)));
            let ty = if jv::truthy(Some(f)) { js::str_of(f.get("type")) } else { String::new() };
            crate::analysis::face_section(&ty, n, o, mm.as_ref()).map(|(n, c)| jv::obj(vec![("n", jv::pt3(n)), ("c", jv::num(c))]))
        }
        "ms.circleOf" => {
            let b = crate::measure::BuildM::from_json(arg(a, 1));
            Ok(crate::measure::circle_of(arg(a, 0), Some(&b), num_arg(a.get(2))).map_or(Value::Null, |(c, r, n)| {
                jv::obj(vec![("c", jv::pt3(c)), ("r", jv::num(r)), ("n", n.map_or(Value::Null, jv::pt3))])
            }))
        }
        "ms.snap" => {
            let b = crate::measure::BuildM::from_json(arg(a, 0));
            Ok(Value::Array(crate::measure::snap_candidates(&b).iter().map(cand_json).collect()))
        }
        "ms.snapAt" => {
            let b = crate::measure::BuildM::from_json(arg(a, 0));
            let cands = crate::measure::snap_candidates(&b);
            let pr = proj(arg(a, 1));
            Ok(crate::measure::snap_at(&cands, &pr, num_arg(a.get(2)), num_arg(a.get(3)), num_arg(a.get(4))).map_or(Value::Null, cand_json))
        }
        "ms.nearestEdge" => {
            let b = crate::measure::BuildM::from_json(arg(a, 0));
            let pr = proj(arg(a, 1));
            let (i, d) = crate::measure::nearest_edge_at(&b, &pr, num_arg(a.get(2)), num_arg(a.get(3)), num_arg(a.get(4)));
            Ok(jv::obj(vec![("index", jv::num(i)), ("d", jv::num(d))]))
        }
        "ms.info" => {
            let b = crate::measure::BuildM::from_json(arg(a, 0));
            crate::measure::info(&b, arg(a, 1))
        }
        "ms.exactSub" => Ok(crate::measure::exact_sub(arg(a, 0))),
        "ms.geometry" => {
            let b = crate::measure::BuildM::from_json(arg(a, 0));
            Ok(geo_json(&crate::measure::geometry_of(&b, arg(a, 1))))
        }
        "ms.pair" => {
            let b = crate::measure::BuildM::from_json(arg(a, 0));
            let (pa, pb) = (arg(a, 1), arg(a, 2));
            match (crate::measure::info(&b, pa), crate::measure::info(&b, pb)) {
                (Ok(ia), Ok(ib)) => {
                    let (ga, gb) = (crate::measure::geometry_of(&b, pa), crate::measure::geometry_of(&b, pb));
                    let res = crate::measure::pair(
                        &js::str_of(pa.get("kind")),
                        &js::str_of(pb.get("kind")),
                        &ia,
                        &ib,
                        a.get(3).filter(|x| !x.is_null()),
                        Some((&ga, &gb)),
                    );
                    let rows = crate::measure::pair_rows(&res);
                    Ok(jv::obj(vec![("res", res), ("rows", rows_json(&rows)), ("text", crate::measure::as_text(&rows).into())]))
                }
                (Err(e), _) | (_, Err(e)) => Err(e),
            }
        }
        "ms.infoRows" => {
            let rows = crate::measure::info_rows(arg(a, 0));
            Ok(jv::obj(vec![("rows", rows_json(&rows)), ("text", crate::measure::as_text(&rows).into())]))
        }
        "ms.meshMass" => {
            let m = mesh_arg(arg(a, 0));
            Ok(crate::measure::mesh_mass(&m.positions, &m.indices).to_json())
        }
        "ms.place" => {
            let p = crate::measure::Mass::from_json(arg(a, 0));
            let mm: Option<[f64; 16]> = a.get(1).filter(|x| x.is_array()).map(|x| {
                let v = jv::arr(Some(x));
                let mut out = [f64::NAN; 16];
                for (i, o) in out.iter_mut().enumerate() {
                    *o = num_arg(v.get(i));
                }
                out
            });
            Ok(crate::measure::place_mass(&p, mm.as_ref(), num_arg(a.get(2))).to_json())
        }
        "ms.combine" => {
            let list: Vec<crate::measure::Mass> = jv::arr(a.first()).iter().map(crate::measure::Mass::from_json).collect();
            Ok(crate::measure::combine_mass(&list).to_json())
        }
        "ms.principal" => {
            let tt = jv::arr(a.first());
            let r = |i: usize| p3(tt.get(i));
            let (mo, ax) = crate::measure::principal(&[r(0), r(1), r(2)]);
            Ok(jv::obj(vec![("moments", jv::nums(&mo)), ("axes", Value::Array(ax.iter().map(|x| jv::pt3(*x)).collect()))]))
        }
        "ms.massRows" => {
            let rows = crate::measure::mass_rows(&crate::measure::Mass::from_json(arg(a, 0)));
            Ok(jv::obj(vec![("rows", rows_json(&rows)), ("text", crate::measure::as_text(&rows).into())]))
        }
        "ms.kindLabel" => Ok(crate::measure::kind_label(&js::string_of(a.first())).into()),
        "adw.fit" => Ok(crate::asm_drawing::fit(&js::string_of(a.first()), num_arg(a.get(1)), num_arg(a.get(2))).into()),
        "adw.balloonsFor" => {
            let anchors: Vec<[f64; 3]> = jv::arr(a.get(2)).iter().map(|x| p3(Some(x))).collect();
            Ok(Value::Array(
                crate::asm_drawing::balloons_for(jv::arr(a.first()), jv::arr(a.get(1)), &anchors)
                    .into_iter()
                    .map(|(item, k)| jv::obj(vec![("item", item), ("anchor", jv::unum(k))]))
                    .collect(),
            ))
        }
        "adw.sheet" => {
            let data = arg(a, 0);
            let view = View::from_json(data.get("view").unwrap_or(&Value::Null));
            let anchors: Vec<[f64; 3]> = jv::arr(data.get("anchors")).iter().map(|x| p3(Some(x))).collect();
            let o = arg(a, 2);
            let balloons: Vec<(Value, usize)> = jv::arr(o.get("balloons"))
                .iter()
                .map(|b| {
                    (
                        b.get("item").cloned().unwrap_or(Value::Null),
                        b.get("anchor").and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok()).unwrap_or(usize::MAX),
                    )
                })
                .collect();
            let trails: Vec<Vec<[f64; 3]>> =
                jv::arr(o.get("trails")).iter().map(|t| jv::arr(Some(t)).iter().map(|p| p3(Some(p))).collect()).collect();
            let name = match o.get("name") {
                None | Some(Value::Null) => "Assembly".to_string(),
                x => js::string_of(x),
            };
            let paper = o.get("paper").and_then(Value::as_str).unwrap_or("a3");
            Ok(crate::asm_drawing::assembly_sheet(
                &view,
                &anchors,
                jv::truthy(data.get("exact")),
                jv::arr(a.get(1)),
                &name,
                &js::slice16(&js::string_of(o.get("date")), 10),
                &balloons,
                &trails,
                o.get("exploded").filter(|x| jv::truthy(Some(x))).and_then(Value::as_str),
                paper,
            ))
        }
        "adw.mesh" => {
            let items: Vec<(Vec<f64>, Vec<u32>, [f64; 16])> = jv::arr(a.first())
                .iter()
                .map(|it| {
                    let m = mesh_arg(it.get("mesh").unwrap_or(&Value::Null));
                    (m.positions, m.indices, m4(it.get("matrix")))
                })
                .collect();
            let (p, i) = crate::asm_drawing::assembly_mesh(&items);
            Ok(jv::obj(vec![("positions", jv::nums(&p)), ("indices", Value::Array(i.iter().map(|x| Value::from(*x)).collect()))]))
        }
        "adw.3mf" => {
            let parts: Vec<(String, String, Vec<f64>, Vec<u32>)> = jv::arr(a.get(1))
                .iter()
                .map(|e| {
                    let x = jv::arr(Some(e));
                    let p = x.get(1).cloned().unwrap_or(Value::Null);
                    let m = mesh_arg(p.get("mesh").unwrap_or(&Value::Null));
                    (js::string_of(x.first()), js::string_of(p.get("name")), m.positions, m.indices)
                })
                .collect();
            let items: Vec<(String, [f64; 16])> =
                jv::arr(a.get(2)).iter().map(|it| (js::string_of(it.get("pid")), m4(it.get("matrix")))).collect();
            Ok(bytes_json(&crate::asm_drawing::assembly_3mf(&js::string_of(a.first()), &parts, &items)))
        }
        "ifc.guid" => Ok(crate::ifc_export::guid_for(&js::string_of(a.first())).into()),
        "ifc.R" => {
            let d = match a.get(1) {
                None | Some(Value::Null) => 6,
                v => js::usize_of(js::i64_of(num_arg(v))),
            };
            Ok(crate::ifc_export::r(num_arg(a.first()), d).into())
        }
        "ifc.Rq" => Ok(crate::ifc_export::rq(num_arg(a.first())).into()),
        "ifc.S" => Ok(crate::ifc_export::s(&match a.first() {
            None | Some(Value::Null) => String::new(),
            v => js::string_of(v),
        })
        .into()),
        "ifc.part" => {
            let g = arg(a, 1);
            let m = mesh_arg(g.get("mesh").unwrap_or(&Value::Null));
            let ts = js::slice16(&js::string_of(a.get(2)), 19);
            let (text, counts) = crate::ifc_export::part_ifc(
                arg(a, 0),
                &m.positions,
                &m.indices,
                g.get("stats").filter(|x| jv::truthy(Some(x))),
                &ts,
                a.get(3).and_then(Value::as_str),
            );
            Ok(jv::obj(vec![("text", text.into()), ("counts", counts)]))
        }
        "ifc.asm" => {
            let builds: std::collections::HashMap<String, crate::ifc_export::BuiltPart> = arg(a, 1)
                .as_object()
                .map(|o| {
                    o.iter()
                        .filter(|(_, b)| jv::truthy(b.get("mesh")))
                        .map(|(k, b)| {
                            let m = mesh_arg(b.get("mesh").unwrap_or(&Value::Null));
                            (
                                k.clone(),
                                crate::ifc_export::BuiltPart {
                                    positions: m.positions,
                                    indices: m.indices,
                                    stats: b.get("stats").filter(|x| jv::truthy(Some(x))).cloned(),
                                },
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            let world: Vec<(String, [f64; 16])> =
                arg(a, 2).as_object().map(|o| o.iter().map(|(k, m)| (k.clone(), m4(Some(m)))).collect()).unwrap_or_default();
            let ts = js::slice16(&js::string_of(a.get(3)), 19);
            crate::ifc_export::assembly_ifc(arg(a, 0), &builds, &world, jv::arr(a.get(4)), &ts, None)
                .map(|(text, counts)| jv::obj(vec![("text", text.into()), ("counts", counts)]))
        }
        "export.xmlEsc" => Ok(export::xml_esc(&js::string_of(a.first())).into()),
        _ => return None,
    };
    Some(r)
}
