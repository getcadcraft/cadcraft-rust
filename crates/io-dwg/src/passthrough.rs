//! Raw passthrough data of a decoded record (port of lib/dwg/passthrough.js captureRaw, read side):
//! `{ ver, n (bits), data (base64), h: [[code, 'HEX'], …], gfx?, ds? }`.

use crate::bits::{BitReader, Codepage};
use crate::obj;
use crate::objects::Record;
use crate::util::{b64, hex_of, slice_bits};
use crate::value::{Obj, V};

pub fn capture_raw(rec: &Record, ver: u32, buf: &[u8]) -> Option<Obj> {
    if buf.is_empty() || rec.endbit < rec.data_start {
        return None;
    }
    let n = rec.endbit.saturating_sub(rec.data_start); // endbit >= data_start checked above
    let mut hs: Vec<V> = Vec::new();
    let mut hr = BitReader::range(buf, ver, Codepage::latin1(), rec.h_pos, rec.obj_end);
    while hr.bits_left() >= 8 && hs.len() < 1_000_000 {
        match hr.h(rec.handle) {
            Ok(v) => hs.push(V::Arr(vec![V::from(hr.hcode), V::Str(hex_of(v.max(0.0)))])),
            Err(_) => break,
        }
    }
    let mut raw = obj! { "ver" => ver, "n" => n, "data" => b64(&slice_bits(buf, rec.data_start, rec.endbit)), "h" => hs };
    if let Some(g) = rec.gfx_pos
        && rec.graphics > 0.0
    {
        let end = crate::util::to_usize(crate::util::usize_to_f64(g) + rec.graphics * 8.0);
        raw.set("gfx", b64(&slice_bits(buf, g, end)));
    }
    if rec.ds.is_some_and(|d| d != 0) {
        raw.set("ds", 1);
    }
    Some(raw)
}
