//! walk.js: what a set of entities draws as, for any output (the screen, paper). Blocks, dimensions and
//! multileaders are expanded, colours resolved (by layer, by block), hidden layers skipped, linetypes cut into
//! dashes, and everything put through an optional transform `m`. The output gets strokes (primitives), fills (flat
//! loops), texts (transformed text entities), points, viewports, text masks and construction lines ([`WalkOut`]).
//!
//! Not ported in M4: XCLIP (clipped references draw whole) and pictures (`pic` / `picName`: a picture's frame and
//! name draw as plain linework and text).

use crate::jsmath::JsMath;
use std::borrow::Cow;

use cc_cad::entity::{Color, Entity, Geo, Obj};
use cc_cad::{Doc, Layer, js, num};
use serde_json::Value;

use crate::geom::*;
use crate::hatch::hatch_lines;
use crate::leader::leader_geometry;
use crate::linetypes::{Layers, dash_prims, pattern_of, resolve_lt, resolve_lw};

/// `o` = { alpha, wipe } (absent = opaque).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Opts {
    pub alpha: Option<f64>,
    pub wipe: bool,
}
impl Opts {
    pub fn to_value(o: Option<Opts>) -> Value {
        match o {
            None => Value::Null,
            Some(o) => {
                let mut m = Obj::new();
                if let Some(a) = o.alpha {
                    m.insert("alpha".into(), num(a));
                }
                if o.wipe {
                    m.insert("wipe".into(), true.into());
                }
                Value::Object(m)
            }
        }
    }
}

/// What walk draws into.
pub trait WalkOut {
    fn stroke(&mut self, color: &str, prims: Vec<Prim>, lw: f64, o: Option<Opts>);
    fn fill(&mut self, color: &str, loops: Vec<Vec<f64>>, solid: bool, o: Option<Opts>);
    fn text(&mut self, color: &str, e: &Entity, o: Option<Opts>);
    fn point(&mut self, color: &str, x: f64, y: f64);
    fn viewport(&mut self, _e: &Entity) {}
    /// The walk starts on the top-level entity `index` of the list it was given (what follows, up to the next call,
    /// is that entity's output).
    fn begin(&mut self, _index: usize) {}
    /// Does this output paint text masks itself (`out.mask`)? Else a mask comes as a solid fill.
    fn has_mask(&self) -> bool {
        false
    }
    fn mask(&mut self, _color: &str, _ring: Vec<f64>, _wipe: bool) {}
    /// Does this output clip construction lines itself (`out.infinite`)? Else they come as strokes of the stand-in.
    fn has_infinite(&self) -> bool {
        false
    }
    fn infinite(&mut self, _color: &str, _e: &Entity, _lw: f64, _o: Option<Opts>) {}
}

/// A viewport's layer view (VP.1, VP.3): upper-case names of layers frozen in it, and per-layer overrides
/// { color, lt, lw, tr } by upper-case name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frozen {
    pub names: Vec<String>,
    pub ovr: Option<Vec<(String, Obj)>>,
}

/// `frozenSet(vp)`: None when the viewport shows every layer as the drawing has it.
pub fn frozen_set(vp: &Entity) -> Option<Frozen> {
    let f: Vec<String> = match vp.get("frz") {
        Some(Value::Array(a)) => a.iter().filter_map(|n| n.as_str().filter(|s| !s.is_empty()).map(str::to_string)).collect(),
        _ => Vec::new(),
    };
    let lov: Vec<(String, Obj)> = match vp.get("lov") {
        Some(Value::Object(o)) => js_keys(o)
            .into_iter()
            .filter_map(|k| o.get(k).and_then(Value::as_object).filter(|_| !k.is_empty()).map(|v| (k.clone(), v.clone())))
            .collect(),
        _ => Vec::new(),
    };
    if f.is_empty() && lov.is_empty() {
        return None;
    }
    let mut names: Vec<String> = Vec::new();
    for n in f {
        let u = n.to_uppercase();
        if !names.contains(&u) {
            names.push(u);
        }
    }
    let ovr = if lov.is_empty() {
        None
    } else {
        let mut m: Vec<(String, Obj)> = Vec::new();
        for (k, v) in lov {
            let u = k.to_uppercase();
            match m.iter_mut().find(|(x, _)| *x == u) {
                Some(slot) => slot.1 = v,
                None => m.push((u, v)),
            }
        }
        Some(m)
    };
    Some(Frozen { names, ovr })
}

