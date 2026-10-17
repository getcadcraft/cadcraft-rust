//! Ported from the frozen Node test (bin/test-takeoffcraft.mjs at 594ab3e): "Feet-inches and metric parsing",
//! "Feet-inches formatting", "Preset scales against PDF points", "Two-point calibration", "Polygon area, cut-outs,
//! perimeter", "Arcs and circles", "Formula parser", "Slope" (units part).

use cc_takeoff::formula;
use cc_takeoff::geom::{self, Op, P};
use cc_takeoff::units as u;
use std::f64::consts::PI;

fn near(a: f64, b: f64, t: f64) -> bool {
    (a - b).abs() <= t * 1f64.max(a.abs()).max(b.abs())
}
fn ft(s: &str) -> Result<f64, String> {
    u::parse_length(s, "ft")
}

#[test]
fn feet_inches_and_metric_parsing() {
    let cases: &[(&str, f64)] = &[
        ("24'-0\"", 24.0),
        ("24'0\"", 24.0),
        ("24' 0\"", 24.0),
        ("24'", 24.0),
        ("24 ft", 24.0),
        ("24 feet", 24.0),
        ("24ft", 24.0),
        ("12' 6 1/2\"", 12.0 + 6.5 / 12.0),
        ("12'-6 1/2\"", 12.0 + 6.5 / 12.0),
        ("12'-6-1/2\"", 12.0 + 6.5 / 12.0),
        ("12'6-1/2\"", 12.0 + 6.5 / 12.0),
        ("12' 6", 12.5),
        ("12'-6\"", 12.5),
        ("6 1/2\"", 6.5 / 12.0),
        ("6-1/2\"", 6.5 / 12.0),
        ("3/4\"", 0.0625),
        ("1/16\"", 1.0 / 192.0),
        ("0'-3/4\"", 0.0625),
        ("5'-3/4\"", 5.0 + 0.75 / 12.0),
        ("100'-11 15/16\"", 100.0 + (11.0 + 15.0 / 16.0) / 12.0),
        ("24.5'", 24.5),
        ("24.5", 24.5),
        (".5'", 0.5),
        ("30\"", 2.5),
        ("30 in", 2.5),
        ("30 inches", 2.5),
        ("6.25\"", 6.25 / 12.0),
        ("24 ft 6 in", 24.5),
        ("1,250'", 1250.0),
        ("12′-6″", 12.5),
        ("12’ 6”", 12.5),
        ("12'6''", 12.5),
        ("3.2 m", 3.2 / 0.3048),
        ("3200 mm", 3.2 / 0.3048),
        ("320 cm", 3.2 / 0.3048),
        ("3.2m", 3.2 / 0.3048),
        ("0.3048 m", 1.0),
        ("304.8mm", 1.0),
        ("  9'-6\"  ", 9.5),
    ];
    for (s, want) in cases {
        let got = ft(s);
        assert!(got.as_ref().is_ok_and(|v| near(*v, *want, 1e-12)), "parse {s} = {want}, got {got:?}");
    }
    assert!(near(u::parse_length("2.5", "m").unwrap(), 2.5 / 0.3048, 1e-9));
    assert!(near(u::parse_length("18", "in").unwrap(), 1.5, 1e-9));
    for bad in ["", "abc", "-5'", "0", "0'-0\"", "1/0\"", "12' 6 7", "6.5 1/2\"", "3 m 20 cm", "5\"6'", "ft", "1.2.3", "12'-6\"x"] {
        assert!(ft(bad).is_err(), "refuses {bad:?}: {:?}", ft(bad));
    }
}

