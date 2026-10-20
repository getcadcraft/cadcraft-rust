//! BimCraft's library items placed from the plan (bimcraft.js `startPlacing` / `moveGhost` / `commitPlace` /
//! `askPlacement` / `typedPlace` / `toggleSnap` / `stopPlacing`; the placing itself is ifc-place.js through
//! `cc_bim::draw::place`). A library item file (libitem.js format 1, `*.item.json` from LibraryCraft or a feed) opened
//! or dropped while an IFC model is being edited starts placing it on the level the plan shows: the pointer carries its
//! footprint, R turns it 90°, Tab snaps its back to the nearest wall face, a click (or X, Y typed in model millimetres)
//! asks for the item number, quantity, notes (and a stand-in height when the item has no 3D body), and places it — one
//! change, Ctrl+Z takes it back; Esc stops. 0.5 places from its 3D view turned to the top; here the plan is that view.
//!
//! Everything is a command: `bim.place.pick`, `bim.place.turn`, `bim.place.snap`, `bim.place.at`, `bim.place.ok`,
//! `bim.place.cancel`, `bim.place.stop`; clicks on the plan are `bim.point` as for the other tools.

use std::sync::Arc;

use cc_bim::draw::m::{self, V2};
use cc_bim::draw::openings::Symbol;
use cc_bim::draw::place::{Face, PlaceOpts, box_from_plan, plan_bounds, plan_polylines, snap_to_wall, wall_faces};
use cc_bim::util::{M4, apply4, inv4};
use egui::RichText;
use serde_json::Value as J;

use crate::bimplan::Overlay;
use crate::bimtools::bt;
use crate::{ImportedFile, Params, UiApp};

/// The placing commands (id, label, shortcut, params doc).
pub const COMMANDS: &[(&str, &str, &str, &str)] = &[
    ("bim.place.pick", "Place a library item", "", "{} → pick a library item file (*.item.json) and start placing it on the plan"),
    ("bim.place.turn", "Turn 90°", "R", "{} → the item being placed turns 90°"),
    ("bim.place.snap", "Wall snap", "Tab", "{} → the item's back snaps to the nearest wall face (on / off)"),
    ("bim.place.at", "Place at X, Y", "", "{name: \"1200, 3400\"} → model mm: place it there (asks for the item number first)"),
    (
        "bim.place.ok",
        "Place it",
        "Enter",
        "{name?: \"<item number>|<quantity>|<stand-in height>|<notes>\"} → the placement dialog's Place (empty parts keep the dialog's values)",
    ),
    ("bim.place.cancel", "Cancel the placement", "", "{} → the placement dialog's Cancel"),
    ("bim.place.stop", "Stop placing", "Esc", "{}"),
];

/// A library entry: the item and where it came from (library-view.js `{ item, meta }`).
#[derive(Clone, Debug)]
pub struct LibEntry {
    pub item: Arc<J>,
    pub name: String,
    pub library: String,
    pub sha256: String,
    pub source_file: String,
}

/// Where the item would go: model mm, degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct At {
    pub x: f64,
    pub y: f64,
    pub rot: f64,
    pub snapped: bool,
}

/// An item being placed (bimcraft.js `placing`).
#[derive(Clone, Debug)]
pub struct Placing {
    pub entry: LibEntry,
    pub rot: f64,
    pub snap: bool,
    pub faces: Option<Vec<Face>>,
    /// the pointer's last plan point
    pub last: Option<V2>,
    pub at: Option<At>,
    pub storey: f64,
    /// the storey's world (mm) and its inverse: plan ↔ model
    pub sw: M4,
    pub swi: M4,
}

/// The placement dialog (`askPlacement`).
#[derive(Clone, Debug)]
pub struct Ask {
    pub at: At,
    pub next: String,
    pub number: String,
    pub qty: String,
    pub notes: String,
    /// Some when the item has no 3D body (the stand-in box's height, mm)
    pub height: Option<String>,
}

fn status(app: &mut UiApp, text: String) {
    app.message = text;
    app.error = None;
}

/// A library item file (format 1)?
pub fn is_item(name: &str, bytes: &[u8]) -> bool {
    let lower = name.to_ascii_lowercase();
    if !lower.ends_with(".json") || bytes.len() > 64 << 20 {
        return false;
    }
    serde_json::from_slice::<J>(bytes).is_ok_and(|v| v.get("format").and_then(J::as_str) == Some("cadcraft-library-item"))
}

