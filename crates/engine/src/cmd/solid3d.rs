//! The 3D commands (commands3d.js): EXTRUDE (Taper angle, Path, Delete originals), SWEEP, REVOLVE, BOX, WEDGE,
//! CYLINDER, EXPORT3D / STLOUT / OBJOUT / GLBOUT; PRESSPULL, 3DORBIT, VIEW, VSCURRENT, UNION, INTERSECT and SUBTRACT
//! need the 3D tab and say so in a host without one.
//!
//! Every 3D edit is one undo step in the drawing's history (model3d.js `Model3D.change`: a snapshot before and after,
//! recorded when they differ). Solids live in meters; typed sizes are in drawing units (a unitless drawing counts as
//! millimeters). The model is `cc_io_cad::m3dmodel` on ModelCraft's kernel; the exports are io-m3d's.

use cc_cad::Doc;
use cc_cad::entity::Geo;
use cc_cad::js;
use cc_cad_geom::geom::prims;
use cc_cad_geom::jsmath::JsMath;
use cc_io_cad::m3dmodel::{self as m3, NotClosed};
use cc_m3d::model::Model;

use crate::cmd::draw::{circle_g, seg};
use crate::engine::{Ctx, Opts, R, Reply};
use crate::input::{Def, Opt, fmt};
use crate::prompt::CmdError;
use cc_cad::entity::Entity;

const TAU: f64 = std::f64::consts::PI * 2.0;

#[path = "bool3d.rs"]
pub mod bool3d;

/// commands3d.js `in3d(c)`: the 3D tab shows.
fn in3d(c: &Ctx) -> bool {
    c.sh().in3d()
}

/// commands3d.js `need3d(c)`: the 3D tab, opened when needed (false in a host without one).
fn need3d(c: &Ctx) -> bool {
    if in3d(c) || c.sh().open3d() {
        return true;
    }
    c.print(&c.t("cmd3d.needs_3d_tab_which"));
    false
}

/// A question whose clicks go to the 3D tab's tool (`{ raw: true, ws: 'tool' }`).
async fn ask_tool(c: &Ctx, prompt: String) -> R<Reply> {
    c.string(prompt, Opts { ws_tool: true, ..Opts::default() }).await
}

/// `drive(c, ws, tool, prompt, {copy, angle})`: clicks and typed values go to a ModelCraft tool until it has made its
/// edit (true) or Enter / Esc (false).
async fn drive(c: &Ctx, tool: &str, prompt: String, copy: bool, angle: bool) -> R<bool> {
    if let Some(Err(m)) = c.sh().with_ws(|ws, d| ws.begin(d, tool, copy)) {
        return Err(CmdError::Msg(m));
    }
    let r = drive_loop(c, prompt, angle).await;
    // `finally { ws.end() }`: a throw there is the command's error
    if let Some(Err(m)) = c.sh().with_ws(|ws, d| ws.end(d)) {
        return Err(CmdError::Msg(m));
    }
    r
}
async fn drive_loop(c: &Ctx, prompt: String, angle: bool) -> R<bool> {
    loop {
        let v = ask_tool(c, prompt.clone()).await?;
        if v == Reply::Done3d {
            return Ok(true);
        }
        let typed = match &v {
            Reply::Str(s) => js::trim(s).to_string(),
            _ => String::new(),
        };
        if typed.is_empty() {
            return Ok(false);
        }
        let ok = c.sh().with_ws(|ws, d| ws.value(d, &typed, angle)).unwrap_or(Ok(false)).map_err(CmdError::Msg)?;
        if !ok {
            // commands3d.js names the typed text `t`, so its i18n call throws for ROTATE (spec gap kept for parity)
            if angle {
                return Err(CmdError::Msg("t is not a function".into()));
            }
            c.print("Type a distance (drawing units), <dx,dy,dz>, or click.");
        }
        if c.sh().with_ws(|ws, _| ws.consume_done()).unwrap_or(false) {
            return Ok(true);
        }
    }
}

/// `select3d(c, ws, what)`: the 3D selection, or ask for one: clicks select (window, crossing, Shift) until Enter.
async fn select3d(c: &Ctx, what: &str) -> R<bool> {
    if c.sh().with_ws(|ws, _| !ws.tools.host.sel.is_empty()).unwrap_or(false) {
        return Ok(true);
    }
    if let Some(Err(m)) = c.sh().with_ws(|ws, d| ws.set_tool(d, "select")) {
        return Err(CmdError::Msg(m));
    }
    loop {
        let v = ask_tool(c, c.tv("cmd3d.select_in_3d", &[("what", what.to_string())])).await?;
        if v == Reply::Done3d {
            continue;
        }
        let typed = match &v {
            Reply::Str(s) => js::trim(s).to_string(),
            _ => String::new(),
        };
        if typed.is_empty() {
            break;
        }
        if typed.to_uppercase() == "ALL" {
            let n = c.sh().with_ws(|ws, _| ws.select_all()).unwrap_or(0);
            c.print(&format!("{n} found"));
        }
    }
    if c.sh().with_ws(|ws, _| ws.tools.host.sel.is_empty()).unwrap_or(true) {
        c.print(&c.t("cmd3d.nothing_selected"));
        return Ok(false);
    }
    Ok(true)
}

pub async fn presspull(c: Ctx) -> R {
    if !need3d(&c) {
        return Ok(());
    }
    if drive(&c, "push", c.t("cmd3d.presspull.click_face_push_pull"), false, false).await? {
        c.print(&c.t("cmd3d.presspull.pushed_pulled_one_face"));
    }
    Ok(())
}

pub async fn orbit3d(c: Ctx) -> R {
    if !need3d(&c) {
        return Ok(());
    }
    if let Some(Err(m)) = c.sh().with_ws(|ws, d| ws.begin(d, "orbit", false)) {
        return Err(CmdError::Msg(m));
    }
    let r = async {
        loop {
            let v = ask_tool(&c, c.t("cmd3d.3dorbit.drag_orbit_press_enter")).await?;
            let empty = match &v {
                Reply::Str(s) => js::trim(s).is_empty(),
                Reply::Done3d => false,
                _ => true,
            };
            if empty {
                return Ok(());
            }
        }
    }
    .await;
    if let Some(Err(m)) = c.sh().with_ws(|ws, d| ws.end(d)) {
        return Err(CmdError::Msg(m));
    }
    r
}

