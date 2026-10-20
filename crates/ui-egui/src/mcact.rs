//! ModelCraft's panel actions as modelcraft.js does them (Outliner, Entity info, Tags, Colors and Materials, Scenes,
//! Style, the solid checker, Check for 3D printing): each edit is one undo step through the same cc-m3d model functions
//! modelcraft.js calls, with the same undo labels and status messages, so a saved `.modelcraft` file is byte-identical
//! to 0.5's after the same actions (`forks/tools/mcuidiff.mjs`).
//!
//! Everything is a command: the panels and `model.edit=<action>` (scripts, cadcraft-shot, the control channel) both go
//! through [`ModelView::act`]. Paths are instance ids from the top joined with `/` ("" = the top level); a selection is
//! `t:id` items joined with `,` (t = f e i g d s).

use cc_m3d::model::{Dim, Model};
use cc_m3d::ops::Sel;
use cc_m3d::vec::{add, dot, mul};

use crate::modelcraft::{ModelView, mt};

/// A number as JS's `String(x)` writes it (status fills).
pub fn ns(x: f64) -> String {
    cc_m3d::js::num_str(x)
}

/// `s.slice(0, n)` on a JS string (UTF-16 units).
pub fn js_slice(s: &str, n: usize) -> String {
    let mut used = 0usize;
    let mut out = String::new();
    for c in s.chars() {
        used = used.saturating_add(c.len_utf16());
        if used > n {
            break;
        }
        out.push(c);
    }
    out
}

/// "3/5" → [3, 5]; "" → [].
pub fn parse_path(s: &str) -> Result<Vec<f64>, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(Vec::new());
    }
    s.split('/')
        .take(33)
        .map(|x| x.trim().parse::<f64>().ok().filter(|v| v.is_finite()).ok_or_else(|| format!("model.edit: bad path {s}")))
        .collect()
}

/// [3, 5] → "3/5".
pub fn path_str(p: &[f64]) -> String {
    p.iter().map(|x| ns(*x)).collect::<Vec<_>>().join("/")
}

/// "f:3,e:5,i:7" → the selection.
pub fn parse_sel(s: &str) -> Result<Vec<Sel>, String> {
    let mut out = Vec::new();
    for item in s.split(',').map(str::trim).filter(|x| !x.is_empty()).take(100_000) {
        let (t, id) = item.split_once(':').ok_or_else(|| format!("model.edit: bad selection item {item}"))?;
        if !matches!(t, "f" | "e" | "i" | "g" | "d" | "s") {
            return Err(format!("model.edit: bad selection kind {t}"));
        }
        let id = id.trim().parse::<f64>().ok().filter(|v| v.is_finite()).ok_or_else(|| format!("model.edit: bad id in {item}"))?;
        out.push(Sel::new(t, id));
    }
    Ok(out)
}

/// The kernel's error as `err.message`.
fn em(e: &cc_m3d::Error) -> String {
    e.to_string()
}

/// The solid checker's list (problemList).
pub fn problem_list(p: &cc_m3d::solids::Problems) -> String {
    let mut parts = Vec::new();
    for (k, n) in [
        ("app.check.holes", p.holes.len()),
        ("app.check.extra", p.extra.len()),
        ("app.check.inner", p.inner.len()),
        ("app.check.stray", p.stray.len()),
        ("app.check.flipped", p.flipped.len()),
    ] {
        if n > 0 {
            parts.push(mt(k, &[("n", &n.to_string())]));
        }
    }
    parts.join(" · ")
}

/// The Colors palette's word for a colour name ("Light gray" → app.color.light_gray).
pub fn color_name(name: &str) -> String {
    mt(&format!("app.color.{}", name.to_lowercase().split_whitespace().collect::<Vec<_>>().join("_")), &[])
}

/// `colorLabel(c)`: a colour's, material's or texture's name.
pub fn color_label(model: &Model, c: Option<&str>) -> String {
    let Some(c) = c.filter(|c| !c.is_empty()) else { return mt("app.color.default", &[]) };
    if cc_m3d::paint::is_material_paint(c) {
        let id = c.get(4..).unwrap_or("");
        return if crate::mcmats::material(id).is_some() { mt(&format!("render.mat.{id}"), &[]) } else { id.to_string() };
    }
    if cc_m3d::paint::is_image_paint(c) {
        let id = cc_m3d::paint::image_id(c);
        let name = model.images.get(&id).map_or_else(|| "?".to_string(), |im| im.name.clone());
        return mt("app.color.image", &[("name", &name)]);
    }
    match cc_m3d::paint::PALETTE.iter().find(|(_, h)| *h == c) {
        Some((name, _)) => color_name(name),
        None => c.to_string(),
    }
}

/// The selection's single selected placement in the open context: (instance id, its definition id).
fn one_inst(v: &ModelView) -> Option<(f64, String)> {
    let s = v.tools.host.sel.first().filter(|_| v.tools.host.sel.len() == 1).filter(|s| s.t == "i")?;
    let ctx = v.ctx_def();
    let inst = v.tools.host.model.def(&ctx)?.insts.get(s.id)?;
    Some((s.id, inst.def.clone()))
}

impl ModelView {
    /// The open context's definition id (modelcraft.js ctxDef()).
    pub fn ctx_def(&self) -> String {
        cc_m3d_tools::tools::ctx_of(&self.tools.host.model, &self.tools.host.path).def
    }

    /// `print(m)`: the status message.
    pub fn print(&mut self, m: String) {
        self.message = m;
    }

    /// modelcraft.js afterHistory: the open context and the selection still exist.
    pub fn after_history(&mut self) {
        // the page's host.units reads model.units: an undo that restores another unit changes what typed values mean
        self.tools.host.units.clone_from(&self.tools.host.model.units);
        let model = &self.tools.host.model;
        if model.context(&self.tools.host.path).is_none() {
            self.tools.host.path.clear();
        }
        let ctx = cc_m3d_tools::tools::ctx_of(model, &self.tools.host.path);
        let def = model.def(&ctx.def);
        self.tools.host.sel.retain(|s| match s.t.as_str() {
            "e" => def.is_some_and(|d| d.edges.get(s.id).is_some()),
            "f" => def.is_some_and(|d| d.faces.get(s.id).is_some()),
            "i" => def.is_some_and(|d| d.insts.get(s.id).is_some()),
            "d" => model.dims.get(s.id).is_some(),
            "s" => model.sections.get(s.id).is_some(),
            _ => model.guides.get(s.id).is_some(),
        });
        let n = self.tools.host.model.scenes.len() as isize;
        self.scene_index = self.scene_index.min(n - 1);
    }

