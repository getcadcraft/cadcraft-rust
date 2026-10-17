//! Reading the plan set's PDFs (`pdfset.js` `openPdf` without pdf.js): the SHA-256 of the bytes, each page's view box
//! (crop ∩ media box), /Rotate, /UserUnit, page labels, the viewport transform pdf.js would give it; pages rendered to
//! ink rasters with `hayro` (pure Rust) for auto count; the page's linework (the paths it draws, as a pdf.js-style
//! operator list for `snap.rs`). Every count read from the file is capped.

use hayro::hayro_interpret::hayro_syntax::Pdf;
use hayro::hayro_interpret::hayro_syntax::object::{Array, Dict, Number, Object};
use hayro::hayro_interpret::hayro_syntax::page::{Page, Rotation};
use hayro::hayro_interpret::util::{RectExt, TransformExt};
use hayro::hayro_interpret::{
    BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, ImageDrawProps, InterpreterCache, InterpreterSettings, SoftMask,
    interpret_page,
};
use hayro::kurbo::{Affine, BezPath, PathEl, Rect};
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::vello_cpu::{Pixmap, RenderContext};
use hayro::{RenderCache, RenderSettings, render_into};
use sha2::{Digest, Sha256};

use cc_takeoff::matcher::{Img, MAX_PIXELS, to_ink};
use cc_takeoff::snap::{self, Arg, Op, PDFJS_OPS};

/// Lowercase hex SHA-256.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let d = Sha256::digest(bytes);
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// An open PDF.
pub struct Doc {
    pub pdf: Pdf,
    pub hash: String,
    pub size: usize,
}

/// One page as `openPdf` describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct PageInfo {
    pub index: usize,
    pub label: String,
    pub number: String,
    pub view: [f64; 4],
    pub rotate: f64,
    pub user_unit: f64,
    pub width: f64,
    pub height: f64,
    pub transform: [f64; 6],
}

/// Open a PDF (no scripting, no forms: only the page tree and its content are read).
pub fn open(bytes: &[u8]) -> Result<Doc, String> {
    let pdf = Pdf::new(bytes.to_vec()).map_err(|e| format!("{e:?}"))?;
    Ok(Doc { pdf, hash: sha256_hex(bytes), size: bytes.len() })
}

fn rotation_deg(r: Rotation) -> f64 {
    match r {
        Rotation::None => 0.0,
        Rotation::Horizontal => 90.0,
        Rotation::Flipped => 180.0,
        Rotation::FlippedHorizontal => 270.0,
    }
}

/// pdf.js's `PageViewport` transform for a view box, scale and rotation.
pub fn viewport(view: &[f64; 4], scale: f64, rotation: f64) -> ([f64; 6], f64, f64) {
    let cx = (view[2] + view[0]) / 2.0;
    let cy = (view[3] + view[1]) / 2.0;
    let r = ((rotation % 360.0) + 360.0) % 360.0;
    let (a, b, c, d) = if r == 90.0 {
        (0.0, 1.0, 1.0, 0.0)
    } else if r == 180.0 {
        (-1.0, 0.0, 0.0, 1.0)
    } else if r == 270.0 {
        (0.0, -1.0, -1.0, 0.0)
    } else {
        (1.0, 0.0, 0.0, -1.0)
    };
    let (ox, oy, w, h) = if a == 0.0 {
        ((cy - view[1]).abs() * scale, (cx - view[0]).abs() * scale, (view[3] - view[1]).abs() * scale, (view[2] - view[0]).abs() * scale)
    } else {
        ((cx - view[0]).abs() * scale, (cy - view[1]).abs() * scale, (view[2] - view[0]).abs() * scale, (view[3] - view[1]).abs() * scale)
    };
    let t = [a * scale, b * scale, c * scale, d * scale, ox - a * scale * cx - c * scale * cy, oy - b * scale * cx - d * scale * cy];
    (t, w, h)
}
/// `convertToViewportPoint`.
pub fn to_viewport(t: &[f64; 6], x: f64, y: f64) -> (f64, f64) {
    (x * t[0] + y * t[2] + t[4], x * t[1] + y * t[3] + t[5])
}
/// `convertToPdfPoint` (pdf.js `applyInverseTransform`).
pub fn to_pdf(t: &[f64; 6], x: f64, y: f64) -> (f64, f64) {
    let d = t[0] * t[3] - t[1] * t[2];
    ((x * t[3] - y * t[2] + t[2] * t[5] - t[4] * t[3]) / d, (-x * t[1] + y * t[0] + t[4] * t[1] - t[5] * t[0]) / d)
}

