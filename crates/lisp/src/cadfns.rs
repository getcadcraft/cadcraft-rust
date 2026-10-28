//! The LISP functions that reach the drawing (cadfns.js): input (initget, getpoint … entsel), selection sets
//! (ssget …), (command …), DXF lists (entget, entmod, entmake …), tables, system variables and the rest. Each calls
//! the drawing side ([`crate::host::Host`]) with plain data and turns the answer into LISP values.

use std::rc::Rc;

use crate::builtins::{arg, arity, bool_v, intv, js_truthy, lst, num, strv};
use crate::host::{EntselAns, GetAns, GetKind, GetOpts, HostErr, MakeAns, Pairs, Pv};
use crate::interp::{Bf, Ctl, LFut, Lisp, await_with};
use crate::syms::S_T;
use crate::value::{EId, LErr, R, V, from_vec, rstr, to_vec, upper16};

fn ok(v: V) -> Ctl {
    Ctl::Ret(v)
}
macro_rules! tri {
    ($e:expr) => {
        match $e {
            Ok(v) => v,
            Err(e) => return Ctl::Throw(e),
        }
    };
}

// ---- plain data ↔ LISP ----

/// Plain data → LISP (toLisp).
pub fn to_lisp(l: &mut Lisp, v: &Pv) -> V {
    to_lisp_d(l, v, 0)
}
fn to_lisp_d(l: &mut Lisp, v: &Pv, depth: u32) -> V {
    if depth > 10_000 {
        return V::Nil;
    }
    match v {
        Pv::Nil => V::Nil,
        Pv::T => V::Sym(S_T),
        Pv::Num(n) => V::Real(*n),
        Pv::Str(s) => V::str(s),
        Pv::List(items) => from_vec(items.iter().map(|x| to_lisp_d(l, x, depth.saturating_add(1))).collect(), V::Nil),
        Pv::Int(i) => V::Int(*i),
        Pv::E(id) => V::Ename(Rc::new(id.clone())),
        Pv::Ids(ids) => l.pickset(ids.clone()),
        Pv::Sym(s) => l.sym(s).map_or(V::Nil, V::Sym),
        _ => V::Nil,
    }
}
/// LISP → plain data (fromLisp).
pub fn from_lisp(l: &Lisp, x: &V) -> Pv {
    from_lisp_d(l, x, 0)
}
fn from_lisp_d(l: &Lisp, x: &V, depth: u32) -> Pv {
    if depth > 10_000 {
        return Pv::Nil;
    }
    match x {
        V::Nil => Pv::Nil,
        V::Sym(S_T) => Pv::T,
        V::Int(i) => Pv::Int(*i),
        V::Real(r) => Pv::Num(*r),
        V::Str(s) => Pv::Str(rstr(s)),
        V::Sym(s) => Pv::Sym(l.name(*s).to_string()),
        V::Ename(id) => Pv::E((**id).clone()),
        V::PickSet(p) => Pv::Ids(p.ids.try_borrow().map(|v| v.clone()).unwrap_or_default()),
        V::Cons(c) => {
            if let V::Int(code) = c.car
                && !matches!(c.cdr, V::Cons(_) | V::Nil)
            {
                return Pv::List(vec![Pv::Num(f64::from(code)), from_lisp_d(l, &c.cdr, depth.saturating_add(1))]);
            }
            Pv::List(to_vec(x).iter().map(|y| from_lisp_d(l, y, depth.saturating_add(1))).collect())
        }
        V::Vla(h) => Pv::Vla((**h).clone()),
        _ => Pv::Nil,
    }
}
/// A DXF list from the drawing → ((code . value) (10 x y z) …).
pub fn pairs_to_lisp(l: &mut Lisp, pairs: Option<Pairs>) -> V {
    let Some(pairs) = pairs else { return V::Nil };
    let mut out = Vec::with_capacity(pairs.len());
    for (code, v) in pairs {
        let item = match &v {
            Pv::List(xs) => V::cons(V::Int(code), from_vec(xs.iter().map(|n| V::Real(n.num().unwrap_or(f64::NAN))).collect(), V::Nil)),
            other => {
                let lv = to_lisp(l, other);
                V::cons(V::Int(code), lv)
            }
        };
        out.push(item);
    }
    from_vec(out, V::Nil)
}
/// A LISP DXF list → pairs; a value that is a list of numbers is a point.
pub fn lisp_to_pairs(l: &Lisp, x: &V) -> R<Pairs> {
    let mut out = Vec::new();
    for item in to_vec(&lst(l, x)?) {
        let V::Cons(c) = &item else { return Err(l.bad("dxf list", &item)) };
        let V::Int(code) = c.car else { return Err(l.bad("dxf list", &item)) };
        if let V::Cons(_) = &c.cdr {
            let a = to_vec(&c.cdr);
            if code == -3 {
                out.push((code, Pv::List(a.iter().map(|y| from_lisp(l, y)).collect())));
                continue;
            }
            if !a.iter().all(V::is_num) {
                return Err(l.bad("dxf list", &item));
            }
            out.push((code, Pv::List(a.iter().filter_map(V::num).map(Pv::Num).collect())));
        } else {
            out.push((code, from_lisp(l, &c.cdr)));
        }
    }
    Ok(out)
}
fn pt_of(l: &Lisp, x: &V) -> R<Vec<f64>> {
    let a = to_vec(&lst(l, x)?);
    if a.len() < 2 || !a.iter().all(V::is_num) {
        return Err(l.bad("2D/3D point", x));
    }
    Ok(a.iter().filter_map(V::num).collect())
}
fn is_pt(x: &V) -> bool {
    if !matches!(x, V::Cons(_)) {
        return false;
    }
    let a = to_vec(x);
    (2..=3).contains(&a.len()) && a.iter().all(V::is_num)
}
fn ename(l: &Lisp, x: &V) -> R<EId> {
    if let V::Ename(id) = x { Ok((**id).clone()) } else { Err(l.bad("lentityp", x)) }
}
fn pickset(l: &Lisp, x: &V) -> R<Rc<crate::value::PickSet>> {
    if let V::PickSet(p) = x { Ok(Rc::clone(p)) } else { Err(l.bad("lselsetp", x)) }
}

