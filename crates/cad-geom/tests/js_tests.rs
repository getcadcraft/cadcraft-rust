//! The geometry checks of the CadCraft app's own Node test (getcadcraft.com/bin/test-cadcraft.mjs, 0.5: sections
//! 'Geometry', 'Area and perimeter', 'STRETCH', 'JOIN', 'BREAK and LENGTHEN', 'Revision clouds', 'Construction
//! lines', 'Fillet'), ported one for one. Exact parity with the JS is checked by forks/tools/gdiff.mjs; these keep the
//! behaviour pinned in `cargo test`.

use cc_cad::{Entity, Geo};
use cc_cad_geom::corners::{corner_target, fillet_pair};
use cc_cad_geom::edit::*;
use cc_cad_geom::geom::*;
use cc_cad_geom::pick::*;
use cc_cad_geom::tx::tx_entity;
use cc_geom::P2;
use serde_json::{Value, json};

fn e(v: Value) -> Entity {
    Entity::from_value(&v)
}
fn j(x: &Entity) -> Value {
    x.to_value()
}
fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6
}
fn f(v: &Value) -> f64 {
    v.as_f64().unwrap()
}
fn line(x1: f64, y1: f64, x2: f64, y2: f64) -> Entity {
    e(json!({ "type": "line", "layer": "0", "x1": x1, "y1": y1, "x2": x2, "y2": y2 }))
}
fn pt(x: f64, y: f64) -> P2 {
    P2::new(x, y)
}

#[test]
fn intersections_and_bulges() {
    let x = intersect(&Prim::l(0.0, 0.0, 10.0, 10.0), &Prim::l(0.0, 10.0, 10.0, 0.0), false, false);
    assert!(x.len() == 1 && near(x[0].x, 5.0) && near(x[0].y, 5.0), "line × line");
    assert_eq!(intersect(&Prim::l(-10.0, 0.0, 10.0, 0.0), &Prim::circle(0.0, 0.0, 5.0), false, false).len(), 2, "line × circle");
    assert_eq!(
        intersect(&Prim::l(-10.0, 0.0, 10.0, 0.0), &Prim::a(0.0, 0.0, 5.0, 0.0, PI / 2.0), false, false).len(),
        1,
        "line × quarter arc"
    );
    assert_eq!(intersect(&Prim::circle(0.0, 0.0, 5.0), &Prim::circle(8.0, 0.0, 5.0), false, false).len(), 2, "circle × circle");
    let a = bulge_arc(pt(0.0, 0.0), pt(10.0, 0.0), 1.0);
    assert!(near(a.r, 5.0) && near(a.cx, 5.0) && near(a.cy, 0.0), "bulge 1 → semicircle radius 5");
    let mid = prim_point(&a, 0.5);
    assert!(near(mid.x, 5.0) && near(mid.y, -5.0), "positive bulge from (0,0) to (10,0) passes below");
}

