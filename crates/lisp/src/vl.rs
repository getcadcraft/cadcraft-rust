//! The vl- functions that need no drawing (vl.js): lists (sort, remove, member-if, some, every …), strings (search,
//! subst, trim, translate …), symbols, the blackboard, catch-all apply, exit, the in-memory directory.

use std::rc::Rc;

use crate::builtins::{MANY, arg, arity, bool_v, equal, intv, js_truthy, lst, strv, symv};
use crate::interp::{Bf, Ctl, Lisp};
use crate::syms::S_T;
use crate::value::{LErr, MSG_QUIT, R, V, from_vec, index_of, is_proper, last_index_of, list_len, lstr, rstr, to_vec, upper16};

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

// ---- lists ----

type Items = Rc<Vec<(V, usize)>>;
type SortK = Box<dyn FnOnce(&mut Lisp, Vec<(V, usize)>) -> Ctl>;

/// A stable top-down merge sort whose comparison is the routine's function (vl.js sortBy): the same calls in the
/// same order.
fn msort(l: &mut Lisp, f: V, a: Items, lo: usize, hi: usize, k: SortK) -> Ctl {
    if hi.saturating_sub(lo) < 2 {
        let part = a.get(lo..hi).unwrap_or(&[]).to_vec();
        return k(l, part);
    }
    let mid = lo.saturating_add(hi) >> 1;
    let (f2, a2) = (f.clone(), Rc::clone(&a));
    msort(
        l,
        f,
        a,
        lo,
        mid,
        Box::new(move |l, left| {
            let f3 = f2.clone();
            msort(l, f2, a2, mid, hi, Box::new(move |l, right| merge(l, f3, Rc::new(left), Rc::new(right), 0, 0, Vec::new(), k)))
        }),
    )
}
#[allow(clippy::too_many_arguments)]
fn merge(l: &mut Lisp, f: V, left: Items, right: Items, i: usize, j: usize, out: Vec<(V, usize)>, k: SortK) -> Ctl {
    if i < left.len() && j < right.len() {
        let (rj, li) = (right.get(j).cloned(), left.get(i).cloned());
        let (Some(rj), Some(li)) = (rj, li) else { return k(l, out) };
        let f2 = f.clone();
        return l.apply_then(f, vec![rj.0.clone(), li.0.clone()], move |l, v| {
            let mut out = out;
            if v.truthy() {
                out.push(rj);
                merge(l, f2, left, right, i, j.saturating_add(1), out, k)
            } else {
                out.push(li);
                merge(l, f2, left, right, i.saturating_add(1), j, out, k)
            }
        });
    }
    let mut out = out;
    out.extend(left.get(i..).unwrap_or(&[]).iter().cloned());
    out.extend(right.get(j..).unwrap_or(&[]).iter().cloned());
    k(l, out)
}
fn sort_common(l: &mut Lisp, a: Vec<V>, indices: bool) -> Ctl {
    tri!(arity(&a, 2, 2));
    let items = tri!(lst(l, &arg(&a, 0)));
    let f = tri!(l.fn_of(&arg(&a, 1)));
    let v: Vec<(V, usize)> = to_vec(&items).into_iter().enumerate().map(|(i, x)| (x, i)).collect();
    let n = v.len();
    msort(
        l,
        f,
        Rc::new(v),
        0,
        n,
        Box::new(move |_, sorted| {
            ok(from_vec(
                sorted.into_iter().map(|(x, i)| if indices { V::Int(i32::try_from(i).unwrap_or(i32::MAX)) } else { x }).collect(),
                V::Nil,
            ))
        }),
    )
}
fn f_sort(l: &mut Lisp, a: Vec<V>) -> Ctl {
    sort_common(l, a, false)
}
fn f_sort_i(l: &mut Lisp, a: Vec<V>) -> Ctl {
    sort_common(l, a, true)
}
fn f_remove(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    let x = arg(a, 0);
    Ok(from_vec(to_vec(&lst(l, &arg(a, 1))?).into_iter().filter(|y| !equal(y, &x, 0.0)).collect(), V::Nil))
}

