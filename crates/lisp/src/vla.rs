//! The drawing as objects: vla- / vlax- functions (vla.js). A VLA object is a handle { kind, key, tag }; every
//! property or method is one call to the drawing side ([`crate::host::Host::vla`]). Variants and safe arrays live
//! here.

use std::cell::RefCell;
use std::rc::Rc;

use crate::builtins::{MANY, arg, arity, bool_v, intv, lst, strv, symv};
use crate::host::{HostErr, Pv, VlaOp};
use crate::interp::{Bf, Ctl, Lisp};
use crate::syms::{S_T, S_VFALSE, S_VNULL, S_VTRUE};
use crate::value::{Builtin, Imp, LErr, R, SafeArray, V, VKey, Variant, VlaH, from_vec, rstr, to_i32, to_vec};

pub const VT_EMPTY: i32 = 0;
pub const VT_INTEGER: i32 = 2;
pub const VT_LONG: i32 = 3;
pub const VT_SINGLE: i32 = 4;
pub const VT_DOUBLE: i32 = 5;
pub const VT_STRING: i32 = 8;
pub const VT_OBJECT: i32 = 9;
pub const VT_BOOLEAN: i32 = 11;
pub const VT_VARIANT: i32 = 12;
pub const VT_ARRAY: i32 = 8192;

fn iface(kind: &str) -> &'static str {
    match kind {
        "App" => "IAcadApplication",
        "Doc" => "IAcadDocument",
        "Blocks" => "IAcadBlocks",
        "Layers" => "IAcadLayers",
        "Layouts" => "IAcadLayouts",
        "Linetypes" => "IAcadLineTypes",
        "TextStyles" => "IAcadTextStyles",
        "DimStyles" => "IAcadDimStyles",
        "Views" => "IAcadViews",
        "Documents" => "IAcadDocuments",
        "Block" => "IAcadBlock",
        "Layer" => "IAcadLayer",
        "Layout" => "IAcadLayout",
        "Linetype" => "IAcadLineType",
        "TextStyle" => "IAcadTextStyle",
        "DimStyle" => "IAcadDimStyle",
        "View" => "IAcadView",
        "Ent" => "IAcadEntity",
        "Att" => "IAcadAttributeReference",
        _ => "IAcadObject",
    }
}

/// `#<VLA-OBJECT IAcadEntity 000000004e1f2a3b>`: a hash of the handle.
pub fn show_vla(h: &VlaH) -> String {
    let text = format!("{}{}{}", h.kind, h.key.as_ref().map(VKey::text).unwrap_or_default(), h.tag.clone().unwrap_or_default());
    let mut n: u32 = 0;
    for c in text.chars() {
        let mut buf = [0u16; 2];
        let unit = c.encode_utf16(&mut buf).first().copied().unwrap_or(0);
        n = n.wrapping_mul(31).wrapping_add(u32::from(unit));
    }
    format!("#<VLA-OBJECT {} {:016x}>", iface(&h.kind), n)
}
/// Two handles for the same object.
pub fn same(a: &VlaH, b: &VlaH) -> bool {
    a.kind == b.kind
        && a.key.as_ref().map(VKey::text).unwrap_or_else(|| "undefined".into())
            == b.key.as_ref().map(VKey::text).unwrap_or_else(|| "undefined".into())
        && a.tag == b.tag
}