#[test]
fn trim_extend_offset_fillet() {
    let list = vec![line(0.0, 0.0, 30.0, 0.0), line(10.0, -5.0, 10.0, 5.0), line(20.0, -5.0, 20.0, 5.0)];
    let t = trim_entity(&list[0], &list, pt(15.0, 0.0), None).unwrap();
    assert!(t.len() == 2 && near(f(&j(&t[0])["x2"]), 10.0) && near(f(&j(&t[1])["x1"]), 20.0), "trim middle → two pieces");
    let t2 = trim_entity(&list[0], &list, pt(25.0, 0.0), None).unwrap();
    assert!(t2.len() == 1 && near(f(&j(&t2[0])["x1"]), 0.0) && near(f(&j(&t2[0])["x2"]), 20.0), "trim end → one piece");
    let l2 = vec![line(0.0, 0.0, 30.0, 0.0), line(0.0, 5.0, 30.0, 5.0)];
    assert!(trim_entity(&l2[0], &l2, pt(15.0, 0.0), None).is_none(), "trim with nothing crossing → None");
    let cl = vec![e(json!({ "type": "circle", "layer": "0", "cx": 0, "cy": 0, "r": 5 })), line(-10.0, 0.0, 10.0, 0.0)];
    let tc = trim_entity(&cl[0], &cl, pt(0.0, 5.0), None).unwrap();
    let p0 = prims(&tc[0], None)[0];
    assert!(
        tc.len() == 1 && tc[0].typ() == "arc" && p0.a1 - p0.a0 > 3.0 && near(prim_point(&p0, 0.5).y, -5.0),
        "trim top of a circle → lower arc"
    );
    let sq = e(
        json!({ "type": "poly", "layer": "0", "closed": true, "pts": [{ "x": 0, "y": 0, "b": 0 }, { "x": 10, "y": 0, "b": 0 }, { "x": 10, "y": 10, "b": 0 }, { "x": 0, "y": 10, "b": 0 }] }),
    );
    let sl = vec![sq.clone(), line(5.0, -5.0, 5.0, 15.0)];
    let tp = trim_entity(&sl[0], &sl, pt(10.0, 5.0), None).unwrap();
    assert!(
        tp.len() == 1 && j(&tp[0])["closed"] == json!(false) && j(&tp[0])["pts"].as_array().unwrap().len() == 4,
        "closed square → open U"
    );

    let el = vec![line(0.0, 0.0, 5.0, 0.0), line(10.0, -5.0, 10.0, 5.0)];
    let ext = extend_entity(&el[0], &el, pt(5.0, 0.0), None, false).unwrap();
    assert!(near(f(&j(&ext)["x2"]), 10.0) && near(f(&j(&ext)["x1"]), 0.0), "extend line to boundary");
    let el2 = vec![line(0.0, 0.0, 5.0, 0.0), line(10.0, 5.0, 10.0, 15.0)];
    assert!(extend_entity(&el2[0], &el2, pt(5.0, 0.0), None, false).is_none(), "no boundary ahead → None");
    let al = vec![
        e(json!({ "type": "arc", "layer": "0", "cx": 0, "cy": 0, "r": 5, "a0": 0, "a1": PI / 4.0 })),
        line(-10.0, 0.0001, -1.0, 0.0001),
        line(0.0, 0.0, 0.0, 10.0),
    ];
    let ae = extend_entity(&al[0], &al, pt(3.5, 3.5), None, false).unwrap();
    assert!(near(f(&j(&ae)["a1"]), PI / 2.0), "extend arc end to a vertical line");

    let off = j(&offset_entity(&sq, 1.0, pt(5.0, 5.0)).unwrap());
    assert!(near(f(&off["pts"][0]["x"]), 1.0) && near(f(&off["pts"][2]["y"]), 9.0), "offset closed square inward");
    let out = j(&offset_entity(&sq, 1.0, pt(-3.0, 5.0)).unwrap());
    assert!(near(f(&out["pts"][0]["x"]), -1.0) && near(f(&out["pts"][2]["y"]), 11.0), "offset closed square outward");
    let c = e(json!({ "type": "circle", "layer": "0", "cx": 0, "cy": 0, "r": 5 }));
    assert!(
        near(f(&j(&offset_entity(&c, 2.0, pt(10.0, 0.0)).unwrap())["r"]), 7.0)
            && near(f(&j(&offset_entity(&c, 2.0, pt(1.0, 0.0)).unwrap())["r"]), 3.0)
    );
    let ol = j(&offset_entity(&line(0.0, 0.0, 10.0, 0.0), 2.0, pt(5.0, -9.0)).unwrap());
    assert!(near(f(&ol["y1"]), -2.0) && near(f(&ol["y2"]), -2.0), "offset line to the picked side");

    let fl = fillet_lines(&line(0.0, 0.0, 10.0, 0.0), &line(0.0, 0.0, 0.0, 10.0), 2.0, pt(8.0, 0.0), pt(0.0, 8.0)).unwrap().to_value();
    assert!(
        near(f(&fl["a"]["x1"]), 2.0) && near(f(&fl["b"]["y1"]), 2.0) && near(f(&fl["arc"]["r"]), 2.0) && near(f(&fl["arc"]["cx"]), 2.0),
        "fillet r=2"
    );
    let f0 = fillet_lines(&line(1.0, 0.0, 10.0, 0.0), &line(0.0, 1.0, 0.0, 10.0), 0.0, pt(8.0, 0.0), pt(0.0, 8.0)).unwrap();
    assert!(f0.arc.is_none() && near(f(&j(&f0.a)["x1"]), 0.0) && near(f(&j(&f0.b)["y1"]), 0.0), "fillet r=0 closes a corner");
}

