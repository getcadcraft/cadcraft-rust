//! BimCraft's grips in the storey plan (model-ui.js `gripWall` / `onDown` / `gripMove` / `onMove` / `onUp`): with
//! Select and one drawn wall picked, its ends (squares) and its middle (a circle) are grips. Dragging an end moves that
//! end (snapping to the other walls' ends, middles and face corners, 15° steps from the fixed end, Shift square,
//! free lengths in 10 mm or 1/2"); dragging the middle moves the whole wall (steps of 90° from where it was taken).
//! Dragging a door or window slides it along its wall, or onto another wall (the middle of a wall snaps, the rest in
//! the round step), its distances to the face's ends shown. A drag is one change through the editor: one undo step.
//!
//! Gestures are commands: `bim.down` / `bim.move` / `bim.up` (or `bim.drag` for the whole press-move-release), plan
//! points in mm, so a script or the shot tool drags exactly what a hand would.

use cc_bim::draw::m::{self, V2};
use cc_bim::draw::openings::{Opening, OpeningChange, opening_type, opening_type_name};
use cc_bim::draw::ops::Op;
use cc_bim::draw::plan::Dim;
use cc_bim::draw::tools::{Snap, SnapOpts, SnapWall, snap_point};
use cc_bim::draw::units::fmt_len;
use cc_bim::draw::walls::{Seg, Wall, WallChange, outline_of, wall_outlines};

use crate::UiApp;
use crate::bimplan::{Overlay, SNAP_PX};
use crate::bimtools::bt;

/// Which grip of a wall.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    A,
    B,
    Mid,
}

/// Where a grip drag puts the wall: its ends and the snap that placed the dragged point.
#[derive(Clone, Debug)]
pub struct GripAt {
    pub a: V2,
    pub b: V2,
    pub snap: Snap,
}

/// A press the plan took (model-ui.js `S.drag`): a wall grip, or a door / window being slid.
#[derive(Clone, Debug)]
pub enum PDrag {
    Grip { wall: Wall, end: End, from: V2, key: f64, at: Option<GripAt> },
    Opening { op: Opening, key: f64, from: V2, moved: bool },
}

impl PDrag {
    fn from(&self) -> V2 {
        match self {
            PDrag::Grip { from, .. } | PDrag::Opening { from, .. } => *from,
        }
    }
}

/// `gripWall()`: the picked drawn wall of the level shown, when exactly one element is picked.
pub fn grip_wall(dr: &crate::bimplan::Draw, sel: Option<f64>) -> Option<Wall> {
    let k = sel?;
    crate::bimplan::walls_here(dr).into_iter().find(|w| w.id == k)
}

/// `gripsOf(w)`: the ends and the middle.
pub fn grips_of(w: &Wall) -> [(V2, End); 3] {
    [(w.a, End::A), (w.b, End::B), ([(w.a[0] + w.b[0]) / 2.0, (w.a[1] + w.b[1]) / 2.0], End::Mid)]
}

/// `onDown(p, e)`: a press on a grip of the picked wall, or on a door / window, starts a drag → taken (no pan).
pub fn on_down(app: &mut UiApp, p: V2) -> bool {
    if app.bimh.draw.tool != "select" || app.bimh.draw.placing.is_some() {
        return false;
    }
    let sel = app.picked_product();
    let dr = &mut app.bimh.draw;
    if let Some(gw) = grip_wall(dr, sel) {
        let tol = dr.view.px_to_mm(9.0);
        if let Some(&(_, end)) = grips_of(&gw).iter().find(|(q, _)| m::hypot(p[0] - q[0], p[1] - q[1]) <= tol) {
            let key = gw.id;
            dr.drag = Some(PDrag::Grip { wall: gw, end, from: p, key, at: None });
            return true;
        }
    }
    let Some(k) = crate::bimplan::hit(&dr.scene, &dr.view, p, 6.0) else { return false };
    let Some(op) = dr.data.as_ref().and_then(|sn| sn.openings.iter().find(|o| o.id == k)).cloned() else { return false };
    dr.drag = Some(PDrag::Opening { op, key: k, from: p, moved: false });
    true
}

