//! Script ops of the file modules (see `cc_detail::script`): `ifc.*`, `nc.*`, `dwg.*`, `dxf.*`. Twins of
//! `forks/tools/dcdiff/ops-ifc.mjs`.

use crate::{dwgout, ifc_import, ifc_out, nc1};
use cc_detail::build::Built;
use cc_detail::js::{arr, jnum, num, string_of, truthy};
use cc_detail::marks::{Marks, mark_footings};
use cc_detail::rebar::{Footing, bending_schedule};
use cc_detail::script::{Call, Registry, built_arg, get, state_arg};
use cc_detail::zip::{DosDate, zip};
use serde_json::{Value, json};

type R = Option<Result<Value, String>>;

fn opt<'a>(o: Option<&'a Value>, k: &str) -> Option<&'a Value> {
    o.and_then(|o| o.get(k))
}

/// `writeIfc` with the inputs the harness names: a state, a kept build and marks, and options
/// `{ footingMark: bool, perPhase: bool, schedule: bool, name, now (ms) }`.
fn ifc_text(reg: &Registry, c: &Call) -> Result<String, String> {
    let st = state_arg(reg, c.raw(0))?;
    let b: &Built = built_arg(reg, c.raw(1))?;
    let m = get::<Marks>(reg, c.raw(2).and_then(Value::as_str))?;
    let o = c.arg(3);
    let footings = arr(st.get("footings")).to_vec();
    let fm = if opt(o, "footingMark").is_some_and(|v| truthy(Some(v))) {
        Some(mark_footings(&footings, opt(o, "perPhase").is_some_and(|v| truthy(Some(v)))))
    } else {
        None
    };
    let typed: Vec<Footing> = footings.iter().map(Footing::from_value).collect();
    let sch = if opt(o, "schedule").is_none_or(|v| truthy(Some(v))) { Some(bending_schedule(&typed)) } else { None };
    let name = opt(o, "name").and_then(Value::as_str);
    let now = opt(o, "now").map_or(0.0, |v| num(Some(v)));
    ifc_out::write_ifc(&st, b, m, fm.as_deref(), sch.as_ref(), name, now)
}

fn part<'a>(reg: &'a Registry, c: &Call) -> Result<&'a cc_detail::build::Part, String> {
    let b: &Built = built_arg(reg, c.raw(0))?;
    let id = c.str(1);
    b.get(id).ok_or_else(|| format!("no part {id}"))
}
fn info_of(v: Option<&Value>, phase_default: &str) -> nc1::NcInfo {
    let s = |k: &str| match opt(v, k) {
        None | Some(Value::Null) => String::new(),
        Some(x) => string_of(Some(x)),
    };
    let count = match opt(v, "count") {
        Some(x) if truthy(Some(x)) => string_of(Some(x)),
        _ => "1".into(),
    };
    let phase = match opt(v, "phase") {
        Some(x) if truthy(Some(x)) => string_of(Some(x)),
        _ => phase_default.into(),
    };
    nc1::NcInfo { order: s("order"), mark: s("mark"), count, phase }
}
fn date_of(v: Option<&Value>) -> DosDate {
    let d = arr(v);
    let g = |i: usize| d.get(i).and_then(Value::as_u64).and_then(|x| u32::try_from(x).ok()).unwrap_or(0);
    DosDate { year: g(0), month: g(1), day: g(2), hours: g(3), minutes: g(4), seconds: g(5) }
}

