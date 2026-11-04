//! PartCraft's Edit menu in the app (M13.7): partcraft.js `editState` / `editAct` (`sketchEdit`, `modelEdit`) over
//! cc-part's clipboard.rs (clipboard.js) — Cut, Copy, Paste, Paste in place, Duplicate, Delete, Select all, Select none,
//! on sketch geometry (with the relations among it) and on sketch-based features (with their sketch). The clip lives
//! in the window (`UiApp::part_clip`), so it goes from one part to the next as 0.5's page keeps it; and the sketcher's
//! Project tool (sketch-tools.js `projectEdge`): a model edge clicked under the sketch becomes fixed geometry, picked as
//! view3d.js `pickAt` does with the camera square to the sketch (the nearest edge within 5 px, unless a face is in
//! front of it). Commands `part.edit=<cmd>`.

use cc_part::{clipboard as C, jv, part as P, sketch as S, vec as V};
use serde_json::Value;

use crate::partview::{Mode, PartView, tr, trv};

/// partcraft.js `EDIT_CMDS`.
pub const EDIT_CMDS: [&str; 8] = ["copy", "cut", "paste", "paste-in-place", "duplicate", "delete", "select-all", "select-none"];

/// The Edit menu (ui.inc): command, catalog key, key name.
pub const EDIT_MENU: &[(&str, &str, &str)] = &[
    ("cut", "ui.tb.editmenu.cut", "Ctrl+X"),
    ("copy", "ui.tb.editmenu.copy", "Ctrl+C"),
    ("paste", "ui.tb.editmenu.paste", "Ctrl+V"),
    ("paste-in-place", "ui.tb.editmenu.paste_in_place", "Ctrl+Shift+V"),
    ("duplicate", "ui.tb.editmenu.duplicate", "Ctrl+D"),
    ("delete", "ui.tb.editmenu.delete", "Delete"),
    ("select-all", "ui.tb.editmenu.select_all", "Ctrl+A"),
    ("select-none", "ui.tb.editmenu.select_none", "Esc"),
];

impl PartView {
    fn sketch_ents(&self) -> Vec<String> {
        let sk = self.sketch();
        self.sk
            .sel
            .iter()
            .filter(|id| id.as_str() != S::ORIGIN && S::entities(sk).iter().any(|e| jv::is_str(e.get("id"), id)))
            .cloned()
            .collect()
    }

    /// partcraft.js `editState()[cmd]`: Ok, or why not.
    pub fn edit_state(&self, cmd: &str, clip: Option<&Value>) -> Result<(), String> {
        let clip_word = clip.map(C::describe_clip).unwrap_or_default();
        let kind = clip.map(|c| jv::s(c, "kind").to_string());
        let w = |k: &str| trv(k, &[("what", clip_word.clone())]);
        if self.mode == Mode::Pick {
            return Err(tr("app.edit.picking"));
        }
        if self.in_asm() {
            return self.asm_edit_state(cmd, clip);
        }
        if let Some(st) = self.drawing.as_ref() {
            let s = st.sel.is_some();
            return match cmd {
                "delete" if s => Ok(()),
                "delete" => Err(tr("app.edit.select_note")),
                "select-all" => Err(tr("app.edit.notes_one_at_a_time")),
                "select-none" if s => Ok(()),
                "select-none" => Err(tr("app.edit.nothing_selected")),
                _ => Err(tr("app.edit.drawing")),
            };
        }
        if self.mode == Mode::Sketch {
            let sk = self.sketch();
            let ents = self.sketch_ents();
            let any = self.sk.sel.iter().any(|id| id != S::ORIGIN);
            let can = if ents.is_empty() { Err(tr("app.edit.select_geometry")) } else { Ok(()) };
            let paste = match kind.as_deref() {
                Some("sketch") => Ok(()),
                Some("instance") => Err(w("app.edit.clip_part_to_asm")),
                Some(_) => Err(w("app.edit.clip_features_exit_sketch")),
                None => Err(tr("app.edit.nothing_to_paste_sketch")),
            };
            return match cmd {
                "copy" | "cut" | "duplicate" => can,
                "paste" | "paste-in-place" => paste,
                "delete" => {
                    if any {
                        Ok(())
                    } else {
                        Err(tr("app.edit.select_geometry_any"))
                    }
                }
                "select-all" => {
                    if S::entities(sk).is_empty() {
                        Err(tr("app.edit.sketch_empty"))
                    } else {
                        Ok(())
                    }
                }
                _ => {
                    if !self.sk.sel.is_empty() || self.sk.tool != "select" {
                        Ok(())
                    } else {
                        Err(tr("app.edit.nothing_selected"))
                    }
                }
            };
        }
        let f = self.sel_feature.as_deref().and_then(|id| P::feature_by_id(&self.part, id));
        let can = match f {
            None => Err(tr("app.edit.select_copyable")),
            Some(f) if C::COPYABLE.contains(&jv::s(f, "type")) => Ok(()),
            Some(f) => Err(trv("app.edit.not_copyable", &[("name", jv::s(f, "name").to_string())])),
        };
        let paste = match kind.as_deref() {
            Some("features") => Ok(()),
            Some("sketch") => Err(w("app.edit.clip_sketch_open_first")),
            Some(_) => Err(w("app.edit.clip_part_to_asm")),
            None => Err(tr("app.edit.nothing_to_paste_feature")),
        };
        match cmd {
            "copy" | "cut" | "duplicate" => can,
            "paste" | "paste-in-place" => paste,
            "delete" => {
                if f.is_some() {
                    Ok(())
                } else {
                    Err(tr("app.edit.select_feature"))
                }
            }
            "select-all" => Err(tr("app.edit.history_one_at_a_time")),
            _ => {
                if self.sel_feature.is_some()
                    || self.sel_plane.is_some()
                    || self.pick.is_some()
                    || !self.multi.is_empty()
                    || self.pending_sketch
                {
                    Ok(())
                } else {
                    Err(tr("app.edit.nothing_selected"))
                }
            }
        }
    }

