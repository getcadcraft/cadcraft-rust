//! `formula.js` — the assembly quantity formulas: a tokenizer + recursive-descent parser and a tree walker, never an
//! evaluator of code. A formula names only the variables it is given and the functions in `FUNCS`.
//!
//!   expr    := term (('+' | '-') term)*
//!   term    := unary (('*' | '/') unary)*
//!   unary   := ('-' | '+') unary | primary
//!   primary := number | name | name '(' expr (',' expr)* ')' | '(' expr ')'
//!
//! Positions are UTF-16 indexes, as the JS reports them.

use crate::i18n::{F, t, tf};
use crate::jv;
use serde_json::{Value, json};

const EPS: f64 = 1e-9;
/// `FUNCTION_NAMES`, in the JS table's order.
pub const FUNCTION_NAMES: [&str; 6] = ["ceil", "floor", "round", "min", "max", "abs"];
const MAX_DEPTH: usize = 100;
const MAX_LEN: usize = 500;

fn func_arity(name: &str) -> Option<(usize, usize)> {
    match name {
        "ceil" | "floor" | "abs" => Some((1, 1)),
        "round" => Some((1, 2)),
        "min" | "max" => Some((1, 99)),
        _ => None,
    }
}

/// A parse or evaluation error (`FormulaError`): the message and, when known, the UTF-16 position.
#[derive(Debug, Clone, PartialEq)]
pub struct FErr {
    pub message: String,
    pub pos: Option<usize>,
}
fn ferr(message: String, pos: Option<usize>) -> FErr {
    FErr { message, pos }
}

/// A parsed formula tree.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Num(f64),
    Var(String),
    Neg(Box<Node>),
    /// op, a, b, pos (only `*` and `/` carry their token's position)
    Bin(char, Box<Node>, Box<Node>, Option<usize>),
    Call(&'static str, Vec<Node>, usize),
}

impl Node {
    /// The tree as the JS builds it (`{k: 'bin', op, a, b, pos}` …).
    pub fn to_json(&self) -> Value {
        match self {
            Node::Num(v) => json!({ "k": "num", "v": jv::n(*v) }),
            Node::Var(n) => json!({ "k": "var", "name": n }),
            Node::Neg(a) => json!({ "k": "neg", "a": a.to_json() }),
            Node::Bin(op, a, b, pos) => {
                let mut o = json!({ "k": "bin", "op": op.to_string(), "a": a.to_json(), "b": b.to_json() });
                if let (Some(p), Value::Object(m)) = (pos, &mut o) {
                    m.insert("pos".into(), jv::n(*p as f64));
                }
                o
            }
            Node::Call(name, args, pos) => {
                json!({ "k": "call", "name": name, "args": args.iter().map(Node::to_json).collect::<Vec<_>>(), "pos": *pos as f64 })
            }
        }
    }
}

#[derive(Debug, Clone)]
enum TokK {
    Num(f64),
    Id(String),
    Op(char),
    End,
}
#[derive(Debug, Clone)]
struct Tok {
    k: TokK,
    pos: usize,
}
impl Tok {
    /// `tok.v ?? tok.t`
    fn text(&self) -> F {
        match &self.k {
            TokK::Num(v) => F::N(*v),
            TokK::Id(s) => F::S(s.clone()),
            TokK::Op(c) => F::S(c.to_string()),
            TokK::End => F::S("end".into()),
        }
    }
}

fn is_ws16(u: u16) -> bool {
    char::from_u32(u32::from(u)).is_some_and(jv::is_space)
}

