//! More op-script steps (the modules of the 0.4 K-items): links, slip-sheeting, spaces, sketch, legend, find
//! text, the workbook summary, BCF import, vector compare, page lists, blank sets, bookmarks. Same record shape as
//! [`crate::script`]; `forks/tools/mkdiff.mjs` runs the frozen JS side.

use crate::bcfread::{Entry, PlanSheet, bcf_of, bcf_value, plan_import, read_bcf, unzip_all};
use crate::csv::{Cell, CsvColumn, SheetRef};
use crate::json::{cal_of, markup_from_value};
use crate::link::{LinkSheet, callout_refs, clean_link, link_target};
use crate::model::{Markup, Store, is_type, type_name};
use crate::pages::{
    Bookmark, blank_info, clean_blank, clean_bookmarks, count_bookmarks, from_sheets, grid_content, order_delete, order_insert, order_move,
    paper_size, papers, parse_page_list, untitled_name,
};
use crate::slip::{SlipSheet, carry_markups, match_sheets, num_value, sheet_no_key, superseded_stamp};
use crate::spaces::{legend_rows, rows_key, sketch_point, space_of};
use crate::textsearch::{Item, find_hits, hit_cloud_rect, range_box, text_index};
use crate::vecdiff::{Seg, clusters, diff_segments};
use cc_m3d::json::Out;
use serde_json::{Map, Value, json};

fn num(v: Option<&Value>) -> f64 {
    v.map_or(f64::NAN, crate::js::to_number)
}
fn s(v: Option<&Value>) -> String {
    crate::js::str_or_empty(v)
}
/// `String(v ?? '')`
fn js_str(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(x) => crate::js::to_js_string(x),
    }
}
fn p2(v: Option<&Value>) -> [f64; 2] {
    let a = v.and_then(Value::as_array);
    let g = |i: usize| a.and_then(|a| a.get(i)).map_or(f64::NAN, crate::js::to_number);
    [g(0), g(1)]
}
fn nums(v: Option<&Value>) -> Vec<f64> {
    v.and_then(Value::as_array).map(|a| a.iter().map(crate::js::to_number).collect()).unwrap_or_default()
}
fn seg_list(v: Option<&Value>) -> Vec<Seg> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|s| {
                    let n = nums(Some(s));
                    let g = |i: usize| n.get(i).copied().unwrap_or(f64::NAN);
                    [g(0), g(1), g(2), g(3)]
                })
                .collect()
        })
        .unwrap_or_default()
}
fn segs_value(segs: &[Seg]) -> Value {
    Value::Array(segs.iter().map(|s| Value::Array(s.iter().map(|x| num_value(*x)).collect())).collect())
}

fn markups_of<'a>(st: &'a Store, step: &Map<String, Value>, owned: &'a [Markup]) -> Vec<&'a Markup> {
    if step.get("markups").is_some() {
        return owned.iter().collect();
    }
    match step.get("ids").and_then(Value::as_array) {
        Some(ids) => ids.iter().filter_map(|id| id.as_str().and_then(|id| st.get(id))).collect(),
        None => st.all(),
    }
}

fn slip_sheet(v: Option<&Value>) -> Option<SlipSheet> {
    let o = v?.as_object()?;
    let w = num(o.get("width"));
    let h = num(o.get("height"));
    let view = o.get("view").map(|x| nums(Some(x))).unwrap_or_else(|| vec![0.0, 0.0, w, h]);
    let g = |i: usize| view.get(i).copied().unwrap_or(f64::NAN);
    Some(SlipSheet {
        key: s(o.get("key")),
        no: js_str(o.get("no")),
        file: num(o.get("file")),
        page: num(o.get("page")),
        width: w,
        height: h,
        view: [g(0), g(1), g(2), g(3)],
        rotate: o.get("rotate").map_or(0.0, crate::js::to_number),
    })
}