/// VIEW's options and what they do.
const VIEW_KEYS: [(&str, &str, &str); 10] = [
    ("I", "opt.iso", "iso"),
    ("T", "opt.top", "top"),
    ("BO", "opt.bottom", "bottom"),
    ("F", "opt.front", "front"),
    ("BA", "opt.back", "back"),
    ("L", "opt.left", "left"),
    ("R", "opt.right", "right"),
    ("P", "opt.perspective", "persp"),
    ("PA", "opt.parallel", "parallel"),
    ("E", "opt.extents", "extents"),
];

pub async fn view(c: Ctx) -> R {
    if !need3d(&c) {
        return Ok(());
    }
    let opts: Vec<Opt> = VIEW_KEYS.iter().map(|(k, l, _)| c.opt(k, &c.t(l))).collect();
    let v = c.keyword(c.t("cmd3d.view.choose_view"), Opts { options: opts.clone(), ..Opts::default() }).await?;
    let Reply::Opt(k) = v else { return Ok(()) };
    let Some((_, _, name)) = VIEW_KEYS.iter().find(|x| x.0 == k) else { return Ok(()) };
    c.sh().with_ws(|ws, d| ws.view(d, name));
    let label = opts.iter().find(|o| o.key == k).map(|o| o.label.clone()).unwrap_or_default();
    let key = if k == "P" || k == "PA" {
        "cmd3d.view.projection"
    } else if k == "E" {
        "cmd3d.view.extents_done"
    } else {
        "cmd3d.view.view_done"
    };
    c.print(&c.tv(key, &[("label", label)]));
    Ok(())
}

/// VSCURRENT's options: key, label (catalog key), style.
const VS_KEYS: [(&str, &str, &str); 5] = [
    ("S", "opt.shaded", "shaded"),
    ("E", "cmd3d.view.shaded_edges", "edges"),
    ("H", "cmd3d.view.hidden_line", "hidden"),
    ("W", "opt.wireframe", "wire"),
    ("X", "opt.xray", "xray"),
];

pub async fn vscurrent(c: Ctx) -> R {
    if !need3d(&c) {
        return Ok(());
    }
    let opts: Vec<Opt> = VS_KEYS.iter().map(|(k, l, _)| c.opt(k, &c.t(l))).collect();
    let v = c.keyword(c.t("cmd3d.vscurrent.visual_style"), Opts { options: opts, ..Opts::default() }).await?;
    let Reply::Opt(k) = v else { return Ok(()) };
    let Some((_, _, style)) = VS_KEYS.iter().find(|x| x.0 == k) else { return Ok(()) };
    c.sh().with_ws(|ws, _| ws.set_style(style));
    c.print(&c.tv("cmd3d.vscurrent.set", &[("style", c.t(&format!("model3d.style.{style}")))]));
    Ok(())
}

// ---- booleans (boolean3d.js on cc-manifold)

/// `pickSolids(c, ws, what)`: the solids picked on the 3D tab (instances of the top level), after asking when nothing
/// is selected.
async fn pick_solids(c: &Ctx, what: &str) -> R<Option<Vec<f64>>> {
    c.sh().with_ws(|ws, _| {
        if !ws.tools.host.path.is_empty() {
            ws.tools.host.path = Vec::new();
            ws.tools.host.sel = Vec::new();
        }
    });
    if !select3d(c, what).await? {
        return Ok(None);
    }
    let ids: Vec<f64> = c.sh().with_ws(|ws, _| ws.tools.host.sel.iter().filter(|s| s.t == "i").map(|s| s.id).collect()).unwrap_or_default();
    if ids.is_empty() {
        c.print(&c.t("cmd.bool.solids"));
        return Ok(None);
    }
    Ok(Some(ids))
}

fn run_boolean(c: &Ctx, op: &str, a: &[f64], b: &[f64]) {
    let tv = t2(c);
    let r = c.sh().with_ws(|ws, d| {
        let r = ws.tcall(d, |t| Ok(bool3d::boolean_solids(&mut t.host.model, op, a, b, &tv)));
        r.unwrap_or_else(|e| Err(e.to_string())).map(|iid| {
            let k = m3::k_of(d);
            (iid, volume_of(c, &ws.tools.host.model, &[iid], k))
        })
    });
    match r {
        Some(Ok((_, v))) => {
            c.sh().with_ws(|ws, _| ws.clear_sel());
            let n = a.len().saturating_add(b.len()).to_string();
            c.print(&c.tv(&format!("cmd.bool.done.{op}"), &[("n", n), ("volume", v)]));
        }
        Some(Err(m)) => c.print(&m),
        None => {}
    }
}

/// `volumeText` of a model in hand (`k`: meters per drawing unit).
fn volume_of(c: &Ctx, m: &Model, iids: &[f64], k: f64) -> String {
    let s: f64 = m3::solid_volumes(m).iter().filter(|(iid, _)| iids.contains(iid)).map(|(_, v)| v.unwrap_or(0.0)).sum();
    let v = s / (k * k * k);
    c.tv("cmd3d.volume", &[("n", js::num_to_string(v)), ("v", fmt(v))])
}

pub async fn union(c: Ctx) -> R {
    if !need3d(&c) {
        return Ok(());
    }
    if let Some(a) = pick_solids(&c, &c.t("cmd.bool.what.union")).await? {
        run_boolean(&c, "union", &a, &[]);
    }
    Ok(())
}
pub async fn intersect(c: Ctx) -> R {
    if !need3d(&c) {
        return Ok(());
    }
    if let Some(a) = pick_solids(&c, &c.t("cmd.bool.what.intersect")).await? {
        run_boolean(&c, "intersect", &a, &[]);
    }
    Ok(())
}
pub async fn subtract(c: Ctx) -> R {
    if !need3d(&c) {
        return Ok(());
    }
    let Some(a) = pick_solids(&c, &c.t("cmd.bool.what.from")).await? else { return Ok(()) };
    c.sh().with_ws(|ws, _| ws.clear_sel());
    let Some(b) = pick_solids(&c, &c.t("cmd.bool.what.away")).await? else { return Ok(()) };
    let keep: Vec<f64> = a.iter().filter(|x| !b.contains(x)).copied().collect();
    if keep.is_empty() {
        c.print(&c.t("cmd.bool.same"));
        return Ok(());
    }
    run_boolean(&c, "subtract", &keep, &b);
    Ok(())
}

