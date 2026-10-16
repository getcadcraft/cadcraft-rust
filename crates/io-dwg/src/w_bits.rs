//! Bit-level and byte-level writers for DWG (port of lib/dwg/bits-write.js, Apache-2.0; ODA spec ch. 2).
//!
//! `BitWriter` writes MSB-first within each byte; multi-byte raw values inside the bit stream are little-endian.
//! Numbers come in as JS numbers (`f64`) and go through the same conversions as the JS (`Math.trunc(v) || 0`,
//! `v & 0xFF`, `v >>> 0` …). The JS `ObjWriter` (objects-write.js: separate handle and string streams) is the same
//! struct here with `obj` set: its `tv` goes to the string stream from R2007 and its `h` to the handle stream.

use std::cell::Cell;

use crate::util::{f64_to_u64_wrapping, to_i32, to_u32, u8_of_i32, usize_from_u32};
use crate::value::V;
use crate::w_js::{nn, to_number};

thread_local! {
    /// encode1252.lost: characters that could not be written (reset by write_dwg).
    pub static LOST: Cell<u32> = const { Cell::new(0) };
}

/// Version codes in order.
pub const ORDER: [&str; 8] = ["AC1012", "AC1014", "AC1015", "AC1018", "AC1021", "AC1024", "AC1027", "AC1032"];
pub fn v_index(v: &str) -> i32 {
    ORDER.iter().position(|x| *x == v).and_then(|i| i32::try_from(i).ok()).unwrap_or(-1)
}
pub fn at_least(v: &str, min: &str) -> bool {
    v_index(v) >= v_index(min)
}
/// header-vars.js VERSION_NUM.
pub fn version_num(v: &str) -> u32 {
    match v {
        "AC1012" => 12,
        "AC1014" => 14,
        "AC1015" => 15,
        "AC1018" => 18,
        "AC1021" => 21,
        "AC1024" => 24,
        "AC1027" => 27,
        "AC1032" => 32,
        _ => 0,
    }
}

#[derive(Clone, Debug)]
pub struct BitWriter {
    pub version: &'static str,
    pub r2004: bool,
    pub r2007: bool,
    pub r2010: bool,
    pub r2013: bool,
    pub r2018: bool,
    pub buf: Vec<u8>,
    pub bit: usize,
    // ---- ObjWriter (objects-write.js)
    pub obj: bool,
    pub inline: bool,
    pub hnd: Option<Box<BitWriter>>,
    pub strw: Option<Box<BitWriter>>,
    pub has_str: bool,
    pub size_at: usize,
}

/// A static version string for a version code (unknown → AC1015).
pub fn static_version(v: &str) -> &'static str {
    ORDER.iter().copied().find(|x| *x == v).unwrap_or("AC1015")
}

