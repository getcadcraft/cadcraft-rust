//! cloud-cad.js `CloudAttach`: a point cloud under the drawing (0.5 SG.1). Not part of the drawing: not saved in the
//! DWG / DXF, untouched by undo; one per window, under whichever drawing shows (the host keeps it: [`crate::Host::cloud`]).
//! The Model tab draws it under the linework — every point seen from above, the highest on top, or only the slice —
//! and in sheet viewports; object snap Node snaps to the points drawn; the 3D tab shows it with its lowest point on
//! the ground.
//!
//! The reading, colours, slices and snap grid are [`cc_io_civil::pointcloud`] (pointcloud.js). Lengths here are in
//! drawing units, converted from the cloud's file units by the drawing's `units` (the callers pass
//! `doc.meta.units || ''`).

// The draw's casts are of values checked first: a pane of at most 32768 device pixels a side, a point size of at most
// a few hundred pixels, screen positions under 10^15.
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use std::sync::Arc;

use cc_cad::js;
use cc_geom::P2;
use cc_io_civil::pointcloud::{self as pcl, Cloud, PlanGrid};

/// `mPerUnit(units)` (model3d.js): metres per drawing unit; unitless = millimetres.
pub fn m_per_unit(units: &str) -> f64 {
    match units {
        "" | "millimeters" => 0.001,
        "inches" => 0.0254,
        "feet" => 0.3048,
        "miles" => 1609.344,
        "centimeters" => 0.01,
        "meters" => 1.0,
        "kilometers" => 1000.0,
        "microinches" => 2.54e-8,
        "mils" => 2.54e-5,
        "yards" => 0.9144,
        _ => 0.001,
    }
}

/// cloud-cad.js `UNIT_M[u]` (undefined → NaN, as the JS arithmetic sees it).
fn unit_m(u: Option<&str>) -> f64 {
    u.and_then(pcl::unit_m).unwrap_or(f64::NAN)
}

/// The slice (a plan cut to trace from): on, its height above the cloud's lowest point and its thickness, in drawing
/// units (None = the defaults: 1.2 m and 0.1 m).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Slice {
    pub on: bool,
    pub h: Option<f64>,
    pub thick: Option<f64>,
}

/// The colour modes, as the JS names them.
pub const COLOR_KEYS: [(&str, &str); 5] = [("F", "rgb"), ("H", "height"), ("I", "intensity"), ("C", "class"), ("O", "single")];

/// A drawn picture of the cloud: `w` × `h` device pixels, each `0xAABBGGRR` (RGBA bytes in little-endian order;
/// transparent where no point fell).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Raster {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u32>,
}

impl Raster {
    /// The pixels as RGBA bytes.
    pub fn rgba(&self) -> impl Iterator<Item = u8> + '_ {
        self.px.iter().flat_map(|p| p.to_le_bytes())
    }
}

/// The draw's transform: file offsets → device pixels, the point's square (`s` px, `h0` its half) and the pane width.
struct Proj {
    a: f64,
    b: f64,
    c: f64,
    e: f64,
    tx: f64,
    ty: f64,
    h0: i64,
    s: i64,
    w: i64,
}

/// The points drawn: positions, colours, and the slice's indices (None = all).
struct Pts<'a> {
    pos: &'a [f32],
    rgb: &'a [u8],
    idx: Option<&'a [u32]>,
}

/// A point's square on screen: its top-left device pixel, or `SKIP` (off the pane, not drawn).
const SKIP: (i32, i32) = (i32::MIN, i32::MIN);

/// Where point `i` lands: `Math.round(a·x + c·y + tx) − h0` (and y), off-pane points skipped.
fn place(p: &Proj, pts: &Pts<'_>, i: usize, hh: i64) -> (i32, i32) {
    let Some(&[x, y, _]) = i.checked_mul(3).and_then(|o| pts.pos.get(o..o.saturating_add(3))) else { return SKIP };
    let (x, y) = (f64::from(x), f64::from(y));
    let fx = js::round(p.a * x + p.c * y + p.tx);
    let fy = js::round(p.b * x + p.e * y + p.ty);
    if !(fx.is_finite() && fy.is_finite() && fx.abs() <= 1e15 && fy.abs() <= 1e15) {
        return SKIP;
    }
    let sx = (fx as i64).saturating_sub(p.h0);
    let sy = (fy as i64).saturating_sub(p.h0);
    if sx.saturating_add(p.s) <= 0 || sy.saturating_add(p.s) <= 0 || sx >= p.w || sy >= hh {
        return SKIP;
    }
    // on the pane: −s < sx < w, −s < sy < h (both under 32769)
    (sx as i32, sy as i32)
}

