//! The editable drawing as the GPU viewer draws it: a [`Doc`] walked (walk.js, as CadCraft's view.js caches it) into
//! the display model of `cc-drawing` (spaces of batches per colour × line weight), so the drafting canvas renders
//! through the same renderer as the DWG viewer.
//!
//! Arcs become polylines here (the JS canvas draws them with `ctx.arc`); texts become one display text per line,
//! anchored on its baseline as view.js `drawTexts` places them. Construction lines are left out (view.js clips them to
//! the screen every frame; the canvas draws them as overlays).

use cc_cad::entity::{Entity, Geo};
use cc_cad::{Doc, SpaceId, js};
use cc_drawing::{Batch, Bounds, Color, Drawing, Fill, Layer, Space, SpaceKind, Stats, Text, Viewport};

use crate::geom::{K, Prim, TAU, text_layout};
use crate::walk::{Opts, WalkOpts, WalkOut, walk};

/// Polyline points of an arc: a segment every 3.75° (96 per circle), at least 2.
fn arc_points(q: &Prim, out: &mut Vec<f64>) {
    let (from, to) = if q.rev == Some(true) { (q.a1, q.a0) } else { (q.a0, q.a1) };
    let mut sweep = if q.rev == Some(true) { from - to } else { to - from };
    if q.full {
        sweep = TAU;
    }
    while sweep < 0.0 {
        sweep += TAU;
    }
    let sweep = if sweep > TAU { TAU } else { sweep };
    let dir = if q.rev == Some(true) { -1.0 } else { 1.0 };
    let n = (sweep / (TAU / 96.0)).ceil().clamp(2.0, 96.0);
    let mut i = 0.0;
    while i <= n {
        let a = from + dir * sweep * (i / n);
        out.push(q.cx + q.r * a.cos());
        out.push(q.cy + q.r * a.sin());
        i += 1.0;
    }
}

fn color_of(c: &str) -> Color {
    Color::parse(c)
}

/// Collects walk's output into batches keyed by colour × line weight.
struct Collect {
    batches: Vec<Batch>,
    bounds: Bounds,
    viewports: Vec<Viewport>,
}

impl Collect {
    fn batch(&mut self, color: &str, lw: f64) -> Option<&mut Batch> {
        let c = color_of(color);
        let lw = if lw.is_finite() && lw >= 0.0 { lw } else { 0.0 };
        let i = match self.batches.iter().position(|b| b.color == c && b.lw == lw) {
            Some(i) => i,
            None => {
                self.batches.push(Batch { layer: String::new(), color: c, lw, lines: Vec::new(), fills: Vec::new(), texts: Vec::new() });
                self.batches.len().saturating_sub(1)
            }
        };
        self.batches.get_mut(i)
    }
}

impl WalkOut for Collect {
    fn stroke(&mut self, color: &str, prims: Vec<Prim>, lw: f64, _o: Option<Opts>) {
        let mut runs: Vec<Vec<f64>> = Vec::new();
        let mut cur: Vec<f64> = Vec::new();
        let (mut lx, mut ly) = (f64::INFINITY, f64::INFINITY);
        for q in &prims {
            let mut pts = Vec::new();
            match q.k {
                K::L => pts.extend_from_slice(&[q.x1, q.y1, q.x2, q.y2]),
                K::A => arc_points(q, &mut pts),
            }
            let (Some(&sx), Some(&sy)) = (pts.first(), pts.get(1)) else { continue };
            if (sx - lx).abs() > 1e-9 || (sy - ly).abs() > 1e-9 {
                if cur.len() >= 4 {
                    runs.push(std::mem::take(&mut cur));
                }
                cur.clear();
                cur.extend_from_slice(&pts);
            } else {
                cur.extend_from_slice(pts.get(2..).unwrap_or(&[]));
            }
            if let [.., x, y] = cur.as_slice() {
                (lx, ly) = (*x, *y);
            }
        }
        if cur.len() >= 4 {
            runs.push(cur);
        }
        for r in &runs {
            self.bounds.grow(r);
        }
        if let Some(b) = self.batch(color, lw) {
            b.lines.extend(runs);
        }
    }
    fn fill(&mut self, color: &str, loops: Vec<Vec<f64>>, solid: bool, _o: Option<Opts>) {
        for l in &loops {
            self.bounds.grow(l);
        }
        if let Some(b) = self.batch(color, -1.0) {
            if !solid {
                for l in &loops {
                    let mut ring = l.clone();
                    if let (Some(&x), Some(&y)) = (l.first(), l.get(1)) {
                        ring.extend_from_slice(&[x, y]);
                    }
                    b.lines.push(ring);
                }
            }
            b.fills.push(Fill { rings: loops, pattern: !solid });
        }
    }
    fn text(&mut self, color: &str, e: &Entity, _o: Option<Opts>) {
        let Geo::Text(t) = &e.geo else { return };
        let lay = text_layout(t);
        let rot = js::or(t.rot, 0.0);
        let (s, c) = rot.sin_cos();
        let ha = match t.ha.as_deref() {
            Some("center") => 1,
            Some("right") => 2,
            _ => 0,
        };
        let texts: Vec<Text> = lay
            .lines
            .iter()
            .zip(&lay.baselines)
            .map(|(line, &bl)| Text { x: t.x - s * bl, y: t.y + c * bl, h: t.h, rot, s: line.clone(), ha, va: 0, mirror: false })
            .collect();
        let b = crate::geom::text_corners(t);
        for q in b {
            self.bounds.grow(&[q.x, q.y]);
        }
        if let Some(bt) = self.batch(color, -1.0) {
            bt.texts.extend(texts);
        }
    }
    fn point(&mut self, color: &str, x: f64, y: f64) {
        // a dot: a zero-length line still draws as one round-capped quad
        self.bounds.grow(&[x, y]);
        if let Some(b) = self.batch(color, -1.0) {
            b.lines.push(vec![x, y, x, y]);
        }
    }
    fn viewport(&mut self, e: &Entity) {
        let Geo::Viewport(v) = &e.geo else { return };
        if !(v.s.is_finite() && v.s > 0.0) {
            return;
        }
        self.viewports.push(Viewport {
            cx: v.x,
            cy: v.y,
            w: v.w,
            h: v.h,
            vx: v.cx,
            vy: v.cy,
            view_height: v.h / v.s,
            twist: js::or(v.tw, 0.0),
            frozen: Vec::new(),
        });
    }
}

