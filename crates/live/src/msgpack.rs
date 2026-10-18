//! MessagePack as Studio Live writes and reads it (docs/LIVE-PROTOCOL.md § 2; live-client.js `pack` / `unpack`):
//! integers in the smallest encoding, every other number float64, byte strings as `bin`, maps with string keys in their
//! order. Reading takes any valid encoding (float32, wide integers, str8 for a short string, ext types), refuses a
//! truncated input, trailing bytes or the unused type 0xc1, and is bounded: nesting ≤ 512, and a container's count is
//! checked against the bytes left before anything is allocated.

/// A decoded value (the JS value live-client.js gives).
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Value {
    #[default]
    Nil,
    Bool(bool),
    /// any integer that fits (JS numbers up to 2^53, BigInt beyond)
    Int(i128),
    F64(f64),
    Str(String),
    Bin(Vec<u8>),
    Arr(Vec<Value>),
    /// keys as strings (`String(k)` for non-string keys), in order; a repeated key keeps its first place, last value
    Map(Vec<(String, Value)>),
    /// an extension type (never written; read as `{ext, data}`)
    Ext(i8, Vec<u8>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Map(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        if let Value::Str(s) = self { Some(s) } else { None }
    }
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::Int(i) => u64::try_from(*i).ok(),
            Value::F64(f) if f.fract() == 0.0 && *f >= 0.0 && *f <= 9_007_199_254_740_991.0 => {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let u = *f as u64;
                Some(u)
            }
            _ => None,
        }
    }
    pub fn as_arr(&self) -> &[Value] {
        if let Value::Arr(a) = self { a } else { &[] }
    }
    pub fn is_nil(&self) -> bool {
        matches!(self, Value::Nil)
    }
    /// A map from (key, value) pairs.
    pub fn map(pairs: Vec<(&str, Value)>) -> Value {
        Value::Map(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::Str(s.to_string())
    }
}
impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::Str(s)
    }
}
impl From<u64> for Value {
    fn from(n: u64) -> Self {
        Value::Int(i128::from(n))
    }
}
impl From<u32> for Value {
    fn from(n: u32) -> Self {
        Value::Int(i128::from(n))
    }
}
impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}

/// Encoding failed: an integer outside 64 bits.
#[derive(Debug, Clone, PartialEq)]
pub struct EncodeError(pub String);

/// `pack(value)`.
pub fn pack(v: &Value) -> Result<Vec<u8>, EncodeError> {
    let mut out = Vec::with_capacity(64);
    write(&mut out, v, 0)?;
    Ok(out)
}

fn head(out: &mut Vec<u8>, n: usize, fix: u8, b16: u8, b32: u8) -> Result<(), EncodeError> {
    if n < 16 {
        out.push(fix | u8::try_from(n).unwrap_or(0));
    } else if let Ok(m) = u16::try_from(n) {
        out.push(b16);
        out.extend_from_slice(&m.to_be_bytes());
    } else {
        let m = u32::try_from(n).map_err(|_| EncodeError("msgpack: too many items".into()))?;
        out.push(b32);
        out.extend_from_slice(&m.to_be_bytes());
    }
    Ok(())
}

fn int(out: &mut Vec<u8>, v: i128) -> Result<(), EncodeError> {
    if v >= 0 {
        if v < 128 {
            out.push(u8::try_from(v).unwrap_or(0));
        } else if let Ok(b) = u8::try_from(v) {
            out.extend_from_slice(&[0xcc, b]);
        } else if let Ok(h) = u16::try_from(v) {
            out.push(0xcd);
            out.extend_from_slice(&h.to_be_bytes());
        } else if let Ok(w) = u32::try_from(v) {
            out.push(0xce);
            out.extend_from_slice(&w.to_be_bytes());
        } else if let Ok(d) = u64::try_from(v) {
            out.push(0xcf);
            out.extend_from_slice(&d.to_be_bytes());
        } else {
            return Err(EncodeError("msgpack: integer too large".into()));
        }
        return Ok(());
    }
    if v >= -32 {
        out.push(i8::try_from(v).unwrap_or(0).to_be_bytes()[0]);
    } else if let Ok(b) = i8::try_from(v) {
        out.push(0xd0);
        out.extend_from_slice(&b.to_be_bytes());
    } else if let Ok(h) = i16::try_from(v) {
        out.push(0xd1);
        out.extend_from_slice(&h.to_be_bytes());
    } else if let Ok(w) = i32::try_from(v) {
        out.push(0xd2);
        out.extend_from_slice(&w.to_be_bytes());
    } else if let Ok(d) = i64::try_from(v) {
        out.push(0xd3);
        out.extend_from_slice(&d.to_be_bytes());
    } else {
        return Err(EncodeError("msgpack: integer too small".into()));
    }
    Ok(())
}