/// `itemHash(item)`: sha256 (hex) of its canonical JSON without `provenance`.
pub fn item_hash(item: &J) -> String {
    cc_io_takeoff::pdfread::sha256_hex(cc_bim::draw::place::item_hash_input(item).as_bytes())
}

/// A library item file opened or dropped: placing starts when an IFC model is being edited → it was one.
pub fn open_item(app: &mut UiApp, f: &ImportedFile) -> bool {
    if !is_item(&f.name, &f.bytes) {
        return false;
    }
    let Ok(item) = serde_json::from_slice::<J>(&f.bytes) else { return false };
    let prov = item.get("provenance").and_then(|p| p.get("sha256")).and_then(J::as_str).filter(|s| !s.is_empty()).map(str::to_string);
    let entry = LibEntry {
        name: item.get("name").and_then(J::as_str).unwrap_or("").to_string(),
        sha256: prov.unwrap_or_else(|| item_hash(&item)),
        item: Arc::new(item),
        library: "file".into(),
        source_file: std::path::Path::new(&f.name).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
    };
    start(app, entry);
    true
}

/// `startPlacing(entry)`
pub fn start(app: &mut UiApp, entry: LibEntry) {
    if app.bimh.ed.is_none() || app.ifc3_bytes().is_none() {
        app.error = Some(bt("place.need.ifc", &[]));
        return;
    }
    if !app.bimh.draw.on {
        crate::bimplan::show(app, true);
    }
    let Some(level) = app.bimh.draw.level else {
        app.error = Some(bt("place.need.storeys", &[]));
        return;
    };
    let Some(doc) = app.bimh.ed.as_mut() else { return };
    let sw = match cc_bim::draw::writer::W::new(&mut doc.ed.api).and_then(|mut w| w.storey_world(level)) {
        Ok(m) => m,
        Err(e) => {
            app.error = Some(e.0);
            return;
        }
    };
    let Some(swi) = inv4(&sw) else {
        app.error = Some(bt("place.need.storeys", &[]));
        return;
    };
    let name = entry.name.clone();
    app.bimh.draw.chain = None;
    app.bimh.draw.drag = None;
    app.bimh.draw.ask = None;
    app.bimh.draw.placing = Some(Placing { entry, rot: 0.0, snap: false, faces: None, last: None, at: None, storey: level, sw, swi });
    status(app, bt("place.start", &[("name", &name)]));
}

/// The storey changed while placing (the place bar's storey: here the plan's level): faces again, snap off.
pub fn level_changed(app: &mut UiApp) {
    let Some(level) = app.bimh.draw.level else { return };
    if app.bimh.draw.placing.as_ref().is_none_or(|p| p.storey == level) {
        return;
    }
    let Some(doc) = app.bimh.ed.as_mut() else { return };
    let Ok(sw) = cc_bim::draw::writer::W::new(&mut doc.ed.api).and_then(|mut w| w.storey_world(level)) else { return };
    let Some(swi) = inv4(&sw) else { return };
    if let Some(pl) = app.bimh.draw.placing.as_mut() {
        pl.storey = level;
        pl.sw = sw;
        pl.swi = swi;
        pl.faces = None;
        pl.snap = false;
    }
    move_ghost(app);
}

/// `stopPlacing()`
pub fn stop(app: &mut UiApp) {
    app.bimh.draw.placing = None;
    app.bimh.draw.ask = None;
}

/// `moveGhost(pt)`: where the item goes for the pointer's plan point (snapped to a wall face within 3 m when on).
fn move_ghost(app: &mut UiApp) {
    let Some(pl) = app.bimh.draw.placing.as_mut() else { return };
    let Some(p) = pl.last else {
        pl.at = None;
        return;
    };
    let [x, y, _] = apply4(&pl.sw, p[0], p[1], 0.0);
    let mut at = At { x, y, rot: pl.rot, snapped: false };
    if pl.snap
        && let Some(faces) = pl.faces.as_ref().filter(|f| !f.is_empty())
        && let Some((sx, sy, rot, d)) = snap_to_wall([x, y], &pl.entry.item, faces)
        && d < 3000.0
    {
        at = At { x: sx, y: sy, rot, snapped: true };
    }
    pl.at = Some(at);
}

