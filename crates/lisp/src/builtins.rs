//! The built-in functions that need no drawing (builtins.js) and the special forms (interp.js installSpecials):
//! math, comparison, lists, strings, conversions, wildcards, geometry, output, files (the interpreter's own, in
//! memory), load, eval / read / apply / mapcar, exit. Installed in 0.5's order (ATOMS-FAMILY lists in it).
//!
//! Each function is checked as 0.5 checks it (argument types, nil cases, integer vs real) and computes as its JS does
//! (doubles then ToInt32 for integers, `Math.imul`, shift counts masked to 5 bits, V8's Math functions).

use std::rc::Rc;

use cc_cad::js;
use cc_cad_geom::jsmath::JsMath;

use crate::interp::{Bf, Ctl, Lisp};
use crate::reader::{read_all, read_one};
use crate::syms::{S_LAMBDA, S_T};
use crate::value::{LErr, LStr, MSG_QUIT, R, Sym, V, from_vec, is_proper, lower16, lstr, rstr, to_i32, to_vec, upper16};

// ---- argument checks (AutoLISP's messages) ----

pub fn arity(a: &[V], min: usize, max: usize) -> R<()> {
    if a.len() < min {
        return Err(LErr::new("too few arguments"));
    }
    if a.len() > max {
        return Err(LErr::new("too many arguments"));
    }
    Ok(())
}
pub const MANY: usize = usize::MAX;

/// The argument at `k`, nil when absent (`a[k]` → undefined reads as nil almost everywhere).
pub fn arg(a: &[V], k: usize) -> V {
    a.get(k).cloned().unwrap_or(V::Nil)
}
pub fn num(l: &Lisp, x: &V) -> R<f64> {
    x.num().ok_or_else(|| l.bad("numberp", x))
}
pub fn intv(l: &Lisp, x: &V) -> R<i32> {
    if let V::Int(i) = x { Ok(*i) } else { Err(l.bad("fixnump", x)) }
}
pub fn strv(l: &Lisp, x: &V) -> R<LStr> {
    if let V::Str(s) = x { Ok(Rc::clone(s)) } else { Err(l.bad("stringp", x)) }
}
pub fn lst(l: &Lisp, x: &V) -> R<V> {
    if x.is_list() { Ok(x.clone()) } else { Err(l.bad("listp", x)) }
}
pub fn symv(l: &Lisp, x: &V) -> R<Sym> {
    if let V::Sym(s) = x { Ok(*s) } else { Err(l.bad("symbolp", x)) }
}
/// A point argument: a list of 2 or 3 numbers → [x, y, z?].
pub fn pt(l: &Lisp, x: &V) -> R<Vec<f64>> {
    let a = to_vec(&lst(l, x)?);
    if a.len() < 2 || !a.iter().all(V::is_num) {
        return Err(l.bad("2D/3D point", x));
    }
    Ok(a.iter().take(3).filter_map(V::num).collect())
}
pub fn pt_list(p: &[f64]) -> V {
    from_vec(p.iter().map(|x| V::Real(*x)).collect(), V::Nil)
}
pub fn bool_v(b: bool) -> V {
    if b { V::Sym(S_T) } else { V::Nil }
}
/// JS truthiness of a LISP value as 0.5's code tests `if (a[1])`: nil, a real 0 / NaN and "" are false.
pub fn js_truthy(x: &V) -> bool {
    match x {
        V::Nil => false,
        V::Real(r) => *r != 0.0 && !r.is_nan(),
        V::Str(s) => !s.is_empty(),
        _ => true,
    }
}
/// `String(x)` of a LISP value as JS would write the object (symbols print their name).
pub fn js_string(l: &Lisp, x: &V) -> String {
    match x {
        V::Nil => "null".into(),
        V::Str(s) => rstr(s),
        V::Real(r) => js::num_to_string(*r),
        V::Sym(s) => l.name(*s).to_string(),
        _ => "[object Object]".into(),
    }
}
fn all_int(a: &[V]) -> bool {
    a.iter().all(|x| matches!(x, V::Int(_)))
}
fn int(v: f64) -> V {
    V::Int(to_i32(v))
}

// ---- equality ----

/// equal (with a fuzz for numbers), without recursion.
pub fn equal(a: &V, b: &V, fuzz: f64) -> bool {
    let mut work: Vec<(V, V)> = vec![(a.clone(), b.clone())];
    while let Some((x, y)) = work.pop() {
        if !equal_step(&x, &y, fuzz, &mut work) {
            return false;
        }
    }
    true
}
fn equal_step(a: &V, b: &V, fuzz: f64, work: &mut Vec<(V, V)>) -> bool {
    if identical(a, b) {
        return true;
    }
    if let (Some(x), Some(y)) = (a.num(), b.num()) {
        return (x - y).abs() <= fuzz;
    }
    match (a, b) {
        (V::Cons(_), V::Cons(_)) => {
            let (mut p, mut q) = (a.clone(), b.clone());
            while let (V::Cons(c1), V::Cons(c2)) = (&p, &q) {
                work.push((c1.car.clone(), c2.car.clone()));
                let (np, nq) = (c1.cdr.clone(), c2.cdr.clone());
                p = np;
                q = nq;
            }
            work.push((p, q));
            true
        }
        (V::Ename(x), V::Ename(y)) => x == y,
        (V::Vla(x), V::Vla(y)) => crate::vla::same(x, y),
        _ => false,
    }
}
/// `a === b` for LISP values (strings by value, objects by identity).
fn identical(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Nil, V::Nil) => true,
        (V::Int(x), V::Int(y)) => x == y,
        (V::Real(x), V::Real(y)) => x == y,
        (V::Str(x), V::Str(y)) => x == y,
        (V::Sym(x), V::Sym(y)) => x == y,
        (V::Cons(x), V::Cons(y)) => Rc::ptr_eq(x, y),
        (V::Ename(x), V::Ename(y)) => Rc::ptr_eq(x, y),
        (V::PickSet(x), V::PickSet(y)) => Rc::ptr_eq(x, y),
        (V::Fun(x), V::Fun(y)) => Rc::ptr_eq(x, y),
        (V::File(x), V::File(y)) => Rc::ptr_eq(x, y),
        (V::Vla(x), V::Vla(y)) => Rc::ptr_eq(x, y),
        (V::Variant(x), V::Variant(y)) => Rc::ptr_eq(x, y),
        (V::SafeArray(x), V::SafeArray(y)) => Rc::ptr_eq(x, y),
        (V::CatchErr(x), V::CatchErr(y)) => Rc::ptr_eq(x, y),
        _ => false,
    }
}
/// eq.
pub fn eq(a: &V, b: &V) -> bool {
    if identical(a, b) {
        return true;
    }
    match (a, b) {
        (V::Int(x), V::Int(y)) => x == y,
        (V::Real(x), V::Real(y)) => x == y,
        (V::Ename(x), V::Ename(y)) => x == y,
        (V::Vla(x), V::Vla(y)) => crate::vla::same(x, y),
        _ => false,
    }
}

// ---- number formatting ----