/// Plot every placed point (in order) into the rows of one band (`r0` its first row; `px` / `zb` its pixels).
fn plot_band(p: &Proj, pts: &Pts<'_>, scr: &[(i32, i32)], r0: i64, px: &mut [u32], zb: &mut [f32]) {
    let wu = p.w as usize;
    if wu == 0 {
        return;
    }
    let r1 = r0.saturating_add(px.len().checked_div(wu).unwrap_or(0) as i64);
    for (k, &(sx, sy)) in scr.iter().enumerate() {
        if sy == i32::MIN {
            continue;
        }
        let (sx, sy) = (i64::from(sx), i64::from(sy));
        if sy.saturating_add(p.s) <= r0 || sy >= r1 {
            continue;
        }
        let i = match pts.idx {
            Some(ix) => ix.get(k).map_or(usize::MAX, |v| *v as usize),
            None => k,
        };
        let o = i.saturating_mul(3);
        let Some(z) = pts.pos.get(o.saturating_add(2)).copied() else { continue };
        let Some(&[r, g, b]) = pts.rgb.get(o..o.saturating_add(3)) else { continue };
        let col = 0xff00_0000u32 | (u32::from(b) << 16) | (u32::from(g) << 8) | u32::from(r);
        let (u0, u1) = (sx.max(0) as usize, p.w.min(sx.saturating_add(p.s)) as usize);
        for v in sy.max(r0)..r1.min(sy.saturating_add(p.s)) {
            let row = (v.saturating_sub(r0) as usize).saturating_mul(wu);
            let (Some(zr), Some(pr)) =
                (zb.get_mut(row.saturating_add(u0)..row.saturating_add(u1)), px.get_mut(row.saturating_add(u0)..row.saturating_add(u1)))
            else {
                continue;
            };
            for (zv, pv) in zr.iter_mut().zip(pr.iter_mut()) {
                if z >= *zv {
                    *zv = z;
                    *pv = col;
                }
            }
        }
    }
}

/// What a slice was made for: (version, height bits, thickness bits).
type ShownKey = (u64, u64, u64);

/// `CloudAttach`.
#[derive(Clone, Debug)]
pub struct CloudAttach {
    pub cloud: Option<Arc<Cloud>>,
    /// "rgb" | "height" | "intensity" | "class" | "single"
    pub color: &'static str,
    /// point size in CSS pixels (1–8)
    pub size: f64,
    pub visible: bool,
    pub slice: Slice,
    /// bumped by every change (the view redraws, the 3D tab rebuilds)
    pub version: u64,
    /// the cloud's lower-left corner in the drawing; None = the file's own coordinates
    pub insert: Option<P2>,
    pub colors: Option<Arc<Vec<u8>>>,
    /// `shown()`'s indices and what they were made for
    shown: Option<(ShownKey, Arc<Vec<u32>>)>,
    /// the snap grid and what it was made for (`_shownKey | version`)
    grid: Option<(Option<ShownKey>, u64, Arc<PlanGrid>)>,
    /// the last `shown()` key (`_shownKey`: kept while the slice is off, as the JS keeps it)
    shown_key: Option<ShownKey>,
    /// the last picture and its depths, kept for the next draw
    buf: Option<Raster>,
    zb: Vec<f32>,
    /// where each drawn point landed (the last draw's)
    scr: Vec<(i32, i32)>,
}

impl Default for CloudAttach {
    fn default() -> Self {
        CloudAttach {
            cloud: None,
            color: "rgb",
            size: 2.0,
            visible: true,
            slice: Slice::default(),
            version: 0,
            insert: None,
            colors: None,
            shown: None,
            grid: None,
            shown_key: None,
            buf: None,
            zb: Vec::new(),
            scr: Vec::new(),
        }
    }
}

fn mode_of(m: &str) -> &'static str {
    COLOR_KEYS.iter().find(|(_, v)| *v == m).map_or("height", |(_, v)| v)
}

impl CloudAttach {
    pub fn has(&self) -> bool {
        self.cloud.is_some()
    }

    /// Drawing units per file unit: from both units when both are known, else 1 (the numbers as they are).
    pub fn scale(&self, units: &str) -> f64 {
        match self.cloud.as_deref() {
            Some(c) if !units.is_empty() && c.unit.is_some() => unit_m(c.unit.as_deref()) / m_per_unit(units),
            _ => 1.0,
        }
    }
    pub fn z_scale(&self, units: &str) -> f64 {
        let Some(c) = self.cloud.as_deref() else { return 1.0 };
        let zu = c.z_unit.as_deref().or(c.unit.as_deref());
        if units.is_empty() || zu.is_none() {
            return 1.0;
        }
        unit_m(zu) / m_per_unit(units)
    }

