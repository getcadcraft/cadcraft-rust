//! The part of pdf.js's operator list vecdiff.js reads (`page.getOperatorList()` with no optimizer, as the API's
//! OPLIST intent runs it): q / Q / cm, form XObjects (`paintFormXObjectBegin` with the form's /Matrix as 32-bit
//! floats) and `constructPath` with the path data pdf.js's `buildPath` writes (re → moveTo + 3 lineTo + close, or
//! moveTo + lineTo + close for an empty rectangle; v / y → curveTo; the data stored in a Float32Array) — flushed by
//! every painting operator and by `n`, a new path buffer for each form. Spec gap: annotation appearance streams (which
//! pdf.js adds to the list with their own transforms) are not included; MarkupCraft compares page content.

use crate::jsdoc::JsDoc;
use crate::obj::{Dict, Lexer, Obj};
use cc_markup::vecdiff::{CLOSE, CURVE_TO, LINE_TO, MOVE_TO, Op};

/// Most operators read from one page (forms included).
const BUDGET: usize = 5_000_000;

struct State {
    cx: f64,
    cy: f64,
}

struct Ctx<'a> {
    doc: &'a JsDoc,
    ops: Vec<Op>,
    budget: usize,
}

fn f32r(x: f64) -> f64 {
    #[allow(clippy::cast_possible_truncation)]
    let v = x as f32;
    f64::from(v)
}