impl BitWriter {
    pub fn new(version: &str) -> Self {
        let version = static_version(version);
        BitWriter {
            version,
            r2004: at_least(version, "AC1018"),
            r2007: at_least(version, "AC1021"),
            r2010: at_least(version, "AC1024"),
            r2013: at_least(version, "AC1027"),
            r2018: at_least(version, "AC1032"),
            buf: Vec::with_capacity(256),
            bit: 0,
            obj: false,
            inline: false,
            hnd: None,
            strw: None,
            has_str: false,
            size_at: 0,
        }
    }
    /// objects-write.js `new ObjWriter(version, inline)`.
    pub fn obj_writer(version: &str, inline: bool) -> Self {
        let mut w = BitWriter::new(version);
        w.obj = true;
        w.inline = inline;
        w.hnd = Some(Box::new(BitWriter::new(version)));
        w.strw = Some(Box::new(BitWriter::new(version)));
        w
    }
    #[inline]
    fn grow(&mut self, n: usize) {
        let need = (self.bit.saturating_add(n) >> 3).saturating_add(2);
        if need > self.buf.len() {
            let target = need.max(self.buf.len().saturating_mul(2));
            self.buf.resize(target, 0);
        }
    }
    #[inline]
    pub fn b(&mut self, v: bool) {
        self.grow(1);
        if v && let Some(x) = self.buf.get_mut(self.bit >> 3) {
            *x |= 0x80u8 >> (self.bit & 7);
        }
        self.bit = self.bit.saturating_add(1);
    }
    /// B with a JS number (`if (v)`).
    pub fn bn(&mut self, v: f64) {
        self.b(v != 0.0 && !v.is_nan());
    }
    /// n plain bits, most significant first.
    pub fn bits(&mut self, v: u32, n: u32) {
        let mut i = n;
        while i > 0 {
            i = i.wrapping_sub(1);
            self.b((v.checked_shr(i).unwrap_or(0)) & 1 != 0);
        }
    }
    pub fn bb(&mut self, v: u32) {
        self.bits(v & 3, 2);
    }
    /// 3B: 0 ('0'), 2 ('10'), 6 ('110'), 7 ('111').
    pub fn b3(&mut self, v: u32) {
        match v {
            0 => self.b(false),
            2 => self.bits(2, 2),
            6 => self.bits(6, 3),
            _ => self.bits(7, 3),
        }
    }
    #[inline]
    pub fn rc8(&mut self, v: u8) {
        self.grow(8);
        let i = self.bit >> 3;
        let o = self.bit & 7;
        if o == 0 {
            if let Some(x) = self.buf.get_mut(i) {
                *x = v;
            }
        } else {
            if let Some(x) = self.buf.get_mut(i) {
                *x |= v >> o;
            }
            if let Some(x) = self.buf.get_mut(i.saturating_add(1)) {
                *x |= ((u32::from(v) << (8usize.wrapping_sub(o))) & 0xFF) as u8;
            }
        }
        self.bit = self.bit.saturating_add(8);
    }
    /// RC with a JS number (`v &= 0xFF`).
    pub fn rc(&mut self, v: f64) {
        self.rc8(u8_of_i32(to_i32(v)));
    }
    pub fn rs_i(&mut self, v: i32) {
        self.rc8(u8_of_i32(v));
        self.rc8(u8_of_i32(v >> 8));
    }
    pub fn rs(&mut self, v: f64) {
        self.rs_i(to_i32(v));
    }
    pub fn rl_u(&mut self, v: u32) {
        for k in 0..4u32 {
            self.rc8(crate::util::u8_of(v.wrapping_shr(k.wrapping_mul(8))));
        }
    }
    pub fn rl(&mut self, v: f64) {
        self.rl_u(to_u32(v));
    }
    pub fn rd(&mut self, v: f64) {
        for b in v.to_le_bytes() {
            self.rc8(b);
        }
    }
    /// BS: `v = Math.trunc(v) || 0`.
    pub fn bs(&mut self, v: f64) {
        let v = if v.is_nan() { 0.0 } else { v.trunc() };
        if v == 0.0 {
            self.bb(2);
        } else if v == 256.0 {
            self.bb(3);
        } else if v > 0.0 && v < 256.0 {
            self.bb(1);
            self.rc(v);
        } else {
            self.bb(0);
            self.rs_i(to_i32(v) & 0xFFFF);
        }
    }
    pub fn bl(&mut self, v: f64) {
        let v = if v.is_nan() { 0.0 } else { v.trunc() };
        if v == 0.0 {
            self.bb(2);
        } else if v > 0.0 && v < 256.0 {
            self.bb(1);
            self.rc(v);
        } else {
            self.bb(0);
            self.rl_u(to_u32(v));
        }
    }
    /// BLL (2.4): Spec gap — the length is 3 plain bits (see bits-write.js).
    pub fn bll(&mut self, v: f64) {
        let x = f64_to_u64_wrapping(v);
        self.bll_u(x);
    }
    pub fn bll_u(&mut self, mut x: u64) {
        let mut bytes = Vec::with_capacity(8);
        while x > 0 {
            bytes.push((x & 0xFF) as u8);
            x >>= 8;
        }
        self.bits(crate::util::u32_of_usize(bytes.len()), 3);
        for b in bytes {
            self.rc8(b);
        }
    }
    pub fn bd(&mut self, v: f64) {
        if v == 0.0 && !v.is_sign_negative() {
            self.bb(2);
        } else if v == 1.0 {
            self.bb(1);
        } else {
            self.bb(0);
            self.rd(v);
        }
    }
    /// DD (2.9): same as the default → 00, else a full RD (11).
    pub fn dd(&mut self, v: f64, def: f64) {
        if v == def || (v.is_nan() && def.is_nan()) {
            self.bb(0);
        } else {
            self.bb(3);
            self.rd(v);
        }
    }
    /// BT (2.10): `if (!v)` one bit.
    pub fn bt(&mut self, v: f64) {
        if v == 0.0 || v.is_nan() {
            self.b(true);
        } else {
            self.b(false);
            self.bd(v);
        }
    }
    /// BE (2.8).
    pub fn be(&mut self, x: f64, y: f64, z: f64) {
        if x == 0.0 && y == 0.0 && z == 1.0 {
            self.b(true);
        } else {
            self.b(false);
            self.bd(x);
            self.bd(y);
            self.bd(z);
        }
    }
    pub fn rd2(&mut self, x: f64, y: f64) {
        self.rd(x);
        self.rd(y);
    }
    pub fn rd3(&mut self, x: f64, y: f64, z: f64) {
        self.rd(x);
        self.rd(y);
        self.rd(z);
    }
    pub fn bd2(&mut self, x: f64, y: f64) {
        self.bd(x);
        self.bd(y);
    }
    pub fn bd3(&mut self, x: f64, y: f64, z: f64) {
        self.bd(x);
        self.bd(y);
        self.bd(z);
    }
    /// P3(p): `BD3(p?.x || 0, p?.y || 0, p?.z || 0)`.
    pub fn p3(&mut self, p: Option<&V>) {
        let g = |k: &str| or0(crate::w_js::field(p, k));
        let (x, y, z) = (g("x"), g("y"), g("z"));
        self.bd3(x, y, z);
    }
    pub fn p3n(&mut self, x: f64, y: f64, z: f64) {
        self.bd3(or0n(x), or0n(y), or0n(z));
    }
    pub fn p2(&mut self, p: Option<&V>) {
        let g = |k: &str| or0(crate::w_js::field(p, k));
        let (x, y) = (g("x"), g("y"));
        self.bd2(x, y);
    }
    /// CmColor (2.11) for non-entities.
    pub fn cmc(&mut self, color: Option<&V>) {
        let c = norm_color(color);
        if !self.r2004 {
            self.bs(c.index.unwrap_or(256.0));
            return;
        }
        self.bs(0.0);
        self.bl(f64::from(cmc_long(&c)));
        let names =
            (if c.name.as_ref().is_some_and(V::truthy) { 1 } else { 0 }) | (if c.book.as_ref().is_some_and(V::truthy) { 2 } else { 0 });
        self.rc(f64::from(names));
        if let Some(n) = c.name.as_ref().filter(|n| n.truthy()) {
            let s = crate::w_js::js_string(Some(n));
            self.tv(&s);
        }
        if let Some(n) = c.book.as_ref().filter(|n| n.truthy()) {
            let s = crate::w_js::js_string(Some(n));
            self.tv(&s);
        }
    }
    /// ENC (2.11, R2004+): BS flags | index, [BL RGB], [BL transparency]. R2000: BS index.
    pub fn enc(&mut self, color: Option<&V>, transparency: Option<f64>) {
        let c = norm_color(color);
        if !self.r2004 {
            self.bs(c.index.unwrap_or(256.0));
            return;
        }
        let has_t = transparency.is_some();
        if let Some(rgb) = c.rgb {
            let idx = to_i32(c.index.unwrap_or(0.0)) & 0xFF;
            self.bs(f64::from(0x8000 | (if has_t { 0x2000 } else { 0 }) | idx));
            self.bl(f64::from(crate::util::i32_from_u32(0xC200_0000 | (rgb & 0xFF_FFFF))));
        } else {
            let idx = to_i32(c.index.unwrap_or(256.0)) & 0x1FF;
            self.bs(f64::from((if has_t { 0x2000 } else { 0 }) | idx));
        }
        if let Some(t) = transparency {
            self.bl(f64::from(to_i32(t)));
        }
    }
    /// OT (2.12).
    pub fn ot(&mut self, t: f64) {
        if !self.r2010 {
            self.bs(t);
            return;
        }
        if t < 256.0 {
            self.bb(0);
            self.rc(t);
        } else if (496.0..752.0).contains(&t) {
            self.bb(1);
            self.rc(t - 496.0);
        } else {
            self.bb(2);
            self.rs(t);
        }
    }
    /// Handle reference in this stream (BitWriter.H).
    pub fn h_data(&mut self, code: u32, value: f64) {
        let bytes = handle_bytes(value);
        self.rc8(crate::util::u8_of(((code & 15) << 4) | (crate::util::u32_of_usize(bytes.len()) & 0xFF)));
        for b in bytes {
            self.rc8(b);
        }
    }
    /// ObjWriter.H: the handle stream (the data stream when `inline` or a plain writer).
    pub fn h(&mut self, code: u32, value: f64) {
        if self.obj && !self.inline {
            if let Some(h) = self.hnd.as_mut() {
                h.h_data(code, value);
            }
        } else {
            self.h_data(code, value);
        }
    }
    /// H with a JS value (handleBytes: a hex string is parsed, else `Math.max(0, Math.trunc(v || 0))`).
    pub fn hv(&mut self, code: u32, value: Option<&V>) {
        let n = handle_value(value);
        self.h(code, n);
    }
    /// ObjWriter.Hd: a handle in the data stream.
    pub fn hd(&mut self, code: u32, value: f64) {
        self.h_data(code, value);
    }
    /// T8: BS length counting a closing NUL, then the code-page bytes.
    pub fn t8(&mut self, s: &str) {
        let b = encode1252(s);
        self.t8_bytes(&b);
    }
    pub fn t8_bytes(&mut self, b: &[u8]) {
        if b.is_empty() {
            self.bs(0.0);
            return;
        }
        self.bs(crate::util::usize_to_f64(b.len()) + 1.0);
        for &c in b {
            self.rc8(c);
        }
        self.rc8(0);
    }
    /// TU: BS character count, then UTF-16LE code units.
    pub fn tu(&mut self, s: &str) {
        let u = clean_units(&crate::w_js::units(s));
        self.tu_clean(&u);
    }
    /// TU of raw UTF-16 units (cleanText applied).
    pub fn tu_units(&mut self, s: &[u16]) {
        let u = clean_units(s);
        self.tu_clean(&u);
    }
    fn tu_clean(&mut self, u: &[u16]) {
        self.bs(crate::util::usize_to_f64(u.len()));
        for &c in u {
            self.rs_i(i32::from(c));
        }
    }
    /// TV: T8 up to R2004, TU from R2007 (an ObjWriter writes it to its string stream).
    pub fn tv(&mut self, s: &str) {
        if self.r2007 {
            if self.obj {
                if let Some(st) = self.strw.as_mut() {
                    st.tu(s);
                }
                self.has_str = true;
            } else {
                self.tu(s);
            }
        } else {
            self.t8(s);
        }
    }
    /// TV of raw UTF-16 units (passthrough strings).
    pub fn tv_units(&mut self, s: &[u16]) {
        if self.r2007 {
            if self.obj {
                if let Some(st) = self.strw.as_mut() {
                    st.tu_units(s);
                }
                self.has_str = true;
            } else {
                self.tu_units(s);
            }
        } else {
            let st = String::from_utf16_lossy(s);
            self.t8(&st);
        }
    }
    /// Copy another writer's first n bits.
    pub fn append_n(&mut self, w: &BitWriter, n: usize) {
        append_bits(self, &w.buf, n);
    }
    pub fn append(&mut self, w: &BitWriter) {
        let n = w.bit;
        append_bits(self, &w.buf, n);
    }
    /// Overwrite 32 bits at a bit position with a little-endian RL.
    pub fn patch_rl(&mut self, at: usize, v: u32) {
        self.grow(0);
        let mut pos = at;
        for i in 0..4u32 {
            let b = v.wrapping_shr(i.wrapping_mul(8)) & 0xFF;
            for k in (0..8u32).rev() {
                let mask = 0x80u8 >> (pos & 7);
                if let Some(x) = self.buf.get_mut(pos >> 3) {
                    if (b >> k) & 1 != 0 {
                        *x |= mask;
                    } else {
                        *x &= !mask;
                    }
                }
                pos = pos.saturating_add(1);
            }
        }
    }
    /// Pad to the byte boundary; the bytes written so far.
    pub fn bytes(&self) -> Vec<u8> {
        let n = self.bit.saturating_add(7) >> 3;
        let mut out = self.buf.get(..n.min(self.buf.len())).map(<[u8]>::to_vec).unwrap_or_default();
        out.resize(n, 0);
        out
    }
    /// ObjWriter.endData: R2007+ appends the string stream, its size and the "present" bit. Returns endbit.
    pub fn end_data(&mut self) -> usize {
        if self.r2007 {
            if self.has_str {
                let st = self.strw.take().unwrap_or_else(|| Box::new(BitWriter::new(self.version)));
                self.append(&st);
                let n = st.bit;
                self.strw = Some(st);
                if n < 0x8000 {
                    self.rs_i(crate::util::i32_of_usize(n));
                } else {
                    self.rs_i(crate::util::i32_of_usize(n >> 15));
                    self.rs_i(crate::util::i32_of_usize((n & 0x7FFF) | 0x8000));
                }
                self.b(true);
            } else {
                self.b(false);
            }
        }
        self.bit
    }
    /// Append the handle stream to the data (finish / finishStreams).
    pub fn append_hnd(&mut self) {
        if let Some(h) = self.hnd.take() {
            self.append(&h);
            self.hnd = Some(h);
        }
    }
}