/// The pointer over the plan while placing → handled.
pub fn on_move(app: &mut UiApp, p: V2) -> bool {
    let Some(pl) = app.bimh.draw.placing.as_mut() else { return false };
    if app.bimh.draw.ask.is_none() {
        pl.last = Some(p);
        move_ghost(app);
    }
    true
}

/// A click on the plan while placing: the item goes where the ghost is (after the dialog) → handled.
pub fn on_click(app: &mut UiApp, p: V2) -> bool {
    if app.bimh.draw.placing.is_none() {
        return false;
    }
    if app.bimh.draw.ask.is_some() {
        return true;
    }
    on_move(app, p);
    if let Some(at) = app.bimh.draw.placing.as_ref().and_then(|pl| pl.at) {
        ask(app, at);
    }
    true
}

/// `askPlacement(item, next)`: the dialog, its fields filled.
fn ask(app: &mut UiApp, at: At) {
    let Some(pl) = app.bimh.draw.placing.as_ref() else { return };
    let no_body = !pl.entry.item.get("body").is_some_and(|b| !b.is_null() && b != &J::Bool(false));
    let Some(doc) = app.bimh.ed.as_mut() else { return };
    let next = match cc_bim::draw::place::next_item_number(&mut doc.ed.api) {
        Ok(n) => n,
        Err(e) => {
            app.error = Some(e.0);
            return;
        }
    };
    app.bimh.draw.ask =
        Some(Ask { at, number: next.clone(), next, qty: "1".into(), notes: String::new(), height: no_body.then(|| "900".into()) });
}

/// The dialog's Place: `commitPlace(at)` with what it says.
fn commit(app: &mut UiApp) -> Result<(), String> {
    let Some(a) = app.bimh.draw.ask.take() else { return Ok(()) };
    let Some(pl) = app.bimh.draw.placing.clone() else { return Ok(()) };
    let js_num = |s: &str| cc_bim::js::str_num(s);
    let n = cc_cad::js::trim(&a.number).to_string();
    let item_number = if n.is_empty() { a.next.clone() } else { n };
    let q = js_num(&a.qty);
    let q = if q != 0.0 && !q.is_nan() { q } else { 1.0 };
    let quantity = m::max(1.0, m::round(q));
    let stand_in_height = match &a.height {
        Some(h) => {
            let v = js_num(h);
            m::max(1.0, if v != 0.0 && !v.is_nan() { v } else { 900.0 })
        }
        None => 0.0,
    };
    let opts = PlaceOpts {
        storey: pl.storey,
        x: a.at.x,
        y: a.at.y,
        rot: a.at.rot,
        item_number: item_number.clone(),
        quantity,
        notes: cc_cad::js::trim(&a.notes).to_string(),
        library: pl.entry.library.clone(),
        sha256: pl.entry.sha256.clone(),
        source_file: pl.entry.source_file.clone(),
        stand_in_height,
    };
    let Some(doc) = app.bimh.ed.as_mut() else { return Ok(()) };
    let t0 = web_time::Instant::now();
    let r = doc.ed.place_item(pl.entry.item.clone(), opts).map_err(|e| e.0)?;
    app.bimh.draw.timings.insert("change", t0.elapsed().as_secs_f64() * 1000.0);
    crate::bimedit::after_change(app, Some(r.id), true);
    let key = if r.type_created { "place.done" } else { "place.done.shared" };
    status(app, bt(key, &[("name", &pl.entry.name), ("no", &item_number)]));
    if let Some(p) = app.bimh.draw.placing.as_mut() {
        p.faces = None;
        if p.snap {
            let f = faces(app);
            if let Some(p) = app.bimh.draw.placing.as_mut() {
                p.faces = Some(f);
            }
        }
    }
    Ok(())
}

