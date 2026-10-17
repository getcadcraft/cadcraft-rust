//! Script ops of crs.rs / crs_pick.rs and of cc-proj itself; twins of forks/tools/cvdiff/ops-crs.mjs. Systems travel as
//! the JS shapes; results are the JS projections (catalog entries by code, points as arrays, `undefined` → null).

use crate::{crs, crs_pick};
use cc_civil::js::{J, arr, numf};
use cc_civil::script::{Call, Registry};
use cc_proj::{JsNum, Out, Proj, Pt};
use serde_json::{Value, json};

type R = Option<Result<Value, String>>;

fn js2(v: [JsNum; 2]) -> Value {
    Value::Array(v.iter().map(|x| x.map_or(Value::Null, |f| f.j())).collect())
}
fn xy(v: &Value) -> (f64, f64) {
    let a = arr(Some(v));
    (numf(a.first()), numf(a.get(1)))
}
fn opt_num(v: Option<&Value>) -> Option<f64> {
    v.filter(|x| !x.is_null()).map(|x| numf(Some(x)))
}
fn each_pt(v: Option<&Value>, mut f: impl FnMut(f64, f64) -> Result<Value, String>) -> Result<Value, String> {
    let mut out = Vec::new();
    for p in arr(v) {
        let (x, y) = xy(p);
        out.push(f(x, y)?);
    }
    Ok(Value::Array(out))
}
fn codes(l: &[&crs::Entry]) -> Value {
    Value::Array(l.iter().map(|e| e.code.j()).collect())
}
fn entry_of(v: Option<&Value>) -> Result<&'static crs::Entry, String> {
    crs::by_code_val(v)?.ok_or_else(|| "Cannot read properties of null (reading 'code')".to_string())
}
fn frame_of(v: Option<&Value>) -> Result<crs::Frame, String> {
    crs::frame(v.unwrap_or(&Value::Null))?.ok_or_else(|| "no frame".to_string())
}
fn out_pt(o: Out) -> Value {
    match o {
        Out::P(p) => {
            let mut m = serde_json::Map::new();
            m.insert("x".into(), p.x.j());
            m.insert("y".into(), p.y.j());
            if let Some(z) = p.z {
                m.insert("z".into(), z.j());
            }
            Value::Object(m)
        }
        Out::Null | Out::Undef => Value::Null,
        Out::Num(n) => n.j(),
    }
}
fn local_of(v: Option<&Value>) -> crs::Local {
    crs::read_local(v).unwrap_or(crs::Local { scale: f64::NAN, rotation: f64::NAN, from: [f64::NAN; 2], to: [f64::NAN; 2] })
}

