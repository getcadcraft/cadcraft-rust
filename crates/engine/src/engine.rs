//! The command line: commands.js `Engine` and the prompt API commands are written in (`c.point`, `c.select`, …).
//!
//! Input reaches the engine as [`Engine::text`] (a typed line: Enter / Space sends it), [`Engine::click`] (a click:
//! the point after snaps and ortho, and the raw cursor), [`Engine::window`] (a selection window), [`Engine::path`]
//! (a freehand drag where TRIM / EXTEND take one) and [`Engine::escape`]. A command is an `async` body (see
//! [`crate::prompt`]) that asks through [`Ctx`]; every edit it makes is folded into one undo step when it ends.
//!
//! The rich question (options, defaults, base point, the selection being built) and the rich answer live in the
//! engine's state next to the [`crate::prompt::Runner`] slot, which only carries the suspension.

use std::cell::{Ref, RefCell, RefMut};
use std::rc::Rc;

use cc_cad::entity::Entity;
use cc_cad::{Doc, js};
use cc_cad_geom::geom::{Bounds, angle_of, dist, rad};
use cc_cad_geom::pick::{Circ, fence_hits};
use cc_cad_geom::units::parse_length;
use cc_geom::P2;

use crate::cmd;
use crate::input::{Def, Opt, PointCtx, is_plain_number, match_option, parse_point, plain_number, prompt_text, select_options};
use crate::lang::Lang;
use crate::prompt::{Answer, CmdError, Kind, Prompter, Request, Runner};
use crate::ucs::{active_ucs, ang_of};