// ---- 2D commands that act on the 3D selection while the 3D tab is open (commands3d.js `wrap`)

/// The 3D tab's version of a command, while the tab shows.
pub fn wrap3d(sh: &crate::engine::Shared, name: &str) -> Option<fn(Ctx) -> crate::prompt::CmdFuture> {
    if !sh.in3d() {
        return None;
    }
    Some(match name {
        "MOVE" => |c| Box::pin(move3d(c)),
        "COPY" => |c| Box::pin(copy3d(c)),
        "ROTATE" => |c| Box::pin(rotate3d(c)),
        "ERASE" => |c| Box::pin(erase3d(c)),
        "ZOOM" => |c| Box::pin(zoom3d(c)),
        "SELECTALL" => |c| Box::pin(selectall3d(c)),
        "DESELECT" => |c| Box::pin(deselect3d(c)),
        "COPYCLIP" => |c| Box::pin(clip2d(c, "COPYCLIP")),
        "CUTCLIP" => |c| Box::pin(clip2d(c, "CUTCLIP")),
        "COPYBASE" => |c| Box::pin(clip2d(c, "COPYBASE")),
        "PASTECLIP" => |c| Box::pin(clip2d(c, "PASTECLIP")),
        "PASTEORIG" => |c| Box::pin(clip2d(c, "PASTEORIG")),
        "DUPLICATE" => |c| Box::pin(clip2d(c, "DUPLICATE")),
        _ => return None,
    })
}

async fn move3d(c: Ctx) -> R {
    if !select3d(&c, &c.t("cmd3d.edit3d.what_move")).await? {
        return Ok(());
    }
    if drive(&c, "move", c.t("cmd3d.edit3d.move_prompt"), false, false).await? {
        c.print(&c.t("cmd3d.edit3d.moved"));
    }
    Ok(())
}
async fn copy3d(c: Ctx) -> R {
    if !select3d(&c, &c.t("cmd3d.edit3d.what_copy")).await? {
        return Ok(());
    }
    if drive(&c, "move", c.t("cmd3d.edit3d.copy_prompt"), true, false).await? {
        c.print(&c.t("cmd3d.edit3d.copied_type_3x_right"));
    }
    Ok(())
}
async fn rotate3d(c: Ctx) -> R {
    if !select3d(&c, &c.t("cmd3d.edit3d.what_rotate")).await? {
        return Ok(());
    }
    if drive(&c, "rotate", c.t("cmd3d.edit3d.click_center_face_turn"), false, true).await? {
        c.print(&c.t("cmd3d.edit3d.rotated"));
    }
    Ok(())
}
async fn erase3d(c: Ctx) -> R {
    if !select3d(&c, &c.t("cmd3d.edit3d.what_erase")).await? {
        return Ok(());
    }
    let n = c.sh().with_ws(|ws, d| ws.erase_sel(d)).unwrap_or(0);
    c.print(&c.tv("cmd3d.edit3d.erased", &[("n", n.to_string())]));
    Ok(())
}
async fn zoom3d(c: Ctx) -> R {
    let v = c
        .keyword(
            c.t("cmd3d.edit3d.zoom_extents"),
            Opts {
                options: vec![c.opt("E", &c.t("opt.extents")), c.opt("W", &c.t("opt.window"))],
                default: Some(Def::Str("E".into())),
                ..Opts::default()
            },
        )
        .await?;
    if v == Reply::Null {
        return Ok(());
    }
    c.sh().with_ws(|ws, d| ws.fit(d, None));
    c.print(&c.t("cmd3d.edit3d.zoomed_everything_3d_tab"));
    Ok(())
}
async fn selectall3d(c: Ctx) -> R {
    let n = c.sh().with_ws(|ws, _| ws.select_all()).unwrap_or(0);
    c.print(&c.tv("cmd3d.edit3d.selected", &[("n", n.to_string())]));
    Ok(())
}
async fn deselect3d(c: Ctx) -> R {
    c.sh().with_ws(|ws, _| ws.clear_sel());
    Ok(())
}
async fn clip2d(c: Ctx, name: &str) -> R {
    c.print(&c.tv("cmd3d.edit3d.clipboard_2d", &[("name", name.to_string())]));
    Ok(())
}

// ---- drawing units (model3d.js setUnits)