#[test]
fn feet_inches_formatting() {
    let cases: &[(f64, &str)] = &[
        (24.0, "24'-0\""),
        (12.5, "12'-6\""),
        (12.0 + 6.5 / 12.0, "12'-6 1/2\""),
        (0.0625, "0'-0 3/4\""),
        (1.0 / 192.0, "0'-0 1/16\""),
        (11.99999, "12'-0\""),
        (11.0 + 11.97 / 12.0, "12'-0\""),
        (11.0 + 11.96 / 12.0, "11'-11 15/16\""),
        (0.0, "0'-0\""),
        (-2.5, "-2'-6\""),
        (100.0 + (11.0 + 15.0 / 16.0) / 12.0, "100'-11 15/16\""),
        (1.0 + 0.5 / 12.0, "1'-0 1/2\""),
        (9.5, "9'-6\""),
        (16.125, "16'-1 1/2\""),
        (3.0 + 4.25 / 12.0, "3'-4 1/4\""),
        (3.0 + 4.125 / 12.0, "3'-4 1/8\""),
        (3.0 + 4.375 / 12.0, "3'-4 3/8\""),
    ];
    for (v, want) in cases {
        assert_eq!(u::ft_in(*v), *want, "format {v}");
    }
    assert_eq!(u::format_ft_in(12.0 + 6.3 / 12.0, 8.0), "12'-6 1/4\"");
    let mut bad = 0;
    for n in 0..=(30 * 12 * 16) {
        let v = f64::from(n) / 192.0;
        let back = u::parse_length(&u::ft_in(v), "ft");
        let ok = if n == 0 { back.is_err() } else { back.as_ref().is_ok_and(|b| near(*b, v, 1e-12)) };
        if !ok {
            bad += 1;
        }
    }
    assert_eq!(bad, 0, "every 1/16\" step 0–30 ft round-trips");
    assert_eq!(u::format_length(3.2 / 0.3048, "m", 2), "3.200 m");
    assert_eq!(u::format_length(1.0, "mm", 2), "305 mm");
    assert_eq!(u::fixed(1_234_567.891, 2), "1,234,567.89");
    assert_eq!(u::round_to(1.005, 2), 1.01);
    assert_eq!(u::round_to(2.675, 2), 2.68);
    assert_eq!(u::round_to(-2.5, 0), -3.0);
}

#[test]
fn preset_scales_and_calibration() {
    let at = |k: &str, units: f64, uu: f64| u::preset_ft_per_unit(k, uu).unwrap() * units;
    assert!(near(at("a1/4", 72.0, 1.0), 4.0, 1e-9));
    assert!(near(u::preset_ft_per_unit("a1/4", 1.0).unwrap(), 1.0 / 18.0, 1e-9));
    assert!(near(at("a1/8", 72.0, 1.0), 8.0, 1e-9));
    assert!(near(at("a3/32", 72.0, 1.0), 128.0 / 12.0, 1e-9));
    assert!(near(at("a1-1/2", 72.0, 1.0), 8.0 / 12.0, 1e-9));
    assert!(near(at("a3", 72.0, 1.0), 1.0 / 3.0, 1e-9));
    for n in [10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 100.0] {
        assert!(near(at(&format!("e{n}"), 72.0, 1.0), n, 1e-9));
    }
    for n in [1.0, 2.0, 5.0, 10.0, 20.0, 25.0, 50.0, 100.0, 200.0, 250.0, 500.0, 1000.0] {
        assert!(near(at(&format!("m{n}"), 72.0, 1.0) * 0.3048, 0.0254 * n, 1e-9));
    }
    assert!(near(at("m100", 72.0 / 25.4 * 10.0, 1.0), 1.0 / 0.3048, 1e-9));
    assert!(near(at("a1/4", 36.0, 2.0), 4.0, 1e-9));
    assert_eq!(u::PRESETS.len(), 30);
    let (f, _) = u::calibrate(930.48, 1157.28, 930.48, 1328.28, ft("9'-6\"").unwrap()).unwrap();
    assert!(near(f, 1.0 / 18.0, 1e-12));
    assert_eq!(u::match_preset(f, 1.0, 0.005).map(|p| p.0.key), Some("a1/4"));
    let (d, _) = u::calibrate(0.0, 0.0, 300.0, 400.0, 25.0).unwrap();
    assert!(near(d, 0.05, 1e-9));
    assert!(u::calibrate(1.0, 1.0, 1.2, 1.0, 10.0).is_err());
    assert!(u::calibrate(0.0, 0.0, 100.0, 0.0, 0.0).is_err());
    assert!(u::match_preset(1.0 / 18.0 * 1.02, 1.0, 0.005).is_none());
    assert_eq!(u::describe_scale(1.0 / 18.0, 1.0), "1\" = 4'");
    let (m, _) = u::calibrate(0.0, 0.0, 72.0 / 25.4 * 50.0, 0.0, ft("5 m").unwrap()).unwrap();
    assert_eq!(u::match_preset(m, 1.0, 0.005).map(|p| p.0.key), Some("m100"));
}