/// rtos: mode 1 scientific, 2 decimal, 3 engineering, 4 architectural, 5 fractional; DIMZIN 8 drops trailing zeros,
/// 4 the leading one. `mode`, `prec` and `dimzin` are numbers as the JS has them (NaN where 0.5 passes an object).
pub fn rtos(v: f64, mode: f64, prec: f64, dimzin: f64) -> String {
    if mode == 2.0 {
        let p = to_fixed_digits(prec);
        let a = if v.abs() < 0.5 * 10f64.jpow(-prec) { 0.0 } else { v };
        let mut s = js::to_fixed(a, p);
        let dz = to_i32(dimzin);
        if dz & 8 != 0 && s.contains('.') {
            s = s.trim_end_matches('0').to_string();
            if let Some(t) = s.strip_suffix('.') {
                s = t.to_string();
            }
        }
        if dz & 4 != 0 {
            if let Some(rest) = s.strip_prefix("0.") {
                s = format!(".{rest}");
            } else if let Some(rest) = s.strip_prefix("-0.") {
                s = format!("-.{rest}");
            }
        }
        return s;
    }
    let prec_v = if prec.is_nan() { serde_json::Value::Null } else { serde_json::Value::from(prec) };
    cc_cad_geom::units::format_length(v, mode, if prec.is_nan() { None } else { Some(&prec_v) })
}
/// toFixed's digits argument: ToIntegerOrInfinity, NaN → 0 (the callers clamp to 0…8 before).
fn to_fixed_digits(p: f64) -> usize {
    if p.is_nan() || p <= 0.0 {
        return 0;
    }
    let t = p.trunc().min(100.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let u = t as usize;
    u
}

/// angtos: 0 degrees, 1 deg/min/sec, 2 grads, 3 radians, 4 surveyor's units (anything else too: 0.5's default
/// mode reaches here as an object).
pub fn angtos(a: f64, mode: f64, prec: f64) -> String {
    let tau = std::f64::consts::PI * 2.0;
    let a = ((a % tau) + tau) % tau;
    let fx = |v: f64| js::to_fixed(v, to_fixed_digits(prec));
    if mode == 0.0 {
        return fx(a * 180.0 / std::f64::consts::PI);
    }
    if mode == 2.0 {
        return fx(a * 200.0 / std::f64::consts::PI) + "g";
    }
    if mode == 3.0 {
        return fx(a) + "r";
    }
    let dms = |deg: f64| -> String {
        let mut d = (deg + 1e-12).floor();
        let m = (deg - d) * 60.0;
        if prec <= 0.0 {
            return format!("{}d", js::num_to_string(js::round(deg)));
        }
        let mut mi = (m + 1e-9).floor();
        let s = (m - mi) * 60.0;
        if prec <= 2.0 {
            mi = js::round(m);
            if mi == 60.0 {
                d += 1.0;
                mi = 0.0;
            }
            return format!("{}d{}'", js::num_to_string(d), js::num_to_string(mi));
        }
        let sd = js::max(0.0, prec - 4.0);
        let mut ss = js::str_to_number(&js::to_fixed(s, to_fixed_digits(sd)));
        if ss >= 60.0 {
            ss = 0.0;
            mi += 1.0;
        }
        if mi >= 60.0 {
            mi = 0.0;
            d += 1.0;
        }
        format!("{}d{}'{}\"", js::num_to_string(d), js::num_to_string(mi), js::to_fixed(ss, to_fixed_digits(sd)))
    };
    if mode == 1.0 {
        return dms(a * 180.0 / std::f64::consts::PI);
    }
    let deg = a * 180.0 / std::f64::consts::PI;
    if (deg - 90.0).abs() < 1e-9 {
        return "E".into();
    }
    if (deg - 270.0).abs() < 1e-9 {
        return "W".into();
    }
    if deg.abs() < 1e-9 || (deg - 360.0).abs() < 1e-9 {
        return "N".into();
    }
    if (deg - 180.0).abs() < 1e-9 {
        return "S".into();
    }
    let ns = if deg < 180.0 { "N" } else { "S" };
    let off = if deg < 180.0 { (90.0 - deg).abs() } else { (270.0 - deg).abs() };
    let ew = if deg > 90.0 && deg < 270.0 { "W" } else { "E" };
    format!("{ns}{}{ew}", dms(off))
}

/// JS `parseFloat` of a whole trimmed text (used where 0.5 parses after its own test).
fn parse_float(s: &str) -> f64 {
    let t = js::trim_start(s);
    let b = t.as_bytes();
    let mut i = usize::from(matches!(b.first(), Some(b'+' | b'-')));
    if t.get(i..).is_some_and(|r| r.starts_with("Infinity")) {
        return if b.first() == Some(&b'-') { f64::NEG_INFINITY } else { f64::INFINITY };
    }
    let s0 = i;
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i = i.saturating_add(1);
    }
    let mut any = i > s0;
    if b.get(i) == Some(&b'.') {
        let mut j = i.saturating_add(1);
        let f0 = j;
        while b.get(j).is_some_and(u8::is_ascii_digit) {
            j = j.saturating_add(1);
        }
        if j > f0 || any {
            any = true;
            i = j;
        }
    }
    if !any {
        return f64::NAN;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        let mut j = i.saturating_add(1);
        if matches!(b.get(j), Some(b'+' | b'-')) {
            j = j.saturating_add(1);
        }
        let e0 = j;
        while b.get(j).is_some_and(u8::is_ascii_digit) {
            j = j.saturating_add(1);
        }
        if j > e0 {
            i = j;
        }
    }
    t.get(..i).map_or(f64::NAN, js::str_to_number)
}

/// angtof: text in a mode → radians, or None.
pub fn angtof(s: &str, mode: i32) -> Option<f64> {
    let t = js::trim(s);
    let pi = std::f64::consts::PI;
    let ends = |c: char| t.chars().last().is_some_and(|x| x.eq_ignore_ascii_case(&c));
    if mode == 3 || ends('r') {
        let v = parse_float(t);
        return v.is_finite().then_some(v);
    }
    if mode == 2 || ends('g') {
        let v = parse_float(t);
        return v.is_finite().then(|| v * pi / 200.0);
    }
    // ^([NS])\s*(.*?)\s*([EW])$  (i)
    let first = t.chars().next();
    let last = t.chars().last();
    if t.chars().count() >= 2
        && let (Some(f), Some(l)) = (first, last)
        && matches!(f.to_ascii_uppercase(), 'N' | 'S')
        && matches!(l.to_ascii_uppercase(), 'E' | 'W')
    {
        let inner = t.get(f.len_utf8()..t.len().saturating_sub(l.len_utf8())).unwrap_or("");
        let inner = inner.trim_matches(|c: char| js::is_ws(c));
        let off = angtof(inner, 1)?;
        let base = if f.eq_ignore_ascii_case(&'N') { 90.0 } else { 270.0 };
        let sgn = if f.eq_ignore_ascii_case(&'N') == l.eq_ignore_ascii_case(&'E') { -1.0 } else { 1.0 };
        return Some(((base * pi / 180.0) + sgn * off + 2.0 * pi) % (2.0 * pi));
    }
    if let Some(v) = dms_parse(t) {
        return Some(v * pi / 180.0);
    }
    // ^[-+]?(\d+\.?\d*|\.\d+)(e[-+]?\d+)?$  (i)
    if plain_number(t) {
        return Some(parse_float(t) * pi / 180.0);
    }
    None
}
/// ^(-?\d+(?:\.\d*)?)d(?:(\d+(?:\.\d*)?)')?(?:(\d+(?:\.\d*)?)")?$  (i) → degrees
fn dms_parse(t: &str) -> Option<f64> {
    let b = t.as_bytes();
    let mut i = 0usize;
    let num = |i: &mut usize, neg_ok: bool| -> Option<f64> {
        let s0 = *i;
        if neg_ok && b.get(*i) == Some(&b'-') {
            *i = i.saturating_add(1);
        }
        let d0 = *i;
        while b.get(*i).is_some_and(u8::is_ascii_digit) {
            *i = i.saturating_add(1);
        }
        if *i == d0 {
            return None;
        }
        if b.get(*i) == Some(&b'.') {
            *i = i.saturating_add(1);
            while b.get(*i).is_some_and(u8::is_ascii_digit) {
                *i = i.saturating_add(1);
            }
        }
        Some(js::str_to_number(t.get(s0..*i)?))
    };
    let d = num(&mut i, true)?;
    if !matches!(b.get(i), Some(b'd' | b'D')) {
        return None;
    }
    i = i.saturating_add(1);
    let save = i;
    let mut m = 0.0;
    if let Some(v) = num(&mut i, false) {
        if b.get(i) == Some(&b'\'') {
            i = i.saturating_add(1);
            m = v;
        } else {
            i = save;
        }
    } else {
        i = save;
    }
    let save2 = i;
    let mut s = 0.0;
    if let Some(v) = num(&mut i, false) {
        if b.get(i) == Some(&b'"') {
            i = i.saturating_add(1);
            s = v;
        } else {
            i = save2;
        }
    } else {
        i = save2;
    }
    if i != b.len() {
        return None;
    }
    Some(d + m / 60.0 + s / 3600.0)
}
/// ^[-+]?(\d+\.?\d*|\.\d+)(e[-+]?\d+)?$  (i)
pub fn plain_number(t: &str) -> bool {
    let b = t.as_bytes();
    let mut i = usize::from(matches!(b.first(), Some(b'+' | b'-')));
    let d0 = i;
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i = i.saturating_add(1);
    }
    if i > d0 {
        if b.get(i) == Some(&b'.') {
            i = i.saturating_add(1);
            while b.get(i).is_some_and(u8::is_ascii_digit) {
                i = i.saturating_add(1);
            }
        }
    } else {
        if b.get(i) != Some(&b'.') {
            return false;
        }
        i = i.saturating_add(1);
        let f0 = i;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i = i.saturating_add(1);
        }
        if i == f0 {
            return false;
        }
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i = i.saturating_add(1);
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i = i.saturating_add(1);
        }
        let e0 = i;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i = i.saturating_add(1);
        }
        if i == e0 {
            return false;
        }
    }
    i == b.len()
}

/// cvunit's table: kind and factor to the base unit.
fn unit(name: &str) -> Option<(&'static str, f64)> {
    let pi = std::f64::consts::PI;
    Some(match name {
        "inch" | "inches" | "in" | "\"" => ("len", 0.0254),
        "foot" | "feet" | "ft" | "'" => ("len", 0.3048),
        "yard" | "yards" | "yd" => ("len", 0.9144),
        "mile" | "miles" | "mi" => ("len", 1609.344),
        "millimeter" | "millimeters" | "mm" => ("len", 0.001),
        "centimeter" | "centimeters" | "cm" => ("len", 0.01),
        "meter" | "meters" | "m" => ("len", 1.0),
        "kilometer" | "kilometers" | "km" => ("len", 1000.0),
        "degree" | "degrees" => ("ang", pi / 180.0),
        "radian" | "radians" => ("ang", 1.0),
        "grad" | "grads" => ("ang", pi / 200.0),
        "kilogram" | "kg" => ("mass", 1.0),
        "gram" | "g" => ("mass", 0.001),
        "pound" | "lb" => ("mass", 0.453_592_37),
        "sq inch" => ("area", 0.000_645_16),
        "sq foot" => ("area", 0.092_903_04),
        "sq meter" => ("area", 1.0),
        "sq mm" => ("area", 1e-6),
        "acre" => ("area", 4_046.856_422_4),
        "hectare" => ("area", 10_000.0),
        _ => return None,
    })
}

