//! ModelCraft's keys as modelcraft.js reads them (its document keydown handler), in both key styles: the push-pull
//! keys ('sketchup', L line, R rectangle, P push/pull, M move, Q rotate, G component, Shift+G group …) and the modeler
//! keys ('blender', Edit › Modeler keys or the status bar's list: W select, B rectangle, E push/pull, G / R / S grab,
//! turn and scale the selection at once with X / Y / Z locking an axis, Shift+D duplicate, X delete, A / Alt+A select
//! all / none, H / Shift+H / Alt+H hide / hide the rest / unhide, Tab opens or closes a group, Shift+Tab snapping,
//! Home or Shift+C everything, the numpad's views, right-click cancels, Alt + drag orbits, Ctrl + middle-drag zooms).
//!
//! Actions (`model.edit=<action>`): keys-sketchup, keys-blender, key:<key>[@<code>] with `Ctrl+`, `Shift+`, `Alt+`
//! in front (`key:Shift+d`, `key:Tab`, `key:7@Numpad7`): the same handler the keyboard reaches, so scripts and the
//! parity harness (`forks/tools/mcuidiff.mjs --keys`) press keys too.

use crate::modelcraft::{ModelView, mt};

/// The modeler keys' tools (modelcraft.js BK).
fn blender_tool(l: &str) -> Option<&'static str> {
    Some(match l {
        " " | "w" => "select",
        "l" => "line",
        "b" => "rect",
        "c" => "circle",
        "n" => "polygon",
        "e" => "push",
        "t" => "tape",
        "d" => "dim",
        _ => return None,
    })
}

/// The characters that start the value box (modelcraft.js VCB_START).
pub fn vcb_start(k: &str) -> bool {
    let mut c = k.chars();
    matches!((c.next(), c.next()), (Some(ch), None) if ch.is_ascii_digit() || ".,-'\"[<*/".contains(ch))
}

impl ModelView {
    /// Whether the modeler keys are in use.
    pub fn blender(&self) -> bool {
        self.keys == "blender"
    }

    /// The key-style actions; None when `name` is not one of them.
    pub fn keys_act(&mut self, name: &str, arg: &str) -> Option<Result<(), String>> {
        let r = match name {
            "keys-sketchup" | "keys-blender" => {
                self.set_keys(name.trim_start_matches("keys-"));
                Ok(())
            }
            "key" => {
                let mut rest = arg;
                let (mut ctrl, mut shift, mut alt) = (false, false, false);
                loop {
                    if let Some(r) = rest.strip_prefix("Ctrl+").filter(|r| !r.is_empty()) {
                        ctrl = true;
                        rest = r;
                    } else if let Some(r) = rest.strip_prefix("Shift+").filter(|r| !r.is_empty()) {
                        shift = true;
                        rest = r;
                    } else if let Some(r) = rest.strip_prefix("Alt+").filter(|r| !r.is_empty()) {
                        alt = true;
                        rest = r;
                    } else {
                        break;
                    }
                }
                let (k, code) = match rest.rsplit_once('@') {
                    Some((k, c)) if !k.is_empty() => (k, c),
                    _ => (rest, ""),
                };
                if k.is_empty() {
                    return Some(Err("model.edit: key:<key>[@<code>]".into()));
                }
                // a key that starts the value box goes there (the box is the page's input, not the handler's)
                if !self.key_down(k, code, shift, ctrl, alt) && vcb_start(k) && !ctrl && !alt {
                    self.vcb = k.to_string();
                }
                // a scripted key is instant: a scene it goes to is reached at once (as scene-next)
                self.finish_anim();
                Ok(())
            }
            "scene-record" => {
                self.record_scenes();
                Ok(())
            }
            // the Views menu: a standard view, Perspective, Presentation view, section planes and cuts
            "view" => {
                if !matches!(arg, "iso" | "top" | "front" | "right" | "back" | "left" | "bottom") {
                    return Some(Err(format!("model.edit: view: unknown view {arg}")));
                }
                self.std_view(arg);
                Ok(())
            }
            "persp" => {
                self.toggle_persp();
                Ok(())
            }
            "present" => {
                self.set_presentation(!self.presenting);
                Ok(())
            }
            "sec-planes" => {
                let on = !self.tools.host.model.section_view.planes;
                self.set_section_view("planes", on);
                Ok(())
            }
            "sec-cuts" => {
                let on = !self.tools.host.model.section_view.cuts;
                self.set_section_view("cuts", on);
                Ok(())
            }
            // the view cube (viewcube.js through viewcube-camera.js cameraCubeOptions): a side, a step, a compass
            // letter, Home (stdView('iso'))
            "cube" => self.cube_act(arg),
            // the status bar's switches
            "toggle" => match arg {
                "snap" => {
                    self.toggle_snap();
                    Ok(())
                }
                "collide" => {
                    let was = self.tools.collide;
                    self.tools.collide = !was;
                    self.print(mt(if was { "app.collide.off" } else { "app.collide.on" }, &[]));
                    Ok(())
                }
                _ => Err(format!("model.edit: toggle: unknown switch {arg}")),
            },
            _ => return None,
        };
        Some(r)
    }