fn items_of(v: Option<&Value>) -> Vec<Item> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|it| Item {
                    str: js_str(it.get("str")),
                    transform: it.get("transform").and_then(Value::as_array).map(|t| t.iter().map(crate::js::to_number).collect()),
                    width: it.get("width").and_then(Value::as_f64).unwrap_or(f64::NAN),
                    has_eol: crate::js::truthy(it.get("hasEOL")),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn bookmark_value(b: &Bookmark) -> Value {
    json!({"title": b.title, "page": num_value(b.page), "children": b.children.iter().map(bookmark_value).collect::<Vec<_>>()})
}

fn sheet_ref(v: &Value) -> SheetRef {
    SheetRef {
        title: s(v.get("title")),
        file: s(v.get("file")),
        page: v.get("page").map(|p| match p {
            Value::Number(n) => Cell::N(n.as_f64().unwrap_or(f64::NAN)),
            v => Cell::S(s(Some(v))),
        }),
    }
}

/// Run one of these steps: None when `op` is not one of them; Some(Err) = the error text the JS throws.
pub fn run_ext(st: &mut Store, op: &str, step: &Map<String, Value>, o: &mut Out) -> Option<Result<(), String>> {
    let owned: Vec<Markup> =
        step.get("markups").and_then(Value::as_array).map(|a| a.iter().map(markup_from_value).collect()).unwrap_or_default();
    let v: Value = match op {
        "cleanLink" => Value::String(clean_link(&js_str(step.get("v")))),
        "sheetNoKey" => {
            let v = step.get("s");
            Value::String(sheet_no_key(&if crate::js::truthy(v) { js_str(v) } else { String::new() }))
        }
        "linkTarget" => {
            let sheets: Vec<LinkSheet> = step
                .get("sheets")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter().map(|x| LinkSheet { key: s(x.get("key")), sheet_no: s(x.get("sheetNo")), title: s(x.get("title")) }).collect()
                })
                .unwrap_or_default();
            let link = js_str(step.get("link"));
            match link_target(&link, &sheets).and_then(|i| sheets.get(i)) {
                Some(sh) => Value::String(sh.key.clone()),
                None => Value::Null,
            }
        }
        "calloutRefs" => {
            let nos: Vec<String> =
                step.get("nos").and_then(Value::as_array).map(|a| a.iter().map(|x| s(Some(x))).collect()).unwrap_or_default();
            let refs = callout_refs(&js_str(step.get("text")), &nos);
            Value::Array(refs.iter().map(|r| json!({"start": r.start, "end": r.end, "ref": r.r#ref, "no": r.no})).collect())
        }
        "matchSheets" => {
            let list = |k: &str| -> Vec<SlipSheet> {
                step.get(k).and_then(Value::as_array).map(|a| a.iter().filter_map(|x| slip_sheet(Some(x))).collect()).unwrap_or_default()
            };
            let pairs = match_sheets(&list("olds"), &list("news"));
            Value::Array(pairs.iter().map(|p| json!({"newKey": p.new_key, "oldKey": p.old_key, "by": p.by})).collect())
        }
        "carryMarkups" => {
            let ms = markups_of(st, step, &owned);
            let from = slip_sheet(step.get("from"));
            let to = slip_sheet(step.get("to"));
            let c = carry_markups(&ms, &s(step.get("toKey")), from.as_ref(), to.as_ref(), crate::js::truthy(step.get("skipClosed")));
            json!({"items": c.items, "sources": c.sources, "left": c.left, "scaled": c.scaled, "closed": c.closed})
        }
        "supersededStamp" => {
            let sheet = step.get("sheet").and_then(Value::as_str);
            match slip_sheet(step.get("page")) {
                Some(page) => Value::Object(superseded_stamp(&page, &s(step.get("label")), &s(step.get("author")), sheet)),
                None => Value::Null,
            }
        }
        "spaceOf" => {
            let ms = markups_of(st, step, &owned);
            let spaces: Vec<&Markup> = step
                .get("spaces")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|id| id.as_str().and_then(|id| st.get(id))).collect())
                .unwrap_or_default();
            let env = crate::shapes::ShapeEnv::default();
            let bo = |m: &Markup| crate::shapes::list_bounds(&crate::shapes::shapes(m, &env));
            Value::Array(ms.iter().map(|m| Value::String(space_of(m, &spaces, &bo))).collect())
        }
        "sketchPoint" => {
            let cal = step.get("cal").filter(|c| c.is_object()).and_then(cal_of);
            match sketch_point(
                p2(step.get("from")),
                p2(step.get("toward")),
                &js_str(step.get("text")),
                cal.as_ref(),
                step.get("rot").map_or(0.0, crate::js::to_number),
            ) {
                Ok(p) => json!({"pt": [num_value(p[0]), num_value(p[1])]}),
                Err(e) => json!({"error": e}),
            }
        }
        "legendRows" => {
            let ms = markups_of(st, step, &owned);
            let name = |m: &Markup| if is_type(&m.kind) { type_name(&m.kind) } else { m.kind.clone() };
            let scope = step.get("scope").and_then(Value::as_str).unwrap_or("sheet");
            let rows = legend_rows(&ms, step.get("sheet").and_then(Value::as_str), scope, &name);
            json!({
                "rows": rows.iter().map(|r| json!({"label": r.label, "color": r.color, "type": r.kind, "n": num_value(r.n)})).collect::<Vec<_>>(),
                "key": rows_key(&rows),
            })
        }
        "findHits" => {
            let idx = text_index(items_of(step.get("items")));
            let max = step.get("max").map_or(2000.0, crate::js::to_number);
            let hits = find_hits(
                &idx,
                &js_str(step.get("query")),
                crate::js::truthy(step.get("matchCase")),
                crate::js::truthy(step.get("wholeWord")),
                max,
            );
            json!({
                "text": idx.text,
                "spans": idx.spans.iter().map(|sp| json!([sp.start, sp.end])).collect::<Vec<_>>(),
                "hits": hits.iter().map(|h| json!({"start": h.start, "end": h.end, "rect": h.rect.iter().map(|x| num_value(*x)).collect::<Vec<_>>(), "snippet": h.snippet})).collect::<Vec<_>>(),
                "clouds": hits.iter().map(|h| hit_cloud_rect(h.rect, step.get("pad").map_or(4.0, crate::js::to_number)).iter().map(|x| num_value(*x)).collect::<Vec<_>>()).collect::<Vec<_>>(),
            })
        }
        "rangeBox" => {
            let idx = text_index(items_of(step.get("items")));
            match range_box(&idx, num(step.get("start")), num(step.get("end"))) {
                Some(b) => Value::Array(b.iter().map(|x| num_value(*x)).collect()),
                None => Value::Null,
            }
        }
        "summarySheets" => {
            let ms = markups_of(st, step, &owned);
            let sheets = step.get("sheets").and_then(Value::as_object).cloned().unwrap_or_default();
            let sheet_of = |m: &Markup| m.sheet.as_ref().and_then(|k| sheets.get(k)).map(sheet_ref);
            let meas = step.get("measure").and_then(Value::as_object).cloned().unwrap_or_default();
            let measure_of = |m: &Markup| s(meas.get(&m.id));
            let cols: Vec<CsvColumn> = step
                .get("columns")
                .and_then(Value::as_array)
                .map(|a| a.iter().map(|c| CsvColumn { key: s(c.get("key")), name: s(c.get("name")) }).collect())
                .unwrap_or_default();
            let env = crate::shapes::ShapeEnv::default();
            let all = st.all();
            let space_fn = |m: &Markup| {
                let spaces: Vec<&Markup> = all.iter().filter(|x| x.kind == "space" && x.sheet == m.sheet).copied().collect();
                space_of(m, &spaces, &|x: &Markup| crate::shapes::list_bounds(&crate::shapes::shapes(x, &env)))
            };
            let sp: Option<&dyn Fn(&Markup) -> String> = if crate::js::truthy(step.get("spaces")) { Some(&space_fn) } else { None };
            Value::Array(crate::summary::summary_sheets(&ms, &sheet_of, &measure_of, sp, &cols))
        }
        "writeXlsx" => {
            let sheets = step.get("sheets").and_then(Value::as_array).cloned().unwrap_or_default();
            let compress = crate::js::truthy(step.get("compress"));
            let bytes = crate::xlsx::write_xlsx(&sheets, &js_str(step.get("title")), compress, st.env.now_ms);
            if compress { unzipped_value(&bytes) } else { json!({"$base64": crate::imagestamp::b64_encode(&bytes)}) }
        }
        "unzipAll" => unzipped_value(&crate::imagestamp::b64_decode(&s(step.get("b64")))),
        "readBcf" => match read_bcf(&crate::imagestamp::b64_decode(&s(step.get("b64")))) {
            Ok(b) => bcf_value(&b),
            Err(e) => return Some(Err(e)),
        },
        "planImport" => {
            let ms = markups_of(st, step, &owned);
            let bcf = bcf_of(step.get("bcf").unwrap_or(&Value::Null));
            let pairs: Vec<(String, String)> = step
                .get("guidOf")
                .and_then(Value::as_array)
                .map(|a| a.iter().map(|p| (s(p.get(0)), s(p.get(1)))).collect())
                .unwrap_or_default();
            let rg: Vec<String> =
                step.get("replyGuids").and_then(Value::as_array).map(|a| a.iter().map(|x| s(Some(x))).collect()).unwrap_or_default();
            let sheets: Vec<PlanSheet> = step
                .get("sheets")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .map(|x| {
                            let vw = nums(x.get("view"));
                            let g = |i: usize| vw.get(i).copied().unwrap_or(f64::NAN);
                            PlanSheet {
                                key: s(x.get("key")),
                                title: s(x.get("title")),
                                sheet_no: s(x.get("sheetNo")),
                                file: s(x.get("file")),
                                view: [g(0), g(1), g(2), g(3)],
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            plan_import(&bcf, &ms, &pairs, &rg, &sheets, &s(step.get("current")), &s(step.get("author")))
        }
        "diffSegments" => {
            let t = step.get("T").and_then(Value::as_array).map(|a| {
                let g = |i: usize| a.get(i).map_or(f64::NAN, crate::js::to_number);
                [g(0), g(1), g(2), g(3), g(4), g(5)]
            });
            let d = diff_segments(
                &seg_list(step.get("old")),
                &seg_list(step.get("new")),
                step.get("tol").map_or(1.5, crate::js::to_number),
                t.as_ref(),
                step.get("step").map_or(2.0, crate::js::to_number),
            );
            json!({"removed": segs_value(&d.removed), "added": segs_value(&d.added), "kept": d.kept})
        }
        "clusters" => {
            let b = clusters(
                &seg_list(step.get("segs")),
                step.get("gap").map_or(18.0, crate::js::to_number),
                step.get("minLen").map_or(6.0, crate::js::to_number),
            );
            segs_value(&b)
        }
        "orderDelete" => Value::Array(order_delete(num(step.get("n")), &nums(step.get("indices"))).into_iter().map(num_value).collect()),
        "orderMove" => Value::Array(
            order_move(num(step.get("n")), &nums(step.get("indices")), num(step.get("to"))).into_iter().map(num_value).collect(),
        ),
        "orderInsert" => Value::Array(
            order_insert(num(step.get("n")), num(step.get("at")), num(step.get("count")))
                .into_iter()
                .map(|x| x.map_or(Value::Null, num_value))
                .collect(),
        ),
        "parsePageList" => match parse_page_list(&js_str(step.get("text")), num(step.get("n"))) {
            Ok(ix) => json!({"indices": ix.into_iter().map(num_value).collect::<Vec<_>>()}),
            Err(e) => json!({"error": e}),
        },
        "cleanBlank" => {
            let b = clean_blank(step.get("raw"));
            let [w, h] = paper_size(&b);
            json!({"paper": b.paper, "orient": b.orient, "grid": b.grid, "pages": num_value(b.pages), "size": [num_value(w), num_value(h)]})
        }
        "papers" => Value::Array(
            papers().iter().map(|p| json!({"key": p.key, "w": num_value(p.w), "h": num_value(p.h), "metric": p.metric})).collect(),
        ),
        "gridContent" => {
            Value::String(grid_content(num(step.get("w")), num(step.get("h")), &s(step.get("grid")), crate::js::truthy(step.get("metric"))))
        }
        "blankInfo" => {
            let raw = step.get("raw");
            let text = match raw {
                None | Some(Value::Null) => None,
                Some(x) => Some(crate::js::to_js_string(x)),
            };
            match blank_info(text.as_deref()) {
                Some(b) => json!({"paper": b.paper, "orient": b.orient, "grid": b.grid, "metric": b.metric}),
                None => Value::Null,
            }
        }
        "untitledName" => {
            let names: Vec<String> =
                step.get("names").and_then(Value::as_array).map(|a| a.iter().map(|x| js_str(Some(x))).collect()).unwrap_or_default();
            let (name, seq) = untitled_name(&names, step.get("seq").map_or(0.0, crate::js::to_number));
            json!({"name": name, "seq": num_value(seq)})
        }
        "cleanBookmarks" => {
            let b = clean_bookmarks(step.get("v"), 0);
            json!({"items": b.iter().map(bookmark_value).collect::<Vec<_>>(), "count": count_bookmarks(&b)})
        }
        "fromSheets" => {
            let pages: Vec<(f64, String)> = step
                .get("pages")
                .and_then(Value::as_array)
                .map(|a| a.iter().map(|p| (num(p.get("index")), js_str(p.get("title")))).collect())
                .unwrap_or_default();
            Value::Array(from_sheets(&pages).iter().map(bookmark_value).collect())
        }
        _ => return None,
    };
    o.value(&v);
    Some(Ok(()))
}

/// A zip's entries as `[[name, {$base64} | null | "too big"]]` (unzipAll's view).
fn unzipped_value(bytes: &[u8]) -> Value {
    match unzip_all(bytes) {
        Ok(entries) => Value::Array(
            entries
                .iter()
                .map(|(n, e)| {
                    json!([
                        n,
                        match e {
                            Entry::Bytes(b) => json!({"$base64": crate::imagestamp::b64_encode(b)}),
                            Entry::TooBig => Value::String("too big".into()),
                            Entry::Null => Value::Null,
                        }
                    ])
                })
                .collect(),
        ),
        Err(e) => Value::String(e.into()),
    }
}