fn cmp16(a: &str, b: &str) -> std::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// `JSON.stringify` of a JSON value (V8's number and string spelling, keys in JS order).
pub fn js_stringify(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => {
            let x = n.as_f64().unwrap_or(f64::NAN);
            if x.is_finite() { js::num_to_string(x) } else { "null".into() }
        }
        Value::String(s) => {
            let mut o = String::from("\"");
            for c in s.chars() {
                match c {
                    '"' => o.push_str("\\\""),
                    '\\' => o.push_str("\\\\"),
                    '\u{8}' => o.push_str("\\b"),
                    '\u{c}' => o.push_str("\\f"),
                    '\n' => o.push_str("\\n"),
                    '\r' => o.push_str("\\r"),
                    '\t' => o.push_str("\\t"),
                    c if u32::from(c) < 0x20 => o.push_str(&format!("\\u{:04x}", u32::from(c))),
                    c => o.push(c),
                }
            }
            o.push('"');
            o
        }
        Value::Array(a) => format!("[{}]", a.iter().map(js_stringify).collect::<Vec<_>>().join(",")),
        Value::Object(m) => format!(
            "{{{}}}",
            js_keys(m)
                .into_iter()
                .map(|k| format!("{}:{}", js_stringify(&Value::String(k.clone())), m.get(k).map_or_else(|| "null".into(), js_stringify)))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

/// `frozenKey(set)`: the key a cache of the model as one viewport shows it is kept under.
pub fn frozen_key(set: Option<&Frozen>) -> String {
    let Some(s) = set else { return String::new() };
    let mut names = s.names.clone();
    names.sort_by(|a, b| cmp16(a, b));
    let mut out = names.join("\n");
    if let Some(ovr) = &s.ovr {
        let mut list = ovr.clone();
        list.sort_by(
            |a, b| if cmp16(&a.0, &b.0) == std::cmp::Ordering::Less { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater },
        );
        let arr = Value::Array(list.into_iter().map(|(k, v)| Value::Array(vec![Value::String(k), Value::Object(v)])).collect());
        out.push('\u{1}');
        out.push_str(&js_stringify(&arr));
    }
    out
}

/// walk's options: an outer transform, plot (skip layers that do not plot), a viewport's layer view.
#[derive(Clone, Debug, Default)]
pub struct WalkOpts<'a> {
    pub m: Option<M>,
    pub plot: bool,
    pub frozen: Option<&'a Frozen>,
    /// display only (not in the JS): an object whose linetype would make more dashes than this draws continuous
    /// (None = the JS rule: up to 20 000 per run)
    pub max_dashes: Option<f64>,
}

#[derive(Clone, Debug, Default)]
struct BlkCtx {
    lt: Option<String>,
    lw: Option<f64>,
    tr: Option<f64>,
}

struct Walker<'a> {
    doc: &'a Doc,
    layers: Cow<'a, Layers>,
    plot: bool,
    frozen: Option<&'a Frozen>,
    ltscale: f64,
    patterns: Vec<(String, Option<Vec<f64>>)>,
    max_dashes: Option<f64>,
    /// entities visited so far (nested inserts count each time they are drawn)
    visits: usize,
}

/// Entities one walk draws at most, counting every copy a nested insert makes. Spec gap vs 0.5: blocks inserted inside
/// blocks (16 levels) multiply without end in the JS, which then hangs; past this the walk draws nothing more.
pub const MAX_WALK_VISITS: usize = 10_000_000;

fn color_value(v: &Value) -> Color {
    Color::from_value(v).unwrap_or(Color::Null)
}

