//! Ported from the frozen Node test (bin/test-takeoffcraft.mjs at 594ab3e): "Snap to the drawing's linework (T1)",
//! "Typed lengths and coordinates (T3)", the raster half of "Revision overlay (T13)", "Symbol matching (synthetic
//! rasters)".

use cc_takeoff::geom::{self, P};
use cc_takeoff::matcher::{self as mt, Img};
use cc_takeoff::overlay as ov;
use cc_takeoff::snap::{self as sn, Arg, Kinds, Linework, Op, PDFJS_OPS, SnapIndex};
use cc_takeoff::typein::{self as ty, Ctx};

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * 1f64.max(a.abs()).max(b.abs())
}
const STROKE: i64 = 20;
const FILL: i64 = 22;
fn path(op: i64, d: &[f64]) -> Op {
    Op { f: PDFJS_OPS.construct_path, args: vec![Arg::Path(op, d.iter().map(|v| f64::from(*v as f32)).collect()), Arg::Other] }
}
fn op(f: i64, args: Vec<Arg>) -> Op {
    Op { f, args }
}

#[test]
fn snap_to_linework() {
    let o = PDFJS_OPS;
    let list = vec![
        path(STROKE, &[0.0, 0.0, 0.0, 1.0, 100.0, 0.0, 1.0, 100.0, 50.0, 4.0]),
        path(o.end_path, &[0.0, 0.0, 0.0, 1.0, 500.0, 500.0]),
        op(o.save, vec![]),
        op(o.transform, vec![Arg::Num(2.0), Arg::Num(0.0), Arg::Num(0.0), Arg::Num(2.0), Arg::Num(10.0), Arg::Num(20.0)]),
        path(STROKE, &[0.0, 0.0, 0.0, 1.0, 5.0, 0.0]),
        op(o.restore, vec![]),
        op(o.paint_form_begin, vec![Arg::List(vec![1.0, 0.0, 0.0, 1.0, 200.0, 0.0]), Arg::List(vec![0.0, 0.0, 10.0, 10.0])]),
        path(FILL, &[0.0, 0.0, 0.0, 1.0, 0.0, 10.0]),
        op(o.paint_form_end, vec![]),
        path(STROKE, &[0.0, 300.0, 0.0, 2.0, 300.0, 55.23, 344.77, 100.0, 400.0, 100.0]),
    ];
    let lw = sn::extract_segments(&o, &list, sn::MAX_SEGS);
    assert_eq!(lw.n, 3 + 1 + 1 + 8);
    let seg = |i: usize| lw.segs[i * 4..i * 4 + 4].to_vec();
    assert_eq!(seg(2), vec![100.0, 50.0, 0.0, 0.0]);
    assert_eq!(seg(3), vec![10.0, 20.0, 20.0, 20.0]);
    assert_eq!(seg(4), vec![200.0, 0.0, 200.0, 10.0]);
    assert!(lw.curve[5] == 3 && lw.curve[12] == 5 && lw.curve[6..12].iter().all(|f| *f == 1), "{:?}", lw.curve);
    let idx = SnapIndex::new(&lw);
    let hit = |x: f64, y: f64| idx.snap(x, y, 3.0, Kinds::default());
    let h = hit(1.0, 1.0).unwrap();
    assert!(h.kind == "end" && h.x == 0.0 && h.y == 0.0);
    let h = hit(50.5, 1.5).unwrap();
    assert!(h.kind == "mid" && h.x == 50.0 && h.y == 0.0);
    let h = hit(70.0, 0.5).unwrap();
    assert!(h.kind == "near" && near(h.x, 70.0) && h.y == 0.0);
    assert!(hit(70.0, 5.0).is_none());
    let h = hit(15.0, 20.4).unwrap();
    assert!(h.kind == "mid" && h.x == 15.0 && h.y == 20.0);
    let q = |t: f64| {
        let u = 1.0 - t;
        (
            u.powi(3) * 300.0 + 3.0 * u * u * t * 300.0 + 3.0 * u * t * t * 344.77 + t.powi(3) * 400.0,
            3.0 * u * u * t * 55.23 + 3.0 * u * t * t * 100.0 + t.powi(3) * 100.0,
        )
    };
    let (mx, my) = q(0.5);
    assert_eq!(hit(mx + 0.5, my + 0.5).unwrap().kind, "near");
    let h = hit(399.0, 99.0).unwrap();
    assert!(h.kind == "end" && h.x == 400.0 && h.y == 100.0);
    // crossings
    let x2 = sn::extract_segments(
        &o,
        &[path(STROKE, &[0.0, 0.0, 0.0, 1.0, 100.0, 100.0]), path(STROKE, &[0.0, 0.0, 40.0, 1.0, 100.0, 40.0])],
        sn::MAX_SEGS,
    );
    let ix = SnapIndex::new(&x2);
    let h = ix.snap(41.5, 40.5, 3.0, Kinds::default()).unwrap();
    assert!(h.kind == "int" && near(h.x, 40.0) && near(h.y, 40.0));
    assert_eq!(ix.snap(41.5, 40.5, 3.0, Kinds { int: false, ..Kinds::default() }).unwrap().kind, "near");
    assert!(sn::intersect(&[0.0, 0.0, 10.0, 0.0, 0.0, 1.0, 10.0, 1.0], 0, 1).is_none());
    assert!(sn::intersect(&[0.0, 0.0, 10.0, 0.0, 10.0, 0.0, 10.0, 10.0], 0, 1).is_none());
    assert_eq!(sn::intersect(&[0.0, 0.0, 10.0, 0.0, 5.0, 0.0, 5.0, 10.0], 0, 1).map(|p| p.0), Some(5.0));
    let x3 = sn::extract_segments(
        &o,
        &[path(STROKE, &[0.0, 0.0, 0.0, 1.0, 100.0, 100.0]), path(STROKE, &[0.0, 0.0, 100.0, 1.0, 100.0, 0.0])],
        sn::MAX_SEGS,
    );
    assert_eq!(SnapIndex::new(&x3).snap(51.0, 50.0, 3.0, Kinds::default()).unwrap().kind, "int");
    let capped = sn::extract_segments(&o, &list, 4);
    assert!(capped.n == 4 && capped.truncated);
    assert!(SnapIndex::new(&Linework::default()).snap(0.0, 0.0, 5.0, Kinds::default()).is_none());
}

