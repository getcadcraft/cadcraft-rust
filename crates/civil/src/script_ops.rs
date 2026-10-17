//! The script ops of the core modules (see [`crate::script`]); each mirrors one function of the JS driver.

use crate::alignment::{self, Alignment};
use crate::geo::{read_p2, read_p2s, read_p3};
use crate::js::{J, arr, numf};
use crate::points::{self, read_pt};
use crate::profile::{self, Profile};
use crate::script::{Call, Registry, keep, obj};
use crate::tin::{self, Breakline, Surface, ZPt};
use crate::units;
use serde_json::{Value, json};

type R = Option<Result<Value, String>>;
fn ok<T: J + ?Sized>(v: &T) -> R {
    Some(Ok(v.j()))
}

fn opt_usize(v: Option<&Value>) -> Option<usize> {
    v.and_then(Value::as_f64).and_then(crate::js::usize_of)
}

pub fn units(c: &Call) -> R {
    match c.op {
        "units.formatStation" => ok(&units::format_station(c.num(0), c.str(1), opt_usize(c.arg(2)))),
        "units.parseStation" => ok(&units::parse_station(&crate::js::string_or_empty(c.arg(0)), c.str(1))),
        "units.isNum" => ok(&units::is_num_str(c.str(0))),
        "units.fixed" => ok(&units::fixed(c.num(0), opt_usize(c.arg(1)).unwrap_or(2))),
        "units.grouped" => ok(&units::grouped(c.num(0), opt_usize(c.arg(1)).unwrap_or(0))),
        "units.pct" => ok(&units::pct(c.num(0), opt_usize(c.arg(1)).unwrap_or(2))),
        "units.volumeText" => ok(&units::volume_text(c.num(0), c.str(1), opt_usize(c.arg(2)).unwrap_or(1))),
        "units.areaText" => ok(&units::area_text(c.num(0), c.str(1), opt_usize(c.arg(2)).unwrap_or(0))),
        "units.bearing" => ok(&units::bearing(c.num(0))),
        "units.dms" => ok(&units::dms(c.num(0))),
        _ => None,
    }
}

pub fn read_pts(v: Option<&Value>) -> Vec<points::Pt> {
    arr(v).iter().map(read_pt).collect()
}

pub fn points(c: &Call) -> R {
    match c.op {
        "points.parsePoints" => {
            let o = c.arg(1);
            let format = o.and_then(|o| o.get("format")).and_then(Value::as_str).unwrap_or("auto");
            let prefer = o.and_then(|o| o.get("prefer")).and_then(Value::as_str).unwrap_or("PNEZD");
            ok(&points::parse_points(c.str(0), format, prefer))
        }
        "points.groupKey" => ok(&points::group_key(&crate::js::string_or_empty(c.arg(0)))),
        "points.pointGroups" => {
            let g = points::point_groups(&read_pts(c.arg(0)), None);
            Some(Ok(Value::Array(g.into_iter().map(|(key, count)| json!({ "key": key, "count": count })).collect())))
        }
        "points.groupPoints" => ok(&points::group_points(&read_pts(c.arg(0)), c.str(1), None)),
        "points.writePoints" => ok(&points::write_points(&read_pts(c.arg(0)), opt_usize(c.arg(1)).unwrap_or(3))),
        "points.lassoPoints" => {
            let pts = read_pts(c.arg(0));
            let picked = points::lasso_points(&pts, &read_p2s(c.arg(1)), c.str(2), c.num(3));
            let ns: Vec<String> = picked.into_iter().filter_map(|i| pts.get(i).map(|p| p.n.clone())).collect();
            ok(&ns)
        }
        _ => None,
    }
}

