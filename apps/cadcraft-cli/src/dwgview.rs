//! `cadcraft-cli dwgview <file.dwg|.dxf> [--xrefs DIR] [--picks N] [--full3d]`: what BimCraft 0.5's DWG view shows for
//! a drawing beyond the drawing itself, as JSON (forks/tools/dwgviewdiff.mjs builds the same from the 0.5 JS): the
//! equipment tab, schedule (every choice: rows as shown, count line, the CSV, its name and status line; filters; the
//! ring after "Where"), the dynamic-block list, model space in 3D (stats, owners, layers, bounds, each batch's arrays
//! hashed or in full) and Properties in 3D, and Properties for picked objects. `--bench N`: timings (best of N).

use std::process::ExitCode;
use std::time::Instant;

use cc_dwgview::{compile3d, dynamic, equipment, page};
use cc_io_dwg::Obj;
use serde_json::{Value, json};

fn opt<'a>(rest: &[&'a str], k: &str) -> Option<&'a str> {
    rest.iter().position(|a| *a == k).and_then(|i| rest.get(i + 1)).copied()
}

fn read_model(name: &str, bytes: &[u8]) -> Obj {
    if name.to_ascii_lowercase().ends_with(".dxf") && !bytes.starts_with(b"AC10") {
        cc_io_dwg::dxf_model::read_dxf_model(&cc_io_dwg::dxf_model::decode_dxf_bytes(bytes))
    } else {
        cc_io_dwg::read_dwg(bytes, &cc_io_dwg::ReadOpts::default())
    }
}

/// JSON number, null for NaN / ±Infinity (as JSON.stringify).
fn n(x: f64) -> Value {
    serde_json::Number::from_f64(x).map_or(Value::Null, Value::Number)
}

/// Two FNV-1a 32-bit hashes over the little-endian bytes (the harness hashes Float64Array / Uint32Array bytes alike).
fn digest(bytes: impl Iterator<Item = u8>) -> String {
    let (mut h1, mut h2) = (0x811c_9dc5u32, 0x050c_5d1fu32);
    for b in bytes {
        h1 = (h1 ^ u32::from(b)).wrapping_mul(16_777_619);
        h2 = (h2 ^ u32::from(b)).wrapping_mul(16_777_619);
        h2 = h2.rotate_left(5);
    }
    format!("{h1:08x}{h2:08x}")
}
fn hf(v: &[f64]) -> String {
    digest(v.iter().flat_map(|x| x.to_le_bytes()))
}
fn hu(v: &[u32]) -> String {
    digest(v.iter().flat_map(|x| x.to_le_bytes()))
}

fn opt_s(s: &Option<String>) -> Value {
    s.as_ref().map_or(Value::Null, |x| Value::String(x.clone()))
}

