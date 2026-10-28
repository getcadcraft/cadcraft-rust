//! The evaluator (interp.js) as an explicit-stack machine.
//!
//! 0.5's evaluator is an async recursive function; here every pending evaluation is a [`Frame`] on a heap stack, so
//! a recursion a million deep (0.5 takes them: its awaits unwind the JS stack) costs memory, not the thread's stack,
//! and the machine can stop anywhere: for the drawing side's answers (a point, a running command) it hands a future
//! out ([`Ctl::Await`]) and the async driver ([`Lisp::run`]) awaits it at its one top level.
//!
//! Semantics, as AutoLISP and 0.5: one namespace (a symbol's value may be a function); DYNAMIC scope — a function's
//! arguments and its locals after "/" are bound while it runs and restored when it returns (or fails); nil is the empty
//! list and false; integers and reals differ ((/ 7 2) is 3). Errors reach the innermost *error* function that is bound
//! when they happen (so a local *error* works), unless vl-catch-all-apply is catching; with none, "; error: <message>"
//! is printed. The messages are AutoLISP's own wording ("bad argument type: numberp: nil"), because routines test them.
//!
//! Budgets (CLAUDE.md § 4: scripts are hostile input): a step limit and a pending-frame limit per run, a stop flag
//! (Esc) checked every 4096 steps, and the string, list and array caps in [`Limits`].

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use crate::host::Host;
use crate::reader::read_all;
use crate::syms::{S_ERROR, S_LAMBDA, S_QUOTE, S_SLASH, S_T, Syms};
use crate::value::{
    Builtin, EId, Fun, Imp, LErr, LFile, LStr, Lambda, MSG_CANCEL, MSG_QUIT, MSG_STACK, PickSet, R, Sym, V, from_vec, lstr, real_text,
    rstr, to_vec,
};

/// A boxed future the machine awaits (the drawing side's answer).
pub type LFut<T> = Pin<Box<dyn Future<Output = T>>>;

/// What a built-in or a frame tells the machine to do next.
pub enum Ctl {
    /// evaluate this form
    Eval(V),
    /// this is the value
    Ret(V),
    /// this error unwinds
    Throw(LErr),
    /// wait for the drawing side, then continue with the answer
    Await(LFut<Box<dyn Any>>, Cont),
}

/// What runs with an awaited answer.
pub type Cont = Box<dyn FnOnce(&mut Lisp, Box<dyn Any>) -> Ctl>;

/// A continuation on a returned value.
pub type Then = Box<dyn FnOnce(&mut Lisp, V) -> Ctl>;
/// Where text goes (a name, a line).
pub type OutFn = Rc<dyn Fn(&str)>;
/// The host's note for a function 0.5 lacks (the function's name → the line).
pub type NoteFn = Rc<dyn Fn(&str) -> String>;

/// Await a future of the drawing side, then continue with `k(answer)`.
pub fn await_with<T: 'static>(fut: LFut<T>, k: impl FnOnce(&mut Lisp, T) -> Ctl + 'static) -> Ctl {
    Ctl::Await(
        Box::pin(async move { Box::new(fut.await) as Box<dyn Any> }),
        Box::new(move |l, a| match a.downcast::<T>() {
            Ok(t) => k(l, *t),
            Err(_) => Ctl::Throw(LErr::new("internal: an answer of the wrong type")),
        }),
    )
}

/// A built-in's body.
#[derive(Clone, Copy)]
pub enum Bf {
    /// a function that computes its value at once
    Sync(fn(&mut Lisp, &[V]) -> R<V>),
    /// a function that may evaluate, wait or push frames
    Ctl(fn(&mut Lisp, Vec<V>) -> Ctl),
    /// a special form: the unevaluated rest of the form
    Special(fn(&mut Lisp, V) -> Ctl),
}

/// Caps on what one interpreter may make.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// evaluation steps per run (None: no cap)
    pub steps: Option<u64>,
    /// pending frames (the recursion depth, roughly three per call)
    pub frames: usize,
    /// UTF-16 units in one string
    pub string: usize,
    /// elements of a safe array
    pub array: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { steps: Some(500_000_000), frames: 6_000_000, string: crate::value::MAX_STR, array: 1 << 24 }
    }
}

/// One bound symbol: its value and when it was first bound (a JS Map's order: ATOMS-FAMILY lists in it).
#[derive(Clone, Debug)]
struct Slot {
    v: V,
    seq: u64,
}

/// A pending evaluation.
enum Frame {
    /// the bottom of one run: errors that reach it go to *error*
    Top,
    /// evaluating a function's arguments
    Args {
        f: V,
        head: V,
        rest: V,
        args: Vec<V>,
    },
    /// evaluating a list as the function of a form
    Head {
        rest: V,
        head: V,
    },
    /// a body: the value of the last form
    Seq {
        body: Rc<[V]>,
        i: usize,
    },
    /// a function called: its bindings come back when it returns or fails
    Lambda {
        saved: Vec<(Sym, Option<V>)>,
    },
    /// *error* running for `err`; then `saved` comes back and `err` goes on
    Handler {
        err: LErr,
        saved: Option<Vec<(Sym, Option<V>)>>,
    },
    Setq {
        sym: Sym,
        rest: V,
    },
    If {
        then: V,
        els: Option<V>,
    },
    Cond {
        body: V,
        rest: Vec<V>,
    },
    While {
        test: V,
        body: Rc<[V]>,
        last: V,
        testing: bool,
    },
    Repeat {
        left: i64,
        body: Rc<[V]>,
        last: V,
        started: bool,
    },
    Foreach {
        var: Sym,
        items: V,
        body: Rc<[V]>,
        last: V,
    },
    And {
        rest: V,
    },
    Or {
        rest: V,
    },
    /// values bound for a while (foreach, vlax-for, a dialog action), back on return or error
    Restore {
        saved: Vec<(Sym, Option<V>)>,
    },
    /// a built-in's continuation (errors pass by)
    Then(Then),
    /// runs on return and on error (a dialog closing, catch depth)
    Finally(Box<dyn FnOnce(&mut Lisp)>),
    /// vl-catch-all-apply: an error (but Esc) becomes its value
    Catch,
}