/// What the engine needs from the window (cadcraft.js in the browser, a stub in tests and the CLI).
pub trait Host {
    fn print(&mut self, s: &str);
    fn prompt(&mut self, s: &str);
    /// The cursor in world coordinates (None before the pointer has been over the drawing).
    fn cursor(&self) -> Option<P2>;
    /// Shift held.
    fn shift(&self) -> bool {
        false
    }
    fn selection(&self) -> Vec<u64>;
    fn set_selection(&mut self, ids: &[u64]);
    /// The object under a point, if any.
    fn pick_at(&self, doc: &Doc, p: P2) -> Option<u64>;
    /// The objects a window (or crossing window) selects.
    fn pick_window(&self, doc: &Doc, w: &Bounds, crossing: bool) -> Vec<u64>;
    /// Rubber-band entities to draw over the drawing (handed over: the host keeps them without a copy).
    fn preview(&mut self, ents: Vec<Entity>);
    fn on_command(&mut self, _name: Option<&str>) {}
    fn zoom_extents(&mut self);
    /// `exact`: ZOOM Center's window, not padded.
    fn zoom_window(&mut self, a: P2, b: P2, exact: bool);
    /// ZOOM Previous → false when there is no previous view (or the host has none).
    fn zoom_previous(&mut self) -> bool {
        false
    }
    fn zoom_by(&mut self, _factor: f64) {}
    /// The view on screen (width, height) in drawing units, when the host knows it.
    fn view_rect(&self) -> Option<(f64, f64)> {
        None
    }
    /// `host.props.open()` / `.close()` (PROPERTIES / PROPERTIESCLOSE)
    fn props(&mut self, _open: bool) {}
    /// `host.props.pickLabel()` (_PROPPICK): what the panel's pick button asks for ("Start point"); None = no panel.
    fn prop_pick_label(&self) -> Option<String> {
        None
    }
    /// `host.props.picked(p)` (_PROPPICK): the point picked for the panel.
    fn prop_picked(&mut self, _p: P2) {}
    /// `host.props.toggleQuick()` (QPMODE)
    fn quick_props(&mut self) {}
    /// `host.plot(c)`: open the Plot dialog → false when the host has none (PLOT then says plotting needs the browser).
    fn plot(&mut self) -> bool {
        false
    }
    /// The window `_PLOTWIN` picked for the Plot dialog (None: cancelled).
    fn plot_window(&mut self, _w: Option<Bounds>) {}
    /// `host.viewExtents()`: the model area on screen (HATCH looks for boundaries there only), when the host has one.
    fn view_extents(&self) -> Option<Bounds> {
        None
    }
    /// The shared clipboard store (another drawing, window or tab pastes from it): keep this copy → false when the
    /// host keeps none (the engine's own copy only; the browser's "too big to share").
    fn clip_put(&mut self, _clip: &crate::clip::Clip) -> bool {
        false
    }
    /// The shared clipboard store's contents, if any (text is untrusted: read through `clip::read_clip`).
    fn clip_get(&self) -> Option<crate::clip::Shared> {
        None
    }
    /// Settings › CadCraft › paste base ('lower-left' | 'center'), when the host has settings.
    fn paste_base(&self) -> Option<String> {
        None
    }
    /// `host.quickSelect`: open the Quick select dialog (it applies its rule itself) → false when there is none
    /// (QSELECT then asks on the command line).
    fn quick_select(&mut self) -> bool {
        false
    }
    /// `host.findDialog`: open the Find and replace dialog → false when there is none (FIND asks on the command line).
    fn find_dialog(&mut self) -> bool {
        false
    }
    /// The time now, in ms since 1970 (a new sheet's title block dates itself; the engine reads no clock itself).
    fn now_ms(&self) -> Option<f64> {
        None
    }
    /// The view's centre on screen (NAMEDVIEW saves it with `view_rect`'s size).
    fn view_center(&self) -> Option<P2> {
        None
    }
    /// `host.setPanes`: split screen (VPORTS in the model) → false when the host has none.
    fn has_panes(&self) -> bool {
        false
    }
    fn set_panes(&mut self, _layout: &str) {}
    /// `host.linkViews()` (LINKVIEWS): whether the panes' views are linked; None when the host has no panes.
    fn link_views(&self) -> Option<bool> {
        None
    }
    fn set_link_views(&mut self, _on: bool) {}
    /// `host.openIn3d()` (OPENIN3D): hand the drawing to the 3D modeling app → the app's name, or (code, message) when
    /// it failed ('blocked' | 'storage' | other); None when the host has no such app.
    fn open_in_3d(&mut self) -> Option<Result<String, (String, String)>> {
        None
    }
    /// OPENIN3D where no 3D modeling app takes the drawing: true = show it on this host's own 3D tab instead (the
    /// window; fork-only, 0.5's page prints `cmd.open3d.none`); false (the default) = 0.5's line.
    fn open_in_3d_tab(&self) -> bool {
        false
    }
    /// `host.settings.lasso` (LASSOMODE): press and drag draws a lasso (true) or a rectangle; None = no such setting.
    fn lasso(&self) -> Option<bool> {
        None
    }
    fn set_lasso(&mut self, _on: bool) {}
    /// `host.shortcuts.open()` (SHORTCUTS) → false when the host has no keys editor.
    fn shortcuts(&mut self) -> bool {
        false
    }
    /// `host.sheetSet` (SHEETSET): show the Sheet Set panel → None when the host has none, else the open set's name.
    fn sheet_set(&mut self) -> Option<Option<String>> {
        None
    }
    /// `host.compareSettings()` (COMPARESETTINGS) → false when the host has no such dialog.
    fn compare_settings(&mut self) -> bool {
        false
    }
    /// `host.enterViewport`: working on the model through a sheet's viewport (MSPACE / PSPACE) → false when the host
    /// has none.
    fn has_viewports(&self) -> bool {
        false
    }
    /// The viewport being worked in: (its sheet's space, its id).
    fn viewport_entered(&self) -> Option<(cc_cad::SpaceId, u64)> {
        None
    }
    /// The viewport (turned on) under a point of the open sheet.
    fn viewport_at(&self, _doc: &Doc, _p: P2) -> Option<u64> {
        None
    }
    fn enter_viewport(&mut self, _space: cc_cad::SpaceId, _id: u64) {}
    fn leave_viewport(&mut self) {}
    /// `host.turnView`: turn the view so a UCS X axis at `ang` is level (UCS, PLAN) → false when the host has none.
    fn has_turn_view(&self) -> bool {
        false
    }
    fn turn_view(&mut self, _ang: f64) {}
    /// `host.settings.lwt` with `host.setToggle` (LWDISPLAY): None when the host has no such setting.
    fn lwt(&self) -> Option<bool> {
        None
    }
    fn set_toggle(&mut self, _name: &str, _on: bool) {}
    /// `host.settings.polarInc` (POLARANG): None when the host has no settings.
    fn polar_inc(&self) -> Option<f64> {
        None
    }
    fn set_polar_inc(&mut self, _v: f64) {}
    /// `host.saveBlob`: hand a file to the person (COUNT's CSV) → false when the host cannot.
    fn can_save_blob(&self) -> bool {
        false
    }
    fn save_blob(&mut self, _name: &str, _data: &[u8], _mime: &str) {}
    /// `host.parameters()` (PARAMETERS): open the window's Parameters panel → false when it has none (the command line
    /// lists them).
    fn parameters(&mut self) -> bool {
        false
    }
    /// `host.plotPrefs()`: the Plot dialog's last ink ("mono" | "color") and pen (mm), which PUBLISH plots with; None
    /// when the host has no Plot dialog (black, 0.25 mm).
    fn plot_prefs(&self) -> Option<(String, f64)> {
        None
    }
    /// The pick tolerance in drawing units (`host.pickRadius`, BVSHOW); None = 1.
    fn pick_radius(&self) -> Option<f64> {
        None
    }
    /// Can this host open files (the JS host has `pickDrawing` / `pickPictures`)?
    fn can_pick_files(&self) -> bool {
        false
    }
    /// `host.pickDrawing()` (XATTACH, XREF Reload): a DWG / DXF the person picks, read as doc.load's input. The window
    /// answers [`Pick::Later`] and hands the file over through [`Engine::deliver_drawing`].
    fn pick_drawing(&mut self) -> Pick<Option<PickedDrawing>> {
        Pick::Unsupported
    }
    /// `host.pickPictures({ multiple })` (IMAGEATTACH, IMAGE): PNG / JPEG files (name and bytes); later through
    /// [`Engine::deliver_files`].
    fn pick_pictures(&mut self, _multiple: bool) -> Pick<Vec<PickedFile>> {
        Pick::Unsupported
    }
    /// `host.chooseBlock(fileName, names)` (INSERT, File): the window's "Insert from <file>" dialog → [`WHOLE`], a
    /// block name or None (cancelled); later through [`Engine::deliver_choice`].
    fn choose_block(&mut self, _file_name: &str, _names: &[String]) -> Pick<Option<String>> {
        Pick::Unsupported
    }
    /// `host.openLibrary()` (LIBRARY): open the library panel → false when the host has none.
    fn open_library(&mut self) -> bool {
        false
    }
    /// `host.library()` (INSERT, Library): the panel picks an item (and its slug); later through
    /// [`Engine::deliver_library_item`].
    fn library(&mut self) -> Pick<Option<(serde_json::Value, Option<String>)>> {
        Pick::Unsupported
    }
    /// `host.choose({ title, text, buttons: [[key, label], …] })`: a question in a dialog → the key pressed.
    fn choose(&mut self, _title: &str, _text: &str, _buttons: &[(String, String)]) -> Pick<Option<String>> {
        Pick::Unsupported
    }
    /// `host.open3d()` (commands3d.js `need3d`): switch to the 3D tab (the engine makes and shows the workspace,
    /// [`crate::ws3d`]) → false when this host has no 3D tab.
    fn open3d(&mut self) -> bool {
        false
    }
    /// `host.cursor = p`: the 3D tab moves the cursor the commands see (a point on the ground under the pointer).
    fn set_cursor(&mut self, _p: P2) {}
    /// `host.appload` (APPLOAD): open the Load applications dialog → false when the host has none (the command line
    /// lists the loaded applications).
    fn appload(&mut self) -> bool {
        false
    }
    /// `host.aliasEdit` (ALIASEDIT): open the alias editor → false when the host has none.
    fn alias_edit(&mut self) -> bool {
        false
    }
    /// `host.aliasText()` (REINIT): the person's aliases file, None when the host keeps none.
    fn alias_text(&self) -> Option<String> {
        None
    }
    /// `host.chooseBlocks({ title, names })` (DELBLOCKS Name): the names picked (None: cancelled); a dialog that answers
    /// later delivers them through [`Engine::deliver_choice`], one per line.
    fn choose_blocks(&mut self, _title: &str, _names: &[String]) -> Pick<Option<Vec<String>>> {
        Pick::Unsupported
    }
    /// `host.viewTwist()`: the view's turn (a routine's VIEWTWIST and (trans … 2)).
    fn view_twist(&self) -> Option<f64> {
        None
    }
    /// `host.pickTolerance()` in drawing units (a routine's ssget at a point, osnap).
    fn pick_tolerance(&self) -> Option<f64> {
        None
    }
    /// The view's size in pixels (SCREENSIZE).
    fn screen_px(&self) -> Option<(f64, f64)> {
        None
    }
    /// Does the window poll a routine that pauses ([`Engine::pump`] every frame)? Then a long routine lets Esc through.
    fn pumps(&self) -> bool {
        false
    }
    /// Can the window pick a text file for a routine's getfiled?
    fn has_getfiled(&self) -> bool {
        false
    }
    /// getfiled: a text file the person picks (its name and bytes); later through [`Engine::deliver_files`].
    fn pick_text_file(&mut self, _title: &str, _accept: &str) -> Pick<Option<PickedFile>> {
        Pick::Unsupported
    }
    /// A routine's DCL dialog (dcl-ui.js): open it → false when the host has no dialogs.
    fn dialog_open(&mut self, _tree: &cc_lisp::host::Tile) -> bool {
        false
    }
    /// An operation on the open dialog → its answer (None: the host has no dialogs).
    fn dialog_op(&mut self, _op: cc_lisp::host::DlgOp) -> Option<Option<String>> {
        None
    }
    /// The open dialog's next event; a dialog that answers later delivers it through [`Engine::deliver_dialog_event`].
    fn dialog_run(&mut self) -> Pick<cc_lisp::host::DlgEvent> {
        Pick::Unsupported
    }
}

/// A window "Select objects" was given: a rectangle, or a lasso's outline (STRETCH stretches what they hold).
#[derive(Clone, Debug, PartialEq)]
pub enum SelWin {
    Rect(Bounds),
    Poly(Vec<P2>),
}
impl SelWin {
    /// Does it hold this point (a rectangle with 1e-9 to spare; a lasso by its polygon)?
    pub fn holds(&self, q: P2) -> bool {
        match self {
            SelWin::Rect(w) => q.x >= w.min_x - 1e-9 && q.x <= w.max_x + 1e-9 && q.y >= w.min_y - 1e-9 && q.y <= w.max_y + 1e-9,
            SelWin::Poly(p) => cc_cad_geom::pick::point_in_poly(q, p),
        }
    }
}

/// What `choose_block` answers for "the whole drawing" (cadcraft.js's `'\u0000whole'` option).
pub const WHOLE: &str = "\u{0}whole";

/// How a host answers a request for a file.
#[derive(Clone, Debug, PartialEq)]
pub enum Pick<T> {
    /// it cannot open files (the JS host has no such method)
    Unsupported,
    /// here it is (or nothing was picked)
    Now(T),
    /// a picker is open: the answer comes through the engine's `deliver_*`
    Later,
}

/// A drawing the host picked and read: `{ drawing, fileName }`. The read turns doc.load's input (readDxf /
/// importDwg) into typed objects ([`crate::cmd::xref::XSource::from_value`]), as the JS reader hands over objects.
#[derive(Clone, Debug, PartialEq)]
pub struct PickedDrawing {
    pub file_name: String,
    pub drawing: crate::cmd::xref::XSource,
}