fn equipment_json(db: &Obj, d: &cc_drawing::Drawing, file: &str) -> Value {
    let Some(list) = equipment::equipment_list(db, d) else { return Value::Null };
    let sheet_name = |k: &str| page::sheet_name(d, k);
    let rows: Vec<Value> = list
        .rows
        .iter()
        .map(|r| {
            json!({
                "no": r.no, "qty": r.qty, "qtys": r.qtys, "category": r.category, "mfr": r.mfr, "model": r.model,
                "remark": r.remark, "elecText": r.elec_text, "plumbText": r.plumb_text, "elRemark": r.el_remark,
                "plRemark": r.pl_remark, "sheets": r.sheets, "clash": r.clash,
                "places": r.places.iter().map(|p| json!({"x": n(p.x), "y": n(p.y), "qty": p.qty, "sheets": p.sheets})).collect::<Vec<_>>(),
            })
        })
        .collect();
    let head = page::equip_head(&list);
    let picks: Vec<Value> = head
        .choices
        .iter()
        .map(|c| {
            let shown = page::equip_shown(&list, &c.value, "");
            let cells: Vec<Value> = shown
                .iter()
                .map(|r| {
                    let x = page::row_cells(r, &sheet_name);
                    json!([
                        x.item,
                        x.qty,
                        x.qty_sub,
                        x.category,
                        x.clash,
                        x.remark,
                        x.mfr,
                        x.model,
                        x.elec,
                        x.el_remark,
                        x.plumb,
                        x.pl_remark,
                        x.where_,
                        x.where_title,
                        x.tags
                    ])
                })
                .collect();
            let name = page::csv_name(file, &c.label, &c.value);
            let csv = format!("\u{feff}{}", equipment::equipment_csv(&shown, &sheet_name));
            json!({"value": c.value, "text": c.text, "count": page::count_line(shown.len(), ""), "rows": cells, "csvName": name,
                "csv": csv, "status": page::downloaded(&name, shown.len())})
        })
        .collect();
    // filters: a word from each of the first rows' fields, a word that matches nothing, two words
    let mut queries: Vec<String> = vec!["zzzz-no-such".into(), "  ".into()];
    for r in list.rows.iter().take(6) {
        for f in [&r.no, &r.category, &r.mfr, &r.model, &r.elec_text, &r.plumb_text] {
            if let Some(w) = f.split_whitespace().next() {
                queries.push(w.to_string());
            }
        }
    }
    if let Some(r) = list.rows.first() {
        queries.push(format!("{} {}", r.category, r.no).to_uppercase());
    }
    let filters: Vec<Value> = queries
        .iter()
        .map(|q| {
            let q = cc_cad::js::trim(q).to_string();
            let shown = page::equip_shown(&list, page::EVERY, &q);
            json!({"query": q, "count": page::count_line(shown.len(), &q), "items": shown.iter().map(|r| r.no.clone()).collect::<Vec<_>>()})
        })
        .collect();
    let rings: Vec<Value> = list
        .rows
        .iter()
        .map(|r| {
            let place = r.places.iter().find(|p| !p.sheets.is_empty()).or(r.places.first());
            match place {
                Some(p) => {
                    let (key, x, y) = page::focus_target(d, p.x, p.y, p.sheets.first().map(String::as_str));
                    json!({"key": key, "x": n(x), "y": n(y), "status": page::ringed(r, &sheet_name(&key))})
                }
                None => Value::Null,
            }
        })
        .collect();
    let (tl, tt) = page::tab_label(&list);
    let (ik, iv) = page::info_row(&list);
    json!({
        "tab": [tl, tt], "info": [ik, iv], "tags": list.tags, "offSheet": list.off_sheet,
        "sheets": list.sheets.iter().map(|s| json!({"key": s.key, "name": s.name, "count": s.count})).collect::<Vec<_>>(),
        "rows": rows,
        "head": {"kind": head.kind, "title": head.title, "intro": head.intro, "which": head.which, "placeholder": head.filter_placeholder,
            "filterLabel": head.filter_label, "csv": head.csv, "headers": head.headers, "note": head.note},
        "picks": picks, "filters": filters, "rings": rings,
    })
}

fn colors(v: &[(String, u64)]) -> Value {
    Value::Array(v.iter().map(|(k, c)| json!([k, c])).collect())
}

fn model3_json(m: &compile3d::Model3, full: bool) -> Value {
    let s = &m.stats;
    let batches: Vec<Value> = m
        .batches
        .iter()
        .map(|b| {
            let mut o = json!({
                "layer": b.layer, "color": compile3d::color_name(b.color), "lines": b.lines.len(), "tris": b.tris.len(),
                "hl": hf(&b.lines), "ht": hf(&b.tris), "hlo": hu(&b.line_owner), "hto": hu(&b.tri_owner),
                "texts": b.texts.iter().map(|t| json!({"owner": t.owner, "s": t.s, "o": t.o.map(n), "xd": t.xd.map(n), "yd": t.yd.map(n),
                    "ha": t.ha.map_or(Value::Null, n), "va": n(t.va)})).collect::<Vec<_>>(),
            });
            if full && let Some(obj) = o.as_object_mut() {
                obj.insert("L".into(), Value::Array(b.lines.iter().map(|x| n(*x)).collect()));
                obj.insert("T".into(), Value::Array(b.tris.iter().map(|x| n(*x)).collect()));
            }
            o
        })
        .collect();
    json!({
        "units": m.units,
        "stats": {"entities": s.entities, "segments": s.segments, "triangles": s.triangles, "drawn": colors(&s.drawn),
            "notDrawn": colors(&s.not_drawn), "flatOnly": colors(&s.flat_only), "polyline3dFixed": s.polyline3d_fixed, "truncated": s.truncated},
        "owners": m.owners.iter().map(|o| json!([o.ty, o.handle, o.layer])).collect::<Vec<_>>(),
        "min": m.min.map(n), "max": m.max.map(n),
        "layers": m.layers.iter().map(|l| json!([l.name, compile3d::color_name(l.color), l.on])).collect::<Vec<_>>(),
        "batches": batches,
    })
}

