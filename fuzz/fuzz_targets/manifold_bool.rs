//! Fuzz cc-manifold (the boolean behind IFC openings and ModelCraft's solid tools): bytes → two meshes → merge,
//! construct, every boolean, a list union / intersection, a chain on the results, getMesh. Degenerate, non-manifold,
//! coincident and huge inputs must give an error status or a mesh, never a panic, a hang or an OOM.
//!
//! The first byte picks how each mesh is made, so the fuzzer reaches deep into the boolean instead of stopping at
//! validation: a parametrised primitive (box / prism with a byte-driven rotation and scale: valid solids), a
//! primitive with byte-driven damage (snapped, collapsed, flipped, duplicated, cut open: degenerate but often still
//! accepted), or raw floats and indices (mostly refused).
#![no_main]

use cc_manifold::{Manifold, Mesh, OpType};
use libfuzzer_sys::fuzz_target;

struct Bytes<'a> {
    d: &'a [u8],
    i: usize,
}

impl Bytes<'_> {
    fn u8(&mut self) -> u8 {
        let b = self.d.get(self.i).copied().unwrap_or(0);
        self.i = self.i.saturating_add(1);
        b
    }
    fn unit(&mut self) -> f32 {
        f32::from(self.u8()) / 255.0
    }
    fn f32_raw(&mut self) -> f32 {
        f32::from_le_bytes([self.u8(), self.u8(), self.u8(), self.u8()])
    }
    fn left(&self) -> bool {
        self.i < self.d.len()
    }
}

const CUBE: [u32; 36] = [1, 0, 4, 2, 4, 0, 1, 3, 0, 3, 1, 5, 3, 2, 0, 3, 7, 2, 5, 4, 6, 5, 1, 4, 6, 4, 2, 7, 6, 2, 7, 3, 5, 7, 5, 6];

fn primitive(b: &mut Bytes) -> Mesh {
    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut tris: Vec<u32> = Vec::new();
    if b.u8() % 2 == 0 {
        let (w, h, d) = (0.1 + b.unit(), 0.1 + b.unit(), 0.1 + b.unit());
        for x in [-w, w] {
            for y in [-h, h] {
                for z in [-d, d] {
                    pos.push([x, y, z]);
                }
            }
        }
        tris.extend_from_slice(&CUBE);
    } else {
        // a prism over an n-gon (or a star), capped with fans
        let n = 3 + u32::from(b.u8() % 40);
        let star = b.u8() % 3 == 0;
        let (z0, z1) = (-0.1 - b.unit(), 0.1 + b.unit());
        for z in [z0, z1] {
            for i in 0..n {
                let a = std::f32::consts::TAU * i as f32 / n as f32;
                let r = if star && i % 2 == 1 { 0.3 } else { 0.7 };
                pos.push([r * a.cos(), r * a.sin(), z]);
            }
        }
        pos.push([0.0, 0.0, z0]);
        pos.push([0.0, 0.0, z1]);
        let (cb, ct) = (2 * n, 2 * n + 1);
        for i in 0..n {
            let j = (i + 1) % n;
            tris.extend_from_slice(&[cb, j, i, ct, n + i, n + j, i, j, n + j, i, n + j, n + i]);
        }
    }
    // rotation (quarter turns keep faces exactly coplanar) and offset
    let quarter = b.u8() % 2 == 0;
    let (a, c) = if quarter {
        let k = f32::from(b.u8() % 4) * std::f32::consts::FRAC_PI_2;
        (k.sin().round(), k.cos().round())
    } else {
        let t = b.unit() * std::f32::consts::TAU;
        (t.sin(), t.cos())
    };
    let off = [b.unit() - 0.5, b.unit() - 0.5, b.unit() - 0.5];
    let snap = b.u8() % 3 == 0;
    let mut v = Vec::with_capacity(pos.len() * 3);
    for p in &pos {
        let mut q = [c * p[0] - a * p[1] + off[0], a * p[0] + c * p[1] + off[1], p[2] + off[2]];
        if snap {
            for x in &mut q {
                *x = (*x * 4.0).round() / 4.0;
            }
        }
        v.extend_from_slice(&q);
    }
    Mesh::new(3, v, tris)
}