impl PickedDrawing {
    /// A drawing as doc.load takes it, read into typed objects.
    pub fn from_value(file_name: &str, v: &serde_json::Value) -> Self {
        PickedDrawing { file_name: file_name.to_string(), drawing: crate::cmd::xref::XSource::from_value(v) }
    }
}

/// A file the host picked: `{ name, bytes }`.
#[derive(Clone, Debug, PartialEq)]
pub struct PickedFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// A set of ids that keeps insertion order (a JS `Set`). The order is the Vec (read it as `.0`); the hash index
/// keeps `has` / `add` / `delete` O(1) (ALL on a drawing of 20,000 objects was quadratic).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IdSet(pub Vec<u64>, std::collections::HashSet<u64>);
impl IdSet {
    /// The ids in this order (repeats dropped).
    pub fn from_vec(ids: Vec<u64>) -> IdSet {
        let mut s = IdSet::default();
        for id in ids {
            s.add(id);
        }
        s
    }
    pub fn has(&self, id: u64) -> bool {
        self.1.contains(&id)
    }
    /// → true when it was not there
    pub fn add(&mut self, id: u64) -> bool {
        if !self.1.insert(id) {
            return false;
        }
        self.0.push(id);
        true
    }
    /// → true when it was there
    pub fn delete(&mut self, id: u64) -> bool {
        if !self.1.remove(&id) {
            return false;
        }
        self.0.retain(|x| *x != id);
        true
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// What "Select objects" ended with: the objects, or one of the command's own options.
#[derive(Clone, Debug, PartialEq)]
pub enum Sel {
    Set(IdSet),
    Opt(String),
}

/// A deferred Tangent snap riding on a clicked point (DR.3): the circle and where it was picked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tan {
    pub c: Circ,
    pub near: P2,
}

/// An answer, as the JS commands receive it.
#[derive(Clone, Debug, PartialEq)]
pub enum Reply {
    /// Enter on an empty line (`null`)
    Null,
    Point(P2, Option<Tan>),
    Num(f64),
    /// a string answer, or a default given as text ('Through', 'I', 'N')
    Str(String),
    /// `{ option: key }`
    Opt(String),
    /// `{ option: '_typed', value }`: a command's own token (ZOOM 2x)
    Typed(f64),
    /// "Select objects" ended
    Set(IdSet),
    /// an object picked: `{ id, point, shift }`
    Pick {
        id: u64,
        at: P2,
        shift: bool,
    },
    /// an empty spot clicked where the prompt takes one (TRIM fence): `{ empty, shift }`
    Empty {
        at: P2,
        shift: bool,
    },
    /// a freehand path dragged (TRIM / EXTEND): `{ path, shift }`
    Path {
        pts: Vec<P2>,
        shift: bool,
    },
    /// the drawing a picker that answered later delivered (None = nothing picked)
    Drawing(Option<PickedDrawing>),
    /// the files a picker that answered later delivered
    Files(Vec<PickedFile>),
    /// a dialog's answer (`host.chooseBlock` / `host.choose`): None = cancelled
    Choice(Option<String>),
    /// the library panel's pick (`host.library()`): the item and its slug, None = closed
    Item(Option<(serde_json::Value, Option<String>)>),
    /// `{ done3d: true }`: the 3D tab's tool made its edit (ws3d-core.js `check`)
    Done3d,
    /// text typed where any text is taken (`{ option: '_typed', value }` with a routine's initget 128)
    Arbitrary(String),
    /// a routine's dialog: what happened in it
    Dialog(cc_lisp::host::DlgEvent),
}
impl Reply {
    pub fn point(&self) -> Option<P2> {
        if let Reply::Point(p, _) = self { Some(*p) } else { None }
    }
    pub fn option(&self) -> Option<&str> {
        if let Reply::Opt(k) = self { Some(k.as_str()) } else { None }
    }
}

/// A question's options (commands.js `opts`).
#[derive(Clone, Default, Debug)]
pub struct Opts {
    pub base: Option<P2>,
    pub options: Vec<Opt>,
    /// Enter answers Null instead of ending the command
    pub none: bool,
    pub default: Option<Def>,
    /// a command's own token, tried after the options (ZOOM's factor)
    pub accept: Option<fn(&str) -> Option<f64>>,
    /// an absolute angle is from the UCS X axis
    pub ucs: bool,
    /// a signed length (number questions)
    pub length: bool,
    pub remove: bool,
    pub single: bool,
    /// a click on nothing answers Empty (TRIM)
    pub empty: bool,
    /// a dragged path answers Path (TRIM / EXTEND)
    pub path: bool,
    /// `ws: 'tool'`: clicks on the 3D tab go to its ModelCraft tool (ws3d-core.js inputMode)
    pub ws_tool: bool,
    /// `height: {rings, z0}`: on the 3D tab a click gives the height along Z under the cursor
    pub height: Option<crate::ws3d::HeightSpec>,
    /// any text answers the question as it is (a routine's initget 128)
    pub any_text: bool,
    /// the base is a copy of a point the drawing keeps (no z, though it was picked on the 3D tab): `P(d.p2)`
    pub flat_base: bool,
}

/// Rubber-band entities for a cursor position.
pub type PreviewFn = Rc<dyn Fn(&Shared, P2) -> Vec<Entity>>;

pub(crate) struct Waiting {
    pub kind: Kind,
    pub prompt: String,
    pub opts: Opts,
    pub set: IdSet,
    /// which question this is (a routine's (command …) tells a new prompt from the same one by it)
    pub seq: u64,
}

/// The value OFFSET remembers: a distance or Through.
#[derive(Clone, Debug, PartialEq)]
pub enum OffsetMemo {
    Dist(f64),
    Through,
}

/// The engine's state between inputs, and what commands remember for the session.
#[derive(Default)]
pub struct State {
    pub(crate) waiting: Option<Waiting>,
    reply: Option<(Reply, IdSet)>,
    pub running: Option<&'static str>,
    keep_selection: bool,
    /// the last command, for Enter-to-repeat
    pub last: Option<&'static str>,
    /// for @ input
    pub last_point: Option<P2>,
    /// the rubber-band base (ortho and direct distance use it)
    pub base: Option<P2>,
    pub(crate) preview_fn: Option<PreviewFn>,
    /// the undo depth when the running command started
    pub cmd_mark: usize,
    pub prev_selection: Option<IdSet>,
    pub last_radius: Option<f64>,
    pub last_sides: Option<f64>,
    pub last_poly_mode: Option<String>,
    pub last_rotation: Option<f64>,
    pub last_offset: Option<OffsetMemo>,
    /// TRIMEXTENDMODE: false = quick (default), true = standard
    pub trim_standard: bool,
    /// EDGEMODE: 1 = cutting edges extended
    pub edge_mode: i32,
    /// the freehand path being dragged (the host sets it; TRIM / EXTEND preview what it crosses)
    pub drag_path: Option<Vec<P2>>,
    /// the windows the last "Select objects" was given (`set.windows`: STRETCH stretches what they hold): rectangles,
    /// and lassos as their outlines
    pub sel_windows: Vec<SelWin>,
    /// what commands remember between runs (`E.hatch`, `E.filletR`, …)
    pub memo: crate::cmd::Memo,
    /// the engine's clipboard (`E.clipboard`; the host's shared store sits behind it)
    pub clipboard: crate::clip::Clipboard,
    /// the z of the last point answered on the 3D tab (`p.z`; None: a 2D point)
    pub z3: Option<f64>,
    /// the points the running command was given on the 3D tab, with their z (a base point's z)
    pub z3_pts: Vec<(P2, f64)>,
    /// the serial number of the last question asked
    pub wait_seq: u64,
    /// CadCraft's LISP in this window (lisp/session.js: made on first use)
    pub lisp: Option<Rc<crate::lisp::LispSession>>,
    /// the commands loaded routines made ((defun c:NAME …)), and the person's aliases (ALIASEDIT / REINIT)
    pub lisp_cmds: Vec<cmd::CmdDef>,
    pub user_aliases: Vec<(String, String)>,
    /// what _LISPLOAD / _LISPEVAL run (`engine.lispPending`)
    pub lisp_pending: Option<crate::lisp::LispJob>,
    /// a routine paused for the window (every few thousand steps): poll it again
    pub wants_poll: bool,
    /// the time and zone DATE / CDATE read (the host's clock; None = the system's)
    pub clock: Option<(f64, cc_clock::Zone)>,
    /// the interpreter's caps (None: cc-lisp's defaults)
    pub lisp_limits: Option<cc_lisp::Limits>,
}

/// What the engine and its commands share.
pub struct Shared {
    pub doc: RefCell<Doc>,
    pub host: RefCell<Box<dyn Host>>,
    pub st: RefCell<State>,
    pub lang: Lang,
    /// the 3D tab's workspace (ws3d-core.js), made the first time the tab opens
    pub ws3d: RefCell<Option<Box<crate::ws3d::Ws3d>>>,
}

impl Shared {
    pub fn t(&self, key: &str) -> String {
        self.lang.s(key)
    }
    pub fn tv(&self, key: &str, vars: &[(&str, String)]) -> String {
        self.lang.t(key, vars)
    }
    pub fn print(&self, s: &str) {
        self.host.borrow_mut().print(s);
    }
    /// Print and clear the drawing's constraint notes (`doc.onNote` → print('  ' + m)).
    pub fn flush_notes(&self) {
        let notes = match self.doc.try_borrow_mut() {
            Ok(mut d) => std::mem::take(&mut d.notes),
            Err(_) => return,
        };
        for n in notes {
            self.print(&format!("  {n}"));
        }
    }
    fn prompt_text(&self) -> String {
        let st = self.st.borrow();
        let Some(w) = st.waiting.as_ref() else { return String::new() };
        let ucs_ang = if w.opts.ucs { ang_of(active_ucs(&self.doc.borrow())) } else { 0.0 };
        prompt_text(&w.prompt, w.kind == Kind::Angle, w.kind == Kind::Point, &w.opts.options, w.opts.default.as_ref(), ucs_ang)
    }
    /// The waiting question's kind, options and selection.
    pub(crate) fn waiting_info(&self) -> Option<(Kind, Opts, IdSet)> {
        let st = self.st.borrow();
        st.waiting.as_ref().map(|w| (w.kind.clone(), w.opts.clone(), w.set.clone()))
    }
    /// The waiting question's serial number.
    pub(crate) fn wait_seq(&self) -> Option<u64> {
        self.st.borrow().waiting.as_ref().map(|w| w.seq)
    }
    pub(crate) fn lunits(&self) -> f64 {
        self.doc.borrow().meta.get("lunits").map_or(2.0, js::to_number)
    }
    /// Change the selection being built by the waiting "Select objects" → a copy of it.
    pub(crate) fn with_set(&self, f: impl FnOnce(&mut IdSet)) -> IdSet {
        let mut st = self.st.borrow_mut();
        match st.waiting.as_mut() {
            Some(w) => {
                f(&mut w.set);
                w.set.clone()
            }
            None => IdSet::default(),
        }
    }
    /// resolve's state part: the waiting question is answered with `v` → false when nothing was waiting.
    pub(crate) fn take_reply(&self, v: Reply) -> bool {
        let mut st = self.st.borrow_mut();
        let Some(w) = st.waiting.take() else { return false };
        if let Reply::Point(p, _) = &v
            && w.kind == Kind::Point
        {
            st.last_point = Some(*p);
        }
        st.reply = Some((v, w.set));
        true
    }