/// V8's `Math.hypot(...xs)`: the Kahan-compensated sum of squares, scaled by the largest argument.
pub fn hypot_n(xs: &[f64]) -> f64 {
    let mut max = 0.0f64;
    let mut nan = false;
    for &x in xs {
        let a = x.abs();
        if a.is_infinite() {
            return f64::INFINITY;
        }
        if a.is_nan() {
            nan = true;
        } else if a > max {
            max = a;
        }
    }
    if nan {
        return f64::NAN;
    }
    if max == 0.0 {
        return 0.0;
    }
    let mut sum = 0.0f64;
    let mut comp = 0.0f64;
    for &x in xs {
        let n = x.abs() / max;
        let summand = n * n - comp;
        let pre = sum + summand;
        comp = (pre - sum) - summand;
        sum = pre;
    }
    sum.sqrt() * max
}

/// `Math.pow` (the JS special cases first: ±1 to ±∞ and anything to NaN are NaN).
pub fn js_pow(b: f64, e: f64) -> f64 {
    if e.is_nan() {
        return f64::NAN;
    }
    if e == 0.0 {
        return 1.0;
    }
    if e.is_infinite() && b.abs() == 1.0 {
        return f64::NAN;
    }
    b.jpow(e)
}

/// Math.exp / Math.log as V8 (fdlibm).
pub fn js_exp(x: f64) -> f64 {
    libm::exp(x)
}
pub fn js_log(x: f64) -> f64 {
    libm::log(x)
}

/// JS `<` for the values a comparison takes (strings, numbers): None when undefined (NaN).
fn js_lt(x: &V, y: &V) -> Option<bool> {
    match (x, y) {
        (V::Str(a), V::Str(b)) => Some(a.as_ref() < b.as_ref()),
        _ => {
            let to_n = |v: &V| match v {
                V::Str(s) => js::str_to_number(&rstr(s)),
                _ => v.num().unwrap_or(f64::NAN),
            };
            let (a, b) = (to_n(x), to_n(y));
            if a.is_nan() || b.is_nan() { None } else { Some(a < b) }
        }
    }
}

fn same_value(x: &V, y: &V) -> bool {
    match (x.num(), y.num()) {
        (Some(a), Some(b)) => a == b,
        _ => match (x, y) {
            (V::Str(a), V::Str(b)) => a == b,
            _ => eq(x, y),
        },
    }
}

fn car(l: &Lisp, x: &V) -> R<V> {
    match x {
        V::Nil => Ok(V::Nil),
        V::Cons(c) => Ok(c.car.clone()),
        _ => Err(l.bad("consp", x)),
    }
}
fn cdr(l: &Lisp, x: &V) -> R<V> {
    match x {
        V::Nil => Ok(V::Nil),
        V::Cons(c) => Ok(c.cdr.clone()),
        _ => Err(l.bad("consp", x)),
    }
}

/// A host variable for rtos / angtos / distof as the JS reads it: a real's value, NaN for an integer (0.5 gets the
/// plain `{ i }` object there and every number test on it fails), the default when there is no answer.
pub fn sysv(l: &Lisp, name: &str, dflt: f64) -> f64 {
    let Some(h) = &l.host else { return dflt };
    match h.getvar(name) {
        Ok(crate::host::Pv::Nil) | Err(_) => dflt,
        Ok(crate::host::Pv::Num(n)) => n,
        Ok(_) => f64::NAN,
    }
}

fn to_file(l: &Lisp, f: &V, text: &[u16]) -> R<()> {
    if let V::File(fh) = f
        && let Ok(mut fh) = fh.try_borrow_mut()
        && fh.open
        && fh.mode != b'r'
    {
        if fh.buf.len().saturating_add(text.len()) > l.limits.string {
            return Err(LErr::new("Invalid string length"));
        }
        fh.buf.extend_from_slice(text);
        return Ok(());
    }
    Err(l.bad("filep", f))
}

/// Text the routine prints (princ, prompt …), as a String.
fn print16(l: &Lisp, s: &str) {
    l.print(s);
}

fn ok(v: V) -> Ctl {
    Ctl::Ret(v)
}

// ---------------------------------------------------------------------------------------------- special forms

fn sp_quote(_: &mut Lisp, l: V) -> Ctl {
    ok(l.car().cloned().unwrap_or(V::Nil))
}
fn sp_function(lisp: &mut Lisp, l: V) -> Ctl {
    let x = l.car().cloned().unwrap_or(V::Nil);
    if let V::Cons(c) = &x
        && matches!(c.car, V::Sym(S_LAMBDA))
    {
        return match lisp.make_lambda(None, &c.cdr) {
            Ok(v) => ok(v),
            Err(e) => Ctl::Throw(e),
        };
    }
    if let V::Sym(s) = &x {
        return ok(lisp.raw(*s).unwrap_or(x));
    }
    ok(x)
}
fn sp_setq(lisp: &mut Lisp, l: V) -> Ctl {
    lisp.push_frame_setq(l)
}
fn sp_defun(lisp: &mut Lisp, l: V) -> Ctl {
    let name = l.car().cloned().unwrap_or(V::Nil);
    let V::Sym(s) = name else { return Ctl::Throw(lisp.bad("symbolp", &name)) };
    let n = lisp.name(s).to_string();
    match lisp.make_lambda(Some(n.clone()), l.cdr().unwrap_or(&V::Nil)) {
        Ok(f) => {
            lisp.set(s, f);
            if n.starts_with("C:") && n.len() > 2 {
                lisp.defun_hook(n.get(2..).unwrap_or(""));
            }
            ok(V::Sym(s))
        }
        Err(e) => Ctl::Throw(e),
    }
}
fn sp_lambda(lisp: &mut Lisp, l: V) -> Ctl {
    match lisp.make_lambda(None, &l) {
        Ok(f) => ok(f),
        Err(e) => Ctl::Throw(e),
    }
}
fn sp_if(lisp: &mut Lisp, l: V) -> Ctl {
    let f = to_vec(&l);
    if f.len() < 2 {
        return Ctl::Throw(LErr::new("too few arguments"));
    }
    if f.len() > 3 {
        return Ctl::Throw(LErr::new("too many arguments"));
    }
    let mut it = f.into_iter();
    let test = it.next().unwrap_or(V::Nil);
    let then = it.next().unwrap_or(V::Nil);
    let els = it.next();
    lisp.push_frame_if(test, then, els)
}
fn sp_cond(lisp: &mut Lisp, l: V) -> Ctl {
    lisp.push_frame_cond(to_vec(&l))
}
fn sp_while(lisp: &mut Lisp, l: V) -> Ctl {
    let f = to_vec(&l);
    let Some(test) = f.first().cloned() else { return ok(V::Nil) };
    let body: Rc<[V]> = f.get(1..).unwrap_or(&[]).to_vec().into();
    lisp.push_frame_while(test, body)
}
fn sp_repeat(lisp: &mut Lisp, l: V) -> Ctl {
    let f = to_vec(&l);
    let Some(n) = f.first().cloned() else { return Ctl::Throw(lisp.bad("fixnump", &V::Nil)) };
    let body: Rc<[V]> = f.get(1..).unwrap_or(&[]).to_vec().into();
    lisp.eval_then(n, move |lisp, c| match c {
        V::Int(k) => lisp.push_frame_repeat(i64::from(k), body),
        other => Ctl::Throw(lisp.bad("fixnump", &other)),
    })
}
fn sp_foreach(lisp: &mut Lisp, l: V) -> Ctl {
    let f = to_vec(&l);
    let v = f.first().cloned().unwrap_or(V::Nil);
    let V::Sym(var) = v else { return Ctl::Throw(lisp.bad("symbolp", &v)) };
    let Some(lform) = f.get(1).cloned() else { return Ctl::Throw(lisp.bad("listp", &V::Nil)) };
    let body: Rc<[V]> = f.get(2..).unwrap_or(&[]).to_vec().into();
    lisp.eval_then(lform, move |lisp, items| {
        if !items.is_list() {
            return Ctl::Throw(lisp.bad("listp", &items));
        }
        lisp.push_frame_foreach(var, items, body)
    })
}
fn sp_progn(lisp: &mut Lisp, l: V) -> Ctl {
    lisp.seq(to_vec(&l).into())
}
fn sp_and(lisp: &mut Lisp, l: V) -> Ctl {
    lisp.push_frame_and(from_vec(to_vec(&l), V::Nil))
}
fn sp_or(lisp: &mut Lisp, l: V) -> Ctl {
    lisp.push_frame_or(from_vec(to_vec(&l), V::Nil))
}

// ---------------------------------------------------------------------------------------------- arithmetic