/// `gripMove(d, p, e)`: where the dragged grip puts the wall (snapped; the middle grip moves the whole wall).
fn grip_move(dr: &crate::bimplan::Draw, w: &Wall, end: End, from: V2, p: V2, shift: bool) -> GripAt {
    let others: Vec<SnapWall> =
        crate::bimplan::walls_here(dr).iter().filter(|x| x.guid != w.guid).map(|x| SnapWall { a: x.a, b: x.b, t: x.t }).collect();
    let tol = dr.view.px_to_mm(SNAP_PX);
    let round = crate::bimplan::round_step(dr);
    if end == End::Mid {
        let s0 = snap_point(p, &others, &[], SnapOpts { tol, from: Some(from), ortho: shift, step: 90.0, round });
        let (dx, dy) = (s0.p[0] - from[0], s0.p[1] - from[1]);
        return GripAt { a: [w.a[0] + dx, w.a[1] + dy], b: [w.b[0] + dx, w.b[1] + dy], snap: s0 };
    }
    let fixed = if end == End::A { w.b } else { w.a };
    let s1 = snap_point(p, &others, &[], SnapOpts { tol, from: Some(fixed), ortho: shift, step: 15.0, round });
    if end == End::A { GripAt { a: s1.p, b: w.b, snap: s1 } } else { GripAt { a: w.a, b: s1.p, snap: s1 } }
}

/// `onMove` while a press is taken → handled.
pub fn on_move(app: &mut UiApp, p: V2, shift: bool) -> bool {
    if app.bimh.draw.drag.is_none() {
        return false;
    }
    let t0 = web_time::Instant::now();
    on_move_in(app, p, shift);
    // what a move costs the page: where the drag goes and its overlay (model-ui.js onMove → drawOverlay)
    let sel = app.picked_product();
    let mut o = Overlay::default();
    overlay(&app.bimh.draw, sel, &mut o);
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    let best = app.bimh.draw.timings.entry("preview").or_insert(ms);
    *best = best.min(ms);
    true
}

fn on_move_in(app: &mut UiApp, p: V2, shift: bool) {
    let Some(d) = app.bimh.draw.drag.clone() else { return };
    match d {
        PDrag::Grip { wall, end, from, key, .. } => {
            let at = grip_move(&app.bimh.draw, &wall, end, from, p, shift);
            app.bimh.draw.drag = Some(PDrag::Grip { wall, end, from, key, at: Some(at) });
        }
        PDrag::Opening { op, key, from, moved } => {
            let dr = &mut app.bimh.draw;
            let mut h = crate::bimplan::hover_opening(dr, p, op.kind(), op.width);
            if let Some(h) = h.as_mut() {
                let walls = crate::bimplan::walls_here(dr);
                if walls.get(h.wall).is_some_and(|w| w.guid == op.wall_guid) {
                    h.face = op.face.clone();
                }
            }
            dr.ohover = h;
            dr.drag = Some(PDrag::Opening { op, key, from, moved: true });
            if !moved {
                // the symbol leaves its place while it is dragged
                crate::bimplan::build_scene(app);
            }
        }
    }
}