    /// `engine.text(raw)` while a question waits: what it does to the question.
    pub(crate) fn text_act(&self, raw: &str) -> Act {
        let typed = js::trim(raw).to_string();
        let Some((kind, opts, set)) = self.waiting_info() else { return Act::Stay };
        if kind == Kind::Text {
            return Act::Resolve(Reply::Str(raw.to_string()));
        }
        if kind == Kind::Select && typed.is_empty() {
            return Act::Resolve(Reply::Set(set));
        }
        // Enter where a point or an object is required (and there is no default) ends the command
        if typed.is_empty() && matches!(kind, Kind::Point | Kind::Entity) && !opts.none && opts.default.is_none() {
            return Act::Escape;
        }
        if typed.is_empty() {
            return Act::Resolve(def_reply(&opts.default));
        }
        if let Some(o) = match_option(&typed, &opts.options) {
            return Act::Resolve(Reply::Opt(o.key.clone()));
        }
        if let Some(acc) = opts.accept
            && let Some(v) = acc(&typed)
        {
            return Act::Resolve(Reply::Typed(v));
        }
        // initget 128: any text answers the question as it is
        if opts.any_text {
            return Act::Resolve(Reply::Arbitrary(typed));
        }
        let lunits = self.lunits();
        match kind {
            Kind::Point => {
                let pc = {
                    let st = self.st.borrow();
                    PointCtx {
                        lunits,
                        ucs: active_ucs(&self.doc.borrow()),
                        last_point: st.last_point,
                        base: opts.base.or(st.base),
                        cursor: self.host.borrow().cursor(),
                    }
                };
                if let Some(p) = parse_point(&typed, &pc) {
                    return Act::Resolve(Reply::Point(p, None));
                }
            }
            Kind::Distance => {
                if let Some(n) = parse_length(&typed, lunits)
                    && n >= 0.0
                {
                    return Act::Resolve(Reply::Num(n));
                }
            }
            Kind::Number => {
                if opts.length {
                    if let Some(n) = parse_length(&typed, lunits) {
                        return Act::Resolve(Reply::Num(n));
                    }
                } else if is_plain_number(&typed, true) {
                    return Act::Resolve(Reply::Num(plain_number(&typed)));
                }
            }
            Kind::Angle => {
                if is_plain_number(&typed, false) {
                    let u = if opts.ucs { ang_of(active_ucs(&self.doc.borrow())) } else { 0.0 };
                    return Act::Resolve(Reply::Num(rad(plain_number(&typed)) + u));
                }
            }
            Kind::Select => {
                let u = typed.to_uppercase();
                if u == "ALL" {
                    let ids: Vec<u64> = {
                        let d = self.doc.borrow();
                        d.entities().filter(|e| d.editable(e)).filter_map(|e| e.id).collect()
                    };
                    let set = self.with_set(|s| {
                        for id in ids {
                            s.add(id);
                        }
                    });
                    let mut h = self.host.borrow_mut();
                    h.set_selection(&set.0);
                    h.print(&format!("{} found", set.len()));
                    return Act::Stay;
                }
                if u == "L" || u == "LAST" {
                    let last = self.doc.borrow().entities().last().and_then(|e| e.id);
                    if let Some(id) = last.filter(|i| *i != 0) {
                        let set = self.with_set(|s| {
                            s.add(id);
                        });
                        let mut h = self.host.borrow_mut();
                        h.set_selection(&set.0);
                        h.print("1 found");
                    }
                    return Act::Stay;
                }
            }
            _ => {}
        }
        let key = match kind {
            Kind::Point => "cmd.invalid.point",
            Kind::Distance => "cmd.invalid.distance",
            Kind::Number => "cmd.invalid.number",
            Kind::Angle => "cmd.invalid.angle",
            Kind::Keyword => "cmd.invalid.keyword",
            Kind::Select => "cmd.invalid.select",
            Kind::Entity => "cmd.invalid.entity",
            Kind::Text | Kind::File => "cmd.invalid_input",
        };
        let msg = self.t(key);
        self.print(&msg);
        let p = self.prompt_text();
        self.host.borrow_mut().prompt(&p);
        Act::Stay
    }

