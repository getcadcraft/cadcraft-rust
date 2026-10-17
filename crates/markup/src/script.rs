//! The op-script runner behind `cadcraft-cli markup <script.json>` and the parity harness (`forks/tools/mkdiff.mjs`):
//! a script is `{env: {now, count}, steps: [...]}`; every step runs against one [`Store`] (or calls one of the
//! module functions) and gives one record `{r, s?, e?}`: its result, the store's state after a step that can change
//! it, and the error text when the JS would throw. Records are written as canonical JSON (non-finite numbers as the
//! parity kit's "$NaN" / "$Inf").

use crate::bcf::{Topic, build_bcf};
use crate::chest::{clean_tool, default_chest, export_chest, merge_chest, parse_chest, tool_from_markup};
use crate::clip::{Clip, PlaceOpts, clean_clip, clean_clip_text, clip_bounds, clip_value, make_clip, place_clip};
use crate::csv::{Cell, CsvColumn, SheetRef, markup_rows, punch_rows, to_csv};
use crate::font::{text_width, win_ansi_hex, wrap_text};
use crate::geom::{GripPatch, apply_grip, bounds_of, grips, move_by, nums_value, pick, pick_window, pts_value, similarity, simplify};
use crate::json::{cal_of, markup_from_value, write_cal, write_markup};
use crate::model::{Env, Markup, SheetName, Store, StoreError, clean_reply, geometry_ok, make_markup, status_reply};
use crate::shapes::{LegendRow, ShapeEnv, shapes, write_list};
use crate::titleblock::{TextPiece, Viewport, is_wording, read_title_block};
use crate::units::{
    calibration_from, format_area, format_length, measure, measure_area, parse_length, preset_calibration, valid_calibration_json,
};
use cc_m3d::json::Out;
use serde_json::{Map, Value};

/// Most steps one script may hold (a script is hostile input like any file).
pub const MAX_STEPS: usize = 200_000;

fn num(v: Option<&Value>) -> f64 {
    v.map_or(f64::NAN, crate::js::to_number)
}
fn p2(v: Option<&Value>) -> [f64; 2] {
    let a = v.and_then(Value::as_array);
    let g = |i: usize| a.and_then(|a| a.get(i)).map_or(f64::NAN, crate::js::to_number);
    [g(0), g(1)]
}
fn s(v: Option<&Value>) -> String {
    crate::js::str_or_empty(v)
}

/// `env` of shapes / picks: `{cal, badge, legend}`.
pub fn shape_env(v: Option<&Value>) -> ShapeEnv {
    let o = v.and_then(Value::as_object);
    let cal = o.and_then(|o| o.get("cal")).filter(|c| c.is_object()).and_then(cal_of);
    let badge = o.and_then(|o| o.get("badge")).map_or(0.0, crate::js::to_number);
    let legend = o
        .and_then(|o| o.get("legend"))
        .and_then(Value::as_array)
        .map(|rows| rows.iter().map(|r| LegendRow { color: s(r.get("color")), label: s(r.get("label")), n: num(r.get("n")) }).collect());
    ShapeEnv { cal, badge: if badge.is_nan() { 0.0 } else { badge }, legend }
}

fn write_value_canon(o: &mut Out, v: &Value) {
    o.value(v);
}

fn markups_arr(o: &mut Out, ms: &[&Markup]) {
    o.ch('[');
    for (i, m) in ms.iter().enumerate() {
        if i > 0 {
            o.ch(',');
        }
        write_markup(o, m);
    }
    o.ch(']');
}

