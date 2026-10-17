//! What pdf.js `page.getAnnotations({intent: 'any'})` hands MarkupCraft's reader: per annotation the fields
//! `pdf-read.js` uses (subtype, id, rect, color, border width, title, contents, dates, flags, reply fields, review
//! state, line / vertex / ink data, whether it has an appearance), built the way pdf.js builds them — including the
//! rectangle pdf.js recomputes for an annotation that has no appearance stream of its own, and its order (form fields,
//! then pop-ups, last).

use crate::jsdoc::{JsDoc, intersect, normalize_rect, string_to_pdf_string};
use crate::obj::{Dict, Obj};

/// One annotation's data.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnnotData {
    pub subtype: Option<String>,
    pub id: String,
    pub rect: [f64; 4],
    /// None = `null` (a markup annotation without /C, or /C [])
    pub color: Option<[u8; 3]>,
    pub border_width: f64,
    /// `titleObj.str` (None: not a markup annotation, so no titleObj)
    pub title: Option<String>,
    pub contents: String,
    pub modification_date: Option<String>,
    pub creation_date: Option<String>,
    pub flags: f64,
    /// `inReplyTo` present (/IRT): Some(Some(id)) for a reference, Some(None) for anything else (null)
    pub in_reply_to: Option<Option<String>>,
    pub reply_type: Option<String>,
    pub state: Option<String>,
    pub state_model: Option<String>,
    pub line_coordinates: Option<[f64; 4]>,
    pub vertices: Option<Vec<f64>>,
    pub ink_lists: Option<Vec<Vec<f64>>>,
    pub has_appearance: bool,
}

/// `Uint8ClampedArray` store.
fn clamp(v: f64) -> u8 {
    cc_markup::compare::clamp_u8(v)
}

/// `getRgbColor(color, default)`.
fn rgb_color(doc: &JsDoc, c: Option<Vec<Obj>>, default: Option<[u8; 3]>) -> Option<[u8; 3]> {
    let Some(a) = c else { return default };
    let v: Vec<f64> = a.iter().map(|x| doc.resolve(x).as_num().unwrap_or(f64::NAN)).collect();
    match v.as_slice() {
        [] => None,
        [g] => {
            let x = clamp(g * 255.0);
            Some([x, x, x])
        }
        [r, g, b] => Some([clamp(r * 255.0), clamp(g * 255.0), clamp(b * 255.0)]),
        [c, m, y, k] => {
            let (c, m, y, k) = (*c, *m, *y, *k);
            let r = 255.0
                + c * (-4.387332384609988 * c
                    + 54.48615194189176 * m
                    + 18.82290502165302 * y
                    + 212.25662451639585 * k
                    + -285.2331026137004)
                + m * (1.7149763477362134 * m - 5.6096736904047315 * y + -17.873870861415444 * k - 5.497006427196366)
                + y * (-2.5217340131683033 * y - 21.248923337353073 * k + 17.5119270841813)
                + k * (-21.86122147463605 * k - 189.48180835922747);
            let g = 255.0
                + c * (8.841041422036149 * c + 60.118027045597366 * m + 6.871425592049007 * y + 31.159100130055922 * k + -79.2970844816548)
                + m * (-15.310361306967817 * m + 17.575251261109482 * y + 131.35250912493976 * k - 190.9453302588951)
                + y * (4.444339102852739 * y + 9.8632861493405 * k - 24.86741582555878)
                + k * (-20.737325471181034 * k - 187.80453709719578);
            let b = 255.0
                + c * (0.8842522430003296 * c + 8.078677503112928 * m + 30.89978309703729 * y - 0.23883238689178934 * k
                    + -14.183576799673286)
                + m * (10.49593273432072 * m + 63.02378494754052 * y + 50.606957656360734 * k - 112.23884253719248)
                + y * (0.03296041114873217 * y + 115.60384449646641 * k + -193.58209356861505)
                + k * (-22.33816807309886 * k - 180.12613974708367);
            Some([clamp(r), clamp(g), clamp(b)])
        }
        _ => default,
    }
}

fn f32r(x: f64) -> f64 {
    #[allow(clippy::cast_possible_truncation)]
    let f = x as f32;
    f64::from(f)
}

fn num_array(doc: &JsDoc, a: &[Obj]) -> Option<Vec<f64>> {
    a.iter().map(|x| doc.resolve(x).as_num()).collect()
}

fn text_of(o: Option<Obj>) -> String {
    match o {
        Some(x) if x.is_string() => string_to_pdf_string(&x.str_bytes().unwrap_or_default()),
        _ => String::new(),
    }
}
fn date_of(o: Option<Obj>) -> Option<String> {
    match o {
        Some(x) if x.is_string() => Some(x.str_bytes().unwrap_or_default().iter().map(|c| char::from(*c)).collect()),
        _ => None,
    }
}