impl Walker<'_> {
    fn shown(&self, name: Option<&str>) -> bool {
        if let Some(f) = self.frozen
            && !f.names.is_empty()
            && f.names.contains(&name.map_or_else(|| "UNDEFINED".to_string(), str::to_uppercase))
        {
            return false;
        }
        match name.and_then(|n| self.layers.get(&n.to_string())) {
            None => true,
            Some(l) => l.on && !l.frozen && (!self.plot || l.plot),
        }
    }
    fn resolve_color(&self, e: &Entity, by_block: Option<&Color>, parent_layer: Option<&str>) -> String {
        cc_cad::color::resolve_color(e, by_block, parent_layer, &|n| self.layers.get(&n.to_string()).map(|l| l.color.clone()))
    }
    fn pattern(&mut self, name: &str) -> Option<Vec<f64>> {
        if let Some((_, p)) = self.patterns.iter().find(|(n, _)| n == name) {
            return p.clone();
        }
        let p = pattern_of(self.doc, Some(name));
        self.patterns.push((name.to_string(), p.clone()));
        p
    }
    #[allow(clippy::too_many_arguments)]
    fn stroke_of(
        &mut self,
        out: &mut dyn WalkOut,
        e: &Entity,
        ps: Vec<Prim>,
        color: &str,
        lw: f64,
        mm: Option<&M>,
        ctx: &BlkCtx,
        parent: Option<&str>,
        o: Option<Opts>,
    ) {
        let lt = resolve_lt(&self.layers, e, ctx.lt.as_deref(), parent);
        let pat = self.pattern(&lt);
        let lts = e.num("lts").filter(|v| *v > 0.0).unwrap_or(1.0);
        let d = match &pat {
            Some(pp) if self.max_dashes.is_none_or(|cap| crate::linetypes::dash_count(&ps, pp, self.ltscale * lts) <= cap) => {
                dash_prims(&ps, Some(pp), self.ltscale * lts)
            }
            _ => ps,
        };
        out.stroke(
            color,
            match mm {
                Some(m) => d.iter().flat_map(|q| tx_prim(q, m)).collect(),
                None => d,
            },
            lw,
            o,
        );
    }
    fn tr_of(&self, e: &Entity, blk_tr: Option<f64>, parent_layer: Option<&str>) -> Option<f64> {
        match e.get("tr") {
            Some(Value::String(s)) if s == "ByBlock" => return blk_tr,
            Some(Value::Number(n)) if n.as_f64().is_some_and(f64::is_finite) => return n.as_f64(),
            _ => {}
        }
        let ln = match (e.layer.as_deref(), parent_layer) {
            (Some("0"), Some(p)) if !p.is_empty() => p,
            (l, _) => l.unwrap_or(""),
        };
        self.layers.get(&ln.to_string()).and_then(|l| js::finite(l.extra.get("tr")))
    }

    #[allow(clippy::too_many_arguments)]
    fn emit(
        &mut self,
        out: &mut dyn WalkOut,
        e: &Entity,
        by_block: Option<&Color>,
        parent: Option<&str>,
        mm: Option<&M>,
        depth: u32,
        blk: &BlkCtx,
    ) {
        self.visits = self.visits.saturating_add(1);
        if depth > 16 || self.visits > MAX_WALK_VISITS {
            return;
        }
        let color = self.resolve_color(e, by_block, parent);
        let lw = resolve_lw(&self.layers, e, blk.lw, parent);
        let tr = self.tr_of(e, blk.tr, parent);
        let o = opts_of(tr);
        let pp = |x: f64, y: f64| match mm {
            Some(m) => m_apply(m, p(x, y)),
            None => p(x, y),
        };
        let tx_loops = |loops: &[Vec<f64>]| -> Vec<Vec<f64>> {
            match mm {
                None => loops.to_vec(),
                Some(m) => loops
                    .iter()
                    .map(|l| {
                        let mut o = Vec::with_capacity(l.len());
                        for c in l.chunks_exact(2) {
                            if let [x, y] = c {
                                let q = m_apply(m, p(*x, *y));
                                o.push(q.x);
                                o.push(q.y);
                            }
                        }
                        o
                    })
                    .collect(),
            }
        };
        match &e.geo {
            Geo::Insert(_) | Geo::Dim(_) | Geo::Mleader(_) => {
                let parts: Vec<Entity>;
                let mut m2: Option<M> = mm.copied();
                if let Geo::Insert(ins) = &e.geo {
                    let Some(b) = self.doc.blocks.get(&ins.block) else { return };
                    parts = if b.attdefs.as_ref().is_some_and(|a| !a.is_empty()) || ins.attrs.is_some() {
                        let mut v = b.entities.clone();
                        v.extend(attr_texts(ins, Some(b), false));
                        v
                    } else {
                        b.entities.clone()
                    };
                    let im = insert_matrix(ins, b);
                    m2 = Some(match mm {
                        Some(m) => m_mul(m, &im),
                        None => im,
                    });
                } else if let Geo::Dim(d) = &e.geo {
                    parts = dim_parts(e, d.block.as_deref(), Some(&self.doc.blocks));
                } else {
                    parts = leader_geometry(e);
                }
                let layer: Option<String> = match (e.layer.as_deref(), parent) {
                    (Some("0"), Some(p)) if !p.is_empty() => Some(p.to_string()),
                    (l, _) => l.map(str::to_string),
                };
                let inner = if matches!(e.geo, Geo::Insert(_)) {
                    BlkCtx { lt: Some(resolve_lt(&self.layers, e, blk.lt.as_deref(), parent)), lw: Some(lw), tr }
                } else {
                    BlkCtx { lt: Some("Continuous".into()), lw: Some(lw), tr }
                };
                let col = Color::Str(color.clone());
                for c in &parts {
                    let ln = if c.layer.as_deref() == Some("0") { layer.as_deref() } else { c.layer.as_deref() };
                    if !self.shown(ln) {
                        continue;
                    }
                    self.emit(out, c, Some(&col), layer.as_deref(), m2.as_ref(), depth + 1, &inner);
                }
            }
            Geo::Text(_) => {
                let te = match mm {
                    Some(m) => crate::tx::tx_entity(e, m),
                    None => e.clone(),
                };
                if let Some(bgm) = e.get("bgm").filter(|b| js::truthy(Some(b))) {
                    let win = bgm.get("color").and_then(Value::as_str) == Some("bg");
                    let scale = match bgm.get("scale") {
                        None => 1.5,
                        Some(v) => v.as_f64().unwrap_or(f64::NAN),
                    };
                    let ring = mask_loop(&te, scale);
                    let mc = if win {
                        "ink".to_string()
                    } else {
                        let mut tmp = e.clone();
                        tmp.color = Some(bgm.get("color").map_or(Color::Null, color_value));
                        if bgm.get("color").is_none() {
                            tmp.color = None;
                        }
                        self.resolve_color(&tmp, by_block, parent)
                    };
                    if out.has_mask() {
                        out.mask(&mc, ring, win);
                    } else {
                        out.fill(&mc, vec![ring], true, if win { Some(Opts { alpha: None, wipe: true }) } else { None });
                    }
                }
                out.text(&color, &te, o);
            }
            Geo::Point(pt) => {
                let q = pp(pt.x, pt.y);
                out.point(&color, q.x, q.y);
            }
            Geo::Hatch(h) => {
                let solid = h.solid == Some(true);
                if let Some(bg) = e.get("bg").filter(|v| !v.is_null())
                    && !solid
                {
                    let mut tmp = e.clone();
                    tmp.color = Some(color_value(bg));
                    let c = self.resolve_color(&tmp, by_block, parent);
                    out.fill(&c, tx_loops(&h.loops), true, o);
                }
                if let Some(lines) = hatch_lines(e, self.doc) {
                    out.stroke(
                        &color,
                        match mm {
                            Some(m) => lines.iter().flat_map(|q| tx_prim(q, m)).collect(),
                            None => lines,
                        },
                        lw,
                        o,
                    );
                    return;
                }
                let wipe = js::truthy(e.get("wipe")) && solid && e.color == Some(Color::Str("bg".into()));
                let oo = if wipe { Some(Opts { alpha: o.and_then(|x| x.alpha), wipe: true }) } else { o };
                out.fill(&color, tx_loops(&h.loops), solid, oo);
            }
            Geo::Ellipse(_) => {
                let pts = ellipse_points(e, 96.0);
                let ps: Vec<Prim> = pts.iter().zip(pts.iter().skip(1)).map(|(a, b)| Prim::l(a.x, a.y, b.x, b.y)).collect();
                self.stroke_of(out, e, ps, &color, lw, mm, blk, parent, o);
            }
            Geo::Poly(_) => match poly_bands(e) {
                None => {
                    let ps = prims(e, None);
                    self.stroke_of(out, e, ps, &color, lw, mm, blk, parent, o);
                }
                Some((loops, plain)) => {
                    if !loops.is_empty() {
                        out.fill(&color, tx_loops(&loops), true, o);
                    }
                    if !plain.is_empty() {
                        self.stroke_of(out, e, plain, &color, lw, mm, blk, parent, o);
                    }
                }
            },
            Geo::Xline(i) | Geo::Ray(i) => {
                if out.has_infinite() {
                    let q = pp(i.x, i.y);
                    let d = match mm {
                        Some(m) => p(m[0] * i.dx + m[2] * i.dy, m[1] * i.dx + m[3] * i.dy),
                        None => p(i.dx, i.dy),
                    };
                    let geo = cc_cad::entity::Inf { x: q.x, y: q.y, dx: d.x, dy: d.y };
                    let ent = Entity::new(if matches!(e.geo, Geo::Ray(_)) { Geo::Ray(geo) } else { Geo::Xline(geo) });
                    out.infinite(&color, &ent, lw, o);
                    return;
                }
                let ps = prims(e, None);
                self.stroke_of(out, e, ps, &color, lw, mm, blk, parent, o);
            }
            _ => {
                let ps = prims(e, None);
                self.stroke_of(out, e, ps, &color, lw, mm, blk, parent, o);
            }
        }
    }
}

