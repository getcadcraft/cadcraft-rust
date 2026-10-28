//! The values of CadCraft's LISP (types.js): nil, integers (32-bit) and reals, strings, symbols, lists, entity names,
//! selection sets, functions, files, VLA objects, variants and safe arrays, and the error a catch-all apply returns.
//!
//! Strings are JS strings: UTF-16 code units ([`LStr`]), so `strlen`, `substr`, `ascii`, `vl-string-search` and the
//! reader count as 0.5 counts (a character outside the BMP is two). Lists are immutable cons cells shared by `Rc`;
//! every container drops its children through one work list ([`drop_later`]), so a list a million long (or a million
//! deep) never drops recursively.

use std::cell::RefCell;
use std::rc::Rc;

use cc_cad::js;

/// A string: UTF-16 code units, as a JS string.
pub type LStr = Rc<[u16]>;

/// `s` as a LISP string.
pub fn lstr(s: &str) -> LStr {
    s.encode_utf16().collect::<Vec<u16>>().into()
}
/// A LISP string as Rust text (an unpaired surrogate becomes U+FFFD).
pub fn rstr(s: &[u16]) -> String {
    String::from_utf16_lossy(s)
}

/// An interned symbol: its index in the interpreter's symbol table (names are upper case, as AutoLISP).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Sym(pub u32);

/// An entity name's id: a number (an object of the model or a sheet) or a string (`b:<block>:<i>` a block member,
/// `a:<insert>:<tag>` an attribute, `s:<insert>` its SEQEND, `T:<table>:<name>` a table record).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum EId {
    N(u64),
    S(Rc<str>),
}

impl EId {
    /// The number, when it is one.
    pub fn num(&self) -> Option<u64> {
        if let EId::N(n) = self { Some(*n) } else { None }
    }
    /// The string id, when it is one.
    pub fn text(&self) -> Option<&str> {
        if let EId::S(s) = self { Some(s) } else { None }
    }
}

/// A VLA object's handle (vla-host.js `{ kind, key, tag }`).
#[derive(Clone, Debug, PartialEq)]
pub struct VlaH {
    pub kind: String,
    pub key: Option<VKey>,
    pub tag: Option<String>,
}

/// A handle's key: a name (string) or an object id (number).
#[derive(Clone, Debug, PartialEq)]
pub enum VKey {
    Num(f64),
    Str(String),
}

impl VKey {
    /// `String(key)`.
    pub fn text(&self) -> String {
        match self {
            VKey::Num(n) => js::num_to_string(*n),
            VKey::Str(s) => s.clone(),
        }
    }
}

/// A selection set: its objects (ssadd / ssdel change it in place) and its serial number.
#[derive(Debug)]
pub struct PickSet {
    pub ids: RefCell<Vec<EId>>,
    pub n: u64,
}

/// A function: one made by defun / lambda, or a built-in.
#[derive(Debug)]
pub enum Fun {
    Lambda(Lambda),
    Builtin(Builtin),
}

/// A function made by defun or lambda: params, locals (after "/"), body.
#[derive(Debug)]
pub struct Lambda {
    pub name: Option<String>,
    pub params: Vec<Sym>,
    pub locals: Vec<Sym>,
    pub body: Rc<[V]>,
}

/// A built-in: which one, its name, and whether it takes its arguments unevaluated (a special form).
#[derive(Debug)]
pub struct Builtin {
    pub name: String,
    pub imp: Imp,
    pub special: bool,
}

/// What a built-in runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Imp {
    /// one of the built-in functions (index into the interpreter's table)
    F(u16),
    /// `vla-get-<property>` made on first use
    VlaGet,
    /// `vla-put-<property>`
    VlaPut,
    /// `vla-<method>`
    VlaMethod,
}

/// An open file (open / read-line / write-line / close) on the interpreter's in-memory files.
#[derive(Debug)]
pub struct LFile {
    pub path: LStr,
    pub mode: u8,
    pub pos: usize,
    pub open: bool,
    pub buf: Vec<u16>,
}

/// A variant (vla.js `Variant`): a type number and a value.
#[derive(Debug)]
pub struct Variant {
    pub vt: i32,
    pub value: V,
}

/// A safe array: element type, bounds per dimension, the elements in row order.
#[derive(Debug)]
pub struct SafeArray {
    pub vt: i32,
    pub bounds: Vec<(i32, i32)>,
    pub data: Vec<V>,
}