/// A drawing-side error → the LISP error (Esc → "Function cancelled").
fn herr(method: &str, e: HostErr) -> LErr {
    match e {
        HostErr::Cancel => LErr::cancel(),
        HostErr::Msg(m) => LErr::new(m),
        HostErr::Missing => LErr::new(format!("no drawing: {method}")),
    }
}
/// The drawing side, or "no drawing: <method>".
fn host(l: &Lisp, method: &str) -> R<Rc<dyn crate::host::Host>> {
    l.host.clone().ok_or_else(|| LErr::new(format!("no drawing: {method}")))
}
fn wait<T: 'static>(method: &'static str, fut: LFut<Result<T, HostErr>>, k: impl FnOnce(&mut Lisp, T) -> Ctl + 'static) -> Ctl {
    await_with(fut, move |l, r| match r {
        Ok(v) => k(l, v),
        Err(e) => Ctl::Throw(herr(method, e)),
    })
}

fn take_init(l: &mut Lisp) -> (i32, Option<String>) {
    match l.initget.take() {
        Some((f, kw)) => (f, kw.map(|k| rstr(&k))),
        None => (0, None),
    }
}
fn answer(l: &mut Lisp, r: GetAns) -> V {
    match r {
        GetAns::None => V::Nil,
        GetAns::Kw(k) | GetAns::Text(k) => V::str(&k),
        GetAns::Value(v) => to_lisp(l, &v),
    }
}