#[derive(Clone, Copy)]
enum Pick {
    RemoveIf,
    RemoveIfNot,
}
fn filter_step(l: &mut Lisp, f: V, items: Rc<Vec<V>>, i: usize, out: Vec<V>, how: Pick) -> Ctl {
    let Some(x) = items.get(i).cloned() else { return ok(from_vec(out, V::Nil)) };
    let f2 = f.clone();
    l.apply_then(f, vec![x.clone()], move |l, v| {
        let mut out = out;
        let keep = match how {
            Pick::RemoveIf => !v.truthy(),
            Pick::RemoveIfNot => v.truthy(),
        };
        if keep {
            out.push(x);
        }
        filter_step(l, f2, items, i.saturating_add(1), out, how)
    })
}
fn filter_common(l: &mut Lisp, a: Vec<V>, how: Pick) -> Ctl {
    tri!(arity(&a, 2, 2));
    let f = tri!(l.fn_of(&arg(&a, 0)));
    let items = to_vec(&tri!(lst(l, &arg(&a, 1))));
    filter_step(l, f, Rc::new(items), 0, Vec::new(), how)
}
fn f_remove_if(l: &mut Lisp, a: Vec<V>) -> Ctl {
    filter_common(l, a, Pick::RemoveIf)
}
fn f_remove_if_not(l: &mut Lisp, a: Vec<V>) -> Ctl {
    filter_common(l, a, Pick::RemoveIfNot)
}
fn member_step(l: &mut Lisp, f: V, x: V, want: bool) -> Ctl {
    let V::Cons(c) = &x else { return ok(V::Nil) };
    let (item, rest) = (c.car.clone(), c.cdr.clone());
    let f2 = f.clone();
    l.apply_then(f, vec![item], move |l, v| if v.truthy() == want { ok(x) } else { member_step(l, f2, rest, want) })
}
fn member_common(l: &mut Lisp, a: Vec<V>, want: bool) -> Ctl {
    tri!(arity(&a, 2, 2));
    let f = tri!(l.fn_of(&arg(&a, 0)));
    let x = tri!(lst(l, &arg(&a, 1)));
    member_step(l, f, x, want)
}
fn f_member_if(l: &mut Lisp, a: Vec<V>) -> Ctl {
    member_common(l, a, true)
}
fn f_member_if_not(l: &mut Lisp, a: Vec<V>) -> Ctl {
    member_common(l, a, false)
}
fn lists(l: &Lisp, a: &[V]) -> R<Vec<Vec<V>>> {
    let mut out = Vec::new();
    for x in a.iter().skip(1) {
        out.push(to_vec(&lst(l, x)?));
    }
    Ok(out)
}
fn some_step(l: &mut Lisp, f: V, ls: Rc<Vec<Vec<V>>>, i: usize, n: usize, every: bool) -> Ctl {
    if i >= n {
        return ok(if every { V::Sym(S_T) } else { V::Nil });
    }
    let args: Vec<V> = ls.iter().map(|x| x.get(i).cloned().unwrap_or(V::Nil)).collect();
    let f2 = f.clone();
    l.apply_then(f, args, move |l, r| {
        if every {
            if !r.truthy() {
                return ok(V::Nil);
            }
        } else if r.truthy() {
            return ok(r);
        }
        some_step(l, f2, ls, i.saturating_add(1), n, every)
    })
}
fn some_common(l: &mut Lisp, a: Vec<V>, every: bool) -> Ctl {
    tri!(arity(&a, 2, MANY));
    let f = tri!(l.fn_of(&arg(&a, 0)));
    let ls = tri!(lists(l, &a));
    let n = ls.iter().map(Vec::len).min().unwrap_or(0);
    some_step(l, f, Rc::new(ls), 0, n, every)
}
fn f_some(l: &mut Lisp, a: Vec<V>) -> Ctl {
    some_common(l, a, false)
}
fn f_every(l: &mut Lisp, a: Vec<V>) -> Ctl {
    some_common(l, a, true)
}
fn f_position(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    let item = arg(a, 0);
    let mut x = lst(l, &arg(a, 1))?;
    let mut i: i32 = 0;
    while let V::Cons(c) = &x {
        if equal(&c.car, &item, 0.0) {
            return Ok(V::Int(i));
        }
        let next = c.cdr.clone();
        x = next;
        i = i.wrapping_add(1);
    }
    Ok(V::Nil)
}
fn f_list_star(_: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, MANY)?;
    let (last, init) = a.split_last().map_or((V::Nil, &[][..]), |(l, i)| (l.clone(), i));
    Ok(from_vec(init.to_vec(), last))
}
fn f_list_length(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let x = lst(l, &arg(a, 0))?;
    if !is_proper(&x) {
        return Ok(V::Nil);
    }
    Ok(V::Int(i32::try_from(list_len(&x)).unwrap_or(i32::MAX)))
}
fn f_consp(_: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    Ok(bool_v(matches!(arg(a, 0), V::Cons(_))))
}

