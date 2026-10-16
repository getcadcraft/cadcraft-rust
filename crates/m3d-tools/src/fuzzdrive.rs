//! A byte-driven session for fuzzing (fuzz/fuzz_targets/m3d_tools.rs) and its regression tests: the bytes choose
//! tools, pointer events (anywhere, or near the model's corners), clicks with modifiers, keys, typed text (from a
//! value-box alphabet, or raw), selections, open contexts, camera moves, flags, undo / redo, tool methods, the
//! Blender-style starts, picking queries and outside model edits, over a small model (boxes, a group, a component,
//! a circle, a curve, a guide, a dimension). Nothing may panic, hang or grow without bound; errors are fine.
//! A component is never placed inside an open group (a definition holding itself is the kernel's walk budget case,
//! slow by design).

use cc_m3d::Model;
use cc_m3d::ops::{Sel, add_polygon, add_spline, connected, ngon_pts, push_pull, rect_pts, tag_curve};
use cc_m3d::vec::{P3, translation};
use serde_json::{Value, json};

use crate::camera::Camera;
use crate::tools::{Ev, Host, PickArg, TOOL_LIST, ToolArg, Tools};

/// Most operations one input runs.
pub const MAX_OPS: usize = 96;
/// The session stops when the model holds more faces than this.
pub const MAX_FACES: usize = 20_000;

struct Bytes<'a> {
    d: &'a [u8],
    i: usize,
}
impl Bytes<'_> {
    fn u8(&mut self) -> Option<u8> {
        let b = self.d.get(self.i).copied();
        self.i = self.i.saturating_add(1);
        b
    }
    fn f(&mut self, lo: f64, hi: f64) -> Option<f64> {
        let a = f64::from(self.u8()?);
        let b = f64::from(self.u8()?);
        Some(lo + (hi - lo) * (a * 256.0 + b) / 65535.0)
    }
}