/// What a run gave: the value, or the error that reached the top (already handed to *error* or printed).
#[derive(Clone, Debug)]
pub struct Outcome {
    pub ok: bool,
    pub value: V,
    pub error: Option<String>,
    pub cancel: bool,
}

/// An in-memory file (loaded applications, temp files): its name as given and its text.
#[derive(Clone, Debug)]
pub struct MemFile {
    pub name: LStr,
    pub text: LStr,
}

/// The interpreter.
pub struct Lisp {
    pub syms: Syms,
    globals: Vec<Option<Slot>>,
    seq: u64,
    /// in-memory files: normalised path → file, in the order made (a JS Map)
    pub files: Vec<(String, MemFile)>,
    pub host: Option<Rc<dyn Host>>,
    /// printed text (princ, prompt, error lines) goes here
    pub out: Rc<dyn Fn(&str)>,
    /// told when (defun c:NAME …) defines a command
    pub on_defun: Option<OutFn>,
    /// the line printed before "no function definition" for a function 0.5 lacks too (catalog 'lisp.missing.known')
    pub missing_note: NoteFn,
    /// Esc: the next 4096-step check stops the run with "Function cancelled"
    pub stopped: Rc<Cell<bool>>,
    pub limits: Limits,
    steps: u64,
    run_steps: u64,
    pub catching: u32,
    in_error: bool,
    missing_said: HashSet<String>,
    /// a pause for the window is due (every 4096 steps, when the drawing side has one)
    yield_due: bool,
    table: Vec<(String, Bf)>,
    frames: Vec<Frame>,
    /// what the built-ins keep (getenv, vl-bb, regapp, temp names, initget, dialogs)
    pub env: HashMap<String, LStr>,
    pub bb: HashMap<Sym, V>,
    pub appids: HashSet<String>,

    pub initget: Option<(i32, Option<LStr>)>,
    pub dcl: crate::dcl::DclState,
    /// the vla-host serials of block members, kept by the drawing side
    pub ext: HashMap<&'static str, Box<dyn Any>>,
}

impl std::fmt::Debug for Lisp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lisp").field("symbols", &self.syms.len()).field("frames", &self.frames.len()).finish()
    }
}

/// A function value for the machine.
fn fun(x: &V) -> Option<&Fun> {
    if let V::Fun(f) = x { Some(f) } else { None }
}

impl Lisp {
    /// A bare interpreter (no built-ins): [`crate::new_lisp`] makes a full one.
    pub fn bare(host: Option<Rc<dyn Host>>, out: Rc<dyn Fn(&str)>) -> Lisp {
        let mut l = Lisp {
            syms: Syms::new(),
            globals: Vec::new(),
            seq: 0,
            files: Vec::new(),
            host,
            out,
            on_defun: None,
            missing_note: Rc::new(crate::missing::english_note),
            stopped: Rc::new(Cell::new(false)),
            limits: Limits::default(),
            steps: 0,
            run_steps: 0,
            catching: 0,
            in_error: false,
            missing_said: HashSet::new(),
            yield_due: false,
            table: Vec::new(),
            frames: Vec::new(),
            env: HashMap::new(),
            bb: HashMap::new(),
            appids: ["ACAD", "CADCRAFT"].iter().map(|s| (*s).to_string()).collect(),

            initget: None,
            dcl: crate::dcl::DclState::default(),
            ext: HashMap::new(),
        };
        l.set(S_T, V::Sym(S_T));
        l
    }

    // ---- symbols ----

    pub fn sym(&mut self, name: &str) -> R<Sym> {
        self.syms.intern(name)
    }
    /// A built-in name (upper case, fixed).
    pub fn fsym(&mut self, name: &str) -> Sym {
        self.syms.fixed(name)
    }
    pub fn name(&self, s: Sym) -> &str {
        self.syms.name(s)
    }
    /// The value of a symbol (nil when unbound).
    pub fn get(&self, s: Sym) -> V {
        self.slot(s).map_or(V::Nil, |x| x.v.clone())
    }
    fn slot(&self, s: Sym) -> Option<&Slot> {
        self.globals.get(s.0 as usize).and_then(Option::as_ref)
    }
    /// Is the symbol bound?
    pub fn bound(&self, s: Sym) -> bool {
        self.slot(s).is_some()
    }
    /// The binding, None when unbound (a JS `globals.get` → undefined).
    pub fn raw(&self, s: Sym) -> Option<V> {
        self.slot(s).map(|x| x.v.clone())
    }
    /// Bind (nil unbinds), as `Map.set` / `delete`: a symbol bound already keeps its place in the order.
    pub fn set(&mut self, s: Sym, v: V) -> V {
        let i = s.0 as usize;
        if v.is_nil() {
            if let Some(x) = self.globals.get_mut(i) {
                *x = None;
            }
            return V::Nil;
        }
        if self.globals.len() <= i {
            self.globals.resize(i.saturating_add(1), None);
        }
        if let Some(x) = self.globals.get_mut(i) {
            match x {
                Some(slot) => slot.v = v.clone(),
                None => {
                    self.seq = self.seq.saturating_add(1);
                    *x = Some(Slot { v: v.clone(), seq: self.seq });
                }
            }
        }
        v
    }
    /// `restore(s, v)`: back to a saved binding (None = unbound).
    pub fn restore(&mut self, s: Sym, v: Option<V>) {
        match v {
            Some(v) if !v.is_nil() => {
                self.set(s, v);
            }
            _ => {
                if let Some(x) = self.globals.get_mut(s.0 as usize) {
                    *x = None;
                }
            }
        }
    }
    /// Every bound symbol in the order bound (ATOMS-FAMILY).
    pub fn bound_symbols(&self) -> Vec<Sym> {
        let mut v: Vec<(u64, Sym)> =
            self.globals.iter().enumerate().filter_map(|(i, s)| s.as_ref().map(|s| (s.seq, Sym(u32::try_from(i).unwrap_or(0))))).collect();
        v.sort_unstable_by_key(|x| x.0);
        v.into_iter().map(|x| x.1).collect()
    }

