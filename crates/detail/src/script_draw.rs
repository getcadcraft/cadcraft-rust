//! Script ops for the drawing modules (issues, paper, pdf, shop, drawings, report, kiss); see [`crate::script`].
//! Twins of `forks/tools/dcdiff/ops-draw.mjs`.

use crate::build::Built;
use crate::display::{List, list_of};
use crate::drawings::{self, Set};
use crate::js::{arr, jnum, num};
use crate::marks::{Marks, mark_footings};
use crate::report::{self, Report};
use crate::script::{Call, Registry, built_arg, get, keep, state_arg};
use serde_json::{Value, json};

type R = Option<Result<Value, String>>;

/// A kept drawing set and the state it was made from.
pub struct KeptSet {
    pub set: Set,
    pub lists: Vec<List>,
}
/// A kept shop drawing.
pub struct KeptShop(pub List);
/// A kept report and its pages.
pub struct KeptReport(pub Report);

fn utc(v: Option<&Value>) -> crate::pdf::Utc {
    let a = arr(v);
    let g = |i: usize, d: f64| a.get(i).and_then(Value::as_f64).unwrap_or(d);
    let u = |i: usize, d: f64| u32::try_from(crate::display::sat_i64(g(i, d).max(0.0))).unwrap_or(0);
    crate::pdf::Utc {
        year: crate::display::sat_i64(g(0, 2026.0)),
        month: u(1, 1.0),
        day: u(2, 1.0),
        hour: u(3, 0.0),
        minute: u(4, 0.0),
        second: u(5, 0.0),
    }
}
fn marks_arg<'a>(reg: &'a Registry, v: Option<&Value>) -> Result<&'a Marks, String> {
    get::<Marks>(reg, v.and_then(Value::as_str))
}
fn str_opt(o: Option<&Value>, k: &str) -> String {
    o.and_then(|o| o.get(k)).map_or_else(String::new, |v| crate::js::string_or_empty(Some(v)))
}
fn b64(bytes: &[u8]) -> Value {
    json!({ "$base64": cc_civil::project::b64(bytes) })
}