    /// partcraft.js `editAct(cmd)`: the clip in, the clip (maybe new) out.
    pub fn edit_act(&mut self, cmd: &str, clip: Option<Value>) -> Result<Option<Value>, String> {
        if !EDIT_CMDS.contains(&cmd) {
            return Err(format!("part.edit: {} , not {cmd}", EDIT_CMDS.join(", ")));
        }
        if self.in_asm() {
            return Ok(self.asm_edit_act(cmd, clip));
        }
        if let Err(why) = self.edit_state(cmd, clip.as_ref()) {
            self.note = why;
            return Ok(clip);
        }
        if self.drawing.is_some() {
            if cmd == "delete" {
                self.delete_note();
            } else if cmd == "select-none" {
                if let Some(st) = self.drawing.as_mut() {
                    st.sel = None;
                }
                self.compose_show();
            }
            return Ok(clip);
        }
        if self.mode == Mode::Sketch {
            return Ok(self.sketch_edit(cmd, clip));
        }
        Ok(self.model_edit(cmd, clip))
    }

    fn sketch_edit(&mut self, cmd: &str, clip: Option<Value>) -> Option<Value> {
        let ents = self.sketch_ents();
        match cmd {
            "copy" | "cut" => {
                let Some(c) = C::copy_sketch(self.sketch(), &ents) else { return clip };
                let dropped = jv::n(&c, "dropped");
                let drop = if dropped > 0.0 { trv("app.clip.will_drop", &[("n", cc_part::js::num_str(dropped))]) } else { String::new() };
                if cmd == "cut" {
                    let ids = ents.clone();
                    let _ = self.sketch_commit(&tr("app.undo.cut"), false, |sk| {
                        S::remove_entities(sk, &ids);
                        Ok(())
                    });
                    self.sk.sel.clear();
                    self.note = trv("app.clip.sketch_cut", &[("what", C::describe_clip(&c)), ("dropped", drop)]);
                } else {
                    let n = c.get("sketch").map_or(0, |s| S::constraints(s).len());
                    let rel = if n > 0 { trv("app.clip.sketch_with_relations", &[("n", n.to_string())]) } else { String::new() };
                    self.note = trv("app.clip.sketch_copied", &[("what", C::describe_clip(&c)), ("relations", rel), ("dropped", drop)]);
                }
                Some(c)
            }
            "paste" => {
                if let Some(c) = clip.clone() {
                    let mut ed = std::mem::take(&mut self.sk);
                    ed.start_paste(self, c, &tr("app.undo.paste"));
                    self.sk = ed;
                }
                clip
            }
            "paste-in-place" => {
                let Some(c) = clip.clone() else { return clip };
                let mut made = Value::Null;
                if self
                    .sketch_commit(&tr("app.undo.paste_in_place"), false, |sk| {
                        made = C::paste_sketch(sk, &c, [0.0, 0.0]);
                        Ok(())
                    })
                    .is_ok()
                {
                    let mut ed = std::mem::take(&mut self.sk);
                    ed.set_tool(self, "select");
                    ed.sel = jv::arr(made.get("ids")).iter().filter_map(|x| x.as_str().map(str::to_string)).collect();
                    self.sk = ed;
                    let dropped = jv::n(&c, "dropped");
                    self.note = trv(
                        "app.clip.sketch_pasted_in_place",
                        &[
                            ("what", C::describe_clip(&c)),
                            (
                                "dropped",
                                if dropped > 0.0 {
                                    trv("app.clip.sketch_dropped", &[("n", cc_part::js::num_str(dropped))])
                                } else {
                                    String::new()
                                },
                            ),
                        ],
                    );
                }
                clip
            }
            "duplicate" => {
                if let Some(c) = C::copy_sketch(self.sketch(), &ents) {
                    let mut ed = std::mem::take(&mut self.sk);
                    ed.start_paste(self, c, &tr("app.undo.duplicate"));
                    self.sk = ed;
                }
                clip
            }
            "delete" => {
                let mut ed = std::mem::take(&mut self.sk);
                ed.delete_selection(self);
                self.sk = ed;
                clip
            }
            "select-all" => {
                let mut ed = std::mem::take(&mut self.sk);
                ed.set_tool(self, "select");
                let ids: Vec<String> = S::entities(self.sketch()).iter().map(|e| jv::s(e, "id").to_string()).collect();
                self.note = trv("app.sketch.selected_n", &[("n", ids.len().to_string())]);
                ed.sel = ids;
                self.sk = ed;
                clip
            }
            _ => {
                let mut ed = std::mem::take(&mut self.sk);
                ed.set_tool(self, "select");
                ed.sel.clear();
                self.sk = ed;
                clip
            }
        }
    }