/// One cons cell.
#[derive(Debug)]
pub struct Cons {
    pub car: V,
    pub cdr: V,
}

/// A LISP value.
#[derive(Clone, Debug, Default)]
pub enum V {
    /// nil: the empty list and false
    #[default]
    Nil,
    Int(i32),
    Real(f64),
    Str(LStr),
    Sym(Sym),
    Cons(Rc<Cons>),
    Ename(Rc<EId>),
    PickSet(Rc<PickSet>),
    Fun(Rc<Fun>),
    File(Rc<RefCell<LFile>>),
    Vla(Rc<VlaH>),
    Variant(Rc<Variant>),
    SafeArray(Rc<RefCell<SafeArray>>),
    /// what vl-catch-all-apply returns when the call failed: the message
    CatchErr(Rc<str>),
}

impl V {
    pub fn is_nil(&self) -> bool {
        matches!(self, V::Nil)
    }
    /// truthy (anything but nil)
    pub fn truthy(&self) -> bool {
        !self.is_nil()
    }
    pub fn is_num(&self) -> bool {
        matches!(self, V::Int(_) | V::Real(_))
    }
    /// A number's value (an integer as a real).
    pub fn num(&self) -> Option<f64> {
        match self {
            V::Int(i) => Some(f64::from(*i)),
            V::Real(r) => Some(*r),
            _ => None,
        }
    }
    pub fn is_list(&self) -> bool {
        matches!(self, V::Nil | V::Cons(_))
    }
    pub fn cons(a: V, b: V) -> V {
        V::Cons(Rc::new(Cons { car: a, cdr: b }))
    }
    pub fn str(s: &str) -> V {
        V::Str(lstr(s))
    }
    pub fn car(&self) -> Option<&V> {
        if let V::Cons(c) = self { Some(&c.car) } else { None }
    }
    pub fn cdr(&self) -> Option<&V> {
        if let V::Cons(c) = self { Some(&c.cdr) } else { None }
    }
}

/// A list of these values (`list(...xs)` / `fromArray(xs, tail)`).
pub fn from_vec(xs: Vec<V>, tail: V) -> V {
    let mut r = tail;
    for x in xs.into_iter().rev() {
        r = V::cons(x, r);
    }
    r
}
/// A proper list as values (a dotted tail is dropped).
pub fn to_vec(l: &V) -> Vec<V> {
    let mut out = Vec::new();
    let mut x = l;
    while let V::Cons(c) = x {
        out.push(c.car.clone());
        x = &c.cdr;
    }
    out
}
/// Does the list end in nil?
pub fn is_proper(l: &V) -> bool {
    let mut x = l;
    while let V::Cons(c) = x {
        x = &c.cdr;
    }
    x.is_nil()
}
/// The number of cells (a dotted tail not counted).
pub fn list_len(l: &V) -> usize {
    let mut n = 0usize;
    let mut x = l;
    while let V::Cons(c) = x {
        n = n.saturating_add(1);
        x = &c.cdr;
    }
    n
}

// ---- dropping without recursion ----

thread_local! {
    /// The values whose children are being dropped (Some while a drop loop runs on this thread).
    static DROPS: RefCell<Option<Vec<V>>> = const { RefCell::new(None) };
}

/// Drop these children one by one from a work list: the first container to drop runs the loop, the ones it frees
/// add their children to it.
fn drop_later(kids: [V; 2]) {
    let [a, b] = kids;
    if !needs_queue(&a) && !needs_queue(&b) {
        return;
    }
    let active = DROPS.with(|d| d.try_borrow().map_or(true, |q| q.is_some()));
    if active {
        DROPS.with(|d| {
            if let Ok(mut q) = d.try_borrow_mut()
                && let Some(list) = q.as_mut()
            {
                list.push(a);
                list.push(b);
            }
        });
        return;
    }
    DROPS.with(|d| {
        if let Ok(mut q) = d.try_borrow_mut() {
            *q = Some(Vec::new());
        }
    });
    let mut next = vec![a, b];
    loop {
        while let Some(v) = next.pop() {
            drop(v);
        }
        let more = DROPS.with(|d| d.try_borrow_mut().ok().and_then(|mut q| q.as_mut().map(std::mem::take)));
        match more {
            Some(list) if !list.is_empty() => next = list,
            _ => break,
        }
    }
    DROPS.with(|d| {
        if let Ok(mut q) = d.try_borrow_mut() {
            *q = None;
        }
    });
}