fn opts_of(tr: Option<f64>) -> Option<Opts> {
    match tr {
        Some(t) if t.is_finite() && t > 0.0 => Some(Opts { alpha: Some(js::max(0.0, js::min(1.0, 1.0 - t / 100.0))), wipe: false }),
        _ => None,
    }
}

/// The layers as one viewport shows them (VP.3 overrides).
fn override_layers<'a>(doc: &'a Doc, frozen: Option<&Frozen>) -> Cow<'a, Layers> {
    let Some(ovr) = frozen.and_then(|f| f.ovr.as_ref()).filter(|o| !o.is_empty()) else { return Cow::Borrowed(&doc.layers) };
    let mut out = Layers::new();
    for (k, l) in doc.layers.iter() {
        let up = k.to_uppercase();
        let nl: Layer = match ovr.iter().find(|(n, _)| *n == up) {
            Some((_, o)) => {
                let mut nl = l.clone();
                if let Some(c) = o.get("color") {
                    nl.color = color_value(c);
                }
                if let Some(lt) = o.get("lt").filter(|v| js::truthy(Some(v))) {
                    nl.linetype = Some(js::to_js_string(lt));
                }
                if let Some(lw) = js::finite(o.get("lw")) {
                    nl.lineweight = Some(lw);
                }
                if let Some(tr) = js::finite(o.get("tr")) {
                    nl.extra.insert("tr".into(), num(tr));
                }
                nl
            }
            None => l.clone(),
        };
        out.insert(k.clone(), nl);
    }
    Cow::Owned(out)
}

