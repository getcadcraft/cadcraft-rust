//! Ported from the frozen Node test (bin/test-takeoffcraft.mjs at 594ab3e): "Scales from the sheets' text (T20)" (the
//! text half; reading a generated set's text goes through pdf.js in the JS and is covered by io-takeoff's tests and
//! forks/tools/todiff.mjs) and "Room fill (T21)".

use cc_takeoff::jv::Obj;
use cc_takeoff::matcher::Img;
use cc_takeoff::model::{self, Takeoff};
use cc_takeoff::roomfill as rf;
use cc_takeoff::scaletext as st;
use cc_takeoff::units as u;
use serde_json::{Value, json};

fn o(v: Value) -> Obj {
    v.as_object().cloned().unwrap_or_default()
}

#[test]
fn scales_from_text() {
    let one = |s: &str| st::sheet_scale(s).preset;
    let got: Vec<&str> = [
        "FLOOR PLAN  SCALE: 1/4\" = 1'-0\"",
        "1/8\"=1'",
        "3/16 \" = 1 ' - 0 \"",
        "DETAIL 1 1/2\" = 1'-0\"",
        "1-1/2\"=1'-0\"",
        "SITE PLAN 1\" = 20'",
        "SCALE 1:100",
        "ratio 1:1000.",
        "3/4 in = 1 ft",
    ]
    .iter()
    .map(|s| one(s).unwrap_or("-"))
    .collect();
    assert_eq!(got.join(","), "a1/4,a1/8,a3/16,a1-1/2,a1-1/2,e20,m100,m1000,a3/4");
    for s in ["Time 1:30 pm", "12\" = 1'-0\"", "1:2.5", "2 1/4\" = 1'-0\""] {
        assert!(one(s).is_none(), "{s}");
    }
    let nts = st::sheet_scale("DETAIL A — N.T.S.");
    assert!(nts.preset.is_none() && nts.nts && nts.why == "nts");
    let sev = st::sheet_scale("ENLARGED 1-1/2\" = 1'-0\"  PLAN 1/4\" = 1'-0\"  SCALE: 1/4\" = 1'-0\"");
    assert!(sev.preset == Some("a1/4") && sev.why == "several" && sev.others == vec!["a1-1/2"]);
    // the text a generated set gives pdf.js (one item per drawn line, joined by the app with spaces)
    let pages =
        [vec!["FIRST FLOOR PLAN", "SCALE: 1/4\" = 1'-0\""], vec!["SITE PLAN", "SCALE 1:100"], vec!["GENERAL NOTES"], vec!["NOT TO SCALE"]];
    let res: Vec<String> = pages.iter().map(|l| st::sheet_scale(&l.join(" "))).map(|r| r.preset.unwrap_or(r.why).to_string()).collect();
    assert_eq!(res.join(","), "a1/4,m100,none,nts");
    let mut t = Takeoff::default();
    let h = "7".repeat(64);
    t.add_file(&o(json!({ "hash": h, "name": "x.pdf", "size": 1, "pages": 2 })));
    let (k0, k1) = (model::sheet_key(&h, 0.0), model::sheet_key(&h, 1.0));
    let n0 = t.undo_stack.len();
    let mut map = Obj::new();
    map.insert(k0.clone(), json!({ "method": "preset", "preset": "a1/4", "ftPerUnit": u::preset_ft_per_unit("a1/4", 1.0).unwrap(), "label": "1/4\"", "cal": null }));
    map.insert(k1.clone(), json!({ "method": "preset", "preset": "m100", "ftPerUnit": u::preset_ft_per_unit("m100", 1.0).unwrap(), "label": "1:100", "cal": null }));
    t.set_scales(&map, None).unwrap();
    assert!(t.scale_of(&k0).unwrap()["preset"] == "a1/4" && t.scale_of(&k1).unwrap()["preset"] == "m100" && t.undo_stack.len() == n0 + 1);
    t.undo();
    assert!(t.scale_of(&k0).is_none() && t.scale_of(&k1).is_none());
    // hostile text never hangs or panics (the regex engine backtracks with a budget)
    let long = "1/".repeat(20_000) + "\" = 1'-0\"" + &"(".repeat(5000);
    let _ = st::sheet_scale(&long);
}