fn page_view(page: &Page<'_>) -> [f64; 4] {
    let r: Rect = page.intersected_crop_box().to_kurbo();
    [r.x0.min(r.x1), r.y0.min(r.y1), r.x0.max(r.x1), r.y0.max(r.y1)]
}

fn user_unit(page: &Page<'_>) -> f64 {
    page.raw().get::<Number>(b"UserUnit".as_slice()).map(|n| n.as_f64()).filter(|u| u.is_finite() && *u > 0.0).unwrap_or(1.0)
}

/// The pages of the document as `openPdf` gives them (labels from /PageLabels when the document has them).
pub fn pages(doc: &Doc) -> Vec<PageInfo> {
    let labels = page_labels(&doc.pdf);
    doc.pdf
        .pages()
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let view = page_view(p);
            let rotate = rotation_deg(p.rotation());
            let uu = user_unit(p);
            let (transform, width, height) = viewport(&view, uu, rotate);
            let label = labels.as_ref().and_then(|l| l.get(i)).map(|s| clean_label(s)).unwrap_or_default();
            let number = crate::pdf::sheet_number(&label, i as f64);
            PageInfo { index: i, label, number, view, rotate, user_unit: uu, width, height, transform }
        })
        .collect()
}

fn clean_label(s: &str) -> String {
    let r: String = s.chars().map(|c| if (c as u32) <= 0x1f { ' ' } else { c }).collect();
    cc_m3d::js::slice16(cc_m3d::js::trim(&r), 80)
}

fn roman(mut n: u64, lower: bool) -> String {
    let table = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut s = String::new();
    for (v, t) in table {
        while n >= v && s.len() < 64 {
            s.push_str(t);
            n = n.saturating_sub(v);
        }
    }
    if lower { s.to_lowercase() } else { s }
}
fn letters(n: u64, lower: bool) -> String {
    // pdf.js: A..Z, AA..ZZ, AAA…: the letter repeated
    let base = if lower { b'a' } else { b'A' };
    let n1 = n.saturating_sub(1);
    let k = u8::try_from(n1 % 26).unwrap_or(0);
    let times = usize::try_from((n1 / 26).saturating_add(1)).unwrap_or(1).min(64);
    std::iter::repeat_n(char::from(base.saturating_add(k)), times).collect()
}

fn pdf_text(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let u: Vec<u16> = bytes.get(2..).unwrap_or(&[]).as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect();
        return String::from_utf16_lossy(&u);
    }
    bytes.iter().map(|b| char::from(*b)).collect()
}