// ---- strings ----

fn pos_v(i: usize) -> V {
    V::Int(i32::try_from(i).unwrap_or(i32::MAX))
}
fn f_search(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 3)?;
    let s = strv(l, &arg(a, 1))?;
    let pat = strv(l, &arg(a, 0))?;
    let from = if a.len() > 2 { f64::from(intv(l, &arg(a, 2))?) } else { 0.0 };
    Ok(index_of(&s, &pat, from).map_or(V::Nil, pos_v))
}
fn f_subst(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 3, 4)?;
    let nw = strv(l, &arg(a, 0))?;
    let pat = strv(l, &arg(a, 1))?;
    let s = strv(l, &arg(a, 2))?;
    let from = if a.len() > 3 { f64::from(intv(l, &arg(a, 3))?) } else { 0.0 };
    let Some(i) = index_of(&s, &pat, from) else { return Ok(V::Str(s)) };
    let mut out: Vec<u16> = s.get(..i).unwrap_or(&[]).to_vec();
    out.extend_from_slice(&nw);
    out.extend_from_slice(s.get(i.saturating_add(pat.len())..).unwrap_or(&[]));
    l.mk_str(out)
}
fn trim_set(l: &mut Lisp, a: &[V], left: bool, right: bool) -> R<V> {
    arity(a, 2, 2)?;
    let set = strv(l, &arg(a, 0))?;
    let s = strv(l, &arg(a, 1))?;
    let (mut i, mut j) = (0usize, s.len());
    if left {
        while i < j && s.get(i).is_some_and(|c| set.contains(c)) {
            i = i.saturating_add(1);
        }
    }
    if right {
        while j > i && s.get(j.saturating_sub(1)).is_some_and(|c| set.contains(c)) {
            j = j.saturating_sub(1);
        }
    }
    Ok(V::Str(s.get(i..j).unwrap_or(&[]).into()))
}
fn f_trim(l: &mut Lisp, a: &[V]) -> R<V> {
    trim_set(l, a, true, true)
}
fn f_ltrim(l: &mut Lisp, a: &[V]) -> R<V> {
    trim_set(l, a, true, false)
}
fn f_rtrim(l: &mut Lisp, a: &[V]) -> R<V> {
    trim_set(l, a, false, true)
}
/// `[...s]`: the code points of a JS string as unit runs (a lone surrogate is one).
fn code_points(s: &[u16]) -> Vec<&[u16]> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < s.len() {
        let c = s.get(i).copied().unwrap_or(0);
        let next = s.get(i.saturating_add(1)).copied().unwrap_or(0);
        let w = if (0xD800..0xDC00).contains(&c) && (0xDC00..0xE000).contains(&next) { 2 } else { 1 };
        out.push(s.get(i..i.saturating_add(w)).unwrap_or(&[]));
        i = i.saturating_add(w);
    }
    out
}
fn f_translate(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 3, 3)?;
    let src = strv(l, &arg(a, 0))?;
    let dst = strv(l, &arg(a, 1))?;
    let s = strv(l, &arg(a, 2))?;
    let mut out: Vec<u16> = Vec::with_capacity(s.len());
    for c in code_points(&s) {
        match index_of(&src, c, 0.0) {
            None => out.extend_from_slice(c),
            Some(k) => match dst.get(k) {
                Some(&u) => out.push(u),
                None => match dst.last() {
                    Some(&u) => out.push(u),
                    None => out.extend_from_slice(c),
                },
            },
        }
    }
    l.mk_str(out)
}
fn f_position_str(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 4)?;
    let ch = u16::try_from(intv(l, &arg(a, 0))? & 0xffff).unwrap_or(0);
    let s = strv(l, &arg(a, 1))?;
    let start = if a.len() > 2 && !arg(a, 2).is_nil() { Some(f64::from(intv(l, &arg(a, 2))?)) } else { None };
    let back = arg(a, 3).truthy();
    let i = if back {
        #[allow(clippy::cast_precision_loss)]
        let from = start.unwrap_or(s.len() as f64);
        last_index_of(&s, &[ch], from)
    } else {
        index_of(&s, &[ch], start.unwrap_or(0.0))
    };
    Ok(i.map_or(V::Nil, pos_v))
}
fn f_mismatch(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 5)?;
    let s1 = strv(l, &arg(a, 0))?;
    let s2 = strv(l, &arg(a, 1))?;
    let p1 = if js_truthy(&arg(a, 2)) { i64::from(intv(l, &arg(a, 2))?) } else { 0 };
    let p2 = if js_truthy(&arg(a, 3)) { i64::from(intv(l, &arg(a, 3))?) } else { 0 };
    let ic = arg(a, 4).truthy();
    let at = |s: &[u16], k: i64| -> Option<u16> { usize::try_from(k).ok().and_then(|k| s.get(k).copied()) };
    let len = |s: &[u16]| i64::try_from(s.len()).unwrap_or(i64::MAX);
    let mut n: i64 = 0;
    while p1.saturating_add(n) < len(&s1) && p2.saturating_add(n) < len(&s2) {
        let (x, y) = (at(&s1, p1.saturating_add(n)), at(&s2, p2.saturating_add(n)));
        let same = if ic {
            match (x, y) {
                (Some(x), Some(y)) => upper16(&[x]) == upper16(&[y]),
                _ => return Err(LErr::new("Cannot read properties of undefined (reading 'toUpperCase')")),
            }
        } else {
            x == y
        };
        if !same {
            break;
        }
        n = n.saturating_add(1);
        if n > len(&s1).saturating_add(len(&s2)).saturating_add(1 << 31) {
            break;
        }
    }
    Ok(V::Int(crate::value::to_i32(n as f64)))
}
fn f_elt(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    let s = strv(l, &arg(a, 0))?;
    let i = intv(l, &arg(a, 1))?;
    match usize::try_from(i).ok().and_then(|k| s.get(k)) {
        Some(&c) => Ok(V::Int(i32::from(c))),
        None => Err(LErr::new(format!("bad argument value: {}", l.show(&arg(a, 1), true)))),
    }
}
fn f_string_to_list(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let s = strv(l, &arg(a, 0))?;
    Ok(from_vec(code_points(&s).into_iter().map(|c| V::Int(i32::from(c.first().copied().unwrap_or(0)))).collect(), V::Nil))
}
fn f_list_to_string(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let mut out = Vec::new();
    for c in to_vec(&lst(l, &arg(a, 0))?) {
        out.push(u16::try_from(intv(l, &c)? & 0xffff).unwrap_or(0));
    }
    l.mk_str(out)
}
fn f_princ_to_string(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let s = l.show(&arg(a, 0), false);
    l.mk_str(lstr(&s).to_vec())
}
fn f_prin1_to_string(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let s = l.show(&arg(a, 0), true);
    l.mk_str(lstr(&s).to_vec())
}

