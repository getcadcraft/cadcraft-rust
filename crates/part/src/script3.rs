//! Stage-3 ops for the call-script runner (mirrors `forks/tools/pcdiff/ops-stage3.mjs`): planar faces of a mesh,
//! named faces and edges of the mesh kernel, reference geometry.

use crate::faces::{self, Mesh};
use crate::js;
use crate::jv;
use crate::meshfaces::{self as mf, Surf};
use crate::refgeom::{self, EdgeRes, FaceRes, PlaneRes, Resolve};
use crate::script::num_arg;
use crate::vec::{Frame, M4, V3};
use serde_json::Value;

fn arg(a: &[Value], i: usize) -> &Value {
    a.get(i).unwrap_or(&Value::Null)
}
fn nums(v: Option<&Value>) -> Vec<f64> {
    jv::arr(v).iter().map(|x| num_arg(Some(x))).collect()
}
fn mesh_arg(v: &Value) -> Mesh {
    let indices = jv::arr(v.get("indices")).iter().map(|x| x.as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(u32::MAX)).collect();
    let tri_face = v
        .get("triFace")
        .filter(|x| x.is_array())
        .map(|t| jv::arr(Some(t)).iter().map(|x| x.as_i64().and_then(|n| i32::try_from(n).ok()).unwrap_or(-1)).collect());
    // the kernel's positions are a Float32Array: the harness's numbers are stored the same way
    Mesh { positions: nums(v.get("positions")).into_iter().map(fround).collect(), indices, tri_face }
}
#[allow(clippy::cast_possible_truncation)]
fn fround(x: f64) -> f64 {
    f64::from(x as f32)
}
fn p3(v: Option<&Value>) -> V3 {
    let a = jv::arr(v);
    [num_arg(a.first()), num_arg(a.get(1)), num_arg(a.get(2))]
}
fn group_json(g: &faces::Group) -> Value {
    let mut o =
        jv::obj(vec![("tris", Value::Array(g.tris.iter().map(|&t| jv::unum(t)).collect())), ("n", jv::pt3(g.n)), ("d", jv::num(g.d))]);
    if let Some(f) = g.face {
        jv::set(&mut o, "face", Value::from(f));
    }
    o
}
fn groups_json(face_of: &[usize], groups: &[faces::Group]) -> Value {
    jv::obj(vec![
        ("faceOf", Value::Array(face_of.iter().map(|&x| jv::unum(x)).collect())),
        ("groups", Value::Array(groups.iter().map(group_json).collect())),
    ])
}

/// Resolvers from tables keyed by `JSON.stringify(ref)` (the harness's stand-in for a kernel).
struct Tables<'a>(&'a Value);
impl Tables<'_> {
    fn look(&self, tab: &str, r: &Value) -> Result<&Value, String> {
        let k = jv::stringify(r);
        self.0.get(tab).and_then(|t| t.get(&k)).ok_or_else(|| format!("unresolved {k}"))
    }
}
impl Resolve for Tables<'_> {
    fn plane(&self, r: &Value) -> Result<PlaneRes, String> {
        let v = self.look("planes", r)?;
        Ok(PlaneRes { frame: Frame::from_json(v), center: v.get("center").filter(|c| jv::truthy(Some(c))).map(|c| p3(Some(c))) })
    }
    fn dir(&self, r: &Value) -> Result<(V3, V3), String> {
        let v = self.look("dirs", r)?;
        Ok((p3(v.get("o")), p3(v.get("d"))))
    }
    fn point(&self, r: &Value) -> Result<V3, String> {
        let v = self.look("points", r)?;
        Ok(p3(Some(v)))
    }
    fn face(&self, r: &Value) -> Result<FaceRes, String> {
        let v = self.look("faces", r)?;
        let axis = v.get("axis").filter(|x| jv::truthy(Some(x))).map(|a| (p3(a.get("o")), p3(a.get("d"))));
        Ok(FaceRes {
            ty: js::str_of(v.get("type")),
            n: p3(v.get("n")),
            o: p3(v.get("o")),
            axis,
            r: jv::as_num(v.get("r")).unwrap_or(f64::NAN),
        })
    }
    fn edge(&self, r: &Value) -> Result<EdgeRes, String> {
        let v = self.look("edges", r)?;
        let axis = v.get("axis").filter(|x| jv::truthy(Some(x))).map(|a| (p3(a.get("o")), p3(a.get("d"))));
        Ok(EdgeRes { ty: js::str_of(v.get("type")), a: p3(v.get("a")), b: p3(v.get("b")), axis })
    }
}