/// `setUnits(doc, units)`: change the drawing's units. Without solids it is a setting (not undone); with solids the 3D
/// model is scaled by the ratio of meters per unit so it stays on the drawing, and the units and the scaling are one
/// undo step. → (scaled, ratio)
pub fn set_units(sh: &crate::engine::Shared, units: &str) -> (bool, f64) {
    let faces = sh.t("model3d.faces_file");
    let label = sh.t("model3d.drawing_units");
    let before = {
        let d = sh.doc.borrow();
        d.meta.get("units").filter(|v| js::truthy(Some(v))).map(js::to_js_string).unwrap_or_default()
    };
    let after = units.to_string();
    let ratio = m3::m_per_unit(&after) / m3::m_per_unit(&before);
    if before == after {
        return (false, 1.0);
    }
    // the model in hand: the 3D tab's, else the drawing's
    let has_ws = sh.ws3d.borrow().is_some();
    let empty = if has_ws {
        sh.with_ws(|ws, _| m3::is_empty(&ws.tools.host.model)).unwrap_or(true)
    } else {
        m3::is_empty(&m3::model_of(&sh.doc.borrow(), &faces))
    };
    if empty || (ratio - 1.0).abs() < 1e-12 {
        let mut d = sh.doc.borrow_mut();
        d.meta.insert("units".into(), serde_json::Value::String(after.clone()));
        d.version = d.version.wrapping_add(1);
        // `m?.syncUnits()`: the model in hand speaks the new unit (not a step)
        if let Some(cur) = d.m3d.clone() {
            let mut m = Model::new();
            if m.restore(&cc_m3d::model::SnapRef::Str(std::rc::Rc::from(&*cur))).is_ok() {
                m.units = m3::display_unit(&after).to_string();
                d.m3d = Some(std::sync::Arc::from(m.snapshot().as_str()));
            }
        }
        drop(d);
        sh.with_ws(|ws, d| {
            ws.tools.host.model.units = m3::display_unit(&after).to_string();
            ws.sync_to(d);
        });
        return (false, ratio);
    }
    let scale = |m: &mut Model| -> cc_m3d::Res<()> {
        m.change(&label, |mm| {
            // every vertex and every placement's offset × ratio (placements stay rigid; a uniform scale commutes with them)
            for d in mm.defs.values_mut() {
                for p in d.verts.values_mut() {
                    p[0] *= ratio;
                    p[1] *= ratio;
                    p[2] *= ratio;
                }
                for inst in d.insts.values_mut() {
                    inst.m[12] *= ratio;
                    inst.m[13] *= ratio;
                    inst.m[14] *= ratio;
                }
                d.cache = None;
            }
            for g in mm.guides.values_mut() {
                match g {
                    cc_m3d::model::Guide::Line { p, .. } | cc_m3d::model::Guide::Point { p } => {
                        *p = [p[0] * ratio, p[1] * ratio, p[2] * ratio]
                    }
                }
            }
            Ok(())
        })
    };
    let rec = |d: &mut Doc, m: &mut Model| {
        let step = m.undo_stack.pop();
        m.undo_stack.clear();
        m.redo_stack.clear();
        let Some(step) = step else { return };
        let (b, a) = (step.b.s(), step.a.s());
        let bref = d.m3d.clone().filter(|x| **x == *b).unwrap_or_else(|| std::sync::Arc::from(b.as_str()));
        if !d.meta.contains_key("units") {
            // `before` is `doc.meta.units || ''`: an undo leaves '' there
            d.meta.insert("units".into(), serde_json::Value::String(String::new()));
        }
        let mut patch = cc_cad::Obj::new();
        patch.insert("units".into(), serde_json::Value::String(after.clone()));
        let _ = d.change("UNITS", |dd| {
            dd.set_meta(&patch)?;
            dd.record_m3d(&label, Some(bref), std::sync::Arc::from(a.as_str()))
        });
        // `m.syncUnits()` after the step: the model in hand speaks the new unit; the step keeps the old one
        m.units = m3::display_unit(&after).to_string();
        d.m3d = Some(std::sync::Arc::from(m.snapshot().as_str()));
    };
    if has_ws {
        sh.with_ws(|ws, d| {
            let m = &mut ws.tools.host.model;
            m.undo_stack.clear();
            if scale(m).is_ok() {
                rec(d, m);
            }
            ws.sync_to(d);
        });
    } else {
        let mut m = m3::model_of(&sh.doc.borrow(), &faces);
        if scale(&mut m).is_ok() {
            rec(&mut sh.doc.borrow_mut(), &mut m);
        }
    }
    (true, ratio)
}

fn faces_name(c: &Ctx) -> String {
    c.t("model3d.faces_file")
}

type Snap = std::sync::Arc<str>;

thread_local! {
    /// The model the last 3D command left, with the snapshot it matches: the next one starts from it while the drawing
    /// still holds that snapshot (undo, redo or another drawing hand over a different one, and it is read again).
    static LIVE: std::cell::RefCell<Option<(Snap, Model)>> = const { std::cell::RefCell::new(None) };
    /// The snapshot the model in hand matches (set by `model_before` and `record`), for `keep`.
    static SRC: std::cell::RefCell<Option<Snap>> = const { std::cell::RefCell::new(None) };
}

/// doc.js load → `model3d.fromFile`: the model a file brought, read when the drawing is opened (not when it is first
/// used) and kept as the drawing's snapshot; the model itself waits for the first 3D command or the 3D tab.
pub fn preload(sh: &crate::engine::Shared) {
    let faces = sh.t("model3d.faces_file");
    let mut d = sh.doc.borrow_mut();
    if d.m3d.is_some() || (d.model3d_text.is_none() && d.faces3d.is_empty()) {
        return;
    }
    let mut m = m3::from_file(&d, &faces);
    if m3::is_empty(&m) {
        return;
    }
    let s: Snap = std::sync::Arc::from(m.snapshot().as_str());
    d.m3d = Some(std::sync::Arc::clone(&s));
    LIVE.with(|l| *l.borrow_mut() = Some((s, m)));
}

/// The model `preload` (or the last 3D command) left, when the drawing still holds its snapshot (the 3D tab takes it).
pub fn take_live(doc: &Doc) -> Option<Model> {
    let cur = doc.m3d.as_ref()?;
    let (la, m) = LIVE.with(|l| l.borrow_mut().take())?;
    std::sync::Arc::ptr_eq(&la, cur).then_some(m)
}

/// The drawing's model (the one the last 3D command left while the drawing still holds its snapshot).
fn model_now(c: &Ctx) -> Model {
    // the 3D tab's model is the drawing's live model (model3d.js keeps one object; a snapshot read again would check,
    // and refuse, what the live model already holds)
    if let Some(m) = c.sh().with_ws(|ws, d| {
        SRC.with(|s| *s.borrow_mut() = d.m3d.clone());
        let mut m = ws.tools.host.model.clone();
        m.old_previews = false;
        m
    }) {
        return m;
    }
    let cur = c.doc().m3d.clone();
    SRC.with(|s| *s.borrow_mut() = cur.clone());
    if let Some(a) = &cur
        && let Some((la, m)) = LIVE.with(|l| l.borrow_mut().take())
        && std::sync::Arc::ptr_eq(&la, a)
    {
        return m;
    }
    m3::model_of(&c.doc(), &faces_name(c))
}

/// Read the drawing's model without changing it (VIEWBASE): the one the last 3D command left, kept for the next.
pub(crate) fn with_model<T>(c: &Ctx, f: impl FnOnce(&Model) -> T) -> T {
    let m = model_now(c);
    let r = f(&m);
    keep(c, m);
    r
}