/// Is this a container that may hold more containers?
fn needs_queue(v: &V) -> bool {
    matches!(v, V::Cons(_) | V::Variant(_) | V::SafeArray(_) | V::Fun(_))
}

impl Drop for Cons {
    fn drop(&mut self) {
        if !needs_queue(&self.car) && !needs_queue(&self.cdr) {
            return;
        }
        drop_later([std::mem::take(&mut self.car), std::mem::take(&mut self.cdr)]);
    }
}
impl Drop for Variant {
    fn drop(&mut self) {
        if needs_queue(&self.value) {
            drop_later([std::mem::take(&mut self.value), V::Nil]);
        }
    }
}
impl Drop for SafeArray {
    fn drop(&mut self) {
        let data = std::mem::take(&mut self.data);
        for v in data {
            if needs_queue(&v) {
                drop_later([v, V::Nil]);
            }
        }
    }
}

// ---- errors ----

/// A LISP error: AutoLISP's own wording (routines compare the messages).
#[derive(Clone, Debug, PartialEq)]
pub struct LErr {
    pub msg: String,
    /// Esc / a cancelled prompt ("Function cancelled")
    pub cancel: bool,
    /// already handed to *error* (or printed)
    pub handled: bool,
    /// a line printed before the error ("not built yet" for a function 0.5 lacks)
    pub note: Option<String>,
    /// the reader ran out of text in a list or string (the command line keeps reading)
    pub incomplete: bool,
}

impl LErr {
    pub fn new(msg: impl Into<String>) -> LErr {
        LErr { msg: msg.into(), cancel: false, handled: false, note: None, incomplete: false }
    }
    pub fn cancel() -> LErr {
        LErr { cancel: true, ..LErr::new(MSG_CANCEL) }
    }
    pub fn incomplete(msg: &str) -> LErr {
        LErr { incomplete: true, ..LErr::new(msg) }
    }
}

/// (exit) / (quit) and a cancelled prompt: *error* hears these messages as AutoLISP's own.
pub const MSG_CANCEL: &str = "Function cancelled";
pub const MSG_QUIT: &str = "quit / exit abort";
/// What V8 says when a recursion has no end (fnOf of a symbol bound to itself; a depth the stack cannot take).
pub const MSG_STACK: &str = "Maximum call stack size exceeded";

pub type R<T = V> = Result<T, LErr>;

// ---- numbers ----

/// `v | 0` (ToInt32).
pub fn to_i32(v: f64) -> i32 {
    let w = js::int32(v);
    // int32 is in [-2^31, 2^31)
    #[allow(clippy::cast_possible_truncation)]
    let i = w as i32;
    i
}

/// A real as AutoLISP prints it: six significant digits, always with a decimal point ("3.0", "1.12346", "1.0e+007").
pub fn real_text(x: f64) -> String {
    if !x.is_finite() {
        return if x.is_nan() {
            "-1.#IND".into()
        } else if x > 0.0 {
            "1.#INF".into()
        } else {
            "-1.#INF".into()
        };
    }
    if x == 0.0 {
        return "0.0".into();
    }
    let e5 = js::to_exponential(x, 5);
    let (m0, e0) = e5.split_once('e').unwrap_or((e5.as_str(), "0"));
    let ex: i64 = e0.parse().unwrap_or(0);
    if !(-4..6).contains(&ex) {
        let m = m0.trim_end_matches('0');
        let m = if let Some(stripped) = m.strip_suffix('.') { format!("{stripped}.0") } else { m.to_string() };
        let mag = ex.unsigned_abs();
        return format!("{m}e{}{:03}", if ex < 0 { '-' } else { '+' }, mag);
    }
    let places = usize::try_from(5i64.saturating_sub(ex).max(0)).unwrap_or(0);
    let mut s = js::to_fixed(x, places);
    if s.contains('.') {
        let t = s.trim_end_matches('0');
        s = if let Some(stripped) = t.strip_suffix('.') { format!("{stripped}.0") } else { t.to_string() };
    } else {
        s.push_str(".0");
    }
    s
}

/// `Number.prototype.toString` of a number.
pub fn num_text(x: f64) -> String {
    js::num_to_string(x)
}