/// `wallFaces(storey)`: the faces of the walls on the storey, from the model's geometry (model mm).
fn faces(app: &mut UiApp) -> Vec<Face> {
    let Some(bytes) = app.ifc3_bytes() else { return Vec::new() };
    let Some(storey) = app.bimh.draw.placing.as_ref().map(|p| p.storey) else { return Vec::new() };
    let Some(doc) = app.bimh.ed.as_mut() else { return Vec::new() };
    let api = &mut doc.ed.api;
    let mut walls = api.ids("IFCWALL", true);
    if walls.is_empty() {
        walls = api.ids("IFCWALL", false);
        walls.extend(api.ids("IFCWALLSTANDARDCASE", false));
    }
    let so = match cc_bim::draw::writer::W::new(api).and_then(|w| w.storey_of()) {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };
    let Ok(v) = cc_io_ifc::view::view(&bytes, &cc_io_ifc::geo::bool_ops::ManifoldOps) else { return Vec::new() };
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for w in walls {
        let k = cc_bim::draw::plan::key_u64(w);
        if !seen.insert(k) || so.get(&cc_io_ifc::model::id_key(w)) != Some(&storey) {
            continue;
        }
        let mut pts = Vec::new();
        for pc in v.pieces.iter().filter(|p| p.product == w) {
            let Some(mesh) = v.meshes.get(pc.mesh) else { continue };
            for c in mesh.pos.as_chunks::<3>().0 {
                // GetVertexArray hands out float32 vertices (file units, the mesh's own frame)
                let f = |v: f64| f64::from(v as f32);
                let q = apply4(&pc.matrix, f(c[0]), f(c[1]), f(c[2]));
                pts.push([q[0] * 1000.0, q[1] * 1000.0]);
            }
        }
        out.extend(wall_faces(&pts));
    }
    out
}

/// Turn 90° (R; the button). Snap goes off, as in 0.5.
fn turn(app: &mut UiApp) {
    if let Some(pl) = app.bimh.draw.placing.as_mut() {
        pl.rot = (pl.rot + 90.0) % 360.0;
        pl.snap = false;
    }
    move_ghost(app);
}

/// `toggleSnap()` (Tab; the button).
fn toggle_snap(app: &mut UiApp) -> Result<(), String> {
    let Some(pl) = app.bimh.draw.placing.as_mut() else { return Ok(()) };
    pl.snap = !pl.snap;
    if pl.snap && pl.faces.is_none() {
        let f = faces(app);
        let none = f.is_empty();
        if let Some(pl) = app.bimh.draw.placing.as_mut() {
            pl.faces = Some(f);
            if none {
                pl.snap = false;
            }
        }
        if none {
            move_ghost(app);
            return Err(bt("place.nowalls", &[]));
        }
    }
    move_ghost(app);
    Ok(())
}

/// `typedPlace()`: "1200, 3400" (model mm) → the dialog for that point.
fn typed(app: &mut UiApp, text: &str) -> Result<(), String> {
    let Some(pl) = app.bimh.draw.placing.as_ref() else { return Ok(()) };
    let rot = pl.rot;
    let parse = |s: &str| -> Option<(f64, f64)> {
        // ^\s*(-?\d+(?:\.\d+)?)\s*[,;\s]\s*(-?\d+(?:\.\d+)?)\s*$
        let t = s.trim();
        let num_ok = |x: &str| {
            let x = x.strip_prefix('-').unwrap_or(x);
            let (a, b) = x.split_once('.').map_or((x, None), |(a, b)| (a, Some(b)));
            !a.is_empty()
                && a.bytes().all(|c| c.is_ascii_digit())
                && b.is_none_or(|b| !b.is_empty() && b.bytes().all(|c| c.is_ascii_digit()))
        };
        let cut = t.find([',', ';', ' ', '\t'])?;
        let (a, rest) = t.split_at(cut);
        let a = a.trim_end();
        let rest = rest.trim_start();
        let b = rest.strip_prefix([',', ';']).unwrap_or(rest).trim_start();
        (num_ok(a) && num_ok(b)).then(|| (cc_bim::js::str_num(a), cc_bim::js::str_num(b)))
    };
    let Some((x, y)) = parse(text) else { return Err(bt("place.typed.bad", &[])) };
    let at = At { x, y, rot, snapped: false };
    if let Some(pl) = app.bimh.draw.placing.as_mut() {
        pl.at = Some(at);
    }
    ask(app, at);
    Ok(())
}

