//! `orient2d` of robust-predicates 3.0.3 (github.com/mourner/robust-predicates, a JavaScript port of Jonathan
//! Shewchuk's adaptive predicates), ported one for one so the value — not only its sign — is the JS's: tin.js divides
//! it by a length to decide "within a hair of the line". robust-predicates is released into the public domain under
//! the Unlicense (notice in NOTICE / THIRD_PARTY.md). `incircle` is only read for its sign in tin.js, so it comes from
//! the `robust` crate (the same Shewchuk algorithm; the sign of an exact predicate is unique).

const EPSILON: f64 = 1.110_223_024_625_156_5e-16;
const SPLITTER: f64 = 134_217_729.0;
const RESULTERRBOUND: f64 = (3.0 + 8.0 * EPSILON) * EPSILON;
const CCWERRBOUND_A: f64 = (3.0 + 16.0 * EPSILON) * EPSILON;
const CCWERRBOUND_B: f64 = (2.0 + 12.0 * EPSILON) * EPSILON;
const CCWERRBOUND_C: f64 = (9.0 + 64.0 * EPSILON) * EPSILON * EPSILON;

/// fast_expansion_sum_zeroelim: h = e + f; returns h's length.
fn sum(elen: usize, e: &[f64], flen: usize, f: &[f64], h: &mut [f64]) -> usize {
    let at = |a: &[f64], i: usize| a.get(i).copied().unwrap_or(f64::NAN);
    let mut q;
    let mut qnew;
    let mut hh;
    let mut bvirt;
    let mut enow = at(e, 0);
    let mut fnow = at(f, 0);
    let mut eindex = 0usize;
    let mut findex = 0usize;
    if (fnow > enow) == (fnow > -enow) {
        q = enow;
        eindex += 1;
        enow = at(e, eindex);
    } else {
        q = fnow;
        findex += 1;
        fnow = at(f, findex);
    }
    let mut hindex = 0usize;
    let push = |h: &mut [f64], hindex: &mut usize, v: f64| {
        if let Some(slot) = h.get_mut(*hindex) {
            *slot = v;
        }
        *hindex += 1;
    };
    if eindex < elen && findex < flen {
        if (fnow > enow) == (fnow > -enow) {
            qnew = enow + q;
            hh = q - (qnew - enow);
            eindex += 1;
            enow = at(e, eindex);
        } else {
            qnew = fnow + q;
            hh = q - (qnew - fnow);
            findex += 1;
            fnow = at(f, findex);
        }
        q = qnew;
        if hh != 0.0 {
            push(h, &mut hindex, hh);
        }
        while eindex < elen && findex < flen {
            if (fnow > enow) == (fnow > -enow) {
                qnew = q + enow;
                bvirt = qnew - q;
                hh = q - (qnew - bvirt) + (enow - bvirt);
                eindex += 1;
                enow = at(e, eindex);
            } else {
                qnew = q + fnow;
                bvirt = qnew - q;
                hh = q - (qnew - bvirt) + (fnow - bvirt);
                findex += 1;
                fnow = at(f, findex);
            }
            q = qnew;
            if hh != 0.0 {
                push(h, &mut hindex, hh);
            }
        }
    }
    while eindex < elen {
        qnew = q + enow;
        bvirt = qnew - q;
        hh = q - (qnew - bvirt) + (enow - bvirt);
        eindex += 1;
        enow = at(e, eindex);
        q = qnew;
        if hh != 0.0 {
            push(h, &mut hindex, hh);
        }
    }
    while findex < flen {
        qnew = q + fnow;
        bvirt = qnew - q;
        hh = q - (qnew - bvirt) + (fnow - bvirt);
        findex += 1;
        fnow = at(f, findex);
        q = qnew;
        if hh != 0.0 {
            push(h, &mut hindex, hh);
        }
    }
    if q != 0.0 || hindex == 0 {
        push(h, &mut hindex, q);
    }
    hindex
}

fn estimate(elen: usize, e: &[f64]) -> f64 {
    let mut q = e.first().copied().unwrap_or(f64::NAN);
    for x in e.iter().take(elen).skip(1) {
        q += *x;
    }
    q
}

/// Two_Product split terms: (s1, s0) with s1 = a·b rounded and s0 its error.
#[inline]
fn two_product(a: f64, b: f64) -> (f64, f64) {
    let s1 = a * b;
    let mut c = SPLITTER * a;
    let ahi = c - (c - a);
    let alo = a - ahi;
    c = SPLITTER * b;
    let bhi = c - (c - b);
    let blo = b - bhi;
    let s0 = alo * blo - (s1 - ahi * bhi - alo * bhi - ahi * blo);
    (s1, s0)
}