    /// Define a built-in under this name (a name defined again keeps its place, as `Map.set`).
    pub fn def(&mut self, name: &str, bf: Bf) {
        let s = self.fsym(name);
        let k = u16::try_from(self.table.len()).unwrap_or(u16::MAX);
        self.table.push((name.to_string(), bf));
        let special = matches!(bf, Bf::Special(_));
        self.set(s, V::Fun(Rc::new(Fun::Builtin(Builtin { name: name.to_ascii_uppercase(), imp: Imp::F(k), special }))));
    }

    // ---- output ----

    pub fn print(&self, s: &str) {
        (self.out)(s);
    }

    // ---- errors ----

    /// "bad argument type: <what>: <value>"
    pub fn bad(&self, what: &str, x: &V) -> LErr {
        LErr::new(format!("bad argument type: {what}: {}", self.show(x, true)))
    }

    fn no_function(&mut self, s: Sym) -> LErr {
        let name = self.name(s).to_string();
        let mut err = LErr::new(format!("no function definition: {name}"));
        if self.missing_said.insert(name.clone()) && crate::missing::known_missing(&name) {
            err.note = Some((self.missing_note)(&name));
        }
        err
    }

    /// A function made on first use (vla-get-<property> …), else the error.
    fn auto(&mut self, s: Sym) -> R<V> {
        if let Some(f) = crate::vla::auto_fn(self.name(s)) {
            let v = V::Fun(Rc::new(Fun::Builtin(f)));
            self.set(s, v.clone());
            return Ok(v);
        }
        Err(self.no_function(s))
    }

    /// A function value from what a routine passes: a function, a symbol naming one, or a quoted (lambda …) list.
    pub fn fn_of(&mut self, f: &V) -> R<V> {
        let mut x = f.clone();
        for _ in 0..100_000 {
            match &x {
                V::Sym(s) => {
                    let s = *s;
                    x = match self.raw(s) {
                        Some(v) => v,
                        None => self.auto(s)?,
                    };
                }
                V::Cons(c) => {
                    if matches!(c.car, V::Sym(S_LAMBDA)) {
                        return self.make_lambda(None, &c.cdr);
                    }
                    return self.make_lambda(None, &x);
                }
                V::Fun(_) => return Ok(x),
                _ => return Err(LErr::new(format!("bad function: {}", self.show(&x, true)))),
            }
        }
        Err(LErr::new(MSG_STACK))
    }

    /// `(args / locals) body…` → a function.
    pub fn make_lambda(&mut self, name: Option<String>, rest: &V) -> R<V> {
        let V::Cons(c) = rest else { return Err(LErr::new("bad argument list")) };
        let mut params = Vec::new();
        let mut locals = Vec::new();
        let mut in_locals = false;
        for p in to_vec(&c.car) {
            match p {
                V::Sym(S_SLASH) => in_locals = true,
                V::Sym(s) => {
                    if in_locals {
                        locals.push(s);
                    } else {
                        params.push(s);
                    }
                }
                _ => return Err(LErr::new(format!("bad argument list: {}", self.show(&c.car, true)))),
            }
        }
        Ok(V::Fun(Rc::new(Fun::Lambda(Lambda { name, params, locals, body: to_vec(&c.cdr).into() }))))
    }

    // ---- the machine ----

    /// Run these forms at the top level (load, the command line): an error goes to *error* if no function took it.
    pub async fn top(&mut self, forms: Vec<V>) -> Outcome {
        self.run_steps = 0;
        let base = self.frames.len();
        self.frames.push(Frame::Top);
        let ctl = self.seq(forms.into());
        let r = self.drive(ctl, base).await;
        match r {
            Ok(v) => Outcome { ok: true, value: v, error: None, cancel: false },
            Err(e) => Outcome { ok: false, value: V::Nil, cancel: e.cancel || e.msg == MSG_QUIT, error: Some(e.msg) },
        }
    }

    /// Run one form (an action, a routine's command) to its value; errors are returned, not handled.
    async fn drive(&mut self, first: Ctl, base: usize) -> R<V> {
        let mut ctl = first;
        loop {
            if self.yield_due {
                self.yield_due = false;
                let pause = self.host.as_ref().and_then(|h| h.pause());
                if let Some(p) = pause {
                    p.await;
                    if self.stopped.get() && !matches!(ctl, Ctl::Throw(_)) {
                        ctl = Ctl::Throw(LErr::cancel());
                    }
                }
            }
            ctl = match ctl {
                Ctl::Eval(x) => self.eval(x),
                Ctl::Ret(v) => {
                    if self.frames.len() <= base {
                        return Ok(v);
                    }
                    match self.frames.pop() {
                        Some(fr) => self.resume(fr, v),
                        None => return Ok(v),
                    }
                }
                Ctl::Throw(e) => {
                    if self.frames.len() <= base {
                        return Err(e);
                    }
                    match self.frames.pop() {
                        Some(fr) => self.unwind(fr, e),
                        None => return Err(e),
                    }
                }
                Ctl::Await(fut, k) => {
                    let a = fut.await;
                    k(self, a)
                }
            };
            if self.frames.len() > self.limits.frames {
                // the recursion has no end in sight: what V8 says when its stack runs out
                ctl = Ctl::Throw(LErr::new(MSG_STACK));
                // drop the deepest frames' work but keep the bindings they hold
                while self.frames.len() > self.limits.frames.saturating_sub(1000) {
                    if let Some(fr) = self.frames.pop() {
                        self.discard(fr);
                    }
                }
            }
        }
    }