    /// The page's openContext(path): open a group or component for editing.
    pub fn open_ctx(&mut self, path: Vec<f64>) {
        let Some(ctx) = self.tools.host.model.context(&path) else { return };
        let name = self.tools.host.model.def(&ctx.def).map(|d| d.name.clone()).unwrap_or_default();
        self.tools.host.path = path;
        self.tools.host.sel.clear();
        let tool = if self.tools.name == "place" { "select".to_string() } else { self.tools.name.clone() };
        let _ = self.tools.set(&tool, cc_m3d_tools::tools::ToolArg::None);
        self.print(mt("app.ctx.editing", &[("name", &name)]));
        self.seen_path = self.tools.host.path.clone();
    }

    /// The page's closeContext(): close the innermost open group, selecting it.
    pub fn close_ctx(&mut self) {
        let Some(leaving) = self.tools.host.path.pop() else { return };
        self.tools.host.sel = vec![Sel::new("i", leaving)];
        self.seen_path = self.tools.host.path.clone();
    }

    /// The tools opened or closed a context themselves (double-click in Select): the page's hooks run.
    pub fn sync_ctx(&mut self) {
        let now = self.tools.host.path.clone();
        if now == self.seen_path {
            return;
        }
        let was = std::mem::replace(&mut self.seen_path, now.clone());
        if now.len() > was.len() {
            // the tools' openContext: run the page's (message, the tool restarts)
            self.tools.host.path = was;
            self.open_ctx(now);
        } else if now.len() + 1 == was.len()
            && was.starts_with(&now)
            && let Some(&leaving) = was.last()
        {
            self.tools.host.sel = vec![Sel::new("i", leaving)];
        }
    }

    /// `setTool(name)` from a panel.
    pub fn set_tool_named(&mut self, name: &str) {
        self.set_tool(name);
    }