fn check_nums(l: &Lisp, a: &[V]) -> R<()> {
    for x in a {
        num(l, x)?;
    }
    Ok(())
}
fn f_add(l: &mut Lisp, a: &[V]) -> R<V> {
    check_nums(l, a)?;
    if all_int(a) {
        return Ok(int(a.iter().filter_map(V::num).fold(0.0, |s, x| s + x)));
    }
    Ok(V::Real(a.iter().filter_map(V::num).fold(0.0, |s, x| s + x)))
}
fn f_sub(l: &mut Lisp, a: &[V]) -> R<V> {
    check_nums(l, a)?;
    match a {
        [] => Ok(V::Int(0)),
        [V::Int(i)] => Ok(int(-f64::from(*i))),
        [x] => Ok(V::Real(-x.num().unwrap_or(0.0))),
        [first, rest @ ..] => {
            let r = rest.iter().filter_map(V::num).fold(first.num().unwrap_or(0.0), |s, x| s - x);
            Ok(if all_int(a) { int(r) } else { V::Real(r) })
        }
    }
}
fn f_mul(l: &mut Lisp, a: &[V]) -> R<V> {
    check_nums(l, a)?;
    if a.is_empty() {
        return Ok(V::Int(0));
    }
    if all_int(a) {
        let p = a.iter().fold(1i32, |s, x| if let V::Int(i) = x { s.wrapping_mul(*i) } else { s });
        return Ok(V::Int(p));
    }
    Ok(V::Real(a.iter().filter_map(V::num).fold(1.0, |s, x| s * x)))
}
fn f_div(l: &mut Lisp, a: &[V]) -> R<V> {
    check_nums(l, a)?;
    let Some(first) = a.first() else { return Ok(V::Int(0)) };
    let mut ints = matches!(first, V::Int(_));
    let mut r = first.num().unwrap_or(0.0);
    for x in a.iter().skip(1) {
        let v = x.num().unwrap_or(0.0);
        if v == 0.0 {
            return Err(LErr::new("divide by zero"));
        }
        ints = ints && matches!(x, V::Int(_));
        r = if ints { (r / v).trunc() } else { r / v };
    }
    Ok(if ints { int(r) } else { V::Real(r) })
}
fn f_lognot(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    Ok(V::Int(!intv(l, &arg(a, 0))?))
}
fn f_inc(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let x = arg(a, 0);
    num(l, &x)?;
    Ok(match x {
        V::Int(i) => int(f64::from(i) + 1.0),
        V::Real(r) => V::Real(r + 1.0),
        _ => V::Nil,
    })
}
fn f_dec(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let x = arg(a, 0);
    num(l, &x)?;
    Ok(match x {
        V::Int(i) => int(f64::from(i) - 1.0),
        V::Real(r) => V::Real(r - 1.0),
        _ => V::Nil,
    })
}
fn f_abs(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let x = arg(a, 0);
    num(l, &x)?;
    Ok(match x {
        V::Int(i) => int(f64::from(i).abs()),
        V::Real(r) => V::Real(r.abs()),
        _ => V::Nil,
    })
}
fn f_fix(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    Ok(int(num(l, &arg(a, 0))?.trunc()))
}
fn f_float(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    Ok(V::Real(num(l, &arg(a, 0))?))
}
fn f_rem(l: &mut Lisp, a: &[V]) -> R<V> {
    check_nums(l, a)?;
    let Some(first) = a.first() else { return Ok(V::Int(0)) };
    let mut r = first.num().unwrap_or(0.0);
    for x in a.iter().skip(1) {
        let v = x.num().unwrap_or(0.0);
        if v == 0.0 {
            return Err(LErr::new("divide by zero"));
        }
        r %= v;
    }
    Ok(if all_int(a) { int(r) } else { V::Real(r) })
}
fn gcd2(x: f64, y: f64) -> f64 {
    let (mut x, mut y) = (x.abs(), y.abs());
    while y != 0.0 {
        let t = x % y;
        x = y;
        y = t;
    }
    x
}
fn f_gcd(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, MANY)?;
    let mut vals = Vec::with_capacity(a.len());
    for x in a {
        vals.push(f64::from(intv(l, x)?));
    }
    let mut it = vals.into_iter();
    let first = it.next().unwrap_or(0.0);
    Ok(int(it.fold(first, gcd2)))
}
fn f_lcm(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, MANY)?;
    let mut vals = Vec::with_capacity(a.len());
    for x in a {
        vals.push(f64::from(intv(l, x)?));
    }
    let mut it = vals.into_iter();
    let first = it.next().unwrap_or(0.0);
    Ok(int(it.fold(first, |x, y| if x != 0.0 && y != 0.0 { (x * y).abs() / gcd2(x, y) } else { 0.0 })))
}
fn f_min(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, MANY)?;
    check_nums(l, a)?;
    let v = js::min_of(a.iter().filter_map(V::num));
    Ok(if all_int(a) { int(v) } else { V::Real(v) })
}
fn f_max(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, MANY)?;
    check_nums(l, a)?;
    let v = js::max_of(a.iter().filter_map(V::num));
    Ok(if all_int(a) { int(v) } else { V::Real(v) })
}
fn f_sqrt(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let v = num(l, &arg(a, 0))?;
    if v < 0.0 {
        return Err(LErr::new(format!("function undefined for argument: {}", l.show(&arg(a, 0), true))));
    }
    Ok(V::Real(v.sqrt()))
}
fn f_exp(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    Ok(V::Real(js_exp(num(l, &arg(a, 0))?)))
}
fn f_log(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let v = num(l, &arg(a, 0))?;
    if v <= 0.0 {
        return Err(LErr::new(format!("function undefined for argument: {}", l.show(&arg(a, 0), true))));
    }
    Ok(V::Real(js_log(v)))
}
fn f_expt(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    let b = num(l, &arg(a, 0))?;
    let e = num(l, &arg(a, 1))?;
    if matches!((arg(a, 0), arg(a, 1)), (V::Int(_), V::Int(_))) && e >= 0.0 {
        return Ok(int(js_pow(b, e)));
    }
    Ok(V::Real(js_pow(b, e)))
}
fn f_sin(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    Ok(V::Real(num(l, &arg(a, 0))?.jsin()))
}
fn f_cos(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    Ok(V::Real(num(l, &arg(a, 0))?.jcos()))
}
fn f_atan(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 2)?;
    if a.len() == 2 {
        let y = num(l, &arg(a, 0))?;
        let x = num(l, &arg(a, 1))?;
        return Ok(V::Real(y.jatan2(x)));
    }
    Ok(V::Real(num(l, &arg(a, 0))?.jatan()))
}
fn f_minusp(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    Ok(bool_v(num(l, &arg(a, 0))? < 0.0))
}
fn f_zerop(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    Ok(bool_v(num(l, &arg(a, 0))? == 0.0))
}
fn f_numberp(_: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    Ok(bool_v(arg(a, 0).is_num()))
}
fn f_boole(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, MANY)?;
    let op = intv(l, &arg(a, 0))?;
    let mut r = intv(l, &arg(a, 1))?;
    for x in a.iter().skip(2) {
        let b = intv(l, x)?;
        let mut out: i32 = 0;
        for bit in 0..32u32 {
            let p = (r.cast_unsigned() >> bit) & 1;
            let q = (b.cast_unsigned() >> bit) & 1;
            let k = if p != 0 && q != 0 {
                1
            } else if p != 0 {
                2
            } else if q != 0 {
                4
            } else {
                8
            };
            if op & k != 0 {
                out |= 1i32.wrapping_shl(bit);
            }
        }
        r = out;
    }
    Ok(V::Int(r))
}
fn f_logand(l: &mut Lisp, a: &[V]) -> R<V> {
    let mut r: i32 = -1;
    for x in a {
        r &= intv(l, x)?;
    }
    Ok(V::Int(if a.is_empty() { 0 } else { r }))
}
fn f_logior(l: &mut Lisp, a: &[V]) -> R<V> {
    let mut r: i32 = 0;
    for x in a {
        r |= intv(l, x)?;
    }
    Ok(V::Int(r))
}
fn f_lsh(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 0, 2)?;
    let v = if a.is_empty() { 0 } else { intv(l, &arg(a, 0))? };
    let n = if a.len() > 1 { intv(l, &arg(a, 1))? } else { 0 };
    if n >= 0 {
        return Ok(V::Int(v.wrapping_shl(n.cast_unsigned() & 31)));
    }
    let m = i64::from(n).wrapping_neg();
    let sh = u32::try_from(m & 31).unwrap_or(0);
    Ok(int(f64::from(v.cast_unsigned() >> sh)))
}

// ---------------------------------------------------------------------------------------------- comparison

fn cmp_args(l: &Lisp, a: &[V]) -> R<()> {
    for x in a {
        if !matches!(x, V::Str(_)) {
            num(l, x)?;
        }
    }
    Ok(())
}
fn chain(l: &mut Lisp, a: &[V], test: fn(&V, &V) -> bool) -> R<V> {
    arity(a, 1, MANY)?;
    cmp_args(l, a)?;
    for w in a.windows(2) {
        if let [x, y] = w
            && !test(x, y)
        {
            return Ok(V::Nil);
        }
    }
    Ok(V::Sym(S_T))
}
fn f_lt(l: &mut Lisp, a: &[V]) -> R<V> {
    chain(l, a, |x, y| js_lt(x, y) == Some(true))
}
fn f_gt(l: &mut Lisp, a: &[V]) -> R<V> {
    chain(l, a, |x, y| js_lt(y, x) == Some(true))
}
fn f_le(l: &mut Lisp, a: &[V]) -> R<V> {
    chain(l, a, |x, y| js_lt(y, x) == Some(false))
}
fn f_ge(l: &mut Lisp, a: &[V]) -> R<V> {
    chain(l, a, |x, y| js_lt(x, y) == Some(false))
}
fn f_numeq(_: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, MANY)?;
    for w in a.windows(2) {
        if let [x, y] = w
            && !same_value(x, y)
        {
            return Ok(V::Nil);
        }
    }
    Ok(V::Sym(S_T))
}
fn f_numne(_: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, MANY)?;
    for w in a.windows(2) {
        if let [x, y] = w
            && same_value(x, y)
        {
            return Ok(V::Nil);
        }
    }
    Ok(V::Sym(S_T))
}
fn f_eq(_: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    Ok(bool_v(eq(&arg(a, 0), &arg(a, 1))))
}
fn f_equal(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 3)?;
    let fuzz = if a.len() > 2 { num(l, &arg(a, 2))? } else { 0.0 };
    Ok(bool_v(equal(&arg(a, 0), &arg(a, 1), fuzz)))
}
fn f_not(_: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    Ok(bool_v(!arg(a, 0).truthy()))
}
fn f_atom(_: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    Ok(bool_v(!matches!(arg(a, 0), V::Cons(_))))
}
fn f_listp(_: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    Ok(bool_v(arg(a, 0).is_list()))
}
fn f_boundp(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let s = symv(l, &arg(a, 0))?;
    Ok(bool_v(l.bound(s)))
}
fn f_type(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    match Lisp::type_name(&arg(a, 0)) {
        Some(n) => Ok(V::Sym(l.fsym(n))),
        None => Ok(V::Nil),
    }
}