    /// Restore what a dropped frame holds (an aborted run).
    fn discard(&mut self, fr: Frame) {
        match fr {
            Frame::Lambda { saved } | Frame::Restore { saved } => self.restore_all(saved),
            Frame::Handler { saved, .. } => {
                self.in_error = false;
                if let Some(s) = saved {
                    self.restore_all(s);
                }
            }
            Frame::Finally(f) => f(self),
            Frame::Catch => self.catching = self.catching.saturating_sub(1),
            _ => {}
        }
    }

    /// Drop every pending frame, restoring bindings (a run stopped from outside: its future was dropped).
    pub fn abort(&mut self) {
        while let Some(fr) = self.frames.pop() {
            self.discard(fr);
        }
        self.catching = 0;
        self.in_error = false;
    }

    fn restore_all(&mut self, saved: Vec<(Sym, Option<V>)>) {
        for (s, v) in saved {
            self.restore(s, v);
        }
    }

    /// A step: every 4096 the stop flag and the budget are checked.
    fn tick(&mut self) -> Option<LErr> {
        self.steps = self.steps.wrapping_add(1);
        self.run_steps = self.run_steps.saturating_add(1);
        if self.steps & 4095 == 0 {
            self.yield_due = true;
            if self.stopped.get() {
                return Some(LErr::cancel());
            }
            if self.limits.steps.is_some_and(|m| self.run_steps > m) {
                return Some(LErr::cancel());
            }
        }
        None
    }

    /// Steps taken by the current run.
    pub fn steps_taken(&self) -> u64 {
        self.run_steps
    }

    fn eval(&mut self, x: V) -> Ctl {
        let c = match x {
            V::Sym(s) => return Ctl::Ret(self.get(s)),
            V::Cons(c) => c,
            other => return Ctl::Ret(other),
        };
        if let Some(e) = self.tick() {
            return Ctl::Throw(e);
        }
        match &c.car {
            V::Sym(h) => {
                let h = *h;
                let f = match self.raw(h) {
                    Some(f) => f,
                    None => match self.auto(h) {
                        Ok(f) => f,
                        Err(e) => return Ctl::Throw(e),
                    },
                };
                if let V::Fun(fr) = &f
                    && let Fun::Builtin(b) = fr.as_ref()
                    && b.special
                    && let Imp::F(k) = b.imp
                    && let Some((_, Bf::Special(sf))) = self.table.get(usize::from(k))
                {
                    let sf = *sf;
                    return sf(self, c.cdr.clone());
                }
                self.args(f, c.car.clone(), c.cdr.clone(), Vec::new())
            }
            V::Cons(_) => {
                self.frames.push(Frame::Head { rest: c.cdr.clone(), head: c.car.clone() });
                Ctl::Eval(c.car.clone())
            }
            other => Ctl::Throw(LErr::new(format!("bad function: {}", self.show(other, true)))),
        }
    }

    /// Evaluate the arguments left in `rest` (symbols and atoms at once, lists through frames), then apply.
    fn args(&mut self, f: V, head: V, mut rest: V, mut args: Vec<V>) -> Ctl {
        loop {
            let next = match &rest {
                V::Cons(c) => (c.car.clone(), c.cdr.clone()),
                _ => return self.apply(f, args, Some(head)),
            };
            rest = next.1;
            match next.0 {
                V::Sym(s) => args.push(self.get(s)),
                V::Cons(_) => {
                    let form = next.0;
                    self.frames.push(Frame::Args { f, head, rest, args });
                    return Ctl::Eval(form);
                }
                a => args.push(a),
            }
        }
    }

    /// Call a function with these arguments (`head`: the form's head, for "bad function").
    pub fn apply(&mut self, f: V, args: Vec<V>, head: Option<V>) -> Ctl {
        let f = if fun(&f).is_some() {
            f
        } else {
            if !matches!(f, V::Sym(_) | V::Cons(_)) {
                let shown = head.unwrap_or(f);
                return Ctl::Throw(LErr::new(format!("bad function: {}", self.show(&shown, true))));
            }
            match self.fn_of(&f) {
                Ok(g) => g,
                Err(e) => return Ctl::Throw(e),
            }
        };
        let Some(fr) = fun(&f) else { return Ctl::Throw(LErr::new("bad function")) };
        match fr {
            Fun::Builtin(b) => {
                if b.special {
                    return Ctl::Throw(LErr::new(format!("bad function: {}", b.name)));
                }
                match b.imp {
                    Imp::F(k) => match self.table.get(usize::from(k)).map(|x| x.1) {
                        Some(Bf::Sync(sf)) => match sf(self, &args) {
                            Ok(v) => Ctl::Ret(v),
                            Err(e) => Ctl::Throw(e),
                        },
                        Some(Bf::Ctl(cf)) => cf(self, args),
                        _ => Ctl::Throw(LErr::new(format!("bad function: {}", b.name))),
                    },
                    Imp::VlaGet | Imp::VlaPut | Imp::VlaMethod => {
                        let name = b.name.clone();
                        crate::vla::call_auto(self, b.imp, &name, args)
                    }
                }
            }
            Fun::Lambda(lf) => {
                if args.len() < lf.params.len() {
                    return Ctl::Throw(LErr::new("too few arguments"));
                }
                if args.len() > lf.params.len() {
                    return Ctl::Throw(LErr::new("too many arguments"));
                }
                let mut saved = Vec::with_capacity(lf.params.len().saturating_add(lf.locals.len()));
                for s in lf.params.iter().chain(lf.locals.iter()) {
                    saved.push((*s, self.raw(*s)));
                }
                for (s, a) in lf.params.iter().zip(args) {
                    self.set(*s, a);
                }
                for s in &lf.locals {
                    self.set(*s, V::Nil);
                }
                let body = Rc::clone(&lf.body);
                self.frames.push(Frame::Lambda { saved });
                self.seq(body)
            }
        }
    }

