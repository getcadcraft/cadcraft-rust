//! CadCraft's LISP in the command line (lisp/install.js, session.js; the drawing side host-cad.js, dxflist.js,
//! vla-host.js; aliases.js; delblocks.js): APPLOAD, _LISPLOAD / _LISPEVAL (a loaded file, a typed "(…" or "!x"),
//! ALIASEDIT, REINIT, DELBLOCKS, and every (defun c:NAME …) a loaded routine makes as a command of its own.
//!
//! One session per window ([`LispSession`], made on first use): the loaded applications, the commands they made, and
//! the interpreter (cc-lisp). Everything runs inside an engine command, so a routine is one undo step and its prompts
//! are the command line's own; (command …) runs a command inside it ([`host::CadHost::command`]).

pub mod aliases;
pub mod delblocks;
pub mod dxflist;
pub mod host;
pub mod vlahost;

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use cc_lisp::Lisp;
use cc_lisp::value::{V, lstr};

use crate::cmd::CmdDef;
use crate::engine::{Ctx, R, Shared};
use crate::prompt::{CmdError, CmdFuture};

/// A file to load: its name, text and kind ("lsp" or "dcl").
#[derive(Clone, Debug, PartialEq)]
pub struct AppFile {
    pub name: String,
    pub text: String,
    pub kind: String,
}

/// What _LISPLOAD / _LISPEVAL run (`engine.lispPending`).
#[derive(Clone, Debug, PartialEq)]
pub enum LispJob {
    Load(Vec<AppFile>),
    Eval(String),
}

/// A loaded application: its text and the commands it made.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Loaded {
    pub name: String,
    pub text: String,
    pub kind: String,
    pub commands: Vec<String>,
}

/// The most distinct command names and labels kept for the session's lifetime (names are `&'static str` in the
/// command table; past this a label falls back to its command's own name).
const MAX_NAMES: usize = 20_000;

/// Every distinct name made so far (process-wide and never dropped: the names live as long as the command table).
static NAMES: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, &'static str>>> = std::sync::OnceLock::new();

/// A command name or label as the command table keeps it (each distinct text made once).
pub fn static_name(s: &str) -> &'static str {
    let mut map = NAMES.get_or_init(Default::default).lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(v) = map.get(s) {
        return v;
    }
    if map.len() >= MAX_NAMES {
        return "LISP";
    }
    let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
    map.insert(s.to_string(), leaked);
    leaked
}

/// The command line's text buffer: princ writes pieces, lines go to the history when they end (or before a prompt).
#[derive(Default)]
struct OutBuffer {
    buf: String,
}

/// One LISP session per window.
pub struct LispSession {
    /// the interpreter (taken out while a run has it)
    lisp: RefCell<Option<Box<Lisp>>>,
    pub loaded: RefCell<Vec<Loaded>>,
    /// command name → the file that defined it (a JS Map: the order defined)
    pub defined: RefCell<Vec<(String, String)>>,
    out: RefCell<OutBuffer>,
    busy: Cell<bool>,
    loading: RefCell<Option<String>>,
    pub cad: Rc<host::CadHost>,
    stopped: Rc<Cell<bool>>,
    sh: Weak<Shared>,
    /// the "(…" being typed over several lines (appload.js `pending`)
    pub pending: RefCell<String>,
}

impl std::fmt::Debug for LispSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LispSession").field("loaded", &self.loaded.borrow().len()).finish()
    }
}

impl LispSession {
    /// The session of this engine (made on first use).
    pub fn of(c: &Ctx) -> Rc<LispSession> {
        if let Some(s) = c.st().lisp.clone() {
            return s;
        }
        let sh = Rc::downgrade(&c.sh);
        let pr = c.pr.clone();
        let s = Rc::new_cyclic(|w: &Weak<LispSession>| {
            let cad = host::CadHost::make(sh.clone(), pr, w.clone());
            LispSession {
                lisp: RefCell::new(None),
                loaded: RefCell::new(Vec::new()),
                defined: RefCell::new(Vec::new()),
                out: RefCell::new(OutBuffer::default()),
                busy: Cell::new(false),
                loading: RefCell::new(None),
                cad,
                stopped: Rc::new(Cell::new(false)),
                sh,
                pending: RefCell::new(String::new()),
            }
        });
        c.st().lisp = Some(Rc::clone(&s));
        s
    }