/// buildSurface's input from JSON `{ points, breaklines, boundary, maxEdge }`.
pub fn read_build_input(v: Option<&Value>) -> (Vec<ZPt>, Vec<Breakline>, Option<Vec<crate::geo::P2>>, f64) {
    let v = v.cloned().unwrap_or(Value::Null);
    let pts = arr(v.get("points"))
        .iter()
        .map(|p| ZPt {
            x: numf(p.get("x")),
            y: numf(p.get("y")),
            z: match p.get("z") {
                None | Some(Value::Null) => None,
                z => Some(numf(z)),
            },
        })
        .collect();
    let bls = arr(v.get("breaklines")).iter().map(read_breakline).collect();
    let boundary = v.get("boundary").filter(|b| b.is_array()).map(|b| read_p2s(Some(b)));
    let max_edge = numf(v.get("maxEdge"));
    (pts, bls, boundary, if max_edge.is_nan() { 0.0 } else { max_edge })
}
pub fn read_breakline(b: &Value) -> Breakline {
    Breakline {
        name: crate::js::string_or_empty(b.get("name")),
        pts: arr(b.get("pts")).iter().map(read_p3).collect(),
        from_group: b.get("fromGroup").and_then(Value::as_bool).unwrap_or(false),
    }
}

/// The JSON form of a surface the harness compares (its arrays and origin).
pub fn surface_view(s: &Surface) -> Value {
    json!({
        "X": s.x.j(), "Y": s.y.j(), "Z": s.z.j(), "tris": s.tris.as_vec().j(), "ox": s.ox.j(), "oy": s.oy.j(), "nT": s.n_t,
        "constrainedEdges": s.constrained_edges.j(),
    })
}

pub fn built_json(b: &tin::Built) -> Value {
    json!({ "surface": b.surface.as_ref().map(surface_view), "report": b.report.j() })
}

pub fn tin(reg: &mut Registry, c: &Call) -> R {
    match c.op {
        "tin.buildSurface" => {
            let (pts, bls, bnd, me) = read_build_input(c.arg(0));
            let b = tin::build_surface(&pts, &bls, bnd.as_deref(), me);
            let j = built_json(&b);
            if let Some(s) = b.surface {
                keep(reg, c, s);
            }
            Some(Ok(j))
        }
        "tin.surfaceFromFaces" => {
            let v = c.arg(0).cloned().unwrap_or(Value::Null);
            let pts: Vec<Option<crate::geo::P3>> =
                arr(v.get("pts")).iter().map(|p| if p.is_object() { Some(read_p3(p)) } else { None }).collect();
            let faces: Vec<Vec<f64>> = arr(v.get("faces")).iter().map(|f| arr(Some(f)).iter().map(|x| numf(Some(x))).collect()).collect();
            let b = tin::surface_from_faces(&pts, &faces);
            let j = built_json(&b);
            if let Some(s) = b.surface {
                keep(reg, c, s);
            }
            Some(Ok(j))
        }
        "tin.convexHull" => {
            let raw = arr(c.arg(0));
            Some(Ok(Value::Array(tin::convex_hull_idx(&read_p2s(c.arg(0))).into_iter().filter_map(|i| raw.get(i).cloned()).collect())))
        }
        "tin.polyArea" => ok(&tin::poly_area(&read_p2s(c.arg(0)))),
        "tin.ccw" => {
            let mut raw = arr(c.arg(0)).to_vec();
            if tin::poly_area(&read_p2s(c.arg(0))) < 0.0 {
                raw.reverse();
            }
            Some(Ok(Value::Array(raw)))
        }
        "tin.pointInPoly" => ok(&tin::point_in_poly(c.num(0), c.num(1), &read_p2s(c.arg(2)))),
        "tin.clipConvex" => ok(&tin::clip_convex(&read_p2s(c.arg(0)), &read_p2s(c.arg(1)))),
        _ => None,
    }
}

pub fn surface(reg: &mut Registry, c: &Call) -> R {
    let s: &Surface = match obj(reg, c.this, "surface") {
        Ok(s) => s,
        Err(e) => return Some(Err(e)),
    };
    match c.op {
        "surface.view" => Some(Ok(surface_view(s))),
        "surface.stats" => ok(s.stats()),
        "surface.elevationAt" => {
            let tol = c.arg(2).map_or(1e-9, |v| numf(Some(v)));
            ok(&s.elevation_at(c.num(0), c.num(1), tol))
        }
        "surface.contours" => {
            let o = c.arg(1);
            let base = o.and_then(|o| o.get("base")).map_or(0.0, |v| numf(Some(v)));
            let ml = o.and_then(|o| o.get("maxLevels")).map_or(2000.0, |v| numf(Some(v)));
            ok(&s.contours(c.num(0), base, ml))
        }
        "surface.bands" => {
            let levels: Vec<f64> = arr(c.arg(0)).iter().map(|v| numf(Some(v))).collect();
            ok(&s.bands(&levels))
        }
        "surface.slopes" => ok(&s.slopes()),
        "surface.edges" => ok(&s.edges()),
        "surface.outline" => ok(&s.outline()),
        "surface.toFaces" => {
            let (pts, faces) = s.to_faces();
            Some(Ok(json!({ "pts": pts.j(), "faces": faces.j() })))
        }
        "surface.hull" => ok(&s.hull()),
        "surface.triangles" => {
            let t: Vec<[f64; 9]> = s.triangles().collect();
            ok(&t)
        }
        _ => None,
    }
}