    /// Evaluate a body: the last form's value (nil for none).
    pub fn seq(&mut self, body: Rc<[V]>) -> Ctl {
        match body.len() {
            0 => Ctl::Ret(V::Nil),
            1 => Ctl::Eval(body.first().cloned().unwrap_or(V::Nil)),
            _ => {
                let first = body.first().cloned().unwrap_or(V::Nil);
                self.frames.push(Frame::Seq { body, i: 1 });
                Ctl::Eval(first)
            }
        }
    }

    /// Push a continuation for the value `next` returns.
    pub fn then(&mut self, k: impl FnOnce(&mut Lisp, V) -> Ctl + 'static, next: Ctl) -> Ctl {
        self.frames.push(Frame::Then(Box::new(k)));
        next
    }
    /// Push a continuation (frames pushed after it run first).
    pub fn frames_push_then(&mut self, k: impl FnOnce(&mut Lisp, V) -> Ctl + 'static) {
        self.frames.push(Frame::Then(Box::new(k)));
    }
    /// Evaluate a body, then continue with its value.
    pub fn seq_then(&mut self, body: Rc<[V]>, k: impl FnOnce(&mut Lisp, V) -> Ctl + 'static) -> Ctl {
        self.frames.push(Frame::Then(Box::new(k)));
        self.seq(body)
    }
    /// Push a clean-up that runs on return and on error.
    pub fn finally(&mut self, f: impl FnOnce(&mut Lisp) + 'static) {
        self.frames.push(Frame::Finally(Box::new(f)));
    }
    /// Bindings to put back when what follows returns or fails.
    pub fn push_restore(&mut self, saved: Vec<(Sym, Option<V>)>) {
        self.frames.push(Frame::Restore { saved });
    }
    /// vl-catch-all-apply's frame (catching while it is there).
    pub fn push_catch(&mut self) {
        self.catching = self.catching.saturating_add(1);
        self.frames.push(Frame::Catch);
    }

    fn resume(&mut self, fr: Frame, v: V) -> Ctl {
        match fr {
            Frame::Top => Ctl::Ret(v),
            Frame::Args { f, head, rest, mut args } => {
                args.push(v);
                self.args(f, head, rest, args)
            }
            Frame::Head { rest, head } => {
                // a computed function that is a special form still gets the rest unevaluated
                if let V::Fun(fr) = &v
                    && let Fun::Builtin(b) = fr.as_ref()
                    && b.special
                    && let Imp::F(k) = b.imp
                    && let Some((_, Bf::Special(sf))) = self.table.get(usize::from(k))
                {
                    let sf = *sf;
                    return sf(self, rest);
                }
                self.args(v, head, rest, Vec::new())
            }
            Frame::Seq { body, i } => {
                let Some(x) = body.get(i).cloned() else { return Ctl::Ret(v) };
                if i.saturating_add(1) < body.len() {
                    self.frames.push(Frame::Seq { body, i: i.saturating_add(1) });
                }
                Ctl::Eval(x)
            }
            Frame::Lambda { saved } | Frame::Restore { saved } => {
                self.restore_all(saved);
                Ctl::Ret(v)
            }
            Frame::Handler { err, saved } => {
                self.in_error = false;
                if let Some(s) = saved {
                    self.restore_all(s);
                }
                Ctl::Throw(err)
            }
            Frame::Setq { sym, rest } => {
                self.set(sym, v.clone());
                self.setq(rest, v)
            }
            Frame::If { then, els } => {
                if v.truthy() {
                    Ctl::Eval(then)
                } else {
                    match els {
                        Some(e) => Ctl::Eval(e),
                        None => Ctl::Ret(V::Nil),
                    }
                }
            }
            Frame::Cond { body, rest } => {
                if v.truthy() {
                    if body.is_nil() {
                        return Ctl::Ret(v);
                    }
                    return self.seq(to_vec(&body).into());
                }
                self.cond(rest)
            }
            Frame::While { test, body, last, testing } => {
                if testing {
                    if !v.truthy() {
                        return Ctl::Ret(last);
                    }
                    self.frames.push(Frame::While { test, body: Rc::clone(&body), last, testing: false });
                    self.seq(body)
                } else {
                    if let Some(e) = self.tick() {
                        return Ctl::Throw(e);
                    }
                    self.frames.push(Frame::While { test: test.clone(), body, last: v, testing: true });
                    Ctl::Eval(test)
                }
            }
            Frame::Repeat { left, body, last, started } => {
                let last = if started {
                    if let Some(e) = self.tick() {
                        return Ctl::Throw(e);
                    }
                    v
                } else {
                    last
                };
                if left <= 0 {
                    return Ctl::Ret(last);
                }
                self.frames.push(Frame::Repeat { left: left.saturating_sub(1), body: Rc::clone(&body), last: V::Nil, started: true });
                self.seq(body)
            }
            Frame::Foreach { var, items, body, last } => {
                let _ = last;
                self.foreach_next(var, items, body, v)
            }
            Frame::And { rest } => {
                if !v.truthy() {
                    return Ctl::Ret(V::Nil);
                }
                self.and(rest)
            }
            Frame::Or { rest } => {
                if v.truthy() {
                    return Ctl::Ret(V::Sym(S_T));
                }
                self.or(rest)
            }
            Frame::Then(k) => k(self, v),
            Frame::Finally(f) => {
                f(self);
                Ctl::Ret(v)
            }
            Frame::Catch => {
                self.catching = self.catching.saturating_sub(1);
                Ctl::Ret(v)
            }
        }
    }

