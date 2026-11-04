//! A QR code drawn in code (ISO/IEC 18004, model 2): Studio Live's join QR holds the joiner's link
//! (`http://<host>:<port>/apps/<slug>/#live=<code>`, LIVE-PROTOCOL § 12). Byte mode, error correction level M, the
//! smallest version that fits (1–40), the mask with the lowest penalty. Checked by reading the codes back with an
//! independent decoder (forks/tools/live-studiofin/qrcheck.mjs, jsQR) and by the module tests below.

/// A finished symbol: `size` × `size` modules, true = dark.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Qr {
    pub size: usize,
    pub version: usize,
    pub modules: Vec<bool>,
}

impl Qr {
    pub fn get(&self, x: usize, y: usize) -> bool {
        x < self.size && y < self.size && self.modules.get(y.saturating_mul(self.size).saturating_add(x)).copied().unwrap_or(false)
    }
}

// level M (format bits 00)
const ECC_PER_BLOCK: [usize; 41] = [
    0, 10, 16, 26, 18, 24, 16, 18, 22, 22, 26, 30, 22, 22, 24, 24, 28, 28, 26, 26, 26, 26, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28,
    28, 28, 28, 28, 28, 28, 28,
];
const BLOCKS: [usize; 41] = [
    0, 1, 1, 1, 2, 2, 4, 4, 4, 5, 5, 5, 8, 9, 9, 10, 10, 11, 13, 14, 16, 17, 17, 18, 20, 21, 23, 25, 26, 28, 29, 31, 33, 35, 37, 38, 40,
    43, 45, 47, 49,
];
const FORMAT_LEVEL_M: u32 = 0;

fn raw_modules(ver: usize) -> usize {
    let mut r = (16 * ver + 128) * ver + 64;
    if ver >= 2 {
        let n = ver / 7 + 2;
        r -= (25 * n - 10) * n - 55;
        if ver >= 7 {
            r -= 36;
        }
    }
    r
}
fn data_codewords(ver: usize) -> usize {
    raw_modules(ver) / 8 - ECC_PER_BLOCK.get(ver).copied().unwrap_or(0) * BLOCKS.get(ver).copied().unwrap_or(0)
}

// GF(256) with 0x11d
fn gf_mul(x: u8, y: u8) -> u8 {
    let mut z: u16 = 0;
    for i in (0..8).rev() {
        z = (z << 1) ^ ((z >> 7) * 0x11d);
        z ^= u16::from((y >> i) & 1) * u16::from(x);
    }
    u8::try_from(z & 0xff).unwrap_or(0)
}
fn rs_divisor(degree: usize) -> Vec<u8> {
    let mut r = vec![0u8; degree];
    if let Some(l) = r.last_mut() {
        *l = 1;
    }
    let mut root: u8 = 1;
    for _ in 0..degree {
        for j in 0..degree {
            let next = r.get(j + 1).copied().unwrap_or(0);
            if let Some(v) = r.get_mut(j) {
                *v = gf_mul(*v, root) ^ next;
            }
        }
        root = gf_mul(root, 0x02);
    }
    r
}
fn rs_remainder(data: &[u8], div: &[u8]) -> Vec<u8> {
    let mut r = vec![0u8; div.len()];
    for &b in data {
        let factor = b ^ r.first().copied().unwrap_or(0);
        r.remove(0);
        r.push(0);
        for (x, &d) in r.iter_mut().zip(div) {
            *x ^= gf_mul(d, factor);
        }
    }
    r
}

struct Grid {
    size: usize,
    m: Vec<bool>,
    func: Vec<bool>,
}

impl Grid {
    fn set(&mut self, x: usize, y: usize, dark: bool) {
        let i = y * self.size + x;
        if let Some(v) = self.m.get_mut(i) {
            *v = dark;
        }
        if let Some(f) = self.func.get_mut(i) {
            *f = true;
        }
    }
    fn get(&self, x: usize, y: usize) -> bool {
        self.m.get(y * self.size + x).copied().unwrap_or(false)
    }
    fn is_func(&self, x: usize, y: usize) -> bool {
        self.func.get(y * self.size + x).copied().unwrap_or(true)
    }
}

fn alignment_positions(ver: usize) -> Vec<usize> {
    if ver == 1 {
        return vec![];
    }
    let n = ver / 7 + 2;
    let step = if ver == 32 { 26 } else { (ver * 4 + n * 2 + 1) / (n * 2 - 2) * 2 };
    let size = ver * 4 + 17;
    let mut out = vec![6];
    let mut pos = size - 7;
    let mut rest = vec![];
    for _ in 0..n - 1 {
        rest.push(pos);
        pos = pos.saturating_sub(step);
    }
    rest.reverse();
    out.extend(rest);
    out
}