fn f_initget(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 0, 2)?;
    let (mut flags, mut kw) = (0, None);
    for x in a {
        match x {
            V::Int(i) => flags = *i,
            V::Str(s) => kw = Some(Rc::clone(s)),
            V::Nil => {}
            other => return Err(l.bad("stringp", other)),
        }
    }
    l.initget = Some((flags, kw));
    Ok(V::Nil)
}
fn pt_msg(l: &Lisp, a: &[V]) -> R<(Option<Vec<f64>>, Option<String>)> {
    let (mut base, mut msg) = (None, None);
    for x in a {
        match x {
            V::Str(s) => msg = Some(rstr(s)),
            V::Nil => {}
            other => base = Some(pt_of(l, other)?),
        }
    }
    Ok((base, msg))
}
fn getter(l: &mut Lisp, a: Vec<V>, kind: GetKind, takes_pt: bool) -> Ctl {
    tri!(arity(&a, 0, if takes_pt { 2 } else { 1 }));
    let (base, msg) = if takes_pt {
        tri!(pt_msg(l, &a))
    } else {
        let m = arg(&a, 0);
        (None, if m.is_nil() { None } else { Some(rstr(&tri!(strv(l, &m)))) })
    };
    let (flags, kw) = take_init(l);
    let h = tri!(host(l, "get"));
    wait("get", h.get(kind, GetOpts { msg, base, kw, flags }), |l, r| ok(answer(l, r)))
}
fn f_getpoint(l: &mut Lisp, a: Vec<V>) -> Ctl {
    getter(l, a, GetKind::Point, true)
}
fn f_getdist(l: &mut Lisp, a: Vec<V>) -> Ctl {
    getter(l, a, GetKind::Dist, true)
}
fn f_getangle(l: &mut Lisp, a: Vec<V>) -> Ctl {
    getter(l, a, GetKind::Angle, true)
}
fn f_getorient(l: &mut Lisp, a: Vec<V>) -> Ctl {
    getter(l, a, GetKind::Orient, true)
}
fn f_getreal(l: &mut Lisp, a: Vec<V>) -> Ctl {
    getter(l, a, GetKind::Real, false)
}
fn f_getint(l: &mut Lisp, a: Vec<V>) -> Ctl {
    getter(l, a, GetKind::Int, false)
}
fn f_getkword(l: &mut Lisp, a: Vec<V>) -> Ctl {
    getter(l, a, GetKind::Kword, false)
}
fn f_getcorner(l: &mut Lisp, a: Vec<V>) -> Ctl {
    tri!(arity(&a, 1, 2));
    let (flags, kw) = take_init(l);
    let base = tri!(pt_of(l, &arg(&a, 0)));
    let m = arg(&a, 1);
    let msg = if m.is_nil() { None } else { Some(rstr(&tri!(strv(l, &m)))) };
    let h = tri!(host(l, "get"));
    wait("get", h.get(GetKind::Corner, GetOpts { msg, base: Some(base), kw, flags }), |l, r| ok(answer(l, r)))
}
fn f_getstring(l: &mut Lisp, a: Vec<V>) -> Ctl {
    tri!(arity(&a, 0, 2));
    let mut msg = None;
    for x in &a {
        if let V::Str(s) = x {
            msg = Some(rstr(s));
        }
    }
    take_init(l);
    let h = tri!(host(l, "get"));
    wait("get", h.get(GetKind::Str, GetOpts { msg, ..GetOpts::default() }), |l, r| ok(answer(l, r)))
}
fn sel(l: &mut Lisp, a: Vec<V>, nested: bool) -> Ctl {
    tri!(arity(&a, 0, 2));
    let (_, kw) = take_init(l);
    let msg = if let V::Str(s) = arg(&a, 0) { Some(rstr(&s)) } else { None };
    let h = tri!(host(l, "entsel"));
    wait("entsel", h.entsel(msg, kw, nested), move |_, r| match r {
        EntselAns::None => ok(V::Nil),
        EntselAns::Kw(k) => ok(V::str(&k)),
        EntselAns::Pick { e, p, matrix, parents } => {
            let mut out = vec![V::Ename(Rc::new(e)), from_vec(p.iter().map(|x| V::Real(*x)).collect(), V::Nil)];
            if nested && let Some(m) = matrix {
                out.push(from_vec(m.iter().map(|row| from_vec(row.iter().map(|x| V::Real(*x)).collect(), V::Nil)).collect(), V::Nil));
                out.push(from_vec(parents.into_iter().map(|p| V::Ename(Rc::new(p))).collect(), V::Nil));
            }
            ok(from_vec(out, V::Nil))
        }
    })
}
fn f_entsel(l: &mut Lisp, a: Vec<V>) -> Ctl {
    sel(l, a, false)
}
fn f_nentsel(l: &mut Lisp, a: Vec<V>) -> Ctl {
    sel(l, a, true)
}