/// Run a placing command.
pub fn run(app: &mut UiApp, id: &str, p: &Params) -> Result<(), String> {
    let name = p.name.clone().unwrap_or_default();
    match id {
        "bim.place.pick" => {
            let Some(pick) = app.services.pick_open.as_mut() else {
                if let Some(f) = app.services.open_async.as_mut() {
                    f();
                }
                return Ok(());
            };
            match pick() {
                Some(Ok(f)) => {
                    if !open_item(app, &f) {
                        return Err(format!("{}: not a library item (cadcraft-library-item JSON)", f.name));
                    }
                }
                Some(Err(e)) => return Err(e),
                None => {}
            }
        }
        "bim.place.turn" => turn(app),
        "bim.place.snap" => toggle_snap(app)?,
        "bim.place.at" => typed(app, &name)?,
        "bim.place.ok" => {
            if let Some(a) = app.bimh.draw.ask.as_mut()
                && !name.is_empty()
            {
                let mut it = name.splitn(4, '|');
                for (i, part) in [0, 1, 2, 3].into_iter().zip(&mut it) {
                    if part.is_empty() {
                        continue;
                    }
                    match i {
                        0 => a.number = part.into(),
                        1 => a.qty = part.into(),
                        2 => {
                            if a.height.is_some() {
                                a.height = Some(part.into());
                            }
                        }
                        _ => a.notes = part.into(),
                    }
                }
            }
            commit(app)?;
        }
        "bim.place.cancel" => app.bimh.draw.ask = None,
        "bim.place.stop" => {
            if app.bimh.draw.placing.is_some() {
                stop(app);
                status(app, bt("place.stopped", &[]));
            }
        }
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}

/// The ghost on the plan: the item's footprint and its box where it would go.
pub fn overlay(dr: &crate::bimplan::Draw, o: &mut Overlay) {
    let Some(pl) = dr.placing.as_ref() else { return };
    let Some(at) = dr.ask.as_ref().map(|a| a.at).or(pl.at) else { return };
    let item = &pl.entry.item;
    let r = at.rot * std::f64::consts::PI / 180.0;
    let (c, s) = (m::cos(r), m::sin(r));
    // item → model → plan
    let to_plan = |lx: f64, ly: f64| -> V2 {
        let (x, y) = (at.x + lx * c - ly * s, at.y + lx * s + ly * c);
        let q = apply4(&pl.swi, x, y, pl.sw[14]);
        [q[0], q[1]]
    };
    let mut sym = Symbol::default();
    for line in plan_polylines(item.get("plan")) {
        for w in line.windows(2) {
            if let [a, b] = w {
                let (p, q) = (to_plan(a[0], a[1]), to_plan(b[0], b[1]));
                sym.thin.push([p[0], p[1], q[0], q[1]]);
            }
        }
    }
    // the box the ghost shows (clipboard.js boxFromItem: the body's, else the plan's with a stand-in height)
    let bx = match item.get("body") {
        Some(b) if b.get("type").and_then(J::as_str) == Some("box") => {
            let n = |k: &str| b.get(k).and_then(J::as_f64).unwrap_or(0.0);
            Some([n("x"), n("y"), n("x") + n("w"), n("y") + n("d")])
        }
        Some(b) if b.get("positions").is_some() => {
            let ps: Vec<f64> =
                b.get("positions").and_then(J::as_array).map(|a| a.iter().filter_map(J::as_f64).collect()).unwrap_or_default();
            let mut bb = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
            for q in ps.as_chunks::<3>().0 {
                bb = [bb[0].min(q[0]), bb[1].min(q[1]), bb[2].max(q[0]), bb[3].max(q[1])];
            }
            bb[0].is_finite().then_some(bb)
        }
        _ => box_from_plan(item.get("plan"), 900.0, 0.0).map(|b| [b.x, b.y, b.x + b.w, b.y + b.d]),
    }
    .or_else(|| plan_bounds(item.get("plan")));
    if let Some([x0, y0, x1, y1]) = bx {
        let k = [to_plan(x0, y0), to_plan(x1, y0), to_plan(x1, y1), to_plan(x0, y1)];
        for (a, b) in k.iter().zip(k.iter().cycle().skip(1)) {
            sym.dashed.push([a[0], a[1], b[0], b[1]]);
        }
        // the front (−Y) edge solid: which way it faces
        sym.lines.push([k[0][0], k[0][1], k[1][0], k[1][1]]);
    }
    o.symbols.push(sym);
    let key = if at.snapped { "place.at.wall" } else { "place.at" };
    let text = bt(
        key,
        &[
            ("x", &cc_bim::js::num_str(m::round(at.x))),
            ("y", &cc_bim::js::num_str(m::round(at.y))),
            ("rot", &cc_bim::js::num_str(m::round(at.rot))),
        ],
    );
    o.hud = Some((to_plan(0.0, 0.0), text));
}

/// The place bar under the model bar while placing (0.5's `.bc-placebar`).
pub fn bar(app: &mut UiApp, ui: &mut egui::Ui) {
    let Some(pl) = app.bimh.draw.placing.as_ref() else { return };
    let t = app.tokens;
    let mut run_ids: Vec<(&'static str, String)> = Vec::new();
    let name = pl.entry.name.clone();
    let snap = pl.snap;
    let at_text = match pl.at {
        Some(a) => bt(
            if a.snapped { "place.at.wall" } else { "place.at" },
            &[
                ("x", &cc_bim::js::num_str(m::round(a.x))),
                ("y", &cc_bim::js::num_str(m::round(a.y))),
                ("rot", &cc_bim::js::num_str(m::round(a.rot))),
            ],
        ),
        None => bt("place.rot", &[("rot", &cc_bim::js::num_str(m::round(pl.rot)))]),
    };
    let mut xy = app.bimh.draw.place_xy.clone();
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(bt("place.bar.placing", &[("name", &name)])).strong());
        ui.label(RichText::new(bt("ui.place.hint", &[])).small().color(t.text_dim));
    });
    ui.horizontal_wrapped(|ui| {
        let l = ui.label(bt("ui.place.xy", &[]));
        let r = ui
            .add(egui::TextEdit::singleline(&mut xy).desired_width(110.0).hint_text("1200, 3400").id_salt("bim_place_xy"))
            .labelled_by(l.id);
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if ui.button(bt("ui.place.here", &[])).clicked() || enter {
            run_ids.push(("bim.place.at", xy.clone()));
        }
        if ui.add(egui::Button::new(bt("ui.place.snap", &[])).selected(snap)).on_hover_text(bt("ui.place.snap.title", &[])).clicked() {
            run_ids.push(("bim.place.snap", String::new()));
        }
        ui.label(RichText::new(at_text).color(t.text_dim));
        if ui.button(bt("place.turn", &[])).on_hover_text(bt("place.turn.title", &[])).clicked() {
            run_ids.push(("bim.place.turn", String::new()));
        }
        if ui.button(bt("ui.place.stop", &[])).clicked() {
            run_ids.push(("bim.place.stop", String::new()));
        }
    });
    app.bimh.draw.place_xy = xy;
    for (id, arg) in run_ids {
        app.run_ui(id, &Params { name: (!arg.is_empty()).then_some(arg), ..Default::default() });
    }
}