/// UTF-16 upper case (JS `toUpperCase`; an unpaired surrogate stays).
pub fn upper16(s: &[u16]) -> Vec<u16> {
    case16(s, true)
}
/// UTF-16 lower case.
pub fn lower16(s: &[u16]) -> Vec<u16> {
    case16(s, false)
}
fn case16(s: &[u16], up: bool) -> Vec<u16> {
    if s.iter().all(|&c| c < 0x80) {
        return s
            .iter()
            .map(|&c| {
                if up {
                    u16::from(u8::try_from(c).unwrap_or(0).to_ascii_uppercase())
                } else {
                    u16::from(u8::try_from(c).unwrap_or(0).to_ascii_lowercase())
                }
            })
            .collect();
    }
    let mut out = Vec::with_capacity(s.len());
    for r in char::decode_utf16(s.iter().copied()) {
        match r {
            Ok(c) => {
                let mut buf = [0u16; 2];
                if up {
                    for u in c.to_uppercase() {
                        out.extend_from_slice(u.encode_utf16(&mut buf));
                    }
                } else {
                    for u in lower_js(c) {
                        out.extend_from_slice(u.encode_utf16(&mut buf));
                    }
                }
            }
            Err(e) => out.push(e.unpaired_surrogate()),
        }
    }
    out
}
/// JS `toLowerCase` of one character (Rust's `to_lowercase` but for the final sigma rule, which JS applies by
/// context and Rust's `char` cannot; a lone Σ lowers to σ in both).
fn lower_js(c: char) -> std::char::ToLowercase {
    c.to_lowercase()
}

/// `a < b` for JS strings (UTF-16 code units).
pub fn str_lt(a: &[u16], b: &[u16]) -> bool {
    a < b
}

/// `indexOf(needle, from)` in UTF-16 units (JS clamps `from` to [0, len]).
pub fn index_of(hay: &[u16], needle: &[u16], from: f64) -> Option<usize> {
    let len = hay.len();
    let start = clamp_pos(from, len);
    if needle.is_empty() {
        return Some(start);
    }
    if needle.len() > len {
        return None;
    }
    let last = len.saturating_sub(needle.len());
    (start..=last).find(|&i| hay.get(i..i.saturating_add(needle.len())) == Some(needle))
}
/// `lastIndexOf(needle, from)` in UTF-16 units.
pub fn last_index_of(hay: &[u16], needle: &[u16], from: f64) -> Option<usize> {
    let len = hay.len();
    let start = if from.is_nan() { len } else { clamp_pos(from, len) };
    if needle.len() > len {
        return None;
    }
    let max = len.saturating_sub(needle.len()).min(start);
    (0..=max).rev().find(|&i| hay.get(i..i.saturating_add(needle.len())) == Some(needle))
}
/// A JS position argument clamped to [0, len] (NaN = 0).
pub fn clamp_pos(p: f64, len: usize) -> usize {
    if p.is_nan() || p <= 0.0 {
        return 0;
    }
    let t = p.trunc();
    // len is a string length (≤ 2^29): the comparison makes the cast exact
    #[allow(clippy::cast_precision_loss)]
    let lf = len as f64;
    if t >= lf {
        return len;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let i = t as usize;
    i
}

/// The longest JS string (V8's `String::kMaxLength` on 64-bit): longer ones are "Invalid string length".
pub const MAX_STR: usize = (1 << 29) - 24;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reals_print_as_autolisp() {
        for (x, s) in [
            (3.0, "3.0"),
            (1.123456, "1.12346"),
            (1234567.0, "1.23457e+006"),
            (0.00001, "1.0e-005"),
            (0.0001, "0.0001"),
            (-2.5, "-2.5"),
            (100000.0, "100000.0"),
            (999999.5, "1.0e+006"),
            (f64::NAN, "-1.#IND"),
            (f64::INFINITY, "1.#INF"),
            (-0.0, "0.0"),
            (0.1 + 0.2, "0.3"),
        ] {
            assert_eq!(real_text(x), s, "{x}");
        }
    }

    #[test]
    fn a_long_list_drops_without_recursion() {
        let mut l = V::Nil;
        for i in 0..1_000_000 {
            l = V::cons(V::Int(i), l);
        }
        drop(l);
        let mut d = V::Nil;
        for _ in 0..1_000_000 {
            d = V::cons(d, V::Nil);
        }
        drop(d);
    }

    #[test]
    fn utf16_positions() {
        let s = lstr("a😀b");
        assert_eq!(s.len(), 4);
        assert_eq!(index_of(&s, &lstr("b"), 0.0), Some(3));
        assert_eq!(rstr(&upper16(&lstr("straße"))), "STRASSE");
    }
}