// ---- symbols ----

fn f_symbolp(_: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    Ok(bool_v(matches!(arg(a, 0), V::Sym(_))))
}
fn f_symbol_name(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let s = symv(l, &arg(a, 0))?;
    Ok(V::str(l.name(s)))
}
fn f_symbol_value(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let s = symv(l, &arg(a, 0))?;
    Ok(l.get(s))
}
fn f_bb_set(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    let s = symv(l, &arg(a, 0))?;
    l.bb.insert(s, arg(a, 1));
    Ok(arg(a, 1))
}
fn f_bb_ref(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let s = symv(l, &arg(a, 0))?;
    Ok(l.bb.get(&s).cloned().unwrap_or(V::Nil))
}
fn f_doc_set(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    let s = symv(l, &arg(a, 0))?;
    Ok(l.set(s, arg(a, 1)))
}

// ---- errors ----

fn f_catch_all(l: &mut Lisp, a: Vec<V>) -> Ctl {
    tri!(arity(&a, 2, 2));
    l.push_catch();
    let f = match l.fn_of(&arg(&a, 0)) {
        Ok(f) => f,
        Err(e) => return Ctl::Throw(e),
    };
    let args = match lst(l, &arg(&a, 1)) {
        Ok(x) => to_vec(&x),
        Err(e) => return Ctl::Throw(e),
    };
    l.apply(f, args, None)
}
fn f_catch_p(_: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    Ok(bool_v(matches!(arg(a, 0), V::CatchErr(_))))
}
fn f_catch_msg(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    match arg(a, 0) {
        V::CatchErr(m) => Ok(V::str(&m)),
        other => Err(LErr::new(format!("bad argument type: catch-all-apply-error: {}", l.show(&other, true)))),
    }
}
fn f_exit_value(_: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    Err(LErr::new(MSG_QUIT))
}
fn f_exit_error(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    Err(LErr::new(rstr(&strv(l, &arg(a, 0))?)))
}