pub fn handle(_reg: &mut Registry, c: &Call) -> R {
    let a0 = || c.raw(0).cloned().unwrap_or(Value::Null);
    let r: Result<Value, String> = match c.op {
        "crs.setCatalog" => Ok(crs::set_catalog().len().j()),
        "crs.setWktCatalog" => {
            crs::set_wkt_catalog();
            Ok(Value::Bool(true))
        }
        "crs.catalogLoaded" => Ok(Value::Bool(crs::catalog_loaded())),
        "crs.byCode" => crs::by_code_val(c.raw(0)).map(|e| e.map_or(Value::Null, crs::Entry::value)),
        "crs.search" => {
            let text = c.raw(0).filter(|v| cc_civil::js::truthy(Some(v))).map(cc_civil::js::to_js_string).unwrap_or_default();
            let near = c.arg(1).map(|v| {
                let (x, y) = xy(v);
                [x, y]
            });
            let limit = opt_num(c.raw(2)).unwrap_or(50.0);
            crs::search(&text, near, limit).map(|l| codes(&l))
        }
        "crs.suggest" => {
            let (lon, lat) = xy(&a0());
            let unit = c.arg(1).and_then(Value::as_str);
            let limit = opt_num(c.raw(2)).unwrap_or(12.0);
            crs::suggest(lon, lat, unit, limit).map(|l| codes(&l))
        }
        "crs.crsFromEntry" => entry_of(c.raw(0)).map(crs::crs_from_entry),
        "crs.crsFromPrj" => {
            let w = c.raw(0).filter(|v| cc_civil::js::truthy(Some(v))).map(cc_civil::js::to_js_string).unwrap_or_default();
            crs::crs_from_prj(&w).map(|p| json!({ "crs": p.crs, "matched": p.matched.map(|e| e.code.j()), "notes": p.notes }))
        }
        "crs.crsProblem" => {
            let units = c.arg(1).map(cc_civil::js::to_js_string).unwrap_or_else(|| "ft".into());
            Ok(crs::crs_problem(&a0(), &units).map_or(Value::Null, Value::String))
        }
        "crs.defFor" => Ok(crs::def_for(&a0()).map_or(Value::Null, Value::String)),
        "crs.gridStatus" => Ok(crs::grid_status(&a0(), opt_num(c.raw(1)), opt_num(c.raw(2)))),
        "crs.suggestGrids" => crs::suggest_grids(&a0()).map(Value::Array),
        "crs.gridSuggest" => Ok(Value::Array(crs::grid_suggest())),
        "crs.loadGrid" => {
            let name = c.raw(0).filter(|v| cc_civil::js::truthy(Some(v))).map(cc_civil::js::to_js_string).unwrap_or_default();
            match b64(c.str(1)) {
                Ok(bytes) => crs::load_grid(&name, &bytes).map(|g| g.j()),
                Err(e) => Err(e),
            }
        }
        "crs.gridInfo" => Ok(crs::grid_info(c.str(0)).map_or(Value::Null, |g| g.j())),
        "crs.gridsLoaded" => Ok(crs::grids_loaded().j()),
        "crs.transform" => {
            let from = a0();
            let to = c.raw(1).cloned().unwrap_or(Value::Null);
            each_pt(c.raw(2), |x, y| crs::transform(&from, &to, x, y).map(js2))
        }
        "crs.localFromPairs" => crs::local_from_pairs(&a0()).map(|l| l.j()),
        "crs.groundLocal" => {
            let (x, y) = xy(&c.raw(1).cloned().unwrap_or(Value::Null));
            Ok(crs::ground_local(c.num(0), [x, y]).j())
        }
        "crs.prjFor" => Ok(crs::prj_for(&a0()).map_or(Value::Null, Value::String)),
        "crs.metresPer" => Ok(json!({ "m": 1, "ft": 0.3048, "ftUS": (1200.0f64 / 3937.0).j() })),

        "frame.toGrid" => frame_of(c.raw(0)).and_then(|f| each_pt(c.raw(1), |x, y| Ok(f.to_grid(x, y).j()))),
        "frame.fromGrid" => frame_of(c.raw(0)).and_then(|f| each_pt(c.raw(1), |x, y| Ok(f.from_grid(x, y).j()))),
        "frame.toLonLat" => frame_of(c.raw(0)).and_then(|f| each_pt(c.raw(1), |x, y| f.to_lon_lat(x, y).map(js2))),
        "frame.fromLonLat" => frame_of(c.raw(0)).and_then(|f| each_pt(c.raw(1), |x, y| f.from_lon_lat(x, y).map(|p| p.j()))),
        "frame.to" => {
            let other = c.raw(1).cloned().unwrap_or(Value::Null);
            frame_of(c.raw(0)).and_then(|f| each_pt(c.raw(2), |x, y| f.to(&other, x, y).map(js2)))
        }
        "frame.from" => {
            let other = c.raw(1).cloned().unwrap_or(Value::Null);
            frame_of(c.raw(0)).and_then(|f| each_pt(c.raw(2), |x, y| f.from(&other, x, y).map(|p| p.j())))
        }
        "frame.factors" => frame_of(c.raw(0)).and_then(|f| f.factors(c.num(1), c.num(2), c.raw(3).filter(|v| !v.is_null())).map(|r| r.j())),

        "pick.familyOf" => Ok(crs_pick::family_of(c.str(0)).iter().map(|s| s.to_string()).collect::<Vec<_>>().j()),
        "pick.systemsThatFit" => {
            let p = a0();
            let x = p.get("x").and_then(Value::as_f64);
            let y = p.get("y").and_then(Value::as_f64);
            let limit = opt_num(c.raw(2)).unwrap_or(60.0);
            crs_pick::systems_that_fit(x, y, c.str(1), limit).map(|(l, total)| {
                let list: Vec<Value> = l.iter().map(|f| json!({ "code": f.entry.code.j(), "lon": f.lon.j(), "lat": f.lat.j() })).collect();
                json!({ "list": list, "total": total })
            })
        }
        "pick.unitWords" => Ok(Value::String(crs_pick::unit_words(c.raw(0).filter(|v| !v.is_null())))),
        "pick.areaWords" => Ok(Value::String(crs_pick::area_words(c.raw(0)))),
        "pick.shiftWords" => Ok(Value::String(crs_pick::shift_words(&a0()))),
        "pick.describe" => Ok(crs_pick::describe(&a0())),
        "pick.latLonText" => {
            let mode = c.arg(2).and_then(Value::as_str).unwrap_or("dms");
            Ok(Value::String(crs_pick::lat_lon_text(c.num(0), c.num(1), mode)))
        }
        "pick.parseLatLon" => {
            let t = c.raw(0).filter(|v| cc_civil::js::truthy(Some(v))).map(cc_civil::js::to_js_string).unwrap_or_default();
            Ok(crs_pick::parse_lat_lon(&t).map_or(Value::Null, |p| p.j()))
        }
        "pick.gridOf" => crs_pick::grid_of(&a0(), c.num(1), c.num(2)).map(js2),
        "pick.misclosure" => {
            let (px, py) = xy(&c.raw(1).cloned().unwrap_or(Value::Null));
            let (gx, gy) = xy(&c.raw(2).cloned().unwrap_or(Value::Null));
            Ok(crs_pick::misclosure(&local_of(c.raw(0)), [px, py], [gx, gy]))
        }
        "pick.localToGrid" => Ok(crs_pick::local_to_grid(&local_of(c.raw(0)), c.num(1), c.num(2)).j()),
        "pick.groundFactor" => crs_pick::ground_factor(&a0(), c.num(1), c.num(2), c.raw(3).filter(|v| !v.is_null())),
        "pick.reprojectProject" => {
            let mut state = a0();
            let points: Vec<Value> = arr(c.raw(1)).to_vec();
            let spec = c.raw(2).cloned().unwrap_or(Value::Null);
            let res = if let Some(l) = spec.get("local") {
                let l = local_of(Some(l));
                crs_pick::reproject_project(&mut state, &points, &mut |x, y| Ok(crs_pick::local_to_grid(&l, x, y)))
            } else {
                let from = spec.get("from").cloned().unwrap_or(Value::Null);
                let to = spec.get("to").cloned().unwrap_or(Value::Null);
                crs_pick::reproject_project(&mut state, &points, &mut |x, y| crs::transform(&from, &to, x, y).map(crs::nn))
            };
            res.map(|r| json!({ "result": r, "state": state }))
        }

        "proj.def" => Proj::new(c.str(0)).map(|p| proj_view(&p)),
        "proj.forward" => Proj::new(c.str(0)).and_then(|p| each_pt(c.raw(1), |x, y| p.forward(Pt::xy(x, y)).map(out_pt))),
        "proj.inverse" => Proj::new(c.str(0)).and_then(|p| each_pt(c.raw(1), |x, y| p.inverse(Pt::xy(x, y)).map(out_pt))),
        "proj.transform" => cc_proj::Converter::new(c.str(0), c.str(1)).and_then(|t| each_pt(c.raw(2), |x, y| t.forward(x, y).map(js2))),
        "proj.inverseTransform" => {
            cc_proj::Converter::new(c.str(0), c.str(1)).and_then(|t| each_pt(c.raw(2), |x, y| t.inverse(x, y).map(js2)))
        }
        _ => return None,
    };
    Some(r)
}