// ---- selection sets ----

fn f_ssget(l: &mut Lisp, a: Vec<V>) -> Ctl {
    let mut rest: &[V] = &a;
    let mut mode = None;
    if let Some(V::Str(s)) = a.first() {
        mode = Some(rstr(s));
        rest = a.get(1..).unwrap_or(&[]);
    }
    let mut pts: Vec<Vec<f64>> = Vec::new();
    let mut filter: Option<Pairs> = None;
    for x in rest {
        if x.is_nil() {
            continue;
        }
        if is_pt(x) {
            pts.push(tri!(pt_of(l, x)));
            continue;
        }
        if let V::Cons(c) = x {
            if let V::Cons(c2) = &c.car
                && matches!(c2.car, V::Int(_))
            {
                filter = Some(tri!(lisp_to_pairs(l, x)));
                continue;
            }
            if is_pt(&c.car) {
                for p in to_vec(x) {
                    pts.push(tri!(pt_of(l, &p)));
                }
                continue;
            }
        }
        return Ctl::Throw(l.bad("ssget", x));
    }
    let h = tri!(host(l, "ssget"));
    wait("ssget", h.ssget(mode, pts, filter), |l, r| match r {
        Some(ids) if !ids.is_empty() => ok(l.pickset(ids)),
        _ => ok(V::Nil),
    })
}
fn f_sslength(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let s = pickset(l, &arg(a, 0))?;
    let n = s.ids.try_borrow().map_or(0, |v| v.len());
    Ok(V::Int(i32::try_from(n).unwrap_or(i32::MAX)))
}
fn f_ssname(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    let s = pickset(l, &arg(a, 0))?;
    let i = match arg(a, 1) {
        V::Int(i) => f64::from(i),
        other => num(l, &other)?.trunc(),
    };
    let ids = s.ids.try_borrow().map(|v| v.clone()).unwrap_or_default();
    #[allow(clippy::cast_precision_loss)]
    let n = ids.len() as f64;
    if i >= 0.0 && i < n {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let k = i as usize;
        return Ok(ids.get(k).map_or(V::Nil, |id| V::Ename(Rc::new(id.clone()))));
    }
    Ok(V::Nil)
}
fn f_ssadd(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 0, 2)?;
    if a.is_empty() || arg(a, 0).is_nil() {
        return Ok(l.pickset(Vec::new()));
    }
    let id = ename(l, &arg(a, 0))?;
    if a.len() < 2 || arg(a, 1).is_nil() {
        return Ok(l.pickset(vec![id]));
    }
    let s = pickset(l, &arg(a, 1))?;
    if let Ok(mut ids) = s.ids.try_borrow_mut()
        && !ids.contains(&id)
    {
        ids.push(id);
    }
    Ok(V::PickSet(s))
}
fn f_ssdel(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    let id = ename(l, &arg(a, 0))?;
    let s = pickset(l, &arg(a, 1))?;
    let Ok(mut ids) = s.ids.try_borrow_mut() else { return Ok(V::Nil) };
    let Some(k) = ids.iter().position(|x| *x == id) else { return Ok(V::Nil) };
    ids.remove(k);
    drop(ids);
    Ok(V::PickSet(s))
}
fn f_ssmemb(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    let id = ename(l, &arg(a, 0))?;
    let s = pickset(l, &arg(a, 1))?;
    let has = s.ids.try_borrow().is_ok_and(|v| v.contains(&id));
    Ok(if has { arg(a, 0) } else { V::Nil })
}
fn f_ssnamex(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 2)?;
    let s = pickset(l, &arg(a, 0))?;
    let ids = s.ids.try_borrow().map(|v| v.clone()).unwrap_or_default();
    Ok(from_vec(ids.into_iter().map(|id| from_vec(vec![V::Int(0), V::Ename(Rc::new(id)), V::Int(0)], V::Nil)).collect(), V::Nil))
}
fn f_sssetfirst(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 2)?;
    let ids =
        if js_truthy(&arg(a, 1)) { pickset(l, &arg(a, 1))?.ids.try_borrow().map(|v| v.clone()).unwrap_or_default() } else { Vec::new() };
    let h = host(l, "sssetfirst")?;
    h.sssetfirst(&ids).map_err(|e| herr("sssetfirst", e))?;
    Ok(from_vec(vec![V::Nil, arg(a, 1)], V::Nil))
}
fn f_ssgetfirst(l: &mut Lisp, _: &[V]) -> R<V> {
    let h = host(l, "ssgetfirst")?;
    let r = h.ssgetfirst().map_err(|e| herr("ssgetfirst", e))?;
    let set = r.map_or(V::Nil, |ids| l.pickset(ids));
    Ok(from_vec(vec![V::Nil, set], V::Nil))
}