    fn unwind(&mut self, fr: Frame, mut e: LErr) -> Ctl {
        match fr {
            Frame::Lambda { saved } => {
                if !e.handled && self.catching == 0 {
                    return self.handle_error(e, Some(saved));
                }
                self.restore_all(saved);
                Ctl::Throw(e)
            }
            Frame::Top => {
                if !e.handled {
                    // the bottom: handled here, then the run ends with the error
                    self.frames.push(Frame::Top);
                    let c = self.handle_error(e, None);
                    return c;
                }
                // reached the bottom once more after the handler: stop (Top was re-pushed by handle_error's caller)
                Ctl::Throw(e)
            }
            Frame::Handler { err, saved } => {
                if e.msg != err.msg {
                    self.print(&format!("; error: {}", e.msg));
                }
                self.in_error = false;
                if let Some(s) = saved {
                    self.restore_all(s);
                }
                Ctl::Throw(err)
            }
            Frame::Restore { saved } => {
                self.restore_all(saved);
                Ctl::Throw(e)
            }
            Frame::Finally(f) => {
                f(self);
                Ctl::Throw(e)
            }
            Frame::Catch => {
                self.catching = self.catching.saturating_sub(1);
                if e.cancel {
                    return Ctl::Throw(e);
                }
                e.handled = true;
                Ctl::Ret(V::CatchErr(e.msg.into()))
            }
            _ => Ctl::Throw(e),
        }
    }

    /// handleError: the note, then *error* with the message (or "; error: …" when there is none, or while one runs).
    fn handle_error(&mut self, mut e: LErr, saved: Option<Vec<(Sym, Option<V>)>>) -> Ctl {
        e.handled = true;
        if let Some(n) = e.note.take() {
            self.print(&n);
        }
        let h = self.raw(S_ERROR);
        let callable = matches!(&h, Some(V::Fun(_) | V::Cons(_)));
        if self.in_error || !callable {
            self.print(&format!("; error: {}", e.msg));
            if let Some(s) = saved {
                self.restore_all(s);
            }
            return Ctl::Throw(e);
        }
        self.in_error = true;
        let msg = V::Str(lstr(&e.msg));
        self.frames.push(Frame::Handler { err: e, saved });
        self.apply(h.unwrap_or(V::Nil), vec![msg], None)
    }

    // ---- special forms' frames ----

    fn setq(&mut self, l: V, last: V) -> Ctl {
        let V::Cons(c) = &l else { return Ctl::Ret(last) };
        let V::Sym(s) = c.car else { return Ctl::Throw(self.bad("symbolp", &c.car)) };
        let V::Cons(vc) = &c.cdr else { return Ctl::Throw(LErr::new("too few arguments")) };
        self.frames.push(Frame::Setq { sym: s, rest: vc.cdr.clone() });
        Ctl::Eval(vc.car.clone())
    }
    fn cond(&mut self, mut rest: Vec<V>) -> Ctl {
        if rest.is_empty() {
            return Ctl::Ret(V::Nil);
        }
        let clause = rest.remove(0);
        let V::Cons(c) = &clause else { return Ctl::Throw(self.bad("consp", &clause)) };
        self.frames.push(Frame::Cond { body: c.cdr.clone(), rest });
        Ctl::Eval(c.car.clone())
    }
    fn and(&mut self, rest: V) -> Ctl {
        match &rest {
            V::Cons(c) => {
                self.frames.push(Frame::And { rest: c.cdr.clone() });
                Ctl::Eval(c.car.clone())
            }
            _ => Ctl::Ret(V::Sym(S_T)),
        }
    }
    fn or(&mut self, rest: V) -> Ctl {
        match &rest {
            V::Cons(c) => {
                self.frames.push(Frame::Or { rest: c.cdr.clone() });
                Ctl::Eval(c.car.clone())
            }
            _ => Ctl::Ret(V::Nil),
        }
    }
    fn foreach_next(&mut self, var: Sym, items: V, body: Rc<[V]>, last: V) -> Ctl {
        match &items {
            V::Cons(c) => {
                self.set(var, c.car.clone());
                self.frames.push(Frame::Foreach { var, items: c.cdr.clone(), body: Rc::clone(&body), last: V::Nil });
                self.seq(body)
            }
            _ => Ctl::Ret(last),
        }
    }

    // ---- printing ----

    /// Any value as text; `readable` = prin1 (strings quoted), else princ.
    pub fn show(&self, x: &V, readable: bool) -> String {
        let mut out = String::new();
        self.show_into(&mut out, x, readable, 0);
        out
    }