/// Draw `entities` (of `doc`) into `out`.
pub fn walk(doc: &Doc, entities: &[Entity], out: &mut dyn WalkOut, opts: &WalkOpts) {
    let ltscale = match doc.meta.get("ltscale") {
        Some(v) if js::to_number(v) > 0.0 => js::to_number(v),
        _ => 1.0,
    };
    let mut w = Walker {
        doc,
        layers: override_layers(doc, opts.frozen),
        plot: opts.plot,
        frozen: opts.frozen,
        ltscale,
        patterns: Vec::new(),
        max_dashes: opts.max_dashes,
        visits: 0,
    };
    let m = opts.m;
    for (i, e) in entities.iter().enumerate() {
        out.begin(i);
        if let Geo::Viewport(_) = &e.geo {
            out.viewport(e);
            if w.shown(e.layer.as_deref()) {
                let ps = prims(e, None);
                let ps = match &m {
                    Some(mm) => ps.iter().flat_map(|q| tx_prim(q, mm)).collect(),
                    None => ps,
                };
                let c = w.resolve_color(e, None, None);
                let lw = resolve_lw(&w.layers, e, None, None);
                let o = opts_of(w.tr_of(e, None, None));
                out.stroke(&c, ps, lw, o);
            }
            continue;
        }
        if w.shown(e.layer.as_deref()) {
            w.emit(out, e, None, None, m.as_ref(), 0, &BlkCtx::default());
        }
    }
}