fn tokenize(src: &[u16]) -> Result<Vec<Tok>, FErr> {
    let mut out = Vec::new();
    let mut i = 0usize;
    let digit = |u: Option<&u16>| u.is_some_and(|c| (u16::from(b'0')..=u16::from(b'9')).contains(c));
    let ch = |u: Option<&u16>, c: u8| u.is_some_and(|x| *x == u16::from(c));
    while i < src.len() {
        let c = src.get(i).copied().unwrap_or(0);
        if is_ws16(c) {
            i = i.saturating_add(1);
            continue;
        }
        // number: (\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?
        let mut j = i;
        let mut ok = false;
        if digit(src.get(j)) {
            while digit(src.get(j)) {
                j = j.saturating_add(1);
            }
            if ch(src.get(j), b'.') {
                j = j.saturating_add(1);
                while digit(src.get(j)) {
                    j = j.saturating_add(1);
                }
            }
            ok = true;
        } else if ch(src.get(j), b'.') && digit(src.get(j.saturating_add(1))) {
            j = j.saturating_add(1);
            while digit(src.get(j)) {
                j = j.saturating_add(1);
            }
            ok = true;
        }
        if ok {
            if ch(src.get(j), b'e') || ch(src.get(j), b'E') {
                let mut k = j.saturating_add(1);
                if ch(src.get(k), b'+') || ch(src.get(k), b'-') {
                    k = k.saturating_add(1);
                }
                if digit(src.get(k)) {
                    while digit(src.get(k)) {
                        k = k.saturating_add(1);
                    }
                    j = k;
                }
            }
            let text = jv::from16(src.get(i..j).unwrap_or(&[]));
            out.push(Tok { k: TokK::Num(jv::str_to_number(&text)), pos: i });
            i = j;
            continue;
        }
        let alpha = |u: Option<&u16>| u.is_some_and(|c| char::from_u32(u32::from(*c)).is_some_and(|c| c.is_ascii_alphabetic() || c == '_'));
        if alpha(src.get(i)) {
            let mut j = i.saturating_add(1);
            while alpha(src.get(j)) || digit(src.get(j)) {
                j = j.saturating_add(1);
            }
            out.push(Tok { k: TokK::Id(jv::from16(src.get(i..j).unwrap_or(&[]))), pos: i });
            i = j;
            continue;
        }
        let op = match char::from_u32(u32::from(c)) {
            Some('+') => Some('+'),
            Some('-' | '−' | '–') => Some('-'),
            Some('*' | '×') => Some('*'),
            Some('/' | '÷') => Some('/'),
            Some('(') => Some('('),
            Some(')') => Some(')'),
            Some(',') => Some(','),
            _ => None,
        };
        if let Some(o) = op {
            out.push(Tok { k: TokK::Op(o), pos: i });
            i = i.saturating_add(1);
            continue;
        }
        // Spec gap (JS-engine quirk): a lone surrogate (half of an astral character) is shown as U+FFFD
        let cs = jv::from16(&[c]);
        return Err(ferr(tf("f.err.char", &[("c", F::S(cs))]), Some(i)));
    }
    out.push(Tok { k: TokK::End, pos: src.len() });
    Ok(out)
}

/// A parsed formula: its tree and the variables it uses (in order of first use, with the names as given).
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub ast: Node,
    pub uses: Vec<String>,
}

struct Parser<'a> {
    tk: Vec<Tok>,
    i: usize,
    depth: usize,
    vars: &'a [&'a str],
    allowed: Vec<(String, String)>,
    uses: Vec<String>,
}