    /// `attach(cloud, { insert })`.
    pub fn attach(&mut self, cloud: Cloud, insert: Option<P2>) {
        self.insert = insert;
        if !pcl::color_modes(&cloud).contains(&self.color) {
            self.color = if cloud.rgb.is_some() { "rgb" } else { "height" };
        }
        self.colors = Some(Arc::new(pcl::colorize(&cloud, self.color, [200, 200, 200])));
        // a scan of a building (taller than 2 m, under 200 m across) starts as a plan cut: seen from above it is its
        // ceiling or roof; a site or aerial scan starts with every point
        let or1 = |v: f64| if v == 0.0 || v.is_nan() { 1.0 } else { v };
        let um = or1(unit_m(Some(cloud.unit.as_deref().unwrap_or("m"))));
        let zm = or1(unit_m(Some(cloud.z_unit.as_deref().or(cloud.unit.as_deref()).unwrap_or("m"))));
        let across = js::max(cloud.max[0] - cloud.min[0], cloud.max[1] - cloud.min[1]) * um;
        let tall = (cloud.max[2] - cloud.min[2]) * zm;
        self.slice = Slice { on: tall > 2.0 && across < 200.0, h: None, thick: None };
        self.visible = true;
        self.cloud = Some(Arc::new(cloud));
        self.changed();
    }
    pub fn detach(&mut self) {
        self.cloud = None;
        self.colors = None;
        self.changed();
    }
    /// `changed()`: a new version (the view and the 3D tab redraw from it), the snap grid made again.
    pub fn changed(&mut self) {
        self.version = self.version.wrapping_add(1);
        self.grid = None;
    }
    /// The colour mode by its key (POINTCLOUD Color), colours made again with `single` for "one color".
    pub fn set_color(&mut self, mode: &str, single: [u8; 3]) {
        self.color = mode_of(mode);
        if let Some(c) = self.cloud.as_deref() {
            self.colors = Some(Arc::new(pcl::colorize(c, self.color, single)));
        }
        self.changed();
    }

    /// A file point (absolute, file units) → the drawing.
    pub fn to_drawing(&self, x: f64, y: f64, z: f64, units: &str) -> [f64; 3] {
        let k = self.scale(units);
        let Some(c) = self.cloud.as_deref() else { return [x, y, z] };
        let zz = (z - c.min[2]) * self.z_scale(units);
        match self.insert {
            None => [x * k, y * k, zz],
            Some(i) => [i.x + (x - c.min[0]) * k, i.y + (y - c.min[1]) * k, zz],
        }
    }
    pub fn to_file(&self, p: P2, units: &str) -> [f64; 2] {
        let k = self.scale(units);
        match (self.insert, self.cloud.as_deref()) {
            (Some(i), Some(c)) => [(p.x - i.x) / k + c.min[0], (p.y - i.y) / k + c.min[1]],
            _ => [p.x / k, p.y / k],
        }
    }
    pub fn drawing_point(&self, i: usize, units: &str) -> [f64; 3] {
        let Some(c) = self.cloud.as_deref() else { return [f64::NAN; 3] };
        let p = pcl::point_at(c, i);
        self.to_drawing(p[0], p[1], p[2], units)
    }
    /// The cloud's box in drawing units: (min x, min y, max x, max y).
    pub fn bbox(&self, units: &str) -> Option<[f64; 4]> {
        let c = self.cloud.as_deref()?;
        let a = self.to_drawing(c.min[0], c.min[1], 0.0, units);
        let b = self.to_drawing(c.max[0], c.max[1], 0.0, units);
        Some([js::min(a[0], b[0]), js::min(a[1], b[1]), js::max(a[0], b[0]), js::max(a[1], b[1])])
    }
    /// The slice's height and thickness in drawing units (defaults 1.2 m and 0.1 m, from the drawing's units).
    pub fn slice_spec(&self, units: &str) -> (f64, f64) {
        let per = {
            let p = m_per_unit(units);
            if p == 0.0 { 0.001 } else { p }
        };
        let unitless = units.is_empty();
        let cu = self.cloud.as_deref().and_then(|c| c.unit.as_deref()).unwrap_or("m");
        let m = |v: f64| {
            if unitless {
                let u = unit_m(Some(cu));
                v / if u == 0.0 || u.is_nan() { 1.0 } else { u }
            } else {
                v / per
            }
        };
        (self.slice.h.unwrap_or_else(|| m(1.2)), self.slice.thick.unwrap_or_else(|| m(0.1)))
    }
    /// The indices drawn: the slice's points, or None (every point).
    pub fn shown(&mut self, units: &str) -> Option<Arc<Vec<u32>>> {
        let c = self.cloud.clone()?;
        if !self.slice.on {
            return None;
        }
        let (h, t) = self.slice_spec(units);
        let key = (self.version, h.to_bits(), t.to_bits());
        if let Some((k, s)) = &self.shown
            && *k == key
        {
            self.shown_key = Some(key);
            return Some(Arc::clone(s));
        }
        let zs = self.z_scale(units);
        let s = Arc::new(pcl::slice_of(&c, 2, c.min[2] + h / zs, t / zs));
        self.shown = Some((key, Arc::clone(&s)));
        self.shown_key = Some(key);
        Some(s)
    }