pub fn handle(reg: &mut Registry, c: &Call) -> R {
    let (module, _) = c.op.split_once('.').unwrap_or((c.op, ""));
    if !matches!(module, "paper" | "issues" | "pdf" | "shop" | "drawings" | "report" | "kiss") {
        return None;
    }
    let r = (|| -> Result<Value, String> {
        Ok(match c.op {
            "paper.of" => {
                let p = crate::paper::paper_of(c.str(0));
                {
                    let mut o = json!({ "key": p.key, "w": jnum(p.w), "h": jnum(p.h), "family": p.family, "k": jnum(p.k) });
                    if let (Some(i), Some(m)) = (p.inches, o.as_object_mut()) {
                        m.insert("inches".into(), json!(i));
                    }
                    o
                }
            }
            "paper.label" => Value::from(crate::paper::paper_label(c.str(0))),
            "issues.nextRev" => Value::from(crate::issues::next_rev(&crate::js::string_or_empty(c.raw(0)))),
            "issues.sheetStatus" | "issues.planIssue" => {
                let k = get::<KeptSet>(reg, c.raw(0).and_then(Value::as_str))?;
                let refs: Vec<crate::issues::SheetRef> = k
                    .set
                    .sheets
                    .iter()
                    .map(|sh| crate::issues::SheetRef { id: &sh.id, number: &sh.number, hash: &sh.hash, prints: Some(&sh.prints) })
                    .collect();
                let issues = match c.raw(1) {
                    Some(Value::String(_)) => state_arg(reg, c.raw(1))?.get("issues").cloned().unwrap_or(json!([])),
                    Some(v) => v.clone(),
                    None => json!([]),
                };
                let is = arr(Some(&issues));
                if c.op == "issues.sheetStatus" {
                    let mut o = serde_json::Map::new();
                    for (id, st) in crate::issues::sheet_status(&refs, is) {
                        o.insert(id, st.j());
                    }
                    Value::Object(o)
                } else {
                    let ids: Vec<String> = arr(c.arg(2)).iter().map(|v| crate::js::string_of(Some(v))).collect();
                    let o = c.arg(3);
                    let date = o.and_then(|o| o.get("date")).cloned().unwrap_or(Value::Null);
                    let note = str_opt(o, "note");
                    let force = o.and_then(|o| o.get("force")).is_some_and(|v| crate::js::truthy(Some(v)));
                    Value::Array(crate::issues::plan_issue(&refs, is, &ids, &date, &note, force))
                }
            }
            "issues.cloudBase" => {
                let log: Vec<&Value> = arr(c.arg(0)).iter().collect();
                match crate::issues::cloud_base(&log, c.str(1)) {
                    Some((p, rev)) => json!({ "parts": Value::Object(p), "rev": rev }),
                    None => Value::Null,
                }
            }
            "issues.revsByNumber" => {
                let mut o = serde_json::Map::new();
                for (k, v) in crate::issues::revs_by_number(arr(c.arg(0))) {
                    o.insert(k, Value::from(v));
                }
                Value::Object(o)
            }
            "pdf.num" => Value::from(crate::pdf::num(c.num(0))),
            "pdf.winAnsi" => {
                let (st, m) = crate::pdf::win_ansi(&crate::js::string_or_empty(c.raw(0)));
                json!({ "str": st, "missing": m })
            }
            "pdf.textWidth" => jnum(crate::pdf::text_width(
                &crate::js::string_of(c.raw(0)),
                c.num(1),
                c.arg(2).is_some_and(|v| crate::js::truthy(Some(v))),
            )),
            "pdf.contentStream" => {
                let (text, m) = crate::pdf::content_stream(&list_of(c.raw(0).unwrap_or(&Value::Null)));
                json!({ "text": text, "missing": m })
            }
            "pdf.toPdf" => {
                // args: [source, { title, compress, date: [y, m, d, h, mi, s] }]; source: a kept set / shop name, a list, or lists
                let lists: Vec<List> = match c.raw(0) {
                    Some(Value::String(n)) => {
                        if let Some(k) = reg.get::<KeptSet>(n) {
                            k.lists.clone()
                        } else if let Some(k) = reg.get::<KeptShop>(n) {
                            vec![k.0.clone()]
                        } else if let Some(k) = reg.get::<crate::script_draw::KeptPages>(n) {
                            k.0.clone()
                        } else {
                            return Err(format!("no object {n}"));
                        }
                    }
                    Some(Value::Array(a)) => a.iter().map(list_of).collect(),
                    Some(v) => vec![list_of(v)],
                    None => vec![],
                };
                let o = c.arg(1);
                let title =
                    o.and_then(|o| o.get("title")).map_or_else(|| crate::i18n::t0("pdf.title"), |v| crate::js::string_or_empty(Some(v)));
                let compress = o.and_then(|o| o.get("compress")).is_none_or(|v| crate::js::truthy(Some(v)));
                let (bytes, missing) = crate::pdf::to_pdf(&lists, &title, compress, utc(o.and_then(|o| o.get("date"))));
                json!({ "bytes": pdf_view(&bytes, compress), "missing": missing })
            }
            "shop.refusal" => {
                let b = built_arg(reg, c.raw(0))?;
                crate::shop::shop_refusal(b.get(c.str(1))).map_or(Value::Null, Value::from)
            }
            "shop.drawing" => {
                let b = built_arg(reg, c.raw(0))?;
                let o = c.arg(2);
                let info = crate::shop::Info {
                    paper: str_opt(o, "paper"),
                    mark: str_opt(o, "mark"),
                    asm_mark: str_opt(o, "asmMark"),
                    count: o.and_then(|o| o.get("count")).filter(|v| !v.is_null()).map(|v| num(Some(v))),
                    weld: str_opt(o, "weld"),
                    model: str_opt(o, "model"),
                    date: str_opt(o, "date"),
                };
                let r = match b.get(c.str(1)) {
                    Some(p) => crate::shop::shop_drawing(p, &info),
                    None => Err(crate::shop::shop_refusal(None).unwrap_or_default()),
                };
                let j = crate::shop::shop_json(&r);
                if let Ok(sd) = &r {
                    keep(reg, c, KeptShop(List { w: sd.w, h: sd.h, items: sd.items.clone() }));
                }
                j
            }
            "drawings.set" => {
                let st = state_arg(reg, c.raw(0))?;
                let b = built_arg(reg, c.raw(1))?;
                let m = marks_arg(reg, c.raw(2))?;
                let o = c.arg(3);
                let fm: Option<Vec<(String, String)>> = match o.and_then(|o| o.get("footings")) {
                    Some(v) if crate::js::truthy(Some(v)) => {
                        Some(mark_footings(arr(st.get("footings")), v.get("perPhase").is_some_and(|x| crate::js::truthy(Some(x)))))
                    }
                    _ => None,
                };
                let hidden = o.and_then(|o| o.get("hiddenLines")).is_none_or(|v| crate::js::truthy(Some(v)));
                let set = drawings::drawing_set(&st, b, m, fm.as_deref(), &str_opt(o, "date"), &str_opt(o, "paper"), hidden);
                let j = drawings::set_json(b, &set);
                let lists = set.sheets.iter().map(|s| List { w: s.w, h: s.h, items: s.items.clone() }).collect();
                keep(reg, c, KeptSet { set, lists });
                j
            }
            "drawings.runsBehind" => {
                let pts = pts2(c.arg(0));
                let occ: Vec<Vec<[f64; 2]>> = arr(c.arg(2)).iter().map(|o| pts2(Some(o))).collect();
                Value::Array(
                    drawings::runs_behind(&pts, c.arg(1).is_some_and(|v| crate::js::truthy(Some(v))), &occ)
                        .iter()
                        .map(|r| json!({ "pts": pj(&r.pts), "hidden": r.hidden }))
                        .collect(),
                )
            }
            "drawings.hull" => pj(&drawings::hull(&pts2(c.arg(0)))),
            "drawings.uniq" => Value::Array(
                drawings::uniq(arr(c.arg(0)).iter().filter_map(Value::as_f64), c.arg(1).map_or(0.5, |v| num(Some(v))))
                    .into_iter()
                    .map(jnum)
                    .collect(),
            ),
            "drawings.pickScale" => jnum(drawings::pick_scale(c.num(0), c.num(1))),
            "drawings.modelRef" => Value::from(drawings::model_ref(&state_arg(reg, c.raw(0))?)),
            "drawings.holeGeom" => {
                let b = built_arg(reg, c.raw(0))?;
                let p = b.get(c.str(1)).ok_or("no part")?;
                Value::Array(
                    drawings::hole_geoms(p)
                        .iter()
                        .map(|g| {
                            drawings::drop_nan_t(
                                json!({ "c": crate::js::p3v(g.c), "axis": crate::js::p3v(g.axis), "t": jnum(g.t), "d": jnum(g.d) }),
                                g.t,
                            )
                        })
                        .collect(),
                )
            }
            "drawings.asm" => {
                let b = built_arg(reg, c.raw(0))?;
                let m = marks_arg(reg, c.raw(1))?;
                drawings::assembly_geometry(b, m, c.str(2)).map_or(Value::Null, |a| drawings::asm_json(b, &a))
            }
            "drawings.partPrints" => {
                let st = state_arg(reg, c.raw(0))?;
                let b = built_arg(reg, c.raw(1))?;
                let m = marks_arg(reg, c.raw(2))?;
                let fm = if c.arg(3).is_some_and(|v| crate::js::truthy(Some(v))) {
                    Some(mark_footings(arr(st.get("footings")), false))
                } else {
                    None
                };
                let (d, g) = drawings::part_prints(&st, b, m, fm.as_deref());
                let mo = |l: Vec<(String, String)>| Value::Object(l.into_iter().map(|(k, v)| (k, Value::from(v))).collect());
                json!({ "detail": mo(d), "ga": mo(g) })
            }
            "report.phase" => {
                let st = state_arg(reg, c.raw(0))?;
                let b: &Built = built_arg(reg, c.raw(1))?;
                let m = marks_arg(reg, c.raw(2))?;
                let fs: Vec<crate::rebar::Footing> = arr(st.get("footings")).iter().map(crate::rebar::Footing::from_value).collect();
                let sch = crate::rebar::bending_schedule(&fs);
                let rep = report::phase_report(&st, b, m, &sch);
                let j = report::report_json(&rep);
                keep(reg, c, KeptReport(rep));
                j
            }
            "report.tables" => {
                let r = get::<KeptReport>(reg, c.raw(0).and_then(Value::as_str))?;
                let (summary, phases) = report::report_tables(&r.0);
                json!({ "summary": summary, "phases": phases.iter().map(|p| json!({ "title": p.title, "parts": p.parts, "bolts": p.bolts, "rebar": jnum(p.rebar) })).collect::<Vec<_>>() })
            }
            "report.csv" => Value::from(report::report_csv(&get::<KeptReport>(reg, c.raw(0).and_then(Value::as_str))?.0)),
            "report.sheets" => {
                let r = get::<KeptReport>(reg, c.raw(0).and_then(Value::as_str))?;
                let o = c.arg(1);
                let paper = o.and_then(|o| o.get("paper")).map_or_else(|| "A3".to_string(), |v| crate::js::string_or_empty(Some(v)));
                let pages = report::report_sheets(&r.0, &paper, &str_opt(o, "model"), &str_opt(o, "date"));
                let p = crate::paper::paper_of(&paper);
                let j = Value::Array(pages.iter().map(|pg| json!({ "w": jnum(pg.w), "h": jnum(pg.h), "paper": p.key, "items": pg.items.iter().map(crate::display::Item::j).collect::<Vec<_>>() })).collect());
                keep(reg, c, KeptPages(pages));
                j
            }
            "kiss.file" => {
                let st = state_arg(reg, c.raw(0))?;
                let b = built_arg(reg, c.raw(1))?;
                let m = marks_arg(reg, c.raw(2))?;
                let o = c.arg(3);
                let metric = o.and_then(|o| o.get("metric")).filter(|v| !v.is_null()).map(|v| crate::js::truthy(Some(v)));
                let d = arr(o.and_then(|o| o.get("date")));
                let g = |i: usize, dv: f64| d.get(i).and_then(Value::as_f64).unwrap_or(dv);
                let u = |i: usize, dv: f64| u32::try_from(crate::display::sat_i64(g(i, dv).max(0.0))).unwrap_or(0);
                let when = crate::kiss::When {
                    year: crate::display::sat_i64(g(0, 2026.0)),
                    month: u(1, 1.0),
                    day: u(2, 1.0),
                    hour: u(3, 0.0),
                    minute: u(4, 0.0),
                    second: u(5, 0.0),
                };
                let revs = if o.and_then(|o| o.get("revs")).is_some_and(|v| crate::js::truthy(Some(v))) {
                    Some(crate::issues::revs_by_number(arr(st.get("issues"))))
                } else {
                    None
                };
                let k = crate::kiss::kiss_file(&st, b, m, metric, when, &str_opt(o, "job"), revs.as_deref());
                crate::kiss::kiss_json(&k)
            }
            "kiss.read" => crate::kiss::read_kiss_json(&crate::js::string_or_empty(c.raw(0))),
            "kiss.inchFraction" => Value::from(crate::kiss::inch_fraction(c.num(0), c.arg(1).map_or(16.0, |v| num(Some(v))))),
            "kiss.shape" => match crate::profiles::get_profile(c.raw(0).unwrap_or(&Value::Null), &[]) {
                Some(p) => {
                    let (a, b) = crate::kiss::kiss_shape(&p, c.arg(1).is_some_and(|v| crate::js::truthy(Some(v))));
                    json!([a, b])
                }
                None => Value::Null,
            },
            "kiss.metric" => Value::from(crate::kiss::kiss_metric(built_arg(reg, c.raw(0))?)),
            _ => return Err("\u{0}unknown".into()),
        })
    })();
    match r {
        Err(e) if e == "\u{0}unknown" => None,
        r => Some(r),
    }
}