fn start_model() -> Model {
    let mut m = Model::new();
    m.units = "m".into();
    let _ = m.change("setup", |m| {
        let r = m.root_mut().ok_or(cc_m3d::Error::Type)?;
        add_polygon(r, &rect_pts([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 2.0, 1.0), Some([0.0, 0.0, 1.0]))?;
        if let Some(f) = r.faces.keys().last() {
            push_pull(r, f, 1.0);
        }
        add_polygon(r, &rect_pts([4.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 1.0, 1.0), Some([0.0, 0.0, 1.0]))?;
        let f2 = r.faces.keys().last();
        if let Some(f) = f2 {
            push_pull(r, f, 1.5);
            let sel = connected(r, &[Sel::new("f", f)]);
            let iid = m.make_group("root", &sel, "component", Some("Block"))?;
            if let (Some(iid), Some(r)) = (iid, m.root_mut()) {
                let d = r.insts.get(iid).map(|i| i.def.clone()).unwrap_or_default();
                cc_m3d::model::place(r, &d, &translation([0.0, 4.0, 0.0]));
            }
        }
        let r = m.root_mut().ok_or(cc_m3d::Error::Type)?;
        let pts = ngon_pts([-3.0, 2.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 0.8, 12.0);
        add_polygon(r, &pts, Some([0.0, 0.0, 1.0]))?;
        tag_curve(r, &pts, "circle", true);
        add_spline(r, &[[-2.0, -2.0, 0.0], [0.0, -3.0, 0.0], [2.0, -2.0, 0.5]], false, 8.0, None)?;
        m.add_guide_line([0.0, -1.0, 0.0], [1.0, 0.0, 0.0]);
        m.add_dim([0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, -0.5, 0.0], &[]);
        Ok(())
    });
    m
}

const KEYS: [&str; 10] = ["ArrowRight", "ArrowLeft", "ArrowUp", "ArrowDown", "Shift", "Control", "Alt", "Escape", "Enter", "x"];
const ALPHA: &[u8] = b"0123456789.,;/-+ '\"mcfintxsXS[]<>*e";

/// Run one input.
pub fn run(data: &[u8]) {
    let mut cam = Camera::new();
    cam.set_size(640.0, 480.0);
    cam.eye = [8.0, -10.0, 9.0];
    cam.target = [1.0, 1.0, 0.0];
    cam.clip(30.0);
    let Ok(mut t) = Tools::new(Host::new(start_model(), cam)) else { return };
    let mut b = Bytes { d: data, i: 0 };
    let mut time = 0.0;
    for _ in 0..MAX_OPS {
        let Some(op) = b.u8() else { break };
        let _ = step(&mut t, &mut b, op, &mut time);
        let _ = t.flush();
        let faces: usize = t.host.model.defs.values().map(|d| d.faces.len()).sum();
        if faces > MAX_FACES {
            break;
        }
    }
}

fn point(t: &Tools, b: &mut Bytes<'_>) -> Option<(f64, f64)> {
    let k = b.u8()?;
    if k < 160 {
        // near a corner of the root's geometry
        let r = t.host.model.root()?;
        let n = r.verts.len();
        if n == 0 {
            return Some((0.0, 0.0));
        }
        let p: P3 = r.verts.values().nth(usize::from(k) % n).copied()?;
        let s = t.host.cam.project(p)?;
        let j = f64::from(b.u8()? % 16) - 8.0;
        return Some((s[0] + j, s[1] - j / 2.0));
    }
    Some((b.f(-100.0, 740.0)?, b.f(-100.0, 580.0)?))
}

#[allow(clippy::too_many_lines)]
fn step(t: &mut Tools, b: &mut Bytes<'_>, op: u8, time: &mut f64) -> cc_m3d::Res<()> {
    let ev = |x: f64, y: f64, m: u8, tm: Option<f64>| Ev {
        x,
        y,
        shift: m & 1 != 0,
        ctrl: m & 2 != 0,
        alt: m & 4 != 0,
        time: tm,
        ..Ev::default()
    };
    match op % 24 {
        0 => {
            let name = TOOL_LIST.get(usize::from(b.u8().unwrap_or(0)) % TOOL_LIST.len()).copied().unwrap_or("select");
            let arg = match name {
                "place" if t.host.path.is_empty() => match t.host.model.defs.keys().find(|k| *k != "root").cloned() {
                    Some(d) => ToolArg::Place { def_id: d, align: b.u8().unwrap_or(0) & 1 == 1, hint: None },
                    None => ToolArg::None,
                },
                "place" => return Ok(()),
                "paste" => {
                    let (path, sel) = (t.host.path.clone(), t.host.sel.clone());
                    ToolArg::Paste(t.host.model.copy_clip(&path, &sel)?.unwrap_or(Value::Null))
                }
                "light" => {
                    let k = ["point", "spot", "area"].get(usize::from(b.u8().unwrap_or(0) % 3)).copied().unwrap_or("point");
                    ToolArg::Light(json!({ "kind": k }))
                }
                "pick" => ToolArg::Pick(PickArg { hint: None, need: usize::from(b.u8().unwrap_or(0) % 3), picked: Vec::new() }),
                "curve" => match t.ctx_def_spline() {
                    Some(c) if b.u8().unwrap_or(0) & 1 == 1 => ToolArg::CurveEdit(c),
                    _ => ToolArg::None,
                },
                _ => ToolArg::None,
            };
            t.set(name, arg)?;
        }
        1..=4 | 21..=23 => {
            if let Some((x, y)) = point(t, b) {
                t.mv(&ev(x, y, 0, None))?;
            }
        }
        5 => {
            if let Some((x, y)) = point(t, b) {
                let m = b.u8().unwrap_or(0);
                *time += if m & 8 != 0 { 100.0 } else { 1000.0 };
                t.click(&ev(x, y, m, Some(*time)))?;
            }
        }
        6 => {
            if let Some((x, y)) = point(t, b) {
                *time += 1000.0;
                t.down(&ev(x, y, 0, Some(*time)))?;
            }
        }
        7 => {
            if let Some((x, y)) = point(t, b) {
                t.up(&ev(x, y, 0, None))?;
            }
        }
        8 => {
            let k = KEYS.get(usize::from(b.u8().unwrap_or(0)) % KEYS.len()).copied().unwrap_or("x");
            t.key(k)?;
        }
        9 => t.key_up("Shift")?,
        10 => {
            let n = usize::from(b.u8().unwrap_or(0) % 24);
            let raw = b.u8().unwrap_or(0) & 1 == 1;
            let mut s = String::new();
            for _ in 0..n {
                let c = b.u8().unwrap_or(b'0');
                if raw {
                    s.push(char::from(c));
                } else {
                    s.push(char::from(ALPHA.get(usize::from(c) % ALPHA.len()).copied().unwrap_or(b'0')));
                }
            }
            t.value(&s)?;
        }
        11 => {
            t.escape()?;
        }
        12 => {
            let c = t.ctx();
            let mask = b.u8().unwrap_or(0);
            if let Some(d) = t.host.model.def(&c.def) {
                let mut sel = Vec::new();
                for (k, id) in d.faces.keys().enumerate() {
                    if (mask >> (k % 8)) & 1 == 1 && sel.len() < 8 {
                        sel.push(Sel::new("f", id));
                    }
                }
                for (k, id) in d.edges.keys().enumerate() {
                    if (mask >> ((k + 3) % 8)) & 1 == 1 && sel.len() < 16 {
                        sel.push(Sel::new("e", id));
                    }
                }
                for id in d.insts.keys() {
                    if mask & 0x80 != 0 {
                        sel.push(Sel::new("i", id));
                    }
                }
                t.host.sel = sel;
            }
        }
        13 => {
            let c = t.ctx();
            let first = t.host.model.def(&c.def).and_then(|d| d.insts.first_key());
            match (b.u8().unwrap_or(0) & 1, first) {
                (1, Some(iid)) => {
                    let mut p = t.host.path.clone();
                    p.push(iid);
                    t.host.open_context(p);
                }
                _ => t.host.close_context(),
            }
        }
        14 => match b.u8().unwrap_or(0) % 6 {
            0 => t.collide = !t.collide,
            1 => t.snapping = !t.snapping,
            2 => t.touch = !t.touch,
            3 => t.paint_color = None,
            4 => t.paint_color = Some("#2e9d57".into()),
            _ => t.lock = None,
        },
        15 => {
            let (dx, dy) = (b.f(-60.0, 60.0).unwrap_or(0.0), b.f(-60.0, 60.0).unwrap_or(0.0));
            match b.u8().unwrap_or(0) % 4 {
                0 => t.host.cam.orbit(dx, dy),
                1 => t.host.cam.pan(dx, dy),
                2 => t.host.cam.zoom_at(if dx > 0.0 { 0.8 } else { 1.25 }, Some(320.0 + dx), Some(240.0 + dy), None),
                _ => {
                    let bb = t.host.model.def_bbox("root").map(|bb| crate::camera::Box3 { min: bb.min, max: bb.max });
                    t.host.cam.standard("iso", bb.as_ref());
                }
            }
        }
        16 => {
            if b.u8().unwrap_or(0) & 1 == 0 {
                t.host.model.undo()?;
            } else {
                t.host.model.redo()?;
            }
        }
        17 => {
            let name = ["finish", "back", "run", "apply", "begin"].get(usize::from(b.u8().unwrap_or(0) % 5)).copied().unwrap_or("finish");
            let args: Vec<Value> = match name {
                "apply" => vec![json!([b.f(-3.0, 3.0).unwrap_or(0.0), b.f(-3.0, 3.0).unwrap_or(0.0), 0.0])],
                "begin" => vec![json!([0, 0, 0]), json!(false)],
                "run" => vec![json!(1), json!([1, 2])],
                _ => Vec::new(),
            };
            let _ = t.call(name, &args)?;
        }
        18 => {
            if let Some((x, y)) = point(t, b) {
                match b.u8().unwrap_or(0) % 3 {
                    0 => {
                        t.grab(&ev(x, y, 0, None), true)?;
                    }
                    1 => {
                        t.turn(&ev(x, y, 0, None))?;
                    }
                    _ => {
                        t.resize(&ev(x, y, 0, None))?;
                    }
                }
            }
        }
        19 => {
            if let Some((x, y)) = point(t, b) {
                let _ = t.pick_at(&ev(x, y, 0, None));
                let _ = t.snap(&ev(x, y, 0, None), crate::tools::SnapOpts::default());
                let sc = t.host.scene();
                let _ = crate::infer::pick_window(
                    &sc,
                    &t.host.cam,
                    crate::infer::Rect { x0: x, y0: y, x1: 320.0, y1: 240.0 },
                    x > 320.0,
                    &t.host.path,
                );
            }
        }
        _ => {
            let path = t.host.path.clone();
            let k = b.u8().unwrap_or(0);
            let (x, y) = (f64::from(k % 8) - 4.0, f64::from((k / 8) % 8) - 4.0);
            t.host.model.change("edit", |m| {
                let id = m.context(&path).ok_or(cc_m3d::Error::Type)?.def;
                let d = m.def_mut(&id).ok_or(cc_m3d::Error::Type)?;
                add_polygon(d, &rect_pts([x, y, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 1.0, 1.0), None)?;
                if k & 0x40 != 0
                    && let Some(f) = d.faces.keys().last()
                {
                    push_pull(d, f, 0.5);
                }
                Ok(())
            })?;
        }
    }
    Ok(())
}

impl Tools {
    /// The first smooth curve of the open context (the fuzz driver edits it).
    fn ctx_def_spline(&self) -> Option<String> {
        let c = self.ctx();
        self.host.model.def(&c.def)?.splines.keys().next().cloned()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn some_inputs() {
        super::run(&[]);
        super::run(&[0, 1, 5, 0, 0, 5, 3, 10, 6, 0, 1, 2, 3, 4, 5, 6]);
        let all: Vec<u8> = (0..=255).collect();
        super::run(&all);
        let rev: Vec<u8> = (0..=255).rev().collect();
        super::run(&rev);
    }
}