#[test]
fn mirror_insert_explode_snaps_window() {
    let m = m_mirror(pt(0.0, 0.0), pt(0.0, 1.0));
    let ma = tx_entity(&e(json!({ "type": "arc", "layer": "0", "cx": 5, "cy": 0, "r": 1, "a0": 0, "a1": PI / 2.0 })), &m);
    let mp = prims(&ma, None)[0];
    let mid = prim_point(&mp, 0.5);
    assert!(
        near(f(&j(&ma)["cx"]), -5.0) && near(mid.x, -5.0 - std::f64::consts::FRAC_1_SQRT_2) && near(mid.y, std::f64::consts::FRAC_1_SQRT_2),
        "mirror arc"
    );
    let mt = j(&tx_entity(
        &e(json!({ "type": "text", "layer": "0", "x": 5, "y": 0, "h": 1, "rot": 0, "s": "A", "ha": "left", "va": "baseline" })),
        &m,
    ));
    assert!(near(f(&mt["x"]), -5.0) && near(f(&mt["rot"]).cos(), 1.0), "mirrored text stays readable");
    let blocks: Blocks = [("B".to_string(), cc_cad::Block::new("B", pt(0.0, 0.0), vec![line(0.0, 0.0, 1.0, 0.0)]))].into_iter().collect();
    let ins = e(json!({ "type": "insert", "layer": "0", "block": "B", "x": 10, "y": 10, "sx": 2, "sy": 2, "rot": PI / 2.0 }));
    let ip = prims(&ins, Some(&blocks))[0];
    assert!(near(ip.x2, 10.0) && near(ip.y2, 12.0), "insert transform");
    let ex = cc_cad_geom::corners::explode_entity(&ins, Some(&blocks)).unwrap();
    assert!(ex.len() == 1 && near(f(&j(&ex[0])["x2"]), 10.0) && near(f(&j(&ex[0])["y2"]), 12.0), "explode insert");
    let snaps = snap_points(&line(0.0, 0.0, 10.0, 0.0), pt(4.9, 0.1), 0.5, &["end", "mid"], None, None);
    assert!(snaps.len() == 1 && snaps[0].kind == "mid", "midpoint snap");
    let w = Bounds { min_x: 0.0, min_y: 0.0, max_x: 3.0, max_y: 3.0 };
    assert!(
        in_window(&line(1.0, 1.0, 2.0, 2.0), &w, false, None)
            && !in_window(&line(1.0, 1.0, 5.0, 2.0), &w, false, None)
            && in_window(&line(1.0, 1.0, 5.0, 2.0), &w, true, None)
    );
}