/// A proj4 value as JSON (undefined → null).
fn vj(v: &cc_proj::val::Val) -> Value {
    use cc_proj::val::Val;
    match v {
        Val::Undef | Val::Null => Value::Null,
        Val::Bool(b) => Value::Bool(*b),
        Val::Num(n) => n.j(),
        Val::Str(s) => Value::String(s.clone()),
        Val::Arr(a) => Value::Array(a.iter().map(vj).collect()),
        Val::Obj(o) => Value::Object(o.keys().iter().map(|k| (k.clone(), vj(&o.at(k)))).collect()),
    }
}

/// The fields of a projection the harness compares.
fn proj_view(p: &Proj) -> Value {
    let o = |v: Option<f64>| v.map_or(Value::Null, |x| x.j());
    let mut v = json!({
        "projName": vj(&p.proj_name), "units": vj(&p.units), "datumCode": vj(&p.datum_code),
        "a": p.a.j(), "b": p.b.j(), "rf": p.rf.j(), "es": p.es.j(), "e": p.e.j(), "ep2": p.ep2.j(),
        "k0": o(p.k0), "x0": o(p.x0), "y0": o(p.y0), "lat0": o(p.lat0), "lat1": o(p.lat1), "lat2": o(p.lat2), "long0": o(p.long0),
        "zone": o(p.zone), "to_meter": o(p.to_meter), "from_greenwich": o(p.from_greenwich),
        "datum_type": p.datum.datum_type, "datum_params": p.datum.datum_params.j(),
    });
    // `rf` is undefined in the JS when the definition never set it
    if p.rf.is_nan()
        && let Some(o) = v.as_object_mut()
    {
        o.remove("rf");
    }
    v
}

/// Base64 (standard alphabet, padding optional) → bytes.
pub fn b64(s: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(s.len().saturating_mul(3) / 4);
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    for c in s.bytes() {
        let v: u32 = match c {
            b'A'..=b'Z' => u32::from(c.wrapping_sub(b'A')),
            b'a'..=b'z' => u32::from(c.wrapping_sub(b'a')).wrapping_add(26),
            b'0'..=b'9' => u32::from(c.wrapping_sub(b'0')).wrapping_add(52),
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return Err("bad base64".into()),
        };
        acc = (acc << 6 | v) & 0x00ff_ffff;
        bits = bits.wrapping_add(6);
        if bits >= 8 {
            bits = bits.wrapping_sub(8);
            out.push(u8::try_from((acc >> bits) & 0xff).unwrap_or(0));
        }
    }
    Ok(out)
}