#[test]
fn typed_lengths_and_coordinates() {
    assert_eq!(ty::signed_length("-4'"), Some(-4.0));
    assert_eq!(ty::signed_length("0"), Some(0.0));
    assert_eq!(ty::signed_length("0'"), Some(0.0));
    assert_eq!(ty::signed_length("6'6\""), Some(6.5));
    assert_eq!(ty::signed_length("+3"), Some(3.0));
    assert_eq!(ty::signed_length("x"), None);
    let ctx = Ctx {
        first: Some(P::new(0.0, 0.0)),
        last: Some(P::new(100.0, 100.0)),
        cursor: Some(P::new(200.0, 100.0)),
        ft_per_unit: 1.0 / 18.0,
        linear: [2.0, 0.0, 0.0, -2.0],
    };
    let pe = |t: &str| ty::parse_entry(t, &ctx);
    let (p, k) = pe("10'").unwrap();
    assert!(k == "dist" && near(p.x, 280.0) && near(p.y, 100.0));
    let (p, k) = pe("5'<90").unwrap();
    assert!(k == "polar" && near(p.x, 100.0) && near(p.y, 190.0));
    let (p, _) = pe("10'<-45").unwrap();
    assert!(near(p.x, 100.0 + 180.0 / 2f64.sqrt()) && near(p.y, 100.0 - 180.0 / 2f64.sqrt()));
    let (p, k) = pe("@6',-4'").unwrap();
    assert!(k == "rel" && near(p.x, 208.0) && near(p.y, 28.0));
    let (p, k) = pe("6',4'").unwrap();
    assert!(k == "abs" && near(p.x, 108.0) && near(p.y, 72.0));
    let (p, k) = pe("#6',4'").unwrap();
    assert!(k == "abs" && near(p.x, 108.0));
    let (p, _) = pe("2 m").unwrap();
    assert!(near(p.x, 100.0 + 2.0 / 0.3048 * 18.0));
    assert_eq!(pe("@0,0").unwrap_err(), "typein.err.zero");
    assert_eq!(ty::parse_entry("10'", &Ctx { cursor: Some(P::new(100.0, 100.0)), ..ctx.clone() }).unwrap_err(), "typein.err.dir");
    assert_eq!(ty::parse_entry("10'", &Ctx { cursor: None, ..ctx.clone() }).unwrap_err(), "typein.err.dir");
    assert_eq!(pe("abc").unwrap_err(), "typein.err.read");
    assert_eq!(pe("1',2',3'").unwrap_err(), "typein.err.read");
    assert_eq!(ty::parse_entry("10'", &Ctx { ft_per_unit: 0.0, ..ctx.clone() }).unwrap_err(), "typein.err.noscale");
    assert_eq!(ty::parse_entry("10'", &Ctx { last: None, ..ctx.clone() }).unwrap_err(), "typein.err.first");
    for lin in [[0.0, 2.0, 2.0, 0.0], [-2.0, 0.0, 0.0, 2.0], [0.0, -2.0, -2.0, 0.0]] {
        let c = Ctx { linear: lin, ..ctx.clone() };
        let q = ty::parse_entry("@1',0", &c).unwrap().0;
        let (sx, sy) = (lin[0] * (q.x - 100.0) + lin[2] * (q.y - 100.0), lin[1] * (q.x - 100.0) + lin[3] * (q.y - 100.0));
        let u = ty::parse_entry("@0,1'", &c).unwrap().0;
        let (ux, uy) = (lin[0] * (u.x - 100.0) + lin[2] * (u.y - 100.0), lin[1] * (u.x - 100.0) + lin[3] * (u.y - 100.0));
        assert!(near(sx, 36.0) && near(sy, 0.0) && near(ux, 0.0) && near(uy, -36.0), "{lin:?}");
    }
}

