//! AutoLISP wildcard patterns (wild.js: wcmatch, ssget filters): `#` a digit, `@` a letter, `.` neither, `*` any run,
//! `?` one character, `~` not (first), `[..]` / `[~..]` sets, `` ` `` escapes, `,` or.
//!
//! 0.5 turns a pattern into a JS regular expression (non-Unicode, so `?` is one UTF-16 unit); this matches the same
//! language directly (no backtracking: `*a*a*a…` cannot run away), and a set whose range is out of order fails with
//! the message the RegExp constructor gives.

use crate::value::{LErr, R};

#[derive(Debug, Clone)]
enum Tok {
    Lit(u16),
    Set { inv: bool, items: Vec<(u16, u16)> },
    Any,
    Star,
}

#[derive(Debug)]
struct Alt {
    neg: bool,
    toks: Vec<Tok>,
}

/// escRe: the characters a regular expression escapes.
fn esc_re(c: u16, src: &mut Vec<u16>) {
    if b".*+?^${}()|[]\\/-".iter().any(|&b| u16::from(b) == c) {
        src.push(0x5C);
    }
    src.push(c);
}

/// A pattern → its alternatives (or the RegExp constructor's error).
fn compile(pattern: &[u16]) -> R<Vec<Alt>> {
    // split at "," (a backquoted character stays with its backquote)
    let mut parts: Vec<Vec<u16>> = Vec::new();
    let mut cur: Vec<u16> = Vec::new();
    let mut i = 0usize;
    while let Some(&c) = pattern.get(i) {
        if c == 0x60 && i.saturating_add(1) < pattern.len() {
            cur.push(0x60);
            cur.push(pattern.get(i.saturating_add(1)).copied().unwrap_or(0));
            i = i.saturating_add(2);
            continue;
        }
        if c == 0x2C {
            parts.push(std::mem::take(&mut cur));
            i = i.saturating_add(1);
            continue;
        }
        cur.push(c);
        i = i.saturating_add(1);
    }
    parts.push(cur);
    let mut alts = Vec::new();
    for part in parts {
        let (neg, s) = match part.split_first() {
            Some((0x7E, rest)) => (true, rest.to_vec()),
            _ => (false, part),
        };
        let mut toks = Vec::new();
        let mut src: Vec<u16> = Vec::new();
        let mut k = 0usize;
        while let Some(&c) = s.get(k) {
            if c == 0x60 && k.saturating_add(1) < s.len() {
                k = k.saturating_add(1);
                let n = s.get(k).copied().unwrap_or(0);
                esc_re(n, &mut src);
                toks.push(Tok::Lit(n));
            } else if c == 0x23 {
                toks.push(Tok::Set { inv: false, items: vec![(0x30, 0x39)] });
                src.extend("[0-9]".encode_utf16());
            } else if c == 0x40 {
                toks.push(Tok::Set { inv: false, items: vec![(0x41, 0x5A), (0x61, 0x7A)] });
                src.extend("[A-Za-z]".encode_utf16());
            } else if c == 0x2E {
                toks.push(Tok::Set { inv: true, items: vec![(0x41, 0x5A), (0x61, 0x7A), (0x30, 0x39)] });
                src.extend("[^A-Za-z0-9]".encode_utf16());
            } else if c == 0x2A {
                toks.push(Tok::Star);
                src.extend("[\\s\\S]*".encode_utf16());
            } else if c == 0x3F {
                toks.push(Tok::Any);
                src.extend("[\\s\\S]".encode_utf16());
            } else if c == 0x5B {
                let end = (k.saturating_add(2)..s.len()).find(|&j| s.get(j) == Some(&0x5D));
                let Some(end) = end else {
                    src.extend("\\[".encode_utf16());
                    toks.push(Tok::Lit(0x5B));
                    k = k.saturating_add(1);
                    continue;
                };
                let mut set: &[u16] = s.get(k.saturating_add(1)..end).unwrap_or(&[]);
                let mut inv = false;
                if set.first() == Some(&0x7E) {
                    inv = true;
                    set = set.get(1..).unwrap_or(&[]);
                }
                src.push(0x5B);
                if inv {
                    src.push(0x5E);
                }
                for &u in set {
                    if matches!(u, 0x5C..=0x5E) {
                        src.push(0x5C);
                    }
                    src.push(u);
                }
                src.push(0x5D);
                // the class's ranges: "a-z"; a "-" first, last or after a range is itself
                let mut items: Vec<(u16, u16)> = Vec::new();
                let mut bad = false;
                let mut j = 0usize;
                while let Some(&a) = set.get(j) {
                    if set.get(j.saturating_add(1)) == Some(&0x2D) && j.saturating_add(2) < set.len() {
                        let b = set.get(j.saturating_add(2)).copied().unwrap_or(0);
                        if a > b {
                            bad = true;
                        }
                        items.push((a, b));
                        j = j.saturating_add(3);
                    } else {
                        items.push((a, a));
                        j = j.saturating_add(1);
                    }
                }
                if bad {
                    // the rest of the source as the constructor saw it: the same text up to here plus what follows
                    let mut rest_src = src.clone();
                    let mut kk = end.saturating_add(1);
                    while let Some(&c2) = s.get(kk) {
                        rest_src.extend(source_of(c2, &s, &mut kk));
                    }
                    let body = String::from_utf16_lossy(&rest_src);
                    return Err(LErr::new(format!("Invalid regular expression: /^(?:{body})$/: Range out of order in character class")));
                }
                toks.push(Tok::Set { inv, items });
                k = end;
            } else {
                esc_re(c, &mut src);
                toks.push(Tok::Lit(c));
            }
            k = k.saturating_add(1);
        }
        alts.push(Alt { neg, toks });
    }
    Ok(alts)
}