impl Parser<'_> {
    fn peek(&self) -> &Tok {
        // the token list always ends with End, and i never passes it
        self.tk.get(self.i).or_else(|| self.tk.last()).unwrap_or(&END)
    }
    fn is_op(&self, c: char) -> bool {
        matches!(self.peek().k, TokK::Op(o) if o == c)
    }
    fn bump(&mut self) {
        if self.i.saturating_add(1) < self.tk.len() {
            self.i = self.i.saturating_add(1);
        }
    }
    fn take_close(&mut self) -> Result<(), FErr> {
        if self.is_op(')') {
            self.bump();
            return Ok(());
        }
        let got = self.peek().clone();
        let msg = if matches!(got.k, TokK::End) { t("f.err.paren") } else { tf("f.err.expect_before", &[("tok", got.text())]) };
        Err(ferr(msg, Some(got.pos)))
    }
    fn expr(&mut self) -> Result<Node, FErr> {
        let mut n = self.term()?;
        while self.is_op('+') || self.is_op('-') {
            let op = if self.is_op('+') { '+' } else { '-' };
            self.bump();
            let b = self.term()?;
            n = Node::Bin(op, Box::new(n), Box::new(b), None);
        }
        Ok(n)
    }
    fn term(&mut self) -> Result<Node, FErr> {
        let mut n = self.unary()?;
        while self.is_op('*') || self.is_op('/') {
            let op = if self.is_op('*') { '*' } else { '/' };
            let pos = self.peek().pos;
            self.bump();
            let b = self.unary()?;
            n = Node::Bin(op, Box::new(n), Box::new(b), Some(pos));
        }
        Ok(n)
    }
    fn unary(&mut self) -> Result<Node, FErr> {
        self.depth = self.depth.saturating_add(1);
        let r = if self.depth > MAX_DEPTH {
            Err(ferr(t("f.err.deep"), Some(self.peek().pos)))
        } else if self.is_op('-') {
            self.bump();
            self.unary().map(|a| Node::Neg(Box::new(a)))
        } else if self.is_op('+') {
            self.bump();
            self.unary()
        } else {
            self.primary()
        };
        self.depth = self.depth.saturating_sub(1);
        r
    }
    fn primary(&mut self) -> Result<Node, FErr> {
        let tok = self.peek().clone();
        match &tok.k {
            TokK::Num(v) => {
                self.bump();
                Ok(Node::Num(*v))
            }
            TokK::Op('(') => {
                self.bump();
                let n = self.expr()?;
                self.take_close()?;
                Ok(n)
            }
            TokK::Id(raw) => {
                self.bump();
                let name = jv::lower(raw);
                if self.is_op('(') {
                    let Some(fname) = FUNCTION_NAMES.iter().find(|f| **f == name).copied() else {
                        return Err(ferr(
                            tf("f.err.func", &[("name", F::S(raw.clone())), ("list", F::S(FUNCTION_NAMES.join(", ")))]),
                            Some(tok.pos),
                        ));
                    };
                    let (mn, mx) = func_arity(fname).unwrap_or((1, 1));
                    self.bump();
                    let mut args = vec![self.expr()?];
                    while self.is_op(',') {
                        self.bump();
                        args.push(self.expr()?);
                    }
                    self.take_close()?;
                    if args.len() < mn || args.len() > mx {
                        let msg = if mn == mx {
                            tf("f.err.args", &[("name", F::S(name.clone())), ("n", F::from(mn))])
                        } else if mx > 9 {
                            tf("f.err.args.more", &[("name", F::S(name.clone())), ("n", F::from(mn))])
                        } else {
                            tf("f.err.args.range", &[("name", F::S(name.clone())), ("min", F::from(mn)), ("max", F::from(mx))])
                        };
                        return Err(ferr(msg, Some(tok.pos)));
                    }
                    return Ok(Node::Call(fname, args, tok.pos));
                }
                let Some(v) = self.allowed.iter().rev().find(|(k, _)| *k == name).map(|(_, v)| v.clone()) else {
                    let msg = if self.vars.is_empty() {
                        tf("f.err.var", &[("name", F::S(raw.clone()))])
                    } else {
                        tf("f.err.var.use", &[("name", F::S(raw.clone())), ("list", F::S(self.vars.join(", ")))])
                    };
                    return Err(ferr(msg, Some(tok.pos)));
                };
                if !self.uses.contains(&v) {
                    self.uses.push(v.clone());
                }
                Ok(Node::Var(v))
            }
            TokK::Op(c @ (')' | ',')) => Err(ferr(tf("f.err.missing", &[("tok", F::S(c.to_string()))]), Some(tok.pos))),
            TokK::End => Err(ferr(t("f.err.end"), Some(tok.pos))),
            TokK::Op(_) => Err(ferr(tf("f.err.unexpected", &[("tok", tok.text())]), Some(tok.pos))),
        }
    }
}

static END: Tok = Tok { k: TokK::End, pos: 0 };