    /// The view cube's click: `top` … `right`, `step:up|down|left|right|cw|ccw`, `letter:n|e|s|w`, `home`.
    fn cube_act(&mut self, arg: &str) -> Result<(), String> {
        use cc_m3d_tools::viewcube_camera as vc;
        self.anim = None;
        if arg == "home" {
            self.std_view("iso");
            return Ok(());
        }
        let cam = &mut self.tools.host.cam;
        if let Some(step) = arg.strip_prefix("step:") {
            let v = vc::camera_view(cam);
            vc::face_on(v.dir, Some(v.up)).ok_or("model.edit: cube:step: the view does not look straight at a side")?;
            let (dir, up) = vc::step_view(v.dir, Some(v.up), step).ok_or(format!("model.edit: cube: no step {step}"))?;
            vc::apply_view(cam, dir, Some(up));
            return Ok(());
        }
        if let Some(l) = arg.strip_prefix("letter:") {
            let side = match l {
                "n" => [0.0, 1.0],
                "e" => [1.0, 0.0],
                "s" => [0.0, -1.0],
                "w" => [-1.0, 0.0],
                _ => return Err(format!("model.edit: cube: no letter {l}")),
            };
            // look from that side, keeping the tilt (viewcube.js viewFromLetter)
            let z = cam.axes().z[2].clamp(-0.9999, 0.9999);
            let h = (1.0 - z * z).sqrt();
            vc::apply_view(cam, [-side[0] * h, -side[1] * h, -z], None);
            return Ok(());
        }
        let dir = cc_m3d_tools::camera::view_dir(arg).ok_or(format!("model.edit: cube: no side {arg}"))?;
        vc::apply_view(cam, [-dir[0], -dir[1], -dir[2]], None);
        Ok(())
    }

    /// setKeys(v): the style, and the words that say so.
    pub fn set_keys(&mut self, v: &str) {
        let v = if v == "blender" { "blender" } else { "sketchup" };
        self.keys = v.to_string();
        self.print(mt(if v == "blender" { "app.keys.set.blender" } else { "app.keys.set.sketchup" }, &[]));
    }