fn r(x: f64, y: f64, w: f64, h: f64) -> Vec<P> {
    vec![P::new(x, y), P::new(x + w, y), P::new(x + w, y + h), P::new(x, y + h)]
}

#[test]
fn polygon_area_cutouts_perimeter() {
    assert_eq!(geom::polygon_area(&r(0.0, 0.0, 10.0, 20.0)), 200.0);
    let mut rev = r(0.0, 0.0, 10.0, 20.0);
    rev.reverse();
    assert_eq!(geom::polygon_area(&rev), 200.0);
    assert_eq!(geom::perimeter(&r(0.0, 0.0, 10.0, 20.0)), 60.0);
    let l = [P::new(0.0, 0.0), P::new(30.0, 0.0), P::new(30.0, 10.0), P::new(10.0, 10.0), P::new(10.0, 40.0), P::new(0.0, 40.0)];
    assert_eq!(geom::polygon_area(&l), 600.0);
    assert_eq!(geom::perimeter(&l), 140.0);
    let a = geom::area_with_holes(&r(0.0, 0.0, 100.0, 50.0), &[r(10.0, 10.0, 20.0, 5.0), r(50.0, 20.0, 10.0, 10.0)]);
    assert_eq!((a.gross, a.cut, a.net, a.perimeter, a.outer_perimeter), (5000.0, 200.0, 4800.0, 390.0, 300.0));
    let bow = [P::new(0.0, 0.0), P::new(10.0, 10.0), P::new(10.0, 0.0), P::new(0.0, 10.0)];
    assert!(geom::outline_problem(&bow, "area").unwrap().contains("crosses itself"));
    assert!(geom::outline_problem(&[P::new(0.0, 0.0), P::new(5.0, 0.0), P::new(10.0, 0.0)], "area").unwrap().contains("no area"));
    assert!(geom::outline_problem(&r(0.0, 0.0, 1.0, 1.0), "area").is_none());
    let o = r(0.0, 0.0, 100.0, 50.0);
    assert!(geom::hole_problem(&o, &[], &r(90.0, 40.0, 20.0, 20.0)).unwrap().contains("inside"));
    assert!(geom::hole_problem(&o, &[], &r(200.0, 200.0, 5.0, 5.0)).unwrap().contains("inside"));
    assert!(geom::hole_problem(&o, &[r(10.0, 10.0, 20.0, 20.0)], &r(20.0, 20.0, 20.0, 20.0)).unwrap().contains("overlap"));
    assert!(geom::hole_problem(&o, &[r(10.0, 10.0, 30.0, 30.0)], &r(15.0, 15.0, 5.0, 5.0)).unwrap().contains("overlap"));
    assert!(geom::hole_problem(&o, &[r(10.0, 10.0, 20.0, 20.0)], &r(50.0, 10.0, 20.0, 20.0)).is_none());
    let s = geom::snap45(&P::new(0.0, 0.0), &P::new(100.0, 3.0));
    assert!(s.y == 0.0 && near(s.x, 100.0, 1e-9));
    let s2 = geom::snap45(&P::new(0.0, 0.0), &P::new(50.0, 47.0));
    assert!(near(s2.x, s2.y, 1e-9) && near(s2.x.hypot(s2.y), 97.0 / 2f64.sqrt(), 1e-9));
    let s3 = geom::snap45(&P::new(10.0, 10.0), &P::new(11.0, -80.0));
    assert!(s3.x == 10.0 && near(s3.y, -80.0, 1e-9));
    assert_eq!(geom::polyline_length(&[P::new(0.0, 0.0), P::new(3.0, 4.0), P::new(3.0, 10.0)]), 11.0);
    assert!(near(geom::dist_to_segment(&P::new(5.0, 5.0), &P::new(0.0, 0.0), &P::new(10.0, 0.0)), 5.0, 1e-9));
}