/// `onUp(p, e, { moved, cancel })`: the drag's change, one undo step. `moved`: screen px from the press.
pub fn on_up(app: &mut UiApp, p: V2, cancel: bool) -> Result<(), String> {
    let Some(d) = app.bimh.draw.drag.take() else { return Ok(()) };
    let from = d.from();
    let moved = m::hypot(p[0] - from[0], p[1] - from[1]) * app.bimh.draw.view.scale;
    match d {
        PDrag::Grip { wall, end, key, at, .. } => {
            let Some(mv) = at.filter(|_| !cancel && moved > 4.0) else { return Ok(()) };
            if m::hypot(mv.b[0] - mv.a[0], mv.b[1] - mv.a[1]) < 10.0 {
                return Err(bt("model.err.short", &[]));
            }
            let changes = match end {
                End::A => WallChange { a: Some(mv.a), ..WallChange::default() },
                End::B => WallChange { b: Some(mv.b), ..WallChange::default() },
                End::Mid => WallChange { a: Some(mv.a), b: Some(mv.b), ..WallChange::default() },
            };
            let op = Op::ChangeWall { guid: wall.guid.clone(), changes, carry_joined: true };
            crate::bimplan::commit_sel(app, op, &bt("model.wall", &[]), &bt("model.verb.moved", &[]), Some(key));
            Ok(())
        }
        PDrag::Opening { op, key, .. } => {
            let h = app.bimh.draw.ohover.take();
            let Some(h) = h.filter(|_| !cancel && moved > 4.0) else {
                crate::bimplan::build_scene(app);
                return Ok(());
            };
            if !h.ok {
                crate::bimplan::build_scene(app);
                return Err(bt("model.err.nofit", &[]));
            }
            let walls = crate::bimplan::walls_here(&app.bimh.draw);
            let Some(w) = walls.get(h.wall) else {
                crate::bimplan::build_scene(app);
                return Ok(());
            };
            let mut ch = OpeningChange { offset: Some(h.offset), ..OpeningChange::default() };
            if w.guid != op.wall_guid {
                ch.wall = Some(w.guid.clone());
                if op.door {
                    ch.face = Some(h.face.clone());
                }
            }
            app.select_product(key);
            let label = opening_type(&op.type_key).map(|t| opening_type_name(&t)).unwrap_or_default();
            crate::bimplan::commit_sel(
                app,
                Op::ChangeOpening { guid: op.guid.clone(), changes: ch },
                &label,
                &bt("model.verb.moved", &[]),
                Some(key),
            );
            Ok(())
        }
    }
}

/// `drawOverlay()`'s grip part: the picked wall's grips; while one is dragged, the wall where it would go (joined to
/// the others), its length and the snap; a door or window being slid where it would go.
pub fn overlay(dr: &crate::bimplan::Draw, sel: Option<f64>, o: &mut Overlay) {
    if let (Some(PDrag::Opening { op, .. }), Some(h)) = (&dr.drag, &dr.ohover) {
        let spec = crate::bimplan::OSpec {
            door: op.door,
            type_key: op.type_key.clone(),
            width: op.width,
            height: op.height,
            sill: op.sill,
            hand: op.hand.clone(),
        };
        crate::bimplan::opening_preview(dr, h, &spec, o);
    }
    if dr.tool == "select"
        && let Some(gw) = grip_wall(dr, sel)
    {
        o.grips = grips_of(&gw).iter().map(|(p, e)| (*p, *e == End::Mid)).collect();
    }
    if let Some(PDrag::Grip { wall: w, end, at: Some(mv), .. }) = &dr.drag {
        let mut walls: Vec<Seg> = crate::bimplan::walls_here(dr).iter().filter(|x| x.guid != w.guid).map(|x| x.seg()).collect();
        walls.push(Seg { key: "*".into(), a: mv.a, b: mv.b, t: w.t });
        if let Some(poly) = outline_of(&wall_outlines(&walls, 1.0), "*") {
            o.walls.push(poly.clone());
        }
        let l = m::hypot(mv.b[0] - mv.a[0], mv.b[1] - mv.a[1]);
        o.dims.push(Dim {
            a: mv.a,
            b: mv.b,
            text: fmt_len(l, &dr.units),
            off_px: None,
            off_mm: None,
            off: Some(w.t / 2.0 + dr.view.px_to_mm(18.0)),
            key: None,
        });
        if mv.snap.kind != "free" {
            let p = match end {
                End::Mid => mv.snap.p,
                End::A => mv.a,
                End::B => mv.b,
            };
            o.snap = Some((p, mv.snap.kind));
        }
    }
}