// ---------------------------------------------------------------------------------------------- lists

/// The c[ad]+r functions, by path (e.g. "AD" = cadr).
const CXR: [&str; 30] = [
    "A", "D", "AA", "AD", "DA", "DD", "AAA", "AAD", "ADA", "ADD", "DAA", "DAD", "DDA", "DDD", "AAAA", "AAAD", "AADA", "AADD", "ADAA",
    "ADAD", "ADDA", "ADDD", "DAAA", "DAAD", "DADA", "DADD", "DDAA", "DDAD", "DDDA", "DDDD",
];
fn cxr(l: &Lisp, path: &str, x: &V) -> R<V> {
    let mut v = x.clone();
    for o in path.chars().rev() {
        v = if o == 'A' { car(l, &v)? } else { cdr(l, &v)? };
    }
    Ok(v)
}
macro_rules! cxr_fn {
    ($($name:ident = $path:literal),*) => {
        $(fn $name(l: &mut Lisp, a: &[V]) -> R<V> { arity(a, 1, 1)?; cxr(l, $path, &arg(a, 0)) })*
    };
}
cxr_fn!(
    c_a = "A",
    c_d = "D",
    c_aa = "AA",
    c_ad = "AD",
    c_da = "DA",
    c_dd = "DD",
    c_aaa = "AAA",
    c_aad = "AAD",
    c_ada = "ADA",
    c_add = "ADD",
    c_daa = "DAA",
    c_dad = "DAD",
    c_dda = "DDA",
    c_ddd = "DDD",
    c_aaaa = "AAAA",
    c_aaad = "AAAD",
    c_aada = "AADA",
    c_aadd = "AADD",
    c_adaa = "ADAA",
    c_adad = "ADAD",
    c_adda = "ADDA",
    c_addd = "ADDD",
    c_daaa = "DAAA",
    c_daad = "DAAD",
    c_dada = "DADA",
    c_dadd = "DADD",
    c_ddaa = "DDAA",
    c_ddad = "DDAD",
    c_ddda = "DDDA",
    c_dddd = "DDDD"
);
/// A built-in on its evaluated arguments.
type Plain = fn(&mut Lisp, &[V]) -> R<V>;
const CXR_FNS: [Plain; 30] = [
    c_a, c_d, c_aa, c_ad, c_da, c_dd, c_aaa, c_aad, c_ada, c_add, c_daa, c_dad, c_dda, c_ddd, c_aaaa, c_aaad, c_aada, c_aadd, c_adaa,
    c_adad, c_adda, c_addd, c_daaa, c_daad, c_dada, c_dadd, c_ddaa, c_ddad, c_ddda, c_dddd,
];

fn f_cons(_: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    Ok(V::cons(arg(a, 0), arg(a, 1)))
}
fn f_list(_: &mut Lisp, a: &[V]) -> R<V> {
    Ok(from_vec(a.to_vec(), V::Nil))
}
fn f_append(l: &mut Lisp, a: &[V]) -> R<V> {
    let mut items = Vec::new();
    for x in a {
        items.extend(to_vec(&lst(l, x)?));
    }
    Ok(from_vec(items, V::Nil))
}
fn f_reverse(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let mut v = to_vec(&lst(l, &arg(a, 0))?);
    v.reverse();
    Ok(from_vec(v, V::Nil))
}
fn f_length(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let x = lst(l, &arg(a, 0))?;
    if !is_proper(&x) {
        return Err(l.bad("listp", &x));
    }
    Ok(int(crate::value::list_len(&x) as f64))
}
fn f_nth(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    let mut n = intv(l, &arg(a, 0))?;
    let mut x = lst(l, &arg(a, 1))?;
    if n < 0 {
        return Ok(V::Nil);
    }
    while n > 0 {
        let V::Cons(c) = &x else { break };
        let next = c.cdr.clone();
        x = next;
        n = n.saturating_sub(1);
    }
    Ok(x.car().cloned().unwrap_or(V::Nil))
}
fn f_last(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let mut x = lst(l, &arg(a, 0))?;
    if x.is_nil() {
        return Ok(V::Nil);
    }
    loop {
        let V::Cons(c) = &x else { return Ok(V::Nil) };
        if !matches!(c.cdr, V::Cons(_)) {
            return Ok(c.car.clone());
        }
        let next = c.cdr.clone();
        x = next;
    }
}
fn f_member(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    let item = arg(a, 0);
    let mut x = lst(l, &arg(a, 1))?;
    while let V::Cons(c) = &x {
        if equal(&item, &c.car, 0.0) {
            return Ok(x);
        }
        let next = c.cdr.clone();
        x = next;
    }
    Ok(V::Nil)
}
fn f_assoc(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    let key = arg(a, 0);
    let mut x = lst(l, &arg(a, 1))?;
    while let V::Cons(c) = &x {
        if let V::Cons(item) = &c.car
            && equal(&key, &item.car, 0.0)
        {
            return Ok(c.car.clone());
        }
        let next = c.cdr.clone();
        x = next;
    }
    Ok(V::Nil)
}
fn f_subst(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 3, 3)?;
    let (new, old) = (arg(a, 0), arg(a, 1));
    let v: Vec<V> = to_vec(&lst(l, &arg(a, 2))?).into_iter().map(|x| if equal(&x, &old, 0.0) { new.clone() } else { x }).collect();
    Ok(from_vec(v, V::Nil))
}
fn f_strlsort(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let v = to_vec(&lst(l, &arg(a, 0))?);
    let mut keyed = Vec::with_capacity(v.len());
    for x in v {
        let s = strv(l, &x)?;
        keyed.push((upper16(&s), x));
    }
    keyed.sort_by(|p, q| p.0.cmp(&q.0));
    Ok(from_vec(keyed.into_iter().map(|p| p.1).collect(), V::Nil))
}

// ---------------------------------------------------------------------------------------------- functions

fn f_apply(l: &mut Lisp, a: Vec<V>) -> Ctl {
    if let Err(e) = arity(&a, 2, 2) {
        return Ctl::Throw(e);
    }
    let f = match l.fn_of(&arg(&a, 0)) {
        Ok(f) => f,
        Err(e) => return Ctl::Throw(e),
    };
    match lst(l, &arg(&a, 1)) {
        Ok(x) => l.apply(f, to_vec(&x), None),
        Err(e) => Ctl::Throw(e),
    }
}
fn f_mapcar(l: &mut Lisp, a: Vec<V>) -> Ctl {
    if let Err(e) = arity(&a, 2, MANY) {
        return Ctl::Throw(e);
    }
    let f = match l.fn_of(&arg(&a, 0)) {
        Ok(f) => f,
        Err(e) => return Ctl::Throw(e),
    };
    let mut ls = Vec::new();
    for x in a.iter().skip(1) {
        match lst(l, x) {
            Ok(v) => ls.push(to_vec(&v)),
            Err(e) => return Ctl::Throw(e),
        }
    }
    let n = ls.iter().map(Vec::len).min().unwrap_or(0);
    mapcar_step(l, f, Rc::new(ls), 0, n, Vec::new())
}
fn mapcar_step(l: &mut Lisp, f: V, ls: Rc<Vec<Vec<V>>>, i: usize, n: usize, out: Vec<V>) -> Ctl {
    if i >= n {
        return ok(from_vec(out, V::Nil));
    }
    let args: Vec<V> = ls.iter().map(|x| x.get(i).cloned().unwrap_or(V::Nil)).collect();
    let f2 = f.clone();
    l.apply_then(f, args, move |l, v| {
        let mut out = out;
        out.push(v);
        mapcar_step(l, f2, ls, i.saturating_add(1), n, out)
    })
}
fn f_eval(_: &mut Lisp, a: Vec<V>) -> Ctl {
    if let Err(e) = arity(&a, 1, 1) {
        return Ctl::Throw(e);
    }
    Ctl::Eval(arg(&a, 0))
}
fn f_set(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    let s = symv(l, &arg(a, 0))?;
    Ok(l.set(s, arg(a, 1)))
}
fn f_read(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 0, 1)?;
    if a.is_empty() {
        return Ok(V::Nil);
    }
    let s = strv(l, &arg(a, 0))?;
    if js::trim(&rstr(&s)).is_empty() {
        return Ok(V::Nil);
    }
    Ok(read_one(&s, &mut l.syms)?.0)
}
fn f_atoms_family(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 2)?;
    let names: Vec<String> = l.bound_symbols().into_iter().map(|s| l.name(s).to_string()).collect();
    if a.len() > 1 {
        let items = to_vec(&lst(l, &arg(a, 1))?);
        let mut out = Vec::with_capacity(items.len());
        for s in items {
            let up = js_string(l, &s).to_uppercase();
            if names.contains(&up) {
                if intv(l, &arg(a, 0))? != 0 {
                    out.push(V::str(&up));
                } else {
                    out.push(V::Sym(l.sym(&js_string(l, &s))?));
                }
            } else {
                out.push(V::Nil);
            }
        }
        return Ok(from_vec(out, V::Nil));
    }
    if intv(l, &arg(a, 0))? != 0 {
        return Ok(from_vec(names.iter().map(|n| V::str(n)).collect(), V::Nil));
    }
    let mut out = Vec::with_capacity(names.len());
    for n in &names {
        out.push(V::Sym(l.sym(n)?));
    }
    Ok(from_vec(out, V::Nil))
}
fn f_exit(_: &mut Lisp, _: &[V]) -> R<V> {
    Err(LErr::new(MSG_QUIT))
}
fn f_nil(_: &mut Lisp, _: &[V]) -> R<V> {
    Ok(V::Nil)
}
fn f_ver(_: &mut Lisp, _: &[V]) -> R<V> {
    Ok(V::str("CadCraft LISP 0.4 (en)"))
}