/// The parity view of a PDF (forks/tools/dcdiff/ops-draw.mjs `pdfView`): its bytes when written uncompressed; else the
/// inflated content streams and the file with streams, lengths and the xref left out.
pub fn pdf_view(bytes: &[u8], compressed: bool) -> Value {
    if !compressed {
        return b64(bytes);
    }
    let latin = |b: &[u8]| b.iter().map(|&c| char::from(c)).collect::<String>();
    let find =
        |hay: &[u8], pat: &[u8], from: usize| hay.get(from..).and_then(|h| h.windows(pat.len()).position(|w| w == pat)).map(|i| i + from);
    let mut streams = Vec::new();
    let mut skel = String::new();
    let mut at = 0usize;
    while let Some(i) = find(bytes, b">>\nstream\n", at) {
        let b = i + 10;
        let Some(j) = find(bytes, b"\nendstream", b) else { break };
        skel.push_str(&latin(bytes.get(at..b).unwrap_or(&[])));
        skel.push('*');
        let inflated = miniz_oxide::inflate::decompress_to_vec_zlib(bytes.get(b..j).unwrap_or(&[])).unwrap_or_default();
        streams.push(Value::from(latin(&inflated)));
        at = j;
    }
    skel.push_str(&latin(bytes.get(at..).unwrap_or(&[])));
    // /Length N → /Length *, everything from "xref\n" on → xref*
    let mut out = String::with_capacity(skel.len());
    let mut rest = skel.as_str();
    while let Some(i) = rest.find("/Length ") {
        out.push_str(rest.get(..i + 8).unwrap_or(""));
        let tail = rest.get(i + 8..).unwrap_or("");
        let nd = tail.bytes().take_while(u8::is_ascii_digit).count();
        if nd > 0 {
            out.push('*');
        }
        rest = tail.get(nd..).unwrap_or("");
    }
    out.push_str(rest);
    if let Some(i) = out.find("xref\n") {
        out.truncate(i);
        out.push_str("xref*");
    }
    json!({ "streams": streams, "skeleton": out })
}