/// A host value → LISP as vla-get-* returns it (`raw` = vlax-get: lists and -1 / 0 instead of variants and
/// :vlax-true).
pub fn from_host(l: &Lisp, v: &Pv, raw: bool) -> V {
    match v {
        Pv::Num(n) => V::Real(*n),
        Pv::Str(s) => V::str(s),
        Pv::Int(i) => V::Int(*i),
        Pv::Vla(h) => V::Vla(Rc::new(h.clone())),
        Pv::Bool(b) => {
            if raw {
                V::Int(if *b { -1 } else { 0 })
            } else if *b {
                V::Sym(S_VTRUE)
            } else {
                V::Sym(S_VFALSE)
            }
        }
        Pv::E(id) => V::Ename(Rc::new(id.clone())),
        Pv::Arr(items, vt) => {
            let vals: Vec<V> = items
                .iter()
                .map(|x| match x {
                    Pv::Vla(h) => V::Vla(Rc::new(h.clone())),
                    Pv::Int(i) => V::Int(*i),
                    Pv::Num(n) => V::Real(*n),
                    Pv::Str(s) => V::str(s),
                    _ => V::Nil,
                })
                .collect();
            if raw {
                return from_vec(vals, V::Nil);
            }
            let t = vt.unwrap_or(VT_DOUBLE);
            let hi = i32::try_from(vals.len()).unwrap_or(i32::MAX).saturating_sub(1);
            let sa = V::SafeArray(Rc::new(RefCell::new(SafeArray { vt: t, bounds: vec![(0, hi)], data: vals })));
            let _ = l;
            V::Variant(Rc::new(Variant { vt: VT_ARRAY | t, value: sa }))
        }
        _ => V::Nil,
    }
}
/// LISP → a host value for vla-put-* and method arguments.
pub fn to_host(l: &Lisp, x: &V) -> Pv {
    to_host_d(l, x, 0)
}
fn to_host_d(l: &Lisp, x: &V, depth: u32) -> Pv {
    if depth > 10_000 {
        return Pv::Nil;
    }
    match x {
        V::Nil => Pv::Nil,
        V::Sym(S_VFALSE) => Pv::Bool(false),
        V::Sym(S_VTRUE) | V::Sym(S_T) => Pv::Bool(true),
        V::Int(i) => Pv::Int(*i),
        V::Real(r) => Pv::Num(*r),
        V::Str(s) => Pv::Str(rstr(s)),
        V::Vla(h) => Pv::Vla((**h).clone()),
        V::Variant(v) => to_host_d(l, &v.value, depth.saturating_add(1)),
        V::SafeArray(s) => {
            let data = s.try_borrow().map(|s| s.data.clone()).unwrap_or_default();
            Pv::Arr(data.iter().map(|y| to_host_d(l, y, depth.saturating_add(1))).collect(), None)
        }
        V::Ename(id) => Pv::E((**id).clone()),
        V::Cons(_) => Pv::Arr(to_vec(x).iter().map(|y| to_host_d(l, y, depth.saturating_add(1))).collect(), None),
        V::Sym(s) => Pv::Sym(l.name(*s).to_string()),
        _ => Pv::Nil,
    }
}

fn obj(l: &Lisp, x: &V) -> R<Rc<VlaH>> {
    if let V::Vla(h) = x { Ok(Rc::clone(h)) } else { Err(LErr::new(format!("bad argument type: VLA-OBJECT {}", l.show(x, true)))) }
}
fn prop_name(l: &Lisp, x: &V) -> R<String> {
    if let V::Sym(s) = x {
        return Ok(l.name(*s).to_string());
    }
    Ok(rstr(&strv(l, x)?))
}

/// One call to the drawing side's objects.
pub fn vla(l: &Lisp, op: VlaOp) -> R<Pv> {
    let Some(h) = &l.host else { return Err(LErr::new("no drawing: vla")) };
    match h.vla(op) {
        Ok(v) => Ok(v),
        Err(HostErr::Cancel) => Err(LErr::cancel()),
        Err(HostErr::Msg(m)) => Err(LErr::new(m)),
        Err(HostErr::Missing) => Err(LErr::new("no drawing: vla")),
    }
}

/// A function made on first use: vla-get-<property>, vla-put-<property>, vla-<method>.
pub fn auto_fn(name: &str) -> Option<Builtin> {
    let imp = if name.strip_prefix("VLA-GET-").is_some_and(|p| !p.is_empty()) {
        Imp::VlaGet
    } else if name.strip_prefix("VLA-PUT-").is_some_and(|p| !p.is_empty()) {
        Imp::VlaPut
    } else if name.strip_prefix("VLA-").is_some_and(|p| !p.is_empty()) {
        Imp::VlaMethod
    } else {
        return None;
    };
    Some(Builtin { name: name.to_string(), imp, special: false })
}

/// Run an auto-made function.
pub fn call_auto(l: &mut Lisp, imp: Imp, name: &str, a: Vec<V>) -> Ctl {
    let r = match imp {
        Imp::VlaGet => (|| {
            arity(&a, 1, 1)?;
            let p = name.get(8..).unwrap_or("").to_string();
            let o = obj(l, &arg(&a, 0))?;
            Ok(from_host(l, &vla(l, VlaOp::Get((*o).clone(), p))?, false))
        })(),
        Imp::VlaPut => (|| {
            arity(&a, 2, 2)?;
            let p = name.get(8..).unwrap_or("").to_string();
            let o = obj(l, &arg(&a, 0))?;
            let v = to_host(l, &arg(&a, 1));
            vla(l, VlaOp::Put((*o).clone(), p, v))?;
            Ok(V::Nil)
        })(),
        _ => (|| {
            let m = name.get(4..).unwrap_or("").to_string();
            let o = obj(l, &arg(&a, 0))?;
            method(l, &o, &m, a.get(1..).unwrap_or(&[]), false)
        })(),
    };
    match r {
        Ok(v) => Ctl::Ret(v),
        Err(e) => Ctl::Throw(e),
    }
}

