//! The editable drawing as the GPU viewer draws it: a [`Doc`] walked (walk.js, as CadCraft's view.js caches it) into
//! the display model of `cc-drawing` (spaces of batches per colour × line weight), so the drafting canvas renders
//! through the same renderer as the DWG viewer.
//!
//! Arcs become polylines here (the JS canvas draws them with `ctx.arc`); texts become one display text per line,
//! anchored on its baseline as view.js `drawTexts` places them. Construction lines are left out (view.js clips them to
//! the screen every frame; the canvas draws them as overlays).

use std::collections::HashMap;

use cc_cad::entity::{Entity, Geo};
use cc_cad::omap::OMap;
use cc_cad::{Doc, SpaceId, js};
use cc_drawing::{Batch, Bounds, Color, Drawing, Fill, ImageRef, Layer, Space, SpaceImage, SpaceKind, Stats, Text, Viewport};

use crate::geom::{K, Prim, TAU, text_layout};
use crate::walk::{Opts, PicOut, WalkOpts, WalkOut, walk};

/// The window draws linetypes as 0.5's canvas does (view.js walks with linetypes.js's own limit: a run past 20,000
/// pieces draws continuous), so the screen, plots and the JS agree (M4.52; before, the window stopped at 400 dashes
/// per object). A dense pattern makes millions of dashes (ar1all OVERALL PLAN: 2.35 M segments): they are kept one
/// list per stroke with `PEN_UP` between them, and the cache walks only what an edit changed.
fn display_opts() -> WalkOpts<'static> {
    WalkOpts { max_dashes: None, ..WalkOpts::default() }
}

/// Polyline points of an arc: a segment every 3.75° (96 per circle), at least 1 (a linetype's dash on an arc is a
/// short piece: one chord is within a fraction of a pixel of it, and a dashed circle has thousands of them).
fn arc_points(q: &Prim, out: &mut Vec<f64>) {
    let (from, to) = if q.rev == Some(true) { (q.a1, q.a0) } else { (q.a0, q.a1) };
    // a0..a1 as the canvas's ctx.arc draws it: `full` is not used (a linetype's dash on a circle keeps the circle's
    // `full` flag with its own a0 / a1)
    let mut sweep = if q.rev == Some(true) { from - to } else { to - from };
    while sweep < 0.0 {
        sweep += TAU;
    }
    let sweep = if sweep > TAU { TAU } else { sweep };
    let dir = if q.rev == Some(true) { -1.0 } else { 1.0 };
    let n = (sweep / (TAU / 96.0)).ceil().clamp(1.0, 96.0);
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
    /// the layer the batches being filled belong to (the top-level object's layer, as the viewer's layers panel hides)
    layer: String,
    batches: Vec<Batch>,
    bounds: Bounds,
    viewports: Vec<Viewport>,
    /// pictures (walk's `out.picture`)
    images: Vec<SpaceImage>,
    /// a stroke's separate runs (a linetype's dashes) in one polyline with `PEN_UP` between them (the window's
    /// display; previews keep a polyline per run for their own painting)
    joined: bool,
    /// a primitive's points (reused)
    scratch: Vec<f64>,
}

impl Collect {
    fn new(layer: &str) -> Self {
        Collect {
            layer: layer.to_string(),
            batches: Vec::new(),
            bounds: Bounds::default(),
            viewports: Vec::new(),
            images: Vec::new(),
            joined: false,
            scratch: Vec::new(),
        }
    }
    fn joined(layer: &str) -> Self {
        Collect { joined: true, ..Collect::new(layer) }
    }
    fn batch(&mut self, color: &str, lw: f64) -> Option<&mut Batch> {
        let c = color_of(color);
        let lw = if lw.is_finite() && lw >= 0.0 { lw } else { 0.0 };
        let i = match self.batches.iter().position(|b| b.color == c && b.lw == lw && b.layer == self.layer) {
            Some(i) => i,
            None => {
                self.batches.push(Batch {
                    layer: self.layer.clone(),
                    color: c,
                    lw,
                    lines: Vec::new(),
                    fills: Vec::new(),
                    texts: Vec::new(),
                });
                self.batches.len().saturating_sub(1)
            }
        };
        self.batches.get_mut(i)
    }
}