fn draw_function(g: &mut Grid, ver: usize) {
    let size = g.size;
    for i in 0..size {
        g.set(6, i, i % 2 == 0);
        g.set(i, 6, i % 2 == 0);
    }
    for (cx, cy) in [(3usize, 3usize), (size - 4, 3), (3, size - 4)] {
        for dy in -4i32..=4 {
            for dx in -4i32..=4 {
                let d = dx.abs().max(dy.abs());
                let (x, y) = (cx as i32 + dx, cy as i32 + dy);
                if x >= 0 && y >= 0 && (x as usize) < size && (y as usize) < size {
                    g.set(x as usize, y as usize, d != 2 && d != 4);
                }
            }
        }
    }
    let al = alignment_positions(ver);
    let n = al.len();
    for (i, &ax) in al.iter().enumerate() {
        for (j, &ay) in al.iter().enumerate() {
            if (i == 0 && j == 0) || (i == 0 && j == n - 1) || (i == n - 1 && j == 0) {
                continue;
            }
            for dy in -2i32..=2 {
                for dx in -2i32..=2 {
                    g.set((ax as i32 + dx) as usize, (ay as i32 + dy) as usize, dx.abs().max(dy.abs()) != 1);
                }
            }
        }
    }
    draw_format(g, 0);
    if ver >= 7 {
        let mut rem = ver as u32;
        for _ in 0..12 {
            rem = (rem << 1) ^ ((rem >> 11) * 0x1f25);
        }
        let bits = (ver as u32) << 12 | rem;
        for i in 0..18 {
            let bit = (bits >> i) & 1 != 0;
            let (a, b) = (size - 11 + i % 3, i / 3);
            g.set(a, b, bit);
            g.set(b, a, bit);
        }
    }
}

fn draw_format(g: &mut Grid, mask: u32) {
    let data = FORMAT_LEVEL_M << 3 | mask;
    let mut rem = data;
    for _ in 0..10 {
        rem = (rem << 1) ^ ((rem >> 9) * 0x537);
    }
    let bits = (data << 10 | rem) ^ 0x5412;
    let size = g.size;
    let bit = |i: u32| (bits >> i) & 1 != 0;
    for i in 0..=5 {
        g.set(8, i, bit(i as u32));
    }
    g.set(8, 7, bit(6));
    g.set(8, 8, bit(7));
    g.set(7, 8, bit(8));
    for i in 9..15 {
        g.set(14 - i, 8, bit(i as u32));
    }
    for i in 0..8 {
        g.set(size - 1 - i, 8, bit(i as u32));
    }
    for i in 8..15 {
        g.set(8, size - 15 + i, bit(i as u32));
    }
    g.set(8, size - 8, true);
}

fn draw_codewords(g: &mut Grid, data: &[u8]) {
    let size = g.size;
    let mut i = 0usize;
    let mut right = size as i32 - 1;
    while right >= 1 {
        if right == 6 {
            right = 5;
        }
        for vert in 0..size {
            for j in 0..2 {
                let x = (right - j) as usize;
                let upward = ((right + 1) & 2) == 0;
                let y = if upward { size - 1 - vert } else { vert };
                if !g.is_func(x, y) && i < data.len() * 8 {
                    let b = data.get(i >> 3).is_some_and(|v| (v >> (7 - (i & 7))) & 1 != 0);
                    let idx = y * size + x;
                    if let Some(m) = g.m.get_mut(idx) {
                        *m = b;
                    }
                    i += 1;
                }
            }
        }
        right -= 2;
    }
}

fn apply_mask(g: &mut Grid, mask: u32) {
    let size = g.size;
    for y in 0..size {
        for x in 0..size {
            let inv = match mask {
                0 => (x + y) % 2 == 0,
                1 => y % 2 == 0,
                2 => x % 3 == 0,
                3 => (x + y) % 3 == 0,
                4 => (x / 3 + y / 2) % 2 == 0,
                5 => x * y % 2 + x * y % 3 == 0,
                6 => (x * y % 2 + x * y % 3) % 2 == 0,
                _ => ((x + y) % 2 + x * y % 3) % 2 == 0,
            };
            if !g.is_func(x, y) && inv {
                let i = y * size + x;
                if let Some(m) = g.m.get_mut(i) {
                    *m = !*m;
                }
            }
        }
    }
}

