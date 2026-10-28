//! A GeoJSON text read in one pass into the parts readGeoJSON looks at (geojson.js `JSON.parse` + the walk), without
//! building a JSON tree for the geometry: a position's numbers land in the `Vec<f64>` the feature keeps.
//!
//! The grammar, the number values and the failures are serde_json's (`from_str` with float_roundtrip and
//! preserve_order), which read these files before: the same texts are refused (bad syntax, nesting past 127, a number
//! past ±1.8e308, a lone surrogate escape, a control character in a string, anything after the value), a key given
//! twice keeps its first place and its last value, and numbers are the nearest double (as JS's `Number`). Properties
//! and the `crs` member are built as serde_json values exactly as `from_str` builds them (integers as integers).
//! `tests` checks the whole read against the tree-based reader on generated and hand-written texts.

use serde_json::{Map, Number, Value};

/// The text is not JSON (the reader says geojson.json).
#[derive(Debug)]
pub struct NotJson;
type P<T> = Result<T, NotJson>;

/// serde_json's recursion limit: the 128th open bracket is refused.
const MAX_DEPTH: u32 = 128;

/// A `coordinates` value (or anything inside one).
#[derive(Debug)]
pub enum C {
    Num(f64),
    Null,
    /// a string, boolean or object: its JS truthiness
    Other(bool),
    /// a non-empty array of numbers only
    Nums(Vec<f64>),
    /// any other array
    Arr(Vec<C>),
}
impl C {
    pub fn truthy(&self) -> bool {
        match self {
            C::Num(x) => *x != 0.0 && !x.is_nan(),
            C::Null => false,
            C::Other(t) => *t,
            C::Nums(_) | C::Arr(_) => true,
        }
    }
}

/// A geometry slot's value (`feature.geometry`, an item of `geometries`).
#[derive(Debug)]
pub enum G {
    Null,
    /// a number, string or boolean
    Scalar,
    /// an array (an object to `typeof`, with no members)
    Arr,
    Obj(Box<GeomJ>),
}

/// The members of an object that a geometry walk reads.
#[derive(Debug, Default)]
pub struct GeomJ {
    /// `type` when it is a string
    pub ty: Option<String>,
    pub coords: Option<C>,
    pub geometries: Option<Geoms>,
}

#[derive(Debug)]
pub enum Geoms {
    List(Vec<G>),
    /// not an array: its truthiness
    Other(bool),
}
impl Geoms {
    pub fn truthy(&self) -> bool {
        match self {
            Geoms::List(_) => true,
            Geoms::Other(t) => *t,
        }
    }
}

/// An item of `features`.
#[derive(Debug)]
pub enum FeatJ {
    Scalar,
    /// an array: an object with no members
    Arr,
    Obj {
        properties: Option<Value>,
        geometry: Option<G>,
    },
}

#[derive(Debug)]
pub enum Feats {
    List(Vec<FeatJ>),
    NotList,
}

/// The top-level object's members the reader uses (as a FeatureCollection, a Feature or a bare geometry).
#[derive(Debug, Default)]
pub struct TopObj {
    pub geom: GeomJ,
    pub features: Option<Feats>,
    pub crs: Option<Value>,
    pub properties: Option<Value>,
    pub geometry: Option<G>,
}

#[derive(Debug)]
pub enum Top {
    Scalar,
    Arr,
    Obj(Box<TopObj>),
}

struct Rd<'a> {
    s: &'a str,
    b: &'a [u8],
    i: usize,
    depth: u32,
}

fn hex(c: u8) -> P<u16> {
    match c {
        b'0'..=b'9' => Ok(u16::from(c.wrapping_sub(b'0'))),
        b'a'..=b'f' => Ok(u16::from(c.wrapping_sub(b'a')).wrapping_add(10)),
        b'A'..=b'F' => Ok(u16::from(c.wrapping_sub(b'A')).wrapping_add(10)),
        _ => Err(NotJson),
    }
}

