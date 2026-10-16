//! `parity skp` / `bench skp` (fork-parity contract) for cc-io-skp: the `readSkp` model as JSON, bytes as
//! `{"$bytes": n, "sha256": "<hex>"}` (the kit's canonical form), non-finite numbers as "$NaN" / "$Inf" / "$-Inf".

use std::fmt::Write as _;
use std::io::Write as _;
use std::process::ExitCode;
use std::time::Instant;

use cc_io_skp::{Def, Face, Material, Skp, SkpError, read_skp};

pub fn parity(file: &str) -> ExitCode {
    let bytes = match std::fs::read(file) {
        Ok(b) => b,
        Err(e) => {
            print_error("io", &e.to_string());
            return ExitCode::from(1);
        }
    };
    match read_skp(&bytes) {
        Ok(skp) => {
            // a closed pipe (`| head`) is not an error worth a panic
            let mut out = std::io::stdout().lock();
            let _ = writeln!(out, "{}", to_json(&skp));
            ExitCode::SUCCESS
        }
        Err(e) => refused(&e),
    }
}

fn refused(e: &SkpError) -> ExitCode {
    print_error(e.code, &e.message);
    ExitCode::from(1)
}

fn print_error(code: &str, message: &str) {
    let mut s = String::new();
    s.push_str(r#"{"$error":{"code":"#);
    json_str(&mut s, code);
    s.push_str(r#","message":"#);
    json_str(&mut s, message);
    s.push_str("}}");
    println!("{s}");
}

pub fn bench(file: &str, runs: usize) -> ExitCode {
    let first = match std::fs::read(file) {
        Ok(b) => b,
        Err(e) => {
            print_error("io", &e.to_string());
            return ExitCode::from(1);
        }
    };
    if let Err(e) = read_skp(&first) {
        return refused(&e); // warm-up (the JS kit refuses the same way)
    }
    drop(first);
    let mut open = Vec::new();
    let mut parse = Vec::new();
    for _ in 0..runs.max(1) {
        let t = Instant::now();
        let bytes = match std::fs::read(file) {
            Ok(b) => b,
            Err(e) => {
                print_error("io", &e.to_string());
                return ExitCode::from(1);
            }
        };
        open.push(t.elapsed().as_secs_f64() * 1000.0);
        let t = Instant::now();
        let m = read_skp(&bytes);
        parse.push(t.elapsed().as_secs_f64() * 1000.0);
        drop(m);
    }
    let arr = |v: &[f64]| {
        let mut s = String::from("[");
        for (i, x) in v.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            let _ = write!(s, "{}", (x * 1000.0).round() / 1000.0);
        }
        s.push(']');
        s
    };
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    println!(
        r#"{{"open_ms":{},"parse_ms":{},"first_frame_ms":null,"runtime":"rust {} native, rayon {} threads"}}"#,
        arr(&open),
        arr(&parse),
        env!("CARGO_PKG_VERSION"),
        threads
    );
    ExitCode::SUCCESS
}

// ---------------------------------------------------------------- JSON

fn json_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn num(out: &mut String, v: f64) {
    if v.is_nan() {
        out.push_str("\"$NaN\"");
    } else if v.is_infinite() {
        out.push_str(if v > 0.0 { "\"$Inf\"" } else { "\"$-Inf\"" });
    } else if v == 0.0 {
        out.push('0');
    } else {
        // shortest round-trip digits; JSON.parse gives back the same double
        let _ = write!(out, "{v:?}");
    }
}

fn opt_usize(out: &mut String, v: Option<usize>) {
    match v {
        Some(n) => {
            let _ = write!(out, "{n}");
        }
        None => out.push_str("null"),
    }
}

fn bytes(out: &mut String, b: &[u8]) {
    let _ = write!(out, r#"{{"$bytes":{},"sha256":"{}"}}"#, b.len(), sha256_hex(b));
}

fn p3(out: &mut String, p: &[f64; 3]) {
    out.push('[');
    num(out, p[0]);
    out.push(',');
    num(out, p[1]);
    out.push(',');
    num(out, p[2]);
    out.push(']');
}

fn list<T>(out: &mut String, items: &[T], mut f: impl FnMut(&mut String, &T)) {
    out.push('[');
    for (i, x) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        f(out, x);
    }
    out.push(']');
}

fn color(out: &mut String, c: Option<[u8; 3]>) {
    match c {
        Some([r, g, b]) => {
            let _ = write!(out, "[{r},{g},{b}]");
        }
        None => out.push_str("null"),
    }
}

fn material(out: &mut String, m: &Material) {
    out.push('{');
    if let Some(n) = &m.name {
        out.push_str("\"name\":");
        json_str(out, n);
        out.push(',');
    }
    out.push_str("\"color\":");
    color(out, m.color);
    out.push_str(",\"alpha\":");
    num(out, m.alpha);
    out.push_str(",\"texture\":");
    match &m.texture {
        Some(t) => {
            out.push_str("{\"name\":");
            json_str(out, &t.name);
            out.push_str(",\"mime\":");
            json_str(out, t.mime);
            out.push_str(",\"bytes\":");
            bytes(out, &t.bytes);
            out.push_str(",\"width\":");
            num(out, t.width);
            out.push_str(",\"height\":");
            num(out, t.height);
            out.push('}');
        }
        None => out.push_str("null"),
    }
    out.push('}');
}

fn face(out: &mut String, f: &Face) {
    out.push_str("{\"loops\":");
    list(out, &f.loops, |o, l| list(o, l, p3));
    out.push_str(",\"front\":");
    opt_usize(out, f.front);
    out.push_str(",\"back\":");
    opt_usize(out, f.back);
    out.push_str(",\"layer\":");
    opt_usize(out, f.layer);
    out.push_str(",\"uv\":");
    match &f.uv {
        Some(uv) => list(out, uv, |o, l| {
            list(o, l, |o, p| {
                o.push('[');
                num(o, p[0]);
                o.push(',');
                num(o, p[1]);
                o.push(']');
            })
        }),
        None => out.push_str("null"),
    }
    if let Some(h) = f.hidden {
        out.push_str(if h { ",\"hidden\":true" } else { ",\"hidden\":false" });
    }
    out.push('}');
}

fn def(out: &mut String, d: &Def) {
    out.push_str("{\"name\":");
    json_str(out, &d.name);
    out.push_str(",\"kind\":");
    json_str(out, d.kind);
    out.push_str(",\"guid\":");
    match &d.guid {
        Some(g) => json_str(out, g),
        None => out.push_str("null"),
    }
    out.push_str(",\"faces\":");
    list(out, &d.faces, face);
    out.push_str(",\"edges\":");
    list(out, &d.edges, |o, e| {
        o.push_str("{\"a\":");
        p3(o, &e.a);
        o.push_str(",\"b\":");
        p3(o, &e.b);
        o.push_str(",\"layer\":");
        opt_usize(o, e.layer);
        let _ = write!(o, r#","soft":{},"smooth":{},"hidden":{}}}"#, e.soft, e.smooth, e.hidden);
    });
    out.push_str(",\"soft\":");
    list(out, &d.soft, |o, s| {
        o.push('[');
        p3(o, &s[0]);
        o.push(',');
        p3(o, &s[1]);
        o.push(']');
    });
    out.push_str(",\"insts\":");
    list(out, &d.insts, |o, i| {
        let _ = write!(o, "{{\"def\":{},\"m\":", i.def);
        list(o, &i.m, |o, v| num(o, *v));
        o.push_str(",\"name\":");
        json_str(o, &i.name);
        o.push_str(",\"layer\":");
        opt_usize(o, i.layer);
        let _ = write!(o, r#","hidden":{},"material":"#, i.hidden);
        opt_usize(o, i.material);
        o.push('}');
    });
    out.push('}');
}

/// The model as the JS `readSkp` result, in the kit's canonical shapes.
pub fn to_json(skp: &Skp) -> String {
    let mut o = String::with_capacity(1 << 16);
    o.push_str("{\"format\":\"skp\",\"container\":");
    json_str(&mut o, skp.container.as_str());
    o.push_str(",\"version\":");
    json_str(&mut o, &skp.version);
    o.push_str(",\"major\":");
    num(&mut o, skp.major);
    o.push_str(",\"materials\":");
    list(&mut o, &skp.materials, material);
    o.push_str(",\"layers\":");
    list(&mut o, &skp.layers, |o, l| {
        o.push('{');
        if let Some(n) = &l.name {
            o.push_str("\"name\":");
            json_str(o, n);
            o.push(',');
        }
        if let Some(v) = l.visible {
            let _ = write!(o, "\"visible\":{v},");
        }
        o.push_str("\"color\":");
        color(o, l.color);
        o.push('}');
    });
    o.push_str(",\"defs\":");
    list(&mut o, &skp.defs, def);
    let _ = write!(o, ",\"root\":{},\"thumbnail\":", skp.root);
    match &skp.thumbnail {
        Some(t) => {
            o.push_str("{\"mime\":");
            json_str(&mut o, t.mime);
            o.push_str(",\"bytes\":");
            bytes(&mut o, &t.bytes);
            o.push('}');
        }
        None => o.push_str("null"),
    }
    o.push_str(",\"units\":{\"length\":");
    match skp.units {
        Some(u) => json_str(&mut o, u),
        None => o.push_str("null"),
    }
    o.push_str("},\"notes\":");
    list(&mut o, &skp.notes, |o, n| json_str(o, n));
    let c = &skp.counts;
    let _ = write!(
        o,
        r#","counts":{{"defs":{},"faces":{},"edges":{},"insts":{},"materials":{},"layers":{}}}}}"#,
        c.defs, c.faces, c.edges, c.insts, c.materials, c.layers
    );
    o
}

// ---------------------------------------------------------------- SHA-256 (FIPS 180-4), for the canonical bytes form

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be,
    0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa,
    0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85,
    0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
    0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f,
    0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

// Indexes are loop counters over fixed 64-entry arrays (0..64), never input-derived.
#[allow(clippy::indexing_slicing)]
fn compress(h: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0u32; 64];
    for (x, c) in w.iter_mut().zip(block.as_chunks::<4>().0) {
        *x = u32::from_be_bytes(*c);
    }
    for i in 16..64 {
        let (w15, w2, w16, w7) = (w[i - 15], w[i - 2], w[i - 16], w[i - 7]);
        let s0 = w15.rotate_right(7) ^ w15.rotate_right(18) ^ (w15 >> 3);
        let s1 = w2.rotate_right(17) ^ w2.rotate_right(19) ^ (w2 >> 10);
        w[i] = w16.wrapping_add(s0).wrapping_add(w7).wrapping_add(s1);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = *h;
    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ (!e & g);
        let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);
        hh = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
        *x = x.wrapping_add(y);
    }
}

pub fn sha256_hex(data: &[u8]) -> String {
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let (blocks, rest) = data.as_chunks::<64>();
    for block in blocks {
        compress(&mut h, block);
    }
    let mut tail = Vec::with_capacity(128);
    tail.extend_from_slice(rest);
    tail.push(0x80);
    while tail.len() % 64 != 56 {
        tail.push(0);
    }
    tail.extend_from_slice(&((data.len() as u64).wrapping_mul(8)).to_be_bytes());
    for block in tail.as_chunks::<64>().0 {
        compress(&mut h, block);
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_vectors() {
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        let a = vec![b'a'; 1000];
        assert_eq!(sha256_hex(&a), "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3");
    }
}
