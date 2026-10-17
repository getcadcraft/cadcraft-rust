//! expr.js — the parameter expression language: a tokenizer, a recursive-descent parser (nesting capped at 100, input
//! at 1000 UTF-16 units) and a tree walker. Units (mm cm m in ft ' " for lengths, deg ° rad for angles) convert after
//! a number; trigonometry is in degrees; comparisons give 1 or 0; every non-finite result is an error.
//!
//! Positions (`pos`) count UTF-16 units, as the JS does. Spec gap: a character outside the Basic Multilingual Plane
//! that is not allowed is named whole in the error (the JS names its first surrogate half).

use crate::i18n::{F, t};
use crate::js;
use crate::jv;
use serde_json::Value;

const EPS: f64 = 1e-9;
const D2R: f64 = std::f64::consts::PI / 180.0;

/// The names functions use (FUNCTION_NAMES, in the JS table's order).
pub const FUNCTION_NAMES: [&str; 21] = [
    "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "sqrt", "pow", "exp", "ln", "log", "abs", "sign", "ceil", "floor", "int",
    "round", "min", "max", "if",
];
fn function_names() -> &'static [&'static str] {
    &FUNCTION_NAMES
}
/// `(min, max)` arguments of a function.
fn arity(name: &str) -> Option<(usize, usize)> {
    Some(match name {
        "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "sqrt" | "exp" | "ln" | "log" | "abs" | "sign" | "ceil" | "floor" | "int" => {
            (1, 1)
        }
        "atan2" | "pow" => (2, 2),
        "round" => (1, 2),
        "min" | "max" => (1, 99),
        "if" => (3, 3),
        _ => return None,
    })
}
/// Is this a name `RESERVED` holds (functions, constants, units)?
pub fn reserved(name: &str) -> bool {
    function_names().contains(&name) || matches!(name, "pi" | "mm" | "cm" | "m" | "in" | "ft" | "deg" | "rad")
}
/// `NAME_RE` = /^[A-Za-z_][A-Za-z0-9_]{0,39}$/.
pub fn name_ok(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 40
        && b.first().is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_')
        && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_')
}

/// `UNITS[u]`.
fn unit_factor(u: &str) -> f64 {
    match u {
        "cm" => 10.0,
        "m" => 1000.0,
        "in" | "\"" => 25.4,
        "ft" | "'" => 304.8,
        "rad" => 180.0 / std::f64::consts::PI,
        _ => 1.0,
    }
}