#[test]
fn area_and_perimeter() {
    let a = |v: Value| entity_area(&e(v));
    let c = a(json!({ "type": "circle", "cx": 0, "cy": 0, "r": 2 })).unwrap();
    assert!(near(c.0, 4.0 * PI) && c.2 == "circle");
    let sq = json!({ "type": "poly", "closed": true, "pts": [{ "x": 0, "y": 0 }, { "x": 10, "y": 0 }, { "x": 10, "y": 10 }, { "x": 0, "y": 10 }] });
    let s = a(sq).unwrap();
    assert!(near(s.0, 100.0) && near(s.1, 40.0));
    let bul = a(json!({ "type": "poly", "closed": true, "pts": [{ "x": 0, "y": 0, "b": 1 }, { "x": 10, "y": 0 }, { "x": 10, "y": 10 }, { "x": 0, "y": 10 }] })).unwrap();
    assert!(near(bul.0, 100.0 + PI * 25.0 / 2.0) && near(bul.1, 30.0 + PI * 5.0), "a bulge outward adds its half circle");
    let bin = a(json!({ "type": "poly", "closed": true, "pts": [{ "x": 0, "y": 0, "b": -1 }, { "x": 10, "y": 0 }, { "x": 10, "y": 10 }, { "x": 0, "y": 10 }] })).unwrap();
    assert!(near(bin.0, 100.0 - PI * 25.0 / 2.0));
    let open =
        a(json!({ "type": "poly", "closed": false, "pts": [{ "x": 0, "y": 0 }, { "x": 10, "y": 0 }, { "x": 10, "y": 10 }] })).unwrap();
    assert!(near(open.0, 50.0) && near(open.1, 20.0) && open.2 == "open");
    assert!(near(
        a(json!({ "type": "ellipse", "cx": 0, "cy": 0, "mx": 4, "my": 0, "ratio": 0.5, "t0": 0, "t1": TAU })).unwrap().0,
        PI * 8.0
    ));
    assert!(
        near(a(json!({ "type": "hatch", "loops": [[0, 0, 10, 0, 10, 10, 0, 10], [2, 2, 4, 2, 4, 4, 2, 4]] })).unwrap().0, 96.0),
        "hatch with a hole"
    );
    assert!(a(json!({ "type": "line", "x1": 0, "y1": 0, "x2": 1, "y2": 0 })).is_none());
}

#[test]
fn stretch() {
    let bx = |x0: f64, y0: f64, x1: f64, y1: f64| move |q: P2| q.x >= x0 && q.x <= x1 && q.y >= y0 && q.y <= y1;
    let ln =
        j(&stretch_entity(&e(json!({ "type": "line", "x1": 0, "y1": 0, "x2": 10, "y2": 0 })), &bx(8.0, -1.0, 12.0, 1.0), 5.0, 0.0)
            .unwrap());
    assert!(near(f(&ln["x1"]), 0.0) && near(f(&ln["x2"]), 15.0));
    assert!(
        stretch_entity(&e(json!({ "type": "line", "x1": 0, "y1": 0, "x2": 10, "y2": 0 })), &bx(20.0, 20.0, 30.0, 30.0), 5.0, 0.0).is_none()
    );
    let sp = j(&stretch_entity(
        &e(json!({ "type": "poly", "closed": false, "pts": [{ "x": 0, "y": 0, "b": 0.5 }, { "x": 10, "y": 0 }] })),
        &bx(9.0, -1.0, 11.0, 1.0),
        10.0,
        0.0,
    )
    .unwrap());
    let sag = |b: f64, c: f64| b * c / 2.0;
    assert!(near(sag(f(&sp["pts"][0]["b"]), 20.0), sag(0.5, 10.0)), "an arc segment keeps its height");
    let dm = stretch_entity(
        &e(json!({ "type": "dim", "kind": "linear", "p1": { "x": 0, "y": 0 }, "p2": { "x": 10, "y": 0 }, "loc": { "x": 5, "y": 3 }, "rot": 0, "block": "*D1" })),
        &bx(9.0, -1.0, 11.0, 1.0),
        5.0,
        0.0,
    )
    .unwrap();
    assert!(j(&dm).get("block").is_none() && cc_cad_geom::dims::dim_measure(&dm) == 15.0, "a dimension re-measures (15)");
}