/// Write the store's state.
pub fn write_state(o: &mut Out, st: &Store) {
    o.raw("{\"markups\":");
    markups_arr(o, &st.all());
    o.raw(",\"calibrations\":{");
    for (i, (k, c)) in st.calibrations.iter().enumerate() {
        if i > 0 {
            o.ch(',');
        }
        o.key(k);
        write_cal(o, c);
    }
    o.raw("},\"sheetNames\":{");
    for (i, (k, n)) in st.sheet_names.iter().enumerate() {
        if i > 0 {
            o.ch(',');
        }
        o.key(k);
        o.raw("{\"number\":");
        o.str(&n.number);
        o.raw(",\"title\":");
        o.str(&n.title);
        o.ch('}');
    }
    o.raw("},\"punchNext\":");
    o.num(st.punch_next);
    o.raw(",\"version\":");
    o.num(st.version as f64);
    o.raw(",\"dirty\":");
    o.strs(&st.dirty_sheets.iter().cloned().collect::<Vec<_>>());
    o.raw(",\"undo\":[");
    for (i, step) in st.undo_stack.iter().enumerate() {
        if i > 0 {
            o.ch(',');
        }
        o.raw("{\"label\":");
        o.str(&step.label);
        o.raw(",\"ops\":");
        o.strs(&step.ops.iter().map(|op| op.kind().to_string()).collect::<Vec<_>>());
        o.ch('}');
    }
    o.raw("],\"redo\":");
    o.strs(&st.redo_stack.iter().map(|s| s.label.clone()).collect::<Vec<_>>());
    o.raw(",\"layers\":{");
    for (i, (k, l)) in st.layers.iter().enumerate() {
        if i > 0 {
            o.ch(',');
        }
        o.key(k);
        o.raw("{\"hidden\":");
        o.bool(l.hidden);
        o.raw(",\"locked\":");
        o.bool(l.locked);
        o.ch('}');
    }
    o.raw("},\"columns\":");
    write_value_canon(o, &crate::columns::columns_to_value(&st.columns));
    o.raw(",\"layerNames\":");
    o.strs(&st.layer_names());
    o.raw(",\"punchItems\":");
    o.strs(&st.punch_items().iter().map(|m| m.id.clone()).collect::<Vec<_>>());
    o.ch('}');
}

fn sheet_name(v: &Value) -> Option<SheetName> {
    let o = v.as_object()?;
    Some(SheetName { number: s(o.get("number")), title: s(o.get("title")) })
}

fn write_grip_patch(o: &mut Out, p: &GripPatch) {
    let v = Value::Object(p.to_map());
    o.value(&v);
}

fn clip_from_value(v: &Value) -> Option<Clip> {
    let o = v.as_object()?;
    let items = o.get("items")?.as_array()?.iter().filter_map(|i| i.as_object().cloned()).collect();
    Some(Clip {
        source: s(o.get("source")),
        sheet: s(o.get("sheet")),
        at: s(o.get("at")),
        items,
        skipped: o.get("skipped").and_then(Value::as_f64).map(crate::js::f2usize),
    })
}

fn write_clip(o: &mut Out, c: Option<&Clip>) {
    match c {
        Some(c) => o.value(&clip_value(c)),
        None => o.raw("null"),
    }
}

fn cells_value(rows: &[Vec<Cell>]) -> Value {
    Value::Array(
        rows.iter()
            .map(|r| {
                Value::Array(
                    r.iter()
                        .map(|c| match c {
                            Cell::S(t) => Value::String(t.clone()),
                            Cell::N(n) => serde_json::Number::from_f64(*n).map_or(Value::Null, Value::Number),
                        })
                        .collect(),
                )
            })
            .collect(),
    )
}

fn markup_arg(st: &Store, step: &Map<String, Value>) -> Option<Markup> {
    if let Some(m) = step.get("markup") {
        return Some(markup_from_value(m));
    }
    step.get("id").and_then(Value::as_str).and_then(|id| st.get(id)).cloned()
}

/// Run one script (parsed JSON) and give the records as JSON text.
pub fn run(script: &Value) -> String {
    let mut st = Store::new();
    if let Some(e) = script.get("env") {
        st.env = Env { now_ms: num(e.get("now")), count: e.get("count").and_then(Value::as_u64).unwrap_or(0) };
    }
    let empty = Vec::new();
    let steps = script.get("steps").and_then(Value::as_array).unwrap_or(&empty);
    let mut o = Out::new(true);
    o.ch('[');
    for (i, step) in steps.iter().take(MAX_STEPS).enumerate() {
        if i > 0 {
            o.ch(',');
        }
        let Some(stp) = step.as_object() else {
            o.raw("{\"r\":null,\"e\":\"bad step\"}");
            continue;
        };
        run_step(&mut st, stp, &mut o);
    }
    o.ch(']');
    o.s
}