    /// Run one panel action (see the module doc). Errors name the action; refusals the JS shows are status messages.
    #[allow(clippy::too_many_lines)]
    pub fn act(&mut self, spec: &str) -> Result<(), String> {
        let (name, arg) = spec.split_once(':').map_or((spec, ""), |(a, b)| (a, b));
        let num = |s: &str| -> Result<f64, String> {
            s.trim().parse::<f64>().ok().filter(|v| v.is_finite()).ok_or_else(|| format!("model.edit: {name} needs a number"))
        };
        match name {
            "select-all" => self.select_all(),
            "select-none" => self.tools.host.set_sel(Vec::new()),
            "select" => {
                let s = parse_sel(arg)?;
                self.tools.host.set_sel(s);
            }
            "make-group" => self.make_group("group"),
            "make-component" => self.make_group("component"),
            "explode" => self.explode_sel(),
            "erase" | "delete" => self.erase_selection(),
            "hide" => self.hide_selection(),
            "isolate" => self.isolate_selection(),
            "unhide-all" => self.unhide_everything(),
            "undo" => self.undo(),
            "redo" => self.redo(),
            "open" => {
                let p = parse_path(arg)?;
                self.open_ctx(p);
            }
            "edit" => {
                if let Some((iid, _)) = one_inst(self) {
                    let mut p = self.tools.host.path.clone();
                    p.push(iid);
                    self.open_ctx(p);
                }
            }
            "close" => self.close_ctx(),
            "close-all" => {
                while !self.tools.host.path.is_empty() {
                    self.close_ctx();
                }
            }
            "tree-select" => {
                // a click on an Outliner row: that placement selected in its context (the context is not opened)
                let (p, iid) = arg.rsplit_once(':').ok_or("model.edit: tree-select:<path>:<iid>")?;
                let (p, iid) = (parse_path(p)?, num(iid)?);
                self.tools.host.path = p.clone();
                self.seen_path = p;
                self.tools.host.set_sel(vec![Sel::new("i", iid)]);
            }
            "tree-eye" => {
                let (p, iid) = arg.rsplit_once(':').ok_or("model.edit: tree-eye:<path>:<iid>")?;
                let (p, iid) = (parse_path(p)?, num(iid)?);
                self.tree_eye(&p, iid);
            }
            "tree-rename" => {
                let (id, v) = arg.split_once('=').ok_or("model.edit: tree-rename:<def id>=<name>")?;
                self.tree_rename(id, v);
            }
            "tree-move" => {
                // drag a row onto a group (or the Outliner heading): <from path>:<iid>><to path>
                let (from, to) = arg.split_once('>').ok_or("model.edit: tree-move:<path>:<iid>><to path>")?;
                let (p, iid) = from.rsplit_once(':').ok_or("model.edit: tree-move:<path>:<iid>><to path>")?;
                let (p, iid, to) = (parse_path(p)?, num(iid)?, parse_path(to)?);
                self.tree_move(&p, iid, &to);
            }
            "tree-filter" => self.tree_filter = arg.trim().to_lowercase(),
            "info-name" => self.info_name(arg),
            "tag-add" => {
                self.add_tag_row();
            }
            "tag-rename" => {
                let (old, v) = arg.split_once('=').ok_or("model.edit: tag-rename:<old>=<new>")?;
                self.rename_tag(old, v);
            }
            "tag-delete" => self.delete_tag_named(arg),
            "tag-show" => self.show_tag(arg, true),
            "tag-hide" => self.show_tag(arg, false),
            "tag-set" => self.tag_selection(arg),
            "tag-set-new" => {
                let nm = self.tools.host.model.change(&mt("app.undo.tag_add", &[]), |m| Ok(m.add_tag(""))).map_err(|e| em(&e))?;
                self.tag_selection(&nm);
            }
            "paint" => {
                let c = if arg == "default" || arg.is_empty() { None } else { Some(arg.to_string()) };
                if let Some(c) = &c
                    && !cc_m3d::paint::is_paint(c)
                {
                    return Err(format!("model.edit: paint: not a colour or material: {c}"));
                }
                let label = color_label(&self.tools.host.model, c.as_deref());
                self.choose_paint(c, &label);
            }
            "reverse" => self.reverse_face(),
            "edge-length" => self.len_box("edge", arg),
            "radius" => self.len_box("radius", arg),
            "dim-length" => self.len_box("dim", arg),
            "size" => {
                let (q, v) = arg.split_once('=').ok_or("model.edit: size:<0|1|2>=<length>")?;
                let q = num(q)?;
                if !(0.0..=2.0).contains(&q) {
                    return Err("model.edit: size axis is 0, 1 or 2".into());
                }
                self.len_box(&format!("size{q}"), v);
            }
            "dim-kind" => self.dim_kind(arg),
            "dim-move" => self.dim_move(arg),
            "sec-name" => self.sec_name(arg),
            "sec-active" => self.sec_active(arg == "on" || arg == "true" || arg == "1"),
            "sec-at" => self.sec_at(arg),
            "sec-reverse" => self.sec_reverse(),
            "soften" => self.soften(arg),
            "harden" => self.soften_go(0.0),
            "make-unique" => self.make_unique(),
            "to-component" => self.to_component(),
            "check-show" => self.check_show(),
            "check-fix" => self.check_fix(),
            "del-guides" => {
                let _ = self.tools.host.model.change(&mt("app.undo.del_guides", &[]), |m| {
                    m.guides = cc_m3d::omap::IdMap::new();
                    Ok(())
                });
                self.tools.host.set_sel(Vec::new());
            }
            "del-dims" => {
                let _ = self.tools.host.model.change(&mt("app.undo.del_dims", &[]), |m| {
                    m.dims = cc_m3d::omap::IdMap::new();
                    Ok(())
                });
                self.tools.host.set_sel(Vec::new());
            }
            "scene-add" => self.add_scene(),
            "scene-go" => {
                let i = num(arg)?;
                self.go_scene_with(i as usize, false);
            }
            "scene-update" => {
                let i = num(arg)?;
                self.update_scene(i as usize);
            }
            "scene-rename" => {
                let (i, v) = arg.split_once('=').ok_or("model.edit: scene-rename:<i>=<name>")?;
                let i = num(i)?;
                self.rename_scene(i as usize, v);
            }
            "scene-delete" => {
                let i = num(arg)?;
                self.delete_scene(i as usize);
            }
            "scene-next" => {
                self.step_scene(1);
                self.finish_anim();
            }
            "scene-prev" => {
                self.step_scene(-1);
                self.finish_anim();
            }
            "style" => {
                if !matches!(arg, "shaded" | "hidden" | "wire" | "xray") {
                    return Err(format!("model.edit: unknown style {arg}"));
                }
                self.mode = arg.to_string();
                self.apply_style();
            }
            "edges" => {
                self.edges = arg != "off";
                self.apply_style();
            }
            "shadows" => {
                self.shadows = arg == "on";
                self.apply_style();
            }
            "sun" => {
                let v: Vec<f64> = arg.split(',').map(|x| x.trim().parse::<f64>().unwrap_or(f64::NAN)).collect();
                self.set_sun(&v);
            }
            "cam" => {
                let v: Vec<f64> = arg.split(',').map(|x| x.trim().parse::<f64>().unwrap_or(f64::NAN)).collect();
                let g = |i: usize| v.get(i).copied().filter(|x| x.is_finite());
                let (Some(ex), Some(ey), Some(ez), Some(tx), Some(ty), Some(tz)) = (g(0), g(1), g(2), g(3), g(4), g(5)) else {
                    return Err("model.edit: cam:ex,ey,ez,tx,ty,tz[,fov[,persp 1|0]]".into());
                };
                let persp = g(7).map(|p| p != 0.0);
                self.anim = None;
                self.tools.host.cam.set([ex, ey, ez], [tx, ty, tz], g(6), persp.or(Some(self.tools.host.cam.persp)));
            }
            "units" => {
                if !matches!(arg, "m" | "cm" | "mm" | "in" | "ft-in" | "ft") {
                    return Err(format!("model.edit: unknown units {arg}"));
                }
                self.tools.host.model.units = arg.to_string();
                self.tools.host.units = arg.to_string();
                let w = mt(&format!("app.unit.{}", arg.replace('-', "_")), &[]);
                self.print(mt(if arg == "ft-in" { "app.units.set.ftin" } else { "app.units.set" }, &[("unit", &w)]));
            }
            "print-check" => self.print_check(),
            "print-stl" | "print-3mf" => self.print_download(if name == "print-stl" { "stl" } else { "3mf" })?,
            "print-close" => self.print_report = None,
            "place" => self.place_component(arg),
            "tree-renaming" => self.renaming = Some(arg.to_string()).filter(|x| !x.is_empty()),
            "scene-go-anim" => {
                let i = num(arg)?;
                self.go_scene(i as usize);
            }
            other => match self.edit_act(other, arg) {
                Some(r) => r?,
                None => return Err(format!("model.edit: unknown action {other}")),
            },
        }
        Ok(())
    }

    // ---- hide, isolate, unhide all ----

    fn geo_sel(&self) -> Vec<Sel> {
        self.tools.host.sel.iter().filter(|x| matches!(x.t.as_str(), "e" | "f" | "i")).cloned().collect()
    }

    /// modelcraft.js hideSelection.
    pub fn hide_selection(&mut self) {
        let sel = self.geo_sel();
        if sel.is_empty() {
            self.print(mt("app.hide.none", &[]));
            return;
        }
        let _ = self.tools.escape();
        let path = self.tools.host.path.clone();
        let _ = self.tools.host.model.change(&mt("app.undo.hide", &[]), |m| {
            let def = m.context(&path).map(|c| c.def).unwrap_or_else(|| "root".into());
            if let Some(d) = m.defs.get_mut(&def) {
                cc_m3d::ops::hide_sel(d, &sel, true);
            }
            Ok(())
        });
        self.tools.host.set_sel(Vec::new());
        self.print(mt("app.hide.done", &[("n", &sel.len().to_string())]));
    }

