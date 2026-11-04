//! The `sheets` step of `cadcraft-cli bimdraw` (bmrestooldiff.mjs): the drawing set built from the drawings the JS
//! made of the same model — its plan scenes, its model triangles for the elevations and sections, its schedules and
//! framing — so the comparison is of res-views.js and res-sheets.js themselves.

use serde_json::{Value as J, json};

use crate::draw::m::V2;
use crate::draw::openings::{Arc, Symbol};
use crate::draw::plan::{Dim, SCut, SRoom, SSym, SWall, Scene};
use crate::draw::ressheets::{Align, FrameBox, Item, SetInput, build_set, set_pdf};
use crate::draw::resviews::{CutAt, Elem, View, project_view};
use crate::draw::schedule::Schedule;
use crate::js::jn;

fn f(v: Option<&J>) -> f64 {
    v.and_then(J::as_f64).unwrap_or(f64::NAN)
}
fn of(v: Option<&J>) -> Option<f64> {
    v.and_then(J::as_f64)
}
fn arr(v: Option<&J>) -> &[J] {
    v.and_then(J::as_array).map_or(&[], Vec::as_slice)
}
fn p2(v: &J) -> V2 {
    [f(v.get(0)), f(v.get(1))]
}
fn pts(v: Option<&J>) -> Vec<V2> {
    arr(v).iter().map(p2).collect()
}
fn seg4(v: Option<&J>) -> Vec<[f64; 4]> {
    arr(v).iter().map(|q| [f(q.get(0)), f(q.get(1)), f(q.get(2)), f(q.get(3))]).collect()
}
fn flat(v: Option<&J>) -> Vec<f64> {
    arr(v).iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect()
}
fn st(v: Option<&J>) -> String {
    v.and_then(J::as_str).unwrap_or("").to_string()
}

/// A plan scene from the harness's JSON (job.rs `scene_json`'s shape).
pub fn scene_from_json(o: &J) -> Scene {
    Scene {
        walls: arr(o.get("walls"))
            .iter()
            .map(|w| SWall {
                key: f(w.get("key")),
                guid: st(w.get("guid")),
                poly: pts(w.get("poly")),
                pieces: arr(w.get("pieces")).iter().map(|p| pts(Some(p))).collect(),
            })
            .collect(),
        symbols: arr(o.get("symbols"))
            .iter()
            .map(|s| SSym {
                key: f(s.get("key")),
                guid: s.get("guid").and_then(J::as_str).map(str::to_string),
                roof: s.get("roof").and_then(J::as_bool).unwrap_or(false),
                sym: Symbol {
                    lines: seg4(s.get("lines")),
                    thin: seg4(s.get("thin")),
                    dashed: seg4(s.get("dashed")),
                    poly: pts(s.get("poly")),
                    arcs: arr(s.get("arcs"))
                        .iter()
                        .map(|a| Arc { c: a.get("c").map_or([f64::NAN; 2], p2), r: f(a.get("r")), a0: f(a.get("a0")), a1: f(a.get("a1")) })
                        .collect(),
                    fill: Vec::new(),
                },
            })
            .collect(),
        rooms: arr(o.get("rooms"))
            .iter()
            .map(|r| SRoom {
                key: f(r.get("key")),
                poly: pts(r.get("poly")),
                at: r.get("at").map_or([f64::NAN; 2], p2),
                label: arr(r.get("label")).iter().map(|x| x.as_str().unwrap_or("").to_string()).collect(),
            })
            .collect(),
        dims: arr(o.get("dims"))
            .iter()
            .map(|d| Dim {
                a: d.get("a").map_or([f64::NAN; 2], p2),
                b: d.get("b").map_or([f64::NAN; 2], p2),
                text: st(d.get("text")),
                off_px: of(d.get("offPx")),
                off_mm: of(d.get("offMm")),
                off: of(d.get("off")),
                key: of(d.get("key")),
            })
            .collect(),
        cut: arr(o.get("cut"))
            .iter()
            .map(|c| SCut {
                key: f(c.get("key")),
                segs: flat(c.get("segs")),
                loops: arr(c.get("loops")).iter().map(|l| flat(Some(l))).collect(),
            })
            .collect(),
        below: arr(o.get("below")).iter().map(|c| SCut { key: f(c.get("key")), segs: flat(c.get("segs")), loops: Vec::new() }).collect(),
        wall_edges: o.get("wallEdges").and_then(J::as_array).map(|e| e.iter().map(|p| pts(Some(p))).collect()),
    }
}

fn elems(v: Option<&J>) -> Vec<Elem> {
    arr(v).iter().map(|e| Elem { key: f(e.get("key")), cls: st(e.get("cls")), tris: flat(e.get("tris")) }).collect()
}
fn sched(o: &J) -> Schedule {
    let row = |r: &J| {
        arr(Some(r))
            .iter()
            .map(|c| c.as_str().map_or_else(|| crate::js::num_str(c.as_f64().unwrap_or(f64::NAN)), str::to_string))
            .collect::<Vec<_>>()
    };
    Schedule {
        head: row(o.get("head").unwrap_or(&J::Null)),
        rows: arr(o.get("rows")).iter().map(row).collect(),
        total: o.get("total").filter(|t| t.is_array()).map(row),
        nums: Vec::new(),
    }
}