    /// `snap(p, r)`: the drawn point nearest `p` within `r` (drawing units), in the drawing.
    pub fn snap(&mut self, p: P2, r: f64, units: &str) -> Option<P2> {
        if !self.visible {
            return None;
        }
        let c = self.cloud.clone()?;
        let idx = self.shown(units);
        let fresh = matches!(&self.grid, Some((k, v, _)) if *k == self.shown_key && *v == self.version);
        if !fresh {
            let g = Arc::new(PlanGrid::new(&c, idx.as_deref().map(Vec::as_slice), 0.0));
            self.grid = Some((self.shown_key, self.version, g));
        }
        let g = Arc::clone(&self.grid.as_ref()?.2);
        let [fx, fy] = self.to_file(p, units);
        let i = g.nearest(&c, fx, fy, r / self.scale(units))?;
        let q = self.drawing_point(i, units);
        Some(P2::new(q[0], q[1]))
    }

    /// `draw(ctx, T, view)`'s picture: every point shown, in one image of the pane, the highest point winning a pixel.
    /// `t`: drawing → CSS px of the pane (a canvas transform `[a, b, c, d, e, f]`); `w`, `h`: the pane in CSS px; `dpr`:
    /// device pixels per CSS px. None when nothing is drawn (no cloud, hidden). The picture's buffers are kept for the
    /// next draw, as the JS keeps its canvas; natively the pane is drawn in bands of rows at once (each band takes
    /// every point in order, so the picture is the one-thread picture).
    pub fn draw(&mut self, t: [f64; 6], w: f64, h: f64, dpr: f64, units: &str) -> Option<&Raster> {
        if !self.visible {
            return None;
        }
        let c = self.cloud.clone()?;
        let rgb = self.colors.clone()?;
        let d = dpr;
        let ww = js::max(1.0, (w * d).ceil());
        let hh = js::max(1.0, (h * d).ceil());
        // a pane past 32k device pixels a side is not a screen: refuse rather than allocate gigabytes
        if !(ww.is_finite() && hh.is_finite() && ww <= 32768.0 && hh <= 32768.0) {
            return None;
        }
        let k = self.scale(units);
        let (ox, oy) = match self.insert {
            Some(i) => (i.x + (c.origin[0] - c.min[0]) * k, i.y + (c.origin[1] - c.min[1]) * k),
            None => (c.origin[0] * k, c.origin[1] * k),
        };
        let s = js::max(1.0, js::round(self.size * d * 0.75));
        if !(s <= 512.0) {
            return None;
        }
        // drawing x = ox + k·pos.x → device px = d·(T0·x + T2·y + T4)
        let pr = Proj {
            a: d * t[0] * k,
            b: d * t[1] * k,
            c: d * t[2] * k,
            e: d * t[3] * k,
            tx: d * (t[0] * ox + t[2] * oy + t[4]),
            ty: d * (t[1] * ox + t[3] * oy + t[5]),
            h0: (s as i64) >> 1,
            s: s as i64,
            w: ww as i64,
        };
        let (wu, hu) = (ww as usize, hh as usize);
        let len = wu.saturating_mul(hu);
        let mut buf = self.buf.take().unwrap_or_default();
        buf.w = wu;
        buf.h = hu;
        // the bands clear their own rows (px.fill(0); zb.fill(-Infinity))
        buf.px.resize(len, 0);
        self.zb.resize(len, f32::NEG_INFINITY);
        let shown = self.shown(units);
        let idx = shown.as_deref().map(Vec::as_slice);
        let pts = Pts { pos: &c.pos, rgb: &rgb, idx };
        let m = idx.map_or(c.n, <[u32]>::len);
        let hi = hh as i64;
        let at = |k: usize| match idx {
            Some(ix) => ix.get(k).map_or(usize::MAX, |v| *v as usize),
            None => k,
        };
        self.scr.clear();
        self.scr.resize(m, SKIP);
        let bands = 16;
        let rows = hu.div_ceil(bands).max(1);
        let chunk = rows.saturating_mul(wu).max(1);
        #[cfg(not(target_arch = "wasm32"))]
        {
            use rayon::prelude::*;
            // where each point lands (in parallel), then each band of rows takes the points in order
            self.scr.par_chunks_mut(65536).enumerate().for_each(|(ci, ch)| {
                for (k, slot) in ch.iter_mut().enumerate() {
                    *slot = place(&pr, &pts, at(ci.saturating_mul(65536).saturating_add(k)), hi);
                }
            });
            let scr = &self.scr;
            buf.px.par_chunks_mut(chunk).zip(self.zb.par_chunks_mut(chunk)).enumerate().for_each(|(bi, (px, zb))| {
                px.fill(0);
                zb.fill(f32::NEG_INFINITY);
                plot_band(&pr, &pts, scr, bi.saturating_mul(rows) as i64, px, zb);
            });
        }
        #[cfg(target_arch = "wasm32")]
        {
            for (k, slot) in self.scr.iter_mut().enumerate() {
                *slot = place(&pr, &pts, at(k), hi);
            }
            buf.px.fill(0);
            self.zb.fill(f32::NEG_INFINITY);
            plot_band(&pr, &pts, &self.scr, 0, &mut buf.px, &mut self.zb);
            let _ = (chunk, rows);
        }
        self.buf = Some(buf);
        self.buf.as_ref()
    }