/// Parse a formula. `vars` is the list of variable names it may use.
pub fn parse(src: &str, vars: &[&str]) -> Result<Parsed, FErr> {
    if cc_m3d::js::trim(src).is_empty() {
        return Err(ferr(t("f.err.empty"), Some(0)));
    }
    let u = jv::utf16(src);
    if u.len() > MAX_LEN {
        return Err(ferr(tf("f.err.long", &[("n", F::N(500.0))]), Some(0)));
    }
    let tk = tokenize(&u)?;
    let mut p =
        Parser { tk, i: 0, depth: 0, vars, allowed: vars.iter().map(|v| (jv::lower(v), (*v).to_string())).collect(), uses: Vec::new() };
    let ast = p.expr()?;
    if !matches!(p.peek().k, TokK::End) {
        let tok = p.peek().clone();
        let msg = if matches!(tok.k, TokK::Op(')')) { t("f.err.close") } else { tf("f.err.operator", &[("tok", tok.text())]) };
        return Err(ferr(msg, Some(tok.pos)));
    }
    Ok(Parsed { ast, uses: p.uses })
}

/// Evaluate a parsed tree; `values(name)` is the variable's value when it has one (a number, maybe not finite).
pub fn evaluate(ast: &Node, values: &dyn Fn(&str) -> Option<f64>) -> Result<f64, FErr> {
    let r = ev(ast, values, 0)?;
    if !r.is_finite() {
        return Err(ferr(t("f.err.nan"), None));
    }
    Ok(r)
}

fn ev(n: &Node, values: &dyn Fn(&str) -> Option<f64>, depth: usize) -> Result<f64, FErr> {
    // a parsed tree is at most MAX_DEPTH unary levels deep; a hand-made one is cut off the same way
    if depth > 4 * MAX_DEPTH + 8 {
        return Err(ferr(t("f.err.deep"), None));
    }
    let d = depth.saturating_add(1);
    match n {
        Node::Num(v) => Ok(*v),
        Node::Var(name) => match values(name) {
            Some(v) if v.is_finite() => Ok(v),
            _ => Err(ferr(tf("f.err.novalue", &[("name", F::S(name.clone()))]), None)),
        },
        Node::Neg(a) => Ok(-ev(a, values, d)?),
        Node::Bin(op, a, b, pos) => {
            let x = ev(a, values, d)?;
            let y = ev(b, values, d)?;
            match op {
                '+' => Ok(x + y),
                '-' => Ok(x - y),
                '*' => Ok(x * y),
                _ => {
                    if y == 0.0 {
                        return Err(ferr(t("f.err.zero"), *pos));
                    }
                    Ok(x / y)
                }
            }
        }
        Node::Call(name, args, _) => {
            let mut a = Vec::with_capacity(args.len());
            for x in args {
                a.push(ev(x, values, d)?);
            }
            call(name, &a)
        }
    }
}

fn call(name: &str, a: &[f64]) -> Result<f64, FErr> {
    let x = a.first().copied().unwrap_or(f64::NAN);
    match name {
        "ceil" => Ok((x - EPS * cc_m3d::js::max(1.0, x.abs())).ceil()),
        "floor" => Ok((x + EPS * cc_m3d::js::max(1.0, x.abs())).floor()),
        "round" => {
            let dg = a.get(1).copied().unwrap_or(0.0);
            if !cc_m3d::js::is_integer(dg) || dg < 0.0 || dg > 10.0 {
                return Err(ferr(t("f.err.round"), None));
            }
            let f = crate::jsmath::pow(10.0, dg);
            let y = x.abs() * f;
            let r = (y + 0.5 + EPS * cc_m3d::js::max(1.0, y)).floor();
            Ok((if x < 0.0 { -r } else { r }) / f)
        }
        "min" => Ok(a.iter().fold(f64::INFINITY, |m, v| cc_m3d::js::min(m, *v))),
        "max" => Ok(a.iter().fold(f64::NEG_INFINITY, |m, v| cc_m3d::js::max(m, *v))),
        "abs" => Ok(x.abs()),
        _ => Err(ferr(t("f.err.bad"), None)),
    }
}

/// Parse + evaluate: the variables are `values` (name, value) in order; their names are the ones the formula may use.
pub fn compute(src: &str, values: &[(String, f64)]) -> Result<f64, FErr> {
    let names: Vec<&str> = values.iter().map(|(k, _)| k.as_str()).collect();
    let p = parse(src, &names)?;
    evaluate(&p.ast, &|name| values.iter().rev().find(|(k, _)| k == name).map(|(_, v)| *v))
}