// ---- commands ----

fn flat(l: &Lisp, a: &[V]) -> Vec<Pv> {
    let mut out = Vec::new();
    for x in a {
        if let V::Cons(c) = x
            && !is_pt(x)
            && !matches!(c.car, V::Ename(_))
        {
            out.extend(to_vec(x).iter().map(|y| from_lisp(l, y)));
        } else {
            out.push(from_lisp(l, x));
        }
    }
    out
}
fn f_command(l: &mut Lisp, a: Vec<V>) -> Ctl {
    let args = flat(l, &a);
    let h = tri!(host(l, "command"));
    wait("command", h.command(args, false), |_, ()| ok(V::Nil))
}
fn f_command_s(l: &mut Lisp, a: Vec<V>) -> Ctl {
    let args = flat(l, &a);
    let h = tri!(host(l, "command"));
    wait("command", h.command(args, true), |_, ()| ok(V::Nil))
}
fn f_vl_cmdf(l: &mut Lisp, a: Vec<V>) -> Ctl {
    if let Some(V::Str(s)) = a.first()
        && !s.is_empty()
    {
        let h = tri!(host(l, "getcname"));
        let known = tri!(h.getcname(&rstr(s)).map_err(|e| herr("getcname", e)));
        if known.is_none() {
            return ok(V::Nil);
        }
    }
    let args = flat(l, &a);
    let h = tri!(host(l, "command"));
    wait("command", h.command(args, false), |_, ()| ok(V::Sym(S_T)))
}

// ---- system variables ----

fn vname(l: &Lisp, x: &V) -> R<String> {
    if let V::Sym(s) = x {
        return Ok(l.name(*s).to_string());
    }
    Ok(rstr(&strv(l, x)?))
}
fn f_getvar(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let n = vname(l, &arg(a, 0))?;
    let h = host(l, "getvar")?;
    let v = h.getvar(&n).map_err(|e| herr("getvar", e))?;
    Ok(to_lisp(l, &v))
}
fn f_trans(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 3, 4)?;
    let p = from_lisp(l, &arg(a, 0));
    let pts: Option<Vec<f64>> = match &p {
        Pv::List(xs) if xs.len() >= 2 => xs.iter().map(|x| if let Pv::Num(n) = x { Some(*n) } else { None }).collect(),
        _ => None,
    };
    let Some(pts) = pts else { return Err(l.bad("pointp", &arg(a, 0))) };
    let sys = |x: &V| -> f64 {
        match x {
            V::Int(i) => f64::from(*i),
            V::Real(r) => *r,
            V::Ename(_) => 0.0,
            other => {
                if matches!(from_lisp(l, other), Pv::List(_)) {
                    0.0
                } else {
                    -1.0
                }
            }
        }
    };
    let f = sys(&arg(a, 1));
    let t2 = sys(&arg(a, 2));
    if f < 0.0 || f > 3.0 {
        return Err(l.bad("fixnump", &arg(a, 1)));
    }
    if t2 < 0.0 || t2 > 3.0 {
        return Err(l.bad("fixnump", &arg(a, 2)));
    }
    let code = |x: f64| -> i32 {
        if x == 1.0 {
            1
        } else if x == 2.0 {
            2
        } else {
            0
        }
    };
    let h = host(l, "trans")?;
    let r = h.trans(&pts, code(f), code(t2), arg(a, 3).truthy()).map_err(|e| herr("trans", e))?;
    Ok(from_vec(r.into_iter().map(V::Real).collect(), V::Nil))
}
fn f_setvar(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    let name = vname(l, &arg(a, 0))?;
    let v = from_lisp(l, &arg(a, 1));
    let h = host(l, "setvar")?;
    let okv = h.setvar(&name, v).map_err(|e| herr("setvar", e))?;
    if !okv {
        let up = V::str(&rstr(&upper16(&crate::value::lstr(&name))));
        return Err(LErr::new(format!("variable setting rejected: {} {}", l.show(&up, true), l.show(&arg(a, 1), true))));
    }
    Ok(arg(a, 1))
}

