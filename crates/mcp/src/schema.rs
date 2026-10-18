//! The JSON Schema subset tool arguments are checked against — tools/mcp/core.mjs `checkSchema` (0.5), message for
//! message: type, enum, const, minimum, maximum, minLength, maxLength, pattern, items, minItems, maxItems, properties,
//! required, additionalProperties (false or a schema), anyOf / oneOf; defaults of object properties are filled in.
//!
//! `pattern` is matched by a small matcher of our own ([`Pattern`]): anchors, literals, escapes, classes and counted
//! repeats — what the tool schemas use. It runs in time linear in the input for each piece of the pattern (no
//! backtracking), so a hostile string cannot stall it.

use serde_json::{Map, Value};

use crate::json::{js_string, len16};

/// Recursion bound for schemas (ours are four levels deep) and values (a value deeper than the schema is never walked).
const MAX_DEPTH: usize = 64;

fn type_of(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Array(_) => "array",
        Value::Number(n) => {
            if n.is_i64() || n.is_u64() || n.as_f64().is_some_and(|f| f.is_finite() && f.fract() == 0.0) {
                "integer"
            } else {
                "number"
            }
        }
        Value::String(_) => "string",
        Value::Bool(_) => "boolean",
        Value::Object(_) => "object",
    }
}

fn fits(t: &str, v: &Value) -> bool {
    let k = type_of(v);
    t == k || (t == "number" && k == "integer")
}

/// JavaScript's `===` / `includes` for the primitives schemas compare (objects and arrays are never equal).
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Null, Value::Null) => true,
        _ => false,
    }
}

/// Check `value` against `schema`; fills defaults into objects. Returns the problems (empty = fine).
pub fn check(schema: &Value, value: &mut Value, at: &str) -> Vec<String> {
    let mut out = Vec::new();
    check_into(schema, value, at, &mut out, 0);
    out
}