const MARKUP: [&str; 15] = [
    "Text",
    "FreeText",
    "Line",
    "Square",
    "Circle",
    "PolyLine",
    "Polygon",
    "Caret",
    "Ink",
    "Highlight",
    "Underline",
    "Squiggly",
    "StrikeOut",
    "Stamp",
    "FileAttachment",
];
const KNOWN: [&str; 23] = [
    "Link",
    "Text",
    "Widget",
    "Popup",
    "FreeText",
    "Line",
    "Square",
    "Circle",
    "PolyLine",
    "Polygon",
    "Caret",
    "Ink",
    "Highlight",
    "Underline",
    "Squiggly",
    "StrikeOut",
    "Stamp",
    "FileAttachment",
    "RichMedia",
    "Screen",
    "Sound",
    "Movie",
    "3D",
];

/// The rect of a bounding box over Float32 points (BBOX_INIT then rectBoundingBox).
fn bbox_of(pts: &[[f64; 4]]) -> [f64; 4] {
    use cc_markup::js::{max, min};
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for [x0, y0, x1, y1] in pts {
        b[0] = min(min(b[0], *x0), *x1);
        b[1] = min(min(b[1], *y0), *y1);
        b[2] = max(max(b[2], *x0), *x1);
        b[3] = max(max(b[3], *y0), *y1);
    }
    b
}

/// The appearance points pdf.js uses: quad points, else the rectangle as Float32.
fn rect_points(r: [f64; 4]) -> [f64; 8] {
    [f32r(r[0]), f32r(r[3]), f32r(r[2]), f32r(r[3]), f32r(r[0]), f32r(r[1]), f32r(r[2]), f32r(r[1])]
}