    /// `engine.click(point, raw)` while a question waits → what it does, and whether the click was taken (false: the
    /// host starts a selection window).
    pub(crate) fn click_act(&self, point: P2, tan: Option<Tan>, raw: P2) -> (Act, bool) {
        let Some((kind, opts, set)) = self.waiting_info() else { return (Act::Stay, false) };
        match kind {
            Kind::Point => (Act::Resolve(Reply::Point(point, tan)), true),
            Kind::Distance | Kind::Angle => match opts.base {
                Some(b) => (Act::Resolve(Reply::Num(if kind == Kind::Distance { dist(b, point) } else { angle_of(b, point) })), true),
                None => {
                    {
                        let mut st = self.st.borrow_mut();
                        if let Some(w) = st.waiting.as_mut() {
                            w.opts.base = Some(point);
                        }
                        st.base = Some(point);
                    }
                    let msg = self.t("cmd.specify_second_point");
                    self.host.borrow_mut().prompt(&msg);
                    (Act::Stay, true)
                }
            },
            Kind::Select => {
                let id = self.host.borrow().pick_at(&self.doc.borrow(), raw);
                let Some(id) = id else { return (Act::Stay, false) };
                let shift = self.host.borrow().shift();
                let set = self.with_set(|s| {
                    if opts.remove || (s.has(id) && shift) {
                        s.delete(id);
                    } else {
                        s.add(id);
                    }
                });
                self.host.borrow_mut().set_selection(&set.0);
                if opts.single {
                    return (Act::Resolve(Reply::Set(set)), true);
                }
                (Act::Stay, true)
            }
            Kind::Entity => {
                let id = self.host.borrow().pick_at(&self.doc.borrow(), raw);
                let shift = self.host.borrow().shift();
                let _ = set;
                if let Some(id) = id {
                    return (Act::Resolve(Reply::Pick { id, at: raw, shift }), true);
                }
                if opts.empty {
                    return (Act::Resolve(Reply::Empty { at: raw, shift }), true);
                }
                let msg = self.t("cmd.nothing_found_there");
                self.print(&msg);
                (Act::Stay, true)
            }
            _ => (Act::Stay, false),
        }
    }

    /// A command by name or alias: the built-in ones, the routines' and the person's aliases (commands.js `lookup`
    /// with lisp/install.js's additions).
    pub fn lookup(&self, name: &str) -> Option<cmd::CmdDef> {
        let st = self.st.borrow();
        cmd::lookup_with(name, &st.lisp_cmds, &st.user_aliases)
    }

    /// `engine.preview(cursor)`: the rubber band for a cursor position.
    pub fn preview_at(&self, cursor: P2) -> Vec<Entity> {
        let f = self.st.borrow().preview_fn.clone();
        f.map_or_else(Vec::new, |f| f(self, cursor))
    }
}

/// What an input does to the waiting question.
pub(crate) enum Act {
    Resolve(Reply),
    Escape,
    Stay,
}

fn def_reply(d: &Option<Def>) -> Reply {
    match d {
        None => Reply::Null,
        Some(Def::Num(n)) => Reply::Num(*n),
        Some(Def::Str(s)) => Reply::Str(s.clone()),
        Some(Def::Point(p)) => Reply::Point(*p, None),
    }
}

/// The command line.
pub struct Engine {
    pub(crate) sh: Rc<Shared>,
    pub(crate) runner: Runner,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine").field("running", &self.running()).finish()
    }
}