    /// modelcraft.js isolateSelection.
    pub fn isolate_selection(&mut self) {
        let sel = self.geo_sel();
        if sel.is_empty() {
            self.print(mt("app.isolate.none", &[]));
            return;
        }
        let _ = self.tools.escape();
        let path = self.tools.host.path.clone();
        let n = self
            .tools
            .host
            .model
            .change(&mt("app.undo.isolate", &[]), |m| {
                let def = m.context(&path).map(|c| c.def).unwrap_or_else(|| "root".into());
                Ok(m.defs.get_mut(&def).map_or(0.0, |d| cc_m3d::ops::isolate_sel(d, &sel)))
            })
            .unwrap_or(0.0);
        let key = if self.tools.host.path.is_empty() { "app.isolate.done" } else { "app.isolate.done.group" };
        self.print(mt(key, &[("n", &ns(n))]));
    }

    /// modelcraft.js unhideEverything.
    pub fn unhide_everything(&mut self) {
        if cc_m3d::model::hidden_count(&self.tools.host.model) == 0.0 {
            self.print(mt("app.unhide.none", &[]));
            return;
        }
        let n = self.tools.host.model.change(&mt("ui.editmenu.unhide_all", &[]), |m| Ok(cc_m3d::model::unhide_all(m))).unwrap_or(0.0);
        self.print(mt("app.unhide.done", &[("n", &ns(n))]));
    }

    // ---- Outliner ----

    /// The eye on an Outliner row: hide or show that placement.
    pub fn tree_eye(&mut self, path: &[f64], iid: f64) {
        let Some(hidden) =
            self.tools.host.model.context(path).and_then(|c| self.tools.host.model.def(&c.def)?.insts.get(iid).map(|i| i.hidden))
        else {
            return;
        };
        let p = path.to_vec();
        let _ = self.tools.host.model.change(&mt(if hidden { "app.undo.unhide" } else { "app.undo.hide" }, &[]), |m| {
            let def = m.context(&p).map(|c| c.def).unwrap_or_else(|| "root".into());
            if let Some(d) = m.defs.get_mut(&def) {
                cc_m3d::ops::hide_sel(d, &[Sel::new("i", iid)], !hidden);
            }
            Ok(())
        });
        self.tools.host.sel.retain(|s| s.id != iid);
        self.after_history();
    }

    /// F2 on an Outliner row: rename the definition.
    pub fn tree_rename(&mut self, def_id: &str, v: &str) {
        let Some(old) = self.tools.host.model.def(def_id).map(|d| d.name.clone()) else { return };
        let v = js_slice(v.trim(), 120);
        if !v.is_empty() && v != old {
            let id = def_id.to_string();
            let nv = v.clone();
            let _ = self.tools.host.model.change(&mt("app.scene.rename", &[]), |m| {
                if let Some(d) = m.def_mut(&id) {
                    d.name = nv;
                }
                Ok(())
            });
            self.print(mt("app.tree.renamed", &[("name", &v)]));
        }
        self.after_history();
    }

    /// moveRow: a row dropped onto a group (or the top level), keeping where it is in the world.
    pub fn tree_move(&mut self, parent: &[f64], iid: f64, to: &[f64]) {
        let (p, t) = (parent.to_vec(), to.to_vec());
        let nid = match self.tools.host.model.change(&mt("app.undo.tree_move", &[]), |m| m.move_into(&p, iid, &t)) {
            Ok(n) => n,
            Err(e) => {
                self.print(em(&e));
                return;
            }
        };
        if nid == iid && parent == to {
            return;
        }
        self.tools.host.path = to.to_vec();
        self.seen_path = to.to_vec();
        self.tools.host.set_sel(vec![Sel::new("i", nid)]);
        let model = &self.tools.host.model;
        let ctx = model.context(to);
        let cdef = ctx.as_ref().and_then(|c| model.def(&c.def));
        let name = cdef.and_then(|d| d.insts.get(nid)).and_then(|i| model.def(&i.def)).map(|d| d.name.clone()).unwrap_or_default();
        let msg = if to.is_empty() {
            mt("app.tree.moved_top", &[("name", &name)])
        } else {
            mt("app.tree.moved_in", &[("name", &name), ("group", &cdef.map(|d| d.name.clone()).unwrap_or_default())])
        };
        self.print(msg);
        self.after_history();
    }

    // ---- Entity info ----

    /// The Name box of a selected group or component.
    pub fn info_name(&mut self, v: &str) {
        let Some((_, did)) = one_inst(self) else { return };
        let old = self.tools.host.model.def(&did).map(|d| d.name.clone()).unwrap_or_default();
        let v = js_slice(v.trim(), 120);
        if !v.is_empty() && v != old {
            let _ = self.tools.host.model.change(&mt("app.scene.rename", &[]), |m| {
                if let Some(d) = m.def_mut(&did) {
                    d.name = v;
                }
                Ok(())
            });
        }
    }

    /// Reverse face (one face selected).
    pub fn reverse_face(&mut self) {
        let Some(s) = self.tools.host.sel.first().filter(|s| s.t == "f" && self.tools.host.sel.len() == 1).cloned() else { return };
        let path = self.tools.host.path.clone();
        let _ = self.tools.host.model.change(&mt("app.info.reverse", &[]), |m| {
            let def = m.context(&path).map(|c| c.def).unwrap_or_else(|| "root".into());
            if let Some(d) = m.defs.get_mut(&def) {
                cc_m3d::ops::reverse_faces(d, &[s.id]);
            }
            Ok(())
        });
    }

    /// The whole curve the selection is (one arc, circle, polygon or curve), as renderInfo finds it.
    pub fn sel_curve(&self) -> Option<cc_m3d::ops::CurveInfo> {
        let sel = &self.tools.host.sel;
        if sel.is_empty() || sel.iter().any(|s| s.t != "e") {
            return None;
        }
        let ctx = self.ctx_def();
        let def = self.tools.host.model.def(&ctx)?;
        let cid = def.edges.get(sel.first()?.id)?.curve.clone()?;
        if !sel.iter().all(|s| def.edges.get(s.id).is_some_and(|e| e.curve.as_deref() == Some(cid.as_str()))) {
            return None;
        }
        cc_m3d::ops::curve_info(def, &cid).filter(|ci| ci.edges.len() == sel.len())
    }