/// The placement dialog (`askPlacement`): item number, quantity, notes, the stand-in height; Place / Cancel.
pub fn dialog(app: &mut UiApp, ctx: &egui::Context) {
    let Some(mut a) = app.bimh.draw.ask.clone() else { return };
    let name = app.bimh.draw.placing.as_ref().map(|p| p.entry.name.clone()).unwrap_or_default();
    let mut act: Option<&'static str> = None;
    egui::Window::new(bt("place.dlg.title", &[("name", &name)]))
        .min_width(440.0)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            let field = |ui: &mut egui::Ui, key: &str, v: &mut String, id: &str| {
                let l = ui.label(bt(key, &[]));
                ui.add(egui::TextEdit::singleline(v).desired_width(240.0).id_salt(id)).labelled_by(l.id);
            };
            field(ui, "place.dlg.number", &mut a.number, "bim_place_no");
            field(ui, "place.dlg.qty", &mut a.qty, "bim_place_qty");
            field(ui, "place.dlg.notes", &mut a.notes, "bim_place_notes");
            if let Some(h) = a.height.as_mut() {
                field(ui, "place.dlg.height", h, "bim_place_h");
            }
            ui.horizontal(|ui| {
                if ui.button(bt("place.dlg.ok", &[])).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    act = Some("bim.place.ok");
                }
                if ui.button(bt("dialog.cancel", &[])).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    act = Some("bim.place.cancel");
                }
            });
        });
    app.bimh.draw.ask = Some(a);
    if let Some(id) = act {
        app.run_ui(id, &Params::default());
    }
}