pub fn handle(reg: &mut Registry, c: &Call) -> R {
    let (module, _) = c.op.split_once('.').unwrap_or((c.op, ""));
    if !matches!(module, "ifc" | "nc" | "dwg" | "dxf") {
        return None;
    }
    let r = (|| -> Result<Value, String> {
        Ok(match c.op {
            "ifc.guid" => Value::from(ifc_out::guid_for(c.str(0))),
            "ifc.R" => Value::from(ifc_out::r(c.num(0))),
            "ifc.S" => Value::from(ifc_out::s(&string_of(c.raw(0)))),
            "ifc.write" => Value::from(ifc_text(reg, c)?),
            "ifc.importText" => {
                let r = ifc_import::import_ifc(c.str(0).as_bytes(), c.arg(1).and_then(Value::as_str).unwrap_or(""))?;
                let lines = ifc_import::report_lines(r.get("report").unwrap_or(&Value::Null));
                json!({ "state": r.get("state"), "report": r.get("report"), "lines": lines })
            }
            "ifc.importFile" => {
                let path = c.str(0);
                let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
                if meta.len() > 1 << 30 {
                    return Err("file too large".into());
                }
                let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
                let r = ifc_import::import_ifc(&bytes, c.arg(1).and_then(Value::as_str).unwrap_or(""))?;
                let lines = ifc_import::report_lines(r.get("report").unwrap_or(&Value::Null));
                json!({ "state": r.get("state"), "report": r.get("report"), "lines": lines })
            }
            "ifc.importOwn" => {
                let text = ifc_text(reg, c)?;
                let r = ifc_import::import_ifc(text.as_bytes(), c.arg(4).and_then(Value::as_str).unwrap_or(""))?;
                let lines = ifc_import::report_lines(r.get("report").unwrap_or(&Value::Null));
                json!({ "state": r.get("state"), "report": r.get("report"), "lines": lines })
            }
            "nc.refusal" => nc1::nc_refusal(part(reg, c)?).map_or(Value::Null, Value::from),
            "nc.write" => {
                let p = part(reg, c)?;
                Value::from(nc1::write_nc1(p, &info_of(c.arg(2), "1")))
            }
            "nc.read" => nc1::read_nc1(c.str(0))?,
            "nc.set" => {
                let b: &Built = built_arg(reg, c.raw(0))?;
                let m = get::<Marks>(reg, c.raw(1).and_then(Value::as_str))?;
                let order = string_of(c.raw(2));
                let mut files: Vec<(String, Vec<u8>)> = Vec::new();
                let mut refused = Vec::new();
                for row in &m.parts {
                    let Some(p) = row.ids.first().and_then(|id| b.get(id)) else {
                        return Err("Cannot read properties of undefined (reading 'profile')".into());
                    };
                    if let Some(why) = nc1::nc_refusal(p) {
                        refused.push(json!({ "mark": row.mark, "why": why }));
                        continue;
                    }
                    let ph = cc_detail::js::num_to_string(cc_detail::js::or(p.phase, 1.0));
                    let info = nc1::NcInfo {
                        order: order.clone(),
                        mark: row.mark.clone(),
                        count: if row.count > 0 { row.count.to_string() } else { "1".into() },
                        phase: ph,
                    };
                    files.push((format!("{}.nc1", row.mark), nc1::write_nc1(p, &info).into_bytes()));
                }
                let z = zip(&files, date_of(c.arg(3)));
                json!({
                    "files": files.iter().map(|(n, d)| json!({ "name": n, "data": String::from_utf8_lossy(d) })).collect::<Vec<_>>(),
                    "refused": refused,
                    "zip": { "$base64": cc_civil::project::b64(&z) },
                })
            }
            "dwg.pen" => dwgout::pen_of(c.raw(0).unwrap_or(&Value::Null)).map_or(Value::Null, |p| dwgout::pen_json(&p)),
            "dwg.layers" => Value::Array(dwgout::layers_of(c.raw(0).unwrap_or(&Value::Null))?.iter().map(dwgout::pen_json).collect()),
            "dwg.model" => dwgout::sheet_model(c.raw(0).unwrap_or(&Value::Null), &title(c.arg(1)))?,
            "dwg.write" => {
                let model = dwgout::sheet_model(c.raw(0).unwrap_or(&Value::Null), &title(c.arg(1)))?;
                let v = cc_io_dwg::w_js::from_json(&model, 0);
                let cc_io_dwg::V::Obj(obj) = v else { return Err("the model is not an object".into()) };
                let opts = cc_io_dwg::WriteOpts {
                    version: c.arg(2).and_then(Value::as_str).unwrap_or("AC1015").to_string(),
                    created_ms: c.arg(3).and_then(Value::as_f64),
                    guid: cc_io_dwg::GuidMode::Sequence,
                };
                let (bytes, report) = cc_io_dwg::write_dwg_opts(&obj, &opts)?;
                let rep: Value = serde_json::from_str(&cc_io_dwg::V::Obj(report).to_json()).unwrap_or(Value::Null);
                json!({ "bytes": { "$base64": cc_civil::project::b64(&bytes) }, "notes": rep.get("notes") })
            }
            "dxf.write" => dwgout::sheet_dxf(c.raw(0).unwrap_or(&Value::Null), &title(c.arg(1)))?,
            _ => return Err("\u{0}unknown".into()),
        })
    })();
    match r {
        Err(e) if e == "\u{0}unknown" => None,
        r => Some(r),
    }
}

fn title(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(x) if truthy(Some(x)) => string_of(Some(x)),
        _ => String::new(),
    }
}

/// `jnum` re-exported for tests.
pub fn n(x: f64) -> Value {
    jnum(x)
}