impl<'a> Rd<'a> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }
    fn bump(&mut self) {
        self.i = self.i.saturating_add(1);
    }
    fn ws(&mut self) {
        while let Some(b' ' | b'\n' | b'\t' | b'\r') = self.peek() {
            self.bump();
        }
    }
    /// Past white space: the next byte (not consumed).
    fn next(&mut self) -> P<u8> {
        self.ws();
        self.peek().ok_or(NotJson)
    }
    fn lit(&mut self, w: &[u8]) -> P<()> {
        let end = self.i.saturating_add(w.len());
        if self.b.get(self.i..end) == Some(w) {
            self.i = end;
            Ok(())
        } else {
            Err(NotJson)
        }
    }
    fn enter(&mut self) -> P<()> {
        self.depth = self.depth.saturating_add(1);
        if self.depth >= MAX_DEPTH { Err(NotJson) } else { Ok(()) }
    }
    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    // ------------------------------------------------------------------------------------------- scalars

    /// A number at the cursor: its value and whether it was written as an integer.
    fn number(&mut self) -> P<(f64, &'a str, bool)> {
        let start = self.i;
        if self.peek() == Some(b'-') {
            self.bump();
        }
        match self.peek() {
            Some(b'0') => self.bump(),
            Some(b'1'..=b'9') => {
                while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    self.bump();
                }
            }
            _ => return Err(NotJson),
        }
        let mut int = true;
        if self.peek() == Some(b'.') {
            int = false;
            self.bump();
            if !self.peek().is_some_and(|c| c.is_ascii_digit()) {
                return Err(NotJson);
            }
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.bump();
            }
        }
        if let Some(b'e' | b'E') = self.peek() {
            int = false;
            self.bump();
            if let Some(b'+' | b'-') = self.peek() {
                self.bump();
            }
            if !self.peek().is_some_and(|c| c.is_ascii_digit()) {
                return Err(NotJson);
            }
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.bump();
            }
        }
        let text = self.s.get(start..self.i).ok_or(NotJson)?;
        let x: f64 = text.parse().map_err(|_| NotJson)?;
        if x.is_infinite() {
            return Err(NotJson);
        }
        Ok((x, text, int))
    }

    /// A number as serde_json's Value holds it (u64, then i64 except −0, then f64).
    fn number_value(&mut self) -> P<Value> {
        let (x, text, int) = self.number()?;
        if int {
            if let Ok(u) = text.parse::<u64>() {
                return Ok(Value::Number(Number::from(u)));
            }
            if let Ok(i) = text.parse::<i64>()
                && i < 0
            {
                return Ok(Value::Number(Number::from(i)));
            }
        }
        Ok(Number::from_f64(x).map_or(Value::Null, Value::Number))
    }

    /// A string at the cursor (after its opening quote is checked here): borrowed when it holds no escape.
    fn string(&mut self) -> P<std::borrow::Cow<'a, str>> {
        if self.peek() != Some(b'"') {
            return Err(NotJson);
        }
        self.bump();
        let start = self.i;
        loop {
            match self.peek() {
                None => return Err(NotJson),
                Some(b'"') => {
                    let s = self.s.get(start..self.i).ok_or(NotJson)?;
                    self.bump();
                    return Ok(std::borrow::Cow::Borrowed(s));
                }
                Some(b'\\') => break,
                Some(c) if c < 0x20 => return Err(NotJson),
                Some(_) => self.bump(),
            }
        }
        // escapes: the rest built
        let mut out = String::from(self.s.get(start..self.i).ok_or(NotJson)?);
        self.string_rest(&mut out)?;
        Ok(std::borrow::Cow::Owned(out))
    }
    fn hex4(&mut self) -> P<u16> {
        let mut n: u16 = 0;
        for _ in 0..4 {
            let c = self.peek().ok_or(NotJson)?;
            n = (n << 4) | hex(c)?;
            self.bump();
        }
        Ok(n)
    }
    fn string_rest(&mut self, out: &mut String) -> P<()> {
        loop {
            let run = self.i;
            while let Some(c) = self.peek() {
                if c == b'"' || c == b'\\' || c < 0x20 {
                    break;
                }
                self.bump();
            }
            out.push_str(self.s.get(run..self.i).ok_or(NotJson)?);
            match self.peek() {
                Some(b'"') => {
                    self.bump();
                    return Ok(());
                }
                Some(b'\\') => {
                    self.bump();
                    let e = self.peek().ok_or(NotJson)?;
                    self.bump();
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let n = self.hex4()?;
                            let code = if (0xDC00..=0xDFFF).contains(&n) {
                                return Err(NotJson);
                            } else if (0xD800..=0xDBFF).contains(&n) {
                                self.lit(b"\\u")?;
                                let n2 = self.hex4()?;
                                if !(0xDC00..=0xDFFF).contains(&n2) {
                                    return Err(NotJson);
                                }
                                (u32::from(n.wrapping_sub(0xD800)) << 10 | u32::from(n2.wrapping_sub(0xDC00))).wrapping_add(0x1_0000)
                            } else {
                                u32::from(n)
                            };
                            out.push(char::from_u32(code).ok_or(NotJson)?);
                        }
                        _ => return Err(NotJson),
                    }
                }
                _ => return Err(NotJson),
            }
        }
    }

    // ------------------------------------------------------------------------------------------- any value

    /// Any value as serde_json's `Value` (preserve_order: a key given twice keeps its first place, its last value).
    fn value(&mut self) -> P<Value> {
        match self.next()? {
            b'n' => self.lit(b"null").map(|()| Value::Null),
            b't' => self.lit(b"true").map(|()| Value::Bool(true)),
            b'f' => self.lit(b"false").map(|()| Value::Bool(false)),
            b'"' => Ok(Value::String(self.string()?.into_owned())),
            b'[' => {
                self.enter()?;
                self.bump();
                let mut a = Vec::new();
                if self.next()? == b']' {
                    self.bump();
                } else {
                    loop {
                        a.push(self.value()?);
                        match self.next()? {
                            b',' => self.bump(),
                            b']' => {
                                self.bump();
                                break;
                            }
                            _ => return Err(NotJson),
                        }
                    }
                }
                self.leave();
                Ok(Value::Array(a))
            }
            b'{' => {
                let mut m = Map::new();
                self.object(|r, k| {
                    let v = r.value()?;
                    m.insert(k.into_owned(), v);
                    Ok(())
                })?;
                Ok(Value::Object(m))
            }
            _ => self.number_value(),
        }
    }

    /// Any value skipped (checked as `value` checks it, nothing built).
    fn skip(&mut self) -> P<()> {
        match self.next()? {
            b'n' => self.lit(b"null"),
            b't' => self.lit(b"true"),
            b'f' => self.lit(b"false"),
            b'"' => self.string().map(drop),
            b'[' => self.array(|r| r.skip()),
            b'{' => self.object(|r, _| r.skip()),
            _ => self.number().map(drop),
        }
    }

    /// A value's kind and truthiness, skipped.
    fn truthiness(&mut self) -> P<bool> {
        match self.next()? {
            b'n' => self.lit(b"null").map(|()| false),
            b't' => self.lit(b"true").map(|()| true),
            b'f' => self.lit(b"false").map(|()| false),
            b'"' => self.string().map(|s| !s.is_empty()),
            b'[' => self.array(|r| r.skip()).map(|()| true),
            b'{' => self.object(|r, _| r.skip()).map(|()| true),
            _ => self.number().map(|(x, _, _)| x != 0.0),
        }
    }

    /// `[ item, … ]` at the cursor, each item handed to `f`.
    fn array(&mut self, mut f: impl FnMut(&mut Self) -> P<()>) -> P<()> {
        if self.next()? != b'[' {
            return Err(NotJson);
        }
        self.enter()?;
        self.bump();
        if self.next()? == b']' {
            self.bump();
        } else {
            loop {
                f(self)?;
                match self.next()? {
                    b',' => self.bump(),
                    b']' => {
                        self.bump();
                        break;
                    }
                    _ => return Err(NotJson),
                }
            }
        }
        self.leave();
        Ok(())
    }

    /// `{ "key": value, … }` at the cursor, each key handed to `f` with the cursor on its value.
    fn object(&mut self, mut f: impl FnMut(&mut Self, std::borrow::Cow<'a, str>) -> P<()>) -> P<()> {
        if self.next()? != b'{' {
            return Err(NotJson);
        }
        self.enter()?;
        self.bump();
        if self.next()? == b'}' {
            self.bump();
        } else {
            loop {
                if self.next()? != b'"' {
                    return Err(NotJson);
                }
                let k = self.string()?;
                if self.next()? != b':' {
                    return Err(NotJson);
                }
                self.bump();
                f(self, k)?;
                match self.next()? {
                    b',' => self.bump(),
                    b'}' => {
                        self.bump();
                        break;
                    }
                    _ => return Err(NotJson),
                }
            }
        }
        self.leave();
        Ok(())
    }

    // ------------------------------------------------------------------------------------------- GeoJSON parts

    fn coords(&mut self) -> P<C> {
        match self.next()? {
            b'n' => self.lit(b"null").map(|()| C::Null),
            b't' => self.lit(b"true").map(|()| C::Other(true)),
            b'f' => self.lit(b"false").map(|()| C::Other(false)),
            b'"' => self.string().map(|s| C::Other(!s.is_empty())),
            b'{' => self.object(|r, _| r.skip()).map(|()| C::Other(true)),
            b'[' => {
                // numbers only (the common case: a position) in one list; anything else turns it into a list of values
                let mut nums: Vec<f64> = Vec::new();
                let mut items: Option<Vec<C>> = None;
                self.array(|r| {
                    let c = r.next()?;
                    if items.is_none() && (c == b'-' || c.is_ascii_digit()) {
                        nums.push(r.number()?.0);
                        return Ok(());
                    }
                    let it = items.get_or_insert_with(|| nums.drain(..).map(C::Num).collect());
                    it.push(r.coords()?);
                    Ok(())
                })?;
                Ok(match items {
                    Some(it) => C::Arr(it),
                    None if nums.is_empty() => C::Arr(Vec::new()),
                    None => C::Nums(nums),
                })
            }
            _ => self.number().map(|(x, _, _)| C::Num(x)),
        }
    }

    fn geom(&mut self) -> P<G> {
        match self.next()? {
            b'n' => self.lit(b"null").map(|()| G::Null),
            b'[' => self.array(|r| r.skip()).map(|()| G::Arr),
            b'{' => {
                let mut g = GeomJ::default();
                self.object(|r, k| r.geom_member(&mut g, &k))?;
                Ok(G::Obj(Box::new(g)))
            }
            _ => self.skip().map(|()| G::Scalar),
        }
    }

    /// A member of an object read as a geometry: `type`, `coordinates`, `geometries`; the rest skipped.
    fn geom_member(&mut self, g: &mut GeomJ, k: &str) -> P<()> {
        match k {
            "type" => {
                g.ty = match self.next()? {
                    b'"' => Some(self.string()?.into_owned()),
                    _ => {
                        self.skip()?;
                        None
                    }
                };
            }
            "coordinates" => g.coords = Some(self.coords()?),
            "geometries" => {
                g.geometries = Some(if self.next()? == b'[' {
                    let mut l = Vec::new();
                    self.array(|r| {
                        l.push(r.geom()?);
                        Ok(())
                    })?;
                    Geoms::List(l)
                } else {
                    Geoms::Other(self.truthiness()?)
                });
            }
            _ => self.skip()?,
        }
        Ok(())
    }

    fn feature(&mut self) -> P<FeatJ> {
        match self.next()? {
            b'[' => self.array(|r| r.skip()).map(|()| FeatJ::Arr),
            b'{' => {
                let mut properties = None;
                let mut geometry = None;
                self.object(|r, k| {
                    match &*k {
                        "properties" => properties = Some(r.value()?),
                        "geometry" => geometry = Some(r.geom()?),
                        _ => r.skip()?,
                    }
                    Ok(())
                })?;
                Ok(FeatJ::Obj { properties, geometry })
            }
            _ => self.skip().map(|()| FeatJ::Scalar),
        }
    }

    fn top(&mut self) -> P<Top> {
        let t = match self.next()? {
            b'[' => self.array(|r| r.skip()).map(|()| Top::Arr)?,
            b'{' => {
                let mut t = TopObj::default();
                self.object(|r, k| {
                    match &*k {
                        "features" => {
                            t.features = Some(if r.next()? == b'[' {
                                let mut l = Vec::new();
                                r.array(|r| {
                                    l.push(r.feature()?);
                                    Ok(())
                                })?;
                                Feats::List(l)
                            } else {
                                r.skip()?;
                                Feats::NotList
                            });
                        }
                        "crs" => t.crs = Some(r.value()?),
                        "properties" => t.properties = Some(r.value()?),
                        "geometry" => t.geometry = Some(r.geom()?),
                        other => r.geom_member(&mut t.geom, other)?,
                    }
                    Ok(())
                })?;
                Top::Obj(Box::new(t))
            }
            _ => self.skip().map(|()| Top::Scalar)?,
        };
        self.ws();
        if self.i < self.b.len() { Err(NotJson) } else { Ok(t) }
    }
}

/// `JSON.parse(text)` of a GeoJSON text, into the parts the reader walks.
pub fn parse(text: &str) -> P<Top> {
    Rd { s: text, b: text.as_bytes(), i: 0, depth: 0 }.top()
}

/// `JSON.parse(text)` as serde_json's Value (for the tests and the fuzz target: the same grammar as [`parse`]).
pub fn parse_value(text: &str) -> P<Value> {
    let mut r = Rd { s: text, b: text.as_bytes(), i: 0, depth: 0 };
    let v = r.value()?;
    r.ws();
    if r.i < r.b.len() { Err(NotJson) } else { Ok(v) }
}