    /// modelcraft.js's keydown handler for one key (`e.key` as `k`, `e.code` as `code`): → whether it was taken (a
    /// key it leaves alone may start the value box).
    #[allow(clippy::too_many_lines)]
    pub fn key_down(&mut self, k: &str, code: &str, shift: bool, ctrl: bool, alt: bool) -> bool {
        let lower = if k.chars().count() == 1 { k.to_lowercase() } else { k.to_string() };
        let l = lower.as_str();
        let b = self.blender();
        if ctrl {
            match l {
                "z" if !shift => self.undo(),
                "y" | "z" => self.redo(),
                "a" => self.select_all(),
                "c" => self.run_key_act("copy"),
                "x" => self.run_key_act("cut"),
                "v" => self.run_key_act(if shift { "paste-in-place" } else { "paste" }),
                "d" => self.run_key_act("duplicate"),
                "g" if b => self.make_group(if shift { "component" } else { "group" }),
                _ if b && matches!(code, "Numpad1" | "Numpad3" | "Numpad7") => {
                    self.std_view(match code {
                        "Numpad1" => "back",
                        "Numpad3" => "left",
                        _ => "bottom",
                    });
                }
                "Control" | "Meta" => {
                    let _ = self.tools.key("Control");
                }
                _ => return false,
            }
            return true;
        }
        if b && alt && l == "a" {
            self.tools.host.set_sel(Vec::new());
            return true;
        }
        if b && alt && l == "h" {
            self.unhide_everything();
            return true;
        }
        if k == "Alt" {
            let _ = self.tools.key("Alt");
            return true;
        }
        if shift && l == "p" && !alt {
            // presentation view (both key styles)
            self.set_presentation(!self.presenting);
            return true;
        }
        if k == "Shift" {
            let _ = self.tools.key("Shift");
            return true;
        }
        if k == "Escape" {
            // Esc stops an animation being recorded (and nothing else)
            if let Some(r) = self.recording.as_mut() {
                r.stop = true;
                return true;
            }
            self.stop_play();
            if self.tools.escape().unwrap_or(false) {
            } else if self.tools.name != "select" {
                self.set_tool("select");
            } else if !self.tools.host.sel.is_empty() {
                self.tools.host.set_sel(Vec::new());
            } else if !self.tools.host.path.is_empty() {
                self.close_ctx();
            }
            return true;
        }
        if let Some(dir) = k.strip_prefix("Arrow") {
            if self.lockable() && self.tools.busy() && self.tools.key(k).unwrap_or(false) {
                self.say_lock();
                return true;
            }
            let s = 18.0;
            let pick = |a: &str, va: f64, bb: &str, vb: f64| {
                if dir == a {
                    va
                } else if dir == bb {
                    vb
                } else {
                    0.0
                }
            };
            let cam = &mut self.tools.host.cam;
            if shift {
                cam.pan(pick("Left", s, "Right", -s), pick("Up", s, "Down", -s));
            } else {
                cam.orbit(pick("Left", -s, "Right", s), pick("Up", -s, "Down", s));
            }
            return true;
        }
        if b {
            if self.tools.busy() && matches!(l, "x" | "y" | "z") && !shift {
                let arrow = match l {
                    "x" => "ArrowRight",
                    "y" => "ArrowLeft",
                    _ => "ArrowUp",
                };
                if self.lockable() && self.tools.key(arrow).unwrap_or(false) {
                    self.say_lock();
                }
                return true;
            }
            if k == "Enter" && self.tools.busy() && matches!(self.tools.name.as_str(), "move" | "rotate" | "scale" | "push") {
                self.finish_transform();
                return true;
            }
            let numpad =
                code.strip_prefix("Numpad").is_some_and(|d| d == "Decimal" || (d.len() == 1 && d.chars().all(|c| c.is_ascii_digit())));
            if numpad && !self.tools.busy() {
                match code {
                    "NumpadDecimal" => self.frame_selection(),
                    "Numpad5" => self.toggle_persp(),
                    "Numpad1" => self.std_view("front"),
                    "Numpad3" => self.std_view("right"),
                    "Numpad7" => self.std_view("top"),
                    _ => {}
                }
                return true;
            }
            if k == "Tab" {
                if shift {
                    self.toggle_snap();
                    return true;
                }
                let sel = &self.tools.host.sel;
                let inst = sel.first().filter(|s| sel.len() == 1 && s.t == "i").map(|s| s.id);
                if let Some(id) = inst {
                    let mut p = self.tools.host.path.clone();
                    p.push(id);
                    self.open_ctx(p);
                } else if !self.tools.host.path.is_empty() {
                    self.close_ctx();
                } else {
                    self.print(mt("app.tab.none", &[]));
                }
                return true;
            }
            if k == "Home" {
                self.fit();
                return true;
            }
        }
        if k == "Backspace" && self.tools.call("back", &[]).ok().flatten().and_then(|v| v.as_bool()).unwrap_or(false) {
            return true;
        }
        match k {
            "Delete" | "Backspace" => {
                self.erase_selection();
                return true;
            }
            "PageDown" | "PageUp" => {
                self.step_scene(if k == "PageUp" { -1 } else { 1 });
                return true;
            }
            "Enter" => {
                if self.tools.busy() {
                    let _ = self.tools.call("finish", &[]);
                }
                return true;
            }
            _ => {}
        }
        if vcb_start(k) || alt {
            return false;
        }
        if b {
            match l {
                "d" if shift => self.run_key_act("duplicate"),
                "c" if shift => self.fit(),
                "h" if shift => self.isolate_selection(),
                "h" => self.hide_selection(),
                _ if shift => return false,
                "g" => {
                    let e = self.cursor_ev();
                    let _ = self.tools.grab(&e, false);
                }
                "r" => {
                    let e = self.cursor_ev();
                    let _ = self.tools.turn(&e);
                }
                "s" => {
                    let e = self.cursor_ev();
                    let _ = self.tools.resize(&e);
                }
                "x" => self.erase_selection(),
                "a" => self.select_all(),
                _ => match blender_tool(l) {
                    Some(t) => self.set_tool(t),
                    None => return false,
                },
            }
            return true;
        }
        if l == "z" && shift {
            self.fit();
            return true;
        }
        if l == "g" {
            self.make_group(if shift { "group" } else { "component" });
            return true;
        }
        if !shift && let Some(t) = cc_m3d_tools::tools::tool_key(l) {
            self.set_tool(t);
            return true;
        }
        false
    }