/// The drawing's model and its snapshot before an edit.
fn model_before(c: &Ctx) -> (Model, String) {
    let mut m = model_now(c);
    let s = match c.doc().m3d.as_ref() {
        Some(a) => a.to_string(),
        None => m.snapshot(),
    };
    (m, s)
}

/// Keep the model for the next 3D command when the drawing holds the snapshot it matches.
fn keep(c: &Ctx, m: Model) {
    let src = SRC.with(|s| s.borrow_mut().take());
    // straight to the 3D tab (not through `with_ws`, whose sync would read the new snapshot again first)
    if let Some(ws) = c.sh().ws3d.borrow_mut().as_mut() {
        ws.adopt(m, src.as_ref(), &c.doc());
        return;
    }
    if let (Some(src), Some(cur)) = (src, c.doc().m3d.clone())
        && std::sync::Arc::ptr_eq(&src, &cur)
    {
        LIVE.with(|l| *l.borrow_mut() = Some((cur, m)));
    }
}

/// `Model3D.change` inside a `doc.change` already open (or a change of its own): the model's new state recorded when
/// it differs from `before`.
fn record(d: &mut Doc, label: &str, m: &mut Model, before: &str) -> Result<(), cc_cad::DocError> {
    // the kernel keeps each definition's JSON until it is edited in place: the solids here only add definitions and
    // place them at the root, so only the root's is stale
    if let Some(r) = m.root_mut() {
        r.cache = None;
    }
    m.bump_safe();
    let after = m.snapshot();
    if after == before {
        return Ok(());
    }
    let b: Snap = d.m3d.clone().unwrap_or_else(|| std::sync::Arc::from(before));
    let a: Snap = std::sync::Arc::from(after.as_str());
    SRC.with(|s| *s.borrow_mut() = Some(std::sync::Arc::clone(&a)));
    d.record_m3d(label, Some(b), a)
}

/// `volumeText(m3, iids)`
fn volume_text(c: &Ctx, m: &Model, iids: &[f64]) -> String {
    let k = m3::k_of(&c.doc());
    volume_of(c, m, iids, k)
}

fn t1(c: &Ctx) -> impl Fn(&str) -> String + '_ {
    move |k: &str| c.t(k)
}
fn t2(c: &Ctx) -> impl Fn(&str, &[(&str, String)]) -> String + '_ {
    move |k: &str, v: &[(&str, String)]| c.tv(k, v)
}

/// `outline2d(e)` → the ring, or the message why not.
fn outline(c: &Ctx, e: &cc_cad::entity::Entity) -> Result<Vec<[f64; 2]>, String> {
    m3::outline2d(e).map_err(|w| match w {
        NotClosed::Key(k) => c.t(k),
        NotClosed::What(k) => c.tv("model3d.not_closed", &[("what", c.t(k))]),
    })
}

/// A height: typed (drawing units, negative = down). Null = Enter; Opt = an option.
async fn ask_height(c: &Ctx, rings: Vec<Vec<[f64; 2]>>, z0: f64, label: String, options: Vec<Opt>) -> R<Reply> {
    let height = Some(crate::ws3d::HeightSpec { rings, z0, base: None });
    loop {
        let v = c.number(label.clone(), Opts { options: options.clone(), height: height.clone(), ..Opts::default() }).await?;
        match v {
            Reply::Opt(_) => return Ok(v),
            Reply::Num(h) => {
                if h.abs() < 1e-9 {
                    c.print(&c.t("cmd3d.height_must_not_zero"));
                    continue;
                }
                return Ok(v);
            }
            _ => return Ok(Reply::Null),
        }
    }
}

struct Shape {
    id: u64,
    ring: Vec<[f64; 2]>,
}

fn shapes_of(c: &Ctx, ids: &[u64]) -> Vec<Shape> {
    let mut out = Vec::new();
    for id in ids {
        let e = c.doc().get(*id).cloned();
        let Some(e) = e else { continue };
        match outline(c, &e) {
            Ok(ring) => out.push(Shape { id: *id, ring }),
            Err(m) => c.print(&m),
        }
    }
    out
}

fn uniq(v: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in v {
        if !out.contains(&s) {
            out.push(s);
        }
    }
    out
}

fn scaled(ring: &[[f64; 2]], k: f64) -> Vec<[f64; 2]> {
    ring.iter().map(|[x, y]| [x * k, y * k]).collect()
}

// ---- EXTRUDE