/// Kept report pages.
pub struct KeptPages(pub Vec<List>);

fn pts2(v: Option<&Value>) -> Vec<[f64; 2]> {
    arr(v).iter().map(|p| [num(p.get(0)), num(p.get(1))]).collect()
}
fn pj(p: &[[f64; 2]]) -> Value {
    Value::Array(p.iter().map(|q| json!([jnum(q[0]), jnum(q[1])])).collect())
}

/// Speed of drawingSet + toPdf (compressed) on a model file: `DC_BENCH_MODEL=<model.json> cargo test --release -p cc-detail
/// -- --ignored --nocapture bench_draw` (the JS twin is forks/tools/dcbench-draw.mjs).
#[cfg(test)]
mod bench {
    use crate::{build::build, drawings, marks, model::read_model, pdf};
    use serde_json::Value;
    use std::time::Instant;

    #[test]
    #[ignore]
    fn bench_draw() {
        let Ok(path) = std::env::var("DC_BENCH_MODEL") else { return };
        let text = std::fs::read_to_string(path).unwrap();
        let runs: usize = std::env::var("DC_BENCH_RUNS").ok().and_then(|r| r.parse().ok()).unwrap_or(7);
        let (st, _) = read_model(&Value::String(text)).unwrap();
        let b = build(&st).unwrap();
        let m = marks::mark_parts(&b, 1.0, false);
        let fm = marks::mark_footings(crate::js::arr(st.get("footings")), false);
        let now = pdf::Utc { year: 2026, month: 10, day: 13, hour: 0, minute: 0, second: 0 };
        let (mut best_set, mut best_pdf) = (f64::INFINITY, f64::INFINITY);
        let mut size = 0;
        for _ in 0..=runs {
            let t0 = Instant::now();
            let set = drawings::drawing_set(&st, &b, &m, Some(&fm), "2026-10-13", "", true);
            let lists: Vec<crate::display::List> =
                set.sheets.iter().map(|s| crate::display::List { w: s.w, h: s.h, items: s.items.clone() }).collect();
            let t1 = Instant::now();
            let (bytes, _) = pdf::to_pdf(&lists, "Drawings", true, now);
            let t2 = Instant::now();
            size = bytes.len();
            best_set = best_set.min((t1 - t0).as_secs_f64() * 1e3);
            best_pdf = best_pdf.min((t2 - t1).as_secs_f64() * 1e3);
        }
        println!("BENCH drawingSet {best_set:.2} ms  toPdf {best_pdf:.2} ms  ({size} bytes)");
    }
}