impl WalkOut for Collect {
    /// A picture: pixel (x right, y down) → world, as the viewer's DWG images (dwg-compile.js) — its frame clips it.
    fn picture(&mut self, p: &PicOut) {
        let m = [p.ux, p.uy, -p.vx, -p.vy, p.x + p.h * p.vx, p.y + p.h * p.vy];
        if !(m.iter().all(|v| v.is_finite()) && p.w > 0.0 && p.h > 0.0) {
            return;
        }
        let alpha = (p.alpha < 1.0).then_some(p.alpha.max(0.0));
        self.images.push(SpaceImage {
            m,
            w: p.w,
            h: p.h,
            file: p.file.clone(),
            layer: self.layer.clone(),
            clip: Some(p.clip.clone()),
            alpha,
        });
    }
    fn stroke(&mut self, color: &str, prims: Vec<Prim>, lw: f64, _o: Option<Opts>) {
        let mut runs: Vec<Vec<f64>> = Vec::new();
        // joined: every run goes into `all`, PEN_UP between them; `start` = where the current run begins in it
        let mut all: Vec<f64> = Vec::new();
        let mut start = 0usize;
        let mut cur: Vec<f64> = Vec::new();
        let (mut lx, mut ly) = (f64::INFINITY, f64::INFINITY);
        let mut pts = std::mem::take(&mut self.scratch);
        for q in &prims {
            pts.clear();
            match q.k {
                K::L => pts.extend_from_slice(&[q.x1, q.y1, q.x2, q.y2]),
                K::A => arc_points(q, &mut pts),
            }
            let (Some(&sx), Some(&sy)) = (pts.first(), pts.get(1)) else { continue };
            let run = if self.joined { &mut all } else { &mut cur };
            if (sx - lx).abs() > 1e-9 || (sy - ly).abs() > 1e-9 {
                if self.joined {
                    // a run shorter than one segment is dropped, as below
                    if all.len().saturating_sub(start) < 4 {
                        all.truncate(start);
                    }
                    if !all.is_empty() {
                        all.extend_from_slice(&cc_drawing::PEN_UP);
                    }
                    start = all.len();
                    all.extend_from_slice(&pts);
                } else {
                    if run.len() >= 4 {
                        runs.push(std::mem::take(run));
                    }
                    cur.clear();
                    cur.extend_from_slice(&pts);
                }
            } else {
                run.extend_from_slice(pts.get(2..).unwrap_or(&[]));
            }
            let last = if self.joined { &all } else { &cur };
            if let [.., x, y] = last.as_slice() {
                (lx, ly) = (*x, *y);
            }
        }
        self.scratch = pts;
        if self.joined {
            if all.len().saturating_sub(start) < 4 {
                all.truncate(start);
                // and the PEN_UP before the dropped run
                if all.len() >= 2 && all.as_chunks::<2>().0.last().is_some_and(|p| cc_drawing::is_pen_up(p[0], p[1])) {
                    all.truncate(all.len().saturating_sub(2));
                }
            }
            if !all.is_empty() {
                runs.push(all);
            }
        } else if cur.len() >= 4 {
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
pub fn walk_batches<E: std::borrow::Borrow<Entity>>(doc: &Doc, ents: &[E]) -> Vec<Batch> {
    let mut out = Collect::new("");
    walk(doc, ents, &mut out, &display_opts());
    out.batches
}

fn space_of(doc: &Doc, space: SpaceId, key: String, name: String, kind: SpaceKind, paper: Option<Bounds>) -> Space {
    // top-level objects grouped by layer (first-seen order), one walk per layer
    let mut groups: Vec<(String, Vec<&Entity>)> = Vec::new();
    let mut at: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for e in doc.space_entities(space).filter(|e| !matches!(e.geo, Geo::Xline(_) | Geo::Ray(_))) {
        let l = e.layer.as_deref().unwrap_or("0");
        let i = *at.entry(l).or_insert_with(|| {
            groups.push((l.to_string(), Vec::new()));
            groups.len().saturating_sub(1)
        });
        if let Some((_, g)) = groups.get_mut(i) {
            g.push(e);
        }
    }
    let mut out = Collect::joined("");
    let opts = display_opts();
    for (l, ents) in &groups {
        out.layer.clone_from(l);
        walk(doc, ents, &mut out, &opts);
    }
    out.batches.retain(|b| !(b.lines.is_empty() && b.fills.is_empty() && b.texts.is_empty()));
    let paper_unit = paper.map(|_| "mm".to_string());
    Space { key, name, kind, batches: out.batches, bounds: out.bounds, paper, paper_unit, viewports: out.viewports, images: out.images }
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
    drawing_of(doc, spaces)
}

/// One top-level entity's share of the display: its batches (in first-use order), bounds and viewports.
#[derive(Clone, Debug, Default)]
struct Piece {
    batches: Vec<Batch>,
    bounds: Bounds,
    viewports: Vec<Viewport>,
    images: Vec<SpaceImage>,
}

/// Collects a walk's output per top-level entity (`WalkOut::begin`).
struct PieceCollect {
    layer: String,
    pieces: Vec<Piece>,
    cur: Collect,
}

impl PieceCollect {
    fn flush(&mut self) {
        let c = std::mem::replace(&mut self.cur, Collect::joined(&self.layer));
        self.pieces.push(Piece { batches: c.batches, bounds: c.bounds, viewports: c.viewports, images: c.images });
    }
}

impl WalkOut for PieceCollect {
    fn begin(&mut self, index: usize) {
        if index > 0 {
            self.flush();
        }
    }
    fn stroke(&mut self, color: &str, prims: Vec<Prim>, lw: f64, o: Option<Opts>) {
        self.cur.stroke(color, prims, lw, o);
    }
    fn fill(&mut self, color: &str, loops: Vec<Vec<f64>>, solid: bool, o: Option<Opts>) {
        self.cur.fill(color, loops, solid, o);
    }
    fn text(&mut self, color: &str, e: &Entity, o: Option<Opts>) {
        self.cur.text(color, e, o);
    }
    fn point(&mut self, color: &str, x: f64, y: f64) {
        self.cur.point(color, x, y);
    }
    fn viewport(&mut self, e: &Entity) {
        self.cur.viewport(e);
    }
    fn picture(&mut self, p: &PicOut) {
        self.cur.picture(p);
    }
}

fn same_map<K: PartialEq + Eq + std::hash::Hash + Clone, V: PartialEq>(a: &OMap<K, V>, b: &OMap<K, V>) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x == y)
}

/// What a walk reads from the document besides the entity itself: when any of it changes, every entity is walked
/// again.
#[derive(Clone)]
struct Ctx {
    layers: OMap<String, cc_cad::Layer>,
    linetypes: OMap<String, cc_cad::doc::Linetype>,
    text_styles: OMap<String, cc_cad::Obj>,
    dim_styles: OMap<String, cc_cad::Obj>,
    blocks: OMap<String, cc_cad::Block>,
    meta: cc_cad::Obj,
    /// a picture file came: its name text goes (walk leaves it out once the file is loaded)
    pictures: u64,
}

impl Ctx {
    fn of(doc: &Doc) -> Ctx {
        Ctx {
            layers: doc.layers.clone(),
            linetypes: doc.linetypes.clone(),
            text_styles: doc.text_styles.clone(),
            dim_styles: doc.dim_styles.clone(),
            blocks: doc.blocks.clone(),
            meta: doc.meta.clone(),
            pictures: doc.picture_version,
        }
    }
    fn matches(&self, doc: &Doc) -> bool {
        same_map(&self.layers, &doc.layers)
            && same_map(&self.linetypes, &doc.linetypes)
            && same_map(&self.text_styles, &doc.text_styles)
            && same_map(&self.dim_styles, &doc.dim_styles)
            && same_map(&self.blocks, &doc.blocks)
            && self.meta == doc.meta
            && self.pictures == doc.picture_version
    }
}

/// [`doc_drawing`] with each top-level entity's display kept between calls: after an edit only the entities that
/// changed are walked again (all of them when a layer, linetype, style, block or the drawing settings changed).
/// The result is identical to [`doc_drawing`] (tested).
#[derive(Default)]
pub struct DisplayCache {
    ctx: Option<Ctx>,
    ents: HashMap<(SpaceId, u64), (Entity, Piece)>,
    /// entities walked / reused by the last call
    pub walked: usize,
    pub reused: usize,
}

impl std::fmt::Debug for DisplayCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DisplayCache")
            .field("entities", &self.ents.len())
            .field("walked", &self.walked)
            .field("reused", &self.reused)
            .finish()
    }
}