fn method(l: &mut Lisp, o: &VlaH, meth: &str, args: &[V], raw: bool) -> R<V> {
    let m = meth.to_uppercase();
    if m == "GETBOUNDINGBOX" {
        let r = vla(l, VlaOp::Call(o.clone(), m, Vec::new()))?;
        let Pv::Bbox(mn, mx) = r else { return Ok(V::Nil) };
        let out = |p: [f64; 3]| -> V {
            if raw {
                from_vec(p.iter().map(|x| V::Real(*x)).collect(), V::Nil)
            } else {
                V::SafeArray(Rc::new(RefCell::new(SafeArray {
                    vt: VT_DOUBLE,
                    bounds: vec![(0, 2)],
                    data: p.iter().map(|x| V::Real(*x)).collect(),
                })))
            }
        };
        if let Some(V::Sym(s)) = args.first() {
            l.set(*s, out(mn));
        }
        if let Some(V::Sym(s)) = args.get(1) {
            l.set(*s, out(mx));
        }
        return Ok(V::Nil);
    }
    let hv: Vec<Pv> = args.iter().map(|x| to_host(l, x)).collect();
    let r = vla(l, VlaOp::Call(o.clone(), m, hv))?;
    Ok(from_host(l, &r, raw))
}

fn f_acad(_: &mut Lisp, _: &[V]) -> R<V> {
    Ok(V::Vla(Rc::new(VlaH { kind: "App".into(), key: None, tag: None })))
}
fn f_get_property(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    let o = obj(l, &arg(a, 0))?;
    let p = prop_name(l, &arg(a, 1))?.to_uppercase();
    Ok(from_host(l, &vla(l, VlaOp::Get((*o).clone(), p))?, false))
}
fn f_put_property(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 3, 3)?;
    let o = obj(l, &arg(a, 0))?;
    let p = prop_name(l, &arg(a, 1))?.to_uppercase();
    let v = to_host(l, &arg(a, 2));
    vla(l, VlaOp::Put((*o).clone(), p, v))?;
    Ok(V::Nil)
}
fn f_get(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    let o = obj(l, &arg(a, 0))?;
    let p = prop_name(l, &arg(a, 1))?.to_uppercase();
    Ok(from_host(l, &vla(l, VlaOp::Get((*o).clone(), p))?, true))
}
fn f_invoke_method(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, MANY)?;
    let o = obj(l, &arg(a, 0))?;
    let m = prop_name(l, &arg(a, 1))?;
    method(l, &o, &m, a.get(2..).unwrap_or(&[]), false)
}
fn f_invoke(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, MANY)?;
    let o = obj(l, &arg(a, 0))?;
    let m = prop_name(l, &arg(a, 1))?;
    method(l, &o, &m, a.get(2..).unwrap_or(&[]), true)
}
fn as_bool(p: &Pv) -> bool {
    match p {
        Pv::Nil | Pv::Bool(false) => false,
        Pv::Num(n) => *n != 0.0 && !n.is_nan(),
        Pv::Str(s) => !s.is_empty(),
        _ => true,
    }
}
fn f_property_available(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 3)?;
    let o = obj(l, &arg(a, 0))?;
    let p = prop_name(l, &arg(a, 1))?.to_uppercase();
    let r = vla(l, VlaOp::Has((*o).clone(), p, arg(a, 2).truthy()))?;
    Ok(bool_v(as_bool(&r)))
}
fn f_method_applicable(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    let o = obj(l, &arg(a, 0))?;
    let p = prop_name(l, &arg(a, 1))?.to_uppercase();
    let r = vla(l, VlaOp::Can((*o).clone(), p))?;
    Ok(bool_v(as_bool(&r)))
}
fn f_ename_to_vla(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let V::Ename(id) = arg(a, 0) else { return Err(LErr::new(format!("bad argument type: lentityp {}", l.show(&arg(a, 0), true)))) };
    Ok(from_host(l, &vla(l, VlaOp::FromEname((*id).clone()))?, false))
}
fn f_vla_to_ename(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let o = obj(l, &arg(a, 0))?;
    match vla(l, VlaOp::ToEname((*o).clone()))? {
        Pv::E(id) => Ok(V::Ename(Rc::new(id))),
        _ => Ok(V::Nil),
    }
}
fn f_erased(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let o = obj(l, &arg(a, 0))?;
    let r = vla(l, VlaOp::Erased((*o).clone()))?;
    Ok(bool_v(as_bool(&r)))
}
fn f_nil(_: &mut Lisp, _: &[V]) -> R<V> {
    Ok(V::Nil)
}
fn f_t(_: &mut Lisp, _: &[V]) -> R<V> {
    Ok(V::Sym(S_T))
}