    fn run_key_act(&mut self, a: &str) {
        if let Err(e) = self.act(a) {
            self.message = e;
        }
    }

    /// finishTransform(): Enter ends a move, turn, scale or push under way.
    fn finish_transform(&mut self) {
        let st = self.tools.tool_state();
        let go = match self.tools.name.as_str() {
            "move" => st.get("p0").is_some_and(|p| !p.is_null()),
            "rotate" | "scale" => st.get("stage").and_then(serde_json::Value::as_f64) == Some(2.0),
            "push" => st.get("fid").is_some_and(|p| !p.is_null()),
            _ => false,
        };
        if go {
            let _ = self.tools.call("finish", &[]);
        }
    }

    /// stdView(name): a standard view, and the words that say so.
    pub fn std_view(&mut self, name: &str) {
        self.anim = None;
        self.standard(name);
        let v = mt(&format!("ui.views.{name}"), &[]);
        self.print(mt("app.view.done", &[("view", &v)]));
    }

    /// frameSelection(): the selection's box (5 cm round it) fills the view; nothing selected, everything.
    pub fn frame_selection(&mut self) {
        let Some((lo, hi)) = self.tools.sel_box() else {
            self.fit();
            return;
        };
        let pad = 0.05;
        let b = cc_m3d_tools::camera::Box3 { min: lo.map(|v| v - pad), max: hi.map(|v| v + pad) };
        self.anim = None;
        self.tools.host.cam.fit(Some(&b), None);
    }

    /// The Views menu's Perspective switch.
    pub fn toggle_persp(&mut self) {
        let c = &mut self.tools.host.cam;
        c.persp = !c.persp;
        let on = c.persp;
        self.print(mt(if on { "app.persp.on" } else { "app.persp.off" }, &[]));
    }

    /// setPresentation(on): only the model on screen (no axes, guides, marks or selection).
    pub fn set_presentation(&mut self, on: bool) {
        self.presenting = on;
        self.print(mt(if on { "app.present.on" } else { "app.present.off" }, &[]));
    }

    /// setSectionView(key, on): section planes or cuts shown, one undo step (kept in the file).
    pub fn set_section_view(&mut self, key: &str, on: bool) {
        let label = mt(if on { "app.undo.sec_show" } else { "app.undo.sec_hide" }, &[]);
        let planes = key == "planes";
        let _ = self.tools.host.model.change(&label, |m| {
            if planes {
                m.section_view.planes = on;
            } else {
                m.section_view.cuts = on;
            }
            Ok(())
        });
        if planes && !on {
            self.tools.host.sel.retain(|s| s.t != "s");
        }
        self.print(mt(&format!("app.sec.{key}.{}", if on { "on" } else { "off" }), &[]));
        self.after_history();
    }

    /// The status bar's Snap switch (Shift+Tab in the modeler keys).
    pub fn toggle_snap(&mut self) {
        let was = self.tools.snapping;
        self.tools.snapping = !was;
        self.print(mt(if was { "app.snap.off" } else { "app.snap.on" }, &[]));
    }
}