/// A text's background mask (WL.8) as a ring [x, y, …]: the text's box (its wrap width when it has one) grown by
/// (scale − 1) × the text height on every side.
pub fn mask_loop(e: &Entity, scale: f64) -> Vec<f64> {
    let Geo::Text(t) = &e.geo else { return Vec::new() };
    let b = text_layout(t).bx;
    let mg = js::max(0.0, (if scale.is_finite() { scale } else { 1.5 }) - 1.0) * t.h;
    let (mut x0, mut x1) = (b[0], b[0] + b[2]);
    if let Some(ww) = t.ww.filter(|w| *w > 0.0 && *w > b[2]) {
        match t.ha.as_deref() {
            Some("center") => {
                let c = (x0 + x1) / 2.0;
                x0 = c - ww / 2.0;
                x1 = c + ww / 2.0;
            }
            Some("right") => x0 = x1 - ww,
            _ => x1 = x0 + ww,
        }
    }
    let r = js::or(t.rot, 0.0);
    let (c, s) = (r.jcos(), r.jsin());
    let pt = |x: f64, y: f64| [t.x + x * c - y * s, t.y + x * s + y * c];
    let pts = [pt(x0 - mg, b[1] - mg), pt(x1 + mg, b[1] - mg), pt(x1 + mg, b[1] + b[3] + mg), pt(x0 - mg, b[1] + b[3] + mg)];
    let mut out: Vec<f64> = pts.iter().flatten().copied().collect();
    out.extend(pts[0]);
    out
}