fn check_into(schema: &Value, value: &mut Value, at: &str, out: &mut Vec<String>, depth: usize) {
    let Some(s) = schema.as_object() else { return };
    if depth > MAX_DEPTH {
        out.push(format!("{at} is nested too deeply."));
        return;
    }
    if let Some(alts) = s.get("anyOf").or_else(|| s.get("oneOf")) {
        let list = alts.as_array().map(Vec::as_slice).unwrap_or(&[]);
        let mut any = false;
        for alt in list {
            let mut probs = Vec::new();
            check_into(alt, value, at, &mut probs, depth.saturating_add(1));
            if probs.is_empty() {
                any = true;
                break;
            }
        }
        if !any {
            out.push(format!("{at} does not match any of the allowed shapes."));
        }
        return;
    }
    if let Some(t) = s.get("type") {
        let ts: Vec<&str> = match t {
            Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
            Value::String(x) => vec![x.as_str()],
            _ => Vec::new(),
        };
        if !ts.iter().any(|t| fits(t, value)) {
            out.push(format!("{at} must be {}.", ts.join(" or ")));
            return;
        }
    }
    if let Some(c) = s.get("const")
        && !same(value, c)
    {
        out.push(format!("{at} must be {}.", crate::json::stringify(c)));
    }
    if let Some(Value::Array(list)) = s.get("enum")
        && !list.iter().any(|x| same(value, x))
    {
        let shown: Vec<String> = list.iter().map(crate::json::stringify).collect();
        out.push(format!("{at} must be one of {}.", shown.join(", ")));
    }
    if let Some(x) = value.as_f64().filter(|_| value.is_number()) {
        if let Some(m) = s.get("minimum").filter(|m| m.is_number())
            && x < m.as_f64().unwrap_or(f64::NEG_INFINITY)
        {
            out.push(format!("{at} must be at least {}.", js_string(m)));
        }
        if let Some(m) = s.get("maximum").filter(|m| m.is_number())
            && x > m.as_f64().unwrap_or(f64::INFINITY)
        {
            out.push(format!("{at} must be at most {}.", js_string(m)));
        }
    }
    if let Value::String(text) = &*value {
        let n = len16(text);
        if let Some(m) = s.get("minLength").and_then(Value::as_f64)
            && (n as f64) < m
        {
            out.push(format!("{at} must have at least {} characters.", js_string(&s["minLength"])));
        }
        if let Some(m) = s.get("maxLength").and_then(Value::as_f64)
            && (n as f64) > m
        {
            out.push(format!("{at} must have at most {} characters.", js_string(&s["maxLength"])));
        }
        if let Some(p) = s.get("pattern").and_then(Value::as_str) {
            let ok = Pattern::parse(p).is_some_and(|pat| pat.test(text));
            if !ok {
                out.push(format!("{at} is not in the expected form."));
            }
        }
    }
    if let Value::Array(items) = value {
        let n = items.len() as f64;
        if let Some(m) = s.get("minItems").and_then(Value::as_f64)
            && n < m
        {
            out.push(format!("{at} must have at least {} items.", js_string(&s["minItems"])));
        }
        if let Some(m) = s.get("maxItems").and_then(Value::as_f64)
            && n > m
        {
            out.push(format!("{at} must have at most {} items.", js_string(&s["maxItems"])));
        }
        if let Some(item) = s.get("items") {
            for (i, x) in items.iter_mut().enumerate() {
                check_into(item, x, &format!("{at}[{i}]"), out, depth.saturating_add(1));
            }
        }
    }
    if let Value::Object(obj) = value {
        let empty = Map::new();
        let props = s.get("properties").and_then(Value::as_object).unwrap_or(&empty);
        if let Some(Value::Array(req)) = s.get("required") {
            for r in req.iter().filter_map(Value::as_str) {
                if !obj.contains_key(r) {
                    out.push(format!("{at}.{r} is required."));
                }
            }
        }
        let extra = s.get("additionalProperties");
        for (k, v) in obj.iter_mut() {
            if let Some(p) = props.get(k) {
                check_into(p, v, &format!("{at}.{k}"), out, depth.saturating_add(1));
            } else if extra == Some(&Value::Bool(false)) {
                out.push(format!("{at}.{k} is not a known argument."));
            } else if let Some(e @ Value::Object(_)) = extra {
                check_into(e, v, &format!("{at}.{k}"), out, depth.saturating_add(1));
            }
        }
        for (k, p) in props {
            if !obj.contains_key(k)
                && let Some(d) = p.get("default")
            {
                obj.insert(k.clone(), d.clone());
            }
        }
    }
}

/// A character class: ranges, possibly negated.
#[derive(Debug, Clone)]
struct Class {
    ranges: Vec<(char, char)>,
    negated: bool,
}

impl Class {
    fn one(c: char) -> Self {
        Class { ranges: vec![(c, c)], negated: false }
    }
    fn matches(&self, c: char) -> bool {
        self.ranges.iter().any(|&(a, b)| c >= a && c <= b) != self.negated
    }
}

#[derive(Debug, Clone)]
struct Piece {
    class: Class,
    min: usize,
    max: usize,
}

/// A pattern of the subset tool schemas use: `^`, `$`, literals, `\d \w \s` (and their negations), escaped
/// punctuation, `.`, `[…]` classes with ranges and `^`, and `* + ? {m} {m,} {m,n}`. No groups or alternation.
#[derive(Debug, Clone)]
pub struct Pattern {
    start: bool,
    end: bool,
    pieces: Vec<Piece>,
}

fn escape_class(c: char) -> Class {
    let digit = vec![('0', '9')];
    let word = vec![('0', '9'), ('A', 'Z'), ('a', 'z'), ('_', '_')];
    let space =
        vec![(' ', ' '), ('\t', '\r'), ('\u{a0}', '\u{a0}'), ('\u{2000}', '\u{200a}'), ('\u{3000}', '\u{3000}'), ('\u{feff}', '\u{feff}')];
    match c {
        'd' => Class { ranges: digit, negated: false },
        'D' => Class { ranges: digit, negated: true },
        'w' => Class { ranges: word, negated: false },
        'W' => Class { ranges: word, negated: true },
        's' => Class { ranges: space, negated: false },
        'S' => Class { ranges: space, negated: true },
        'n' => Class::one('\n'),
        't' => Class::one('\t'),
        'r' => Class::one('\r'),
        other => Class::one(other),
    }
}