pub async fn extrude(c: Ctx) -> R {
    let set = c.select(Some(c.t("cmd3d.extrude.select_closed_objects_extrude")), false).await?;
    let shapes = shapes_of(&c, &set.0);
    if shapes.is_empty() {
        c.print(&c.t("cmd3d.extrude.nothing_extrude_pick_closed"));
        return Ok(());
    }
    if !c.doc().in_model() {
        c.print(&c.t("cmd3d.extrude.extrude_works_model_model"));
        return Ok(());
    }
    c.set_selection(&shapes.iter().map(|s| s.id).collect::<Vec<_>>());
    let mut del = c.doc().current.delobj.unwrap_or(false);
    let mut taper = 0.0;
    let h;
    loop {
        let dlabel = if del { c.t("cmd3d.extrude.keep_originals") } else { c.t("cmd3d.extrude.delete_originals") };
        let options = vec![c.opt("D", &dlabel), c.opt("T", &c.t("opt.taper")), c.opt("P", &c.t("opt.path"))];
        let rings: Vec<Vec<[f64; 2]>> = shapes.iter().map(|s| s.ring.clone()).collect();
        let v = ask_height(&c, rings, 0.0, c.t("cmd3d.extrude.specify_height_extrusion"), options).await?;
        match v {
            Reply::Null => return Ok(()),
            Reply::Opt(o) if o == "T" => {
                let a = c
                    .number(c.tv("cmd.extrude.taper", &[("cur", fmt(taper))]), Opts { default: Some(Def::Num(taper)), ..Opts::default() })
                    .await?;
                match a {
                    Reply::Num(a) if a.is_finite() && a.abs() < 89.5 => {
                        taper = a;
                        c.print(&c.tv("cmd.extrude.taper.set", &[("a", fmt(a))]));
                    }
                    Reply::Null => {}
                    _ => c.print(&c.t("cmd.shape3d.taper.range")),
                }
            }
            Reply::Opt(o) if o == "P" => return sweep_shapes(&c, &shapes, del).await,
            Reply::Opt(_) => {
                del = !del;
                c.sh().doc.borrow_mut().current.delobj = Some(del);
                c.print(&c.t(if del { "cmd3d.extrude.2d_objects_will_deleted" } else { "cmd3d.extrude.2d_objects_stay_after" }));
            }
            Reply::Num(x) => {
                h = x;
                break;
            }
            _ => return Ok(()),
        }
    }
    let k = m3::k_of(&c.doc());
    let (mut m, before) = model_before(&c);
    let mut made: Vec<(u64, f64)> = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    {
        let (t, tv) = (t1(&c), t2(&c));
        for s in &shapes {
            let ring = scaled(&s.ring, k);
            let r = if taper != 0.0 {
                m3::make_tapered(&mut m, &ring, 0.0, h * k, taper * std::f64::consts::PI / 180.0, &tv)
            } else {
                m3::make_prism(&mut m, &ring, 0.0, h * k, "Extrusion", &t)
            };
            match r {
                Ok(iid) => made.push((s.id, iid)),
                Err(e) => failed.push(e),
            }
        }
    }
    c.change("EXTRUDE", |d| {
        record(d, "Extrude", &mut m, &before)?;
        if del {
            for (id, _) in &made {
                d.remove(*id)?;
            }
        }
        Ok(())
    })?;
    for f in uniq(failed) {
        c.print(&f);
    }
    if !made.is_empty() {
        let iids: Vec<f64> = made.iter().map(|x| x.1).collect();
        let n = made.len().to_string();
        let parts = [
            c.tv("cmd3d.extrude.made", &[("n", n.clone()), ("h", fmt(h)), ("vol", volume_text(&c, &m, &iids))]),
            if del { c.t("cmd3d.extrude.deleted_2d") } else { String::new() },
            if in3d(&c) { String::new() } else { c.tv("cmd3d.open_3d_tab", &[("n", n)]) },
        ];
        c.print(&parts.iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(" "));
    }
    keep(&c, m);
    Ok(())
}

// ---- SWEEP, REVOLVE (shapes3d.js)

/// `closedShapes(c, prompt)`
async fn closed_shapes(c: &Ctx, prompt: String) -> R<Option<Vec<Shape>>> {
    let set = c.select(Some(prompt), false).await?;
    let shapes = shapes_of(c, &set.0);
    if shapes.is_empty() {
        if !set.0.is_empty() {
            c.print(&c.t("cmd.shape3d.none"));
        }
        return Ok(None);
    }
    if !c.doc().in_model() {
        c.print(&c.t("cmd.shape3d.model"));
        return Ok(None);
    }
    Ok(Some(shapes))
}

/// `pathOf(e)`: a path's points (arcs in 7.5° steps) and whether it closes.
fn path_of(e: &cc_cad::entity::Entity) -> Option<(Vec<[f64; 2]>, bool)> {
    match &e.geo {
        Geo::Circle(_) => m3::outline2d(e).ok().map(|p| (p, true)),
        Geo::Ellipse(el) => m3::outline2d(e).ok().map(|p| (p, !((el.t1 - el.t0).abs() < TAU - 1e-6))),
        Geo::Line(_) | Geo::Poly(_) | Geo::Arc(_) => {
            let mut pts: Vec<[f64; 2]> = Vec::new();
            let push = |pts: &mut Vec<[f64; 2]>, x: f64, y: f64| {
                if pts.last().is_none_or(|l| js::hypot(l[0] - x, l[1] - y) > 1e-9) {
                    pts.push([x, y]);
                }
            };
            for q in prims(e, None) {
                if q.is_l() {
                    push(&mut pts, q.x1, q.y1);
                    push(&mut pts, q.x2, q.y2);
                    continue;
                }
                let m = ((q.a1 - q.a0) % TAU + TAU) % TAU;
                let sw = if m == 0.0 || m.is_nan() { TAU } else { m };
                let nf = js::max(2.0, (sw / (std::f64::consts::PI / 24.0)).ceil());
                let n = if nf.is_finite() { nf.min(1.0e5) } else { 2.0 };
                let (from, dir) = if q.rev.unwrap_or(false) { (q.a1, -1.0) } else { (q.a0, 1.0) };
                let mut i = 0.0;
                while i <= n {
                    let a = from + dir * sw * i / n;
                    push(&mut pts, q.cx + q.r * a.jcos(), q.cy + q.r * a.jsin());
                    i += 1.0;
                }
            }
            let closed = matches!(&e.geo, Geo::Poly(p) if p.is_closed());
            (pts.len() >= 2).then_some((pts, closed))
        }
        _ => None,
    }
}

async fn sweep_shapes(c: &Ctx, shapes: &[Shape], del: bool) -> R {
    let (path, pid) = loop {
        let pick = c.entity(c.t("cmd.sweep.path"), Opts { none: true, ..Opts::default() }).await?;
        let Reply::Pick { id, .. } = pick else { return Ok(()) };
        let e = c.doc().get(id).cloned();
        if let Some(p) = e.as_ref().and_then(path_of) {
            break (p, id);
        }
        c.print(&c.t("cmd.sweep.notpath"));
    };
    let k = m3::k_of(&c.doc());
    let (mut m, before) = model_before(c);
    let mut made: Vec<(u64, f64)> = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    {
        let tv = t2(c);
        let pp = scaled(&path.0, k);
        for s in shapes.iter().filter(|s| s.id != pid) {
            match m3::make_swept(&mut m, &scaled(&s.ring, k), &pp, path.1, &tv) {
                Ok(iid) => made.push((s.id, iid)),
                Err(e) => failed.push(e),
            }
        }
    }
    c.change("SWEEP", |d| {
        record(d, "Sweep", &mut m, &before)?;
        if del {
            for (id, _) in &made {
                d.remove(*id)?;
            }
        }
        Ok(())
    })?;
    for f in uniq(failed) {
        c.print(&f);
    }
    if !made.is_empty() {
        let iids: Vec<f64> = made.iter().map(|x| x.1).collect();
        let see = if in3d(c) { String::new() } else { format!(" {}", c.t("cmd.shape3d.see")) };
        c.print(&format!("{}{see}", c.tv("cmd.sweep.done", &[("n", made.len().to_string()), ("volume", volume_text(c, &m, &iids))])));
    }
    keep(c, m);
    Ok(())
}