fn sp_vlax_for(l: &mut Lisp, form: V) -> Ctl {
    let f = to_vec(&form);
    let v = f.first().cloned().unwrap_or(V::Nil);
    let var = match symv(l, &v) {
        Ok(s) => s,
        Err(e) => return Ctl::Throw(e),
    };
    let coll_form = f.get(1).cloned().unwrap_or(V::Nil);
    let body: Rc<[V]> = f.get(2..).unwrap_or(&[]).to_vec().into();
    l.eval_then(coll_form, move |l, coll| {
        let o = match obj(l, &coll) {
            Ok(o) => o,
            Err(e) => return Ctl::Throw(e),
        };
        let items = match vla(l, VlaOp::Items((*o).clone())) {
            Ok(Pv::List(items)) => items,
            Ok(_) => Vec::new(),
            Err(e) => return Ctl::Throw(e),
        };
        let handles: Vec<V> = items.into_iter().filter_map(|p| if let Pv::Vla(h) = p { Some(V::Vla(Rc::new(h))) } else { None }).collect();
        let old = l.raw(var);
        l.push_restore(vec![(var, old)]);
        for_step(l, var, Rc::new(handles), 0, Rc::clone(&body), V::Nil)
    })
}
fn for_step(l: &mut Lisp, var: crate::value::Sym, items: Rc<Vec<V>>, i: usize, body: Rc<[V]>, last: V) -> Ctl {
    let Some(h) = items.get(i).cloned() else { return Ctl::Ret(last) };
    l.set(var, h);
    let b2 = Rc::clone(&body);
    l.seq_then(body, move |l, v| for_step(l, var, items, i.saturating_add(1), b2, v))
}

// ---- variants and safe arrays ----