    fn shared(&self) -> Option<Rc<Shared>> {
        self.sh.upgrade()
    }
    fn print(&self, s: &str) {
        if let Some(sh) = self.shared() {
            sh.print(s);
        }
    }
    fn t(&self, key: &str, vars: &[(&str, String)]) -> String {
        self.shared().map_or_else(|| key.to_string(), |sh| sh.tv(key, vars))
    }

    /// princ's text into the buffer: whole lines print, the rest waits.
    pub fn write(&self, s: &str) {
        let lines: Vec<String> = {
            let Ok(mut o) = self.out.try_borrow_mut() else { return };
            o.buf.push_str(s);
            let mut parts: Vec<String> = o.buf.split('\n').map(str::to_string).collect();
            o.buf = parts.pop().unwrap_or_default();
            parts
        };
        for line in lines {
            if !line.is_empty() {
                self.print(line.strip_suffix('\r').unwrap_or(&line));
            }
        }
    }
    /// What is left in the buffer prints (before a prompt, at the end of a run).
    pub fn flush(&self) {
        let rest = match self.out.try_borrow_mut() {
            Ok(mut o) => std::mem::take(&mut o.buf),
            Err(_) => return,
        };
        if !rest.is_empty() {
            self.print(&rest);
        }
    }

    /// Is a routine running?
    pub fn busy(&self) -> bool {
        self.busy.get()
    }

    /// Make the interpreter (first use).
    fn ensure_lisp(self: &Rc<Self>) {
        if self.lisp.try_borrow().map_or(true, |l| l.is_some()) {
            return;
        }
        let w = Rc::downgrade(self);
        let w2 = w.clone();
        let host: Rc<dyn cc_lisp::Host> = self.cad.clone();
        let mut l = cc_lisp::new_lisp(
            Some(host),
            Rc::new(move |s: &str| {
                if let Some(me) = w.upgrade() {
                    me.write(s);
                }
            }),
        );
        l.on_defun = Some(Rc::new(move |n: &str| {
            if let Some(me) = w2.upgrade() {
                me.defun(n);
            }
        }));
        l.stopped = Rc::clone(&self.stopped);
        if let Some(lim) = self.shared().and_then(|sh| sh.st.borrow().lisp_limits) {
            l.limits = lim;
        }
        if let Some(sh) = self.shared() {
            let lang = sh.lang.clone();
            l.missing_note = Rc::new(move |name: &str| lang.t("lisp.missing.known", &[("name", name.to_string())]));
        }
        if let Ok(mut slot) = self.lisp.try_borrow_mut() {
            *slot = Some(Box::new(l));
        }
    }

    /// A (defun c:NAME …): the command is the loading file's (or the command line's).
    fn defun(&self, name: &str) {
        let n = name.to_uppercase();
        let owner = self.loading.borrow().clone().unwrap_or_else(|| self.t("lisp.owner.cmdline", &[]));
        let before = {
            let mut d = self.defined.borrow_mut();
            let before = d.iter().find(|(k, _)| *k == n).map(|(_, v)| v.clone());
            match d.iter_mut().find(|(k, _)| *k == n) {
                Some(slot) => slot.1 = owner.clone(),
                None => d.push((n.clone(), owner.clone())),
            }
            before
        };
        if before.as_deref() != Some(owner.as_str()) {
            self.sync(std::slice::from_ref(&n), &[]);
        }
        if let Some(f) = self.loaded.borrow_mut().iter_mut().find(|f| f.name == owner)
            && !f.commands.contains(&n)
        {
            f.commands.push(n);
        }
    }