// ---- files and registry ----

/// One character of a JS case-insensitive (non-Unicode) match: its upper case when that is one character.
fn fold(c: char) -> char {
    let mut up = c.to_uppercase();
    match (up.next(), up.next()) {
        (Some(u), None) => u,
        _ => c,
    }
}
/// `*` any run, `?` one character, the rest literal, case-insensitive (vl-directory-files' pattern).
fn glob(p: &[char], s: &[char]) -> bool {
    let n = p.len();
    let mut cur = vec![false; n.saturating_add(1)];
    let close = |set: &mut Vec<bool>| {
        for i in 0..n {
            if set.get(i).copied().unwrap_or(false)
                && p.get(i) == Some(&'*')
                && let Some(x) = set.get_mut(i.saturating_add(1))
            {
                *x = true;
            }
        }
    };
    if let Some(x) = cur.first_mut() {
        *x = true;
    }
    close(&mut cur);
    for &c in s {
        let mut next = vec![false; n.saturating_add(1)];
        for i in 0..n {
            if !cur.get(i).copied().unwrap_or(false) {
                continue;
            }
            let Some(&t) = p.get(i) else { continue };
            let hit = t == '*' || t == '?' || fold(t) == fold(c);
            if hit {
                let to = if t == '*' { i } else { i.saturating_add(1) };
                if let Some(x) = next.get_mut(to) {
                    *x = true;
                }
            }
        }
        close(&mut next);
        cur = next;
    }
    cur.get(n).copied().unwrap_or(false)
}
fn f_directory_files(l: &mut Lisp, a: &[V]) -> R<V> {
    let pat = if js_truthy(&arg(a, 1)) { rstr(&strv(l, &arg(a, 1))?) } else { "*.*".to_string() };
    let p: Vec<char> = pat.chars().collect();
    let mut out = Vec::new();
    for (_, f) in &l.files {
        let name = rstr(&f.name).replace('\\', "/");
        let base = name.rsplit('/').next().unwrap_or("").to_string();
        let chars: Vec<char> = base.chars().collect();
        if glob(&p, &chars) {
            out.push(V::str(&base));
        }
    }
    Ok(from_vec(out, V::Nil))
}
fn f_file_copy(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 3)?;
    let src = strv(l, &arg(a, 0))?;
    let Some(f) = l.find_file(&src) else { return Ok(V::Nil) };
    let dst = strv(l, &arg(a, 1))?;
    let mut text: Vec<u16> = if arg(a, 2).truthy() { l.find_file(&dst).map(|x| x.text.to_vec()).unwrap_or_default() } else { Vec::new() };
    text.extend_from_slice(&f.text);
    if text.len() > l.limits.string {
        return Err(LErr::new("Invalid string length"));
    }
    l.put_file(&dst, &text);
    Ok(V::Int(i32::try_from(f.text.len()).unwrap_or(i32::MAX)))
}
fn f_t(_: &mut Lisp, _: &[V]) -> R<V> {
    Ok(V::Sym(S_T))
}
fn f_nil(_: &mut Lisp, _: &[V]) -> R<V> {
    Ok(V::Nil)
}