fn type_of(x: &V) -> i32 {
    match x {
        V::Nil => VT_EMPTY,
        V::Int(_) => VT_LONG,
        V::Real(_) => VT_DOUBLE,
        V::Str(_) => VT_STRING,
        V::Sym(S_VTRUE) | V::Sym(S_VFALSE) => VT_BOOLEAN,
        V::Vla(_) => VT_OBJECT,
        V::SafeArray(s) => VT_ARRAY | s.try_borrow().map_or(0, |s| s.vt),
        _ => VT_VARIANT,
    }
}
fn variant(vt: i32, value: V) -> V {
    V::Variant(Rc::new(Variant { vt, value }))
}
fn f_make_variant(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 0, 2)?;
    let v = arg(a, 0);
    let t = if a.len() > 1 && !arg(a, 1).is_nil() { intv(l, &arg(a, 1))? } else { type_of(&v) };
    Ok(variant(t, v))
}
fn f_variant_value(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    match arg(a, 0) {
        V::Variant(v) => Ok(v.value.clone()),
        x => Err(LErr::new(format!("bad argument type: variantp {}", l.show(&x, true)))),
    }
}
fn f_variant_type(_: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    Ok(V::Int(if let V::Variant(v) = arg(a, 0) { v.vt } else { VT_EMPTY }))
}
fn f_variant_change(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    let V::Variant(var) = arg(a, 0) else { return Err(LErr::new(format!("bad argument type: variantp {}", l.show(&arg(a, 0), true)))) };
    let to = intv(l, &arg(a, 1))?;
    let v = var.value.clone();
    let as_num = |v: &V| match v {
        V::Int(i) => f64::from(*i),
        V::Real(r) => *r,
        V::Str(s) => parse_float(&rstr(s)),
        _ => f64::NAN,
    };
    if to == VT_STRING {
        let s = match &v {
            V::Int(i) => i.to_string(),
            V::Real(r) => cc_cad::js::num_to_string(*r),
            other => l.show(other, false),
        };
        return Ok(variant(to, V::str(&s)));
    }
    if to == VT_DOUBLE || to == VT_SINGLE {
        let n = as_num(&v);
        return Ok(if n.is_finite() { variant(to, V::Real(n)) } else { V::Nil });
    }
    if to == VT_INTEGER || to == VT_LONG {
        let n = as_num(&v);
        return Ok(if n.is_finite() { variant(to, V::Int(to_i32(cc_cad::js::round(n)))) } else { V::Nil });
    }
    if to == VT_BOOLEAN {
        let t = v.truthy() && !matches!(v, V::Sym(S_VFALSE)) && v.num() != Some(0.0);
        return Ok(variant(to, V::Sym(if t { S_VTRUE } else { S_VFALSE })));
    }
    Ok(variant(to, v))
}
/// JS `parseFloat`.
fn parse_float(s: &str) -> f64 {
    let t = cc_cad::js::trim_start(s);
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
    t.get(..i).map_or(f64::NAN, cc_cad::js::str_to_number)
}
fn sa(l: &Lisp, x: &V) -> R<Rc<RefCell<SafeArray>>> {
    match x {
        V::Variant(v) => {
            if let V::SafeArray(s) = &v.value {
                return Ok(Rc::clone(s));
            }
        }
        V::SafeArray(s) => return Ok(Rc::clone(s)),
        _ => {}
    }
    Err(LErr::new(format!("bad argument type: safearrayp {}", l.show(x, true))))
}
fn f_make_safearray(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, MANY)?;
    let t = intv(l, &arg(a, 0))?;
    let mut bounds = Vec::new();
    for b in a.iter().skip(1) {
        let V::Cons(c) = b else { return Err(LErr::new(format!("bad argument type: consp {}", l.show(b, true)))) };
        bounds.push((intv(l, &c.car)?, intv(l, &c.cdr)?));
    }
    let size = bounds.iter().fold(1.0f64, |n, (lo, hi)| n * (f64::from(*hi) - f64::from(*lo) + 1.0));
    #[allow(clippy::cast_precision_loss)]
    let cap = l.limits.array as f64;
    if !(0.0..=cap).contains(&size) {
        return Err(LErr::new("Invalid array length"));
    }
    let blank = if t == VT_STRING {
        V::Str(Rc::from(Vec::<u16>::new()))
    } else if t == VT_DOUBLE || t == VT_SINGLE {
        V::Real(0.0)
    } else if t == VT_OBJECT {
        V::Nil
    } else {
        V::Int(0)
    };
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let n = size as usize;
    Ok(V::SafeArray(Rc::new(RefCell::new(SafeArray { vt: t, bounds, data: vec![blank; n] }))))
}
fn size_of(s: &SafeArray) -> f64 {
    s.bounds.iter().fold(1.0f64, |n, (lo, hi)| n * (f64::from(*hi) - f64::from(*lo) + 1.0))
}
fn f_safearray_fill(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    let s = sa(l, &arg(a, 0))?;
    let vals = to_vec(&lst(l, &arg(a, 1))?);
    let mut st = s.try_borrow_mut().map_err(|_| LErr::new("safearray in use"))?;
    let flat: Vec<V> = if st.bounds.len() > 1 { vals.iter().flat_map(to_vec).collect() } else { vals };
    #[allow(clippy::cast_precision_loss)]
    let n = flat.len() as f64;
    if n > size_of(&st) {
        return Err(LErr::new("too many elements"));
    }
    let vt = st.vt;
    for (i, v) in flat.into_iter().enumerate() {
        let v = if vt == VT_DOUBLE
            && let V::Int(k) = v
        {
            V::Real(f64::from(k))
        } else {
            v
        };
        if let Some(slot) = st.data.get_mut(i) {
            *slot = v;
        } else {
            st.data.push(v);
        }
    }
    drop(st);
    Ok(V::SafeArray(s))
}
fn f_safearray_list(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let s = sa(l, &arg(a, 0))?;
    let st = s.try_borrow().map_err(|_| LErr::new("safearray in use"))?;
    if st.bounds.len() < 2 {
        return Ok(from_vec(st.data.clone(), V::Nil));
    }
    let (lo, hi) = st.bounds.get(1).copied().unwrap_or((0, 0));
    let cols = usize::try_from(i64::from(hi).saturating_sub(i64::from(lo)).saturating_add(1)).unwrap_or(0).max(1);
    let rows: Vec<V> = st.data.chunks(cols).map(|r| from_vec(r.to_vec(), V::Nil)).collect();
    Ok(from_vec(rows, V::Nil))
}
fn idx(l: &Lisp, s: &SafeArray, ks: &[V]) -> R<usize> {
    let mut k: i64 = 0;
    for (dim, (lo, hi)) in s.bounds.iter().enumerate() {
        let i = intv(l, &ks.get(dim).cloned().unwrap_or(V::Nil))?;
        if i < *lo || i > *hi {
            return Err(LErr::new(format!("bad argument value: index {i}")));
        }
        k = k
            .saturating_mul(i64::from(*hi).saturating_sub(i64::from(*lo)).saturating_add(1))
            .saturating_add(i64::from(i).saturating_sub(i64::from(*lo)));
    }
    usize::try_from(k).map_err(|_| LErr::new("bad argument value: index"))
}
fn f_get_element(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, MANY)?;
    let s = sa(l, &arg(a, 0))?;
    let st = s.try_borrow().map_err(|_| LErr::new("safearray in use"))?;
    let k = idx(l, &st, a.get(1..).unwrap_or(&[]))?;
    Ok(st.data.get(k).cloned().unwrap_or(V::Nil))
}
fn f_put_element(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 3, MANY)?;
    let s = sa(l, &arg(a, 0))?;
    let last = a.last().cloned().unwrap_or(V::Nil);
    let ks = a.get(1..a.len().saturating_sub(1)).unwrap_or(&[]);
    let k = {
        let st = s.try_borrow().map_err(|_| LErr::new("safearray in use"))?;
        idx(l, &st, ks)?
    };
    if let Ok(mut st) = s.try_borrow_mut()
        && let Some(slot) = st.data.get_mut(k)
    {
        *slot = last.clone();
    }
    Ok(last)
}
fn f_get_dim(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let s = sa(l, &arg(a, 0))?;
    let n = s.try_borrow().map_or(0, |s| s.bounds.len());
    Ok(V::Int(i32::try_from(n).unwrap_or(i32::MAX)))
}
fn bound(l: &Lisp, a: &[V], upper: bool) -> R<V> {
    arity(a, 2, 2)?;
    let s = sa(l, &arg(a, 0))?;
    let d = intv(l, &arg(a, 1))?;
    let st = s.try_borrow().map_err(|_| LErr::new("safearray in use"))?;
    let b = usize::try_from(i64::from(d).saturating_sub(1)).ok().and_then(|k| st.bounds.get(k)).copied();
    match b {
        Some((lo, hi)) => Ok(V::Int(if upper { hi } else { lo })),
        None => Err(LErr::new("bad argument value: dimension")),
    }
}
fn f_lbound(l: &mut Lisp, a: &[V]) -> R<V> {
    bound(l, a, false)
}
fn f_ubound(l: &mut Lisp, a: &[V]) -> R<V> {
    bound(l, a, true)
}
fn f_sa_type(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let s = sa(l, &arg(a, 0))?;
    Ok(V::Int(s.try_borrow().map_or(0, |s| s.vt)))
}
/// numVal: a number's value as a real; anything else as it is.
fn num_val(x: &V) -> V {
    match x {
        V::Int(i) => V::Real(f64::from(*i)),
        other => other.clone(),
    }
}
fn f_3d_point(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 3)?;
    let mut p: Vec<V> =
        if a.len() == 1 { to_vec(&lst(l, &arg(a, 0))?).iter().map(num_val).collect() } else { a.iter().map(num_val).collect() };
    while p.len() < 3 {
        p.push(V::Real(0.0));
    }
    p.truncate(3);
    let s = V::SafeArray(Rc::new(RefCell::new(SafeArray { vt: VT_DOUBLE, bounds: vec![(0, 2)], data: p })));
    Ok(variant(VT_ARRAY | VT_DOUBLE, s))
}

