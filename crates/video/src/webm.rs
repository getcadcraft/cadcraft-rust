//! A WebM file (Matroska, EBML) holding one VP8 video track: the EBML header, Segment › Info (milliseconds, the
//! duration, the writing app), Tracks (V_VP8, its size), Clusters of SimpleBlocks (a cluster from each key frame, its
//! SimpleBlock flagged as one) and Cues pointing at each cluster, so a player can seek.

/// An EBML element: its id bytes, then its size as a variable-length integer, then the body.
fn el(id: u32, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 12);
    let idb = id.to_be_bytes();
    let skip = idb.iter().take_while(|b| **b == 0).count().min(3);
    out.extend(idb.iter().skip(skip));
    out.extend(size_vint(body.len() as u64));
    out.extend_from_slice(body);
    out
}

/// A size as the shortest EBML variable-length integer that holds it (all ones is reserved for "unknown").
fn size_vint(n: u64) -> Vec<u8> {
    for len in 1u32..=8 {
        let max = (1u64 << (7 * len)) - 1;
        if n < max {
            let v = n | (1u64 << (7 * len));
            return v.to_be_bytes().iter().skip(8 - len as usize).copied().collect();
        }
    }
    vec![0x01, 0, 0, 0, 0, 0, 0, 0]
}

fn uint(id: u32, v: u64) -> Vec<u8> {
    let b = v.to_be_bytes();
    let skip = b.iter().take_while(|x| **x == 0).count().min(7);
    el(id, b.get(skip..).unwrap_or(&[]))
}

fn float(id: u32, v: f64) -> Vec<u8> {
    el(id, &v.to_be_bytes())
}

fn text(id: u32, s: &str) -> Vec<u8> {
    el(id, s.as_bytes())
}

/// Frames (VP8 data, whether a key frame; the first one is) at `fps` frames a second, `w` × `h` pixels, into a WebM
/// file.
pub fn mux(frames: &[(Vec<u8>, bool)], w: u32, h: u32, fps: f64, app: &str) -> Vec<u8> {
    let fps = if fps.is_finite() && fps > 0.0 { fps } else { 30.0 };
    let ms = |i: usize| ((i as f64) * 1000.0 / fps).round() as u64;
    let header = el(
        0x1A45_DFA3,
        &[uint(0x4286, 1), uint(0x42F7, 1), uint(0x42F2, 4), uint(0x42F3, 8), text(0x4282, "webm"), uint(0x4287, 2), uint(0x4285, 2)]
            .concat(),
    );
    let info = el(
        0x1549_A966,
        &[uint(0x2AD7B1, 1_000_000), float(0x4489, ms(frames.len()) as f64), text(0x4D80, app), text(0x5741, app)].concat(),
    );
    let video = el(0xE0, &[uint(0xB0, u64::from(w)), uint(0xBA, u64::from(h))].concat());
    let track = el(
        0xAE,
        &[uint(0xD7, 1), uint(0x73C5, 1), uint(0x83, 1), uint(0x9C, 0), text(0x86, "V_VP8"), uint(0x23E383, (1e9 / fps) as u64), video]
            .concat(),
    );
    let tracks = el(0x1654_AE6B, &track);
    // a cluster from each key frame (and after 30 s, the SimpleBlocks' relative times being 16 bits); their positions
    // from the start of the segment's body for the cues
    let mut starts: Vec<usize> = Vec::new();
    for (i, (_, key)) in frames.iter().enumerate() {
        if i == 0 || *key || starts.last().is_some_and(|s| ms(i).saturating_sub(ms(*s)) > 30_000) {
            starts.push(i);
        }
    }
    let mut body: Vec<u8> = [info, tracks].concat();
    let mut cues: Vec<u8> = Vec::new();
    for (k, first) in starts.iter().copied().enumerate() {
        let end = starts.get(k + 1).copied().unwrap_or(frames.len());
        let t0 = ms(first);
        let mut c = uint(0xE7, t0);
        for (j, (f, key)) in frames.get(first..end).unwrap_or(&[]).iter().enumerate() {
            let rel = ms(first + j).saturating_sub(t0).min(32767) as i16;
            let mut b = vec![0x81];
            b.extend(rel.to_be_bytes());
            b.push(if *key { 0x80 } else { 0x00 });
            b.extend_from_slice(f);
            c.extend(el(0xA3, &b));
        }
        let pos = body.len() as u64;
        body.extend(el(0x1F43_B675, &c));
        let tp = el(0xB7, &[uint(0xF7, 1), uint(0xF1, pos)].concat());
        cues.extend(el(0xBB, &[uint(0xB3, t0), tp].concat()));
    }
    body.extend(el(0x1C53_BB6B, &cues));
    [header, el(0x1853_8067, &body)].concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_elements() {
        assert_eq!(size_vint(0), vec![0x80]);
        assert_eq!(size_vint(126), vec![0xfe]);
        assert_eq!(size_vint(127), vec![0x40, 0x7f]);
        assert_eq!(el(0xE7, &[5]), vec![0xE7, 0x81, 5]);
        assert_eq!(uint(0xD7, 1), vec![0xD7, 0x81, 1]);
        let f = mux(&[(vec![1, 2, 3], true)], 16, 16, 30.0, "t");
        assert_eq!(&f[..4], &[0x1A, 0x45, 0xDF, 0xA3]);
        assert!(f.windows(5).any(|w| w == b"V_VP8"));
    }
}