#[test]
fn arcs_and_circles() {
    assert!(geom::bulge_through(&P::new(1.0, 0.0), &P::new(0.0, 1.0), &P::new(-1.0, 0.0)) > 0.9999999);
    assert!(near(geom::bulge_through(&P::new(1.0, 0.0), &P::new(0.0, -1.0), &P::new(-1.0, 0.0)), -1.0, 1e-9));
    assert_eq!(geom::bulge_through(&P::new(0.0, 0.0), &P::new(5.0, 0.0), &P::new(10.0, 0.0)), 0.0);
    let q = std::f64::consts::FRAC_1_SQRT_2;
    assert!(near(geom::bulge_through(&P::new(1.0, 0.0), &P::new(q, q), &P::new(0.0, 1.0)), (PI / 8.0).tan(), 1e-9));
    assert!(near(geom::bulge_through(&P::new(1.0, 0.0), &P::new(-1.0, 0.0), &P::new(0.0, -1.0)), (3.0 * PI / 8.0).tan(), 1e-9));
    let (rr, sl) = (2.0, 10.0);
    let slot = [P::new(0.0, 0.0), P::arc(sl, 0.0, 1.0), P::new(sl, 2.0 * rr), P::arc(0.0, 2.0 * rr, 1.0)];
    assert!(near(geom::polygon_area(&slot), 2.0 * rr * sl + PI * rr * rr, 1e-13));
    assert!(near(geom::perimeter(&slot), 2.0 * sl + 2.0 * PI * rr, 1e-13));
    let big = 20.0;
    let quarter = [P::new(0.0, 0.0), P::arc(big, 0.0, (PI / 8.0).tan()), P::new(0.0, big)];
    assert!(near(geom::polygon_area(&quarter), PI * big * big / 4.0, 1e-13));
    assert!(near(geom::perimeter(&quarter), 2.0 * big + PI * big / 2.0, 1e-13));
    let run = [P::new(0.0, 0.0), P::arc(10.0, 0.0, 1.0), P::new(10.0, 10.0), P::arc(0.0, 10.0, -(PI / 8.0).tan()), P::new(-4.0, 14.0)];
    assert!(near(geom::polyline_length(&run), 10.0 + 5.0 * PI + 10.0 + 2.0 * PI, 1e-13));
    let segs = geom::segment_lengths(&run);
    assert!(near(segs[1], 5.0 * PI, 1e-9) && near(segs[3], 2.0 * PI, 1e-9) && segs[0] == 10.0 && segs[2] == 10.0);
    let circle = |cx: f64, cy: f64, rad: f64| vec![P::arc(cx + rad, cy, 1.0), P::arc(cx - rad, cy, 1.0)];
    assert!(
        near(geom::polygon_area(&circle(5.0, 5.0, 3.0)), 9.0 * PI, 1e-13) && near(geom::perimeter(&circle(5.0, 5.0, 3.0)), 6.0 * PI, 1e-13)
    );
    assert!(geom::outline_problem(&circle(0.0, 0.0, 1.0), "area").is_none());
    assert!(geom::outline_problem(&[P::new(0.0, 0.0), P::new(1.0, 0.0)], "area").is_some());
    let sq = r(0.0, 0.0, 10.0, 10.0);
    let aw = geom::area_with_holes(&sq, &[circle(5.0, 5.0, 1.0)]);
    assert!(near(aw.net, 100.0 - PI, 1e-13) && near(aw.perimeter, 40.0 + 2.0 * PI, 1e-13));
    assert!(geom::hole_problem(&sq, &[], &circle(5.0, 5.0, 1.0)).is_none());
    assert!(geom::hole_problem(&sq, &[], &circle(9.5, 5.0, 1.0)).unwrap().contains("inside"));
    assert!(geom::self_intersects(&[P::new(0.0, 0.0), P::new(10.0, 0.0), P::arc(10.0, 4.0, -1.0), P::new(0.0, 4.0)]));
    assert!(!geom::self_intersects(&[P::new(0.0, 0.0), P::new(10.0, 0.0), P::arc(10.0, 10.0, -0.5), P::new(0.0, 10.0)]));
    assert!(geom::point_in_polygon(&P::new(11.5, 2.0), &slot) && !geom::point_in_polygon(&P::new(12.5, 2.0), &slot));
    let mut worst = 0.0f64;
    let mut prev = P::new(100.0, 0.0);
    for op in geom::path_ops(&circle(0.0, 0.0, 100.0), true) {
        if let Op::C(c1, c2, e) = op {
            let mut tt = 0.0;
            while tt <= 1.0 {
                let uu = 1.0 - tt;
                let x = uu * uu * uu * prev.x + 3.0 * uu * uu * tt * c1.x + 3.0 * uu * tt * tt * c2.x + tt * tt * tt * e.x;
                let y = uu * uu * uu * prev.y + 3.0 * uu * uu * tt * c1.y + 3.0 * uu * tt * tt * c2.y + tt * tt * tt * e.y;
                worst = worst.max((x.hypot(y) - 100.0).abs());
                tt += 0.05;
            }
            prev = e;
        }
    }
    assert!(worst < 0.03, "{worst}");
    assert!(geom::flatten(&circle(0.0, 0.0, 1.0), true).len() >= 72);
    let (th, rad) = (2.0 * PI / 3.0, 7.0);
    let p1 = P::new(rad, 0.0);
    let pm = P::new(rad * (th / 2.0).cos(), rad * (th / 2.0).sin());
    let p2 = P::new(rad * th.cos(), rad * th.sin());
    let fan = [P::new(0.0, 0.0), P::arc(p1.x, p1.y, geom::bulge_through(&p1, &pm, &p2)), p2];
    assert!(near(geom::polygon_area(&fan), rad * rad * th / 2.0, 1e-12));
}