#[test]
fn room_fill() {
    let (w0, h0) = (37usize, 23usize);
    let src: Vec<u8> = (0..w0 * h0).map(|i| u8::from(((i as u64).wrapping_mul(2_654_435_761) % (1u64 << 32)).is_multiple_of(97))).collect();
    for r in [1usize, 3, 7] {
        let fast = rf::dilate_bin(&src, w0, h0, r);
        for y in 0..h0 {
            for x in 0..w0 {
                let mut v = 0u8;
                for yy in y.saturating_sub(r)..=(y + r).min(h0 - 1) {
                    for xx in x.saturating_sub(r)..=(x + r).min(w0 - 1) {
                        if src[yy * w0 + xx] != 0 {
                            v = 1;
                        }
                    }
                }
                assert_eq!(v, fast[y * w0 + x], "r {r} at {x},{y}");
            }
        }
    }
    let mut l = vec![0u8; 30];
    for (x, y) in [(1, 1), (2, 1), (3, 1), (1, 2), (1, 3), (2, 3), (2, 2)] {
        l[y * 6 + x] = 1;
    }
    let ring = |ps: &[cc_takeoff::geom::P]| ps.iter().map(|p| format!("{},{}", p.x, p.y)).collect::<Vec<_>>().join(" ");
    assert_eq!(ring(&rf::trace(&l, 6, 5)), "1,1 4,1 4,2 3,2 3,4 1,4");
    let (w, h) = (400usize, 300usize);
    let mut data = vec![0f32; w * h];
    let mut wall = |x0: usize, y0: usize, x1: usize, y1: usize| {
        for y in y0.min(y1) - 1..=y0.max(y1) + 1 {
            for x in x0.min(x1) - 1..=x0.max(x1) + 1 {
                data[y * w + x] = 1.0;
            }
        }
    };
    wall(20, 20, 380, 20);
    wall(380, 20, 380, 280);
    wall(20, 280, 380, 280);
    wall(20, 20, 20, 280);
    wall(200, 20, 200, 120);
    wall(200, 150, 200, 280);
    for i in 0..6 {
        data[205 * w + 60 + i * 3] = 1.0;
    }
    let mut ink = Img { data, w, h };
    // the JS defaults: on 0.25, maxShare 0.6
    let a = rf::room_at(&ink, 100.0, 100.0, 40.0, 0.25, 0.6).unwrap();
    assert_eq!(ring(&a.0), "22,22 199,22 199,279 22,279");
    let b = rf::room_at(&ink, 300.0, 200.0, 40.0, 0.25, 0.6).unwrap();
    assert_eq!(ring(&b.0), "202,22 379,22 379,279 202,279");
    assert_eq!(rf::room_at(&ink, 200.0, 60.0, 40.0, 0.25, 0.6).unwrap_err(), "wall");
    let both = rf::room_at(&ink, 100.0, 100.0, 20.0, 0.25, 0.95).unwrap();
    assert_eq!(ring(&both.0), "22,22 199,22 199,122 202,122 202,22 379,22 379,279 202,279 202,149 199,149 199,279 22,279");
    for x in 100..140 {
        for y in 279..=281 {
            ink.data[y * w + x] = 0.0;
        }
    }
    assert_eq!(rf::room_at(&ink, 100.0, 100.0, 20.0, 0.25, 0.6).unwrap_err(), "open");
    assert!(rf::room_at(&ink, 100.0, 100.0, 50.0, 0.25, 0.6).is_ok());
    // hostile inputs
    let _ = rf::room_at(&Img::default(), 0.0, 0.0, 10.0, 0.25, 0.6);
    let _ = rf::room_at(&ink, f64::NAN, -1e300, 1e9, 0.25, 0.6);
    let _ = rf::dilate_bin(&[], 0, 0, 5);
}