/// The stage-3 ops.
pub fn handle(op: &str, a: &[Value]) -> Option<Result<Value, String>> {
    let r = match op {
        "faces.planarGroupsMesh" => {
            let m = mesh_arg(arg(a, 0));
            let tol = match a.get(1) {
                None | Some(Value::Null) => 1e-5,
                v => num_arg(v),
            };
            let (fo, g) = faces::planar_groups_mesh(&m, tol);
            Ok(groups_json(&fo, &g))
        }
        "faces.faceGroups" => {
            let m = mesh_arg(arg(a, 0));
            let (fo, g) = faces::face_groups(&m);
            Ok(groups_json(&fo, &g))
        }
        "faces.loops" => {
            let m = mesh_arg(arg(a, 0));
            let (_, groups) = faces::planar_groups(&m, 1e-5);
            Ok(Value::Array(
                groups
                    .iter()
                    .map(|g| {
                        let raw = faces::boundary_loops(&m, &g.tris);
                        let (frame, loops, area) = faces::face_loops_2d(&m, g);
                        jv::obj(vec![
                            ("raw", Value::Array(raw.iter().map(|l| Value::Array(l.iter().map(|&x| jv::unum(x)).collect())).collect())),
                            ("frame", frame.to_json()),
                            ("loops", Value::Array(loops.iter().map(|l| Value::Array(l.iter().map(|p| jv::pt2(*p)).collect())).collect())),
                            ("area", jv::num(area)),
                        ])
                    })
                    .collect(),
            ))
        }
        "faces.dropCollinear" => {
            let lp: Vec<[f64; 2]> = jv::arr(a.first())
                .iter()
                .map(|p| {
                    let q = jv::arr(Some(p));
                    [num_arg(q.first()), num_arg(q.get(1))]
                })
                .collect();
            let tol = match a.get(1) {
                None | Some(Value::Null) => 1e-6,
                v => num_arg(v),
            };
            Ok(Value::Array(faces::drop_collinear(&lp, tol).iter().map(|p| jv::pt2(*p)).collect()))
        }
        "mf.namedFaces" => {
            let m = mesh_arg(arg(a, 0));
            let tt: Vec<i64> = jv::arr(a.get(1)).iter().map(|x| x.as_i64().unwrap_or(-1)).collect();
            let surfs: Vec<Vec<Surf>> = jv::arr(a.get(2)).iter().map(|l| jv::arr(Some(l)).iter().map(Surf::from_json).collect()).collect();
            let seg = match a.get(3) {
                None | Some(Value::Null) => 64.0,
                v => num_arg(v),
            };
            Ok(mf::named_faces(&m, &tt, &surfs, seg).to_json())
        }
        "mf.moveSurfs" => {
            let mv = nums(a.get(1));
            let mut m4: M4 = [f64::NAN; 16];
            for (i, x) in m4.iter_mut().enumerate() {
                *x = mv.get(i).copied().unwrap_or(f64::NAN);
            }
            Ok(mf::move_surfs(a.first(), &m4, &js::str_of(a.get(2))))
        }
        "ref.feature" => refgeom::ref_feature(arg(a, 0), &Tables(arg(a, 1))).map(|x| x.to_json()),
        "ref.planeQuad" => {
            let g = arg(a, 0);
            let f = Frame::from_json(g.get("frame").unwrap_or(&Value::Null));
            Ok(Value::Array(refgeom::plane_quad(&f, p3(g.get("center")), num_arg(a.get(1))).iter().map(|p| jv::pt3(*p)).collect()))
        }
        _ => return None,
    };
    Some(r)
}