#[test]
fn join() {
    let ln = |x1: f64, y1: f64, x2: f64, y2: f64, id: u64| {
        e(json!({ "id": id, "type": "line", "layer": "0", "x1": x1, "y1": y1, "x2": x2, "y2": y2 }))
    };
    let (r1, _) = join_entities(&[ln(0.0, 0.0, 10.0, 0.0, 1), ln(10.0, 10.0, 10.0, 0.0, 2), ln(10.0, 10.0, 0.0, 10.0, 3)], None);
    let p1 = j(&r1[0].to);
    assert!(p1["type"] == "poly" && p1["closed"] == json!(false) && p1["pts"].as_array().unwrap().len() == 4 && r1[0].from.len() == 3);
    let (r2, _) = join_entities(
        &[ln(0.0, 0.0, 10.0, 0.0, 1), ln(10.0, 0.0, 10.0, 10.0, 2), ln(10.0, 10.0, 0.0, 10.0, 3), ln(0.0, 10.0, 0.0, 0.0, 4)],
        None,
    );
    assert!(j(&r2[0].to)["closed"] == json!(true) && j(&r2[0].to)["pts"].as_array().unwrap().len() == 4);
    let (r4, _) = join_entities(&[ln(0.0, 0.0, 4.0, 0.0, 1), ln(6.0, 0.0, 10.0, 0.0, 2)], None);
    assert!(r4[0].to.typ() == "line" && near(f(&j(&r4[0].to)["x2"]), 10.0));
    let arc = |a0: f64, a1: f64, id: u64| e(json!({ "id": id, "type": "arc", "layer": "0", "cx": 0, "cy": 0, "r": 5, "a0": a0, "a1": a1 }));
    let (r5, _) = join_entities(&[arc(0.0, 1.0, 1), arc(1.2, 2.0, 2)], None);
    assert!(r5[0].to.typ() == "arc" && near(f(&j(&r5[0].to)["a0"]), 0.0) && near(f(&j(&r5[0].to)["a1"]), 2.0));
    let (r6, _) = join_entities(&[arc(0.0, PI, 1), arc(PI, 0.0, 2)], None);
    assert_eq!(r6[0].to.typ(), "circle");
    let (r7, left) = join_entities(&[ln(0.0, 0.0, 1.0, 0.0, 1), ln(1.0, 0.0, 1.0, 1.0, 2), ln(5.0, 5.0, 6.0, 7.0, 3)], None);
    assert!(r7.len() == 1 && left.contains(&Some(3)));
}

#[test]
fn break_and_lengthen() {
    let l = e(json!({ "type": "line", "x1": 0, "y1": 0, "x2": 10, "y2": 0 }));
    let b1 = break_entity(&l, pt(3.0, 0.0), pt(7.0, 0.1)).unwrap();
    assert!(b1.len() == 2 && near(f(&j(&b1[0])["x2"]), 3.0) && near(f(&j(&b1[1])["x1"]), 7.0));
    let b3 = break_entity(&e(json!({ "type": "circle", "cx": 0, "cy": 0, "r": 5 })), pt(5.0, 0.0), pt(0.0, 5.0)).unwrap();
    assert!(b3[0].typ() == "arc" && near(f(&j(&b3[0])["a0"]), PI / 2.0) && near(f(&j(&b3[0])["a1"]), 0.0));
    assert_eq!(break_entity(&e(json!({ "type": "circle", "cx": 0, "cy": 0, "r": 5 })), pt(5.0, 0.0), pt(5.0, 0.0)), Err("circle"));
    let sq = e(
        json!({ "type": "poly", "closed": true, "pts": [{ "x": 0, "y": 0 }, { "x": 10, "y": 0 }, { "x": 10, "y": 10 }, { "x": 0, "y": 10 }] }),
    );
    let b4 = j(&break_entity(&sq, pt(5.0, 0.0), pt(10.0, 5.0)).unwrap()[0]);
    let pts = b4["pts"].as_array().unwrap();
    assert!(pts.len() == 5 && near(f(&pts[0]["x"]), 10.0) && near(f(&pts[0]["y"]), 5.0) && near(f(&pts[4]["x"]), 5.0));
    assert!(near(f(&j(&lengthen_entity(&l, pt(9.0, 0.0), 5.0, None).unwrap())["x2"]), 15.0));
    assert_eq!(lengthen_entity(&l, pt(9.0, 0.0), -10.0, None), Err("short"));
    let a = e(json!({ "type": "arc", "cx": 0, "cy": 0, "r": 10, "a0": 0, "a1": PI / 2.0 }));
    assert!(near(f(&j(&lengthen_entity(&a, pt(0.0, 10.0), 0.0, Some(PI / 2.0)).unwrap())["a1"]), PI));
    assert_eq!(lengthen_entity(&a, pt(10.0, 0.0), 0.0, Some(TAU)), Err("full"));
    let pa = e(json!({ "type": "poly", "closed": false, "pts": [{ "x": 0, "y": 0 }, { "x": 10, "y": 0, "b": 1 }, { "x": 10, "y": 10 }] }));
    let lpa = lengthen_entity(&pa, pt(10.0, 10.0), PI * 5.0 / 2.0, None).unwrap();
    assert!(near(curve_length(&lpa).unwrap(), curve_length(&pa).unwrap() + PI * 5.0 / 2.0));
}