    /// The routines' commands into the command table (install.js `sync`): a built-in command of the same name stays.
    fn sync(&self, added: &[String], removed: &[String]) {
        let Some(sh) = self.shared() else { return };
        for n in added {
            let exact = sh.lookup(n);
            let builtin =
                crate::cmd::COMMANDS.iter().any(|c| c.name == n.as_str()) || exact.is_some_and(|c| c.name == n.as_str() && !is_lisp(&c));
            if builtin {
                sh.print(&sh.tv("lisp.cmd.shadow", &[("name", n.clone())]));
                continue;
            }
            let mut st = sh.st.borrow_mut();
            if !st.lisp_cmds.iter().any(|c| c.name == n.as_str()) {
                st.lisp_cmds.push(CmdDef { name: static_name(n), aliases: &[], no_repeat: false, keep_selection: false, run: run_routine });
            }
        }
        if !removed.is_empty() {
            sh.st.borrow_mut().lisp_cmds.retain(|c| !removed.iter().any(|r| r == c.name));
        }
    }

    /// Run a job on the interpreter: busy meanwhile, the buffer flushed after. A run stopped from outside (its
    /// command dropped) leaves the interpreter in place; the next run puts its bindings back first.
    // the interpreter stays borrowed for the whole run on purpose: a nested run (a routine's command calling LISP)
    // finds it busy and gets `dflt`, as 0.5's one runner does
    #[allow(clippy::await_holding_refcell_ref)]
    async fn run<T>(self: &Rc<Self>, job: impl AsyncFnOnce(&mut Lisp) -> T, dflt: T) -> T {
        self.ensure_lisp();
        let Ok(mut slot) = self.lisp.try_borrow_mut() else { return dflt };
        let Some(l) = slot.as_mut() else { return dflt };
        l.abort();
        self.busy.set(true);
        self.stopped.set(false);
        let guard = RunGuard { s: Rc::clone(self) };
        let r = job(l).await;
        drop(guard);
        drop(slot);
        self.flush();
        r
    }

    /// Load a file's text (kind "lsp" | "dcl"; a .dcl only goes into the session's files for load_dialog) → the outcome,
    /// the commands of the file, the ones it added.
    pub async fn load(self: &Rc<Self>, name: &str, text: &str, kind: &str) -> (cc_lisp::Outcome, Vec<String>) {
        {
            let mut loaded = self.loaded.borrow_mut();
            let commands = loaded.iter().find(|f| f.name == name).map(|f| f.commands.clone()).unwrap_or_default();
            let entry = Loaded { name: name.into(), text: text.into(), kind: kind.into(), commands };
            match loaded.iter_mut().find(|f| f.name == name) {
                Some(slot) => *slot = entry,
                None => loaded.push(entry),
            }
        }
        if kind == "dcl" {
            self.ensure_lisp();
            if let Ok(mut slot) = self.lisp.try_borrow_mut()
                && let Some(l) = slot.as_mut()
            {
                l.put_file(&lstr(name), &lstr(text));
            }
            return (cc_lisp::Outcome { ok: true, value: V::Nil, error: None, cancel: false }, Vec::new());
        }
        *self.loading.borrow_mut() = Some(name.to_string());
        let (n2, t2) = (lstr(name), lstr(text));
        let me = Rc::clone(self);
        let res = self
            .run(
                async move |l: &mut Lisp| {
                    l.put_file(&n2, &t2);
                    let r = l.load_text(&t2, &rstr_of(&n2)).await;
                    *me.loading.borrow_mut() = None;
                    r
                },
                busy_outcome(),
            )
            .await;
        *self.loading.borrow_mut() = None;
        let commands = self.loaded.borrow().iter().find(|f| f.name == name).map(|f| f.commands.clone()).unwrap_or_default();
        (res, commands)
    }