    fn model_edit(&mut self, cmd: &str, clip: Option<Value>) -> Option<Value> {
        match cmd {
            "delete" => {
                self.delete_selected(false);
                return clip;
            }
            "select-none" => {
                self.pick = None;
                self.sel_feature = None;
                self.sel_plane = None;
                self.multi.clear();
                self.pending_sketch = false;
                self.sel_face = None;
                self.retint();
                return clip;
            }
            "paste" | "paste-in-place" => {
                let Some(c) = clip.clone() else { return clip };
                let label = tr(if cmd == "paste" { "app.undo.paste" } else { "app.undo.paste_in_place" });
                let mut out = Value::Null;
                if let Err(e) = self.commit(&label, |p| {
                    out = C::paste_features(p, &c)?;
                    Ok(())
                }) {
                    self.note = trv("app.clip.paste_failed", &[("error", e)]);
                    return clip;
                }
                let feats = jv::arr(out.get("features")).to_vec();
                if let Some(last) = feats.last() {
                    self.select_feature(Some(jv::s(last, "id").to_string()));
                }
                let bad = feats
                    .iter()
                    .filter_map(|g| self.regen.results.get(jv::s(g, "id")))
                    .find(|r| jv::is_str(r.get("status"), "error"))
                    .map(|r| jv::s(r, "message").to_string());
                let names = feats.iter().map(|g| jv::s(g, "name").to_string()).collect::<Vec<_>>().join(&tr("app.edge.and_sep"));
                let from = jv::s(&c, "from").to_string();
                let mut words = if !from.is_empty() && from != jv::s(&self.part, "name") {
                    trv("app.clip.features_pasted_from", &[("names", names), ("from", from)])
                } else {
                    trv("app.clip.features_pasted", &[("names", names)])
                };
                let note = jv::s(&out, "note");
                if !note.is_empty() {
                    words.push(' ');
                    words.push_str(note);
                }
                words.push(' ');
                words.push_str(&bad.unwrap_or_else(|| tr("app.clip.change_on_right")));
                self.note = words;
                return clip;
            }
            _ => {}
        }
        let Some(id) = self.sel_feature.clone() else { return clip };
        let c = match C::copy_feature(&self.part, &id) {
            Ok(c) => c,
            Err(e) => {
                self.note = e;
                return clip;
            }
        };
        match cmd {
            "copy" => {
                self.note = trv("app.clip.feature_copied", &[("what", C::describe_clip(&c))]);
                Some(c)
            }
            "cut" => {
                self.delete_selected(false);
                Some(c)
            }
            _ => {
                let mut out = Value::Null;
                if self
                    .commit(&tr("app.undo.duplicate"), |p| {
                        out = C::paste_features(p, &c)?;
                        Ok(())
                    })
                    .is_ok()
                {
                    let feats = jv::arr(out.get("features")).to_vec();
                    if let Some(last) = feats.last() {
                        self.select_feature(Some(jv::s(last, "id").to_string()));
                    }
                    let names = feats.iter().map(|g| jv::s(g, "name").to_string()).collect::<Vec<_>>().join(&tr("app.edge.and_sep"));
                    self.note = trv("app.clip.feature_duplicated", &[("names", names)]);
                }
                clip
            }
        }
    }