/// Keys while placing (bimcraft.js keydown): Esc stops, R turns, Tab snaps → taken.
pub fn keys(app: &mut UiApp, ctx: &egui::Context) -> bool {
    if app.bimh.draw.placing.is_none() || app.bimh.draw.ask.is_some() {
        return false;
    }
    let (esc, r, tab) =
        ctx.input(|i| (i.key_pressed(egui::Key::Escape), i.key_pressed(egui::Key::R) && !i.modifiers.any(), i.key_pressed(egui::Key::Tab)));
    if esc {
        app.run_ui("bim.place.stop", &Params::default());
        return true;
    }
    if ctx.egui_wants_keyboard_input() {
        return false;
    }
    if r {
        app.run_ui("bim.place.turn", &Params::default());
    }
    if tab {
        app.run_ui("bim.place.snap", &Params::default());
    }
    r || tab
}

#[cfg(test)]
mod tests {
    use crate::{Params, UiApp};

    fn run(app: &mut UiApp, id: &str, name: &str) -> Result<(), String> {
        app.run(id, &Params { name: (!name.is_empty()).then(|| name.into()), ..Default::default() })
    }
    fn count(app: &UiApp, up: &str) -> usize {
        app.bimh.ed.as_ref().map_or(0, |d| d.ed.api.ids(up, false).len())
    }

    #[test]
    fn library_items_are_placed_from_the_plan() {
        let mut app = UiApp::default();
        for (id, n) in [("bim.clock", "1791979200000"), ("bim.new", "Lib|2|3000 mm"), ("bim.units", "mm"), ("bim.tool", "wall")] {
            run(&mut app, id, n).unwrap();
        }
        for p in ["0,0", "6000,0", "6000,4000", "0,4000", "0,0"] {
            run(&mut app, "bim.typed", p).unwrap();
        }
        let item = br#"{"format":"cadcraft-library-item","format_version":1,"name":"Desk","ifc":{"class":"IfcFurniture"},
            "body":{"type":"box","x":-600,"y":-700,"z":0,"w":1200,"d":700,"h":740},
            "plan":{"entities":[{"type":"poly","closed":true,"pts":[[-600,-700],[600,-700],[600,0],[-600,0]]}]}}"#;
        // not an item: not taken; an item: placing starts, the model stays
        assert!(!super::is_item("x.json", b"{\"format\":\"other\"}"));
        app.open_bytes("desk.item.json", item);
        assert!(app.bimh.draw.placing.is_some());
        assert!(app.message.contains("Desk"), "{}", app.message);
        run(&mut app, "bim.point", "3000,2000").unwrap();
        let ask = app.bimh.draw.ask.clone().unwrap();
        assert_eq!((ask.number.as_str(), ask.qty.as_str(), ask.height.is_none()), ("1", "1", true));
        run(&mut app, "bim.place.ok", "").unwrap();
        assert_eq!(count(&app, "IFCFURNITURE"), 1);
        assert!(app.message.starts_with("Placed Desk as item 1"), "{}", app.message);
        // turned, typed, the second shares the type; a bad typed point is refused
        run(&mut app, "bim.place.turn", "").unwrap();
        assert!(run(&mut app, "bim.place.at", "12 x").is_err());
        run(&mut app, "bim.place.at", "1200, 3400").unwrap();
        run(&mut app, "bim.place.ok", "K-7|2||by the window").unwrap();
        assert_eq!(count(&app, "IFCFURNITURE"), 2);
        assert_eq!(count(&app, "IFCFURNITURETYPE"), 1);
        assert!(app.message.contains("item K-7"), "{}", app.message);
        // Tab: the back against the nearest wall face
        run(&mut app, "bim.place.snap", "").unwrap();
        run(&mut app, "bim.point", "5800,2000").unwrap();
        let at = app.bimh.draw.ask.as_ref().unwrap().at;
        assert!(at.snapped && (at.rot - 270.0).abs() < 1e-9, "{at:?}");
        run(&mut app, "bim.place.cancel", "").unwrap();
        // one undo step per placement; Esc stops
        run(&mut app, "bim.undo", "").unwrap();
        assert_eq!(count(&app, "IFCFURNITURE"), 1);
        run(&mut app, "bim.place.stop", "").unwrap();
        assert!(app.bimh.draw.placing.is_none());
        assert_eq!(app.message, crate::bimtools::bt("place.stopped", &[]));
    }
}