// ---- objects ----

fn f_entget(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 2)?;
    let id = ename(l, &arg(a, 0))?;
    let h = host(l, "entget")?;
    let p = h.entget(&id).map_err(|e| herr("entget", e))?;
    Ok(pairs_to_lisp(l, p))
}
fn f_entmod(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let pairs = lisp_to_pairs(l, &arg(a, 0))?;
    let h = host(l, "entmod")?;
    let r = h.entmod(pairs).map_err(|e| herr("entmod", e))?;
    Ok(if r { arg(a, 0) } else { V::Nil })
}
fn make(l: &mut Lisp, a: &[V], as_name: bool) -> R<V> {
    arity(a, 0, 1)?;
    if a.is_empty() || arg(a, 0).is_nil() {
        return Ok(V::Nil);
    }
    let pairs = lisp_to_pairs(l, &arg(a, 0))?;
    let h = host(l, "entmake")?;
    match h.entmake(pairs).map_err(|e| herr("entmake", e))? {
        MakeAns::Error => Ok(V::Nil),
        MakeAns::Block(n) => Ok(V::str(&n)),
        MakeAns::E(id) if as_name => Ok(V::Ename(Rc::new(id))),
        _ => Ok(arg(a, 0)),
    }
}
fn f_entmake(l: &mut Lisp, a: &[V]) -> R<V> {
    make(l, a, false)
}
fn f_entmakex(l: &mut Lisp, a: &[V]) -> R<V> {
    make(l, a, true)
}
fn f_entdel(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let id = ename(l, &arg(a, 0))?;
    let h = host(l, "entdel")?;
    Ok(if h.entdel(&id).map_err(|e| herr("entdel", e))? { arg(a, 0) } else { V::Nil })
}
fn id_v(id: Option<EId>) -> V {
    id.map_or(V::Nil, |i| V::Ename(Rc::new(i)))
}
fn f_entlast(l: &mut Lisp, _: &[V]) -> R<V> {
    let h = host(l, "entlast")?;
    Ok(id_v(h.entlast().map_err(|e| herr("entlast", e))?))
}
fn f_entnext(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 0, 1)?;
    let id = if js_truthy(&arg(a, 0)) { Some(ename(l, &arg(a, 0))?) } else { None };
    let h = host(l, "entnext")?;
    Ok(id_v(h.entnext(id.as_ref()).map_err(|e| herr("entnext", e))?))
}
fn f_handent(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let s = rstr(&strv(l, &arg(a, 0))?);
    let h = host(l, "handent")?;
    Ok(id_v(h.handent(&s).map_err(|e| herr("handent", e))?))
}
fn f_entupd(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    ename(l, &arg(a, 0))?;
    Ok(arg(a, 0))
}

// ---- tables ----

