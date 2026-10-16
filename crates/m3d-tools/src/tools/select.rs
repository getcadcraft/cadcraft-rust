//! Select: click, double-click (open a group, a face with its edges, an edge with its faces), triple-click (all
//! connected), window (left to right) and crossing (right to left) selection; Shift toggles, Ctrl adds, Shift+Ctrl
//! removes. An edge of an arc, circle or polygon stands for the whole curve.

use cc_m3d::def::{edge_faces, ef_get};
use cc_m3d::js::num_str;
use cc_m3d::omap::OMap;
use cc_m3d::ops::{Sel, connected, face_with_edges};

use super::{Base, Ev, Res, Ret, ToolImpl, Tools, transform::MoveTool};
use crate::i18n::t0;
use crate::infer::{Rect, pick_window};
use crate::overlay::{Overlay, RectOv};
use crate::units::Value;

#[derive(Clone, Debug)]
pub struct SelectTool {
    pub b: Base,
    start: Option<Ev>,
}

impl SelectTool {
    pub fn new(name: &str) -> SelectTool {
        SelectTool { b: Base::new(name), start: None }
    }
    fn combine(&self, t: &mut Tools, found: Vec<Sel>, ev: &Ev) {
        let c = t.ctx();
        let found: Vec<Sel> = match t.host.model.def(&c.def) {
            Some(def) => found
                .into_iter()
                .flat_map(|s| {
                    let curve = if s.t == "e" { def.edges.get(s.id).and_then(|e| e.curve.clone()).filter(|c| !c.is_empty()) } else { None };
                    match curve {
                        Some(cv) => def
                            .edges
                            .iter()
                            .filter(|(_, e)| e.curve.as_deref() == Some(cv.as_str()))
                            .map(|(id, _)| Sel::new("e", id))
                            .collect(),
                        None => vec![s],
                    }
                })
                .collect(),
            None => found,
        };
        let k = |s: &Sel| format!("{}{}", s.t, num_str(s.id));
        let mut cur: OMap<String, Sel> = OMap::new();
        for s in &t.host.sel {
            cur.set(k(s), s.clone());
        }
        if ev.shift && ev.ctrl {
            for s in &found {
                cur.delete(&k(s));
            }
        } else if ev.shift {
            for s in &found {
                if cur.has(&k(s)) {
                    cur.delete(&k(s));
                } else {
                    cur.set(k(s), s.clone());
                }
            }
        } else if ev.ctrl {
            for s in &found {
                cur.set(k(s), s.clone());
            }
        } else {
            cur.clear();
            for s in &found {
                cur.set(k(s), s.clone());
            }
        }
        t.host.set_sel(cur.values().cloned().collect());
    }
}

impl ToolImpl for SelectTool {
    fn base(&self) -> &Base {
        &self.b
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.b
    }
    fn hint(&self, _t: &Tools) -> String {
        t0("m3d.tool.click_to_select_drag_left_to")
    }
    fn down(&mut self, _t: &mut Tools, ev: &Ev) -> Res<()> {
        self.start = Some(*ev);
        self.b.busy = false;
        Ok(())
    }
    fn mv(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        if let Some(st) = self.start
            && ev.dragging
        {
            self.b.busy = true;
            t.host.overlay(Some(Overlay {
                rect: Some(RectOv { x0: st.x, y0: st.y, x1: ev.x, y1: ev.y, crossing: ev.x < st.x }),
                ..Overlay::default()
            }));
            return Ok(());
        }
        let s = t.pick_at(ev);
        t.host.overlay(s.map(|s| Overlay { hover: Some(Some(s)), ..Overlay::default() }));
        Ok(())
    }
    fn up(&mut self, t: &mut Tools, ev: &Ev) -> Res<()> {
        let Some(start) = self.start.take() else { return Ok(()) };
        if ev.dragged {
            let sc = t.host.scene();
            let found = pick_window(&sc, &t.host.cam, Rect { x0: start.x, y0: start.y, x1: ev.x, y1: ev.y }, ev.x < start.x, &t.host.path);
            self.b.busy = false;
            t.host.overlay(None);
            self.combine(t, found, ev);
            return Ok(());
        }
        let s = t.pick_at(ev);
        let c = t.ctx();
        if ev.count >= 3.0
            && let Some(s) = &s
            && !matches!(s.t.as_str(), "i" | "g" | "d" | "s")
        {
            let r = t.host.model.def(&c.def).map(|d| connected(d, std::slice::from_ref(s))).unwrap_or_default();
            t.host.set_sel(r);
            return Ok(());
        }
        if ev.count == 2.0 {
            let Some(s) = &s else {
                if !t.host.path.is_empty() {
                    t.host.close_context();
                }
                return Ok(());
            };
            match s.t.as_str() {
                "i" => {
                    let mut p = t.host.path.clone();
                    p.push(s.id);
                    t.host.open_context(p);
                    return Ok(());
                }
                "f" => {
                    let r = t.host.model.def_mut(&c.def).map(|d| face_with_edges(d, s.id)).unwrap_or_default();
                    t.host.set_sel(r);
                    return Ok(());
                }
                "e" => {
                    let mut out = vec![s.clone()];
                    if let Some(d) = t.host.model.def(&c.def)
                        && let Some(e) = d.edges.get(s.id)
                    {
                        let ef = edge_faces(d);
                        out.extend(ef_get(&ef, e.a, e.b).iter().map(|id| Sel::new("f", *id)));
                    }
                    t.host.set_sel(out);
                    return Ok(());
                }
                _ => {}
            }
        }
        self.combine(t, s.into_iter().collect(), ev);
        Ok(())
    }
    fn value(&mut self, t: &mut Tools, v: &Value, text: &str) -> Res<Ret> {
        // 3x or /3 straight after a copy that ended back in Select (Ctrl+D, Shift+D)
        if matches!(v, Value::Array(_) | Value::Divide(_)) && t.last_copy.is_some() {
            let mut mv = MoveTool::new("move");
            return mv.value(t, v, text);
        }
        Ok(Ret::of(t.resize_sel(v)?))
    }
}