fn penalty(g: &Grid) -> u64 {
    let size = g.size;
    let mut score: u64 = 0;
    // runs of five or more, rows and columns
    for horiz in [true, false] {
        for a in 0..size {
            let mut run = 0usize;
            let mut prev = false;
            let mut line: Vec<bool> = Vec::with_capacity(size);
            for b in 0..size {
                let v = if horiz { g.get(b, a) } else { g.get(a, b) };
                line.push(v);
                if b > 0 && v == prev {
                    run += 1;
                } else {
                    if run >= 5 {
                        score += run as u64 - 2;
                    }
                    run = 1;
                }
                prev = v;
            }
            if run >= 5 {
                score += run as u64 - 2;
            }
            // finder-like patterns 1:1:3:1:1 with four light on one side
            let pat = [true, false, true, true, true, false, true];
            for s in 0..size.saturating_sub(6) {
                if line.get(s..s + 7).is_some_and(|w| w == pat) {
                    let before = (s >= 4 && line.get(s - 4..s).is_some_and(|w| w.iter().all(|x| !x)))
                        || s < 4 && line.get(..s).is_some_and(|w| w.iter().all(|x| !x));
                    let after = line.get(s + 7..(s + 11).min(size)).is_some_and(|w| w.iter().all(|x| !x));
                    if before || after {
                        score += 40;
                    }
                }
            }
        }
    }
    for y in 0..size - 1 {
        for x in 0..size - 1 {
            let c = g.get(x, y);
            if c == g.get(x + 1, y) && c == g.get(x, y + 1) && c == g.get(x + 1, y + 1) {
                score += 3;
            }
        }
    }
    let dark = g.m.iter().filter(|x| **x).count() as u64;
    let total = (size * size) as u64;
    let k = ((dark * 20).abs_diff(total * 10)).div_ceil(total).saturating_sub(1);
    score + k * 10
}

/// The QR code of `data` (byte mode, level M), or None when it is too long for version 40.
pub fn encode(data: &[u8]) -> Option<Qr> {
    let mut ver = 0;
    for v in 1..=40usize {
        let count_bits = if v <= 9 { 8 } else { 16 };
        let need = 4 + count_bits + data.len() * 8;
        if need <= data_codewords(v) * 8 {
            ver = v;
            break;
        }
    }
    if ver == 0 {
        return None;
    }
    let count_bits = if ver <= 9 { 8 } else { 16 };
    let cap = data_codewords(ver) * 8;
    let mut bits: Vec<bool> = Vec::with_capacity(cap);
    let push = |v: u32, n: usize, bits: &mut Vec<bool>| {
        for i in (0..n).rev() {
            bits.push((v >> i) & 1 != 0);
        }
    };
    push(0b0100, 4, &mut bits);
    push(u32::try_from(data.len()).ok()?, count_bits, &mut bits);
    for &b in data {
        push(u32::from(b), 8, &mut bits);
    }
    let term = (cap - bits.len()).min(4);
    push(0, term, &mut bits);
    let pad = (8 - bits.len() % 8) % 8;
    push(0, pad, &mut bits);
    let mut words: Vec<u8> = bits.chunks(8).map(|c| c.iter().fold(0u8, |a, &b| a << 1 | u8::from(b))).collect();
    let mut alt = 0xecu8;
    while words.len() < data_codewords(ver) {
        words.push(alt);
        alt ^= 0xec ^ 0x11;
    }
    // blocks and their error correction, interleaved
    let nblocks = BLOCKS.get(ver).copied()?;
    let ecc = ECC_PER_BLOCK.get(ver).copied()?;
    let raw = raw_modules(ver) / 8;
    let short = nblocks - raw % nblocks;
    let short_len = raw / nblocks;
    let div = rs_divisor(ecc);
    let mut blocks: Vec<Vec<u8>> = vec![];
    let mut k = 0usize;
    for i in 0..nblocks {
        let dlen = short_len - ecc + usize::from(i >= short);
        let d = words.get(k..k + dlen)?.to_vec();
        k += dlen;
        let e = rs_remainder(&d, &div);
        let mut b = d;
        if i < short {
            b.push(0);
        }
        b.extend(e);
        blocks.push(b);
    }
    let mut out = vec![];
    for i in 0..=short_len {
        for (j, b) in blocks.iter().enumerate() {
            if (i != short_len - ecc || j >= short)
                && let Some(&x) = b.get(i)
            {
                out.push(x);
            }
        }
    }
    let size = ver * 4 + 17;
    let mut g = Grid { size, m: vec![false; size * size], func: vec![false; size * size] };
    draw_function(&mut g, ver);
    draw_codewords(&mut g, &out);
    let mut best: Option<(u64, u32)> = None;
    for mask in 0..8 {
        let mut t = Grid { size, m: g.m.clone(), func: g.func.clone() };
        apply_mask(&mut t, mask);
        draw_format(&mut t, mask);
        let p = penalty(&t);
        if best.is_none_or(|(bp, _)| p < bp) {
            best = Some((p, mask));
        }
    }
    let mask = best.map_or(0, |b| b.1);
    apply_mask(&mut g, mask);
    draw_format(&mut g, mask);
    Some(Qr { size, version: ver, modules: g.m })
}