fn f_tblnext(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 2)?;
    let t = rstr(&strv(l, &arg(a, 0))?);
    let h = host(l, "tblnext")?;
    let p = h.tblnext(&t, arg(a, 1).truthy()).map_err(|e| herr("tblnext", e))?;
    Ok(pairs_to_lisp(l, p))
}
fn f_tblsearch(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 3)?;
    let t = rstr(&strv(l, &arg(a, 0))?);
    let n = rstr(&strv(l, &arg(a, 1))?);
    let h = host(l, "tblsearch")?;
    let p = h.tblsearch(&t, &n, arg(a, 2).truthy()).map_err(|e| herr("tblsearch", e))?;
    Ok(pairs_to_lisp(l, p))
}
fn f_tblobjname(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    let t = rstr(&strv(l, &arg(a, 0))?);
    let n = rstr(&strv(l, &arg(a, 1))?);
    let h = host(l, "tblobjname")?;
    Ok(id_v(h.tblobjname(&t, &n).map_err(|e| herr("tblobjname", e))?))
}

// ---- misc ----

fn f_textbox(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let pairs = lisp_to_pairs(l, &arg(a, 0))?;
    let h = host(l, "textbox")?;
    match h.textbox(pairs).map_err(|e| herr("textbox", e))? {
        None => Ok(V::Nil),
        Some([p, q]) => Ok(from_vec(
            vec![from_vec(p.iter().map(|x| V::Real(*x)).collect(), V::Nil), from_vec(q.iter().map(|x| V::Real(*x)).collect(), V::Nil)],
            V::Nil,
        )),
    }
}
fn f_osnap(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    let p = pt_of(l, &arg(a, 0))?;
    let m = rstr(&strv(l, &arg(a, 1))?);
    let h = host(l, "osnap")?;
    Ok(h.osnap(&p, &m).map_err(|e| herr("osnap", e))?.map_or(V::Nil, |q| from_vec(q.into_iter().map(V::Real).collect(), V::Nil)))
}
fn f_alert(l: &mut Lisp, a: Vec<V>) -> Ctl {
    tri!(arity(&a, 1, 1));
    let s = rstr(&tri!(strv(l, &arg(&a, 0))));
    let h = tri!(host(l, "alert"));
    wait("alert", h.alert(&s), |_, ()| ok(V::Nil))
}
fn f_getcname(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let s = rstr(&strv(l, &arg(a, 0))?);
    let h = host(l, "getcname")?;
    Ok(h.getcname(&s).map_err(|e| herr("getcname", e))?.map_or(V::Nil, |n| V::str(&n)))
}
fn f_getfiled(l: &mut Lisp, a: Vec<V>) -> Ctl {
    tri!(arity(&a, 4, 4));
    let Some(h) = l.host.clone().filter(|h| h.has("getfiled")) else { return ok(V::Nil) };
    let title = rstr(&tri!(strv(l, &arg(&a, 0))));
    let name = rstr(&tri!(strv(l, &arg(&a, 1))));
    let ext = rstr(&tri!(strv(l, &arg(&a, 2))));
    let flags = tri!(intv(l, &arg(&a, 3)));
    wait("getfiled", h.getfiled(&title, &name, &ext, flags), |_, r| ok(r.map_or(V::Nil, |s| V::str(&s))))
}
fn f_colordlg(l: &mut Lisp, a: Vec<V>) -> Ctl {
    tri!(arity(&a, 1, 2));
    let Some(h) = l.host.clone().filter(|h| h.has("colorDialog")) else { return ok(arg(&a, 0)) };
    let c = tri!(intv(l, &arg(&a, 0)));
    wait("colorDialog", h.color_dialog(c), |_, r| ok(r.map_or(V::Nil, V::Int)))
}
fn f_truecolordlg(_: &mut Lisp, a: &[V]) -> R<V> {
    Ok(arg(a, 0))
}
fn f_redraw(l: &mut Lisp, _: &[V]) -> R<V> {
    let h = host(l, "redraw")?;
    h.redraw().map_err(|e| herr("redraw", e))?;
    Ok(V::Nil)
}
fn f_quiet(_: &mut Lisp, _: &[V]) -> R<V> {
    Ok(V::Nil)
}
fn f_layoutlist(l: &mut Lisp, _: &[V]) -> R<V> {
    let h = host(l, "layoutNames")?;
    let names = h.layout_names().map_err(|e| herr("layoutNames", e))?;
    Ok(from_vec(names.iter().map(|n| V::str(n)).collect(), V::Nil))
}
fn f_regapp(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let n = rstr(&upper16(&strv(l, &arg(a, 0))?));
    let valid =
        (1..=255).contains(&n.chars().count()) && n.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || "_$-".contains(c));
    if !valid || l.appids.contains(&n) {
        return Ok(V::Nil);
    }
    l.appids.insert(n);
    Ok(arg(a, 0))
}
fn f_vports(_: &mut Lisp, _: &[V]) -> R<V> {
    let p = |a: f64, b: f64| from_vec(vec![V::Real(a), V::Real(b)], V::Nil);
    Ok(from_vec(vec![from_vec(vec![V::Int(1), p(0.0, 0.0), p(1.0, 1.0)], V::Nil)], V::Nil))
}