    /// Forget a loaded file and the commands it made → those commands.
    pub fn unload(&self, name: &str) -> Vec<String> {
        let f = {
            let mut loaded = self.loaded.borrow_mut();
            let Some(k) = loaded.iter().position(|f| f.name == name) else { return Vec::new() };
            loaded.remove(k)
        };
        let gone: Vec<String> = {
            let d = self.defined.borrow();
            f.commands.iter().filter(|c| d.iter().any(|(k, v)| k == *c && v == name)).cloned().collect()
        };
        self.defined.borrow_mut().retain(|(k, _)| !gone.contains(k));
        if !gone.is_empty() {
            self.sync(&[], &gone);
        }
        gone
    }

    /// The command line's "(…" and "!symbol": the value is echoed.
    pub async fn eval_text(self: &Rc<Self>, text: &str) -> cc_lisp::Outcome {
        let t = lstr(text);
        let res = self.run(async move |l: &mut Lisp| l.eval_text(&t).await, busy_outcome()).await;
        if res.ok {
            let shown = self.show(&res.value);
            self.write(&format!("\n{shown}\n"));
            self.flush();
        }
        res
    }

    /// Run a (defun c:…) command; a command it started and left running is the person's to finish.
    pub async fn call(self: &Rc<Self>, c: &Ctx, name: &str) -> cc_lisp::Outcome {
        let n = name.to_string();
        let res = self.run(async move |l: &mut Lisp| l.call_command(&n).await, busy_outcome()).await;
        let silent = res.ok && matches!(&res.value, V::Str(s) if s.is_empty());
        if res.ok && !silent {
            let shown = self.show(&res.value);
            self.write(&format!("\n{shown}\n"));
            self.flush();
        }
        self.cad.finish_nested(c).await;
        res
    }

    /// prin1's text of a value (the interpreter's symbols name it).
    fn show(&self, v: &V) -> String {
        match self.lisp.try_borrow() {
            Ok(l) => l.as_ref().map_or_else(|| "nil".into(), |l| l.show(v, true)),
            Err(_) => "nil".into(),
        }
    }

    /// Esc while a routine runs: it stops at its next check.
    pub fn stop(&self, sh: &Shared) {
        self.stopped.set(true);
        let _ = sh;
        let msg = self.t("lisp.stopped", &[]);
        self.write(&format!("\n{msg}\n"));
    }

    /// A value typed into a waiting prompt ("(* 2 3.5)" at "Specify radius:"): its prin1 text, or the error line. The
    /// drawing side answers what it can at once (getvar …); anything that would ask the person is refused.
    pub fn eval_for_prompt(self: &Rc<Self>, text: &str) -> Result<String, String> {
        if self.busy() {
            return Err(String::new());
        }
        self.ensure_lisp();
        let Ok(mut slot) = self.lisp.try_borrow_mut() else { return Err(String::new()) };
        let Some(l) = slot.as_mut() else { return Err(String::new()) };
        self.cad.prompt_eval.set(true);
        let units = lstr(text);
        let r = cc_lisp::block_on(l.eval_text(&units));
        self.cad.prompt_eval.set(false);
        let out = match r {
            Some(o) if o.ok => Ok(l.show(&o.value, true)),
            Some(o) => Err(format!("; error: {}", o.error.unwrap_or_default())),
            None => Err(String::new()),
        };
        l.abort();
        out
    }

    /// The commands a loaded file made.
    pub fn commands_of(&self, name: &str) -> Vec<String> {
        self.loaded.borrow().iter().find(|f| f.name == name).map(|f| f.commands.clone()).unwrap_or_default()
    }
}

fn busy_outcome() -> cc_lisp::Outcome {
    cc_lisp::Outcome { ok: false, value: V::Nil, error: Some("LISP is busy".into()), cancel: false }
}

fn rstr_of(s: &[u16]) -> String {
    cc_lisp::value::rstr(s)
}

/// Puts the interpreter's state right when a run is dropped half way (its command was stopped from outside).
struct RunGuard {
    s: Rc<LispSession>,
}
impl Drop for RunGuard {
    fn drop(&mut self) {
        self.s.busy.set(false);
    }
}