    /// A length box of Entity info (lenBox): edge, radius, dim, size0..2. `text` is what was typed.
    pub fn len_box(&mut self, which: &str, text: &str) {
        let u = self.tools.host.model.units.clone();
        let label = match which {
            "edge" | "dim" => mt("app.info.length", &[]),
            "radius" => mt("app.info.radius", &[]),
            "size0" => mt("app.info.width", &[]),
            "size1" => mt("app.info.depth", &[]),
            "size2" => mt("app.info.height", &[]),
            _ => return,
        };
        let l = cc_m3d_tools::units::parse_length(text, &u);
        let Some(l) = l.filter(|l| *l > 0.0) else {
            self.print(mt("app.len.bad", &[]));
            return;
        };
        let sel = self.tools.host.sel.clone();
        let path = self.tools.host.path.clone();
        let r: cc_m3d::Res<()> = match which {
            "edge" => {
                let Some(s) = sel.first().filter(|s| s.t == "e" && sel.len() == 1) else { return };
                let id = s.id;
                self.tools.host.model.change(&mt("app.undo.edge_length", &[]), |m| m.set_edge_length(&path, id, l).map(|_| ()))
            }
            "radius" => {
                let Some(ci) = self.sel_curve().filter(|c| c.round && c.flat) else { return };
                self.tools.host.model.change(&mt("app.info.radius", &[]), |m| {
                    let def = m.context(&path).map(|c| c.def).unwrap_or_else(|| "root".into());
                    match m.defs.get_mut(&def) {
                        Some(d) => cc_m3d::ops::set_curve_radius(d, &ci.id, l).map(|_| ()),
                        None => Ok(()),
                    }
                })
            }
            "dim" => {
                let Some(s) = sel.first().filter(|s| s.t == "d" && sel.len() == 1) else { return };
                let id = s.id;
                self.tools.host.model.change(&mt("app.undo.dim_length", &[]), |m| m.set_dim_length(id, l).map(|_| ()))
            }
            _ => {
                let Some((_, did)) = one_inst(self) else { return };
                let q: usize = which.get(4..).and_then(|x| x.parse().ok()).unwrap_or(0);
                let comp = self.tools.host.model.def(&did).is_some_and(|d| d.kind == "component");
                let lab = mt(if comp { "app.undo.resize_component" } else { "app.undo.resize_group" }, &[]);
                self.tools.host.model.change(&lab, |m| m.set_def_size(&did, q, l))
            }
        };
        match r {
            Ok(()) => self.print(mt("app.len.set", &[("what", &label), ("len", &cc_m3d::units::format_length(l, &u))])),
            Err(e) => self.print(mt("app.failed", &[("error", &em(&e))])),
        }
        self.after_history();
    }

    fn one_dim(&self) -> Option<f64> {
        self.tools.host.sel.first().filter(|s| s.t == "d" && self.tools.host.sel.len() == 1).map(|s| s.id)
    }

    /// A radius / diameter dimension shows as radius or diameter.
    pub fn dim_kind(&mut self, v: &str) {
        let Some(id) = self.one_dim() else { return };
        if !matches!(v, "rad" | "dia") {
            return;
        }
        let v = v.to_string();
        let _ = self.tools.host.model.change(&mt("app.undo.dim_kind", &[]), |m| {
            if let Some(Dim::Rad { kind, .. }) = m.dims.get_mut(id) {
                *kind = v;
            }
            Ok(())
        });
    }

    /// Which end of a length dimension moves (a, b, both).
    pub fn dim_move(&mut self, v: &str) {
        let Some(id) = self.one_dim() else { return };
        if !matches!(v, "a" | "b" | "both") {
            return;
        }
        let v = v.to_string();
        let _ = self.tools.host.model.change(&mt("app.undo.dim_end", &[]), |m| {
            if let Some(Dim::Lin { mv, .. }) = m.dims.get_mut(id) {
                *mv = v;
            }
            Ok(())
        });
    }

    fn one_section(&self) -> Option<f64> {
        let id = self.tools.host.sel.first().filter(|s| s.t == "s" && self.tools.host.sel.len() == 1).map(|s| s.id)?;
        self.tools.host.model.sections.get(id).map(|_| id)
    }

    pub fn sec_name(&mut self, v: &str) {
        let Some(id) = self.one_section() else { return };
        let old = self.tools.host.model.sections.get(id).map(|s| s.name.clone()).unwrap_or_default();
        let v = js_slice(v.trim(), 80);
        if !v.is_empty() && v != old {
            let _ = self.tools.host.model.change(&mt("app.scene.rename", &[]), |m| {
                if let Some(s) = m.sections.get_mut(id) {
                    s.name = v;
                }
                Ok(())
            });
        }
    }

    pub fn sec_active(&mut self, on: bool) {
        let Some(id) = self.one_section() else { return };
        let _ = self.tools.host.model.change(&mt("app.undo.sec_active", &[]), |m| {
            m.active_section = if on { Some(id) } else { None };
            if on {
                m.section_view.cuts = true;
            }
            Ok(())
        });
        self.after_history();
    }

    pub fn sec_at(&mut self, text: &str) {
        let Some(id) = self.one_section() else { return };
        let u = self.tools.host.model.units.clone();
        let t = text.trim_start();
        let neg = t.starts_with('-');
        let l0 = cc_m3d_tools::units::parse_length(if neg { t.get(1..).unwrap_or("") } else { text }, &u);
        let l = l0.map(|x| if neg { -x } else { x });
        let Some(l) = l.filter(|x| x.is_finite()) else {
            self.print(mt("app.len.bad", &[]));
            return;
        };
        let name = self.tools.host.model.sections.get(id).map(|s| s.name.clone()).unwrap_or_default();
        let _ = self.tools.host.model.change(&mt("app.undo.sec_move", &[]), |m| {
            if let Some(s) = m.sections.get_mut(id) {
                s.p = add(s.p, mul(s.n, l - dot(s.p, s.n)));
            }
            Ok(())
        });
        self.print(mt("app.sec.moved", &[("name", &name), ("len", &cc_m3d::units::format_length(l, &u))]));
        self.after_history();
    }