fn c(s: &str, vars: &[(&str, f64)]) -> Result<f64, String> {
    let v: Vec<(String, f64)> = vars.iter().map(|(k, x)| ((*k).to_string(), *x)).collect();
    formula::compute(s, &v).map_err(|e| e.message)
}

#[test]
fn formula_parser() {
    let v = [("L", 10.0), ("H", 8.0), ("A", 80.0), ("N", 3.0), ("P", 44.0)];
    let cases: &[(&str, f64)] = &[
        ("2 + 3 * 4", 14.0),
        ("(2 + 3) * 4", 20.0),
        ("2 * 3 + 4", 10.0),
        ("10 - 4 - 3", 3.0),
        ("100 / 10 / 5", 2.0),
        ("10 / 4", 2.5),
        ("-2 * 3", -6.0),
        ("- (2 + 3)", -5.0),
        ("--4", 4.0),
        ("+4", 4.0),
        ("2 * -3", -6.0),
        ("10 - -2", 12.0),
        ("2 × 3 ÷ 4", 1.5),
        ("5 − 2", 3.0),
        ("L", 10.0),
        ("l * h", 80.0),
        ("L * H * 2 / 32", 5.0),
        ("ceil(L / 1.333) + 1", 9.0),
        ("ceil(L/1.333)+N", 11.0),
        ("ceil(0.1 * 3 / 0.3)", 1.0),
        ("floor(0.7 * 10)", 7.0),
        ("ceil(2)", 2.0),
        ("floor(-1.5)", -2.0),
        ("ceil(-1.5)", -1.0),
        ("round(2.5)", 3.0),
        ("round(-2.5)", -3.0),
        ("round(2.675, 2)", 2.68),
        ("round(1.005, 2)", 1.01),
        ("round(L / 3, 1)", 3.3),
        ("min(3, 1, 2)", 1.0),
        ("max(L, H, 12)", 12.0),
        ("max(L)", 10.0),
        ("abs(-7)", 7.0),
        ("((((L))))", 10.0),
        ("1e3", 1000.0),
        (".5 * L", 5.0),
        ("A / 32", 2.5),
        ("P * 1.1", 44.0 * 1.1),
        ("ceil((L * H) / 32 * 1.1)", 3.0),
    ];
    for (s, want) in cases {
        let got = c(s, &v);
        assert!(got.as_ref().is_ok_and(|g| near(*g, *want, 1e-12)), "{s} = {want}, got {got:?}");
    }
    let long150 = format!("{}1{}", "(".repeat(150), ")".repeat(150));
    let long501 = "L".repeat(501);
    let bad: Vec<(&str, &str)> = vec![
        ("", "Type a formula"),
        ("L +", "ends too soon"),
        ("(L + 2", ")\" is missing"),
        ("L + 2)", "without its"),
        ("2L", "operator missing"),
        ("L H", "operator missing"),
        ("X * 2", "not a measured value"),
        ("foo(L)", "not a function"),
        ("L / 0", "divides by zero"),
        ("L / (H - 8)", "divides by zero"),
        ("ceil()", "missing before"),
        ("ceil(1, 2)", "takes 1 value"),
        ("round(1, 2, 3)", "takes 1 or 2"),
        ("round(L, 1.5)", "whole digits"),
        ("L ^ 2", "not allowed"),
        ("L % 2", "not allowed"),
        ("L; 2", "not allowed"),
        ("\"a\"", "not allowed"),
        ("'a'", "not allowed"),
        ("`a`", "not allowed"),
        ("[1]", "not allowed"),
        ("{}", "not allowed"),
        ("L.constructor", "not allowed"),
        ("constructor", "not a measured value"),
        ("__proto__", "not a measured value"),
        ("toString(1)", "not a function"),
        ("constructor(1)", "not a function"),
        ("hasOwnProperty(1)", "not a function"),
        ("valueOf(1)", "not a function"),
        ("process", "not a measured value"),
        ("globalThis", "not a measured value"),
        ("this", "not a measured value"),
        ("eval(1)", "not a function"),
        ("Function(1)", "not a function"),
        ("alert(1)", "not a function"),
        ("require(1)", "not a function"),
        ("import(1)", "not a function"),
        ("new Date", "not a measured value"),
        ("min(1,)", "missing before"),
        ("L = 2", "not allowed"),
        ("1e999 * 2", "not give a number"),
        (&long150, "nested too deeply"),
        (&long501, "too long"),
    ];
    for (s, want) in bad {
        let got = c(s, &v);
        assert!(got.as_ref().is_err_and(|e| e.contains(want)), "refuses {s} ({want}): {got:?}");
    }
    assert!(formula::parse("A", &["L", "H", "N"]).is_err());
    assert!(formula::parse("H", &["A", "P", "H", "N"]).is_ok());
    assert_eq!(c("N", &[("N", 0.0)]), Ok(0.0));
    assert!(c("L", &[("L", f64::NAN)]).is_err());
}