/// Install, in cadfns.js's order.
pub fn install(l: &mut Lisp) {
    use Bf::{Ctl as C, Sync as S};
    l.def("INITGET", S(f_initget));
    l.def("GETPOINT", C(f_getpoint));
    l.def("GETCORNER", C(f_getcorner));
    l.def("GETDIST", C(f_getdist));
    l.def("GETANGLE", C(f_getangle));
    l.def("GETORIENT", C(f_getorient));
    l.def("GETREAL", C(f_getreal));
    l.def("GETINT", C(f_getint));
    l.def("GETKWORD", C(f_getkword));
    l.def("GETSTRING", C(f_getstring));
    l.def("ENTSEL", C(f_entsel));
    l.def("NENTSEL", C(f_nentsel));
    l.def("NENTSELP", C(f_nentsel));
    l.def("SSGET", C(f_ssget));
    for (n, f) in [
        ("SSLENGTH", f_sslength as fn(&mut Lisp, &[V]) -> R<V>),
        ("SSNAME", f_ssname),
        ("SSADD", f_ssadd),
        ("SSDEL", f_ssdel),
        ("SSMEMB", f_ssmemb),
        ("SSNAMEX", f_ssnamex),
        ("SSSETFIRST", f_sssetfirst),
        ("SSGETFIRST", f_ssgetfirst),
    ] {
        l.def(n, S(f));
    }
    l.def("COMMAND", C(f_command));
    l.def("COMMAND-S", C(f_command_s));
    l.def("VL-CMDF", C(f_vl_cmdf));
    for (n, f) in [
        ("GETVAR", f_getvar as fn(&mut Lisp, &[V]) -> R<V>),
        ("TRANS", f_trans),
        ("SETVAR", f_setvar),
        ("ENTGET", f_entget),
        ("ENTMOD", f_entmod),
        ("ENTMAKE", f_entmake),
        ("ENTMAKEX", f_entmakex),
        ("ENTDEL", f_entdel),
        ("ENTLAST", f_entlast),
        ("ENTNEXT", f_entnext),
        ("HANDENT", f_handent),
        ("ENTUPD", f_entupd),
        ("TBLNEXT", f_tblnext),
        ("TBLSEARCH", f_tblsearch),
        ("TBLOBJNAME", f_tblobjname),
        ("TEXTBOX", f_textbox),
        ("OSNAP", f_osnap),
    ] {
        l.def(n, S(f));
    }
    l.def("ALERT", C(f_alert));
    l.def("GETCNAME", S(f_getcname));
    l.def("GETFILED", C(f_getfiled));
    l.def("ACAD_COLORDLG", C(f_colordlg));
    l.def("ACAD_TRUECOLORDLG", S(f_truecolordlg));
    l.def("REDRAW", S(f_redraw));
    for n in ["REGEN", "GRDRAW", "GRVECS", "GRTEXT", "GRCLEAR", "MENUCMD", "MENUGROUP", "HELP", "SETFUNHELP", "STARTAPP", "SETVIEW"] {
        l.def(n, S(f_quiet));
    }
    l.def("LAYOUTLIST", S(f_layoutlist));
    l.def("REGAPP", S(f_regapp));
    l.def("VPORTS", S(f_vports));
    let _ = bool_v;
}