/// `x || 0` for a value that should be a number (a truthy non-number goes through ToNumber, as RD would).
pub fn or0(v: Option<&V>) -> f64 {
    match v {
        Some(x) if x.truthy() => to_number(Some(x)),
        _ => 0.0,
    }
}
pub fn or0n(v: f64) -> f64 {
    if v == 0.0 || v.is_nan() { 0.0 } else { v }
}

/// Copy n bits of `u8s` (from bit 0) into the writer (passthrough.js appendBits / BitWriter.append).
pub fn append_bits(w: &mut BitWriter, u8s: &[u8], n: usize) {
    let mut i = 0usize;
    while i.saturating_add(8) <= n {
        w.rc8(u8s.get(i >> 3).copied().unwrap_or(0));
        i = i.saturating_add(8);
    }
    while i < n {
        let byte = u8s.get(i >> 3).copied().unwrap_or(0);
        w.b((byte >> (7usize.wrapping_sub(i & 7))) & 1 != 0);
        i = i.saturating_add(1);
    }
}

/// The bit count appendBits takes from a JS value (`i < n` with coercion; bits past the data read as zeros, as the
/// JS's `undefined` bytes do), capped at the data plus 16 Mbit so a hostile count cannot exhaust memory.
pub fn js_bit_count(n: Option<&V>, avail_bits: usize) -> usize {
    let x = to_number(n);
    if x.is_nan() || x <= 0.0 {
        return 0;
    }
    crate::util::to_usize(x.ceil()).min(avail_bits.saturating_add(1 << 24))
}