impl DisplayCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// The document for the viewer, walking only what changed since the last call.
    pub fn drawing(&mut self, doc: &Doc) -> Drawing {
        if !self.ctx.as_ref().is_some_and(|c| c.matches(doc)) {
            self.ents.clear();
            self.ctx = Some(Ctx::of(doc));
        }
        self.walked = 0;
        self.reused = 0;
        let mut seen: HashMap<(SpaceId, u64), ()> = HashMap::with_capacity(self.ents.len());
        // the block editor (BEDIT) opens a space of its own: it shows in the model's place while it is open
        let loose = doc.space != cc_cad::MODEL && !doc.layouts.iter().any(|l| l.space == doc.space);
        let first = if loose { doc.space } else { cc_cad::MODEL };
        let mut spaces = vec![self.space(doc, first, "model".into(), "Model".into(), SpaceKind::Model, None, &mut seen)];
        for (i, l) in doc.layouts.iter().enumerate() {
            spaces.push(self.space(
                doc,
                l.space,
                format!("layout:{i}"),
                l.name.clone(),
                SpaceKind::Paper,
                paper_bounds(&l.paper),
                &mut seen,
            ));
        }
        self.ents.retain(|k, _| seen.contains_key(k));
        drawing_of(doc, spaces)
    }

    #[allow(clippy::too_many_arguments)]
    fn space(
        &mut self,
        doc: &Doc,
        space: SpaceId,
        key: String,
        name: String,
        kind: SpaceKind,
        paper: Option<Bounds>,
        seen: &mut HashMap<(SpaceId, u64), ()>,
    ) -> Space {
        // top-level objects grouped by layer (first-seen order), as space_of walks them
        let mut groups: Vec<(&str, Vec<&Entity>)> = Vec::new();
        let mut at: HashMap<&str, usize> = HashMap::new();
        for e in doc.space_entities(space).filter(|e| !matches!(e.geo, Geo::Xline(_) | Geo::Ray(_))) {
            let l = e.layer.as_deref().unwrap_or("0");
            let i = *at.entry(l).or_insert_with(|| {
                groups.push((l, Vec::new()));
                groups.len().saturating_sub(1)
            });
            if let Some((_, g)) = groups.get_mut(i) {
                g.push(e);
            }
        }
        let opts = display_opts();
        let mut batches: Vec<Batch> = Vec::new();
        let mut bounds = Bounds::default();
        let mut viewports: Vec<Viewport> = Vec::new();
        let mut images: Vec<SpaceImage> = Vec::new();
        for (layer, ents) in &groups {
            // walk the entities that are new or changed (an entity without an id is walked every time)
            let stale: Vec<&Entity> = ents
                .iter()
                .copied()
                .filter(|e| match e.id {
                    Some(id) => self.ents.get(&(space, id)).is_none_or(|(old, _)| old != *e),
                    None => true,
                })
                .collect();
            let mut fresh: Vec<Piece> = Vec::new();
            if !stale.is_empty() {
                let list: Vec<Entity> = stale.iter().map(|e| (*e).clone()).collect();
                let mut pc =
                    PieceCollect { layer: (*layer).to_string(), pieces: Vec::with_capacity(list.len()), cur: Collect::joined(layer) };
                walk(doc, &list, &mut pc, &opts);
                pc.flush();
                fresh = pc.pieces;
                self.walked = self.walked.saturating_add(list.len());
            }
            let mut fresh = fresh.into_iter();
            let start = batches.len();
            for e in ents {
                let piece = match e.id {
                    Some(id) if !stale.iter().any(|s| std::ptr::eq(*s, *e)) => {
                        self.reused = self.reused.saturating_add(1);
                        seen.insert((space, id), ());
                        self.ents.get(&(space, id)).map(|(_, p)| p.clone()).unwrap_or_default()
                    }
                    id => {
                        let p = fresh.next().unwrap_or_default();
                        if let Some(id) = id {
                            seen.insert((space, id), ());
                            self.ents.insert((space, id), ((*e).clone(), p.clone()));
                        }
                        p
                    }
                };
                for pb in piece.batches {
                    let slot = batches.iter_mut().skip(start).find(|b| b.color == pb.color && b.lw == pb.lw && b.layer == pb.layer);
                    match slot {
                        Some(b) => {
                            b.lines.extend(pb.lines);
                            b.fills.extend(pb.fills);
                            b.texts.extend(pb.texts);
                        }
                        None => batches.push(pb),
                    }
                }
                if !piece.bounds.is_empty() {
                    bounds.grow(&[piece.bounds.min_x, piece.bounds.min_y, piece.bounds.max_x, piece.bounds.max_y]);
                }
                viewports.extend(piece.viewports);
                images.extend(piece.images);
            }
        }
        batches.retain(|b| !(b.lines.is_empty() && b.fills.is_empty() && b.texts.is_empty()));
        let paper_unit = paper.map(|_| "mm".to_string());
        Space { key, name, kind, batches, bounds, paper, paper_unit, viewports, images }
    }
}