#[test]
fn revision_overlay_rasters() {
    let a = ov::fit_two_points(&P::new(0.0, 0.0), &P::new(10.0, 0.0), &P::new(5.0, 5.0), &P::new(5.0, 25.0)).unwrap();
    let (q1, q2, q3) = (geom::apply(&a, &P::new(0.0, 0.0)), geom::apply(&a, &P::new(10.0, 0.0)), geom::apply(&a, &P::new(0.0, 10.0)));
    assert!(near(q1.x, 5.0) && near(q1.y, 5.0) && near(q2.x, 5.0) && near(q2.y, 25.0));
    assert!(near(ov::scale_of(&a), 2.0) && near(q3.x, -15.0) && near(q3.y, 5.0));
    assert!(ov::fit_two_points(&P::new(1.0, 1.0), &P::new(1.0, 1.0), &P::new(0.0, 0.0), &P::new(5.0, 0.0)).is_none());
    let (w, h) = (60usize, 40usize);
    let img = |lines: &[[usize; 4]]| {
        let mut d = vec![0f32; w * h];
        for [x0, y0, x1, y1] in lines {
            for y in *y0..=*y1 {
                for x in *x0..=*x1 {
                    d[y * w + x] = 1.0;
                }
            }
        }
        Img { data: d, w, h }
    };
    let old = img(&[[5, 10, 50, 10], [5, 30, 50, 30], [20, 12, 20, 28]]);
    let new = img(&[[5, 11, 50, 11], [5, 30, 50, 30], [40, 12, 40, 28]]);
    let same = ov::warp(&old, &ov::pixel_map(&ov::IDENTITY, &ov::IDENTITY, &ov::IDENTITY, 1.0), w, h);
    assert!(same.data.iter().zip(&old.data).all(|(a, b)| (a - b).abs() < 1e-6));
    let shifted = ov::warp(&old, &ov::pixel_map(&ov::IDENTITY, &ov::IDENTITY, &[1.0, 0.0, 0.0, 1.0, 3.0, 0.0], 1.0), w, h);
    assert!(shifted.data[10 * w + 23] > 0.99 && shifted.data[10 * w + 4] == 0.0);
    assert_eq!(ov::dilate(&img(&[[30, 20, 30, 20]]), 2).data.iter().map(|v| f64::from(*v)).sum::<f64>(), 25.0);
    let cm = ov::change_mask(&old, &new, 2, 0.35, 0.2);
    assert_eq!((cm.removed, cm.added), (14, 15));
    let px = |p: &P| P::new(p.x, p.y);
    let wall = [P::new(5.0, 10.0), P::new(50.0, 10.0)];
    let part = [P::new(20.0, 12.0), P::new(20.0, 28.0)];
    let room = [P::new(22.0, 12.0), P::new(38.0, 12.0), P::new(38.0, 28.0), P::new(22.0, 28.0)];
    let dot = [P::new(45.0, 20.0)];
    assert_eq!(ov::mark_change("linear", &wall, &[], &cm, &px, 3.0).total, 0);
    assert_eq!(ov::mark_change("linear", &part, &[], &cm, &px, 3.0).removed, 14);
    assert_eq!(ov::mark_change("area", &room, &[], &cm, &px, 1.0).total, 0);
    assert!(ov::mark_change("count", &dot, &[], &cm, &px, 6.0).added > 0);
    let (rw, rp) = (ov::mark_change("linear", &wall, &[], &cm, &px, 2.0), ov::mark_change("linear", &part, &[], &cm, &px, 2.0));
    assert!(!ov::changed(&rw, "linear", 0.15) && ov::changed(&rp, "linear", 0.15));
    assert!(ov::changed(&ov::mark_change("count", &dot, &[], &cm, &px, 6.0), "count", 0.15));
    let longer = ov::change_mask(&img(&[[5, 35, 30, 35]]), &img(&[[5, 35, 50, 35]]), 2, 0.35, 0.2);
    let rl = ov::mark_change("linear", &[P::new(5.0, 35.0), P::new(30.0, 35.0)], &[], &longer, &px, 3.0);
    assert!(rl.corner && rl.share < 0.15 && ov::changed(&rl, "linear", 0.15));
    assert_eq!(ov::box_sum(&ov::sum_of(&[1, 0, 1, 1], 2, 2), 2, 2, 0.0, 0.0, 1.0, 1.0), 3);
}