/// Draws the code `side` points wide (with its four-module quiet zone) in the theme's paper colours: dark modules on
/// white, as scanners expect, in light and dark themes alike.
pub fn paint(ui: &mut egui::Ui, q: &Qr, side: f32, _tk: &crate::theme::Tokens) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, egui::Color32::WHITE);
    let n = q.size as f32 + 8.0;
    let m = side / n;
    for y in 0..q.size {
        for x in 0..q.size {
            if q.get(x, y) {
                let r = egui::Rect::from_min_size(
                    rect.min + egui::vec2((x as f32 + 4.0) * m, (y as f32 + 4.0) * m),
                    egui::vec2(m + 0.3, m + 0.3),
                );
                p.rect_filled(r, 0.0, egui::Color32::BLACK);
            }
        }
    }
    resp
}

/// The code as a PBM-like text grid ('#' dark, '.' light), for checks outside the app.
pub fn to_text(q: &Qr) -> String {
    let mut s = String::with_capacity(q.size * (q.size + 1));
    for y in 0..q.size {
        for x in 0..q.size {
            s.push(if q.get(x, y) { '#' } else { '.' });
        }
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_and_structure() {
        // level M byte capacity: version 1 holds 14 bytes, version 2 holds 26, version 10 holds 213
        assert_eq!(encode(&[b'a'; 14]).unwrap().version, 1);
        assert_eq!(encode(&[b'a'; 15]).unwrap().version, 2);
        assert_eq!(encode(&[b'a'; 26]).unwrap().version, 2);
        assert_eq!(encode(&[b'a'; 213]).unwrap().version, 10);
        assert_eq!(encode(&[b'a'; 214]).unwrap().version, 11);
        assert!(encode(&vec![b'a'; 2400]).is_none());
        let q = encode(b"http://192.168.1.20:47620/apps/cadcraft/#live=K7-Q4M2X").unwrap();
        assert_eq!(q.size, q.version * 4 + 17);
        // the three finders: dark corners, light separators
        for (x, y) in [(0, 0), (q.size - 1, 0), (0, q.size - 1)] {
            assert!(q.get(x, y));
        }
        assert!(!q.get(7, 7) && q.get(8, q.size - 8));
        // timing patterns alternate
        assert!((8..q.size - 8).all(|i| q.get(i, 6) == (i % 2 == 0)));
        assert_eq!(alignment_positions(7), vec![6, 22, 38]);
        assert_eq!(alignment_positions(32), vec![6, 34, 60, 86, 112, 138]);
    }

    /// With CC_QR_OUT set, writes codes of many lengths as text grids for an outside decoder to read back.
    #[test]
    fn grids_for_an_outside_decoder() {
        let Ok(dir) = std::env::var("CC_QR_OUT") else { return };
        let mut list = vec![];
        for n in [1usize, 5, 14, 15, 30, 42, 55, 62, 80, 106, 134, 154, 180, 213, 250, 321, 500, 800, 1200, 2000] {
            let text: String = (0..n).map(|i| char::from(b"http://192.168.1.20:47620/apps/cadcraft/#live=K7-Q4M2X"[i % 54])).collect();
            let q = encode(text.as_bytes()).unwrap();
            std::fs::write(format!("{dir}/q{n}.txt"), to_text(&q)).unwrap();
            list.push(format!("q{n}.txt\t{text}"));
        }
        std::fs::write(format!("{dir}/list.tsv"), list.join("\n")).unwrap();
    }

    #[test]
    fn reed_solomon_matches_the_standard_example() {
        // ISO/IEC 18004 Annex I: "01234567" at 1-M, data codewords → error correction codewords
        let data = [0x10, 0x20, 0x0c, 0x56, 0x61, 0x80, 0xec, 0x11, 0xec, 0x11, 0xec, 0x11, 0xec, 0x11, 0xec, 0x11];
        let ecc = rs_remainder(&data, &rs_divisor(10));
        assert_eq!(ecc, vec![0xa5, 0x24, 0xd4, 0xc1, 0xed, 0x36, 0xc7, 0x87, 0x2c, 0x55]);
    }
}
