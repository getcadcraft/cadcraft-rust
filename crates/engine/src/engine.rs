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
    /// Rubber-band entities to draw over the drawing.
    fn preview(&mut self, ents: &[Entity]);
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
}

/// Rubber-band entities for a cursor position.
pub type PreviewFn = Rc<dyn Fn(&Shared, P2) -> Vec<Entity>>;

pub(crate) struct Waiting {
    pub kind: Kind,
    pub prompt: String,
    pub opts: Opts,
    pub set: IdSet,
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
    /// the windows the last "Select objects" was given (`set.windows`: STRETCH stretches what they hold)
    pub sel_windows: Vec<Bounds>,
    /// what commands remember between runs (`E.hatch`, `E.filletR`, …)
    pub memo: crate::cmd::Memo,
}

/// What the engine and its commands share.
pub struct Shared {
    pub doc: RefCell<Doc>,
    pub host: RefCell<Box<dyn Host>>,
    pub st: RefCell<State>,
    pub lang: Lang,
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
    fn prompt_text(&self) -> String {
        let st = self.st.borrow();
        let Some(w) = st.waiting.as_ref() else { return String::new() };
        let ucs_ang = if w.opts.ucs { ang_of(active_ucs(&self.doc.borrow())) } else { 0.0 };
        prompt_text(&w.prompt, w.kind == Kind::Angle, w.kind == Kind::Point, &w.opts.options, w.opts.default.as_ref(), ucs_ang)
    }
    /// `engine.preview(cursor)`: the rubber band for a cursor position.
    pub fn preview_at(&self, cursor: P2) -> Vec<Entity> {
        let f = self.st.borrow().preview_fn.clone();
        f.map_or_else(Vec::new, |f| f(self, cursor))
    }
}

fn def_reply(d: &Option<Def>) -> Reply {
    match d {
        None => Reply::Null,
        Some(Def::Num(n)) => Reply::Num(*n),
        Some(Def::Str(s)) => Reply::Str(s.clone()),
    }
}

/// The command line.
pub struct Engine {
    sh: Rc<Shared>,
    runner: Runner,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine").field("running", &self.running()).finish()
    }
}

impl Engine {
    pub fn new(doc: Doc, host: Box<dyn Host>, lang: Lang) -> Self {
        let sh = Rc::new(Shared { doc: RefCell::new(doc), host: RefCell::new(host), st: RefCell::new(State::default()), lang });
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

    fn ctx(&self) -> Ctx {
        Ctx { sh: Rc::clone(&self.sh), pr: self.runner.prompter.clone() }
    }

    /// Start a command by name or alias.
    pub fn run(&mut self, name: &str) {
        let Some(c) = cmd::lookup(name) else {
            let msg = self.sh.tv("cmd.unknown", &[("name", name.to_uppercase())]);
            self.sh.print(&msg);
            return;
        };
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
        self.runner.start((c.run)(self.ctx()));
        self.after_poll();
    }

    /// The command ended (done, Esc or an error): print, fold its edits into one undo step, reset.
    fn after_poll(&mut self) {
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
        }
        let mut h = self.sh.host.borrow_mut();
        if !keep {
            h.set_selection(&[]);
        }
        h.preview(&[]);
        h.prompt("");
        h.on_command(None);
    }

    fn resolve(&mut self, v: Reply) {
        {
            let mut st = self.sh.st.borrow_mut();
            let Some(w) = st.waiting.take() else { return };
            if let Reply::Point(p, _) = &v
                && w.kind == Kind::Point
            {
                st.last_point = Some(*p);
            }
            st.reply = Some((v, w.set));
        }
        self.runner.answer(Answer::None);
        self.after_poll();
    }

    fn waiting_info(&self) -> Option<(Kind, Opts, IdSet)> {
        let st = self.sh.st.borrow();
        st.waiting.as_ref().map(|w| (w.kind.clone(), w.opts.clone(), w.set.clone()))
    }