// ---------------------------------------------------------------------------------------------- strings

fn f_strcat(l: &mut Lisp, a: &[V]) -> R<V> {
    let mut out: Vec<u16> = Vec::new();
    for x in a {
        let s = strv(l, x)?;
        if out.len().saturating_add(s.len()) > l.limits.string {
            return Err(LErr::new("Invalid string length"));
        }
        out.extend_from_slice(&s);
    }
    l.mk_str(out)
}
fn f_strlen(l: &mut Lisp, a: &[V]) -> R<V> {
    let mut n = 0usize;
    for x in a {
        n = n.saturating_add(strv(l, x)?.len());
    }
    Ok(int(n as f64))
}
fn f_substr(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 3)?;
    let s = strv(l, &arg(a, 0))?;
    let start = intv(l, &arg(a, 1))?;
    if start < 1 {
        return Err(LErr::new(format!("bad argument value: positive {}", l.show(&arg(a, 1), true))));
    }
    let n = if a.len() > 2 { Some(intv(l, &arg(a, 2))?) } else { None };
    if n.is_some_and(|n| n < 0) {
        return Err(LErr::new(format!("bad argument value: non-negative {}", l.show(&arg(a, 2), true))));
    }
    let from = usize::try_from(start.saturating_sub(1)).unwrap_or(0).min(s.len());
    let len = n.map_or(s.len(), |n| usize::try_from(n).unwrap_or(0));
    let to = from.saturating_add(len).min(s.len());
    Ok(V::Str(s.get(from..to).unwrap_or(&[]).into()))
}
fn f_strcase(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 2)?;
    let s = strv(l, &arg(a, 0))?;
    let lower = a.len() > 1 && arg(a, 1).truthy();
    l.mk_str(if lower { lower16(&s) } else { upper16(&s) })
}
fn f_ascii(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let s = strv(l, &arg(a, 0))?;
    Ok(V::Int(i32::from(s.first().copied().unwrap_or(0))))
}
fn f_chr(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let n = intv(l, &arg(a, 0))?;
    if n == 0 {
        return Ok(V::Str(Rc::from(Vec::<u16>::new())));
    }
    let u = u16::try_from(n & 0xffff).unwrap_or(0);
    Ok(V::Str(Rc::from(vec![u])))
}
fn f_itoa(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    Ok(V::str(&intv(l, &arg(a, 0))?.to_string()))
}
fn f_atoi(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let s = rstr(&strv(l, &arg(a, 0))?);
    let t = js::trim(&s);
    let b = t.as_bytes();
    let i0 = usize::from(matches!(b.first(), Some(b'+' | b'-')));
    let mut i = i0;
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i = i.saturating_add(1);
    }
    if i == i0 {
        return Ok(V::Int(0));
    }
    Ok(int(js::str_to_number(t.get(..i).unwrap_or("0"))))
}
fn f_atof(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let s = rstr(&strv(l, &arg(a, 0))?);
    let t = js::trim(&s);
    // ^[-+]?(\d+\.?\d*|\.\d+)(e[-+]?\d+)?  (i)
    let b = t.as_bytes();
    let mut i = usize::from(matches!(b.first(), Some(b'+' | b'-')));
    let d0 = i;
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i = i.saturating_add(1);
    }
    if i > d0 {
        if b.get(i) == Some(&b'.') {
            i = i.saturating_add(1);
            while b.get(i).is_some_and(u8::is_ascii_digit) {
                i = i.saturating_add(1);
            }
        }
    } else {
        if b.get(i) != Some(&b'.') {
            return Ok(V::Real(0.0));
        }
        let f0 = i.saturating_add(1);
        let mut j = f0;
        while b.get(j).is_some_and(u8::is_ascii_digit) {
            j = j.saturating_add(1);
        }
        if j == f0 {
            return Ok(V::Real(0.0));
        }
        i = j;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        let mut j = i.saturating_add(1);
        if matches!(b.get(j), Some(b'+' | b'-')) {
            j = j.saturating_add(1);
        }
        let e0 = j;
        while b.get(j).is_some_and(u8::is_ascii_digit) {
            j = j.saturating_add(1);
        }
        if j > e0 {
            i = j;
        }
    }
    Ok(V::Real(parse_float(t.get(..i).unwrap_or(""))))
}
fn f_wcmatch(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    let s = strv(l, &arg(a, 0))?;
    let p = strv(l, &arg(a, 1))?;
    Ok(bool_v(crate::wild::wcmatch(&s, &p)?))
}
fn f_rtos(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 3)?;
    let v = num(l, &arg(a, 0))?;
    let mode = if a.len() > 1 && !arg(a, 1).is_nil() { f64::from(intv(l, &arg(a, 1))?) } else { sysv(l, "LUNITS", 2.0) };
    let prec = if a.len() > 2 && !arg(a, 2).is_nil() { f64::from(intv(l, &arg(a, 2))?) } else { sysv(l, "LUPREC", 4.0) };
    if mode < 1.0 || mode > 5.0 {
        return Err(LErr::new(format!("bad argument value: {}", l.show(&arg(a, 1), true))));
    }
    let p = js::max(0.0, js::min(8.0, prec));
    let dz = sysv(l, "DIMZIN", 0.0);
    Ok(V::str(&rtos(v, mode, p, dz)))
}
fn f_angtos(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 3)?;
    let v = num(l, &arg(a, 0))?;
    let mode = if a.len() > 1 && !arg(a, 1).is_nil() { f64::from(intv(l, &arg(a, 1))?) } else { sysv(l, "AUNITS", 0.0) };
    let prec = if a.len() > 2 && !arg(a, 2).is_nil() { f64::from(intv(l, &arg(a, 2))?) } else { sysv(l, "AUPREC", 0.0) };
    Ok(V::str(&angtos(v, mode, js::max(0.0, js::min(8.0, prec)))))
}
fn f_distof(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 2)?;
    let mode = if a.len() > 1 && !arg(a, 1).is_nil() { f64::from(intv(l, &arg(a, 1))?) } else { sysv(l, "LUNITS", 2.0) };
    let s0 = rstr(&strv(l, &arg(a, 0))?);
    let s = js::trim(&s0);
    if mode == 1.0 || s.contains(['e', 'E']) {
        let v = js::str_to_number(s);
        return Ok(if v.is_finite() { V::Real(v) } else { V::Nil });
    }
    let lu = if mode == 2.0 || mode == 5.0 { 4.0 } else { mode };
    Ok(cc_cad_geom::units::parse_length(s, lu).map_or(V::Nil, V::Real))
}
fn f_angtof(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 2)?;
    let s = rstr(&strv(l, &arg(a, 0))?);
    let mode = if a.len() > 1 && !arg(a, 1).is_nil() { intv(l, &arg(a, 1))? } else { 0 };
    Ok(angtof(&s, mode).map_or(V::Nil, V::Real))
}
fn f_cvunit(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 3, 3)?;
    let from = rstr(&strv(l, &arg(a, 1))?);
    let to = rstr(&strv(l, &arg(a, 2))?);
    let (Some(f), Some(t)) = (unit(&js::trim(&from).to_lowercase()), unit(&js::trim(&to).to_lowercase())) else { return Ok(V::Nil) };
    if f.0 != t.0 {
        return Ok(V::Nil);
    }
    let conv = |v: f64| v * f.1 / t.1;
    let x = arg(a, 0);
    if let Some(v) = x.num() {
        return Ok(V::Real(conv(v)));
    }
    if let V::Cons(_) = &x {
        let mut out = Vec::new();
        for e in to_vec(&x) {
            out.push(V::Real(conv(num(l, &e)?)));
        }
        return Ok(from_vec(out, V::Nil));
    }
    Err(l.bad("numberp", &x))
}

// ---------------------------------------------------------------------------------------------- output