fn pick_json(v: Option<page::PickView>) -> Value {
    match v {
        None => Value::Null,
        Some(page::PickView::Many { title, hint, counts }) => json!({"many": title, "hint": hint, "counts": counts}),
        Some(page::PickView::One { cls, name, rows, attrs, back }) => json!({"cls": cls, "name": name, "rows": rows,
            "attrs": attrs.map(|(s, r)| json!([s, r])), "back": back}),
    }
}

fn props3d_json(p: &page::Props3d) -> Value {
    let counts = |c: &Option<page::Counts>| c.as_ref().map_or(Value::Null, |c| json!({"title": c.title, "rows": c.rows, "note": c.note}));
    json!({"hint": p.hint, "intro": p.intro, "rows": p.rows, "flat": counts(&p.flat), "missing": counts(&p.missing)})
}

/// `--lookup <cases.json>`: bimcraft.js `findLinked` / `findPicture` on made-up folders:
/// `[{files: [relative paths], added: [names], path?: drawing's place, xref?: name, picture?: name}]` →
/// `[{linked: {where, exact, found} | null, picture: … | null}]`.
fn lookup(file: &str) -> ExitCode {
    use cc_dwgview::folder::{Folder, Found, find_linked, find_picture};
    let Ok(text) = std::fs::read_to_string(file) else {
        eprintln!("{file}: cannot read");
        return ExitCode::from(1);
    };
    let Ok(Value::Array(cases)) = serde_json::from_str::<Value>(&text) else {
        eprintln!("{file}: not a JSON array");
        return ExitCode::from(1);
    };
    let strs = |v: Option<&Value>| -> Vec<String> {
        v.and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default()
    };
    let hit = |h: Option<cc_dwgview::folder::Hit>| -> Value {
        h.map_or(Value::Null, |h| {
            let found = match h.found {
                Found::Added(n) => format!("added:{n}"),
                Found::Path(p) => format!("path:{p}"),
            };
            json!({"where": h.where_, "exact": h.exact, "found": found})
        })
    };
    let out: Vec<Value> = cases
        .iter()
        .map(|c| {
            let folder = c.get("files").map(|_| Folder::new("F", strs(c.get("files")), Vec::new()));
            let added = strs(c.get("added"));
            let path = c.get("path").and_then(Value::as_str);
            let linked = c.get("xref").and_then(Value::as_str).map(|x| find_linked(x, path, &added, folder.as_ref()));
            let picture = c.get("picture").and_then(Value::as_str).map(|x| find_picture(x, path, &added, folder.as_ref()));
            json!({"linked": hit(linked.flatten()), "picture": hit(picture.flatten())})
        })
        .collect();
    println!("{}", Value::Array(out));
    ExitCode::SUCCESS
}