#[cfg(test)]
mod tests {
    use crate::mcact::ns;
    use crate::mcedit::testkit::{groups, root_insts};

    /// The view cube's clicks: a side looks from it, a step turns from a side-on view, a letter keeps the tilt, Home is
    /// the iso view; anything else is refused with words.
    #[test]
    fn view_cube_turns_the_camera() {
        let mut v = groups(&[0.0, 2.0]);
        v.act("view-size:1000,800").unwrap();
        v.act("cube:top").unwrap();
        let ax = v.tools.host.cam.axes();
        assert!((ax.z[2] - 1.0).abs() < 1e-3, "{:?}", ax.z);
        v.act("cube:step:down").unwrap();
        let ax = v.tools.host.cam.axes();
        assert!(ax.z[2].abs() < 1e-3, "{:?}", ax.z);
        v.act("cube:letter:e").unwrap();
        let ax = v.tools.host.cam.axes();
        assert!((ax.z[0] - 1.0).abs() < 1e-3, "{:?}", ax.z);
        v.act("cube:home").unwrap();
        assert_eq!(v.message, "Iso view.");
        assert!(v.act("cube:sideways").is_err());
        assert!(v.act("cube:letter:q").is_err());
    }

    /// Settings › ModelCraft reach the open model, and its own switches are the settings kept.
    #[test]
    fn modelcraft_settings_follow_the_model() {
        let mut app = crate::UiApp::new(crate::Services::default());
        app.mc = Some(groups(&[0.0]));
        app.run("settings.set", &crate::Params { name: Some("mc.keys=blender".into()), ..Default::default() }).unwrap();
        app.run("settings.set", &crate::Params { name: Some("mc.corner=axes".into()), ..Default::default() }).unwrap();
        let mc = app.mc.as_mut().unwrap();
        assert!(mc.blender());
        assert_eq!(mc.corner, "axes");
        mc.act("toggle:collide").unwrap();
        let v = app.settings_json();
        assert_eq!(v["modelcraft"]["keys"], "blender");
        assert_eq!(v["modelcraft"]["collide"], false);
        let mut b = crate::UiApp::new(crate::Services::default());
        b.load_settings(&v);
        assert_eq!(b.mcset.corner, "axes");
        assert!(!b.mcset.collide);
        assert!(app.run("settings.set", &crate::Params { name: Some("mc.scroll=sideways".into()), ..Default::default() }).is_err());
    }

    #[test]
    fn modeler_keys() {
        let mut v = groups(&[0.0, 2.0]);
        v.act("keys-blender").unwrap();
        assert!(v.message.starts_with("Modeler keys"), "{}", v.message);
        v.act("key:b").unwrap();
        assert_eq!(v.tools.name, "rect");
        v.act("key:w").unwrap();
        assert_eq!(v.tools.name, "select");
        v.act("key:Tab").unwrap();
        assert_eq!(v.message, crate::modelcraft::mt("app.tab.none", &[]));
        let iid = root_insts(&v)[0];
        v.act(&format!("select:i:{}", ns(iid))).unwrap();
        v.act("key:Tab").unwrap();
        assert_eq!(v.tools.host.path, vec![iid]);
        v.act("key:Tab").unwrap();
        assert!(v.tools.host.path.is_empty());
        v.act("view-size:1000,800").unwrap();
        v.act("key:g").unwrap();
        assert_eq!(v.tools.name, "move");
        assert!(v.tools.busy());
        v.act("key:z").unwrap();
        assert_eq!(v.message, crate::modelcraft::mt("app.lock.blue", &[]));
        v.act("key:Escape").unwrap();
        v.act("key:7@Numpad7").unwrap();
        assert_eq!(v.message, "Top view.");
        v.act("key:Shift+Tab").unwrap();
        assert!(!v.tools.snapping);
        v.act("key:x").unwrap();
        assert_eq!(root_insts(&v).len(), 1);
        v.act("key:Alt+h").unwrap();
        v.act("keys-sketchup").unwrap();
        v.act("key:r").unwrap();
        assert_eq!(v.tools.name, "rect");
        v.act("key:2").unwrap();
        assert_eq!(v.vcb, "2");
    }
}