/// Any entities of a document (previews, a selection) walked into batches, in world coordinates.
pub fn walk_batches(doc: &Doc, ents: &[Entity]) -> Vec<Batch> {
    let mut out = Collect { batches: Vec::new(), bounds: Bounds::default(), viewports: Vec::new() };
    walk(doc, ents, &mut out, &WalkOpts::default());
    out.batches
}

fn space_of(doc: &Doc, space: SpaceId, key: String, name: String, kind: SpaceKind, paper: Option<Bounds>) -> Space {
    let ents: Vec<Entity> = doc.space_entities(space).filter(|e| !matches!(e.geo, Geo::Xline(_) | Geo::Ray(_))).cloned().collect();
    let mut out = Collect { batches: Vec::new(), bounds: Bounds::default(), viewports: Vec::new() };
    walk(doc, &ents, &mut out, &WalkOpts::default());
    out.batches.retain(|b| !(b.lines.is_empty() && b.fills.is_empty() && b.texts.is_empty()));
    let paper_unit = paper.map(|_| "mm".to_string());
    Space { key, name, kind, batches: out.batches, bounds: out.bounds, paper, paper_unit, viewports: out.viewports, images: Vec::new() }
}

fn paper_bounds(p: &cc_cad::Obj) -> Option<Bounds> {
    let n = |k: &str, d: f64| p.get(k).and_then(serde_json::Value::as_f64).filter(|v| v.is_finite()).unwrap_or(d);
    let (x0, y0, w, h) = (n("x0", 0.0), n("y0", 0.0), n("w", 0.0), n("h", 0.0));
    (w > 0.0 && h > 0.0).then_some(Bounds { min_x: x0, min_y: y0, max_x: x0 + w, max_y: y0 + h })
}

/// The whole document for the viewer: the model, then each sheet in tab order.
pub fn doc_drawing(doc: &Doc) -> Drawing {
    let mut spaces = vec![space_of(doc, cc_cad::MODEL, "model".into(), "Model".into(), SpaceKind::Model, None)];
    for (i, l) in doc.layouts.iter().enumerate() {
        spaces.push(space_of(doc, l.space, format!("layout:{i}"), l.name.clone(), SpaceKind::Paper, paper_bounds(&l.paper)));
    }
    let layers = doc
        .layers
        .values()
        .map(|l| Layer { name: l.name.clone(), color: Color::Ink, on: l.on && !l.frozen, lw: 0.0, plot: l.plot })
        .collect();
    let entities = doc.every_entity().count() as u64;
    Drawing {
        version: String::new(),
        release: String::new(),
        units: String::new(),
        xrefs: Vec::new(),
        images: Vec::new(),
        layers,
        spaces,
        stats: Stats { entities, ..Stats::default() },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn walks_lines_arcs_texts_into_batches() {
        let mut d = Doc::new();
        d.load(&json!({ "entities": [
            { "type": "line", "x1": 0, "y1": 0, "x2": 10, "y2": 0 },
            { "type": "circle", "cx": 5, "cy": 5, "r": 2, "color": 1 },
            { "type": "text", "x": 0, "y": 20, "h": 2, "s": "A\nB" }
        ]}));
        let dr = doc_drawing(&d);
        let m = &dr.spaces[0];
        assert_eq!(m.kind, SpaceKind::Model);
        let lines: usize = m.batches.iter().map(|b| b.lines.len()).sum();
        assert_eq!(lines, 2, "{:?}", m.batches);
        let circle = m.batches.iter().find(|b| b.color != Color::Ink).unwrap();
        assert_eq!(circle.lines[0].len(), 97 * 2, "a full circle: 96 segments");
        let texts: Vec<&Text> = m.batches.iter().flat_map(|b| &b.texts).collect();
        assert_eq!(texts.len(), 2);
        assert!(texts[1].y < texts[0].y, "the second line under the first");
        assert!(m.bounds.min_x <= 0.0 && m.bounds.max_x >= 10.0 && m.bounds.max_y > 20.0);
    }
}