/// A polyline's widths (`w`, `pts[i].sw` / `ew`) → (filled bands per wide segment, prims of the segments without
/// width), or None when it has no width at all.
pub fn poly_bands(e: &Entity) -> Option<(Vec<Vec<f64>>, Vec<Prim>)> {
    let Geo::Poly(pl) = &e.geo else { return None };
    let pts = &pl.pts;
    let g = e.num("w").filter(|w| w.is_finite() && *w > 0.0).unwrap_or(0.0);
    if g == 0.0 && !pts.iter().any(|q| q.num("sw").is_some_and(|v| v > 0.0) || q.num("ew").is_some_and(|v| v > 0.0)) {
        return None;
    }
    let n = pts.len();
    let segs = if pl.is_closed() { n } else { n.saturating_sub(1) };
    let mut loops = Vec::new();
    let mut plain = Vec::new();
    for i in 0..segs {
        let (Some(a), Some(q)) = (pts.get(i), pts.get((i + 1) % n)) else { continue };
        let ws = a.num("sw").filter(|v| v.is_finite()).unwrap_or(g);
        let we = a.num("ew").filter(|v| v.is_finite()).unwrap_or(g);
        let one =
            poly_prims(&[cc_cad::Vertex::new(a.x, a.y, a.bulge()), cc_cad::Vertex { x: q.x, y: q.y, b: None, extra: Obj::new() }], false);
        if !(ws > 0.0 || we > 0.0) {
            plain.extend(one);
            continue;
        }
        let Some(pr) = one.first() else { continue };
        let mut along: Vec<[f64; 5]> = Vec::new();
        if pr.is_l() {
            let l = js::or(Some(js::hypot(pr.x2 - pr.x1, pr.y2 - pr.y1)), 1.0);
            let nx = -(pr.y2 - pr.y1) / l;
            let ny = (pr.x2 - pr.x1) / l;
            along.push([pr.x1, pr.y1, nx, ny, 0.0]);
            along.push([pr.x2, pr.y2, nx, ny, 1.0]);
        } else {
            let sweep = js::or(Some((pr.a1 - pr.a0 + TAU) % TAU), TAU);
            let ccw = a.bulge() > 0.0;
            let k = steps(2.0, sweep / (PI / 18.0));
            for j in 0..=k {
                let t = f(j) / f(k);
                let ang = if ccw { pr.a0 + sweep * t } else { pr.a1 - sweep * t };
                let (cx, cy) = (ang.jcos(), ang.jsin());
                along.push([pr.cx + pr.r * cx, pr.cy + pr.r * cy, if ccw { -cx } else { cx }, if ccw { -cy } else { cy }, t]);
            }
        }
        let mut left = Vec::new();
        let mut right: Vec<f64> = Vec::new();
        for [x, y, nx, ny, t] in along {
            let h = (ws + (we - ws) * t) / 2.0;
            left.push(x + nx * h);
            left.push(y + ny * h);
            right.splice(0..0, [x - nx * h, y - ny * h]);
        }
        let mut ring = left;
        ring.extend(right);
        if let (Some(&x), Some(&y)) = (ring.first(), ring.get(1)) {
            ring.push(x);
            ring.push(y);
        }
        loops.push(ring);
    }
    Some((loops, plain))
}

/// An output that records every call as JSON (`{op, color, …}`), for tests and the parity harness. `mask` /
/// `infinite` say whether it offers those optional calls.
#[derive(Debug, Default)]
pub struct Recorder {
    pub calls: Vec<Value>,
    pub mask: bool,
    pub infinite: bool,
}
impl WalkOut for Recorder {
    fn stroke(&mut self, color: &str, prims: Vec<Prim>, lw: f64, o: Option<Opts>) {
        self.calls.push(serde_json::json!({ "op": "stroke", "color": color, "prims": prims.iter().map(Prim::to_value).collect::<Vec<_>>(), "lw": num(lw), "o": Opts::to_value(o) }));
    }
    fn fill(&mut self, color: &str, loops: Vec<Vec<f64>>, solid: bool, o: Option<Opts>) {
        let l: Vec<Value> = loops.iter().map(|l| Value::Array(l.iter().map(|v| num(*v)).collect())).collect();
        self.calls.push(serde_json::json!({ "op": "fill", "color": color, "loops": l, "solid": solid, "o": Opts::to_value(o) }));
    }
    fn text(&mut self, color: &str, e: &Entity, o: Option<Opts>) {
        self.calls.push(serde_json::json!({ "op": "text", "color": color, "e": e.to_value(), "o": Opts::to_value(o) }));
    }
    fn point(&mut self, color: &str, x: f64, y: f64) {
        self.calls.push(serde_json::json!({ "op": "point", "color": color, "x": num(x), "y": num(y) }));
    }
    fn viewport(&mut self, e: &Entity) {
        self.calls.push(serde_json::json!({ "op": "viewport", "id": e.id }));
    }
    fn has_mask(&self) -> bool {
        self.mask
    }
    fn mask(&mut self, color: &str, ring: Vec<f64>, wipe: bool) {
        self.calls.push(
            serde_json::json!({ "op": "mask", "color": color, "loop": ring.iter().map(|v| num(*v)).collect::<Vec<_>>(), "wipe": wipe }),
        );
    }
    fn has_infinite(&self) -> bool {
        self.infinite
    }
    fn infinite(&mut self, color: &str, e: &Entity, lw: f64, o: Option<Opts>) {
        self.calls.push(serde_json::json!({ "op": "infinite", "color": color, "e": e.to_value(), "lw": num(lw), "o": Opts::to_value(o) }));
    }
}