fn run_step(st: &mut Store, step: &Map<String, Value>, o: &mut Out) {
    let op = step.get("op").and_then(Value::as_str).unwrap_or("");
    let mut state = false;
    let mut err: Option<String> = None;
    o.raw("{\"r\":");
    match op {
        "env" => {
            st.env.now_ms = num(step.get("now"));
            if let Some(c) = step.get("count").and_then(Value::as_u64) {
                st.env.count = c;
            }
            o.raw("null");
        }
        "change" => {
            state = true;
            let label = s(step.get("label"));
            let empty = Vec::new();
            let ops = step.get("tx").and_then(Value::as_array).unwrap_or(&empty).clone();
            let mut results: Vec<String> = Vec::new();
            let res = st.change(&label, |tx| {
                for t in &ops {
                    let Some(t) = t.as_object() else { continue };
                    if let Some(f) = t.get("add") {
                        let fm = f.as_object().cloned().unwrap_or_default();
                        let m = tx.add_fields(&fm)?;
                        let mut oo = Out::new(true);
                        oo.str(&m.id);
                        results.push(oo.s);
                    } else if let Some(u) = t.get("update") {
                        let id = s(u.get(0));
                        let patch = u.get(1).and_then(Value::as_object).cloned().unwrap_or_default();
                        let touch = !matches!(u.get(2).and_then(|x| x.get("touch")), Some(Value::Bool(false)));
                        let m = tx.update(&id, &patch, touch);
                        let mut oo = Out::new(true);
                        match m {
                            Some(m) => write_markup(&mut oo, &m),
                            None => oo.raw("null"),
                        }
                        results.push(oo.s);
                    } else if let Some(r) = t.get("remove") {
                        tx.remove(&s(Some(r)));
                        results.push("null".into());
                    } else if let Some(c) = t.get("cal") {
                        let sheet = s(c.get(0));
                        let cal = c.get(1).filter(|v| v.is_object()).and_then(cal_of);
                        tx.set_calibration(&sheet, cal);
                        results.push("null".into());
                    } else if let Some(n) = t.get("punch") {
                        tx.set_punch_next(crate::js::to_number(n));
                        results.push("null".into());
                    } else if let Some(n) = t.get("name") {
                        let sheet = s(n.get(0));
                        let nm = n.get(1).and_then(sheet_name);
                        tx.set_sheet_name(&sheet, nm);
                        results.push("null".into());
                    } else {
                        return Err(StoreError::Type("unknown tx op".into()));
                    }
                }
                Ok(())
            });
            if let Err(e) = res {
                err = Some(e.to_string());
            }
            o.ch('[');
            o.raw(&results.join(","));
            o.ch(']');
        }
        "undo" | "redo" => {
            state = true;
            let r = if op == "undo" { st.undo() } else { st.redo() };
            match r {
                Some(l) => o.str(&l),
                None => o.raw("null"),
            }
        }
        "load" => {
            state = true;
            let ms: Vec<Markup> =
                step.get("markups").and_then(Value::as_array).map(|a| a.iter().map(markup_from_value).collect()).unwrap_or_default();
            let cals = step
                .get("calibrations")
                .and_then(Value::as_object)
                .map(|m| m.iter().filter_map(|(k, v)| cal_of(v).map(|c| (k.clone(), c))).collect())
                .unwrap_or_default();
            let names = step
                .get("sheetNames")
                .and_then(Value::as_object)
                .map(|m| m.iter().filter_map(|(k, v)| sheet_name(v).map(|n| (k.clone(), n))).collect())
                .unwrap_or_default();
            st.load(ms, cals, names, step.get("layers"), step.get("columns"));
            o.raw("null");
        }
        "dropSheets" => {
            state = true;
            st.drop_sheets(&s(step.get("prefix")));
            o.raw("null");
        }
        "setLayer" => {
            state = true;
            let p = step.get("patch").and_then(Value::as_object);
            let flag = |k: &str| p.and_then(|p| p.get(k)).map(|v| crate::js::truthy(Some(v)));
            st.set_layer(&s(step.get("name")), flag("hidden"), flag("locked"));
            o.raw("null");
        }
        "dropLayer" => {
            state = true;
            st.drop_layer(&s(step.get("name")));
            o.raw("null");
        }
        "setColumns" => {
            state = true;
            st.set_columns(step.get("cols"));
            o.raw("null");
        }
        "state" => {
            state = true;
            o.raw("null");
        }
        "shown" | "open" => match step.get("id").and_then(Value::as_str).and_then(|id| st.get(id)) {
            Some(m) => o.bool(if op == "shown" { st.shown(m) } else { st.open(m) }),
            None => o.raw("null"),
        },
        "makeMarkup" => {
            let f = step.get("fields").and_then(Value::as_object).cloned().unwrap_or_default();
            let m = make_markup(&mut st.env, &f);
            write_markup(o, &m);
        }
        "geometryOk" => match markup_arg(st, step) {
            Some(m) => o.bool(geometry_ok(&m)),
            None => o.raw("null"),
        },
        "cleanReply" => match clean_reply(&mut st.env, step.get("r").unwrap_or(&Value::Null)) {
            Some(r) => crate::json::write_reply(o, &r),
            None => o.raw("null"),
        },
        "statusReply" => {
            let r = status_reply(&mut st.env, &s(step.get("status")), &s(step.get("author")));
            match r {
                Some(r) => crate::json::write_reply(o, &r),
                None => o.raw("null"),
            }
        }
        "shapes" => match markup_arg(st, step) {
            Some(m) => write_list(o, &shapes(&m, &shape_env(step.get("env")))),
            None => o.raw("null"),
        },
        "bounds" => match markup_arg(st, step).and_then(|m| bounds_of(&m, &shape_env(step.get("env")))) {
            Some(b) => o.nums(&b),
            None => o.raw("null"),
        },
        "grips" => match markup_arg(st, step) {
            Some(m) => {
                o.ch('[');
                for (i, g) in grips(&m).iter().enumerate() {
                    if i > 0 {
                        o.ch(',');
                    }
                    o.raw("{\"key\":");
                    o.str(&g.key);
                    o.raw(",\"kind\":");
                    o.str(g.kind);
                    if let Some(k) = g.i {
                        o.raw(",\"i\":");
                        o.num(k as f64);
                    }
                    o.raw(",\"x\":");
                    o.num(g.x);
                    o.raw(",\"y\":");
                    o.num(g.y);
                    o.ch('}');
                }
                o.ch(']');
            }
            None => o.raw("null"),
        },
        "applyGrip" => match markup_arg(st, step) {
            Some(m) => {
                let key = s(step.get("key"));
                match grips(&m).into_iter().find(|g| g.key == key) {
                    Some(g) => write_grip_patch(o, &apply_grip(&m, &g, p2(step.get("p")), crate::js::truthy(step.get("shift")))),
                    None => o.raw("null"),
                }
            }
            None => o.raw("null"),
        },
        "moveBy" => match markup_arg(st, step) {
            Some(m) => o.value(&Value::Object(move_by(&m, num(step.get("dx")), num(step.get("dy"))))),
            None => o.raw("null"),
        },
        "pick" | "pickWindow" => {
            let sheet = s(step.get("sheet"));
            let list: Vec<&Markup> = st.on_sheet(&sheet);
            let env = shape_env(step.get("env"));
            let env_of = |_: &Markup| env.clone();
            if op == "pick" {
                match pick(&list, p2(step.get("p")), num(step.get("tol")), &env_of) {
                    Some(m) => o.str(&m.id),
                    None => o.raw("null"),
                }
            } else {
                let r = step.get("rect").map(crate::model::rect_of).unwrap_or([f64::NAN; 4]);
                let ids: Vec<String> =
                    pick_window(&list, r, crate::js::truthy(step.get("crossing")), &env_of).iter().map(|m| m.id.clone()).collect();
                o.strs(&ids);
            }
        }
        "simplify" => {
            let pts = step.get("pts").map(crate::model::pts_of).unwrap_or_default();
            o.value(&pts_value(&simplify(&pts, num(step.get("tol")))));
        }
        "similarity" => {
            let r = similarity(p2(step.get("a1")), p2(step.get("a2")), p2(step.get("b1")), p2(step.get("b2")));
            match r {
                Some(t) => o.nums(&t),
                None => o.raw("null"),
            }
        }
        "makeClip" => {
            let ids: Vec<String> =
                step.get("ids").and_then(Value::as_array).map(|a| a.iter().map(|v| s(Some(v))).collect()).unwrap_or_default();
            let ms: Vec<&Markup> = ids.iter().filter_map(|id| st.get(id)).collect();
            let c = make_clip(&st.env, &ms, &s(step.get("source")), &s(step.get("sheet")));
            write_clip(o, c.as_ref());
        }
        "cleanClip" => {
            let raw = step.get("raw").unwrap_or(&Value::Null);
            let c = match raw {
                Value::String(t) => clean_clip_text(&mut st.env, t),
                v => clean_clip(&mut st.env, v),
            };
            write_clip(o, c.as_ref());
        }
        "placeClip" => {
            let clip = step.get("clip").and_then(clip_from_value);
            let op_ = step.get("opts").and_then(Value::as_object);
            let g = |k: &str| op_.and_then(|x| x.get(k));
            let opts = PlaceOpts {
                sheet: s(g("sheet")),
                author: s(g("author")),
                rot: g("rot").filter(|v| !v.is_null()).map(crate::js::to_number),
                delta: g("delta").filter(|v| !v.is_null()).map(|v| p2(Some(v))),
                center: g("center").filter(|v| !v.is_null()).map(|v| p2(Some(v))),
            };
            match clip {
                Some(c) => {
                    let ms = place_clip(&mut st.env, &c, &opts);
                    markups_arr(o, &ms.iter().collect::<Vec<_>>());
                }
                None => o.raw("[]"),
            }
        }
        "clipBounds" => {
            let items: Vec<Map<String, Value>> = step
                .get("items")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|i| i.as_object().cloned()).collect())
                .unwrap_or_default();
            match clip_bounds(&items) {
                Some(b) => o.nums(&b),
                None => o.raw("null"),
            }
        }
        "csvMarkups" | "csvPunch" => {
            let ids: Option<Vec<String>> = step.get("ids").and_then(Value::as_array).map(|a| a.iter().map(|v| s(Some(v))).collect());
            let ms: Vec<&Markup> = match &ids {
                Some(ids) => ids.iter().filter_map(|id| st.get(id)).collect(),
                None => st.all(),
            };
            let sh = step.get("sheet").and_then(Value::as_object).map(|x| SheetRef {
                title: s(x.get("title")),
                file: s(x.get("file")),
                page: x.get("page").map(|p| match p {
                    Value::Number(n) => Cell::N(n.as_f64().unwrap_or(f64::NAN)),
                    v => Cell::S(s(Some(v))),
                }),
            });
            let sheet_of = |_: &Markup| sh.clone();
            let meas = step.get("measure").and_then(Value::as_object).cloned().unwrap_or_default();
            let measure_of = |m: &Markup| s(meas.get(&m.id));
            let cols: Vec<CsvColumn> = step
                .get("columns")
                .and_then(Value::as_array)
                .map(|a| a.iter().map(|c| CsvColumn { key: s(c.get("key")), name: s(c.get("name")) }).collect())
                .unwrap_or_default();
            let rows = if op == "csvMarkups" { markup_rows(&ms, &sheet_of, &measure_of, None, &cols) } else { punch_rows(&ms, &sheet_of) };
            o.raw("{\"rows\":");
            o.value(&cells_value(&rows));
            o.raw(",\"csv\":");
            o.str(&to_csv(&rows));
            o.ch('}');
        }
        "cleanTool" => {
            let i = step.get("i").and_then(Value::as_u64).unwrap_or(0);
            match clean_tool(&st.env, step.get("t").unwrap_or(&Value::Null), usize::try_from(i).unwrap_or(0)) {
                Some(t) => o.value(&t),
                None => o.raw("null"),
            }
        }
        "defaultChest" => o.value(&Value::Array(default_chest())),
        "parseChest" => match parse_chest(&st.env, &s(step.get("text"))) {
            Ok((tools, skipped)) => {
                o.raw("{\"tools\":");
                o.value(&Value::Array(tools));
                o.raw(",\"skipped\":");
                o.num(skipped as f64);
                o.ch('}');
            }
            Err(e) => {
                o.raw("null");
                err = Some(e.0);
            }
        },
        "exportChest" => {
            let tools = step.get("tools").and_then(Value::as_array).cloned().unwrap_or_default();
            o.str(&export_chest(&st.env, &tools));
        }
        "mergeChest" => {
            let a = step.get("chest").and_then(Value::as_array).cloned().unwrap_or_default();
            let b = step.get("incoming").and_then(Value::as_array).cloned().unwrap_or_default();
            o.value(&Value::Array(merge_chest(&a, &b)));
        }
        "toolFromMarkup" => match markup_arg(st, step).and_then(|m| tool_from_markup(&st.env, &m, &s(step.get("name")))) {
            Some(t) => o.value(&t),
            None => o.raw("null"),
        },
        "parseLength" => match parse_length(&s(step.get("text")), step.get("unit").and_then(Value::as_str).unwrap_or("ft-in")) {
            Some(v) => o.num(v),
            None => o.raw("null"),
        },
        "formatLength" => o.str(&format_length(num(step.get("mm")), step.get("unit").and_then(Value::as_str).unwrap_or("ft-in"))),
        "formatArea" => o.str(&format_area(num(step.get("mm2")), step.get("unit").and_then(Value::as_str).unwrap_or("ft-in"))),
        "measure" => {
            let cal = step.get("cal").filter(|c| c.is_object()).and_then(cal_of);
            o.str(&measure(num(step.get("len")), cal.as_ref()));
        }
        "measureArea" => {
            let cal = step.get("cal").filter(|c| c.is_object()).and_then(cal_of);
            let pts = step.get("pts").map(crate::model::pts_of).unwrap_or_default();
            let holes: Vec<Vec<[f64; 2]>> =
                step.get("holes").and_then(Value::as_array).map(|h| h.iter().map(crate::model::pts_of).collect()).unwrap_or_default();
            let (a, p) = measure_area(&pts, cal.as_ref(), &holes);
            o.raw("{\"area\":");
            o.str(&a);
            o.raw(",\"perimeter\":");
            o.str(&p);
            o.ch('}');
        }
        "calibrationFrom" => match calibration_from(num(step.get("len")), num(step.get("mm")), &s(step.get("unit"))) {
            Some(c) => write_cal(o, &c),
            None => o.raw("null"),
        },
        "presetCalibration" => write_cal(o, &preset_calibration(&s(step.get("label")), num(step.get("ratio")), &s(step.get("unit")))),
        "validCalibration" => o.bool(valid_calibration_json(step.get("cal").unwrap_or(&Value::Null))),
        "textWidth" => o.num(text_width(&s(step.get("s")), num(step.get("size")), crate::js::truthy(step.get("bold")))),
        "wrapText" => {
            o.strs(&wrap_text(&s(step.get("text")), num(step.get("size")), num(step.get("max")), crate::js::truthy(step.get("bold"))))
        }
        "winAnsiHex" => {
            let (h, m) = win_ansi_hex(&s(step.get("s")));
            o.raw("{\"hex\":");
            o.str(&h);
            o.raw(",\"missing\":");
            o.num(m as f64);
            o.ch('}');
        }
        "arcThrough" => match crate::arcs::arc_through(p2(step.get("a")), p2(step.get("m")), p2(step.get("b"))) {
            Some(a) => {
                o.raw("{\"c\":");
                o.nums(&a.c);
                o.raw(",\"r\":");
                o.num(a.r);
                o.raw(",\"a0\":");
                o.num(a.a0);
                o.raw(",\"sweep\":");
                o.num(a.sweep);
                o.ch('}');
            }
            None => o.raw("null"),
        },
        "angleAt" => o.num(crate::arcs::angle_at(p2(step.get("a")), p2(step.get("v")), p2(step.get("b")))),
        "formatAngle" => o.str(&crate::arcs::format_angle(num(step.get("deg")), 1)),
        "readTitleBlock" => {
            let items: Vec<TextPiece> = step
                .get("items")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .map(|it| {
                            let tr = it.get("transform").and_then(Value::as_array);
                            let g = |i: usize| tr.and_then(|t| t.get(i)).map_or(f64::NAN, crate::js::to_number);
                            TextPiece {
                                str: s(it.get("str")),
                                transform: [g(0), g(1), g(2), g(3), g(4), g(5)],
                                width: num(it.get("width")),
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            let vp = step.get("viewport");
            let tr = vp.and_then(|v| v.get("transform")).and_then(Value::as_array);
            let g = |i: usize| tr.and_then(|t| t.get(i)).map_or(f64::NAN, crate::js::to_number);
            let v = Viewport {
                transform: [g(0), g(1), g(2), g(3), g(4), g(5)],
                width: num(vp.and_then(|v| v.get("width"))),
                height: num(vp.and_then(|v| v.get("height"))),
            };
            match read_title_block(&items, &v) {
                Some(tb) => {
                    o.raw("{\"number\":");
                    o.str(&tb.number);
                    o.raw(",\"title\":");
                    o.str(&tb.title);
                    o.ch('}');
                }
                None => o.raw("null"),
            }
        }
        "isWording" => o.bool(is_wording(&s(step.get("text")))),
        "bcf" => {
            let topics_v = step.get("topics").and_then(Value::as_array).cloned().unwrap_or_default();
            let ms: Vec<Markup> = topics_v
                .iter()
                .map(|t| t.get("markup").map(markup_from_value).unwrap_or_else(|| markup_from_value(&Value::Null)))
                .collect();
            let topics: Vec<Topic<'_>> = topics_v
                .iter()
                .zip(ms.iter())
                .map(|(t, m)| Topic {
                    markup: m,
                    sheet_title: s(t.get("sheetTitle")),
                    file_name: s(t.get("fileName")),
                    value: s(t.get("value")),
                    bbox: t.get("box").filter(|b| b.is_array()).map(crate::model::rect_of),
                    png: t.get("png").and_then(Value::as_str).map(crate::imagestamp::b64_decode),
                })
                .collect();
            let exported = crate::js::parse_date(&s(step.get("exported")));
            let (bytes, count) =
                build_bcf(&topics, step.get("project").and_then(Value::as_str).unwrap_or("MarkupCraft"), exported, st.env.now_ms);
            o.raw("{\"bytes\":{\"$base64\":");
            o.str(&crate::imagestamp::b64_encode(&bytes));
            o.raw("},\"count\":");
            o.num(count as f64);
            o.ch('}');
        }
        "combine" => {
            let bytes = |k: &str| -> Vec<u8> {
                step.get(k)
                    .and_then(Value::as_array)
                    .map(|a| a.iter().map(|v| crate::compare::clamp_u8(crate::js::to_number(v))).collect())
                    .unwrap_or_default()
            };
            let (a, b) = (bytes("a"), bytes("b"));
            let mut out =
                vec![0u8; step.get("n").and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok()).unwrap_or(a.len()).min(64 << 20)];
            let c = step.get("colors");
            let col = |k: &str| -> [f64; 3] {
                let a = c.and_then(|c| c.get(k)).and_then(Value::as_array);
                let g = |i: usize| a.and_then(|a| a.get(i)).map_or(f64::NAN, crate::js::to_number);
                [g(0), g(1), g(2)]
            };
            let colors = crate::compare::MixColors { bg: col("bg"), same: col("same"), old: col("old"), new: col("new") };
            let bal = step.get("balance").map_or(0.5, crate::js::to_number);
            let stats = crate::compare::combine(&a, &b, &mut out, &colors, bal);
            o.raw("{\"out\":");
            o.nums(&out.iter().map(|v| f64::from(*v)).collect::<Vec<_>>());
            o.raw(",\"stats\":{\"changedOld\":");
            o.num(stats.changed_old as f64);
            o.raw(",\"changedNew\":");
            o.num(stats.changed_new as f64);
            o.raw(",\"same\":");
            o.num(stats.same as f64);
            o.raw("}}");
        }
        "points" => {
            // a plain echo used by the harness to check number text
            o.value(&nums_value(
                &step
                    .get("xs")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().map(crate::js::to_number).collect::<Vec<_>>())
                    .unwrap_or_default(),
            ));
        }
        _ => match crate::script2::run_ext(st, op, step, o) {
            Some(Ok(())) => {}
            Some(Err(e)) => {
                o.raw("null");
                err = Some(e);
            }
            None => {
                o.raw("null");
                err = Some(format!("unknown op {op}"));
            }
        },
    }
    if state {
        o.raw(",\"s\":");
        write_state(o, st);
    }
    if let Some(e) = err {
        o.raw(",\"e\":");
        o.str(&e);
    }
    o.ch('}');
}