pub async fn sweep(c: Ctx) -> R {
    let Some(shapes) = closed_shapes(&c, c.t("cmd.sweep.select")).await? else { return Ok(()) };
    let del = c.doc().current.delobj.unwrap_or(false);
    sweep_shapes(&c, &shapes, del).await
}

pub async fn revolve(c: Ctx) -> R {
    let Some(shapes) = closed_shapes(&c, c.t("cmd.revolve.select")).await? else { return Ok(()) };
    c.set_selection(&shapes.iter().map(|s| s.id).collect::<Vec<_>>());
    let a0 = c
        .point(
            c.t("cmd.revolve.axis1"),
            Opts { options: vec![c.opt("X", &c.t("opt.xaxis")), c.opt("Y", &c.t("opt.yaxis"))], ..Opts::default() },
        )
        .await?;
    let (a, b) = match a0 {
        Reply::Opt(o) if o == "X" => ([0.0, 0.0], [1.0, 0.0]),
        Reply::Opt(o) if o == "Y" => ([0.0, 0.0], [0.0, 1.0]),
        Reply::Point(a, _) => {
            c.preview(Some(std::rc::Rc::new(move |_, cur| vec![seg(a, cur)])));
            // a question that ends the command leaves the preview to the engine, as the JS's await throws past it
            let b = c.point(c.t("cmd.revolve.axis2"), Opts { base: Some(a), ..Opts::default() }).await?;
            c.preview(None);
            let Reply::Point(b, _) = b else { return Ok(()) };
            ([a.x, a.y], [b.x, b.y])
        }
        _ => return Ok(()),
    };
    let base = cc_geom::P2::new(a[0], a[1]);
    let ang = c.angle(c.t("cmd.revolve.angle"), Opts { base: Some(base), default: Some(Def::Num(TAU)), ..Opts::default() }).await?;
    let Reply::Num(ang) = ang else { return Ok(()) };
    let th = if ang.abs() < 1e-9 { TAU } else { ang };
    let k = m3::k_of(&c.doc());
    let del = c.doc().current.delobj.unwrap_or(false);
    let (mut m, before) = model_before(&c);
    let mut made: Vec<(u64, f64)> = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    {
        let tv = t2(&c);
        for s in &shapes {
            match m3::make_revolved(&mut m, &scaled(&s.ring, k), [a[0] * k, a[1] * k], [b[0] * k, b[1] * k], th, &tv) {
                Ok(iid) => made.push((s.id, iid)),
                Err(e) => failed.push(e),
            }
        }
    }
    c.change("REVOLVE", |d| {
        record(d, "Revolve", &mut m, &before)?;
        if del {
            for (id, _) in &made {
                d.remove(*id)?;
            }
        }
        Ok(())
    })?;
    for f in uniq(failed) {
        c.print(&f);
    }
    if !made.is_empty() {
        let iids: Vec<f64> = made.iter().map(|x| x.1).collect();
        let see = if in3d(&c) { String::new() } else { format!(" {}", c.t("cmd.shape3d.see")) };
        c.print(&format!(
            "{}{see}",
            c.tv(
                "cmd.revolve.done",
                &[("n", made.len().to_string()), ("deg", fmt(th * 180.0 / std::f64::consts::PI)), ("volume", volume_text(&c, &m, &iids))]
            )
        ));
    }
    keep(&c, m);
    Ok(())
}

// ---- BOX, WEDGE, CYLINDER

struct Base {
    a: [f64; 2],
    b: [f64; 2],
    /// the first corner's z (a point picked on the 3D tab), else 0
    z0: f64,
}

impl Base {
    fn ring(&self) -> Vec<[f64; 2]> {
        vec![self.a, [self.b[0], self.a[1]], self.b, [self.a[0], self.b[1]]]
    }
}

/// The z of the point just answered (`Number.isFinite(p.z) ? p.z : 0`).
fn z_of(c: &Ctx) -> f64 {
    c.st().z3.filter(|z| z.is_finite()).unwrap_or(0.0)
}

/// `" " + t('cmd3d.open_3d_tab')` unless the 3D tab shows.
fn see_tab(c: &Ctx) -> String {
    if in3d(c) { String::new() } else { format!(" {}", c.tv("cmd3d.open_3d_tab", &[("n", "1".into())])) }
}

/// `baseCorners(c)`
async fn base_corners(c: &Ctx) -> R<Option<Base>> {
    let Reply::Point(a, _) = c.point(c.t("cmd3d.revolve.specify_first_corner"), Opts::default()).await? else { return Ok(None) };
    let z0 = z_of(c);
    c.preview(Some(std::rc::Rc::new(move |_, cur| {
        let v = |x: f64, y: f64| cc_cad::entity::Vertex { x, y, b: None, extra: Default::default() };
        vec![Entity::new(Geo::Poly(cc_cad::entity::Poly {
            pts: vec![v(a.x, a.y), v(cur.x, a.y), v(cur.x, cur.y), v(a.x, cur.y)],
            closed: Some(true),
        }))]
    })));
    let b = c.point(c.t("cmd3d.revolve.specify_other_corner"), Opts { base: Some(a), ..Opts::default() }).await?;
    c.preview(None);
    let Reply::Point(b, _) = b else { return Ok(None) };
    if (b.x - a.x).abs() < 1e-9 || (b.y - a.y).abs() < 1e-9 {
        c.print(&c.t("cmd3d.revolve.base_has_no_area"));
        return Ok(None);
    }
    Ok(Some(Base { a: [a.x, a.y], b: [b.x, b.y], z0 }))
}

/// `m3.change(label, fn)` with no change open: a step of its own, labelled as the model's change.
fn change3d(c: &Ctx, label: &str, f: impl FnOnce(&mut Model) -> Result<f64, String>) -> R<(Model, f64)> {
    let (mut m, before) = model_before(c);
    let iid = f(&mut m).map_err(CmdError::Msg)?;
    c.change(label, |d| record(d, label, &mut m, &before))?;
    Ok((m, iid))
}