    pub fn sec_reverse(&mut self) {
        let Some(id) = self.one_section() else { return };
        let _ = self.tools.host.model.change(&mt("app.undo.sec_reverse", &[]), |m| {
            if let Some(s) = m.sections.get_mut(id) {
                s.n = mul(s.n, -1.0);
            }
            Ok(())
        });
        self.after_history();
    }

    /// The Soften box: an angle from 0 to 180.
    pub fn soften(&mut self, text: &str) {
        // Number(inp.value): an empty box is 0
        let t = text.trim();
        let v = if t.is_empty() { 0.0 } else { t.parse::<f64>().unwrap_or(f64::NAN) };
        if !(0.0..=180.0).contains(&v) {
            self.print(mt("app.soften.bad", &[]));
            return;
        }
        self.soften_deg = v;
        self.soften_go(v);
    }

    /// softenRow's go(deg): the selected placement's definition, or the selected edges and the edges of the selected faces.
    pub fn soften_go(&mut self, deg: f64) {
        let label = mt(if deg > 0.0 { "app.undo.soften" } else { "app.undo.harden" }, &[]);
        let target_def: String;
        let eids: Vec<f64>;
        if let Some((_, did)) = one_inst(self) {
            eids = self.tools.host.model.def(&did).map(|d| d.edges.key_vec()).unwrap_or_default();
            target_def = did;
        } else {
            let sel = &self.tools.host.sel;
            if !sel.iter().any(|s| s.t == "f" || s.t == "e") {
                return;
            }
            let ctx = self.ctx_def();
            let Some(def) = self.tools.host.model.def(&ctx) else { return };
            let mut ids: Vec<f64> = Vec::new();
            let mut seen = std::collections::HashSet::new();
            for s in sel.iter().filter(|x| x.t == "e") {
                if seen.insert(s.id.to_bits()) {
                    ids.push(s.id);
                }
            }
            let mut want = std::collections::HashSet::new();
            for s in sel.iter().filter(|x| x.t == "f") {
                if let Some(f) = def.faces.get(s.id) {
                    for l in &f.loops {
                        for (i, a) in l.iter().enumerate() {
                            if let Some(b) = l.get((i + 1) % l.len()) {
                                want.insert(cc_m3d::def::key(*a, *b));
                            }
                        }
                    }
                }
            }
            if !want.is_empty() {
                for (eid, e) in def.edges.iter() {
                    if want.contains(&cc_m3d::def::key(e.a, e.b)) && seen.insert(eid.to_bits()) {
                        ids.push(eid);
                    }
                }
            }
            eids = ids;
            target_def = ctx;
        }
        let n = self
            .tools
            .host
            .model
            .change(&label, |m| Ok(m.defs.get_mut(&target_def).map_or(0.0, |d| cc_m3d::edges::soften_edges(d, &eids, deg))))
            .unwrap_or(0.0);
        let msg =
            if deg > 0.0 { mt("app.soften.done", &[("deg", &ns(deg)), ("n", &ns(n))]) } else { mt("app.harden.done", &[("n", &ns(n))]) };
        self.print(msg);
        self.after_history();
    }

    /// Make unique (a component placed more than once).
    pub fn make_unique(&mut self) {
        let Some((iid, _)) = one_inst(self) else { return };
        let path = self.tools.host.path.clone();
        let _ = self.tools.host.model.change(&mt("app.info.make_unique", &[]), |m| {
            let def = m.context(&path).map(|c| c.def).unwrap_or_else(|| "root".into());
            m.make_unique(&def, iid).map(|_| ())
        });
    }

    /// A group becomes a component (Entity info's Make component).
    pub fn to_component(&mut self) {
        let Some((_, did)) = one_inst(self) else { return };
        if self.tools.host.model.def(&did).is_none_or(|d| d.kind != "group") {
            return;
        }
        let base = mt("app.info.component", &[]);
        let _ = self.tools.host.model.change(&mt("app.undo.make_component", &[]), |m| {
            let nm = m.unique_name(&base);
            if let Some(d) = m.def_mut(&did) {
                d.kind = "component".into();
                d.name = nm;
            }
            Ok(())
        });
    }

    /// The solid checker's Show problems: open it with what is wrong selected.
    pub fn check_show(&mut self) {
        let Some((iid, did)) = one_inst(self) else { return };
        let Some(p) = self.tools.host.model.def(&did).map(cc_m3d::solids::solid_problems) else { return };
        let list = problem_list(&p);
        if list.is_empty() {
            return;
        }
        let mut path = self.tools.host.path.clone();
        path.push(iid);
        self.open_ctx(path);
        let mut sel: Vec<Sel> = p.holes.iter().chain(&p.extra).chain(&p.stray).map(|&id| Sel::new("e", id)).collect();
        sel.extend(p.inner.iter().chain(&p.flipped).map(|&id| Sel::new("f", id)));
        self.tools.host.set_sel(sel);
        self.print(mt("app.check.showing", &[("list", &list)]));
    }

    /// The solid checker's Fix.
    pub fn check_fix(&mut self) {
        let Some((_, did)) = one_inst(self) else { return };
        let Some(r) = self
            .tools
            .host
            .model
            .change(&mt("app.undo.fix_solid", &[]), |m| Ok(m.def_mut(&did).map(cc_m3d::solids::fix_solid)))
            .ok()
            .flatten()
        else {
            return;
        };
        let mut done = Vec::new();
        for (k, n) in [
            ("app.check.fixed.stray", r.stray),
            ("app.check.fixed.inner", r.inner),
            ("app.check.fixed.flipped", r.flipped),
            ("app.check.fixed.holes", r.holes),
        ] {
            if n != 0.0 {
                done.push(mt(k, &[("n", &ns(n))]));
            }
        }
        let done = done.join(" · ");
        let left = self.tools.host.model.def(&did).map(|d| problem_list(&cc_m3d::solids::solid_problems(d))).unwrap_or_default();
        let msg = if done.is_empty() {
            mt("app.check.nothing", &[("left", &left)])
        } else if !left.is_empty() {
            mt("app.check.fixed_left", &[("done", &done), ("left", &left)])
        } else {
            mt("app.check.fixed", &[("done", &done)])
        };
        self.print(msg);
        self.after_history();
    }