    /// The 3D tab: the cloud's points (relative to its origin, file units) → metres, Z up, its lowest point on the
    /// ground (column-major 4 × 4).
    pub fn matrix3d(&self, m_per: f64, units: &str) -> Option<[f64; 16]> {
        let c = self.cloud.as_deref()?;
        let k = self.scale(units) * m_per;
        let kz = self.z_scale(units) * m_per;
        let o = self.to_drawing(c.origin[0], c.origin[1], c.origin[2], units);
        Some([k, 0.0, 0.0, 0.0, 0.0, k, 0.0, 0.0, 0.0, 0.0, kz, 0.0, o[0] * m_per, o[1] * m_per, o[2] * m_per, 1.0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cloud() -> Cloud {
        Cloud {
            name: "t.xyz".into(),
            kind: pcl::Kind::Xyz,
            read: 3.0,
            n: 3,
            origin: [100.0, 200.0, 10.0],
            pos: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.2, 2.0, 0.0, 3.0],
            rgb: None,
            inten: None,
            cls: None,
            min: [100.0, 200.0, 10.0],
            max: [102.0, 201.0, 13.0],
            class_counts: vec![0.0; 256],
            unit: Some("m".into()),
            z_unit: None,
            notes: Vec::new(),
        }
    }

    #[test]
    fn attach_slice_snap_and_draw() {
        let mut a = CloudAttach::default();
        a.attach(cloud(), None);
        assert_eq!(a.color, "height");
        assert!(a.slice.on);
        assert_eq!(a.scale("millimeters"), 1000.0);
        assert_eq!(a.slice_spec("meters"), (1.2, 0.1));
        assert_eq!(a.shown("meters").as_deref(), Some(&vec![1u32]));
        assert_eq!(a.snap(P2::new(101.1, 201.1), 0.5, "meters"), Some(P2::new(101.0, 201.0)));
        assert_eq!(a.snap(P2::new(100.0, 200.0), 0.5, "meters"), None);
        a.slice.on = false;
        a.changed();
        assert_eq!(a.snap(P2::new(100.1, 200.0), 0.5, "meters"), Some(P2::new(100.0, 200.0)));
        let r = a.draw([10.0, 0.0, 0.0, -10.0, -995.0, 2025.0], 40.0, 30.0, 1.0, "meters").cloned().unwrap_or_default();
        assert_eq!((r.w, r.h), (40, 30));
        assert!(r.px.iter().filter(|p| **p >> 24 == 255).count() >= 3);
        assert_eq!(a.bbox("meters"), Some([100.0, 200.0, 102.0, 201.0]));
        // a huge pane is refused, not allocated
        assert!(a.draw([1.0, 0.0, 0.0, 1.0, 0.0, 0.0], 1e9, 1e9, 1.0, "").is_none());
    }
}