fn run(ctx: &mut Ctx<'_>, data: &[u8], resources: &Dict, st0: State, depth: usize, seen: &mut Vec<(u32, u16)>) {
    let mut lx = Lexer::pdfjs(data);
    let mut args: Vec<Obj> = Vec::new();
    let mut stack: Vec<State> = Vec::new();
    let mut st = st0;
    let mut path: Vec<f64> = Vec::new();
    while !lx.done() {
        if ctx.budget == 0 {
            return;
        }
        ctx.budget = ctx.budget.saturating_sub(1);
        lx.skip_ws_comments();
        let start = lx.pos;
        if let Some(o) = lx.object(0) {
            if args.len() < 64 {
                args.push(o);
            }
            continue;
        }
        lx.pos = start;
        let mut kw = Vec::new();
        while let Some(c) = lx.peek() {
            if crate::obj::is_ws(c) || crate::obj::is_delim(c) {
                break;
            }
            kw.push(c);
            lx.pos = lx.pos.saturating_add(1);
        }
        if kw.is_empty() {
            lx.pos = lx.pos.saturating_add(1);
            args.clear();
            continue;
        }
        // the last N operands (pdf.js moves extra leading operands aside)
        let take = |n: usize| -> Option<Vec<f64>> {
            let k = args.len().checked_sub(n)?;
            Some(args.get(k..).unwrap_or(&[]).iter().map(|a| a.as_num().unwrap_or(f64::NAN)).collect())
        };
        let g = |v: &[f64], i: usize| v.get(i).copied().unwrap_or(f64::NAN);
        match kw.as_slice() {
            b"q" => {
                stack.push(State { cx: st.cx, cy: st.cy });
                ctx.ops.push(Op::Save);
            }
            b"Q" => {
                if let Some(s) = stack.pop() {
                    st = s;
                }
                ctx.ops.push(Op::Restore);
            }
            b"cm" => {
                if let Some(v) = take(6) {
                    ctx.ops.push(Op::Transform([g(&v, 0), g(&v, 1), g(&v, 2), g(&v, 3), g(&v, 4), g(&v, 5)]));
                }
            }
            b"m" => {
                if let Some(v) = take(2) {
                    st.cx = g(&v, 0);
                    st.cy = g(&v, 1);
                    path.extend_from_slice(&[MOVE_TO, st.cx, st.cy]);
                }
            }
            b"l" => {
                if let Some(v) = take(2) {
                    st.cx = g(&v, 0);
                    st.cy = g(&v, 1);
                    path.extend_from_slice(&[LINE_TO, st.cx, st.cy]);
                }
            }
            b"c" => {
                if let Some(v) = take(6) {
                    path.push(CURVE_TO);
                    path.extend_from_slice(&v);
                    st.cx = g(&v, 4);
                    st.cy = g(&v, 5);
                }
            }
            b"v" => {
                if let Some(v) = take(4) {
                    path.extend_from_slice(&[CURVE_TO, st.cx, st.cy, g(&v, 0), g(&v, 1), g(&v, 2), g(&v, 3)]);
                    st.cx = g(&v, 2);
                    st.cy = g(&v, 3);
                }
            }
            b"y" => {
                if let Some(v) = take(4) {
                    path.extend_from_slice(&[CURVE_TO, g(&v, 0), g(&v, 1), g(&v, 2), g(&v, 3), g(&v, 2), g(&v, 3)]);
                    st.cx = g(&v, 2);
                    st.cy = g(&v, 3);
                }
            }
            b"h" => path.push(CLOSE),
            b"re" => {
                if let Some(v) = take(4) {
                    let (x, y, w, h) = (g(&v, 0), g(&v, 1), g(&v, 2), g(&v, 3));
                    st.cx = x;
                    st.cy = y;
                    let (xw, yh) = (x + w, y + h);
                    if w == 0.0 || h == 0.0 {
                        path.extend_from_slice(&[MOVE_TO, x, y, LINE_TO, xw, yh, CLOSE]);
                    } else {
                        path.extend_from_slice(&[MOVE_TO, x, y, LINE_TO, xw, y, LINE_TO, xw, yh, LINE_TO, x, yh, CLOSE]);
                    }
                }
            }
            b"S" | b"s" | b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*" | b"n" => {
                if matches!(kw.as_slice(), b"s" | b"b" | b"b*") {
                    path.push(CLOSE);
                }
                if path.is_empty() {
                    ctx.ops.push(Op::Path(None));
                } else {
                    ctx.ops.push(Op::Path(Some(path.iter().map(|x| f32r(*x)).collect())));
                    path.clear();
                }
            }
            b"Do" if depth < 12 => {
                let name = args.last().and_then(Obj::as_name).unwrap_or("").to_string();
                let xo = ctx.doc.dget(resources, "XObject");
                let raw = xo.as_ref().and_then(|x| x.as_dict()).and_then(|x| x.get(&name).cloned());
                let key = raw.as_ref().and_then(Obj::as_ref);
                if key.is_some_and(|k| seen.contains(&k)) {
                    args.clear();
                    continue;
                }
                let x = xo.as_ref().and_then(|x| x.as_dict()).and_then(|x| ctx.doc.dget(x, &name));
                if let Some(Obj::Stream(s)) = x
                    && ctx.doc.dget(&s.dict, "Subtype").as_ref().and_then(Obj::as_name) == Some("Form")
                {
                    let matrix = match ctx.doc.dget(&s.dict, "Matrix") {
                        Some(Obj::Arr(m)) if m.len() == 6 => {
                            let v: Vec<f64> = m.iter().map(|x| ctx.doc.resolve(x).as_num().unwrap_or(f64::NAN)).collect();
                            if v.iter().all(|x| x.is_finite()) {
                                <[f64; 6]>::try_from(v.iter().map(|x| f32r(*x)).collect::<Vec<_>>()).ok()
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };
                    let res = match ctx.doc.dget(&s.dict, "Resources") {
                        Some(Obj::Dict(r)) => r,
                        _ => resources.clone(),
                    };
                    ctx.ops.push(Op::FormBegin(matrix));
                    if let Some(k) = key {
                        seen.push(k);
                    }
                    let data = s.data.clone();
                    run(ctx, &data, &res, State { cx: st.cx, cy: st.cy }, depth.saturating_add(1), seen);
                    if key.is_some() {
                        seen.pop();
                    }
                    ctx.ops.push(Op::FormEnd);
                }
            }
            b"BI" => {
                while !lx.done() {
                    if lx.match_kw(b"EI") && lx.peek().is_none_or(crate::obj::is_ws) {
                        break;
                    }
                    lx.pos = lx.pos.saturating_add(1);
                }
            }
            _ => {}
        }
        args.clear();
    }
    // pdf.js closes the q it is still inside at the end of each content stream
    for _ in 0..stack.len() {
        ctx.ops.push(Op::Restore);
    }
}

/// The operator list of page `index` (the parts [`Op`] holds).
pub fn page_ops(doc: &JsDoc, index: usize) -> Vec<Op> {
    let Some(page) = doc.pages.get(index) else { return Vec::new() };
    let data = doc.content(&page.dict);
    let resources = match doc.inherited(&page.dict, "Resources") {
        Some(Obj::Dict(d)) => d,
        _ => Dict::new(),
    };
    let mut ctx = Ctx { doc, ops: Vec::new(), budget: BUDGET };
    let mut seen = Vec::new();
    run(&mut ctx, &data, &resources, State { cx: 0.0, cy: 0.0 }, 0, &mut seen);
    ctx.ops
}