fn damaged(b: &mut Bytes) -> Mesh {
    let mut m = primitive(b);
    let nv = m.num_vert().max(1);
    for _ in 0..(b.u8() % 6) {
        match b.u8() % 7 {
            0 => {
                // two vertices made coincident
                let (i, j) = (usize::from(b.u8()) % nv, usize::from(b.u8()) % nv);
                for k in 0..3 {
                    if let Some(&x) = m.vert_properties.get(3 * j + k) {
                        if let Some(y) = m.vert_properties.get_mut(3 * i + k) {
                            *y = x;
                        }
                    }
                }
            }
            1 => {
                // a triangle flipped
                let t = 3 * (usize::from(b.u8()) % m.num_tri().max(1));
                if t + 2 < m.tri_verts.len() {
                    m.tri_verts.swap(t + 1, t + 2);
                }
            }
            2 => {
                // a triangle duplicated
                let t = 3 * (usize::from(b.u8()) % m.num_tri().max(1));
                let tri: Vec<u32> = m.tri_verts.iter().skip(t).take(3).copied().collect();
                m.tri_verts.extend(tri);
            }
            3 => {
                // cut open
                let t = 3 * (usize::from(b.u8()) % m.num_tri().max(1));
                if t + 3 <= m.tri_verts.len() {
                    m.tri_verts.drain(t..t + 3);
                }
            }
            4 => {
                // a vertex moved by a tiny amount (near-coincident, near-coplanar)
                let i = 3 * (usize::from(b.u8()) % nv) + usize::from(b.u8() % 3);
                if let Some(x) = m.vert_properties.get_mut(i) {
                    *x = f32::from_bits(x.to_bits().wrapping_add(u32::from(b.u8() % 4)));
                }
            }
            5 => m.tolerance = b.unit() * 0.5,
            _ => {
                // extra properties
                let k = u32::from(b.u8() % 4);
                let mut vp = Vec::new();
                for (vi, p) in m.vert_properties.chunks(3).enumerate() {
                    vp.extend_from_slice(p);
                    for j in 0..k {
                        vp.push(((vi as u32 + j) % 5) as f32 * 0.25);
                    }
                }
                m.num_prop = 3 + k;
                m.vert_properties = vp;
            }
        }
    }
    m
}

fn raw(b: &mut Bytes) -> Mesh {
    let nv = usize::from(b.u8() % 64);
    let nt = usize::from(b.u8() % 128);
    let num_prop = 3 + u32::from(b.u8() % 3);
    let small = b.u8() % 2 == 0;
    let mut v = Vec::new();
    for _ in 0..nv * num_prop as usize {
        v.push(if small { f32::from(b.u8() % 8) * 0.5 } else { b.f32_raw() });
    }
    let mut t = Vec::new();
    for _ in 0..nt * 3 {
        t.push(u32::from(b.u8()) % (nv as u32 + 2));
    }
    Mesh::new(num_prop, v, t)
}

fn mesh(b: &mut Bytes) -> Mesh {
    match b.u8() % 4 {
        0 | 1 => primitive(b),
        2 => damaged(b),
        _ => raw(b),
    }
}

fn check(m: &Manifold) {
    let _ = m.status();
    let g = m.get_mesh();
    let np = g.num_prop as usize;
    assert!(np >= 3 && g.vert_properties.len() % np == 0, "mesh shape");
    let nv = g.vert_properties.len() / np;
    assert!(g.tri_verts.iter().all(|&v| (v as usize) < nv), "index in range");
}

fuzz_target!(|data: &[u8]| {
    let mut b = Bytes { d: data, i: 0 };
    let mut ms = Vec::new();
    for _ in 0..2 + usize::from(b.u8() % 3) {
        let mut m = mesh(&mut b);
        if b.u8() % 4 != 0 {
            m.merge();
        }
        ms.push(Manifold::from_mesh_unchecked(&m));
        if !b.left() {
            break;
        }
    }
    let (Some(a), Some(c)) = (ms.first().cloned(), ms.get(1).cloned()) else { return };
    for op in [OpType::Subtract, OpType::Add, OpType::Intersect] {
        check(&a.boolean(&c, op));
    }
    check(&Manifold::union(&ms));
    check(&Manifold::intersection(&ms));
    check(&Manifold::difference(&ms));
    // a chain with handles dropped (the C++ tree collapses then) and kept
    let mut acc = a.clone();
    let mut kept = Vec::new();
    for (k, m) in ms.iter().enumerate().skip(1) {
        let next = match b.u8() % 3 {
            0 => acc.subtract(m),
            1 => acc.add(m),
            _ => acc.intersect(m),
        };
        if k % 2 == 1 {
            kept.push(acc);
        }
        acc = next;
    }
    check(&acc);
    drop(kept);
});