    // ---- Tags ----

    /// addTagRow: a new tag "Tag", "Tag 2" …
    pub fn add_tag_row(&mut self) -> String {
        let nm = self.tools.host.model.change(&mt("app.undo.tag_add", &[]), |m| Ok(m.add_tag(""))).unwrap_or_default();
        self.print(mt("app.tag.added", &[("name", &nm)]));
        nm
    }

    /// A tag's name box changed.
    pub fn rename_tag(&mut self, old: &str, v: &str) {
        let v = v.trim().to_string();
        if v.is_empty() || v == old {
            return;
        }
        let (o, nv) = (old.to_string(), v.clone());
        match self.tools.host.model.change(&mt("app.undo.tag_rename", &[]), |m| m.rename_tag(&o, &nv)) {
            Ok(_) => {
                let model = &self.tools.host.model;
                let name = model.tag(&v).and_then(|g| model.tags.get(g)).map(|g| g.name.clone()).unwrap_or_default();
                self.print(mt("app.tag.renamed", &[("old", old), ("name", &name)]));
            }
            Err(e) => self.print(em(&e)),
        }
        self.after_history();
    }

    pub fn delete_tag_named(&mut self, name: &str) {
        let nm = name.to_string();
        let n = self.tools.host.model.change(&mt("app.undo.tag_delete", &[]), |m| Ok(m.delete_tag(&nm))).unwrap_or(0.0);
        self.print(mt("app.tag.deleted", &[("name", name), ("n", &ns(n))]));
        self.after_history();
    }

    /// tagSelection(name): the selected groups and components get tag `name` ('' = untagged).
    pub fn tag_selection(&mut self, name: &str) {
        let iids: Vec<f64> = self.tools.host.sel.iter().filter(|s| s.t == "i").map(|s| s.id).collect();
        if iids.is_empty() {
            return;
        }
        let path = self.tools.host.path.clone();
        let nm = name.to_string();
        let r = self.tools.host.model.change(&mt("app.undo.tag_set", &[]), |m| {
            let def = m.context(&path).map(|c| c.def).unwrap_or_else(|| "root".into());
            m.set_tag(&def, &iids, Some(&nm))
        });
        if r.is_err() {
            // the page's model.change throws: nothing more happens
            return;
        }
        let model = &self.tools.host.model;
        let hidden = !name.is_empty() && model.tag(name).and_then(|g| model.tags.get(g)).is_some_and(|g| g.hidden);
        if hidden {
            self.tools.host.sel.retain(|s| s.t != "i");
            self.print(mt("app.tag.set_hidden", &[("name", name)]));
        } else if !name.is_empty() {
            self.print(mt("app.tag.set", &[("name", name), ("n", &iids.len().to_string())]));
        } else {
            self.print(mt("app.tag.unset", &[("n", &iids.len().to_string())]));
        }
        self.after_history();
    }

    // ---- Colors and Materials ----

    /// choosePaint(c, name): the Paint tool's colour; with faces or placements selected (and another tool), paint them.
    pub fn choose_paint(&mut self, c: Option<String>, name: &str) {
        self.tools.paint_color = c.clone();
        let sel: Vec<Sel> = self.tools.host.sel.iter().filter(|x| x.t == "f" || x.t == "i").cloned().collect();
        if !sel.is_empty() && self.tools.name != "paint" {
            let path = self.tools.host.path.clone();
            let _ = self.tools.host.model.change(&mt("app.undo.paint", &[]), |m| {
                let def = m.context(&path).map(|c| c.def).unwrap_or_else(|| "root".into());
                if let Some(d) = m.defs.get_mut(&def) {
                    let fids: Vec<f64> = sel.iter().filter(|x| x.t == "f").map(|x| x.id).collect();
                    cc_m3d::paint::paint_faces(d, &fids, c.as_deref())?;
                    for x in sel.iter().filter(|x| x.t == "i") {
                        cc_m3d::paint::paint_instance(d, x.id, c.as_deref())?;
                    }
                }
                Ok(())
            });
            self.print(mt("app.paint.sel", &[("color", name)]));
        } else {
            self.set_tool("paint");
            self.print(mt("app.paint.start", &[("color", name)]));
        }
    }

    // ---- Scenes ----

    /// renameScene(i, name).
    pub fn rename_scene(&mut self, i: usize, name: &str) {
        let name = name.trim();
        if name.is_empty() || i >= self.tools.host.model.scenes.len() {
            return;
        }
        let nm = js_slice(name, 80);
        let _ = self.tools.host.model.change(&mt("app.scene.rename.h", &[]), |m| {
            if let Some(s) = m.scenes.get_mut(i) {
                s.name = nm;
            }
            Ok(())
        });
    }

    // ---- Check for 3D printing ----

    /// print3d(): the selection or everything shown as a printer mesh, checked; the report shows in the panel.
    pub fn print_check(&mut self) {
        let (tris, what) = cc_io_m3d::printmc::print_triangles(&self.tools.host.model, &self.tools.host.path, &self.tools.host.sel);
        let t0 = web_time::Instant::now();
        let mesh = cc_io_m3d::print3d::mesh_from_triangles(&tris);
        let rep = cc_io_m3d::print3d::print_check(&mesh, &cc_io_m3d::print3d::CheckOpts::default());
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        let report = crate::mcprint::PrintReport::of(rep, what, ms);
        self.print(mt(if report.ok { "app.print3d.ready" } else { "app.print3d.notready" }, &[]));
        self.print_report = Some(report);
    }