#[test]
fn revision_clouds() {
    let sq = [pt(0.0, 0.0), pt(40.0, 0.0), pt(40.0, 20.0), pt(0.0, 20.0)];
    let rs = resample_path(&sq, true, 10.0);
    assert!(rs.len() == 12 && near(rs[4].x, 40.0) && near(rs[6].y, 20.0));
    let cl = revcloud(&sq, true, 10.0, false);
    assert!(entity_area(&cl).unwrap().0 > 800.0);
    assert!(entity_area(&revcloud(&sq, true, 10.0, true)).unwrap().0 < 800.0);
    let mut rev = sq.to_vec();
    rev.reverse();
    assert!(entity_area(&revcloud(&rev, true, 10.0, false)).unwrap().0 > 800.0);
    let (p, closed) = path_of(&e(json!({ "type": "circle", "cx": 0, "cy": 0, "r": 10 }))).unwrap();
    assert!(closed && p.len() == 96);
}

#[test]
fn construction_lines() {
    let xl = e(json!({ "type": "xline", "layer": "0", "x": 0, "y": 0, "dx": 1, "dy": 0 }));
    let ry = e(json!({ "type": "ray", "layer": "0", "x": 0, "y": 0, "dx": 0, "dy": 1 }));
    assert!(near(dist_to_entity(&xl, pt(5e6, 3.0), None), 3.0) && near(dist_to_entity(&ry, pt(0.0, -4.0), None), 4.0));
    let w = Bounds { min_x: -10.0, min_y: -10.0, max_x: 10.0, max_y: 10.0 };
    assert!(!in_window(&xl, &w, false, None) && in_window(&xl, &w, true, None));
    let hit = intersect(
        &prims(
            &e(json!({ "type": "xline", "x": 3, "y": 4, "dx": std::f64::consts::FRAC_1_SQRT_2, "dy": std::f64::consts::FRAC_1_SQRT_2 })),
            None,
        )[0],
        &prims(&line(100.0, -50.0, 100.0, 500.0), None)[0],
        false,
        false,
    );
    assert!(hit.len() == 1 && near(hit[0].x, 100.0) && near(hit[0].y, 101.0));
    let bx = Bounds { min_x: 0.0, min_y: 0.0, max_x: 10.0, max_y: 10.0 };
    assert_eq!(clip_infinite(pt(5.0, 5.0), pt(1.0, 0.0), &bx, false), Some((-5.0, 5.0)));
    assert_eq!(clip_infinite(pt(20.0, 5.0), pt(1.0, 0.0), &bx, true), None);
    let cuts = vec![xl.clone(), line(10.0, -5.0, 10.0, 5.0), line(20.0, -5.0, 20.0, 5.0)];
    let sp = trim_split(&cuts[0], &cuts, None, false).unwrap();
    assert_eq!(sp.pieces.len(), 3);
    let kept = trim_result(&sp, &[piece_at(&sp, pt(15.0, 0.0))]).unwrap();
    assert!(kept.len() == 2 && kept.iter().all(|k| k.typ() == "ray"));
    let b1 = break_entity(&xl, pt(4.0, 0.0), pt(4.0, 0.0)).unwrap();
    assert!(b1.len() == 2 && b1.iter().all(|k| k.typ() == "ray" && near(f(&j(k)["x"]), 4.0)));
    let off = j(&offset_entity(&xl, 5.0, pt(99.0, -1.0)).unwrap());
    assert!(near(f(&off["y"]), -5.0) && near(f(&off["dx"]), 1.0));
    let rot = j(&tx_entity(&xl, &m_about(pt(0.0, 0.0), &m_rotate(PI / 2.0))));
    assert!(near(f(&rot["dx"]), 0.0) && near(f(&rot["dy"]), 1.0));
}