/// handleBytes for a number: `Math.max(0, Math.trunc(v || 0))`, bytes MSB first.
pub fn handle_bytes(value: f64) -> Vec<u8> {
    let mut v = if value.is_nan() || value == 0.0 { 0.0 } else { value.trunc().max(0.0) };
    let mut bytes = Vec::new();
    let mut guard: u32 = 0;
    while v > 0.0 && guard < 1100 {
        bytes.insert(0, crate::util::u8_of(to_u32(v % 256.0)));
        v = (v / 256.0).floor();
        guard = guard.saturating_add(1);
    }
    bytes
}
/// The number handleBytes takes from a JS value (a string → `parseInt(v, 16) || 0`).
pub fn handle_value(value: Option<&V>) -> f64 {
    match value {
        Some(V::Str(s)) => {
            let n = crate::w_js::parse_int16(s);
            if n.is_nan() { 0.0 } else { n }
        }
        Some(x) if x.truthy() => to_number(Some(x)),
        _ => 0.0,
    }
}

/// A normalised colour (bits-write.js normColor).
#[derive(Clone, Debug, Default)]
pub struct NormColor {
    pub rgb: Option<u32>,
    pub index: Option<f64>,
    pub name: Option<V>,
    pub book: Option<V>,
}
pub fn norm_color(c: Option<&V>) -> NormColor {
    match nn(c) {
        None => NormColor { index: Some(256.0), ..Default::default() },
        Some(V::Num(n)) => NormColor { index: Some(*n), ..Default::default() },
        Some(V::Obj(o)) => {
            let name = o.get("name").cloned();
            let book = o.get("book").cloned();
            if let Some(rgb) = nn(o.get("rgb")) {
                NormColor { rgb: Some(to_u32(to_number(Some(rgb)))), index: nn(o.get("index")).map(|x| to_number(Some(x))), name, book }
            } else {
                let idx = match o.get("index") {
                    Some(V::Num(n)) if n.is_finite() && n.trunc() == *n => *n,
                    _ => 256.0,
                };
                NormColor { index: Some(idx), name, book, rgb: None }
            }
        }
        // a string / bool / array: `c.rgb` and `c.index` are undefined
        Some(_) => NormColor { index: Some(256.0), ..Default::default() },
    }
}
fn cmc_long(c: &NormColor) -> i32 {
    if let Some(rgb) = c.rgb {
        return crate::util::i32_from_u32(0xC200_0000 | (rgb & 0xFF_FFFF));
    }
    let i = c.index.unwrap_or(256.0);
    if i == 256.0 {
        return crate::util::i32_from_u32(0xC000_0000);
    }
    if i == 0.0 {
        return crate::util::i32_from_u32(0xC100_0000);
    }
    if i == 257.0 {
        return crate::util::i32_from_u32(0xC800_0000);
    }
    crate::util::i32_from_u32(0xC300_0000 | (crate::util::u32_from_i32(to_i32(i.abs())) & 0xFF))
}