pub fn view_json(v: &View) -> J {
    json!({
        "lines": v.lines.iter().map(|l| l.iter().map(|x| jn(*x)).collect::<Vec<_>>()).collect::<Vec<_>>(),
        "cuts": v.cuts.iter().map(|c| json!({ "key": jn(c.key), "cls": c.cls, "loops": c.loops.iter().map(|l| l.iter().map(|q| json!([jn(q[0]), jn(q[1])])).collect::<Vec<_>>()).collect::<Vec<_>>() })).collect::<Vec<_>>(),
        "bounds": v.bounds.map(|b| b.iter().map(|x| jn(*x)).collect::<Vec<_>>()),
        "n": v.n,
        "ground": v.ground,
    })
}
pub fn item_json(it: &Item) -> J {
    let pj = |p: &[V2]| p.iter().map(|q| json!([jn(q[0]), jn(q[1])])).collect::<Vec<_>>();
    match it {
        Item::Line { pts, w, g, dash } => {
            json!({ "t": "line", "pts": pj(pts), "w": jn(*w), "g": jn(*g), "dash": dash.map(|d| vec![jn(d[0]), jn(d[1])]) })
        }
        Item::Fill { polys, g } => json!({ "t": "fill", "polys": polys.iter().map(|p| pj(p)).collect::<Vec<_>>(), "g": jn(*g) }),
        Item::Text { s, x, y, size, align, bold, rot, g } => json!({
            "t": "text", "s": s, "x": jn(*x), "y": jn(*y), "size": jn(*size),
            "align": match align { Align::Left => "left", Align::Right => "right", Align::Center => "center" },
            "bold": bold, "rot": jn(*rot), "g": jn(*g),
        }),
    }
}

/// The step: { input: { plans, elevations, sections, schedules, framing }, paper, units, project, date }.
pub fn sheets_step(o: &J) -> J {
    let input = o.get("input").unwrap_or(&J::Null);
    let paper = st(o.get("paper"));
    let units = st(o.get("units"));
    let scenes: Vec<(String, Scene)> =
        arr(input.get("plans")).iter().map(|p| (st(p.get("title")), scene_from_json(p.get("scene").unwrap_or(&J::Null)))).collect();
    let view_of = |v: &J| {
        let cut = v.get("o").map(|q| CutAt { o: p2(q), far: v.get("far").and_then(J::as_f64).unwrap_or(0.0) });
        (
            st(v.get("title")),
            project_view(&elems(v.get("elements")), v.get("dir").map_or([0.0, 1.0], p2), cut, 1600.0, [3000.0, 1500.0, 500.0]),
        )
    };
    let els: Vec<(String, View)> = arr(input.get("elevations")).iter().map(view_of).collect();
    let secs: Vec<(String, View)> = arr(input.get("sections")).iter().map(view_of).collect();
    let schs: Vec<(String, Schedule)> = arr(input.get("schedules")).iter().map(|s| (st(s.get("title")), sched(s))).collect();
    let frs: Vec<(String, Vec<FrameBox>)> = arr(input.get("framing"))
        .iter()
        .map(|fr| {
            let b = arr(fr.get("boxes"))
                .iter()
                .map(|b| FrameBox {
                    role: st(b.get("role")),
                    x0: f(b.get("x0")),
                    x1: f(b.get("x1")),
                    z0: f(b.get("z0")),
                    z1: f(b.get("z1")),
                })
                .collect();
            (st(fr.get("title")), b)
        })
        .collect();
    let set_in = SetInput {
        plans: scenes.iter().map(|(t, s)| (t.clone(), s)).collect(),
        elevations: els.iter().map(|(t, v)| (t.clone(), v)).collect(),
        sections: secs.iter().map(|(t, v)| (t.clone(), v)).collect(),
        schedules: schs.iter().map(|(t, s)| (t.clone(), s)).collect(),
        framing: frs.iter().map(|(t, b)| (t.clone(), b.as_slice())).collect(),
    };
    let project = st(o.get("project"));
    let sheets = build_set(&set_in, &paper, &units, &project, &st(o.get("date")));
    let pdf = set_pdf(&sheets, &paper, &project, None);
    if let Some(out) = o.get("out").and_then(J::as_str) {
        let _ = std::fs::write(out, &pdf);
    }
    json!({
        "views": els.iter().chain(secs.iter()).map(|(_, v)| view_json(v)).collect::<Vec<_>>(),
        "sheets": sheets.iter().map(|s| json!({ "number": s.number, "title": s.title, "items": s.items.iter().map(item_json).collect::<Vec<_>>() })).collect::<Vec<_>>(),
        "pdf": { "bytes": pdf.len(), "digest": crate::digest::bytes(&pdf) },
    })
}