pub fn alignment(reg: &mut Registry, c: &Call) -> R {
    match c.op {
        "alignment.normAng" => return ok(&alignment::norm_ang(c.num(0))),
        "alignment.build" => {
            let v = c.arg(0).cloned().unwrap_or(Value::Null);
            let pis: Vec<alignment::Pi> = arr(v.get("pis")).iter().map(alignment::read_pi).collect();
            let start = v.get("start").map_or(0.0, |s| numf(Some(s)));
            let a = alignment::build_alignment(&pis, start);
            let j = a.j();
            keep(reg, c, a);
            return Some(Ok(j));
        }
        _ => {}
    }
    let a: &Alignment = match obj(reg, c.this, "alignment") {
        Ok(a) => a,
        Err(e) => return Some(Err(e)),
    };
    match c.op {
        "al.pointAt" => ok(&a.point_at(c.num(0))),
        "al.stationOffset" => ok(&a.station_offset(c.num(0), c.num(1))),
        "al.polyline" => {
            let ms = c.arg(0).map_or(f64::INFINITY, |v| numf(Some(v)));
            let mt = c.arg(1).map_or(std::f64::consts::PI / 360.0, |v| numf(Some(v)));
            let p: Vec<Value> =
                a.polyline(ms, mt).into_iter().map(|(x, y, sta)| json!({ "x": x.j(), "y": y.j(), "sta": sta.j() })).collect();
            Some(Ok(Value::Array(p)))
        }
        "al.ticks" => {
            let p: Vec<Value> = a
                .ticks(c.num(0))
                .into_iter()
                .map(|(sta, p)| json!({ "sta": sta.j(), "x": p.x.j(), "y": p.y.j(), "dir": p.dir.j() }))
                .collect();
            Some(Ok(Value::Array(p)))
        }
        "al.keyStations" => {
            let p: Vec<Value> = a.key_stations().into_iter().map(|(sta, kind)| json!({ "sta": sta.j(), "kind": kind })).collect();
            Some(Ok(Value::Array(p)))
        }
        _ => None,
    }
}

pub fn profile(reg: &mut Registry, c: &Call) -> R {
    match c.op {
        "profile.sampleSurface" => {
            let s: Option<&Surface> = c.arg(0).and_then(Value::as_str).and_then(|n| reg.get::<Surface>(n));
            let a: &Alignment = match obj(reg, c.arg(1).and_then(Value::as_str), "alignment") {
                Ok(a) => a,
                Err(e) => return Some(Err(e)),
            };
            let step = c.arg(2).and_then(|o| o.get("step")).map_or(f64::NAN, |v| numf(Some(v)));
            return ok(&profile::sample_surface(s, a, step));
        }
        "profile.build" => {
            let pv: Vec<profile::Pvi> = arr(c.arg(0)).iter().map(profile::read_pvi).collect();
            let p = profile::build_profile(&pv);
            let j = p.j();
            keep(reg, c, p);
            return Some(Ok(j));
        }
        _ => {}
    }
    let p: &Profile = match obj(reg, c.this, "profile") {
        Ok(p) => p,
        Err(e) => return Some(Err(e)),
    };
    match c.op {
        "prof.elevationAt" => ok(&p.elevation_at(c.num(0))),
        "prof.gradeAt" => ok(&p.grade_at(c.num(0))),
        "prof.sample" => ok(&p.sample(c.arg(0).map_or(f64::NAN, |v| numf(Some(v))))),
        _ => None,
    }
}

/// `read_p2` re-exported for other op modules.
pub fn p2_of(v: &Value) -> crate::geo::P2 {
    read_p2(v)
}