fn write(out: &mut Vec<u8>, v: &Value, depth: usize) -> Result<(), EncodeError> {
    if depth > MAX_DEPTH {
        return Err(EncodeError("msgpack: too deep".into()));
    }
    match v {
        Value::Nil => out.push(0xc0),
        Value::Bool(b) => out.push(if *b { 0xc3 } else { 0xc2 }),
        Value::Int(i) => int(out, *i)?,
        Value::F64(f) => {
            // `Number.isInteger(v) && Number.isSafeInteger(v)` → an integer, as JS writes it
            if f.is_finite() && f.fract() == 0.0 && f.abs() <= 9_007_199_254_740_991.0 {
                #[allow(clippy::cast_possible_truncation)]
                let i = *f as i64;
                int(out, i128::from(i))?;
            } else {
                out.push(0xcb);
                out.extend_from_slice(&f.to_be_bytes());
            }
        }
        Value::Str(s) => {
            let n = s.len();
            if n < 32 {
                out.push(0xa0 | u8::try_from(n).unwrap_or(0));
            } else if let Ok(b) = u8::try_from(n) {
                out.extend_from_slice(&[0xd9, b]);
            } else if let Ok(h) = u16::try_from(n) {
                out.push(0xda);
                out.extend_from_slice(&h.to_be_bytes());
            } else {
                let w = u32::try_from(n).map_err(|_| EncodeError("msgpack: string too long".into()))?;
                out.push(0xdb);
                out.extend_from_slice(&w.to_be_bytes());
            }
            out.extend_from_slice(s.as_bytes());
        }
        Value::Bin(b) => {
            let n = b.len();
            if let Ok(x) = u8::try_from(n) {
                out.extend_from_slice(&[0xc4, x]);
            } else if let Ok(h) = u16::try_from(n) {
                out.push(0xc5);
                out.extend_from_slice(&h.to_be_bytes());
            } else {
                let w = u32::try_from(n).map_err(|_| EncodeError("msgpack: bytes too long".into()))?;
                out.push(0xc6);
                out.extend_from_slice(&w.to_be_bytes());
            }
            out.extend_from_slice(b);
        }
        Value::Arr(a) => {
            head(out, a.len(), 0x90, 0xdc, 0xdd)?;
            for x in a {
                write(out, x, depth.saturating_add(1))?;
            }
        }
        Value::Map(m) => {
            let live: Vec<&(String, Value)> = m.iter().collect();
            head(out, live.len(), 0x80, 0xde, 0xdf)?;
            for (k, x) in live {
                write(out, &Value::Str(k.clone()), depth.saturating_add(1))?;
                write(out, x, depth.saturating_add(1))?;
            }
        }
        Value::Ext(_, _) => return Err(EncodeError("msgpack: ext types are not written".into())),
    }
    Ok(())
}

const MAX_DEPTH: usize = 512;