fn f_princ(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 0, 2)?;
    if a.is_empty() {
        return Ok(V::Str(Rc::from(Vec::<u16>::new())));
    }
    let s = l.show(&arg(a, 0), false);
    if js_truthy(&arg(a, 1)) {
        to_file(l, &arg(a, 1), &lstr(&s))?;
    } else {
        print16(l, &s);
    }
    Ok(arg(a, 0))
}
fn f_prin1(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 0, 2)?;
    if a.is_empty() {
        return Ok(V::Str(Rc::from(Vec::<u16>::new())));
    }
    let s = l.show(&arg(a, 0), true);
    if js_truthy(&arg(a, 1)) {
        to_file(l, &arg(a, 1), &lstr(&s))?;
    } else {
        print16(l, &s);
    }
    Ok(arg(a, 0))
}
fn f_print(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 0, 2)?;
    if a.is_empty() {
        return Ok(V::Str(Rc::from(Vec::<u16>::new())));
    }
    let s = format!("\n{} ", l.show(&arg(a, 0), true));
    if js_truthy(&arg(a, 1)) {
        to_file(l, &arg(a, 1), &lstr(&s))?;
    } else {
        print16(l, &s);
    }
    Ok(arg(a, 0))
}
fn f_terpri(l: &mut Lisp, _: &[V]) -> R<V> {
    print16(l, "\n");
    Ok(V::Nil)
}
fn f_prompt(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let s = strv(l, &arg(a, 0))?;
    print16(l, &rstr(&s));
    Ok(V::Nil)
}

// ---------------------------------------------------------------------------------------------- geometry

fn f_polar(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 3, 3)?;
    let p = pt(l, &arg(a, 0))?;
    let ang = num(l, &arg(a, 1))?;
    let d = num(l, &arg(a, 2))?;
    let (x, y) = (p.first().copied().unwrap_or(0.0), p.get(1).copied().unwrap_or(0.0));
    let mut q = vec![x + d * ang.jcos(), y + d * ang.jsin()];
    if let Some(z) = p.get(2) {
        q.push(*z);
    }
    Ok(pt_list(&q))
}
fn f_distance(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    let p = pt(l, &arg(a, 0))?;
    let q = pt(l, &arg(a, 1))?;
    let g = |v: &[f64], k: usize| v.get(k).copied().unwrap_or(0.0);
    let dz = if p.len() > 2 && q.len() > 2 { g(&p, 2) - g(&q, 2) } else { 0.0 };
    Ok(V::Real(hypot_n(&[g(&p, 0) - g(&q, 0), g(&p, 1) - g(&q, 1), dz])))
}
fn f_angle(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    let p = pt(l, &arg(a, 0))?;
    let q = pt(l, &arg(a, 1))?;
    let g = |v: &[f64], k: usize| v.get(k).copied().unwrap_or(0.0);
    let v = (g(&q, 1) - g(&p, 1)).jatan2(g(&q, 0) - g(&p, 0));
    Ok(V::Real(if v < 0.0 { v + std::f64::consts::PI * 2.0 } else { v }))
}
fn f_inters(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 4, 5)?;
    let p1 = pt(l, &arg(a, 0))?;
    let p2 = pt(l, &arg(a, 1))?;
    let p3 = pt(l, &arg(a, 2))?;
    let p4 = pt(l, &arg(a, 3))?;
    let onseg = a.len() < 5 || arg(a, 4).truthy();
    let g = |v: &[f64], k: usize| v.get(k).copied().unwrap_or(0.0);
    let (dx1, dy1, dx2, dy2) = (g(&p2, 0) - g(&p1, 0), g(&p2, 1) - g(&p1, 1), g(&p4, 0) - g(&p3, 0), g(&p4, 1) - g(&p3, 1));
    let den = dx1 * dy2 - dy1 * dx2;
    let scale = js::hypot(dx1, dy1) * js::hypot(dx2, dy2);
    if den.abs() < 1e-12 * (if scale != 0.0 && !scale.is_nan() { scale } else { 1.0 }) {
        return Ok(V::Nil);
    }
    let u = ((g(&p3, 0) - g(&p1, 0)) * dy2 - (g(&p3, 1) - g(&p1, 1)) * dx2) / den;
    let v = ((g(&p3, 0) - g(&p1, 0)) * dy1 - (g(&p3, 1) - g(&p1, 1)) * dx1) / den;
    if onseg && (u < -1e-10 || u > 1.0 + 1e-10 || v < -1e-10 || v > 1.0 + 1e-10) {
        return Ok(V::Nil);
    }
    let mut q = vec![g(&p1, 0) + u * dx1, g(&p1, 1) + u * dy1];
    if p1.len() > 2 && p2.len() > 2 {
        q.push(g(&p1, 2) + u * (g(&p2, 2) - g(&p1, 2)));
    }
    Ok(pt_list(&q))
}
fn f_trans_world(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 3, 4)?;
    pt(l, &arg(a, 0))?;
    Ok(arg(a, 0))
}

// ---------------------------------------------------------------------------------------------- environment

fn f_getenv(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let k = rstr(&upper16(&strv(l, &arg(a, 0))?));
    Ok(l.env.get(&k).map_or(V::Nil, |s| V::Str(Rc::clone(s))))
}
fn f_setenv(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    let k = rstr(&upper16(&strv(l, &arg(a, 0))?));
    let v = strv(l, &arg(a, 1))?;
    l.env.insert(k, v);
    Ok(arg(a, 1))
}

// ---------------------------------------------------------------------------------------------- files (in memory)

fn f_findfile(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let n = strv(l, &arg(a, 0))?;
    Ok(l.find_file(&n).map_or(V::Nil, |f| V::Str(f.name)))
}
fn f_open(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 3)?;
    let path = strv(l, &arg(a, 0))?;
    let m = lower16(&strv(l, &arg(a, 1))?);
    let mode = match m.first() {
        Some(0x72) => b'r',
        Some(0x77) => b'w',
        Some(0x61) => b'a',
        _ => return Err(LErr::new(format!("bad argument value: {}", l.show(&arg(a, 1), true)))),
    };
    let f = l.find_file(&path);
    if mode == b'r' && f.is_none() {
        return Ok(V::Nil);
    }
    let name = f.as_ref().map_or_else(|| Rc::clone(&path), |f| Rc::clone(&f.name));
    let buf = match (&f, mode) {
        (Some(f), b'r' | b'a') => f.text.to_vec(),
        _ => Vec::new(),
    };
    Ok(Lisp::file(name, mode, buf))
}
fn f_close(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let x = arg(a, 0);
    let V::File(f) = &x else { return Err(l.bad("filep", &x)) };
    let (open, mode, path, buf) = {
        let fh = f.try_borrow().map_err(|_| l.bad("filep", &x))?;
        (fh.open, fh.mode, Rc::clone(&fh.path), fh.buf.clone())
    };
    if open && mode != b'r' {
        l.put_file(&path, &buf);
    }
    if let Ok(mut fh) = f.try_borrow_mut() {
        fh.open = false;
    }
    Ok(V::Nil)
}
fn f_read_line(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 0, 1)?;
    let x = arg(a, 0);
    let V::File(f) = &x else { return Ok(V::Nil) };
    let Ok(mut fh) = f.try_borrow_mut() else { return Ok(V::Nil) };
    if fh.mode != b'r' || !fh.open || fh.pos >= fh.buf.len() {
        return Ok(V::Nil);
    }
    let nl = (fh.pos..fh.buf.len()).find(|&i| fh.buf.get(i) == Some(&0x0A)).unwrap_or(fh.buf.len());
    let mut line: Vec<u16> = fh.buf.get(fh.pos..nl).unwrap_or(&[]).to_vec();
    if line.last() == Some(&0x0D) {
        line.pop();
    }
    fh.pos = nl.saturating_add(1);
    drop(fh);
    l.mk_str(line)
}
fn f_read_char(_: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 0, 1)?;
    let x = arg(a, 0);
    let V::File(f) = &x else { return Ok(V::Nil) };
    let Ok(mut fh) = f.try_borrow_mut() else { return Ok(V::Nil) };
    if fh.mode != b'r' || fh.pos >= fh.buf.len() {
        return Ok(V::Nil);
    }
    let c = fh.buf.get(fh.pos).copied().unwrap_or(0);
    fh.pos = fh.pos.saturating_add(1);
    Ok(V::Int(i32::from(c)))
}
fn f_write_line(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 2)?;
    let s = strv(l, &arg(a, 0))?;
    if js_truthy(&arg(a, 1)) {
        let mut t = s.to_vec();
        t.push(0x0A);
        to_file(l, &arg(a, 1), &t)?;
    } else {
        print16(l, &rstr(&s));
    }
    Ok(V::Str(s))
}
fn f_write_char(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 2)?;
    let n = intv(l, &arg(a, 0))?;
    let u = u16::try_from(n & 0xffff).unwrap_or(0);
    if js_truthy(&arg(a, 1)) {
        to_file(l, &arg(a, 1), &[u])?;
    } else {
        print16(l, &String::from_utf16_lossy(&[u]));
    }
    Ok(arg(a, 0))
}
/// /\.[^.]*$/ in UTF-16: the last dot and what follows (the dot itself when nothing follows).
fn ext_of(s: &[u16]) -> Option<usize> {
    s.iter().rposition(|&c| c == 0x2E)
}
fn f_mktemp(l: &mut Lisp, a: &[V]) -> R<V> {
    let pat: Vec<u16> = if js_truthy(&arg(a, 0)) { strv(l, &arg(a, 0))?.to_vec() } else { lstr("$VL~~").to_vec() };
    let dir: Vec<u16> = if js_truthy(&arg(a, 1)) {
        let mut d = strv(l, &arg(a, 1))?.to_vec();
        if matches!(d.last(), Some(0x5C | 0x2F)) {
            d.pop();
        }
        d.push(0x2F);
        d
    } else {
        lstr("temp/").to_vec()
    };
    let ext: Vec<u16> = if js_truthy(&arg(a, 2)) { strv(l, &arg(a, 2))?.to_vec() } else { Vec::new() };
    let k = l.next_temp();
    let mut out = dir;
    match ext_of(&pat) {
        Some(i) => out.extend_from_slice(pat.get(..i).unwrap_or(&[])),
        None => out.extend_from_slice(&pat),
    }
    out.extend(format!("{k:03}").encode_utf16());
    if ext.is_empty() {
        if let Some(i) = ext_of(&pat) {
            out.extend_from_slice(pat.get(i..).unwrap_or(&[]));
        }
    } else {
        out.extend_from_slice(&ext);
    }
    l.mk_str(out)
}
fn f_file_delete(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let n = strv(l, &arg(a, 0))?;
    Ok(bool_v(l.delete_file(&n)))
}
fn f_file_rename(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    let from = strv(l, &arg(a, 0))?;
    let Some(f) = l.find_file(&from) else { return Ok(V::Nil) };
    let to = strv(l, &arg(a, 1))?;
    if l.find_file(&to).is_some() {
        return Ok(V::Nil);
    }
    l.delete_file(&f.name);
    l.put_file(&to, &f.text);
    Ok(V::Sym(S_T))
}
fn f_file_size(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let n = strv(l, &arg(a, 0))?;
    Ok(l.find_file(&n).map_or(V::Nil, |f| int(f.text.len() as f64)))
}
fn last_segment(s: &[u16]) -> Vec<u16> {
    let t: Vec<u16> = s.iter().map(|&c| if c == 0x5C { 0x2F } else { c }).collect();
    let i = t.iter().rposition(|&c| c == 0x2F).map_or(0, |i| i.saturating_add(1));
    t.get(i..).unwrap_or(&[]).to_vec()
}
fn f_filename_base(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let seg = last_segment(&strv(l, &arg(a, 0))?);
    let cut = ext_of(&seg).unwrap_or(seg.len());
    l.mk_str(seg.get(..cut).unwrap_or(&[]).to_vec())
}
fn f_filename_ext(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let seg = last_segment(&strv(l, &arg(a, 0))?);
    match ext_of(&seg) {
        Some(i) => l.mk_str(seg.get(i..).unwrap_or(&[]).to_vec()),
        None => Ok(V::Nil),
    }
}
fn f_filename_dir(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let s = strv(l, &arg(a, 0))?;
    match s.iter().rposition(|&c| c == 0x2F || c == 0x5C) {
        Some(i) => l.mk_str(s.get(..i).unwrap_or(&[]).to_vec()),
        None => Ok(V::Str(Rc::from(Vec::<u16>::new()))),
    }
}
fn f_load(l: &mut Lisp, a: Vec<V>) -> Ctl {
    if let Err(e) = arity(&a, 1, 2) {
        return Ctl::Throw(e);
    }
    let name = match strv(l, &arg(&a, 0)) {
        Ok(n) => n,
        Err(e) => return Ctl::Throw(e),
    };
    let f = l.find_file(&name).or_else(|| {
        let mut n2 = name.to_vec();
        n2.extend(".lsp".encode_utf16());
        l.find_file(&n2)
    });
    let Some(f) = f else {
        if a.len() > 1 {
            return ok(arg(&a, 1));
        }
        return Ctl::Throw(LErr::new(format!("LOAD failed: {}", l.show(&V::Str(name), true))));
    };
    match read_all(&f.text, &mut l.syms) {
        Ok(forms) => l.seq(forms.into()),
        Err(e) => Ctl::Throw(e),
    }
}