/// One annotation dictionary → its data (None where pdf.js gives none: not a dictionary, or creation failed).
pub fn annot_data(doc: &JsDoc, dict: &Dict, id: String) -> Option<AnnotData> {
    let subtype = match doc.dget(dict, "Subtype") {
        Some(Obj::Name(n)) => Some(n),
        _ => None,
    };
    let sub = subtype.clone().unwrap_or_default();
    let is_markup = MARKUP.contains(&sub.as_str());
    let generic = !KNOWN.contains(&sub.as_str());
    let mut a = AnnotData { subtype: subtype.clone(), id, ..Default::default() };
    let title = text_of(doc.dget(dict, "T"));
    a.contents = text_of(doc.dget(dict, "Contents"));
    a.modification_date = date_of(doc.dget(dict, "M"));
    let fl = doc.dget(dict, "F").and_then(|f| f.as_num()).unwrap_or(0.0);
    let mut flags = if fl.fract() == 0.0 && fl > 0.0 { fl } else { 0.0 };
    if !generic && cc_markup::js::f2u(flags) & 1 != 0 {
        flags -= 1.0;
    }
    a.flags = flags;
    let rect = doc
        .dget_array(dict, "Rect")
        .filter(|r| r.len() == 4)
        .and_then(|r| num_array(doc, &r))
        .and_then(|v| <[f64; 4]>::try_from(v).ok())
        .map_or([0.0; 4], normalize_rect);
    let mut rectangle = rect;
    a.rect = rect;
    let mut color = rgb_color(doc, doc.dget_array(dict, "C"), Some([0, 0, 0]));
    // border style
    let mut width = 1.0;
    if dict.has("BS") {
        if let Some(Obj::Dict(bs)) = doc.dget(dict, "BS") {
            let ty = doc.dget(&bs, "Type");
            if ty.is_none() || ty.as_ref().and_then(Obj::as_name) == Some("Border") {
                match doc.dget(&bs, "W") {
                    Some(Obj::Name(_)) => width = 0.0,
                    Some(Obj::Num(w)) => width = border_width(w, rect),
                    _ => {}
                }
            }
        }
    } else if dict.has("Border") {
        if let Some(arr) = doc.dget_array(dict, "Border") {
            if arr.len() >= 3 {
                match arr.get(2) {
                    Some(Obj::Name(_)) => width = 0.0,
                    Some(Obj::Num(w)) => width = border_width(*w, rect),
                    _ => {}
                }
            } else if arr.is_empty() {
                width = 0.0;
            }
        }
    } else {
        width = 0.0;
    }
    a.border_width = width;
    // appearance
    let mut has_ap = false;
    if let Some(Obj::Dict(ap)) = doc.dget(dict, "AP") {
        match doc.dget(&ap, "N") {
            Some(Obj::Stream(_)) => has_ap = true,
            Some(Obj::Dict(states)) => {
                if let Some(Obj::Name(st)) = doc.dget(dict, "AS") {
                    has_ap = matches!(doc.dget(&states, &st), Some(Obj::Stream(_)));
                }
            }
            _ => {}
        }
    }
    a.has_appearance = has_ap;
    if is_markup {
        if dict.has("IRT") {
            a.in_reply_to = Some(match dict.get("IRT") {
                Some(Obj::Ref(n, g)) => Some(crate::refkey(*n, *g)),
                _ => None,
            });
            a.reply_type = Some(match doc.dget(dict, "RT") {
                Some(Obj::Name(n)) => n,
                _ => "R".into(),
            });
        }
        if a.reply_type.as_deref() == Some("Group") {
            let Some(Obj::Dict(parent)) = doc.dget(dict, "IRT") else { return None };
            a.title = Some(text_of(doc.dget(&parent, "T")));
            a.contents = text_of(doc.dget(&parent, "Contents"));
            a.creation_date = if parent.has("CreationDate") { date_of(doc.dget(&parent, "CreationDate")) } else { None };
            a.modification_date = if parent.has("M") { date_of(doc.dget(&parent, "M")) } else { None };
            color = if parent.has("C") { rgb_color(doc, doc.dget_array(&parent, "C"), Some([0, 0, 0])) } else { None };
        } else {
            a.title = Some(title);
            a.creation_date = date_of(doc.dget(dict, "CreationDate"));
            if !dict.has("C") {
                color = None;
            }
        }
    }
    a.color = color;
    let stroke_bw = if width == 0.0 || width.is_nan() { 1.0 } else { width };
    match sub.as_str() {
        "Text" => {
            if !has_ap {
                a.rect[1] = a.rect[3] - 22.0;
                a.rect[2] = a.rect[0] + 22.0;
            }
            if dict.has("State") {
                a.state = doc.dget(dict, "State").and_then(|s| state_str(&s));
                a.state_model = doc.dget(dict, "StateModel").and_then(|s| state_str(&s));
            }
        }
        "Line" => {
            let lc = doc
                .dget_array(dict, "L")
                .filter(|l| l.len() == 4)
                .and_then(|l| num_array(doc, &l))
                .and_then(|v| <[f64; 4]>::try_from(v).ok())
                .unwrap_or([0.0; 4]);
            let lc = normalize_rect(lc);
            a.line_coordinates = Some(lc);
            if !has_ap {
                let adj = 2.0 * stroke_bw;
                let bbox = [lc[0] - adj, lc[1] - adj, lc[2] + adj, lc[3] + adj];
                if intersect(rectangle, bbox).is_none() {
                    rectangle = bbox;
                }
                let p = rect_points(rectangle);
                a.rect = bbox_of(&[[p[0] - stroke_bw, p[7] - stroke_bw, p[2] + stroke_bw, p[3] + stroke_bw]]);
            }
        }
        "Square" | "Circle" => {
            if !has_ap {
                let fill = rgb_color(doc, doc.dget_array(dict, "IC"), None);
                if !(width == 0.0 && fill.is_none()) {
                    let p = rect_points(rectangle);
                    a.rect = bbox_of(&[[p[0], p[7], p[2], p[3]]]);
                }
            }
        }
        "PolyLine" | "Polygon" => {
            let raw = doc.dget_array(dict, "Vertices").and_then(|v| num_array(doc, &v));
            if let Some(raw) = raw {
                let v: Vec<f64> = raw.iter().map(|x| f32r(*x)).collect();
                if !has_ap {
                    let adj = 2.0 * stroke_bw;
                    let mut boxes = Vec::new();
                    for pair in v.chunks(2) {
                        let (x, y) = (pair.first().copied().unwrap_or(f64::NAN), pair.get(1).copied().unwrap_or(f64::NAN));
                        boxes.push([x - adj, y - adj, x + adj, y + adj]);
                    }
                    let bbox = bbox_of(&boxes);
                    if intersect(rectangle, bbox).is_none() {
                        rectangle = bbox;
                    }
                    let p = rect_points(rectangle);
                    a.rect = bbox_of(&[[p[0], p[7], p[2], p[3]]]);
                }
                a.vertices = Some(v);
            }
        }
        "Ink" => {
            if let Some(lists) = doc.dget_array(dict, "InkList") {
                let mut ink: Vec<Vec<f64>> = Vec::new();
                for l in lists {
                    let Obj::Arr(l) = l else { continue };
                    let mut out = vec![0.0f64; l.len()];
                    let mut j = 0usize;
                    while j < l.len() {
                        let x = l.get(j).map(|v| doc.resolve(v));
                        let y = l.get(j.saturating_add(1)).map(|v| doc.resolve(v));
                        if let (Some(Obj::Num(x)), Some(Obj::Num(y))) = (x, y) {
                            if let Some(s) = out.get_mut(j) {
                                *s = f32r(x);
                            }
                            if let Some(s) = out.get_mut(j.saturating_add(1)) {
                                *s = f32r(y);
                            }
                        }
                        j = j.saturating_add(2);
                    }
                    ink.push(out);
                }
                if !has_ap {
                    let adj = 2.0 * stroke_bw;
                    let mut boxes = Vec::new();
                    for l in &ink {
                        for pair in l.chunks(2) {
                            let (x, y) = (pair.first().copied().unwrap_or(f64::NAN), pair.get(1).copied().unwrap_or(f64::NAN));
                            boxes.push([x - adj, y - adj, x + adj, y + adj]);
                        }
                    }
                    let bbox = bbox_of(&boxes);
                    if intersect(rectangle, bbox).is_none() {
                        rectangle = bbox;
                    }
                    let p = rect_points(rectangle);
                    a.rect = bbox_of(&[[p[0], p[7], p[2], p[3]]]);
                }
                a.ink_lists = Some(ink);
            } else {
                a.ink_lists = Some(Vec::new());
            }
        }
        "Highlight" => {
            if !has_ap && let Some(q) = quad_points(doc, dict) {
                let mut boxes = Vec::new();
                for c in q.chunks(8) {
                    let g = |i: usize| c.get(i).copied().unwrap_or(f64::NAN);
                    boxes.push([g(0), g(7), g(2), g(3)]);
                }
                a.rect = bbox_of(&boxes);
            }
        }
        _ => {}
    }
    Some(a)
}