/// Two_Two_Diff of (s1, s0) − (t1, t0) into a 4-term expansion.
#[inline]
fn two_two_diff(s1: f64, s0: f64, t1: f64, t0: f64) -> [f64; 4] {
    let mut out = [0.0; 4];
    let mut i = s0 - t0;
    let mut bvirt = s0 - i;
    out[0] = s0 - (i + bvirt) + (bvirt - t0);
    let j = s1 + i;
    bvirt = j - s1;
    let z = s1 - (j - bvirt) + (i - bvirt);
    i = z - t1;
    bvirt = z - i;
    out[1] = z - (i + bvirt) + (bvirt - t1);
    let u3 = j + i;
    bvirt = u3 - j;
    out[2] = j - (u3 - bvirt) + (i - bvirt);
    out[3] = u3;
    out
}

fn orient2dadapt(ax: f64, ay: f64, bx: f64, by: f64, cx: f64, cy: f64, detsum: f64) -> f64 {
    let acx = ax - cx;
    let bcx = bx - cx;
    let acy = ay - cy;
    let bcy = by - cy;

    let (s1, s0) = two_product(acx, bcy);
    let (t1, t0) = two_product(acy, bcx);
    let b = two_two_diff(s1, s0, t1, t0);

    let mut det = estimate(4, &b);
    let mut errbound = CCWERRBOUND_B * detsum;
    if det >= errbound || -det >= errbound {
        return det;
    }

    let mut bvirt = ax - acx;
    let acxtail = ax - (acx + bvirt) + (bvirt - cx);
    bvirt = bx - bcx;
    let bcxtail = bx - (bcx + bvirt) + (bvirt - cx);
    bvirt = ay - acy;
    let acytail = ay - (acy + bvirt) + (bvirt - cy);
    bvirt = by - bcy;
    let bcytail = by - (bcy + bvirt) + (bvirt - cy);

    if acxtail == 0.0 && acytail == 0.0 && bcxtail == 0.0 && bcytail == 0.0 {
        return det;
    }

    errbound = CCWERRBOUND_C * detsum + RESULTERRBOUND * det.abs();
    det += (acx * bcytail + bcy * acxtail) - (acy * bcxtail + bcx * acytail);
    if det >= errbound || -det >= errbound {
        return det;
    }

    let (s1, s0) = two_product(acxtail, bcy);
    let (t1, t0) = two_product(acytail, bcx);
    let u = two_two_diff(s1, s0, t1, t0);
    let mut c1 = [0.0f64; 8];
    let c1len = sum(4, &b, 4, &u, &mut c1);

    let (s1, s0) = two_product(acx, bcytail);
    let (t1, t0) = two_product(acy, bcxtail);
    let u = two_two_diff(s1, s0, t1, t0);
    let mut c2 = [0.0f64; 12];
    let c2len = sum(c1len, &c1, 4, &u, &mut c2);

    let (s1, s0) = two_product(acxtail, bcytail);
    let (t1, t0) = two_product(acytail, bcxtail);
    let u = two_two_diff(s1, s0, t1, t0);
    let mut d = [0.0f64; 16];
    let dlen = sum(c2len, &c2, 4, &u, &mut d);

    d.get(dlen.wrapping_sub(1)).copied().unwrap_or(f64::NAN)
}

/// robust-predicates' `orient2d(ax, ay, bx, by, cx, cy)`: positive when a, b, c run clockwise in y-up
/// coordinates (the JS library's sign, the negation of Shewchuk's).
pub fn orient2d(ax: f64, ay: f64, bx: f64, by: f64, cx: f64, cy: f64) -> f64 {
    let detleft = (ay - cy) * (bx - cx);
    let detright = (ax - cx) * (by - cy);
    let det = detleft - detright;
    let detsum = (detleft + detright).abs();
    if det.abs() >= CCWERRBOUND_A * detsum {
        return det;
    }
    -orient2dadapt(ax, ay, bx, by, cx, cy, detsum)
}

/// robust-predicates' `incircle(…)`: > 0 when d lies inside the circle through a, b, c (counter-clockwise).
#[allow(clippy::too_many_arguments)]
pub fn incircle(ax: f64, ay: f64, bx: f64, by: f64, cx: f64, cy: f64, dx: f64, dy: f64) -> f64 {
    robust::incircle(
        robust::Coord { x: ax, y: ay },
        robust::Coord { x: bx, y: by },
        robust::Coord { x: cx, y: cy },
        robust::Coord { x: dx, y: dy },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signs() {
        assert!(orient2d(0.0, 0.0, 1.0, 0.0, 0.0, 1.0) < 0.0);
        assert!(orient2d(0.0, 0.0, 0.0, 1.0, 1.0, 0.0) > 0.0);
        // exactly collinear (adaptive path)
        assert_eq!(orient2d(0.1, 0.1, 0.2, 0.2, 0.30000000000000004, 0.30000000000000004), 0.0);
        assert!(incircle(0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.25, 0.25) > 0.0);
    }
}