/// Decoding failed.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodeError(pub &'static str);

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn left(&self) -> usize {
        self.b.len().saturating_sub(self.p)
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let end = self.p.checked_add(n).ok_or(DecodeError("msgpack: truncated"))?;
        let s = self.b.get(self.p..end).ok_or(DecodeError("msgpack: truncated"))?;
        self.p = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(*self.take(1)?.first().ok_or(DecodeError("msgpack: truncated"))?)
    }
    fn arr<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        self.take(N)?.try_into().map_err(|_| DecodeError("msgpack: truncated"))
    }
    fn n16(&mut self) -> Result<usize, DecodeError> {
        Ok(usize::from(u16::from_be_bytes(self.arr()?)))
    }
    fn n32(&mut self) -> Result<usize, DecodeError> {
        usize::try_from(u32::from_be_bytes(self.arr()?)).map_err(|_| DecodeError("msgpack: truncated"))
    }
    fn text(&mut self, n: usize) -> Result<Value, DecodeError> {
        Ok(Value::Str(String::from_utf8_lossy(self.take(n)?).into_owned()))
    }
    fn ext(&mut self, n: usize) -> Result<Value, DecodeError> {
        let t = i8::from_be_bytes([self.u8()?]);
        Ok(Value::Ext(t, self.take(n)?.to_vec()))
    }
    fn any(&mut self, depth: usize) -> Result<Value, DecodeError> {
        if depth > MAX_DEPTH {
            return Err(DecodeError("msgpack: too deep"));
        }
        let t = self.u8()?;
        if t < 0x80 {
            return Ok(Value::Int(i128::from(t)));
        }
        if t >= 0xe0 {
            return Ok(Value::Int(i128::from(i8::from_be_bytes([t]))));
        }
        if t & 0xf0 == 0x80 {
            return self.map(usize::from(t & 0x0f), depth);
        }
        if t & 0xf0 == 0x90 {
            return self.list(usize::from(t & 0x0f), depth);
        }
        if t & 0xe0 == 0xa0 {
            return self.text(usize::from(t & 0x1f));
        }
        Ok(match t {
            0xc0 => Value::Nil,
            0xc2 => Value::Bool(false),
            0xc3 => Value::Bool(true),
            0xc4 => {
                let n = usize::from(self.u8()?);
                Value::Bin(self.take(n)?.to_vec())
            }
            0xc5 => {
                let n = self.n16()?;
                Value::Bin(self.take(n)?.to_vec())
            }
            0xc6 => {
                let n = self.n32()?;
                Value::Bin(self.take(n)?.to_vec())
            }
            0xc7 => {
                let n = usize::from(self.u8()?);
                self.ext(n)?
            }
            0xc8 => {
                let n = self.n16()?;
                self.ext(n)?
            }
            0xc9 => {
                let n = self.n32()?;
                self.ext(n)?
            }
            0xca => Value::F64(f64::from(f32::from_be_bytes(self.arr()?))),
            0xcb => Value::F64(f64::from_be_bytes(self.arr()?)),
            0xcc => Value::Int(i128::from(self.u8()?)),
            0xcd => Value::Int(i128::from(u16::from_be_bytes(self.arr()?))),
            0xce => Value::Int(i128::from(u32::from_be_bytes(self.arr()?))),
            0xcf => Value::Int(i128::from(u64::from_be_bytes(self.arr()?))),
            0xd0 => Value::Int(i128::from(i8::from_be_bytes(self.arr()?))),
            0xd1 => Value::Int(i128::from(i16::from_be_bytes(self.arr()?))),
            0xd2 => Value::Int(i128::from(i32::from_be_bytes(self.arr()?))),
            0xd3 => Value::Int(i128::from(i64::from_be_bytes(self.arr()?))),
            0xd4 => self.ext(1)?,
            0xd5 => self.ext(2)?,
            0xd6 => self.ext(4)?,
            0xd7 => self.ext(8)?,
            0xd8 => self.ext(16)?,
            0xd9 => {
                let n = usize::from(self.u8()?);
                self.text(n)?
            }
            0xda => {
                let n = self.n16()?;
                self.text(n)?
            }
            0xdb => {
                let n = self.n32()?;
                self.text(n)?
            }
            0xdc => {
                let n = self.n16()?;
                self.list(n, depth)?
            }
            0xdd => {
                let n = self.n32()?;
                self.list(n, depth)?
            }
            0xde => {
                let n = self.n16()?;
                self.map(n, depth)?
            }
            0xdf => {
                let n = self.n32()?;
                self.map(n, depth)?
            }
            _ => return Err(DecodeError("msgpack: unknown type")),
        })
    }
    fn list(&mut self, n: usize, depth: usize) -> Result<Value, DecodeError> {
        if n > self.left() {
            return Err(DecodeError("msgpack: truncated"));
        }
        let mut a = Vec::with_capacity(n);
        for _ in 0..n {
            a.push(self.any(depth.saturating_add(1))?);
        }
        Ok(Value::Arr(a))
    }
    fn map(&mut self, n: usize, depth: usize) -> Result<Value, DecodeError> {
        if n.saturating_mul(2) > self.left() {
            return Err(DecodeError("msgpack: truncated"));
        }
        let mut m: Vec<(String, Value)> = Vec::with_capacity(n);
        for _ in 0..n {
            let k = self.any(depth.saturating_add(1))?;
            let key = key_string(&k);
            let v = self.any(depth.saturating_add(1))?;
            if key == "__proto__" {
                continue;
            }
            match m.iter_mut().find(|(x, _)| *x == key) {
                Some(slot) => slot.1 = v,
                None => m.push((key, v)),
            }
        }
        Ok(Value::Map(m))
    }
}

/// `String(k)` for a map key.
fn key_string(k: &Value) -> String {
    match k {
        Value::Str(s) => s.clone(),
        Value::Int(i) => i.to_string(),
        Value::F64(f) => cc_cad::js::num_to_string(*f),
        Value::Bool(b) => b.to_string(),
        Value::Nil => "null".into(),
        Value::Bin(b) => b.iter().map(u8::to_string).collect::<Vec<_>>().join(","),
        Value::Arr(_) => String::new(),
        Value::Map(_) | Value::Ext(..) => "[object Object]".into(),
    }
}

/// `unpack(bytes)`: exactly one value.
pub fn unpack(bytes: &[u8]) -> Result<Value, DecodeError> {
    let mut r = Reader { b: bytes, p: 0 };
    let v = r.any(0)?;
    if r.p != bytes.len() {
        return Err(DecodeError("msgpack: trailing bytes"));
    }
    Ok(v)
}