impl Engine {
    pub fn new(mut doc: Doc, host: Box<dyn Host>, lang: Lang) -> Self {
        doc.unnamed_view = lang.s("field.vpview.none");
        // constraints: the drawing's edit hook, the catalog its notes come from, a file's constraints resolved
        cc_gcons::install();
        cc_cad::i18n::set_catalog(lang.cat());
        cc_gcons::finish_load(&mut doc);
        let sh = Rc::new(Shared {
            doc: RefCell::new(doc),
            host: RefCell::new(host),
            st: RefCell::new(State::default()),
            lang,
            ws3d: RefCell::new(None),
        });
        // model3d.js fromFile: the file's 3D model is read with the drawing
        crate::cmd::solid3d::preload(&sh);
        Engine { sh, runner: Runner::default() }
    }
    pub fn doc(&self) -> Ref<'_, Doc> {
        self.sh.doc.borrow()
    }
    pub fn doc_mut(&self) -> RefMut<'_, Doc> {
        self.sh.doc.borrow_mut()
    }
    pub fn host(&self) -> Ref<'_, Box<dyn Host>> {
        self.sh.host.borrow()
    }
    pub fn host_mut(&self) -> RefMut<'_, Box<dyn Host>> {
        self.sh.host.borrow_mut()
    }
    pub fn state(&self) -> Ref<'_, State> {
        self.sh.st.borrow()
    }
    /// The language the command line speaks.
    pub fn lang(&self) -> &Lang {
        &self.sh.lang
    }
    /// Print the drawing's constraint notes (an edit the window made itself: grips, the Properties panel, the Parameters
    /// panel).
    pub fn flush_notes(&self) {
        self.sh.flush_notes();
    }
    /// The running command's name.
    pub fn running(&self) -> Option<&'static str> {
        self.sh.st.borrow().running
    }
    pub fn busy(&self) -> bool {
        self.running().is_some()
    }
    /// What the waiting question wants.
    pub fn expects(&self) -> Option<Kind> {
        self.sh.st.borrow().waiting.as_ref().map(|w| w.kind.clone())
    }
    /// The prompt line of the waiting question ('' when none).
    pub fn prompt_line(&self) -> String {
        self.sh.prompt_text()
    }
    /// Rubber-band entities for the cursor (the host calls it on every move).
    pub fn preview(&self, cursor: P2) -> Vec<Entity> {
        self.sh.preview_at(cursor)
    }
    /// The freehand path being dragged (`engine.dragPath`), or None when the drag ends.
    pub fn set_drag_path(&self, pts: Option<Vec<P2>>) {
        self.sh.st.borrow_mut().drag_path = pts;
    }

    pub(crate) fn ctx(&self) -> Ctx {
        Ctx { sh: Rc::clone(&self.sh), pr: self.runner.prompter.clone() }
    }

    /// Start a command by name or alias.
    pub fn run(&mut self, name: &str) {
        let Some(c) = self.sh.lookup(name) else {
            let msg = self.sh.tv("cmd.unknown", &[("name", name.to_uppercase())]);
            self.sh.print(&msg);
            return;
        };
        self.run_def(c);
    }

    /// Run a hidden command under a label (lisp/install.js `runHidden`): the command line echoes the label
    /// ("Command: (+ 1 2)", "Command: APPLOAD") and Undo names it, as for a typed command.
    pub fn run_hidden(&mut self, name: &str, label: &str, job: Option<crate::lisp::LispJob>) {
        let Some(mut c) = self.sh.lookup(name) else { return };
        if self.busy() {
            self.escape();
        }
        self.sh.st.borrow_mut().lisp_pending = job;
        c.name = crate::lisp::static_name(&cc_cad::js::slice16(label, 200));
        self.run_def(c);
    }

    /// Caps for the window's LISP (steps, frames, strings, arrays): set before the first routine runs.
    pub fn set_lisp_limits(&mut self, limits: cc_lisp::Limits) {
        self.sh.st.borrow_mut().lisp_limits = Some(limits);
    }

    /// The time and zone DATE / CDATE read (None: the system's clock and zone).
    pub fn set_clock(&mut self, clock: Option<(f64, cc_clock::Zone)>) {
        self.sh.st.borrow_mut().clock = clock;
    }

    /// A routine paused for the window (every few thousand steps): let it run on. → true when it is still busy.
    pub fn pump(&mut self) -> bool {
        let due = std::mem::take(&mut self.sh.st.borrow_mut().wants_poll);
        if due {
            self.runner.poll_now();
            self.after_poll();
        }
        self.sh.st.borrow().wants_poll
    }

    /// Stop a routine that is busy (not waiting for an answer): Esc while it runs (appload.js `cancelEdit`).
    pub fn stop_lisp(&mut self) -> bool {
        let sess = self.sh.st.borrow().lisp.clone();
        match sess {
            Some(s) if s.busy() && self.expects().is_none() => {
                s.stop(&self.sh);
                self.pump();
                true
            }
            _ => false,
        }
    }

    fn run_def(&mut self, c: cmd::CmdDef) {
        if self.busy() {
            self.escape();
        }
        let mark = self.sh.doc.borrow().mark();
        {
            let mut st = self.sh.st.borrow_mut();
            if !c.no_repeat {
                st.last = Some(c.name);
            }
            st.running = Some(c.name);
            st.keep_selection = c.keep_selection;
            st.cmd_mark = mark;
        }
        self.sh.host.borrow_mut().on_command(Some(c.name));
        let echo = self.sh.tv("cmd.echo", &[("name", c.name.to_string())]);
        self.sh.print(&echo);
        // commands3d.js `wrap`: on the 3D tab MOVE, COPY, ROTATE, ERASE, ZOOM, SELECTALL, DESELECT and the clipboard
        // commands work on the 3D selection
        let fut = match crate::cmd::solid3d::wrap3d(&self.sh, c.name) {
            Some(f) => f(self.ctx()),
            None => (c.run)(self.ctx()),
        };
        self.runner.start(fut);
        self.after_poll();
    }

    /// The command ended (done, Esc or an error): print, fold its edits into one undo step, reset.
    fn after_poll(&mut self) {
        // notes from changes made outside Ctx::change (the parametric edits change the drawing themselves)
        self.sh.flush_notes();
        if self.runner.running() {
            return;
        }
        let Some(name) = self.sh.st.borrow().running else { return };
        match self.runner.last_result.take() {
            Some(Err(CmdError::Cancelled)) => self.sh.print("*Cancel*"),
            Some(Err(CmdError::Msg(m))) => self.sh.print(&format!("Error: {m}")),
            _ => {}
        }
        let (mark, keep) = {
            let st = self.sh.st.borrow();
            (st.cmd_mark, st.keep_selection)
        };
        self.sh.doc.borrow_mut().squash(mark, name);
        {
            let mut st = self.sh.st.borrow_mut();
            st.running = None;
            st.waiting = None;
            st.reply = None;
            st.base = None;
            st.preview_fn = None;
            st.z3_pts.clear();
        }
        let mut h = self.sh.host.borrow_mut();
        if !keep {
            h.set_selection(&[]);
        }
        h.preview(Vec::new());
        h.prompt("");
        h.on_command(None);
    }

    pub(crate) fn resolve(&mut self, v: Reply) {
        self.resolve_z(v, None);
    }
    /// Answer the waiting question; `z` = the z of a point picked on the 3D tab.
    pub(crate) fn resolve_z(&mut self, v: Reply, z: Option<f64>) {
        if !self.sh.take_reply(v) {
            return;
        }
        self.sh.st.borrow_mut().z3 = z;
        self.runner.answer(Answer::None);
        self.after_poll();
    }

    pub(crate) fn waiting_info(&self) -> Option<(Kind, Opts, IdSet)> {
        self.sh.waiting_info()
    }

    /// A line typed at the command line.
    pub fn text(&mut self, raw: &str) {
        if self.waiting_info().is_none() {
            let typed = js::trim(raw).to_string();
            if typed.is_empty() {
                let last = self.sh.st.borrow().last;
                if let Some(l) = last {
                    self.run(l);
                }
                return;
            }
            self.run(&typed);
            return;
        }
        match self.sh.text_act(raw) {
            Act::Resolve(v) => self.resolve(v),
            Act::Escape => {
                self.escape();
            }
            Act::Stay => {}
        }
    }

    /// Change the selection being built by the waiting "Select objects" → a copy of it.
    pub(crate) fn with_set(&self, f: impl FnOnce(&mut IdSet)) -> IdSet {
        self.sh.with_set(f)
    }

    /// The drawing a picker that answered [`Pick::Later`] has read (None = cancelled).
    pub fn deliver_drawing(&mut self, d: Option<PickedDrawing>) {
        if matches!(self.waiting_info(), Some((Kind::File, _, _))) {
            self.resolve(Reply::Drawing(d));
        }
    }
    /// The answer of a dialog that answered [`Pick::Later`] (`choose_block`, `choose`; None = cancelled).
    pub fn deliver_choice(&mut self, k: Option<String>) {
        if matches!(self.waiting_info(), Some((Kind::File, _, _))) {
            self.resolve(Reply::Choice(k));
        }
    }
    /// The library item the panel handed over (None: closed without placing).
    pub fn deliver_library_item(&mut self, it: Option<(serde_json::Value, Option<String>)>) {
        if matches!(self.waiting_info(), Some((Kind::File, _, _))) {
            self.resolve(Reply::Item(it));
        }
    }
    /// What happened in a routine's dialog (a host whose `dialog_run` answered [`Pick::Later`]).
    pub fn deliver_dialog_event(&mut self, ev: cc_lisp::host::DlgEvent) {
        if matches!(self.waiting_info(), Some((Kind::File, _, _))) {
            self.resolve(Reply::Dialog(ev));
        }
    }
    /// The files a picker that answered [`Pick::Later`] has read (empty = cancelled).
    pub fn deliver_files(&mut self, f: Vec<PickedFile>) {
        if matches!(self.waiting_info(), Some((Kind::File, _, _))) {
            self.resolve(Reply::Files(f));
        }
    }

    /// A click: `point` after snaps and ortho (with a deferred tangent when the snap gave one), `raw` the cursor.
    /// → false when the host should start a selection window instead.
    pub fn click(&mut self, point: P2, tan: Option<Tan>, raw: P2) -> bool {
        let (act, taken) = self.sh.click_act(point, tan, raw);
        match act {
            Act::Resolve(v) => self.resolve(v),
            Act::Escape => {
                self.escape();
            }
            Act::Stay => {}
        }
        taken
    }

    /// A selection window while selecting.
    pub fn window(&mut self, rect: Bounds, crossing: bool) {
        let Some((Kind::Select, opts, _)) = self.waiting_info() else { return };
        let ids = self.sh.host.borrow().pick_window(&self.sh.doc.borrow(), &rect, crossing);
        let shift = self.sh.host.borrow().shift();
        let set = self.with_set(|s| {
            for id in &ids {
                if shift || opts.remove {
                    s.delete(*id);
                } else {
                    s.add(*id);
                }
            }
        });
        if !shift && !opts.remove {
            self.sh.st.borrow_mut().sel_windows.push(SelWin::Rect(rect));
        }
        let msg = if opts.remove { self.sh.tv("sel.removed", &[("n", ids.len().to_string())]) } else { format!("{} found", ids.len()) };
        let mut h = self.sh.host.borrow_mut();
        h.set_selection(&set.0);
        h.print(&msg);
    }

    /// commands.js `lasso(pts, mode)`: a lasso dragged while "Select objects" waits ('window' | 'crossing' | 'fence').
    pub fn lasso(&mut self, pts: &[P2], mode: &str) {
        let Some((Kind::Select, opts, _)) = self.waiting_info() else { return };
        let ids = {
            let d = self.sh.doc.borrow();
            let ents: Vec<Entity> = d.entities().filter(|e| d.editable(e)).cloned().collect();
            crate::lasso::lasso_hits(&ents, pts, mode, Some(&d.blocks))
        };
        let shift = self.sh.host.borrow().shift();
        let set = self.with_set(|s| {
            for id in &ids {
                if shift || opts.remove {
                    s.delete(*id);
                } else {
                    s.add(*id);
                }
            }
        });
        if mode != "fence" && !shift && !opts.remove {
            self.sh.st.borrow_mut().sel_windows.push(SelWin::Poly(pts.to_vec()));
        }
        let msg = if opts.remove { self.sh.tv("sel.removed", &[("n", ids.len().to_string())]) } else { format!("{} found", ids.len()) };
        let mut h = self.sh.host.borrow_mut();
        h.set_selection(&set.0);
        h.print(&msg);
    }

    /// A freehand path dragged where the prompt takes one (TRIM / EXTEND).
    pub fn path(&mut self, pts: &[P2]) -> bool {
        let Some((Kind::Entity, opts, _)) = self.waiting_info() else { return false };
        if !opts.path || pts.len() < 2 {
            return false;
        }
        let shift = self.sh.host.borrow().shift();
        self.resolve(Reply::Path { pts: pts.to_vec(), shift });
        true
    }

    /// Esc: cancels the waiting question (and with it the command). → false when nothing was waiting.
    pub fn escape(&mut self) -> bool {
        if self.sh.st.borrow_mut().waiting.take().is_none() {
            return false;
        }
        self.runner.cancel();
        self.after_poll();
        true
    }
}