/// Install, in vla.js's order.
pub fn install(l: &mut Lisp) {
    use Bf::{Special as Sp, Sync as S};
    for s in [S_VTRUE, S_VFALSE, S_VNULL] {
        l.set(s, V::Sym(s));
    }
    for (k, v) in [
        ("VLAX-VBEMPTY", 0),
        ("VLAX-VBNULL", 1),
        ("VLAX-VBINTEGER", 2),
        ("VLAX-VBLONG", 3),
        ("VLAX-VBSINGLE", 4),
        ("VLAX-VBDOUBLE", 5),
        ("VLAX-VBCURRENCY", 6),
        ("VLAX-VBDATE", 7),
        ("VLAX-VBSTRING", 8),
        ("VLAX-VBOBJECT", 9),
        ("VLAX-VBERROR", 10),
        ("VLAX-VBBOOLEAN", 11),
        ("VLAX-VBVARIANT", 12),
        ("VLAX-VBARRAY", 8192),
        ("ACACTIVEVIEWPORT", 0),
        ("ACALLVIEWPORTS", 1),
        ("ACBYBLOCK", 0),
        ("ACBYLAYER", 256),
        ("ACRED", 1),
        ("ACYELLOW", 2),
        ("ACGREEN", 3),
        ("ACCYAN", 4),
        ("ACBLUE", 5),
        ("ACMAGENTA", 6),
        ("ACWHITE", 7),
        ("ACMODELSPACE", 1),
        ("ACPAPERSPACE", 0),
        ("ACEXTENTS", 1),
        ("ACDISPLAY", 0),
        ("ACLNWTBYLAYER", -1),
        ("ACLNWTBYBLOCK", -2),
        ("ACLNWTBYLWDEFAULT", -3),
        ("ACALIGNMENTLEFT", 0),
        ("ACALIGNMENTCENTER", 1),
        ("ACALIGNMENTRIGHT", 2),
        ("ACALIGNMENTMIDDLE", 4),
    ] {
        let s = l.fsym(k);
        l.set(s, V::Int(v));
    }
    for (n, f) in [
        ("VLAX-GET-ACAD-OBJECT", f_acad as fn(&mut Lisp, &[V]) -> R<V>),
        ("VLAX-GET-PROPERTY", f_get_property),
        ("VLAX-PUT-PROPERTY", f_put_property),
        ("VLAX-GET", f_get),
        ("VLAX-PUT", f_put_property),
        ("VLAX-INVOKE-METHOD", f_invoke_method),
        ("VLAX-INVOKE", f_invoke),
        ("VLAX-PROPERTY-AVAILABLE-P", f_property_available),
        ("VLAX-METHOD-APPLICABLE-P", f_method_applicable),
        ("VLAX-ENAME->VLA-OBJECT", f_ename_to_vla),
        ("VLAX-VLA-OBJECT->ENAME", f_vla_to_ename),
        ("VLAX-ERASED-P", f_erased),
        ("VLAX-RELEASE-OBJECT", f_nil),
        ("VLAX-OBJECT-RELEASED-P", f_nil),
        ("VLAX-WRITE-ENABLED-P", f_t),
        ("VLAX-READ-ENABLED-P", f_t),
        ("VLAX-TYPEINFO-AVAILABLE-P", f_t),
    ] {
        l.def(n, S(f));
    }
    l.def("VLAX-FOR", Sp(sp_vlax_for));
    for (n, f) in [
        ("VLAX-MAKE-VARIANT", f_make_variant as fn(&mut Lisp, &[V]) -> R<V>),
        ("VLAX-VARIANT-VALUE", f_variant_value),
        ("VLAX-VARIANT-TYPE", f_variant_type),
        ("VLAX-VARIANT-CHANGE-TYPE", f_variant_change),
        ("VLAX-MAKE-SAFEARRAY", f_make_safearray),
        ("VLAX-SAFEARRAY-FILL", f_safearray_fill),
        ("VLAX-SAFEARRAY->LIST", f_safearray_list),
        ("VLAX-SAFEARRAY-GET-ELEMENT", f_get_element),
        ("VLAX-SAFEARRAY-PUT-ELEMENT", f_put_element),
        ("VLAX-SAFEARRAY-GET-DIM", f_get_dim),
        ("VLAX-SAFEARRAY-GET-L-BOUND", f_lbound),
        ("VLAX-SAFEARRAY-GET-U-BOUND", f_ubound),
        ("VLAX-SAFEARRAY-TYPE", f_sa_type),
        ("VLAX-3D-POINT", f_3d_point),
    ] {
        l.def(n, S(f));
    }
}