fn drawing_of(doc: &Doc, spaces: Vec<Space>) -> Drawing {
    let layers = doc
        .layers
        .values()
        .map(|l| {
            let color = match &l.color {
                cc_cad::entity::Color::Num(n) => color_of(&cc_cad::color::aci_hex(*n)),
                cc_cad::entity::Color::Str(s) => color_of(s),
                cc_cad::entity::Color::Null => Color::Ink,
            };
            Layer { name: l.name.clone(), color, on: l.on && !l.frozen, lw: 0.0, plot: l.plot }
        })
        .collect();
    let entities = doc.every_entity().count() as u64;
    // the picture files the drawing shows, with their uses (the viewer's `images`)
    let mut images: Vec<ImageRef> = Vec::new();
    for im in spaces.iter().flat_map(|s| s.images.iter()) {
        match images.iter_mut().find(|r| r.file == im.file) {
            Some(r) => r.uses = r.uses.saturating_add(1),
            None => images.push(ImageRef { name: cc_cad::pictures::base_name(&im.file).to_string(), file: im.file.clone(), uses: 1 }),
        }
    }
    Drawing {
        version: String::new(),
        release: String::new(),
        units: String::new(),
        xrefs: Vec::new(),
        images,
        layers,
        spaces,
        stats: Stats { entities, ..Stats::default() },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_cad::Block;
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
        assert!(m.batches.iter().all(|b| b.layer == "0"), "batches carry their layer");
        let circle = m.batches.iter().find(|b| b.color != Color::Ink).unwrap();
        assert_eq!(circle.lines[0].len(), 97 * 2, "a full circle: 96 segments");
        let texts: Vec<&Text> = m.batches.iter().flat_map(|b| &b.texts).collect();
        assert_eq!(texts.len(), 2);
        assert!(texts[1].y < texts[0].y, "the second line under the first");
        assert!(m.bounds.min_x <= 0.0 && m.bounds.max_x >= 10.0 && m.bounds.max_y > 20.0);
    }

    /// M4.52: the window dashes as 0.5's canvas (20,000 pieces per run, not 400 per object), each stroke's dashes in
    /// one polyline with PEN_UP between them, the same segments as one polyline per dash.
    #[test]
    fn dense_linetypes_dash_as_the_js_canvas() {
        let mut d = Doc::new();
        d.load(&json!({
            "meta": { "insunits": 1 },
            "layers": [{ "name": "0" }, { "name": "D", "linetype": "DOTTY" }],
            "linetypes": [{ "name": "DOTTY", "pattern": [0.5, -0.5] }],
            "entities": [
                { "type": "line", "x1": 0, "y1": 0, "x2": 1000, "y2": 0, "layer": "D" },
                { "type": "line", "x1": 0, "y1": 5, "x2": 10001, "y2": 5, "layer": "D" },
                { "type": "circle", "cx": 0, "cy": 50, "r": 10, "layer": "D" }
            ]
        }));
        let dr = doc_drawing(&d);
        let lines: Vec<&Vec<f64>> = dr.spaces[0].batches.iter().flat_map(|b| &b.lines).collect();
        assert_eq!(lines.len(), 3, "one polyline per stroke");
        let pieces = |l: &Vec<f64>| l.as_chunks::<2>().0.iter().filter(|p| cc_drawing::is_pen_up(p[0], p[1])).count() + 1;
        // 1000 long, period 1, two pieces per period: 2,000 ≤ 20,000 → 1,000 dashes (it was continuous past 400)
        assert_eq!(pieces(lines[0]), 1000);
        // 10001 long: 20,002 pieces > 20,000 → continuous, as linetypes.js
        assert_eq!(pieces(lines[1]), 1);
        assert_eq!(lines[1].len(), 4);
        assert!(pieces(lines[2]) > 1, "the circle is dashed");
        // the same segments a polyline per dash gives
        let mut sep = Collect::new("D");
        walk(&d, &d.model().cloned().collect::<Vec<_>>(), &mut sep, &display_opts());
        let segs = |ls: &mut dyn Iterator<Item = &Vec<f64>>| -> usize {
            ls.map(|l| l.as_chunks::<2>().0.split(|p| cc_drawing::is_pen_up(p[0], p[1])).map(|r| r.len().saturating_sub(1)).sum::<usize>())
                .sum()
        };
        let joined = segs(&mut dr.spaces[0].batches.iter().flat_map(|b| &b.lines));
        let separate = segs(&mut sep.batches.iter().flat_map(|b| &b.lines));
        assert_eq!(joined, separate);
        assert!(sep.batches.iter().flat_map(|b| &b.lines).all(|l| l.iter().all(|v| v.is_finite())), "previews: no PEN_UP");
    }

    #[test]
    fn the_cache_draws_what_a_full_walk_draws() {
        let mut d = Doc::new();
        d.load(&json!({
            "layers": [{ "name": "0" }, { "name": "A", "color": 3, "linetype": "DASHED" }],
            "linetypes": [{ "name": "DASHED", "pattern": [0.5, -0.25] }],
            "blocks": [{ "name": "B", "base": { "x": 0, "y": 0 }, "entities": [
                { "type": "line", "x1": 0, "y1": 0, "x2": 1, "y2": 1 }, { "type": "circle", "cx": 0, "cy": 0, "r": 0.5, "color": 0 } ] }],
            "entities": [
                { "type": "line", "x1": 0, "y1": 0, "x2": 10, "y2": 0 },
                { "type": "circle", "cx": 5, "cy": 5, "r": 2, "color": 1, "layer": "A" },
                { "type": "text", "x": 0, "y": 20, "h": 2, "s": "A\nB" },
                { "type": "insert", "block": "B", "x": 3, "y": 3, "color": 5 },
                { "type": "poly", "closed": true, "pts": [{ "x": 0, "y": 0 }, { "x": 4, "y": 0, "b": 0.5 }, { "x": 4, "y": 4 }], "layer": "A" },
                { "type": "hatch", "solid": true, "loops": [[0, 0, 2, 0, 2, 2]], "color": 2 }
            ]
        }));
        let mut cache = DisplayCache::new();
        let check = |cache: &mut DisplayCache, d: &Doc, what: &str| {
            // compared as text: a PEN_UP pair holds a NaN, which is never == itself
            assert_eq!(format!("{:?}", cache.drawing(d)), format!("{:?}", doc_drawing(d)), "{what}");
        };
        check(&mut cache, &d, "first build");
        assert_eq!(cache.reused, 0);
        check(&mut cache, &d, "unchanged");
        assert_eq!(cache.walked, 0, "nothing changed: nothing walked");
        let first = d.model().next().and_then(|e| e.id).unwrap();
        d.change("move", |d| {
            let mut e = d.get(first).cloned().unwrap();
            if let Geo::Line(l) = &mut e.geo {
                l.x2 = 20.0;
            }
            d.update(first, e)?;
            d.add(Entity::new(Geo::Circle(cc_cad::entity::Circle { cx: 1.0, cy: 1.0, r: 1.0 })))
        })
        .unwrap();
        check(&mut cache, &d, "one moved, one added");
        assert_eq!(cache.walked, 2, "{cache:?}");
        d.change("erase", |d| d.remove(first)).unwrap();
        check(&mut cache, &d, "erased");
        d.undo();
        check(&mut cache, &d, "undo");
        d.change("layer", |d| d.set_layer("A", json!({ "color": 4 }).as_object().unwrap(), false)).unwrap();
        check(&mut cache, &d, "a layer colour");
        assert_eq!(cache.reused, 0, "a layer change walks everything");
        let b = Block::new(
            "B",
            cc_geom::P2::new(0.0, 0.0),
            vec![Entity::new(Geo::Circle(cc_cad::entity::Circle { cx: 0.0, cy: 0.0, r: 3.0 }))],
        );
        d.change("block", |d| d.define_block(&b)).unwrap();
        check(&mut cache, &d, "a block redefined");
        d.set_space(Some("Layout1"));
        check(&mut cache, &d, "other space");
    }
}