/// The prompt API a command is written in (commands.js `c`).
#[derive(Clone)]
pub struct Ctx {
    pub(crate) sh: Rc<Shared>,
    pub(crate) pr: Prompter,
}

pub type R<T = ()> = Result<T, CmdError>;

pub(crate) fn doc_err(e: cc_cad::DocError) -> CmdError {
    CmdError::Msg(e.to_string())
}

impl Ctx {
    pub fn sh(&self) -> &Shared {
        &self.sh
    }
    pub fn doc(&self) -> Ref<'_, Doc> {
        self.sh.doc.borrow()
    }
    pub fn st(&self) -> RefMut<'_, State> {
        self.sh.st.borrow_mut()
    }
    pub fn print(&self, s: &str) {
        self.sh.print(s);
    }
    pub fn t(&self, key: &str) -> String {
        self.sh.t(key)
    }
    pub fn tv(&self, key: &str, vars: &[(&str, String)]) -> String {
        self.sh.tv(key, vars)
    }
    pub fn opt(&self, key: &str, label: &str) -> Opt {
        crate::input::opt(&self.sh.lang, key, label)
    }
    pub fn set_selection(&self, ids: &[u64]) {
        self.sh.host.borrow_mut().set_selection(ids);
    }

    /// Ask a question → the answer and (for "Select objects") the selection it built.
    async fn ask_set(&self, kind: Kind, prompt: String, opts: Opts, set: IdSet) -> R<(Reply, IdSet)> {
        let req = Request::new(kind.clone(), prompt.clone());
        {
            let mut st = self.st();
            st.base = opts.base;
            st.wait_seq = st.wait_seq.wrapping_add(1);
            let seq = st.wait_seq;
            st.waiting = Some(Waiting { kind, prompt, opts, set, seq });
        }
        let p = self.sh.prompt_text();
        self.sh.host.borrow_mut().prompt(&p);
        self.pr.ask(req).await?;
        Ok(self.st().reply.take().unwrap_or((Reply::Null, IdSet::default())))
    }
    pub async fn ask(&self, kind: Kind, prompt: String, opts: Opts) -> R<Reply> {
        Ok(self.ask_set(kind, prompt, opts, IdSet::default()).await?.0)
    }
    pub async fn point(&self, prompt: String, opts: Opts) -> R<Reply> {
        self.ask(Kind::Point, prompt, opts).await
    }
    pub async fn distance(&self, prompt: String, opts: Opts) -> R<Reply> {
        self.ask(Kind::Distance, prompt, opts).await
    }
    pub async fn angle(&self, prompt: String, opts: Opts) -> R<Reply> {
        self.ask(Kind::Angle, prompt, opts).await
    }
    pub async fn number(&self, prompt: String, opts: Opts) -> R<Reply> {
        self.ask(Kind::Number, prompt, opts).await
    }
    pub async fn string(&self, prompt: String, opts: Opts) -> R<Reply> {
        self.ask(Kind::Text, prompt, opts).await
    }
    pub async fn keyword(&self, prompt: String, opts: Opts) -> R<Reply> {
        self.ask(Kind::Keyword, prompt, opts).await
    }
    pub async fn entity(&self, prompt: String, opts: Opts) -> R<Reply> {
        self.ask(Kind::Entity, prompt, opts).await
    }

    /// A point answer (questions with no options and no Enter only ever give one).
    pub async fn must_point(&self, prompt: String, opts: Opts) -> R<P2> {
        let r = self.point(prompt, opts).await?;
        r.point().ok_or_else(|| CmdError::Msg("a point was expected".into()))
    }

    /// `await c.host.pickDrawing()`: None when the host cannot open files, Some(None) when nothing was picked.
    pub async fn pick_drawing(&self, prompt: String) -> R<Option<Option<PickedDrawing>>> {
        if !self.sh.host.borrow().can_pick_files() {
            return Ok(None);
        }
        let p = self.sh.host.borrow_mut().pick_drawing();
        match p {
            Pick::Unsupported => Ok(None),
            Pick::Now(d) => Ok(Some(d)),
            Pick::Later => match self.ask(Kind::File, prompt, Opts::default()).await? {
                Reply::Drawing(d) => Ok(Some(d)),
                _ => Ok(Some(None)),
            },
        }
    }
    /// `await c.host.pickPictures({ multiple })`: None when the host cannot open files.
    pub async fn pick_pictures(&self, prompt: String, multiple: bool) -> R<Option<Vec<PickedFile>>> {
        if !self.sh.host.borrow().can_pick_files() {
            return Ok(None);
        }
        let p = self.sh.host.borrow_mut().pick_pictures(multiple);
        match p {
            Pick::Unsupported => Ok(None),
            Pick::Now(f) => Ok(Some(f)),
            Pick::Later => match self.ask(Kind::File, prompt, Opts::default()).await? {
                Reply::Files(f) => Ok(Some(f)),
                _ => Ok(Some(Vec::new())),
            },
        }
    }

    /// `await c.host.chooseBlock(fileName, names)` (INSERT, File): Some(answer) when the host has the dialog, None when
    /// not (the command line asks); the answer is [`WHOLE`] for the whole drawing, a block name, or None (cancelled).
    pub async fn choose_block(&self, file_name: &str, names: &[String]) -> R<Option<Option<String>>> {
        let p = self.sh.host.borrow_mut().choose_block(file_name, names);
        self.dialog_answer(p, self.sh.lang.t("app.insertf.title", &[("file", file_name.to_string())])).await
    }
    /// `await c.host.choose({ title, text, buttons })`: the key of the button pressed (None: closed), or None when the
    /// host has no such dialog.
    pub async fn choose(&self, title: &str, text: &str, buttons: &[(String, String)]) -> R<Option<Option<String>>> {
        let p = self.sh.host.borrow_mut().choose(title, text, buttons);
        self.dialog_answer(p, title.to_string()).await
    }
    /// `await c.host.library()` (INSERT, Library): Some(pick) when the host has a library panel, None when not.
    pub async fn library(&self) -> R<Option<Option<(serde_json::Value, Option<String>)>>> {
        let p = self.sh.host.borrow_mut().library();
        match p {
            Pick::Unsupported => Ok(None),
            Pick::Now(k) => Ok(Some(k)),
            Pick::Later => match self.ask(Kind::File, self.sh.lang.t("lib.panel.library", &[]), Opts::default()).await? {
                Reply::Item(k) => Ok(Some(k)),
                _ => Ok(Some(None)),
            },
        }
    }
    /// The dialog's answer; `prompt` shows on the command line while it is open.
    async fn dialog_answer(&self, p: Pick<Option<String>>, prompt: String) -> R<Option<Option<String>>> {
        match p {
            Pick::Unsupported => Ok(None),
            Pick::Now(k) => Ok(Some(k)),
            Pick::Later => match self.ask(Kind::File, prompt, Opts::default()).await? {
                Reply::Choice(k) => Ok(Some(k)),
                _ => Ok(Some(None)),
            },
        }
    }

    /// Set the rubber band (None clears it); drawn at once for the cursor.
    pub fn preview(&self, f: Option<PreviewFn>) {
        let has = f.is_some();
        self.st().preview_fn = f;
        let cur = self.sh.host.borrow().cursor();
        let ents = match (has, cur) {
            (true, Some(c)) => self.sh.preview_at(c),
            _ => Vec::new(),
        };
        self.sh.host.borrow_mut().preview(ents);
    }

    /// One change of the document (an undo step, folded into the command's when it ends).
    pub fn change<T>(&self, label: &str, f: impl FnOnce(&mut Doc) -> Result<T, cc_cad::DocError>) -> R<T> {
        let r = self.sh.doc.borrow_mut().change(label, f).map_err(doc_err);
        self.flush_notes();
        r
    }
    /// What the drawing said about constraints during the last changes (`doc.onNote`: printed indented, as
    /// params-panel.js prints them).
    pub fn flush_notes(&self) {
        self.sh.flush_notes();
    }
    /// `c.add(e)`
    pub fn add(&self, e: Entity) -> R<Entity> {
        self.change("add", |d| d.add(e))
    }

    /// Objects to work on: the current selection (noun-verb), else ask until Enter. Besides clicks and windows the
    /// prompt takes ALL, L(ast), P(revious), F(ence), W(indow), C(rossing), R(emove) / A(dd) and ? (the list).
    pub async fn select(&self, prompt: Option<String>, fresh: bool) -> R<IdSet> {
        match self.select_with(prompt, fresh, Vec::new()).await? {
            Sel::Set(s) => Ok(s),
            Sel::Opt(_) => Ok(IdSet::default()),
        }
    }

    /// `c.select(prompt, { fresh, options })`: as [`Ctx::select`], and the command's own options answer `Sel::Opt`.
    pub async fn select_with(&self, prompt: Option<String>, fresh: bool, own: Vec<Opt>) -> R<Sel> {
        self.select_opts(prompt, fresh, own, false).await
    }

    /// `c.select(prompt, { fresh, options, single })`: `single` = one pick ends it (a routine's ssget ":S").
    pub async fn select_opts(&self, prompt: Option<String>, fresh: bool, own: Vec<Opt>, single: bool) -> R<Sel> {
        let pre: Vec<u64> = {
            let d = self.doc();
            self.sh.host.borrow().selection().into_iter().filter(|id| d.get(*id).is_some_and(|e| d.editable(e))).collect()
        };
        self.st().sel_windows.clear();
        if !pre.is_empty() && !fresh {
            self.print(&format!("{} found", pre.len()));
            self.st().prev_selection = Some(IdSet::from_vec(pre.clone()));
            return Ok(Sel::Set(IdSet::from_vec(pre)));
        }
        self.set_selection(&[]);
        let prompt = prompt.unwrap_or_else(|| self.t("cmd.sim.select"));
        let mut set = IdSet::default();
        let mut remove = false;
        loop {
            let mut options = own.clone();
            options.extend(select_options(&self.sh.lang));
            let opts = Opts { none: true, remove, options, single, ..Opts::default() };
            let p = if remove { self.t("sel.remove.prompt") } else { prompt.clone() };
            let (r, s) = self.ask_set(Kind::Select, p, opts, set).await?;
            set = s;
            if let Reply::Opt(k) = r {
                if k == "R" || k == "A" {
                    remove = k == "R";
                    continue;
                }
                if self.select_option(&k, &mut set, remove).await? {
                    continue;
                }
                return Ok(Sel::Opt(k));
            }
            break;
        }
        if !set.is_empty() {
            self.st().prev_selection = Some(set.clone());
        }
        Ok(Sel::Set(set))
    }

    fn take(&self, ids: &[u64], set: &mut IdSet, remove: bool) {
        let mut n = 0usize;
        for id in ids {
            if if remove { set.delete(*id) } else { set.add(*id) } {
                n = n.saturating_add(1);
            }
        }
        self.set_selection(&set.0);
        let msg = if remove { self.tv("sel.removed", &[("n", n.to_string())]) } else { format!("{n} found") };
        self.print(&msg);
    }

    /// A selection option typed at "Select objects" → true when handled here.
    async fn select_option(&self, key: &str, set: &mut IdSet, remove: bool) -> R<bool> {
        let keep = self.st().preview_fn.clone();
        let r = self.select_option_inner(key, set, remove).await;
        self.st().preview_fn = keep;
        self.sh.host.borrow_mut().preview(Vec::new());
        r
    }
    async fn select_option_inner(&self, key: &str, set: &mut IdSet, remove: bool) -> R<bool> {
        match key {
            "P" => {
                let ids: Vec<u64> = {
                    let d = self.doc();
                    let prev = self.st().prev_selection.clone().unwrap_or_default();
                    prev.0.into_iter().filter(|id| d.get(*id).is_some_and(|e| d.editable(e))).collect()
                };
                if ids.is_empty() {
                    self.print(&self.t("sel.previous.none"));
                    return Ok(true);
                }
                self.take(&ids, set, remove);
                Ok(true)
            }
            "W" | "C" => {
                let a = self.must_point(self.t("sel.window.first"), Opts::default()).await?;
                self.preview(Some(Rc::new(move |_, cur| vec![crate::cmd::draw::rect(a, cur)])));
                let b = self.must_point(self.t("sel.window.second"), Opts { base: Some(a), ..Opts::default() }).await?;
                let w = Bounds { min_x: js::min(a.x, b.x), min_y: js::min(a.y, b.y), max_x: js::max(a.x, b.x), max_y: js::max(a.y, b.y) };
                let ids = self.sh.host.borrow().pick_window(&self.doc(), &w, key == "C");
                self.take(&ids, set, remove);
                if !remove {
                    self.st().sel_windows.push(SelWin::Rect(w));
                }
                Ok(true)
            }
            "F" => {
                let mut pts: Vec<P2> = Vec::new();
                loop {
                    let last = pts.last().copied();
                    let snap = pts.clone();
                    self.preview(Some(Rc::new(move |_, cur| {
                        if snap.is_empty() {
                            return Vec::new();
                        }
                        let mut v = snap.clone();
                        v.push(cur);
                        vec![crate::cmd::draw::open_poly(&v)]
                    })));
                    let options = if pts.is_empty() { Vec::new() } else { vec![self.opt("U", &self.t("opt.undo"))] };
                    let prompt = if pts.is_empty() { self.t("sel.fence.first") } else { self.t("sel.fence.next") };
                    // the fence's points are copies ({x, y}): no z on the 3D tab
                    let p = self.point(prompt, Opts { base: last, none: true, options, flat_base: true, ..Opts::default() }).await?;
                    match p {
                        Reply::Null => break,
                        Reply::Opt(k) if k == "U" => {
                            pts.pop();
                        }
                        Reply::Point(q, _) => pts.push(q),
                        _ => break,
                    }
                }
                if pts.len() < 2 {
                    return Ok(true);
                }
                let ids: Vec<u64> = {
                    let d = self.doc();
                    let ents: Vec<Entity> = d.entities().filter(|e| d.editable(e)).cloned().collect();
                    fence_hits(&ents, &pts, Some(&d.blocks)).into_iter().flatten().collect()
                };
                self.take(&ids, set, remove);
                Ok(true)
            }
            "?" => {
                self.print(&self.t("sel.help"));
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}