#[test]
fn pitch_units() {
    for (txt, rise) in [
        ("6:12", 6.0),
        ("6/12", 6.0),
        ("6", 6.0),
        ("4:10", 4.8),
        ("6 in 12", 6.0),
        ("", 0.0),
        ("0", 0.0),
        ("45°", 12.0),
        ("26.565051177°", 6.0),
    ] {
        let r = u::parse_pitch(txt);
        assert!(r.as_ref().is_ok_and(|v| near(*v, rise, 1e-6)), "{txt}: {r:?}");
    }
    for (txt, err) in [("abc", "pitch.err.read"), ("90°", "pitch.err.angle"), ("200:12", "pitch.err.steep"), ("6:0", "pitch.err.read")] {
        assert_eq!(u::parse_pitch(txt).unwrap_err(), err);
    }
    assert_eq!(format!("{}|{}|{}", u::format_pitch(6.0), u::format_pitch(6.5), u::format_pitch(0.0)), "6:12|6.5:12|");
    assert!(near(u::slope_factor(12.0), 2f64.sqrt(), 1e-9) && near(u::slope_factor(6.0), 1.25f64.sqrt(), 1e-9));
    assert!(near(u::hip_factor(12.0), 1.5f64.sqrt(), 1e-9));
}

#[test]
fn js_number_text() {
    // halfway cases: the even digit (Number::toString), as V8 writes them
    assert_eq!(cc_takeoff::jv::num_str(f64::from(-8.365_067_f32)), "-8.365066528320312");
    assert_eq!(cc_takeoff::jv::num_str(f64::from(2.228_553_8_f32)), "2.2285537719726562");
    assert_eq!(cc_takeoff::jv::num_str(1e21), "1e+21");
    assert_eq!(cc_takeoff::jv::num_str(0.000001), "0.000001");
    assert_eq!(cc_takeoff::jv::num_str(1e-7), "1e-7");
}