fn state_str(o: &Obj) -> Option<String> {
    // `dict.get("State") || null`: a string comes over as text; other truthy values are not used by the reader
    match o {
        x if x.is_string() => {
            let s: String = x.str_bytes().unwrap_or_default().iter().map(|c| char::from(*c)).collect();
            if s.is_empty() { None } else { Some(s) }
        }
        Obj::Name(n) => Some(n.clone()),
        _ => None,
    }
}

/// `getQuadPoints(dict, null)` → Float32 [minX, maxY, maxX, maxY, minX, minY, maxX, minY] per quad.
fn quad_points(doc: &JsDoc, dict: &Dict) -> Option<Vec<f64>> {
    use cc_markup::js::{max, min};
    let q = doc.dget_array(dict, "QuadPoints").and_then(|v| num_array(doc, &v))?;
    if q.is_empty() || q.len() % 8 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(q.len());
    for c in q.chunks(8) {
        let g = |i: usize| c.get(i).copied().unwrap_or(f64::NAN);
        let min_x = min(min(min(g(0), g(2)), g(4)), g(6));
        let max_x = max(max(max(g(0), g(2)), g(4)), g(6));
        let min_y = min(min(min(g(1), g(3)), g(5)), g(7));
        let max_y = max(max(max(g(1), g(3)), g(5)), g(7));
        out.extend([min_x, max_y, max_x, max_y, min_x, min_y, max_x, min_y].map(f32r));
    }
    Some(out)
}

fn border_width(w: f64, rect: [f64; 4]) -> f64 {
    if w > 0.0 {
        let mw = (rect[2] - rect[0]) / 2.0;
        let mh = (rect[3] - rect[1]) / 2.0;
        if mw > 0.0 && mh > 0.0 && (w > mw || w > mh) {
            return 1.0;
        }
    }
    w
}

/// Every annotation of a page in pdf.js's order (others, then widgets, then pop-ups).
pub fn page_annotations(doc: &JsDoc, page_index: usize) -> Vec<AnnotData> {
    let Some(page) = doc.pages.get(page_index) else { return Vec::new() };
    let annots = match doc.inherited(&page.dict, "Annots") {
        Some(Obj::Arr(a)) => a,
        _ => return Vec::new(),
    };
    let (mut main, mut widgets, mut popups) = (Vec::new(), Vec::new(), Vec::new());
    let mut direct = 0usize;
    for item in annots.iter().take(100_000) {
        let (id, d) = match item {
            Obj::Ref(n, g) => (crate::refkey(*n, *g), doc.get((*n, *g))),
            Obj::Dict(d) => {
                direct = direct.saturating_add(1);
                (format!("annot_p{page_index}_{direct}"), Obj::Dict(d.clone()))
            }
            _ => continue,
        };
        let Obj::Dict(dict) = d else { continue };
        let Some(a) = annot_data(doc, &dict, id) else { continue };
        match a.subtype.as_deref() {
            Some("Widget") => widgets.push(a),
            Some("Popup") => popups.push(a),
            _ => main.push(a),
        }
    }
    main.extend(widgets);
    main.extend(popups);
    main
}