#[test]
fn symbol_matching_synthetic() {
    let (w, h) = (120usize, 80usize);
    let mut img = Img { data: vec![0.0; w * h], w, h };
    let mut glyph = Img { data: vec![0.0; 96], w: 12, h: 8 };
    for y in 0..8 {
        for x in 0..12 {
            glyph.data[y * 12 + x] = if y == 0 || y == 7 || x == 0 || x == 11 || x == y + 2 || (x > 7 && y > 3) { 1.0 } else { 0.0 };
        }
    }
    let stamp = |img: &mut Img, t: &Img, x0: usize, y0: usize| {
        for y in 0..t.h {
            for x in 0..t.w {
                img.data[(y0 + y) * w + x0 + x] = t.data[y * t.w + x];
            }
        }
    };
    let r90 = mt::rot90(&glyph);
    let r180 = mt::rot90(&r90);
    let r270 = mt::rot90(&r180);
    stamp(&mut img, &glyph, 5, 5);
    stamp(&mut img, &r90, 40, 5);
    stamp(&mut img, &r180, 70, 10);
    stamp(&mut img, &r270, 100, 40);
    let mut bx = Img { data: vec![0.0; 96], w: 12, h: 8 };
    for y in 0..8 {
        for x in 0..12 {
            bx.data[y * 12 + x] = if y == 0 || y == 7 || x == 0 || x == 11 { 1.0 } else { 0.0 };
        }
    }
    stamp(&mut img, &bx, 10, 50);
    assert_eq!(mt::rot90(&mt::rot90(&r270)).data, r90.data);
    assert_eq!(mt::rot90(&r270).data, glyph.data);
    let all = mt::find_symbol(&img, &glyph, &[0.0, 90.0, 180.0, 270.0], 0.5, mt::BLUR, None, None).unwrap();
    let found = mt::suppress(&all, 0.8, 0.25);
    assert_eq!(found.len(), 4);
    let mut rots: Vec<f64> = found.iter().map(|c| c.rot).collect();
    rots.sort_by(f64::total_cmp);
    assert_eq!(rots, vec![0.0, 90.0, 180.0, 270.0]);
    assert!(found.iter().all(|c| c.score > 0.999));
    let mut at: Vec<String> = found.iter().map(|c| format!("{},{}", c.x, c.y)).collect();
    at.sort();
    let mut want: Vec<String> = ["5,5", "40,5", "70,10", "100,40"].iter().map(|s| (*s).to_string()).collect();
    want.sort();
    assert_eq!(at, want);
    assert!(mt::template_problem(&Img { data: vec![0.0; 100], w: 10, h: 10 }).unwrap().contains("empty"));
    assert!(mt::template_problem(&Img { data: vec![1.0; 30], w: 3, h: 10 }).unwrap().contains("too small"));
    let mut loose = Img { data: vec![0.0; 280], w: 20, h: 14 };
    for y in 0..8 {
        for x in 0..12 {
            loose.data[(y + 3) * 20 + x + 4] = glyph.data[y * 12 + x];
        }
    }
    let tr = mt::trim(&loose, 0.08);
    assert!(tr.img.w == 14 && tr.img.h == 10);
    let stop = || true;
    let mut progress = |_: f64| {};
    assert!(mt::find_symbol(&img, &glyph, &[0.0, 90.0, 180.0, 270.0], 0.5, mt::BLUR, Some(&mut progress), Some(&stop)).is_none());
    // hostile sizes never panic
    let _ = mt::find_symbol(&Img::default(), &glyph, &[0.0], 0.5, 0, None, None);
    let _ = mt::find_symbol(&img, &Img::default(), &[0.0], 0.5, 0, None, None);
    let _ = mt::crop(&img, -1e9, f64::NAN, 1e12, -5.0);
}