/// The scene leaves out the door or window being slid (`makeScene`'s `S.drag.op.guid` filter).
pub fn dragged_symbol(dr: &crate::bimplan::Draw) -> Option<f64> {
    match &dr.drag {
        Some(PDrag::Opening { key, .. }) => Some(*key),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::{Params, UiApp};

    fn run(app: &mut UiApp, id: &str, name: &str) -> Result<(), String> {
        app.run(id, &Params { name: Some(name.into()), ..Default::default() })
    }
    fn walls(app: &UiApp) -> Vec<([f64; 2], [f64; 2])> {
        app.bimh.draw.data.as_ref().map(|s| s.walls.iter().map(|w| (w.a, w.b)).collect()).unwrap_or_default()
    }

    #[test]
    fn grips_drag_wall_ends_middles_and_doors_one_undo_step_each() {
        let mut app = UiApp::default();
        for (id, n) in [("bim.clock", "1791979200000"), ("bim.new", "Grips|2|3000 mm"), ("bim.units", "mm"), ("bim.tool", "wall")] {
            run(&mut app, id, n).unwrap();
        }
        for p in ["0,0", "6000,0", "6000,4000", "0,4000", "0,0"] {
            run(&mut app, "bim.typed", p).unwrap();
        }
        run(&mut app, "bim.tool", "door").unwrap();
        run(&mut app, "bim.point", "1500,0").unwrap();
        run(&mut app, "bim.tool", "select").unwrap();
        run(&mut app, "bim.view", "3000,2000,0.1").unwrap();
        // the right wall picked: its b end up 1210 mm (free: 10 mm steps), the joined top wall follows
        run(&mut app, "bim.point", "6000,2000").unwrap();
        let picked = app.picked_product();
        assert!(super::grip_wall(&app.bimh.draw, picked).is_some());
        run(&mut app, "bim.drag", "6000,4000,6000,5210").unwrap();
        let ws = walls(&app);
        assert!(ws.iter().any(|(a, b)| *a == [6000.0, 0.0] && *b == [6000.0, 5210.0]), "{ws:?}");
        assert_eq!(app.picked_product(), picked, "the dragged wall stays picked");
        // one undo step puts it back
        run(&mut app, "bim.undo", "").unwrap();
        assert!(walls(&app).iter().any(|(a, b)| *a == [6000.0, 0.0] && *b == [6000.0, 4000.0]));
        // too short: refused, nothing changes
        run(&mut app, "bim.point", "6000,2000").unwrap();
        let before = walls(&app);
        let e = run(&mut app, "bim.drag", "6000,4000,6000,5");
        assert!(e.is_err());
        assert_eq!(walls(&app), before);
        // the middle grip moves the whole wall
        run(&mut app, "bim.drag", "6000,2000,6800,2000").unwrap();
        assert!(walls(&app).iter().any(|(a, b)| *a == [6800.0, 0.0] && *b == [6800.0, 4000.0]), "{:?}", walls(&app));
        // a door slides along its wall
        let off0 = app.bimh.draw.data.as_ref().unwrap().openings[0].offset;
        run(&mut app, "bim.drag", "1500,-50,2420,30").unwrap();
        let off1 = app.bimh.draw.data.as_ref().unwrap().openings[0].offset;
        assert!((off1 - off0 - 920.0).abs() < 20.0, "{off0} → {off1}");
        // a 2 px move is a click, not a drag; a drag off the plan's elements pans
        let n = app.bimh.ed.as_ref().unwrap().ed.changes.len();
        run(&mut app, "bim.drag", "2420,0,2440,0").unwrap();
        run(&mut app, "bim.drag", "3000,2000,3500,2600").unwrap();
        assert_eq!(app.bimh.ed.as_ref().unwrap().ed.changes.len(), n);
        assert!((app.bimh.draw.view.cx - 2500.0).abs() < 1e-9);
    }
}