/// Install the built-ins, in builtins.js's order.
pub fn install(l: &mut Lisp) {
    use Bf::{Ctl as C, Special as Sp, Sync as S};
    for (n, f) in [
        ("QUOTE", sp_quote as fn(&mut Lisp, V) -> Ctl),
        ("FUNCTION", sp_function),
        ("SETQ", sp_setq),
        ("DEFUN", sp_defun),
        ("DEFUN-Q", sp_defun),
        ("LAMBDA", sp_lambda),
        ("IF", sp_if),
        ("COND", sp_cond),
        ("WHILE", sp_while),
        ("REPEAT", sp_repeat),
        ("FOREACH", sp_foreach),
        ("PROGN", sp_progn),
        ("AND", sp_and),
        ("OR", sp_or),
    ] {
        l.def(n, Sp(f));
    }
    let pi = l.fsym("PI");
    l.set(pi, V::Real(std::f64::consts::PI));
    let pause = l.fsym("PAUSE");
    l.set(pause, V::str("\\"));
    let sync: &[(&str, Plain)] = &[
        ("+", f_add),
        ("-", f_sub),
        ("*", f_mul),
        ("/", f_div),
        ("~", f_lognot),
        ("1+", f_inc),
        ("1-", f_dec),
        ("ABS", f_abs),
        ("FIX", f_fix),
        ("FLOAT", f_float),
        ("REM", f_rem),
        ("GCD", f_gcd),
        ("LCM", f_lcm),
        ("MIN", f_min),
        ("MAX", f_max),
        ("SQRT", f_sqrt),
        ("EXP", f_exp),
        ("LOG", f_log),
        ("EXPT", f_expt),
        ("SIN", f_sin),
        ("COS", f_cos),
        ("ATAN", f_atan),
        ("MINUSP", f_minusp),
        ("ZEROP", f_zerop),
        ("NUMBERP", f_numberp),
        ("BOOLE", f_boole),
        ("LOGAND", f_logand),
        ("LOGIOR", f_logior),
        ("LSH", f_lsh),
        ("=", f_numeq),
        ("/=", f_numne),
        ("<", f_lt),
        (">", f_gt),
        ("<=", f_le),
        (">=", f_ge),
        ("EQ", f_eq),
        ("EQUAL", f_equal),
        ("NOT", f_not),
        ("NULL", f_not),
        ("ATOM", f_atom),
        ("LISTP", f_listp),
        ("BOUNDP", f_boundp),
        ("TYPE", f_type),
    ];
    for (n, f) in sync {
        l.def(n, S(*f));
    }
    for (path, f) in CXR.iter().zip(CXR_FNS) {
        l.def(&format!("C{path}R"), S(f));
    }
    for (n, f) in [
        ("CONS", f_cons as fn(&mut Lisp, &[V]) -> R<V>),
        ("LIST", f_list),
        ("APPEND", f_append),
        ("REVERSE", f_reverse),
        ("LENGTH", f_length),
        ("NTH", f_nth),
        ("LAST", f_last),
        ("MEMBER", f_member),
        ("ASSOC", f_assoc),
        ("SUBST", f_subst),
        ("ACAD_STRLSORT", f_strlsort),
    ] {
        l.def(n, S(f));
    }
    l.def("APPLY", C(f_apply));
    l.def("MAPCAR", C(f_mapcar));
    l.def("EVAL", C(f_eval));
    l.def("SET", S(f_set));
    l.def("READ", S(f_read));
    l.def("ATOMS-FAMILY", S(f_atoms_family));
    l.def("EXIT", S(f_exit));
    l.def("QUIT", S(f_exit));
    for n in [
        "GC",
        "TRACE",
        "UNTRACE",
        "VL-LOAD-COM",
        "VL-LOAD-ALL",
        "VL-LOAD-REACTORS",
        "VL-ACAD-DEFUN",
        "VL-ACAD-UNDEFUN",
        "VL-PROPAGATE",
        "VL-ARX-IMPORT",
        "VL-DOC-EXPORT",
        "VL-DOC-IMPORT",
        "ACAD-PUSH-DBMOD",
        "ACAD-POP-DBMOD",
    ] {
        l.def(n, S(f_nil));
    }
    l.def("VER", S(f_ver));
    for (n, f) in [
        ("STRCAT", f_strcat as fn(&mut Lisp, &[V]) -> R<V>),
        ("STRLEN", f_strlen),
        ("SUBSTR", f_substr),
        ("STRCASE", f_strcase),
        ("ASCII", f_ascii),
        ("CHR", f_chr),
        ("ITOA", f_itoa),
        ("ATOI", f_atoi),
        ("ATOF", f_atof),
        ("WCMATCH", f_wcmatch),
        ("RTOS", f_rtos),
        ("ANGTOS", f_angtos),
        ("DISTOF", f_distof),
        ("ANGTOF", f_angtof),
        ("CVUNIT", f_cvunit),
        ("PRINC", f_princ),
        ("PRIN1", f_prin1),
        ("PRINT", f_print),
        ("TERPRI", f_terpri),
        ("PROMPT", f_prompt),
        ("POLAR", f_polar),
        ("DISTANCE", f_distance),
        ("ANGLE", f_angle),
        ("INTERS", f_inters),
        ("TRANS", f_trans_world),
        ("GETENV", f_getenv),
        ("SETENV", f_setenv),
        ("FINDFILE", f_findfile),
        ("OPEN", f_open),
        ("CLOSE", f_close),
        ("READ-LINE", f_read_line),
        ("READ-CHAR", f_read_char),
        ("WRITE-LINE", f_write_line),
        ("WRITE-CHAR", f_write_char),
        ("VL-FILENAME-MKTEMP", f_mktemp),
        ("VL-FILE-DELETE", f_file_delete),
        ("VL-FILE-RENAME", f_file_rename),
        ("VL-FILE-SIZE", f_file_size),
        ("VL-FILE-DIRECTORY-P", f_nil),
        ("VL-FILENAME-BASE", f_filename_base),
        ("VL-FILENAME-EXTENSION", f_filename_ext),
        ("VL-FILENAME-DIRECTORY", f_filename_dir),
    ] {
        l.def(n, S(f));
    }
    l.def("LOAD", C(f_load));
}