/// A parsed expression.
#[derive(Clone, Debug)]
pub enum Ast {
    Num { v: f64, name: Option<&'static str> },
    Var { name: String, pos: usize },
    Neg(Box<Ast>),
    Cmp { op: &'static str, a: Box<Ast>, b: Box<Ast> },
    Bin { op: &'static str, a: Box<Ast>, b: Box<Ast>, pos: Option<usize> },
    Call { name: String, args: Vec<Ast>, pos: usize },
}

impl Ast {
    /// The tree as the JS objects look (`{k, v, …}`), for the parity harness.
    pub fn to_json(&self) -> Value {
        match self {
            Ast::Num { v, name } => {
                let mut o = jv::obj(vec![("k", "num".into()), ("v", jv::num(*v))]);
                if let Some(n) = name {
                    jv::set(&mut o, "name", (*n).into());
                }
                o
            }
            Ast::Var { name, pos } => jv::obj(vec![("k", "var".into()), ("name", name.as_str().into()), ("pos", jv::unum(*pos))]),
            Ast::Neg(a) => jv::obj(vec![("k", "neg".into()), ("a", a.to_json())]),
            Ast::Cmp { op, a, b } => jv::obj(vec![("k", "cmp".into()), ("op", (*op).into()), ("a", a.to_json()), ("b", b.to_json())]),
            Ast::Bin { op, a, b, pos } => {
                let mut o = jv::obj(vec![("k", "bin".into()), ("op", (*op).into()), ("a", a.to_json()), ("b", b.to_json())]);
                if let Some(p) = pos {
                    jv::set(&mut o, "pos", jv::unum(*p));
                }
                o
            }
            Ast::Call { name, args, pos } => jv::obj(vec![
                ("k", "call".into()),
                ("name", name.as_str().into()),
                ("args", Value::Array(args.iter().map(Ast::to_json).collect())),
                ("pos", jv::unum(*pos)),
            ]),
        }
    }
}

/// An expression error: the message and where (UTF-16 position, when known).
#[derive(Clone, Debug, PartialEq)]
pub struct ExprError {
    pub message: String,
    pub pos: Option<usize>,
}
impl ExprError {
    fn at(message: String, pos: usize) -> Self {
        ExprError { message, pos: Some(pos) }
    }
    fn new(message: String) -> Self {
        ExprError { message, pos: None }
    }
}

#[derive(Clone, Debug)]
enum TokT {
    Num(f64),
    Id(String),
    Op(&'static str),
    End,
}
#[derive(Clone, Debug)]
struct Tok {
    t: TokT,
    pos: usize,
}
impl Tok {
    /// `t.t`.
    fn kind(&self) -> &str {
        match &self.t {
            TokT::Num(_) => "num",
            TokT::Id(_) => "id",
            TokT::Op(o) => o,
            TokT::End => "end",
        }
    }
    /// `t.v ?? t.t` as text.
    fn shown(&self) -> String {
        match &self.t {
            TokT::Num(v) => js::num_str(*v),
            TokT::Id(s) => s.clone(),
            TokT::Op(o) => (*o).to_string(),
            TokT::End => "end".into(),
        }
    }
}

fn op_of(c: u16) -> Option<&'static str> {
    Some(match c {
        0x2b => "+",
        0x2d | 0x2212 | 0x2013 => "-",
        0x2a | 0xd7 | 0xb7 => "*",
        0x2f | 0xf7 => "/",
        0x25 => "%",
        0x5e => "^",
        0x28 => "(",
        0x29 => ")",
        0x2c => ",",
        0x3c => "<",
        0x3e => ">",
        0x2264 => "<=",
        0x2265 => ">=",
        0x2260 => "!=",
        _ => return None,
    })
}

fn is_digit(u: Option<&u16>) -> bool {
    u.is_some_and(|c| (0x30..=0x39).contains(c))
}
fn is_id_start(u: Option<&u16>) -> bool {
    u.is_some_and(|c| (0x41..=0x5a).contains(c) || (0x61..=0x7a).contains(c) || *c == 0x5f)
}
fn is_id_char(u: Option<&u16>) -> bool {
    is_id_start(u) || is_digit(u)
}
fn ascii(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

fn tokenize(src: &[u16]) -> Result<Vec<Tok>, ExprError> {
    let mut out = Vec::new();
    let mut i = 0usize;
    let n = src.len();
    while i < n {
        let c = src.get(i).copied().unwrap_or(0);
        if char::from_u32(u32::from(c)).is_some_and(js::is_ws) {
            i = i.saturating_add(1);
            continue;
        }
        // number: (\d+(\.\d*)?|\.\d+)([eE][+-]?\d+)?
        let mut j = i;
        let mut ok = false;
        if is_digit(src.get(j)) {
            while is_digit(src.get(j)) {
                j = j.saturating_add(1);
            }
            if src.get(j) == Some(&0x2e) {
                j = j.saturating_add(1);
                while is_digit(src.get(j)) {
                    j = j.saturating_add(1);
                }
            }
            ok = true;
        } else if src.get(j) == Some(&0x2e) && is_digit(src.get(j.saturating_add(1))) {
            j = j.saturating_add(1);
            while is_digit(src.get(j)) {
                j = j.saturating_add(1);
            }
            ok = true;
        }
        if ok {
            if matches!(src.get(j), Some(&0x65 | &0x45)) {
                let mut k = j.saturating_add(1);
                if matches!(src.get(k), Some(&0x2b | &0x2d)) {
                    k = k.saturating_add(1);
                }
                if is_digit(src.get(k)) {
                    while is_digit(src.get(k)) {
                        k = k.saturating_add(1);
                    }
                    j = k;
                }
            }
            let lit = ascii(src.get(i..j).unwrap_or(&[]));
            let mut v = js::str_num(&lit);
            let at = i;
            i = j;
            // rest = src.slice(i).replace(/^ (?=[°"'])/, '')
            let mut r = i;
            if src.get(r) == Some(&0x20) && matches!(src.get(r.saturating_add(1)), Some(&0xb0 | &0x22 | &0x27)) {
                r = r.saturating_add(1);
            }
            let unit = unit_at(src, r);
            if let Some((u, len)) = unit {
                v *= unit_factor(u);
                i = r.saturating_add(len);
            }
            out.push(Tok { t: TokT::Num(v), pos: at });
            continue;
        }
        if is_id_start(src.get(i)) {
            let mut k = i;
            while is_id_char(src.get(k)) {
                k = k.saturating_add(1);
            }
            out.push(Tok { t: TokT::Id(ascii(src.get(i..k).unwrap_or(&[]))), pos: i });
            i = k;
            continue;
        }
        let next = src.get(i.saturating_add(1)).copied();
        let two = match (c, next) {
            (0x3c, Some(0x3d)) => Some("<="),
            (0x3e, Some(0x3d)) => Some(">="),
            (0x3d, Some(0x3d)) => Some("=="),
            (0x21, Some(0x3d)) => Some("!="),
            _ => None,
        };
        if let Some(op) = two {
            out.push(Tok { t: TokT::Op(op), pos: i });
            i = i.saturating_add(2);
            continue;
        }
        if let Some(op) = op_of(c) {
            out.push(Tok { t: TokT::Op(op), pos: i });
            i = i.saturating_add(1);
            continue;
        }
        let ch = char::decode_utf16(src.get(i..).unwrap_or(&[]).iter().copied())
            .next()
            .and_then(Result::ok)
            .map_or_else(|| String::from('\u{fffd}'), String::from);
        return Err(ExprError::at(t("expr.not_allowed_expression", &[("c", F::S(ch))]), i));
    }
    out.push(Tok { t: TokT::End, pos: n });
    Ok(out)
}

/// UNIT_RE at `r`: (mm|cm|m|in|ft|deg|rad|°|"|')(?![A-Za-z0-9_]) → (unit, length).
fn unit_at(src: &[u16], r: usize) -> Option<(&'static str, usize)> {
    const ALTS: [(&str, &[u16]); 10] = [
        ("mm", &[0x6d, 0x6d]),
        ("cm", &[0x63, 0x6d]),
        ("m", &[0x6d]),
        ("in", &[0x69, 0x6e]),
        ("ft", &[0x66, 0x74]),
        ("deg", &[0x64, 0x65, 0x67]),
        ("rad", &[0x72, 0x61, 0x64]),
        ("°", &[0xb0]),
        ("\"", &[0x22]),
        ("'", &[0x27]),
    ];
    for (u, w) in ALTS {
        let end = r.saturating_add(w.len());
        if src.get(r..end) == Some(w) && !is_id_char(src.get(end)) {
            return Some((u, w.len()));
        }
    }
    None
}

struct Parser<'a> {
    tk: &'a [Tok],
    i: usize,
    depth: usize,
    allowed: Option<&'a [String]>,
    uses: Vec<String>,
}

impl Parser<'_> {
    fn peek(&self) -> &Tok {
        self.tk.get(self.i).or_else(|| self.tk.last()).unwrap_or(&Tok { t: TokT::End, pos: 0 })
    }
    fn next(&mut self) -> Tok {
        let t = self.peek().clone();
        self.i = self.i.saturating_add(1);
        t
    }
    fn take(&mut self, want: &str) -> Result<Tok, ExprError> {
        let p = self.peek().clone();
        if p.kind() != want {
            return Err(ExprError::at(expected(want, &p), p.pos));
        }
        self.i = self.i.saturating_add(1);
        Ok(p)
    }
    fn deeper(&mut self) -> Result<(), ExprError> {
        self.depth = self.depth.saturating_add(1);
        if self.depth > 100 {
            return Err(ExprError::at(t("expr.expression_nested_too_deeply", &[]), self.peek().pos));
        }
        Ok(())
    }
    fn cond(&mut self) -> Result<Ast, ExprError> {
        let a = self.sum()?;
        if let TokT::Op(op @ ("<" | "<=" | ">" | ">=" | "==" | "!=")) = self.peek().t {
            self.i = self.i.saturating_add(1);
            let b = self.sum()?;
            return Ok(Ast::Cmp { op, a: Box::new(a), b: Box::new(b) });
        }
        Ok(a)
    }
    fn sum(&mut self) -> Result<Ast, ExprError> {
        let mut n = self.term()?;
        while let TokT::Op(op @ ("+" | "-")) = self.peek().t {
            self.i = self.i.saturating_add(1);
            let b = self.term()?;
            n = Ast::Bin { op, a: Box::new(n), b: Box::new(b), pos: None };
        }
        Ok(n)
    }
    fn term(&mut self) -> Result<Ast, ExprError> {
        let mut n = self.power()?;
        while let TokT::Op(op @ ("*" | "/" | "%")) = self.peek().t {
            let pos = self.next().pos;
            let b = self.power()?;
            n = Ast::Bin { op, a: Box::new(n), b: Box::new(b), pos: Some(pos) };
        }
        Ok(n)
    }
    fn power(&mut self) -> Result<Ast, ExprError> {
        let a = self.unary()?;
        if self.peek().kind() != "^" {
            return Ok(a);
        }
        let pos = self.next().pos;
        self.deeper()?;
        let b = self.power();
        self.depth = self.depth.saturating_sub(1);
        Ok(Ast::Bin { op: "^", a: Box::new(a), b: Box::new(b?), pos: Some(pos) })
    }
    fn unary(&mut self) -> Result<Ast, ExprError> {
        self.deeper()?;
        let r = (|| match self.peek().t {
            TokT::Op("-") => {
                self.i = self.i.saturating_add(1);
                let a = self.unary_pow()?;
                Ok(Ast::Neg(Box::new(a)))
            }
            TokT::Op("+") => {
                self.i = self.i.saturating_add(1);
                self.unary_pow()
            }
            _ => self.primary(),
        })();
        self.depth = self.depth.saturating_sub(1);
        r
    }
    fn unary_pow(&mut self) -> Result<Ast, ExprError> {
        let a = self.unary()?;
        if self.peek().kind() != "^" {
            return Ok(a);
        }
        let pos = self.next().pos;
        let b = self.power()?;
        Ok(Ast::Bin { op: "^", a: Box::new(a), b: Box::new(b), pos: Some(pos) })
    }
    fn primary(&mut self) -> Result<Ast, ExprError> {
        let tk = self.peek().clone();
        match &tk.t {
            TokT::Num(v) => {
                self.i = self.i.saturating_add(1);
                Ok(Ast::Num { v: *v, name: None })
            }
            TokT::Op("(") => {
                self.i = self.i.saturating_add(1);
                let n = self.cond()?;
                self.take(")")?;
                Ok(n)
            }
            TokT::Id(v) => {
                self.i = self.i.saturating_add(1);
                if self.peek().kind() == "(" {
                    let name = js::lower(v);
                    let Some((mn, mx)) = arity(&name) else {
                        return Err(ExprError::at(
                            t("expr.not_function_use", &[("v", F::S(v.clone())), ("v2", F::S(function_names().join(", ")))]),
                            tk.pos,
                        ));
                    };
                    self.i = self.i.saturating_add(1);
                    let mut args = vec![self.cond()?];
                    while self.peek().kind() == "," {
                        self.i = self.i.saturating_add(1);
                        args.push(self.cond()?);
                    }
                    self.take(")")?;
                    if args.len() < mn || args.len() > mx {
                        let msg = if mn == mx {
                            t("expr.args.exact", &[("name", F::S(name.clone())), ("n", F::from(mn))])
                        } else if mx > 9 {
                            t("expr.args.atleast", &[("name", F::S(name.clone())), ("min", F::from(mn))])
                        } else {
                            t("expr.args.range", &[("name", F::S(name.clone())), ("min", F::from(mn)), ("max", F::from(mx))])
                        };
                        return Err(ExprError::at(msg, tk.pos));
                    }
                    return Ok(Ast::Call { name, args, pos: tk.pos });
                }
                if v == "pi" {
                    return Ok(Ast::Num { v: std::f64::consts::PI, name: Some("pi") });
                }
                if let Some(allowed) = self.allowed
                    && !allowed.iter().any(|a| a == v)
                {
                    return Err(ExprError::at(t("expr.there_no_parameter_called", &[("v", F::S(v.clone()))]), tk.pos));
                }
                if !self.uses.contains(v) {
                    self.uses.push(v.clone());
                }
                Ok(Ast::Var { name: v.clone(), pos: tk.pos })
            }
            TokT::Op(o @ (")" | ",")) => Err(ExprError::at(t("expr.value_missing_before", &[("t", F::from(*o))]), tk.pos)),
            TokT::End => Err(ExprError::at(t("expr.expression_ends_too_soon", &[]), tk.pos)),
            _ => Err(ExprError::at(t("expr.unexpected", &[("v", F::S(tk.shown()))]), tk.pos)),
        }
    }
}

fn expected(want: &str, got: &Tok) -> String {
    if want == ")" {
        return if matches!(got.t, TokT::End) {
            t("expr.a_is_missing", &[])
        } else {
            t("expr.expected_before", &[("v", F::S(got.shown()))])
        };
    }
    t("expr.expected", &[("t", F::from(want))])
}

/// A successful parse.
#[derive(Clone, Debug)]
pub struct Parsed {
    pub ast: Ast,
    pub uses: Vec<String>,
}

/// `parse(src, names)`: names = the variables it may use (None = any name, checked at evaluation).
pub fn parse(src: &str, names: Option<&[String]>) -> Result<Parsed, ExprError> {
    if js::trim(src).is_empty() {
        return Err(ExprError::at(t("expr.type_value_expression_like", &[]), 0));
    }
    let units: Vec<u16> = src.encode_utf16().collect();
    if units.len() > 1000 {
        return Err(ExprError::at(t("expr.expression_too_long_1000", &[]), 0));
    }
    let tk = tokenize(&units)?;
    let mut p = Parser { tk: &tk, i: 0, depth: 0, allowed: names, uses: Vec::new() };
    let ast = p.cond()?;
    let pk = p.peek().clone();
    if !matches!(pk.t, TokT::End) {
        let msg =
            if pk.kind() == ")" { t("expr.there_without", &[]) } else { t("expr.unexpected_operator_missing", &[("v", F::S(pk.shown()))]) };
        return Err(ExprError::at(msg, pk.pos));
    }
    Ok(Parsed { ast, uses: p.uses })
}

/// `parse(src).ok`.
pub fn parses(src: &str) -> bool {
    parse(src, None).is_ok()
}

/// sin(180) is 1.2e-16 and sin(30) 0.49999999999999994: trig results are kept to 15 digits.
fn clean(x: f64) -> f64 {
    if x.abs() < 1e-15 { 0.0 } else { js::prec_num(x, 15) }
}

fn fin(x: f64, name: &str) -> Result<f64, ExprError> {
    if x.is_finite() { Ok(x) } else { Err(ExprError::new(t("expr.does_not_give_number", &[("name", F::from(name))]))) }
}

fn call(name: &str, a: &[f64]) -> Result<f64, ExprError> {
    let x = a.first().copied().unwrap_or(f64::NAN);
    let y = a.get(1).copied().unwrap_or(f64::NAN);
    let e = |m: &str| Err(ExprError::new(m.to_string()));
    Ok(match name {
        "sin" => clean(js::sin(x * D2R)),
        "cos" => clean(js::cos(x * D2R)),
        "tan" => {
            let c = js::cos(x * D2R);
            if c.abs() < 1e-12 {
                return e("tan() of 90° has no value.");
            }
            clean(js::tan(x * D2R))
        }
        "asin" => {
            if x < -1.0 - EPS || x > 1.0 + EPS {
                return e("asin() takes a value from -1 to 1.");
            }
            js::asin(js::max(-1.0, js::min(1.0, x))) / D2R
        }
        "acos" => {
            if x < -1.0 - EPS || x > 1.0 + EPS {
                return e("acos() takes a value from -1 to 1.");
            }
            js::acos(js::max(-1.0, js::min(1.0, x))) / D2R
        }
        "atan" => js::atan(x) / D2R,
        "atan2" => js::atan2(x, y) / D2R,
        "sqrt" => {
            if x < -EPS {
                return e("sqrt() of a negative number has no value.");
            }
            js::max(0.0, x).sqrt()
        }
        "pow" => fin(js::pow(x, y), "pow")?,
        "exp" => fin(js::exp(x), "exp")?,
        "ln" => {
            if x <= 0.0 {
                return e("ln() takes a number above zero.");
            }
            js::ln(x)
        }
        "log" => {
            if x <= 0.0 {
                return e("log() takes a number above zero.");
            }
            js::log10(x)
        }
        "abs" => x.abs(),
        "sign" => js::sign(x),
        "ceil" => (x - EPS * js::max(1.0, x.abs())).ceil(),
        "floor" => (x + EPS * js::max(1.0, x.abs())).floor(),
        "int" => js::trunc(x + js::sign(x) * EPS * js::max(1.0, x.abs())),
        "round" => {
            let d = if a.len() > 1 { y } else { 0.0 };
            if !js::is_integer(d) || !(0.0..=10.0).contains(&d) {
                return e("round(x, digits) needs whole digits from 0 to 10.");
            }
            let f = js::pow(10.0, d);
            let yy = x.abs() * f;
            let r = (yy + 0.5 + EPS * js::max(1.0, yy)).floor();
            (if x < 0.0 { -r } else { r }) / f
        }
        "min" => js::min_of(a.iter().copied()),
        "max" => js::max_of(a.iter().copied()),
        _ => return Err(ExprError::new(t("expr.bad_expression", &[]))),
    })
}

/// Variable values for [`evaluate`]: `Map`/object lookup (None = `undefined` or not a number).
pub trait Values {
    fn get(&self, name: &str) -> Option<f64>;
}
impl Values for [(String, f64)] {
    fn get(&self, name: &str) -> Option<f64> {
        self.iter().find(|(k, _)| k == name).map(|(_, v)| *v)
    }
}
impl Values for Vec<(String, f64)> {
    fn get(&self, name: &str) -> Option<f64> {
        Values::get(self.as_slice(), name)
    }
}
impl Values for std::collections::HashMap<String, f64> {
    fn get(&self, name: &str) -> Option<f64> {
        std::collections::HashMap::get(self, name).copied()
    }
}

fn ev(n: &Ast, values: &dyn Values) -> Result<f64, ExprError> {
    Ok(match n {
        Ast::Num { v, .. } => *v,
        Ast::Var { name, pos } => match values.get(name) {
            Some(v) if v.is_finite() => v,
            _ => return Err(ExprError::at(t("expr.has_no_value", &[("name", F::S(name.clone()))]), *pos)),
        },
        Ast::Neg(a) => -ev(a, values)?,
        Ast::Cmp { op, a, b } => {
            let a = ev(a, values)?;
            let b = ev(b, values)?;
            let tol = EPS * js::max(js::max(1.0, a.abs()), b.abs());
            let r = match *op {
                "<" => a < b - tol,
                "<=" => a <= b + tol,
                ">" => a > b + tol,
                ">=" => a >= b - tol,
                "==" => (a - b).abs() <= tol,
                _ => (a - b).abs() > tol,
            };
            if r { 1.0 } else { 0.0 }
        }
        Ast::Bin { op, a, b, pos } => {
            let a = ev(a, values)?;
            let b = ev(b, values)?;
            match *op {
                "+" => a + b,
                "-" => a - b,
                "*" => a * b,
                "^" => {
                    let r = js::pow(a, b);
                    if !r.is_finite() {
                        return Err(ExprError { message: t("expr.power_has_no_value", &[]), pos: *pos });
                    }
                    r
                }
                _ => {
                    if b == 0.0 {
                        return Err(ExprError { message: t("expr.expression_divides_zero", &[]), pos: *pos });
                    }
                    if *op == "/" { a / b } else { a - b * (a / b).floor() }
                }
            }
        }
        Ast::Call { name, args, .. } => {
            if name == "if" {
                let c = ev(args.first().ok_or_else(|| ExprError::new(t("expr.bad_expression", &[])))?, values)?;
                let pick = if c != 0.0 { args.get(1) } else { args.get(2) };
                return ev(pick.ok_or_else(|| ExprError::new(t("expr.bad_expression", &[])))?, values);
            }
            let mut xs = Vec::with_capacity(args.len());
            for a in args {
                xs.push(ev(a, values)?);
            }
            call(name, &xs)?
        }
    })
}

/// `evaluate(ast, values)`.
pub fn evaluate(ast: &Ast, values: &dyn Values) -> Result<f64, ExprError> {
    let r = ev(ast, values)?;
    if !r.is_finite() {
        return Err(ExprError::new(t("expr.expression_does_not_give", &[])));
    }
    Ok(r)
}

/// `compute(src, values)` → (value, uses).
pub fn compute(src: &str, values: &dyn Values) -> Result<(f64, Vec<String>), ExprError> {
    let p = parse(src, None)?;
    let v = evaluate(&p.ast, values)?;
    Ok((v, p.uses))
}

/// `literal(src)`: a plain number (with or without a unit), or None.
pub fn literal(src: &str) -> Option<f64> {
    let p = parse(src, Some(&[])).ok()?;
    match &p.ast {
        Ast::Neg(a) => match &**a {
            Ast::Num { v, name: None } => Some(-v),
            _ => None,
        },
        Ast::Num { v, name: None } => Some(*v),
        _ => None,
    }
}

/// `formatNumber(v)`: shortest text that reads back the same to 12 significant digits.
pub fn format_number(v: f64) -> String {
    if !v.is_finite() {
        return js::num_str(v);
    }
    let r = js::prec_num(v, 12);
    if r == 0.0 && r.is_sign_negative() { "0".into() } else { js::num_str(r) }
}

fn polish(x: f64) -> f64 {
    let r = js::prec_num(x, 13);
    if (r - x).abs() <= 1e-12 * js::max(1.0, x.abs()) { r } else { x }
}

/// `solveFor(ast, values, name, target, {tol})`.
pub fn solve_for(ast: &Ast, values: &[(String, f64)], name: &str, target: f64, tol: f64) -> Result<f64, String> {
    let mut base: Vec<(String, f64)> = Vec::with_capacity(values.len().saturating_add(1));
    for (k, v) in values {
        if let Some(slot) = base.iter_mut().find(|(b, _)| b == k) {
            slot.1 = *v;
        } else {
            base.push((k.clone(), *v));
        }
    }
    let x0 = match base.iter().find(|(k, _)| k == name) {
        Some((_, v)) if v.is_finite() => *v,
        _ => 1.0,
    };
    let mut fx = |x: f64| {
        if let Some(slot) = base.iter_mut().find(|(k, _)| k == name) {
            slot.1 = x;
        } else {
            base.push((name.to_string(), x));
        }
        evaluate(ast, &base).map_or(f64::NAN, |v| v - target)
    };
    let scale = js::max(js::max(1.0, target.abs()), x0.abs());
    let close = |y: f64| y.is_finite() && y.abs() <= tol * scale;
    let mut a = x0;
    let mut fa = fx(a);
    if close(fa) {
        return Ok(a);
    }
    let mut b = if x0 == 0.0 { 1e-3 } else { x0 * (1.0 + 1e-4) };
    let mut fb = fx(b);
    let mut k: u32 = 0;
    while k < 60 && fa.is_finite() && fb.is_finite() && fb != fa {
        let c = b - fb * (b - a) / (fb - fa);
        let fc = fx(c);
        if close(fc) {
            return Ok(polish(c));
        }
        a = b;
        fa = fb;
        b = c;
        fb = fc;
        k = k.saturating_add(1);
    }
    let g0 = fx(x0);
    let mut step = js::max(1e-6, x0.abs() * 1e-3);
    while step < 1e9 {
        for s in [x0 + step, x0 - step] {
            let gs = fx(s);
            if close(gs) {
                return Ok(polish(s));
            }
            if g0.is_finite() && gs.is_finite() && js::sign(gs) != js::sign(g0) {
                let (mut lo, mut hi, mut glo) = (x0, s, g0);
                for _ in 0..200 {
                    let mid = (lo + hi) / 2.0;
                    let gm = fx(mid);
                    if close(gm) || (hi - lo).abs() <= 1e-15 * js::max(1.0, mid.abs()) {
                        return Ok(polish(mid));
                    }
                    if js::sign(gm) == js::sign(glo) {
                        lo = mid;
                        glo = gm;
                    } else {
                        hi = mid;
                    }
                }
            }
        }
        step *= 2.0;
    }
    Err(t("expr.no_value_gives", &[("name", F::from(name)), ("target", F::S(format_number(target)))]))
}

/// `renameIn(src, from, to)`: whole names only, never a unit right after a number.
pub fn rename_in(src: &str, from: &str, to: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0usize;
    let is_start = |c: char| c.is_ascii_alphabetic() || c == '_';
    let is_part = |c: char| c.is_ascii_alphanumeric() || c == '_';
    while let Some(&c) = chars.get(i) {
        if is_start(c) {
            let mut j = i;
            while chars.get(j).is_some_and(|&d| is_part(d)) {
                j = j.saturating_add(1);
            }
            let m: String = chars.get(i..j).unwrap_or(&[]).iter().collect();
            let prev = if i > 0 { chars.get(i.saturating_sub(1)).copied() } else { None };
            if prev.is_some_and(|p| p.is_ascii_digit() || p == '.') || m != from {
                out.push_str(&m);
            } else {
                out.push_str(to);
            }
            i = j;
        } else {
            out.push(c);
            i = i.saturating_add(1);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn val(src: &str) -> f64 {
        compute(src, &Vec::new()).map(|r| r.0).unwrap_or(f64::NAN)
    }
    #[test]
    fn units_and_precedence() {
        assert_eq!(val("1in"), 25.4);
        assert_eq!(val("2'"), 609.6);
        assert_eq!(val("1ft + 1in"), 330.2);
        assert_eq!(val("2^3^2"), 512.0);
        assert_eq!(val("-2^2"), -4.0);
        assert_eq!(val("-7 % 3"), 2.0);
        assert_eq!(val("sin(30)"), 0.5);
        assert_eq!(val("round(1.005, 2)"), 1.01);
        assert_eq!(val("atan2(1, 1)"), 45.0);
        assert!(compute("1 / 0", &Vec::new()).is_err());
        assert_eq!(literal("-2in"), Some(-50.8));
        assert_eq!(literal("pi"), None);
        assert_eq!(rename_in("w*2 + 25mm + width", "w", "x"), "x*2 + 25mm + width");
        assert_eq!(rename_in("2m + m", "m", "q"), "2m + q");
    }
    #[test]
    fn deep_nesting_is_refused_without_overflow() {
        let s = format!("{}1{}", "(".repeat(150), ")".repeat(150));
        assert!(parse(&s, None).is_err());
        let s = "-".repeat(999);
        assert!(parse(&s, None).is_err());
    }
}