impl Pattern {
    /// None for a pattern outside the subset (the check then fails, which is the safe side).
    pub fn parse(src: &str) -> Option<Pattern> {
        let cs: Vec<char> = src.chars().collect();
        if cs.len() > 512 {
            return None;
        }
        let mut i = 0usize;
        let mut p = Pattern { start: false, end: false, pieces: Vec::new() };
        if cs.first() == Some(&'^') {
            p.start = true;
            i = 1;
        }
        while i < cs.len() {
            let c = *cs.get(i)?;
            if c == '$' && i.saturating_add(1) == cs.len() {
                p.end = true;
                break;
            }
            let class = match c {
                '\\' => {
                    i = i.checked_add(1)?;
                    escape_class(*cs.get(i)?)
                }
                '.' => Class { ranges: vec![('\n', '\n'), ('\r', '\r'), ('\u{2028}', '\u{2029}')], negated: true },
                '[' => {
                    i = i.checked_add(1)?;
                    let mut cl = Class { ranges: Vec::new(), negated: false };
                    if cs.get(i) == Some(&'^') {
                        cl.negated = true;
                        i = i.checked_add(1)?;
                    }
                    loop {
                        let a = *cs.get(i)?;
                        if a == ']' {
                            break;
                        }
                        let lo = if a == '\\' {
                            i = i.checked_add(1)?;
                            let e = escape_class(*cs.get(i)?);
                            if e.negated || e.ranges.len() != 1 || e.ranges.first().is_some_and(|r| r.0 != r.1) {
                                cl.ranges.extend(e.ranges);
                                i = i.checked_add(1)?;
                                continue;
                            }
                            e.ranges.first()?.0
                        } else {
                            a
                        };
                        if cs.get(i.checked_add(1)?) == Some(&'-') && cs.get(i.checked_add(2)?).is_some_and(|x| *x != ']') {
                            let mut hi = *cs.get(i.checked_add(2)?)?;
                            i = i.checked_add(2)?;
                            if hi == '\\' {
                                i = i.checked_add(1)?;
                                hi = *cs.get(i)?;
                            }
                            if hi < lo {
                                return None;
                            }
                            cl.ranges.push((lo, hi));
                        } else {
                            cl.ranges.push((lo, lo));
                        }
                        i = i.checked_add(1)?;
                    }
                    cl
                }
                '(' | ')' | '|' | '*' | '+' | '?' | '{' | '}' | '^' | '$' => return None,
                other => Class::one(other),
            };
            i = i.checked_add(1)?;
            let (mut min, mut max) = (1usize, 1usize);
            match cs.get(i) {
                Some('*') => {
                    (min, max) = (0, usize::MAX);
                    i = i.checked_add(1)?;
                }
                Some('+') => {
                    (min, max) = (1, usize::MAX);
                    i = i.checked_add(1)?;
                }
                Some('?') => {
                    (min, max) = (0, 1);
                    i = i.checked_add(1)?;
                }
                Some('{') => {
                    let close = cs.iter().skip(i).position(|x| *x == '}')?.checked_add(i)?;
                    let inner: String = cs.get(i.checked_add(1)?..close)?.iter().collect();
                    let (a, b) = match inner.split_once(',') {
                        Some((a, "")) => (a.parse().ok()?, usize::MAX),
                        Some((a, b)) => (a.parse().ok()?, b.parse().ok()?),
                        None => {
                            let n = inner.parse().ok()?;
                            (n, n)
                        }
                    };
                    if b < a {
                        return None;
                    }
                    (min, max) = (a, b);
                    i = close.checked_add(1)?;
                }
                _ => {}
            }
            p.pieces.push(Piece { class, min, max });
            if p.pieces.len() > 64 {
                return None;
            }
        }
        Some(p)
    }