/// Is this one of the routines' commands?
pub fn is_lisp(c: &CmdDef) -> bool {
    std::ptr::fn_addr_eq(c.run, run_routine as fn(Ctx) -> CmdFuture)
}

/// A routine's own command (`<NAME>`): the (defun c:NAME …) called.
pub fn run_routine(c: Ctx) -> CmdFuture {
    Box::pin(async move {
        let name = c.st().running.unwrap_or("");
        let s = LispSession::of(&c);
        let name = s.defined.borrow().iter().find(|(k, _)| k.as_str() == name).map_or_else(|| name.to_string(), |(k, _)| k.clone());
        s.call(&c, &name).await;
        Ok(())
    })
}

/// APPLOAD (AP): the window's Load applications dialog, or the loaded list on the command line.
pub async fn appload(c: Ctx) -> R {
    let s = LispSession::of(&c);
    if c.sh.host.borrow_mut().appload() {
        return Ok(());
    }
    let loaded = s.loaded.borrow().clone();
    if loaded.is_empty() {
        c.print(&c.t("appload.none"));
        return Ok(());
    }
    for f in loaded {
        let cmds = if f.commands.is_empty() { "—".to_string() } else { f.commands.join(", ") };
        c.print(&c.tv("appload.line", &[("name", f.name.clone()), ("commands", cmds)]));
    }
    Ok(())
}

/// _LISPLOAD: load the files of `engine.lispPending` (a dropped .lsp, the Startup Suite, a script).
pub async fn lispload(c: Ctx) -> R {
    let job = c.st().lisp_pending.take();
    let Some(LispJob::Load(files)) = job else { return Ok(()) };
    let s = LispSession::of(&c);
    for f in files {
        let kind = if f.kind.is_empty() { "lsp".to_string() } else { f.kind.clone() };
        let (res, commands) = s.load(&f.name, &f.text, &kind).await;
        let line = if res.ok {
            c.tv("appload.loaded", &[("name", f.name.clone()), ("n", commands.len().to_string()), ("commands", commands.join(", "))])
        } else {
            c.tv("appload.failed", &[("name", f.name.clone()), ("error", res.error.clone().unwrap_or_default())])
        };
        c.print(&line);
    }
    s.cad.drop_nested(&c);
    Ok(())
}

/// _LISPEVAL: the command line's "(…" or "!symbol".
pub async fn lispeval(c: Ctx) -> R {
    let job = c.st().lisp_pending.take();
    let Some(LispJob::Eval(text)) = job else { return Ok(()) };
    let t = cc_cad::js::trim_start(&text);
    let text = t.strip_prefix('!').map_or(text.clone(), str::to_string);
    let s = LispSession::of(&c);
    s.eval_text(&text).await;
    s.cad.drop_nested(&c);
    Ok(())
}

/// ALIASEDIT: the window's alias editor.
pub async fn aliasedit(c: Ctx) -> R {
    if c.sh.host.borrow_mut().alias_edit() {
        return Ok(());
    }
    c.print(&c.t("aliases.nodialog"));
    Ok(())
}

/// REINIT: the person's aliases file applied again.
pub async fn reinit(c: Ctx) -> R {
    let text = c.sh.host.borrow().alias_text().unwrap_or_default();
    let r = aliases::apply(&c.sh, &text);
    c.print(&c.tv("aliases.reinit", &[("n", r.applied.to_string())]));
    for (line, msg) in r.errors {
        c.print(&c.tv("aliases.errline", &[("line", line.to_string()), ("msg", msg)]));
    }
    Ok(())
}

/// The errors of a command a routine ran, as the command line prints them.
pub fn cmd_error_text(e: &CmdError) -> String {
    match e {
        CmdError::Cancelled => "*Cancel*".into(),
        CmdError::Msg(m) => format!("Error: {m}"),
    }
}