/// The regular-expression text of the pattern character at `k` (for an error message), moving `k` past it.
fn source_of(c: u16, s: &[u16], k: &mut usize) -> Vec<u16> {
    let mut out = Vec::new();
    match c {
        0x60 if k.saturating_add(1) < s.len() => {
            *k = k.saturating_add(1);
            esc_re(s.get(*k).copied().unwrap_or(0), &mut out);
        }
        0x23 => out.extend("[0-9]".encode_utf16()),
        0x40 => out.extend("[A-Za-z]".encode_utf16()),
        0x2E => out.extend("[^A-Za-z0-9]".encode_utf16()),
        0x2A => out.extend("[\\s\\S]*".encode_utf16()),
        0x3F => out.extend("[\\s\\S]".encode_utf16()),
        0x5B => {
            let end = (k.saturating_add(2)..s.len()).find(|&j| s.get(j) == Some(&0x5D));
            match end {
                None => out.extend("\\[".encode_utf16()),
                Some(end) => {
                    let mut set: &[u16] = s.get(k.saturating_add(1)..end).unwrap_or(&[]);
                    out.push(0x5B);
                    if set.first() == Some(&0x7E) {
                        out.push(0x5E);
                        set = set.get(1..).unwrap_or(&[]);
                    }
                    for &u in set {
                        if matches!(u, 0x5C..=0x5E) {
                            out.push(0x5C);
                        }
                        out.push(u);
                    }
                    out.push(0x5D);
                    *k = end;
                }
            }
        }
        _ => esc_re(c, &mut out),
    }
    *k = k.saturating_add(1);
    out
}

fn tok_matches(t: &Tok, c: u16) -> bool {
    match t {
        Tok::Lit(l) => *l == c,
        Tok::Any | Tok::Star => true,
        Tok::Set { inv, items } => items.iter().any(|&(a, b)| (a..=b).contains(&c)) != *inv,
    }
}

/// Does the whole of `s` match these tokens? Every token but `*` takes one unit, so the two-pointer glob walk is exact
/// (a star remembers where it may take one more unit); O(len(s) × tokens) at worst, no allocation.
fn full_match(toks: &[Tok], s: &[u16]) -> bool {
    let (mut ti, mut si) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while si < s.len() {
        match toks.get(ti) {
            Some(Tok::Star) => {
                star = Some((ti, si));
                ti = ti.saturating_add(1);
                continue;
            }
            Some(t) if s.get(si).is_some_and(|&c| tok_matches(t, c)) => {
                ti = ti.saturating_add(1);
                si = si.saturating_add(1);
                continue;
            }
            _ => {}
        }
        match star {
            Some((st, ss)) => {
                let ss = ss.saturating_add(1);
                star = Some((st, ss));
                ti = st.saturating_add(1);
                si = ss;
            }
            None => return false,
        }
    }
    toks.get(ti..).is_some_and(|rest| rest.iter().all(|t| matches!(t, Tok::Star)))
}

/// A pattern and its alternatives.
type Compiled = (Vec<u16>, std::rc::Rc<Vec<Alt>>);

thread_local! {
    /// The last pattern compiled (a routine's loop calls wcmatch with one pattern many times; 0.5's RegExp is cached
    /// by V8 the same way).
    static LAST: std::cell::RefCell<Option<Compiled>> = const { std::cell::RefCell::new(None) };
}

/// wcmatch(s, pattern).
pub fn wcmatch(s: &[u16], pattern: &[u16]) -> R<bool> {
    let cached = LAST.with(|l| l.borrow().as_ref().filter(|(p, _)| p.as_slice() == pattern).map(|(_, a)| std::rc::Rc::clone(a)));
    let alts = match cached {
        Some(a) => a,
        None => {
            let a = std::rc::Rc::new(compile(pattern)?);
            if pattern.len() <= 4096 {
                LAST.with(|l| *l.borrow_mut() = Some((pattern.to_vec(), std::rc::Rc::clone(&a))));
            }
            a
        }
    };
    Ok(alts.iter().any(|a| full_match(&a.toks, s) != a.neg))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::lstr;

    fn m(s: &str, p: &str) -> bool {
        wcmatch(&lstr(s), &lstr(p)).unwrap_or(false)
    }

    #[test]
    fn patterns() {
        assert!(m("LINE", "L*"));
        assert!(m("A1", "@#"));
        assert!(!m("A1", "@@"));
        assert!(m("a-b", "a.b"));
        assert!(m("X", "~Y"));
        assert!(!m("Y", "~Y"));
        assert!(m("B", "A,B"));
        assert!(m("c", "[a-d]"));
        assert!(m("e", "[~a-d]"));
        assert!(m("*", "`*"));
        assert!(m("a,b", "a`,b"));
        assert!(m("", ""));
        assert!(m("[", "["));
        assert!(m("ab", "a?"));
        assert!(m(&"a".repeat(60), &"*a".repeat(30)));
        assert!(!m(&"a".repeat(60), &("*a".repeat(30) + "b")));
        assert_eq!(
            wcmatch(&lstr("x"), &lstr("[z-a]")).map_err(|e| e.msg),
            Err("Invalid regular expression: /^(?:[z-a])$/: Range out of order in character class".into())
        );
    }
}