    /// `RegExp.prototype.test`: anywhere in the string unless anchored.
    pub fn test(&self, s: &str) -> bool {
        let cs: Vec<char> = s.chars().collect();
        let n = cs.len();
        // reachable[j]: the pieces so far can end at position j
        let mut reach = vec![!self.start; n.saturating_add(1)];
        if let Some(r) = reach.first_mut() {
            *r = true;
        }
        for piece in &self.pieces {
            // run[j] = how many characters from j on match the class
            let mut run = vec![0usize; n.saturating_add(1)];
            for j in (0..n).rev() {
                let ok = cs.get(j).is_some_and(|c| piece.class.matches(*c));
                let next = run.get(j.saturating_add(1)).copied().unwrap_or(0);
                if let Some(r) = run.get_mut(j) {
                    *r = if ok { next.saturating_add(1) } else { 0 };
                }
            }
            // reachable after = union of [j + min, j + min(max, run[j])] over reachable j (difference array)
            let mut diff = vec![0i64; n.saturating_add(2)];
            for j in 0..=n {
                if !reach.get(j).copied().unwrap_or(false) {
                    continue;
                }
                let have = run.get(j).copied().unwrap_or(0);
                if have < piece.min {
                    continue;
                }
                let lo = j.saturating_add(piece.min);
                let hi = j.saturating_add(have.min(piece.max));
                if let Some(d) = diff.get_mut(lo) {
                    *d = d.saturating_add(1);
                }
                if let Some(d) = diff.get_mut(hi.saturating_add(1)) {
                    *d = d.saturating_sub(1);
                }
            }
            let mut acc = 0i64;
            for (j, r) in reach.iter_mut().enumerate() {
                acc = acc.saturating_add(diff.get(j).copied().unwrap_or(0));
                *r = acc > 0;
            }
        }
        if self.end { reach.get(n).copied().unwrap_or(false) } else { reach.iter().any(|x| *x) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn patterns() {
        let p = Pattern::parse("^d[0-9]{1,6}$").unwrap();
        assert!(p.test("d1") && p.test("d123456") && !p.test("d1234567") && !p.test("x1") && !p.test("d"));
        let q = Pattern::parse("^[A-Za-z0-9]{3,80}$").unwrap();
        assert!(q.test("IfcWall") && !q.test("If") && !q.test("Ifc Wall"));
        let tool = Pattern::parse("^[a-z][a-z0-9_]{0,63}$").unwrap();
        assert!(tool.test("objects_list") && !tool.test("Objects"));
        assert!(Pattern::parse("ab").unwrap().test("xxabyy"));
        assert!(Pattern::parse("(a|b)").is_none());
        // linear time on a long hostile string
        let long = "d".repeat(1_000_000);
        assert!(!p.test(&long));
    }

    #[test]
    fn messages_and_defaults() {
        let schema = json!({"type": "object", "properties": {
            "doc": {"type": "string", "pattern": "^d[0-9]{1,6}$"},
            "limit": {"type": "integer", "minimum": 1, "maximum": 1000, "default": 100},
            "kind": {"type": "string", "enum": ["a", "b"]},
            "ids": {"type": "array", "items": {"type": "integer"}, "maxItems": 2}
        }, "required": ["doc"], "additionalProperties": false});
        let mut v = json!({"limit": 0, "kind": "c", "ids": [1, 2.5, 3], "zz": 1});
        let p = check(&schema, &mut v, "arguments");
        assert_eq!(
            p,
            vec![
                "arguments.doc is required.",
                "arguments.limit must be at least 1.",
                "arguments.kind must be one of \"a\", \"b\".",
                "arguments.ids must have at most 2 items.",
                "arguments.ids[1] must be integer.",
                "arguments.zz is not a known argument."
            ]
        );
        let mut ok = json!({"doc": "d1"});
        assert!(check(&schema, &mut ok, "arguments").is_empty());
        assert_eq!(ok, json!({"doc": "d1", "limit": 100}));
        let mut f = json!(2.0);
        assert!(check(&json!({"type": "integer"}), &mut f, "x").is_empty());
    }
}