/// The command line's side of LISP (appload.js `lispLine`, `lispTyping`, `cancelEdit`, `runLoad`).
impl crate::engine::Engine {
    /// The window's LISP session, made on first use.
    pub fn lisp_session(&mut self) -> Rc<LispSession> {
        let c = Ctx { sh: Rc::clone(&self.sh), pr: self.runner.prompter.clone() };
        LispSession::of(&c)
    }

    /// Is a LISP expression being typed (so a space is part of it, not Enter)?
    pub fn lisp_typing(&self, text: &str) -> bool {
        let pending = self.sh.st.borrow().lisp.as_ref().is_some_and(|s| !s.pending.borrow().is_empty());
        pending || cc_cad::js::trim_start(text).starts_with('(')
    }

    /// A line typed at the command line: "(…" and "!symbol" are LISP (between commands evaluated, at a prompt its
    /// value answers it) → false when it is not LISP (the command line takes it).
    pub fn lisp_line(&mut self, text: &str) -> bool {
        let session = self.lisp_session();
        let pending_empty = session.pending.borrow().is_empty();
        let t = cc_cad::js::trim_start(text);
        if pending_empty && !(t.starts_with('(') || t.starts_with('!')) {
            return false;
        }
        if self.expects().is_some() {
            if !pending_empty || !t.starts_with('(') || session.busy() {
                return false;
            }
            match session.eval_for_prompt(text) {
                Ok(shown) => {
                    let answer = prompt_answer(&shown);
                    self.text(&answer);
                }
                Err(line) => {
                    if !line.is_empty() {
                        self.sh.print(&line);
                    }
                }
            }
            return true;
        }
        let job = {
            let mut p = session.pending.borrow_mut();
            if !p.is_empty() {
                p.push('\n');
            }
            p.push_str(text);
            let bang = cc_cad::js::trim_start(&p).starts_with('!');
            if !cc_lisp::reader::balanced(&p) && !bang {
                drop(p);
                self.sh.host.borrow_mut().prompt("(_>");
                return true;
            }
            std::mem::take(&mut *p)
        };
        let label: String = job.split(|c: char| cc_cad::js::is_ws(c)).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
        self.run_hidden("_LISPEVAL", &label, Some(LispJob::Eval(job)));
        true
    }

    /// Esc: a half-typed "(…" is dropped, a busy routine stopped → false when there was neither.
    pub fn lisp_cancel(&mut self) -> bool {
        let Some(session) = self.sh.st.borrow().lisp.clone() else { return false };
        if !session.pending.borrow().is_empty() && self.expects().is_none() {
            session.pending.borrow_mut().clear();
            self.sh.host.borrow_mut().prompt("");
            return true;
        }
        self.stop_lisp()
    }

    /// The aliases file ALIASEDIT starts from when the person has none (the built-in aliases, aliases.js template).
    pub fn alias_template(&self) -> String {
        aliases::template(&self.sh)
    }

    /// Load applications (APPLOAD's dialog, a dropped .lsp, the Startup Suite): _LISPLOAD under `label`.
    pub fn lisp_load(&mut self, files: Vec<AppFile>, label: &str) {
        self.run_hidden("_LISPLOAD", label, Some(LispJob::Load(files)));
    }
}

/// What a value typed into a prompt answers: a point (x y [z]) as "x,y", a string without its quotes, else as printed.
fn prompt_answer(shown: &str) -> String {
    if let Some(inner) = shown.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
        let parts: Vec<&str> = inner.split(' ').collect();
        let numeric = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit() || matches!(c, '-' | '.' | 'e' | 'E' | '+'));
        if (2..=3).contains(&parts.len()) && parts.iter().all(|p| numeric(p)) {
            return format!("{},{}", parts.first().copied().unwrap_or(""), parts.get(1).copied().unwrap_or(""));
        }
    }
    if shown.len() >= 2
        && let Some(inner) = shown.strip_prefix('"').and_then(|x| x.strip_suffix('"'))
    {
        return inner.to_string();
    }
    shown.to_string()
}