pub async fn box3d(c: Ctx) -> R {
    let Some(base) = base_corners(&c).await? else { return Ok(()) };
    let Reply::Num(h) = ask_height(&c, vec![base.ring()], base.z0, c.t("cmd3d.specify_height"), Vec::new()).await? else { return Ok(()) };
    let k = m3::k_of(&c.doc());
    let (m, iid) = {
        let t = t1(&c);
        change3d(&c, "Box", |m| m3::make_box(m, [base.a[0] * k, base.a[1] * k], [base.b[0] * k, base.b[1] * k], base.z0 * k, h * k, &t))?
    };
    let vars = [
        ("x", fmt((base.b[0] - base.a[0]).abs())),
        ("y", fmt((base.b[1] - base.a[1]).abs())),
        ("z", fmt(h.abs())),
        ("vol", volume_text(&c, &m, &[iid])),
    ];
    c.print(&format!("{}{}", c.tv("cmd3d.box.made", &vars), see_tab(&c)));
    keep(&c, m);
    Ok(())
}

pub async fn wedge(c: Ctx) -> R {
    let Some(base) = base_corners(&c).await? else { return Ok(()) };
    let Reply::Num(h) = ask_height(&c, vec![base.ring()], base.z0, c.t("cmd3d.specify_height"), Vec::new()).await? else { return Ok(()) };
    let k = m3::k_of(&c.doc());
    let (m, iid) = {
        let t = t1(&c);
        change3d(&c, "Wedge", |m| {
            m3::make_wedge(m, [base.a[0] * k, base.a[1] * k], [base.b[0] * k, base.b[1] * k], base.z0 * k, h * k, &t)
        })?
    };
    c.print(&format!("{}{}", c.tv("cmd3d.wedge.made", &[("vol", volume_text(&c, &m, &[iid]))]), see_tab(&c)));
    keep(&c, m);
    Ok(())
}

pub async fn cylinder(c: Ctx) -> R {
    let Reply::Point(ctr, _) = c.point(c.t("cmd3d.cylinder.specify_center_point_base"), Opts::default()).await? else { return Ok(()) };
    let z0 = z_of(&c);
    c.preview(Some(std::rc::Rc::new(move |_, cur| vec![Entity::new(circle_g(ctr.x, ctr.y, js::hypot(cur.x - ctr.x, cur.y - ctr.y)))])));
    let r = c.distance(c.t("cmd3d.cylinder.specify_base_radius"), Opts { base: Some(ctr), ..Opts::default() }).await?;
    c.preview(None);
    let r = match r {
        Reply::Num(r) if r > 0.0 => r,
        Reply::Num(_) => {
            c.print(&c.t("cmd3d.cylinder.radius_must_more_than"));
            return Ok(());
        }
        _ => return Ok(()),
    };
    let ring: Vec<[f64; 2]> = (0..48)
        .map(|i| {
            let a = (f64::from(i) / 48.0) * std::f64::consts::PI * 2.0;
            [ctr.x + r * a.jcos(), ctr.y + r * a.jsin()]
        })
        .collect();
    let Reply::Num(h) = ask_height(&c, vec![ring], z0, c.t("cmd3d.specify_height"), Vec::new()).await? else { return Ok(()) };
    let k = m3::k_of(&c.doc());
    let (m, iid) = {
        let t = t1(&c);
        change3d(&c, "Cylinder", |m| m3::make_cylinder(m, [ctr.x * k, ctr.y * k], r * k, z0 * k, h * k, &t))?
    };
    c.print(&format!(
        "{}{}",
        c.tv("cmd3d.cylinder.made", &[("r", fmt(r)), ("h", fmt(h.abs())), ("vol", volume_text(&c, &m, &[iid]))]),
        see_tab(&c)
    ));
    keep(&c, m);
    Ok(())
}

// ---- files

fn export3d(c: &Ctx, kind: &str) {
    let m = model_now(c);
    if m3::is_empty(&m) {
        c.print(&c.t("cmd3d.export3d.3d_model_empty_make"));
        return;
    }
    let base = c.doc().meta.get("name").filter(|v| js::truthy(Some(v))).map(js::to_js_string).unwrap_or_else(|| "Drawing1".into());
    let name = format!("{base}.{kind}");
    // CadCraft 0.5's copy of the kernel triangulates every face with earcut
    let (data, mime): (Vec<u8>, &str) = cc_m3d::def::with_plain_face_tris(|| match kind {
        "glb" => (cc_io_m3d::gltf::export_glb(&m), "model/gltf-binary"),
        "obj" => (cc_io_m3d::obj::export_obj(&m, None).into_bytes(), "text/plain"),
        _ => (cc_io_m3d::stl::export_stl(&m), "model/stl"),
    });
    if !c.sh().host.borrow().can_save_blob() {
        c.print(&c.t("cmd3d.export3d.downloading_not_available_here"));
        keep(c, m);
        return;
    }
    c.sh().host.borrow_mut().save_blob(&name, &data, mime);
    c.print(&c.tv(&format!("cmd3d.export3d.saved_{kind}"), &[("name", name)]));
    keep(c, m);
}

pub async fn export3d_cmd(c: Ctx) -> R {
    let v = c
        .keyword(
            c.t("cmd3d.export3d.export_3d_model"),
            Opts {
                options: vec![c.opt("G", "GLB"), c.opt("O", "OBJ"), c.opt("S", "STL")],
                default: Some(Def::Str("G".into())),
                ..Opts::default()
            },
        )
        .await?;
    let k = match &v {
        Reply::Opt(o) => o.clone(),
        Reply::Str(s) if s == "G" => "G".into(),
        _ => return Ok(()),
    };
    let kind = match k.as_str() {
        "G" => "glb",
        "O" => "obj",
        _ => "stl",
    };
    export3d(&c, kind);
    Ok(())
}
pub async fn stlout(c: Ctx) -> R {
    export3d(&c, "stl");
    Ok(())
}
pub async fn objout(c: Ctx) -> R {
    export3d(&c, "obj");
    Ok(())
}
pub async fn glbout(c: Ctx) -> R {
    export3d(&c, "glb");
    Ok(())
}