    /// A line typed at the command line.
    pub fn text(&mut self, raw: &str) {
        let typed = js::trim(raw).to_string();
        let Some((kind, opts, set)) = self.waiting_info() else {
            if typed.is_empty() {
                let last = self.sh.st.borrow().last;
                if let Some(l) = last {
                    self.run(l);
                }
                return;
            }
            self.run(&typed);
            return;
        };
        if kind == Kind::Text {
            return self.resolve(Reply::Str(raw.to_string()));
        }
        if kind == Kind::Select && typed.is_empty() {
            return self.resolve(Reply::Set(set));
        }
        // Enter where a point or an object is required (and there is no default) ends the command
        if typed.is_empty() && matches!(kind, Kind::Point | Kind::Entity) && !opts.none && opts.default.is_none() {
            self.escape();
            return;
        }
        if typed.is_empty() {
            return self.resolve(def_reply(&opts.default));
        }
        if let Some(o) = match_option(&typed, &opts.options) {
            return self.resolve(Reply::Opt(o.key.clone()));
        }
        if let Some(acc) = opts.accept
            && let Some(v) = acc(&typed)
        {
            return self.resolve(Reply::Typed(v));
        }
        let lunits = self.lunits();
        match kind {
            Kind::Point => {
                let pc = {
                    let st = self.sh.st.borrow();
                    PointCtx {
                        lunits,
                        ucs: active_ucs(&self.sh.doc.borrow()),
                        last_point: st.last_point,
                        base: opts.base.or(st.base),
                        cursor: self.sh.host.borrow().cursor(),
                    }
                };
                if let Some(p) = parse_point(&typed, &pc) {
                    return self.resolve(Reply::Point(p, None));
                }
            }
            Kind::Distance => {
                if let Some(n) = parse_length(&typed, lunits)
                    && n >= 0.0
                {
                    return self.resolve(Reply::Num(n));
                }
            }
            Kind::Number => {
                if opts.length {
                    if let Some(n) = parse_length(&typed, lunits) {
                        return self.resolve(Reply::Num(n));
                    }
                } else if is_plain_number(&typed, true) {
                    return self.resolve(Reply::Num(plain_number(&typed)));
                }
            }
            Kind::Angle => {
                if is_plain_number(&typed, false) {
                    let u = if opts.ucs { ang_of(active_ucs(&self.sh.doc.borrow())) } else { 0.0 };
                    return self.resolve(Reply::Num(rad(plain_number(&typed)) + u));
                }
            }
            Kind::Select => {
                let u = typed.to_uppercase();
                if u == "ALL" {
                    let ids: Vec<u64> = {
                        let d = self.sh.doc.borrow();
                        d.entities().filter(|e| d.editable(e)).filter_map(|e| e.id).collect()
                    };
                    let set = self.with_set(|s| {
                        for id in ids {
                            s.add(id);
                        }
                    });
                    let mut h = self.sh.host.borrow_mut();
                    h.set_selection(&set.0);
                    h.print(&format!("{} found", set.len()));
                    return;
                }
                if u == "L" || u == "LAST" {
                    let last = self.sh.doc.borrow().entities().last().and_then(|e| e.id);
                    if let Some(id) = last.filter(|i| *i != 0) {
                        let set = self.with_set(|s| {
                            s.add(id);
                        });
                        let mut h = self.sh.host.borrow_mut();
                        h.set_selection(&set.0);
                        h.print("1 found");
                    }
                    return;
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
            Kind::Text => "cmd.invalid_input",
        };
        let msg = self.sh.t(key);
        self.sh.print(&msg);
        let p = self.sh.prompt_text();
        self.sh.host.borrow_mut().prompt(&p);
    }

    fn lunits(&self) -> f64 {
        self.sh.doc.borrow().meta.get("lunits").map_or(2.0, js::to_number)
    }

    /// Change the selection being built by the waiting "Select objects" → a copy of it.
    fn with_set(&self, f: impl FnOnce(&mut IdSet)) -> IdSet {
        let mut st = self.sh.st.borrow_mut();
        match st.waiting.as_mut() {
            Some(w) => {
                f(&mut w.set);
                w.set.clone()
            }
            None => IdSet::default(),
        }
    }

    /// A click: `point` after snaps and ortho (with a deferred tangent when the snap gave one), `raw` the cursor.
    /// → false when the host should start a selection window instead.
    pub fn click(&mut self, point: P2, tan: Option<Tan>, raw: P2) -> bool {
        let Some((kind, opts, set)) = self.waiting_info() else { return false };
        match kind {
            Kind::Point => {
                self.resolve(Reply::Point(point, tan));
                true
            }
            Kind::Distance | Kind::Angle => {
                match opts.base {
                    Some(b) => self.resolve(Reply::Num(if kind == Kind::Distance { dist(b, point) } else { angle_of(b, point) })),
                    None => {
                        {
                            let mut st = self.sh.st.borrow_mut();
                            if let Some(w) = st.waiting.as_mut() {
                                w.opts.base = Some(point);
                            }
                            st.base = Some(point);
                        }
                        let msg = self.sh.t("cmd.specify_second_point");
                        self.sh.host.borrow_mut().prompt(&msg);
                    }
                }
                true
            }
            Kind::Select => {
                let id = self.sh.host.borrow().pick_at(&self.sh.doc.borrow(), raw);
                let Some(id) = id else { return false };
                let shift = self.sh.host.borrow().shift();
                let set = self.with_set(|s| {
                    if opts.remove || (s.has(id) && shift) {
                        s.delete(id);
                    } else {
                        s.add(id);
                    }
                });
                self.sh.host.borrow_mut().set_selection(&set.0);
                if opts.single {
                    self.resolve(Reply::Set(set));
                }
                let _ = set;
                true
            }
            Kind::Entity => {
                let id = self.sh.host.borrow().pick_at(&self.sh.doc.borrow(), raw);
                let shift = self.sh.host.borrow().shift();
                if let Some(id) = id {
                    self.resolve(Reply::Pick { id, at: raw, shift });
                } else if opts.empty {
                    self.resolve(Reply::Empty { at: raw, shift });
                } else {
                    let msg = self.sh.t("cmd.nothing_found_there");
                    self.sh.print(&msg);
                }
                let _ = set;
                true
            }
            _ => false,
        }
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
            self.sh.st.borrow_mut().sel_windows.push(rect);
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
    pr: Prompter,
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
            st.waiting = Some(Waiting { kind, prompt, opts, set });
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

    /// Set the rubber band (None clears it); drawn at once for the cursor.
    pub fn preview(&self, f: Option<PreviewFn>) {
        let has = f.is_some();
        self.st().preview_fn = f;
        let cur = self.sh.host.borrow().cursor();
        let ents = match (has, cur) {
            (true, Some(c)) => self.sh.preview_at(c),
            _ => Vec::new(),
        };
        self.sh.host.borrow_mut().preview(&ents);
    }

    /// One change of the document (an undo step, folded into the command's when it ends).
    pub fn change<T>(&self, label: &str, f: impl FnOnce(&mut Doc) -> Result<T, cc_cad::DocError>) -> R<T> {
        self.sh.doc.borrow_mut().change(label, f).map_err(doc_err)
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
            let opts = Opts { none: true, remove, options, ..Opts::default() };
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
        self.sh.host.borrow_mut().preview(&[]);
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
                    self.st().sel_windows.push(w);
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
                    let p = self.point(prompt, Opts { base: last, none: true, options, ..Opts::default() }).await?;
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