/// Install, in vl.js's order.
pub fn install(l: &mut Lisp) {
    use Bf::{Ctl as C, Sync as S};
    l.def("VL-SORT", C(f_sort));
    l.def("VL-SORT-I", C(f_sort_i));
    l.def("VL-REMOVE", S(f_remove));
    l.def("VL-REMOVE-IF", C(f_remove_if));
    l.def("VL-REMOVE-IF-NOT", C(f_remove_if_not));
    l.def("VL-MEMBER-IF", C(f_member_if));
    l.def("VL-MEMBER-IF-NOT", C(f_member_if_not));
    l.def("VL-SOME", C(f_some));
    l.def("VL-EVERY", C(f_every));
    for (n, f) in [
        ("VL-POSITION", f_position as fn(&mut Lisp, &[V]) -> R<V>),
        ("VL-LIST*", f_list_star),
        ("VL-LIST-LENGTH", f_list_length),
        ("VL-CONSP", f_consp),
        ("VL-STRING-SEARCH", f_search),
        ("VL-STRING-SUBST", f_subst),
        ("VL-STRING-TRIM", f_trim),
        ("VL-STRING-LEFT-TRIM", f_ltrim),
        ("VL-STRING-RIGHT-TRIM", f_rtrim),
        ("VL-STRING-TRANSLATE", f_translate),
        ("VL-STRING-POSITION", f_position_str),
        ("VL-STRING-MISMATCH", f_mismatch),
        ("VL-STRING-ELT", f_elt),
        ("VL-STRING->LIST", f_string_to_list),
        ("VL-LIST->STRING", f_list_to_string),
        ("VL-PRINC-TO-STRING", f_princ_to_string),
        ("VL-PRIN1-TO-STRING", f_prin1_to_string),
        ("VL-SYMBOLP", f_symbolp),
        ("VL-SYMBOL-NAME", f_symbol_name),
        ("VL-SYMBOL-VALUE", f_symbol_value),
        ("VL-BB-SET", f_bb_set),
        ("VL-BB-REF", f_bb_ref),
        ("VL-DOC-SET", f_doc_set),
        ("VL-DOC-REF", f_symbol_value),
    ] {
        l.def(n, S(f));
    }
    l.def("VL-CATCH-ALL-APPLY", C(f_catch_all));
    for (n, f) in [
        ("VL-CATCH-ALL-ERROR-P", f_catch_p as fn(&mut Lisp, &[V]) -> R<V>),
        ("VL-CATCH-ALL-ERROR-MESSAGE", f_catch_msg),
        ("VL-EXIT-WITH-VALUE", f_exit_value),
        ("VL-EXIT-WITH-ERROR", f_exit_error),
        ("VL-DIRECTORY-FILES", f_directory_files),
        ("VL-FILE-COPY", f_file_copy),
        ("VL-MKDIR", f_t),
        ("VL-REGISTRY-READ", f_nil),
        ("VL-REGISTRY-WRITE", f_nil),
        ("VL-REGISTRY-DESCENDENTS", f_nil),
        ("VL-GET-RESOURCE", f_nil),
    ] {
        l.def(n, S(f));
    }
}