/// /PageLabels → one label per page (pdf.js `getPageLabels`), or None when the document has none.
pub fn page_labels(pdf: &Pdf) -> Option<Vec<String>> {
    let xref = pdf.xref();
    let catalog = xref.get::<Dict<'_>>(xref.root_id())?;
    let tree = catalog.get::<Dict<'_>>(b"PageLabels".as_slice())?;
    let mut nums: Vec<(u64, Option<Dict<'_>>)> = Vec::new();
    let mut stack = vec![(tree, 0u32)];
    while let Some((node, depth)) = stack.pop() {
        if depth > 16 || nums.len() > 100_000 {
            break;
        }
        if let Some(arr) = node.get::<Array<'_>>(b"Nums".as_slice()) {
            let items: Vec<Object<'_>> = arr.iter::<Object<'_>>().take(200_000).collect();
            for pair in items.chunks(2) {
                if let [Object::Number(k), v] = pair {
                    let d = match v {
                        Object::Dict(d) => Some(d.clone()),
                        _ => None,
                    };
                    let kk = k.as_f64();
                    if kk.is_finite() && kk >= 0.0 {
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        nums.push((kk as u64, d));
                    }
                }
            }
        }
        if let Some(kids) = node.get::<Array<'_>>(b"Kids".as_slice()) {
            for k in kids.iter::<Dict<'_>>().take(1000) {
                stack.push((k, depth.saturating_add(1)));
            }
        }
    }
    nums.sort_by_key(|(k, _)| *k);
    let n = pdf.pages().len();
    let mut out = vec![String::new(); n];
    let mut cur: Option<&Dict<'_>> = None;
    let mut start = 1u64;
    let mut current_index = 0u64;
    let mut ni = 0usize;
    for (i, slot) in out.iter_mut().enumerate() {
        let iu = i as u64;
        while let Some((k, d)) = nums.get(ni) {
            if *k != iu {
                break;
            }
            cur = d.as_ref();
            let start_f = d
                .as_ref()
                .and_then(|d| d.get::<Number>(b"St".as_slice()))
                .map(|x| x.as_f64())
                .filter(|v| v.is_finite() && *v >= 1.0)
                .map_or(1.0, |v| v.floor().min(1e15));
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let start_u = start_f as u64;
            start = start_u;
            current_index = start;
            ni = ni.saturating_add(1);
        }
        let style = cur
            .and_then(|d| d.get::<hayro::hayro_interpret::hayro_syntax::object::Name<'_>>(b"S".as_slice()))
            .map(|s| String::from_utf8_lossy(&s).to_string());
        let prefix = cur
            .and_then(|d| d.get::<hayro::hayro_interpret::hayro_syntax::object::String<'_>>(b"P".as_slice()))
            .map(|s| pdf_text(s.as_bytes()))
            .unwrap_or_default();
        let body = match style.as_deref() {
            Some("D") => current_index.to_string(),
            Some("R") => roman(current_index, false),
            Some("r") => roman(current_index, true),
            Some("A") => letters(current_index.max(1), false),
            Some("a") => letters(current_index.max(1), true),
            _ => String::new(),
        };
        *slot = format!("{prefix}{body}");
        current_index = current_index.saturating_add(1);
    }
    let _ = start;
    Some(out)
}

/// A page rendered on white at `scale` (canvas size = the viewport's, rounded up) as RGBA, then as ink.
pub fn render_ink(doc: &Doc, index: usize, scale: f64) -> Result<Img, String> {
    let page = doc.pdf.pages().get(index).ok_or("no such page")?;
    let view = page_view(page);
    let rot = rotation_deg(page.rotation());
    let (_, vw, vh) = viewport(&view, scale, rot);
    let (wf, hf) = (vw.ceil(), vh.ceil());
    if !(wf >= 1.0 && hf >= 1.0 && wf <= 65535.0 && hf <= 65535.0) || wf * hf > MAX_PIXELS as f64 {
        return Err("raster too large".into());
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (w, h) = (wf as u16, hf as u16);
    let cache = RenderCache::new();
    let mut ctx = RenderContext::new(w, h);
    let transform = page.initial_transform(true).to_kurbo().then_scale(scale);
    render_into(page, &cache, &InterpreterSettings::default(), &RenderSettings::default(), &mut ctx, transform);
    ctx.flush();
    let mut pixmap = Pixmap::new(w, h);
    ctx.render_with(
        &mut pixmap,
        &mut hayro::vello_cpu::Resources::default(),
        hayro::vello_cpu::RasterizerSettings { target_init: hayro::vello_cpu::TargetInit::Clear(WHITE), ..Default::default() },
    );
    Ok(to_ink(pixmap.data_as_u8_slice(), usize::from(w), usize::from(h)))
}

/// The linework of a page (in user space, through each path's CTM), as snap.rs reads pdf.js's operator list.
pub fn page_linework(doc: &Doc, index: usize, max_segs: usize) -> Result<snap::Linework, String> {
    let page = doc.pdf.pages().get(index).ok_or("no such page")?;
    struct Collect {
        ops: Vec<Op>,
        n: usize,
    }
    impl<'a> Device<'a> for Collect {
        fn draw_path(&mut self, path: &BezPath, props: DrawProps<'a>, _draw_mode: &DrawMode) {
            if self.n > 4_000_000 {
                return;
            }
            let c = props.transform.as_coeffs();
            let mut d = Vec::new();
            for el in path.elements() {
                match el {
                    PathEl::MoveTo(p) => d.extend_from_slice(&[0.0, p.x, p.y]),
                    PathEl::LineTo(p) => d.extend_from_slice(&[1.0, p.x, p.y]),
                    PathEl::CurveTo(a, b, p) => d.extend_from_slice(&[2.0, a.x, a.y, b.x, b.y, p.x, p.y]),
                    PathEl::QuadTo(a, p) => d.extend_from_slice(&[3.0, a.x, a.y, p.x, p.y]),
                    PathEl::ClosePath => d.push(4.0),
                }
            }
            self.n = self.n.saturating_add(d.len());
            self.ops.push(Op { f: PDFJS_OPS.save, args: Vec::new() });
            self.ops.push(Op { f: PDFJS_OPS.transform, args: c.iter().map(|v| Arg::Num(*v)).collect() });
            self.ops.push(Op { f: PDFJS_OPS.construct_path, args: vec![Arg::Path(20, d)] });
            self.ops.push(Op { f: PDFJS_OPS.restore, args: Vec::new() });
        }
        fn push_clip_path(&mut self, _clip_path: &ClipPath) {}
        fn push_transparency_group(&mut self, _opacity: f32, _mask: Option<SoftMask<'a>>, _blend_mode: BlendMode) {}
        fn draw_glyph_run(&mut self, _g: &hayro::hayro_interpret::font::GlyphRun<'_, 'a>, _props: DrawProps<'a>, _draw_mode: &DrawMode) {}
        fn draw_image(&mut self, _image: hayro::hayro_interpret::Image<'a, '_>, _props: ImageDrawProps<'a>) {}
        fn pop_clip(&mut self) {}
        fn pop_transparency_group(&mut self) {}
    }
    let cache = InterpreterCache::new();
    let mut state =
        Context::new(Affine::IDENTITY, hayro::kurbo::Rect::new(-1e9, -1e9, 1e9, 1e9), &cache, page.xref(), InterpreterSettings::default());
    let mut dev = Collect { ops: Vec::new(), n: 0 };
    interpret_page(page, &mut state, &mut dev);
    Ok(snap::extract_segments(&PDFJS_OPS, &dev.ops, max_segs))
}

/// What "Auto count" does: render at the working scale, cut the template from the box (PDF user space of
/// `template_from`), search page `index` (rendered `scale_by` times larger). → working scale and candidates with
/// their centres in PDF user space.
pub struct Search {
    pub w: f64,
    pub cands: Vec<(cc_takeoff::matcher::Cand, (f64, f64))>,
    pub template: Img,
    pub image: Img,
}
pub fn search_sheet(
    doc: &Doc,
    index: usize,
    bx: [f64; 4],
    rotations: &[f64],
    template_from: usize,
    scale_by: f64,
) -> Result<Search, String> {
    let info = pages(doc);
    let tp = info.get(template_from).ok_or("no such page")?;
    let (t1, _, _) = viewport(&tp.view, tp.user_unit, tp.rotate);
    let ds = [(bx[0], bx[1]), (bx[2], bx[3]), (bx[0], bx[3]), (bx[2], bx[1])].map(|(x, y)| to_viewport(&t1, x, y));
    let min_x = ds.iter().map(|p| p.0).fold(f64::INFINITY, cc_m3d::js::min);
    let max_x = ds.iter().map(|p| p.0).fold(f64::NEG_INFINITY, cc_m3d::js::max);
    let min_y = ds.iter().map(|p| p.1).fold(f64::INFINITY, cc_m3d::js::min);
    let max_y = ds.iter().map(|p| p.1).fold(f64::NEG_INFINITY, cc_m3d::js::max);
    let w = cc_takeoff::matcher::work_scale(max_x - min_x, max_y - min_y, 40.0);
    let src = render_ink(doc, template_from, w * tp.user_unit)?;
    let tpl =
        cc_takeoff::matcher::trim(&cc_takeoff::matcher::crop(&src, min_x * w, min_y * w, (max_x - min_x) * w, (max_y - min_y) * w), 0.08)
            .img;
    let wt = w * scale_by;
    let ti = info.get(index).ok_or("no such page")?;
    let img = if template_from == index && scale_by == 1.0 { src } else { render_ink(doc, index, wt * ti.user_unit)? };
    let (vt, _, _) = viewport(&ti.view, ti.user_unit, ti.rotate);
    let found = cc_takeoff::matcher::find_symbol(&img, &tpl, rotations, 0.5, cc_takeoff::matcher::BLUR, None, None).unwrap_or_default();
    let cands = found
        .into_iter()
        .map(|mut c| {
            let center = to_pdf(&vt, (c.x + c.w / 2.0) / wt, (c.y + c.h / 2.0) / wt);
            c.sheet = Some("s".into());
            (c, center)
        })
        .collect();
    Ok(Search { w, cands, template: tpl, image: img })
}