    /// partcraft.js `pickEdge` while sketching: the model edge under sketch point `p` (the camera looks square at the
    /// sketch plane from its +normal side; `scale` = pixels per mm): the nearest edge within 5 px, unless a face is
    /// in front of it.
    pub fn sketch_pick_edge(&self, p: [f64; 2], scale: f64) -> Option<Value> {
        self.regen.faces.as_ref()?;
        let fr = self.current_frame()?;
        let mesh = self.regen.mesh.as_ref()?;
        let tol = 5.0 / scale.max(1e-12);
        let segs: Vec<f64> = jv::arr(mesh.get("edgeSegs")).iter().map(|v| v.as_f64().unwrap_or(f64::NAN)).collect();
        let own = jv::arr(mesh.get("segEdge"));
        let size = self.regen.stats.as_ref().map_or(100.0, |st| {
            let b = st.get("bbox");
            let (mn, mx) = (
                crate::partview::p3(b.and_then(|b| b.get("min")).unwrap_or(&Value::Null)),
                crate::partview::p3(b.and_then(|b| b.get("max")).unwrap_or(&Value::Null)),
            );
            V::len(V::sub(mx, mn))
        });
        let r = cc_part::js::max(size * 4.0, 1000.0);
        let far = r * 3.0;
        let eye = V::add(V::to_world(&fr, p[0], p[1]), V::mul(fr.n, r));
        let depth = |q: [f64; 3]| V::dot(V::sub(eye, q), fr.n);
        let mut best: Option<(f64, usize)> = None;
        for (j, s) in segs.as_chunks::<6>().0.iter().enumerate() {
            let Some(e) = own.get(j).and_then(Value::as_u64).and_then(|e| usize::try_from(e).ok()) else { continue };
            let (a3, b3) = ([s[0], s[1], s[2]], [s[3], s[4], s[5]]);
            let (a, b) = (V::to_plane(&fr, a3), V::to_plane(&fr, b3));
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let l2 = dx * dx + dy * dy;
            let t = if l2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
            if (p[0] - a[0] - t * dx).hypot(p[1] - a[1] - t * dy) > tol {
                continue;
            }
            let d = depth(V::add(a3, V::mul(V::sub(b3, a3), t))) - far * 1e-4;
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, e));
            }
        }
        let (d, e) = best?;
        // a face nearer along the ray hides the edge
        if let Some(m) = self.regen.raw_mesh.as_ref() {
            let dir = V::mul(fr.n, -1.0);
            let pt = |vi: u32| -> [f64; 3] {
                let i = vi as usize * 3;
                [
                    m.positions.get(i).copied().unwrap_or(0.0),
                    m.positions.get(i + 1).copied().unwrap_or(0.0),
                    m.positions.get(i + 2).copied().unwrap_or(0.0),
                ]
            };
            for tri in m.indices.as_chunks::<3>().0 {
                let [a, b, c] = tri.map(pt);
                if let Some(h) = crate::partview::ray_tri(eye, dir, a, b, c)
                    && h < d
                {
                    return None;
                }
            }
        }
        self.edges().get(e).cloned()
    }
}

#[cfg(test)]
mod tests {
    use crate::{Params, UiApp};
    use cc_part::{jv, part as P, sketch as S};

    fn run(app: &mut UiApp, id: &str, name: &str) {
        app.run(id, &Params { name: Some(name.into()), ..Default::default() }).unwrap_or_else(|e| panic!("{id}={name}: {e}"));
    }

    #[test]
    fn copy_paste_between_parts_and_project_an_edge() {
        let mut app = UiApp::default();
        run(&mut app, "part.sample", "");
        run(&mut app, "part.select", "Bolt");
        run(&mut app, "part.edit", "copy");
        assert_eq!(app.part_clip.as_ref().map(|c| jv::s(c, "kind").to_string()).as_deref(), Some("features"));
        run(&mut app, "part.new", "");
        run(&mut app, "part.edit", "paste");
        assert_eq!(P::features(&app.part.as_ref().unwrap().part).len(), 2, "the hole and its sketch came along");
        // a sketch: a model edge projected onto it
        run(&mut app, "part.sample", "");
        run(&mut app, "part.sketch", "");
        run(&mut app, "part.plane", "front");
        run(&mut app, "part.sk.view", "60,35,4,640,600");
        run(&mut app, "part.sk.tool", "project");
        run(&mut app, "part.sk.click", "320,440");
        let pv = app.part.as_ref().unwrap();
        assert_eq!(S::entities(pv.sketch()).iter().filter(|e| jv::s(e, "type") == "line").count(), 1);
        // copy it, paste in place: two lines
        run(&mut app, "part.sk.all", "");
        run(&mut app, "part.edit", "copy");
        run(&mut app, "part.edit", "paste-in-place");
        assert_eq!(S::entities(app.part.as_ref().unwrap().sketch()).iter().filter(|e| jv::s(e, "type") == "line").count(), 2);
        assert!(app.run("part.edit", &Params { name: Some("explode".into()), ..Default::default() }).is_err());
    }
}