#[test]
fn fillet_two_circles() {
    let ca = e(json!({ "type": "circle", "cx": 0, "cy": 0, "r": 2 }));
    let cb = e(json!({ "type": "circle", "cx": 8, "cy": 0, "r": 2 }));
    let ta = corner_target(&ca, pt(2.0, 0.5));
    let tb = corner_target(&cb, pt(6.0, 0.5));
    let r = fillet_pair(ta.as_ref(), tb.as_ref(), 3.0, true, None);
    assert!(r.error.is_none());
    let arc = &r.add[0];
    let Geo::Arc(a) = &arc.geo else { panic!("an arc") };
    assert!(near(a.cx.hypot(a.cy), 5.0) && near((a.cx - 8.0).hypot(a.cy), 5.0) && a.cy > 0.0);
}

#[test]
fn hostile_inputs_do_not_panic() {
    // malformed objects, absurd sizes, recursive blocks: empty results, never a panic
    for v in [
        json!({ "type": "poly", "pts": [] }),
        json!({ "type": "poly", "closed": true, "pts": [{ "x": 1, "y": 1 }] }),
        json!({ "type": "dim", "kind": "linear" }),
        json!({ "type": "mleader", "pts": [] }),
        json!({ "type": "arc", "cx": 0, "cy": 0, "r": 1, "a0": 0, "a1": 1e300 }),
        json!({ "type": "ellipse", "cx": 0, "cy": 0, "mx": 1, "my": 0, "ratio": 1, "t0": 0, "t1": 1e300 }),
        json!({ "type": "hatch", "loops": [[0, 0, 1e300, 0, 0, 1e-300]], "pattern": "ANSI31", "pscale": 1e-300 }),
        json!({ "type": "text", "x": 0, "y": 0, "h": 1, "s": "x", "ww": 1e-300 }),
        json!({ "type": "insert", "block": "R", "x": 0, "y": 0 }),
    ] {
        let x = e(v);
        let blocks: Blocks =
            [("R".to_string(), cc_cad::Block::new("R", pt(0.0, 0.0), vec![e(json!({ "type": "insert", "block": "R", "x": 1, "y": 0 }))]))]
                .into_iter()
                .collect();
        let _ = prims(&x, Some(&blocks));
        let _ = entity_bounds(&x, Some(&blocks));
        let _ = tx_entity(&x, &[1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        let _ = cc_cad_geom::tx::grips(&x);
        let _ = offset_entity(&x, 1.0, pt(1.0, 1.0));
        let _ = cc_cad_geom::corners::explode_entity(&x, Some(&blocks));
        let _ = entity_area(&x);
        let _ = path_of(&x);
        let _ = sub_entity(&x, -5.0, 1e12);
        let _ = cc_cad_geom::dims::dim_geometry(&x);
        let mut d = cc_cad::Doc::new();
        d.load(&json!({ "entities": [x.to_value()], "blocks": [{ "name": "R", "entities": [{ "type": "insert", "block": "R", "x": 1, "y": 0 }] }] }));
        let mut rec = cc_cad_geom::walk::Recorder::default();
        let list = d.all();
        cc_cad_geom::walk::walk(&d, &list, &mut rec, &cc_cad_geom::walk::WalkOpts::default());
    }
}