    fn show_into(&self, out: &mut String, x: &V, readable: bool, depth: u32) {
        if depth > 200 {
            out.push_str("...");
            return;
        }
        match x {
            V::Nil => out.push_str("nil"),
            V::Real(r) => out.push_str(&real_text(*r)),
            V::Int(i) => out.push_str(&i.to_string()),
            V::Str(s) => {
                if readable {
                    out.push('"');
                    for c in char::decode_utf16(s.iter().copied()) {
                        match c {
                            Ok('\\') => out.push_str("\\\\"),
                            Ok('"') => out.push_str("\\\""),
                            Ok('\n') => out.push_str("\\n"),
                            Ok('\r') => out.push_str("\\r"),
                            Ok('\t') => out.push_str("\\t"),
                            Ok('\u{1b}') => out.push_str("\\e"),
                            Ok(c) => out.push(c),
                            Err(_) => out.push('\u{FFFD}'),
                        }
                    }
                    out.push('"');
                } else {
                    out.push_str(&rstr(s));
                }
            }
            V::Sym(s) => out.push_str(self.name(*s)),
            V::Cons(c) => {
                if matches!(c.car, V::Sym(S_QUOTE))
                    && let V::Cons(c2) = &c.cdr
                    && c2.cdr.is_nil()
                {
                    out.push('\'');
                    self.show_into(out, &c2.car, readable, depth.saturating_add(1));
                    return;
                }
                out.push('(');
                let mut l: &V = x;
                let mut n = 0u32;
                let mut first = true;
                let mut cut = false;
                while let V::Cons(cc) = l {
                    if !first {
                        out.push(' ');
                    }
                    first = false;
                    self.show_into(out, &cc.car, readable, depth.saturating_add(1));
                    l = &cc.cdr;
                    n = n.saturating_add(1);
                    if n > 100_000 {
                        out.push_str(" ...");
                        cut = true;
                        break;
                    }
                }
                if !cut && !l.is_nil() {
                    out.push_str(" . ");
                    self.show_into(out, l, readable, depth.saturating_add(1));
                }
                out.push(')');
            }
            V::Ename(id) => {
                out.push_str("<Entity name: ");
                match id.as_ref() {
                    EId::N(n) => {
                        // (0x7ff700000000 + id * 16).toString(16), in doubles as the JS
                        #[allow(clippy::cast_precision_loss)]
                        let v = (0x7ff7_0000_0000_u64 as f64) + (*n as f64) * 16.0;
                        out.push_str(&hex_of(v));
                    }
                    EId::S(s) => out.push_str(s),
                }
                out.push('>');
            }
            V::PickSet(p) => out.push_str(&format!("<Selection set: {}>", p.n)),
            V::Fun(f) => match f.as_ref() {
                Fun::Lambda(l) => out.push_str(&format!("#<USUBR @{}>", l.name.as_deref().unwrap_or("lambda"))),
                Fun::Builtin(b) => out.push_str(&format!("#<SUBR @{}>", b.name)),
            },
            V::File(f) => out.push_str(&format!("#<file \"{}\">", f.try_borrow().map(|f| rstr(&f.path)).unwrap_or_default())),
            V::Vla(h) => out.push_str(&crate::vla::show_vla(h)),
            V::Variant(v) => {
                out.push_str(&format!("#<variant {} ", v.vt));
                if matches!(v.value, V::SafeArray(_)) {
                    out.push_str("...");
                } else {
                    self.show_into(out, &v.value, true, 0);
                }
                out.push('>');
            }
            V::SafeArray(_) => out.push_str("#<safearray...>"),
            V::CatchErr(_) => out.push_str("#<%catch-all-apply-error%>"),
        }
    }