/// `cadcraft-cli dwgview …`.
pub fn run(rest: &[&str]) -> ExitCode {
    if let Some(f) = opt(rest, "--lookup") {
        return lookup(f);
    }
    let Some(path) = rest.first() else {
        eprintln!("usage: cadcraft-cli dwgview <file.dwg|.dxf> [--xrefs DIR] [--picks N] [--full3d] [--bench N]");
        return ExitCode::from(2);
    };
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::from(1);
        }
    };
    let file = std::path::Path::new(path).file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
    let db = read_model(&file, &bytes);
    if let Some(e) = db.obj("report").and_then(|r| r.str("error")) {
        println!("{}", json!({"$error": e}));
        return ExitCode::from(1);
    }
    // linked drawings: "<name>.dwg" in DIR (any case), as the folder lookup finds them
    let mut xrefs: Vec<(String, Obj)> = Vec::new();
    if let Some(dir) = opt(rest, "--xrefs") {
        let names: Vec<String> = db
            .arr("blocks")
            .map(|bs| {
                bs.iter()
                    .filter_map(|b| b.as_obj())
                    .filter(|b| b.num("flags").is_some_and(|f| f.is_finite() && (f as i64) & 4 != 0))
                    .filter_map(|b| b.str("name").map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let listing: Vec<std::path::PathBuf> =
            std::fs::read_dir(dir).map(|r| r.filter_map(Result::ok).map(|e| e.path()).collect()).unwrap_or_default();
        for x in names {
            let want = format!("{x}.dwg").to_lowercase();
            if let Some(p) = listing.iter().find(|p| p.file_name().is_some_and(|f| f.to_string_lossy().to_lowercase() == want))
                && let Ok(b) = std::fs::read(p)
            {
                let m = cc_io_dwg::read_dwg(&b, &cc_io_dwg::ReadOpts::default());
                if m.obj("report").and_then(|r| r.str("error")).is_none() {
                    xrefs.push((x.to_lowercase(), m));
                }
            }
        }
    }
    let tag = if bytes.starts_with(b"AC10") { String::from_utf8_lossy(bytes.get(..6).unwrap_or(&[])).into_owned() } else { String::new() };
    let drawing = cc_io_dwg::compile::compile_drawing(&db, &tag, &xrefs);

    if let Some(runs) = opt(rest, "--bench").and_then(|v| v.parse::<usize>().ok()) {
        let best = |f: &mut dyn FnMut()| {
            f();
            let mut b = f64::INFINITY;
            for _ in 0..runs.max(1) {
                let t = Instant::now();
                f();
                b = b.min(t.elapsed().as_secs_f64() * 1000.0);
            }
            b
        };
        let eq = best(&mut || {
            let _ = equipment::equipment_list(&db, &drawing);
        });
        let csv = best(&mut || {
            if let Some(l) = equipment::equipment_list(&db, &drawing) {
                let rows: Vec<&equipment::Row> = l.rows.iter().collect();
                let _ = equipment::equipment_csv(&rows, &|k| page::sheet_name(&drawing, k));
            }
        });
        let dy = best(&mut || {
            let defs = dynamic::dynamic_defs(&db);
            let _ = dynamic::dynamic_summary(&db, &defs);
        });
        let c3 = best(&mut || {
            let _ = compile3d::compile_model3d(&db, &xrefs);
        });
        let m3 = compile3d::compile_model3d(&db, &xrefs);
        let defs = dynamic::dynamic_defs(&db);
        let ents = page::model_space(&db).len();
        let pk = best(&mut || {
            let ms = page::model_space(&db);
            for i in 0..ents.min(2000) {
                let _ = page::pick_view(ms, &m3.units, &[i], &|e| dynamic::dynamic_of(&db, e, &defs));
            }
        });
        println!("{}", json!({"equipment": eq, "csv": csv, "dynamic": dy, "compile3d": c3, "props2000": pk}));
        return ExitCode::SUCCESS;
    }

    let defs = dynamic::dynamic_defs(&db);
    let summary = dynamic::dynamic_summary(&db, &defs);
    let dyn_view = page::dyn_view(&summary).map_or(Value::Null, |v| json!({"title": v.title, "items": v.items, "note": v.note}));
    let m3 = compile3d::compile_model3d(&db, &xrefs);
    let ents = page::model_space(&db).len();
    let picks_n: usize = opt(rest, "--picks").and_then(|v| v.parse().ok()).unwrap_or(ents);
    let dyn_of = |e: &Obj| dynamic::dynamic_of(&db, e, &defs);
    let picks: Vec<Value> =
        (0..ents.min(picks_n)).map(|i| pick_json(page::pick_view(page::model_space(&db), &m3.units, &[i], &dyn_of))).collect();
    let many: Vec<usize> = (0..ents.min(7)).collect();
    let (status, err) = page::shown_status(&m3);
    let out = json!({
        "file": file,
        "equipment": equipment_json(&db, &drawing, &file),
        "dyn": {"summary": summary.iter().map(|d| json!({"name": d.name, "label": d.label, "count": d.count,
            "shown": d.shown.iter().map(|(k, c)| json!([k, c])).collect::<Vec<_>>(), "states": d.states})).collect::<Vec<_>>(), "view": dyn_view},
        "m3": model3_json(&m3, rest.contains(&"--full3d")),
        "props3d": props3d_json(&page::props3d(&m3)),
        "status3d": [status, err],
        "picks": picks,
        "many": pick_json(page::pick_view(page::model_space(&db), &m3.units, &many, &dyn_of)),
        "dynOf": (0..ents).filter_map(|i| page::model_space(&db).get(i).and_then(|e| e.as_obj()).and_then(dyn_of).map(|d|
            json!([i, d.name, opt_s(&d.label), opt_s(&d.state), d.states, d.others]))).collect::<Vec<_>>(),
    });
    println!("{out}");
    ExitCode::SUCCESS
}