// ---------------------------------------------------------------- byte stream
#[derive(Clone, Debug, Default)]
pub struct ByteWriter {
    pub buf: Vec<u8>,
}
impl ByteWriter {
    pub fn new(size: usize) -> Self {
        ByteWriter { buf: Vec::with_capacity(size.min(1 << 24)) }
    }
    pub fn pos(&self) -> usize {
        self.buf.len()
    }
    pub fn raw(&mut self, a: &[u8]) {
        self.buf.extend_from_slice(a);
    }
    pub fn zeros(&mut self, n: usize) {
        self.buf.resize(self.buf.len().saturating_add(n), 0);
    }
    pub fn rc8(&mut self, v: u8) {
        self.buf.push(v);
    }
    pub fn rc(&mut self, v: f64) {
        self.buf.push(u8_of_i32(to_i32(v)));
    }
    pub fn rs_i(&mut self, v: i32) {
        self.buf.push(u8_of_i32(v));
        self.buf.push(u8_of_i32(v >> 8));
    }
    pub fn rs(&mut self, v: f64) {
        self.rs_i(to_i32(v));
    }
    pub fn rl_u(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn rl(&mut self, v: f64) {
        self.rl_u(to_u32(v));
    }
    pub fn rll_u(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn rll(&mut self, v: f64) {
        self.rll_u(f64_to_u64_wrapping(v));
    }
    pub fn rd(&mut self, v: f64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn set_rl(&mut self, at: usize, v: u32) {
        for (k, b) in v.to_le_bytes().into_iter().enumerate() {
            if let Some(x) = self.buf.get_mut(at.saturating_add(k)) {
                *x = b;
            }
        }
    }
    /// MC (2.6), signed.
    pub fn mc(&mut self, v: f64) {
        let b = mc_bytes(v);
        self.raw(&b);
    }
    /// Unsigned modular char.
    pub fn umc(&mut self, v: f64) {
        let b = umc_bytes(v);
        self.raw(&b);
    }
    /// MS (2.7).
    pub fn ms(&mut self, v: f64) {
        let mut v = v;
        let mut guard: u32 = 0;
        loop {
            let s = to_i32(v) & 0x7FFF;
            v = (v / 32768.0).floor();
            guard = guard.saturating_add(1);
            if v == 0.0 || guard > 80 || v.is_nan() {
                self.rs_i(s);
                break;
            }
            self.rs_i(s | 0x8000);
        }
    }
    pub fn bytes(&self) -> Vec<u8> {
        self.buf.clone()
    }
}
pub fn mc_bytes(v: f64) -> Vec<u8> {
    let neg = v < 0.0;
    let mut a = v.abs();
    let mut out = Vec::new();
    let mut guard: u32 = 0;
    loop {
        let b = to_i32(a) & 0x7F;
        a = (a / 128.0).floor();
        guard = guard.saturating_add(1);
        if (a == 0.0 || a.is_nan() || guard > 160) && b & 0x40 == 0 {
            out.push(u8_of_i32(b | if neg { 0x40 } else { 0 }));
            break;
        }
        out.push(u8_of_i32(b | 0x80));
        if a == 0.0 || a.is_nan() || guard > 160 {
            out.push(if neg { 0x40 } else { 0 });
            break;
        }
    }
    out
}
pub fn umc_bytes(v: f64) -> Vec<u8> {
    let mut v = v;
    let mut out = Vec::new();
    let mut guard: u32 = 0;
    loop {
        let mut b = to_i32(v) & 0x7F;
        v = (v / 128.0).floor();
        guard = guard.saturating_add(1);
        let more = v != 0.0 && !v.is_nan() && guard < 160;
        if more {
            b |= 0x80;
        }
        out.push(u8_of_i32(b));
        if !more {
            break;
        }
    }
    out
}

// ---------------------------------------------------------------- text
/// cleanText on UTF-16 units: NUL goes, other control characters (but TAB) become spaces.
pub fn clean_units(s: &[u16]) -> Vec<u16> {
    s.iter().filter(|&&c| c != 0).map(|&c| if (1..=8).contains(&c) || (0x0A..=0x1F).contains(&c) || c == 0x7F { 0x20 } else { c }).collect()
}
/// cleanText on a string.
pub fn clean_text(s: &str) -> String {
    s.chars()
        .filter(|&c| c != '\0')
        .map(|c| if ('\u{1}'..='\u{8}').contains(&c) || ('\u{a}'..='\u{1f}').contains(&c) || c == '\u{7f}' { ' ' } else { c })
        .collect()
}
/// encode1252: ASCII and U+00A0–U+00FF as single bytes, others as \U+XXXX, astral → '?' (counted as lost).
pub fn encode1252(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for ch in clean_text(s).chars() {
        let c = u32::from(ch);
        if c < 0x80 || (0xA0..=0xFF).contains(&c) {
            out.push(crate::util::u8_of(c));
        } else if c > 0xFFFF {
            out.push(0x3F);
            LOST.with(|l| l.set(l.get().saturating_add(1)));
        } else {
            out.extend_from_slice(format!("\\U+{c:04X}").as_bytes());
        }
    }
    out
}
/// encode1252 of UTF-16 units (lone surrogates become \U+D8xx as `codePointAt` gives them).
pub fn encode1252_units(s: &[u16]) -> Vec<u8> {
    let st = String::from_utf16_lossy(s);
    encode1252(&st)
}
/// The UTF-16 length of a string.
pub fn u16_len(s: &str) -> usize {
    s.encode_utf16().count()
}
/// usize from a small u32.
pub fn us(n: u32) -> usize {
    usize_from_u32(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codes() {
        let mut w = BitWriter::new("AC1015");
        w.bs(0.0);
        w.bs(256.0);
        w.bs(5.0);
        w.bl(-1.0);
        assert_eq!(w.bit, 2 + 2 + 10 + 34);
        assert_eq!(mc_bytes(-5.0), vec![0x45]);
        assert_eq!(mc_bytes(200.0), vec![0xC8, 0x01]);
        assert_eq!(mc_bytes(64.0), vec![0xC0, 0x00]);
        assert_eq!(umc_bytes(300.0), vec![0xAC, 0x02]);
        assert_eq!(handle_bytes(0x1234 as f64), vec![0x12, 0x34]);
        assert_eq!(encode1252("a\u{2013}"), b"a\\U+2013".to_vec());
        let mut w = BitWriter::new("AC1015");
        w.rl(0.0);
        w.patch_rl(0, 0x0102_0304);
        assert_eq!(w.bytes(), vec![4, 3, 2, 1]);
    }
}