    /// The report's Download STL / 3MF: exactly the checked mesh, in millimetres.
    pub fn print_download(&mut self, kind: &str) -> Result<(), String> {
        let rep = self.print_report.as_ref().ok_or("model.edit: run print-check first")?;
        if rep.triangles == 0.0 {
            return Err("model.edit: there is nothing to print".into());
        }
        let base = self.base_name();
        let file = format!("{base}.{kind}");
        let bytes = if kind == "stl" {
            cc_io_m3d::print3d::stl_binary(&rep.mesh, &format!("ModelCraft {base}"))
        } else {
            cc_io_m3d::print3d::three_mf(&rep.mesh, &base, "ModelCraft")
        };
        let n = rep.mesh.indices.len() / 3;
        self.pending_save = Some((file.clone(), bytes));
        self.print(mt("app.print3d.done", &[("file", &file), ("n", &n.to_string())]));
        self.print_report = None;
        Ok(())
    }

    /// modelcraft.js base(): the file's name without .modelcraft / .json ("Untitled" when none).
    pub fn base_name(&self) -> String {
        let n = &self.name;
        let lower = n.to_lowercase();
        let b = if lower.ends_with(".modelcraft") {
            n.get(..n.len().saturating_sub(11)).unwrap_or("")
        } else if lower.ends_with(".json") {
            n.get(..n.len().saturating_sub(5)).unwrap_or("")
        } else {
            n.as_str()
        };
        if b.is_empty() { mt("app.file.untitled", &[]) } else { b.to_string() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxes() -> ModelView {
        // two boxes, each made a group, one tagged
        let mut m = Model::new();
        let _ = m.change("x", |m| {
            let d = m.defs.get_mut("root").unwrap();
            for x in [0.0, 3.0] {
                let before = d.faces.key_vec();
                cc_m3d::ops::add_polygon(
                    d,
                    &cc_m3d::ops::rect_pts([x, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 1.0, 1.0),
                    Some([0.0, 0.0, 1.0]),
                )?;
                let f = d.faces.key_vec().into_iter().find(|k| !before.contains(k)).unwrap();
                cc_m3d::ops::push_pull(d, f, 1.0);
            }
            Ok(())
        });
        let mut v = ModelView::new(m, "Boxes.modelcraft", true).unwrap();
        v.act("units:m").unwrap();
        v.act("select-all").unwrap();
        v
    }

    #[test]
    fn parse_helpers() {
        assert_eq!(parse_path("").unwrap(), Vec::<f64>::new());
        assert_eq!(parse_path("3/5").unwrap(), vec![3.0, 5.0]);
        assert!(parse_path("3/x").is_err());
        assert_eq!(path_str(&[3.0, 5.0]), "3/5");
        let s = parse_sel("f:3, i:7").unwrap();
        assert_eq!((s[0].t.as_str(), s[0].id, s[1].t.as_str()), ("f", 3.0, "i"));
        assert!(parse_sel("q:1").is_err());
        assert_eq!(js_slice("abcdef", 3), "abc");
    }

    #[test]
    fn group_hide_tag_rename_undo() {
        let mut v = boxes();
        v.act("make-group").unwrap();
        let iid = v.tools.host.sel[0].id;
        assert!(v.message.starts_with("Grouped as "), "{}", v.message);
        v.act("tree-eye::".to_string().as_str()).unwrap_err();
        v.act(&format!("tree-eye::{}", ns(iid))).unwrap();
        assert!(v.tools.host.model.root().unwrap().insts.get(iid).unwrap().hidden);
        v.act("unhide-all").unwrap();
        assert!(!v.tools.host.model.root().unwrap().insts.get(iid).unwrap().hidden);
        v.act("tag-add").unwrap();
        assert_eq!(v.tools.host.model.tags[0].name, "Tag");
        v.act("tag-rename:Tag=Walls").unwrap();
        assert_eq!(v.message, "Renamed the tag Tag to Walls.");
        v.act(&format!("select:i:{}", ns(iid))).unwrap();
        v.act("tag-set:Walls").unwrap();
        assert_eq!(v.message, "Tagged Walls.");
        v.act("tag-hide:Walls").unwrap();
        assert!(v.tools.host.sel.is_empty());
        let def = v.tools.host.model.root().unwrap().insts.get(iid).unwrap().def.clone();
        v.act(&format!("tree-rename:{def}=Shed")).unwrap();
        assert_eq!(v.tools.host.model.def(&def).unwrap().name, "Shed");
        v.act("undo").unwrap();
        assert_eq!(v.message, "Undone: Rename.");
        assert_ne!(v.tools.host.model.def(&def).unwrap().name, "Shed");
        assert!(v.act("nope").is_err());
    }

    #[test]
    fn edit_inside_and_size() {
        let mut v = boxes();
        v.act("make-component").unwrap();
        let iid = v.tools.host.sel[0].id;
        v.act("size:0=6m").unwrap();
        assert_eq!(v.message, "Width (red) set to 6 m.");
        let did = v.tools.host.model.root().unwrap().insts.get(iid).unwrap().def.clone();
        let bb = v.tools.host.model.def_bbox(&did).unwrap();
        assert!((bb.max[0] - bb.min[0] - 6.0).abs() < 1e-9);
        v.act("size:0=abc").unwrap();
        assert_eq!(v.message, mt("app.len.bad", &[]));
        v.act("edit").unwrap();
        assert_eq!(v.tools.host.path, vec![iid]);
        assert!(v.message.starts_with("Editing "));
        v.act("close").unwrap();
        assert!(v.tools.host.path.is_empty());
        assert_eq!(v.tools.host.sel[0].id, iid);
        v.act("explode").unwrap();
        assert!(v.tools.host.model.root().unwrap().insts.get(iid).is_none());
    }

    #[test]
    fn print_check_report() {
        let mut v = boxes();
        v.act("select-none").unwrap();
        v.act("print-check").unwrap();
        let r = v.print_report.as_ref().unwrap();
        assert!(r.ok, "{:?}", r.problems);
        assert_eq!(v.message, mt("app.print3d.ready", &[]));
        v.act("print-stl").unwrap();
        let (f, b) = v.pending_save.take().unwrap();
        assert_eq!(f, "Boxes.stl");
        assert_eq!(b.len(), 84 + 50 * 24);
    }
}