    /// AutoLISP's (type x) name.
    pub fn type_name(x: &V) -> Option<&'static str> {
        Some(match x {
            V::Nil => return None,
            V::Int(_) => "INT",
            V::Real(_) => "REAL",
            V::Str(_) => "STR",
            V::Sym(_) => "SYM",
            V::Cons(_) => "LIST",
            V::Ename(_) => "ENAME",
            V::PickSet(_) => "PICKSET",
            V::Fun(f) => match f.as_ref() {
                Fun::Lambda(_) => "USUBR",
                Fun::Builtin(_) => "SUBR",
            },
            V::File(_) => "FILE",
            V::Vla(_) => "VLA-OBJECT",
            V::Variant(_) => "VARIANT",
            V::SafeArray(_) => "SAFEARRAY",
            V::CatchErr(_) => "VL-CATCH-ALL-APPLY-ERROR",
        })
    }

    // ---- in-memory files (load, findfile, open, load_dialog) ----

    pub fn norm(p: &[u16]) -> String {
        rstr(p).replace('\\', "/").to_lowercase()
    }
    /// Keep a file (a name kept already keeps its place).
    pub fn put_file(&mut self, name: &[u16], text: &[u16]) {
        let k = Lisp::norm(name);
        let f = MemFile { name: name.into(), text: text.into() };
        match self.files.iter_mut().find(|(n, _)| *n == k) {
            Some(slot) => slot.1 = f,
            None => self.files.push((k, f)),
        }
    }
    /// A file by path or by its name alone (any folder), case-insensitive.
    pub fn find_file(&self, name: &[u16]) -> Option<MemFile> {
        let n = Lisp::norm(name);
        if let Some((_, f)) = self.files.iter().find(|(k, _)| *k == n) {
            return Some(f.clone());
        }
        let base = n.rsplit('/').next().unwrap_or("").to_string();
        self.files.iter().find(|(k, _)| k.rsplit('/').next().unwrap_or("") == base).map(|(_, f)| f.clone())
    }
    pub fn delete_file(&mut self, name: &[u16]) -> bool {
        let n = Lisp::norm(name);
        let before = self.files.len();
        self.files.retain(|(k, _)| *k != n);
        self.files.len() != before
    }

    // ---- the entry points ----

    /// Read and evaluate a file's text; reader errors are reported and nothing of the file runs.
    pub async fn load_text(&mut self, text: &[u16], name: &str) -> Outcome {
        match read_all(text, &mut self.syms) {
            Ok(forms) => self.top(forms).await,
            Err(e) => {
                self.print(&format!("; error: {} ({name})", e.msg));
                Outcome { ok: false, value: V::Nil, error: Some(e.msg), cancel: false }
            }
        }
    }
    /// The command line's "(…" and "!var".
    pub async fn eval_text(&mut self, text: &[u16]) -> Outcome {
        match read_all(text, &mut self.syms) {
            Ok(forms) => self.top(forms).await,
            Err(e) => {
                self.print(&format!("; error: {}", e.msg));
                Outcome { ok: false, value: V::Nil, error: Some(e.msg), cancel: false }
            }
        }
    }
    /// Run a (defun c:NAME …) command: the function called with no arguments, as typing its name does.
    pub async fn call_command(&mut self, name: &str) -> Outcome {
        let full = if name.to_uppercase().starts_with("C:") { name.to_string() } else { format!("C:{name}") };
        let s = match self.sym(&full) {
            Ok(s) => s,
            Err(e) => return Outcome { ok: false, value: V::Nil, error: Some(e.msg), cancel: false },
        };
        let f = self.raw(s);
        if !matches!(f, Some(V::Fun(_) | V::Cons(_))) {
            return Outcome { ok: false, value: V::Nil, error: Some(format!("no function definition: {}", self.name(s))), cancel: false };
        }
        self.top(vec![V::cons(V::Sym(s), V::Nil)]).await
    }
    /// The commands defined so far (C:NAME → NAME), in the order bound.
    pub fn commands(&self) -> Vec<String> {
        self.bound_symbols()
            .into_iter()
            .filter_map(|s| {
                let n = self.name(s);
                let is_fn = matches!(self.raw(s), Some(V::Fun(ref f)) if matches!(f.as_ref(), Fun::Lambda(_)))
                    || matches!(self.raw(s), Some(V::Cons(_)));
                (n.starts_with("C:") && n.len() > 2 && is_fn).then(|| n.get(2..).unwrap_or("").to_string())
            })
            .collect()
    }

    /// A new selection set's serial number (one count for the whole process, as 0.5's `PickSet.count` is one per page).
    pub fn next_pickset(&self) -> u64 {
        PICKSETS.fetch_add(1, std::sync::atomic::Ordering::Relaxed).saturating_add(1)
    }
    /// vl-filename-mktemp's next number (one count per process, as 0.5's module counter).
    pub fn next_temp(&self) -> u32 {
        TEMPS.fetch_add(1, std::sync::atomic::Ordering::Relaxed).saturating_add(1)
    }
    /// A selection set of these objects.
    pub fn pickset(&self, ids: Vec<EId>) -> V {
        V::PickSet(Rc::new(PickSet { ids: RefCell::new(ids), n: self.next_pickset() }))
    }
    /// A string value, refused past the string cap ("Invalid string length", V8's RangeError).
    pub fn mk_str(&self, units: Vec<u16>) -> R<V> {
        if units.len() > self.limits.string {
            return Err(LErr::new("Invalid string length"));
        }
        Ok(V::Str(units.into()))
    }
    /// An in-memory file handle.
    pub fn file(path: LStr, mode: u8, buf: Vec<u16>) -> V {
        V::File(Rc::new(RefCell::new(LFile { path, mode, pos: 0, open: true, buf })))
    }
    /// A built-in's name by index (error messages).
    pub fn builtin_name(&self, k: u16) -> &str {
        self.table.get(usize::from(k)).map_or("", |x| x.0.as_str())
    }
    /// The cancel message (for drawing-side errors that are Esc).
    pub fn cancel_msg() -> &'static str {
        MSG_CANCEL
    }
    /// Special forms need these: evaluate `body` forms with `v` bound … (exposed to the built-in modules)
    pub(crate) fn push_frame_foreach(&mut self, var: Sym, items: V, body: Rc<[V]>) -> Ctl {
        let old = self.raw(var);
        self.push_restore(vec![(var, old)]);
        self.foreach_next(var, items, body, V::Nil)
    }
    pub(crate) fn push_frame_while(&mut self, test: V, body: Rc<[V]>) -> Ctl {
        self.frames.push(Frame::While { test: test.clone(), body, last: V::Nil, testing: true });
        Ctl::Eval(test)
    }
    pub(crate) fn push_frame_repeat(&mut self, n: i64, body: Rc<[V]>) -> Ctl {
        self.resume(Frame::Repeat { left: n, body, last: V::Nil, started: false }, V::Nil)
    }
    pub(crate) fn push_frame_if(&mut self, test: V, then: V, els: Option<V>) -> Ctl {
        self.frames.push(Frame::If { then, els });
        Ctl::Eval(test)
    }
    pub(crate) fn push_frame_cond(&mut self, clauses: Vec<V>) -> Ctl {
        self.cond(clauses)
    }
    pub(crate) fn push_frame_and(&mut self, rest: V) -> Ctl {
        self.and(rest)
    }
    pub(crate) fn push_frame_or(&mut self, rest: V) -> Ctl {
        self.or(rest)
    }
    pub(crate) fn push_frame_setq(&mut self, l: V) -> Ctl {
        self.setq(l, V::Nil)
    }
    pub(crate) fn defun_hook(&self, name: &str) {
        if let Some(f) = &self.on_defun {
            f(name);
        }
    }
    /// Evaluate a form as a step of a built-in (the value comes back through `then`).
    pub fn eval_then(&mut self, form: V, k: impl FnOnce(&mut Lisp, V) -> Ctl + 'static) -> Ctl {
        self.then(k, Ctl::Eval(form))
    }
    /// Apply a function as a step of a built-in.
    pub fn apply_then(&mut self, f: V, args: Vec<V>, k: impl FnOnce(&mut Lisp, V) -> Ctl + 'static) -> Ctl {
        self.frames.push(Frame::Then(Box::new(k)));
        self.apply(f, args, None)
    }
    /// The "quit / exit abort" message.
    pub fn quit_msg() -> &'static str {
        MSG_QUIT
    }
    /// A list of values.
    pub fn list(xs: Vec<V>) -> V {
        from_vec(xs, V::Nil)
    }
}

static PICKSETS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static TEMPS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// A non-negative integral double in hex (`Number.prototype.toString(16)`).
fn hex_of(v: f64) -> String {
    if v.is_finite() && (0.0..9_007_199_254_740_992.0).contains(&v) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let u = v as u64;
        return format!("{u:x}");
    }
    cc_cad::js::num_to_string(v)
}
