//! The frozen bin/test-modelcraft.mjs's tool sections, ported: the same clicks through a camera, the same typed
//! values, the same checks. (The parity harness forks/tools/tdiff.mjs runs these sessions on the JS as well and
//! compares every step; these tests keep the behaviour checked inside the repo.)

mod common;

use cc_m3d::def::face_normal;
use cc_m3d::ops::{
    Sel, add_polygon, add_spline, connected, curve_edges, curve_info, flip_sel, mirror_matrix, ngon_pts, push_pull, rect_pts,
    set_curve_radius, smooth_pts,
};
use cc_m3d::vec::{IDENT, add, det3, dot, mul, norm, sub, translation};
use cc_m3d_tools::camera::Camera;
use cc_m3d_tools::infer::{Axes3, InferOpts, dim_layout, infer, pick, pick_section, section_layout};
use cc_m3d_tools::scene::{LocalCache, Scene};
use cc_m3d_tools::tools::{Ev, ToolArg};
use cc_m3d_tools::units::{FT, IN, format_area, format_length, format_volume, parse_length, parse_value};
use common::*;

fn scene_of(m: &cc_m3d::Model) -> Scene {
    Scene::of(m, &mut LocalCache::new())
}

#[test]
fn units_sun_camera() {
    let cases = [
        ("10'", "ft-in", 10.0 * FT),
        ("10'6\"", "ft-in", 10.0 * FT + 6.0 * IN),
        ("10'-6 1/2\"", "ft-in", 10.0 * FT + 6.5 * IN),
        ("6 1/2\"", "m", 6.5 * IN),
        ("3/4\"", "m", 0.75 * IN),
        ("18", "ft-in", 18.0 * IN),
        ("3.2m", "ft-in", 3.2),
        ("320cm", "m", 3.2),
        ("3200", "mm", 3.2),
        ("1.5ft", "m", 1.5 * FT),
        ("-2m", "m", -2.0),
    ];
    for (t, u, v) in cases {
        assert!(close(parse_length(t, u).unwrap(), v, 1e-9), "parseLength({t}, {u})");
    }
    assert!(parse_length("abc", "m").is_none() && parse_length("", "m").is_none(), "nonsense is refused");
    assert_eq!(format_length(10.0 * FT + 6.5 * IN, "ft-in"), "10' 6 1/2\"");
    assert_eq!(format_length(3.25, "m"), "3.25 m");
    assert_eq!(format_length(3.25, "mm"), "3250 mm");
    use cc_m3d_tools::units::Value;
    assert!(matches!(parse_value("4m,3m", "m"), Some(Value::List(v)) if c6(v[0], 4.0) && c6(v[1], 3.0)));
    assert_eq!(parse_value("24s", "m").unwrap().n(), Some(24.0));
    assert_eq!(parse_value("3x", "m").unwrap().kind(), "array");
    assert_eq!(parse_value("/4", "m").unwrap().n(), Some(4.0));
    assert_eq!(parse_value("[1,2,3]", "m").unwrap().kind(), "abs");
    assert!(matches!(parse_value("<1,0,0>", "m"), Some(Value::Rel(p)) if p[0] == 1.0));
    assert!(close(parse_length("18", "ft-in").unwrap(), 18.0 * IN, 1e-12) && close(parse_length("2400", "mm").unwrap(), 2.4, 1e-12));
    assert!(matches!(parse_value("4000,3000", "mm"), Some(Value::List(v)) if close(v[1], 3.0, 1e-12)));
    assert!(
        format_area(12.0, "mm") == "12 m²"
            && format_area(0.0016, "mm") == "1600 mm²"
            && format_volume(30.0, "mm") == "30 m³"
            && format_volume(64e-6, "mm") == "64000 mm³"
    );

    use cc_m3d_tools::sun::{When, sun_position};
    let highest = |month: f64| {
        let mut best = -99.0f64;
        let mut h = 10.0;
        while h <= 15.0 {
            best = best.max(sun_position(&When { month, day: 21.0, hour: h, lat: 40.0, lon: -105.0, tz: -7.0, year: 2026.0 }).altitude);
            h += 1.0 / 60.0;
        }
        best
    };
    assert!(close(highest(6.0), 90.0 - 40.0 + 23.44, 0.3), "June solstice");
    assert!(close(highest(12.0), 90.0 - 40.0 - 23.44, 0.3), "December solstice");
    let am = sun_position(&When { month: 3.0, day: 20.0, hour: 8.0, ..When::default() });
    let pm = sun_position(&When { month: 3.0, day: 20.0, hour: 16.0, ..When::default() });
    assert!(am.azimuth > 90.0 && am.azimuth < 135.0 && pm.azimuth > 225.0 && pm.azimuth < 270.0);
    assert!(am.dir[0] > 0.0 && pm.dir[0] < 0.0 && c6(cc_m3d::vec::len(am.dir), 1.0));
    assert!(!sun_position(&When { hour: 1.0, ..When::default() }).up, "no sun at 1 am");

    let mut cam = Camera::new();
    cam.set_size(800.0, 600.0);
    cam.eye = [10.0, -10.0, 10.0];
    cam.target = O;
    cam.clip(10.0);
    let s = cam.project(O).unwrap();
    assert!(c6(s[0], 400.0) && c6(s[1], 300.0), "the target projects to the centre");
    let ground = cc_m3d::vec::Plane { n: Z, d: 0.0 };
    let r = cam.ray(123.0, 456.0);
    let hit = cc_m3d::vec::ray_plane(r.o, r.d, &ground).unwrap().1;
    let back = cam.project(hit).unwrap();
    assert!(c6(back[0], 123.0) && c6(back[1], 456.0), "a pick ray and the projection agree (perspective)");
    cam.persp = false;
    let r2 = cam.ray(700.0, 50.0);
    let hit2 = cc_m3d::vec::ray_plane(r2.o, r2.d, &ground).unwrap().1;
    let back2 = cam.project(hit2).unwrap();
    assert!(c6(back2[0], 700.0) && c6(back2[1], 50.0), "and in parallel projection");
    cam.persp = true;
    let before = cam.ray(600.0, 200.0);
    let pp = cc_m3d::vec::ray_plane(before.o, before.d, &ground).unwrap().1;
    cam.zoom_at(0.5, Some(600.0), Some(200.0), Some(pp));
    let after = cam.project(pp).unwrap();
    assert!(
        c6(after[0], 600.0) && c6(after[1], 200.0) && close(cam.distance(), cc_m3d::vec::len([10.0, -10.0, 10.0]) / 2.0, 1e-9),
        "zoom at the cursor"
    );
    cam.fit(Some(&cc_m3d_tools::camera::Box3 { min: O, max: [4.0, 3.0, 2.0] }), None);
    for c in [
        [0.0, 0.0, 0.0],
        [4.0, 0.0, 0.0],
        [0.0, 3.0, 0.0],
        [4.0, 3.0, 0.0],
        [0.0, 0.0, 2.0],
        [4.0, 0.0, 2.0],
        [0.0, 3.0, 2.0],
        [4.0, 3.0, 2.0],
    ] {
        let p = cam.project(c).unwrap();
        assert!(p[0] > 0.0 && p[0] < 800.0 && p[1] > 0.0 && p[1] < 600.0, "zoom extents shows the whole box");
    }
}

#[test]
fn tools_driven_by_clicks() {
    let mut g = Rig::new();
    g.cam([6.0, -14.0, 12.0], [2.0, 2.0, 0.0], 20.0);
    g.set("rect");
    g.click(O);
    g.move_to([4.0, 3.0, 0.0]);
    g.value("4m,3m");
    let f0 = g.root().faces.first_key().unwrap();
    assert!(g.root().faces.len() == 1 && c6(area(g.root(), f0), 12.0), "rectangle tool: a 12 m² face");
    let sc = scene_of(g.model());
    let cam = g.t.host.cam.clone();
    let at = |p| cam.project(p).unwrap();
    let o = InferOpts::default();
    let s = infer(&sc, &cam, at([4.0, 0.0, 0.0])[0] + 4.0, at([4.0, 0.0, 0.0])[1] - 3.0, &o);
    assert!(s.kind == "end" && dist(s.p, [4.0, 0.0, 0.0]) < 1e-9, "inference: endpoint");
    let mp = at([2.0, 0.0, 0.0]);
    let s = infer(&sc, &cam, mp[0] + 2.0, mp[1] + 2.0, &o);
    assert!(s.kind == "mid" && dist(s.p, [2.0, 0.0, 0.0]) < 1e-9, "inference: midpoint");
    let ep = at([1.0, 3.0, 0.0]);
    let s = infer(&sc, &cam, ep[0], ep[1] + 1.0, &o);
    assert!(s.kind == "edge" && (s.p[1] - 3.0).abs() < 1e-6, "inference: on edge");
    let fp = at([2.5, 1.2, 0.0]);
    let s = infer(&sc, &cam, fp[0], fp[1], &o);
    assert!(s.kind == "face" && c6(s.p[2], 0.0) && s.face.is_some(), "inference: on face");
    let ax = Axes3 { x: R, y: G, z: Z };
    let a8 = at([8.0, 0.0, 0.0]);
    let s = infer(&sc, &cam, a8[0], a8[1] + 5.0, &InferOpts { from: Some([4.0, 0.0, 0.0]), axes: Some(ax), ..InferOpts::default() });
    assert!(s.kind == "axis-x" && c6(s.p[1], 0.0) && c6(s.p[2], 0.0), "inference: red axis");
    let up = at([4.0, 3.0, 2.0]);
    let s = infer(&sc, &cam, up[0] + 3.0, up[1], &InferOpts { from: Some([4.0, 3.0, 0.0]), axes: Some(ax), ..InferOpts::default() });
    assert!(s.kind == "axis-z" && c6(s.p[0], 4.0) && c6(s.p[1], 3.0), "inference: blue axis");
    let s =
        infer(&sc, &cam, up[0] + 40.0, up[1] + 30.0, &InferOpts { from: Some([4.0, 3.0, 0.0]), lock_dir: Some(Z), ..InferOpts::default() });
    assert!(s.kind == "lock" && c6(s.p[0], 4.0) && c6(s.p[1], 3.0), "a locked axis keeps the point on it");

    g.set("push");
    g.click([2.0, 1.5, 0.0]);
    g.value("2.5");
    assert!(closed(g.root()) && c6(volume(g.root()), 30.0), "push/pull tool: 30 m³ box");
    g.set("push");
    g.click([2.0, 1.5, 2.5]);
    g.move_to([2.0, 1.5, 3.5]);
    g.click([2.0, 1.5, 3.5]);
    assert!(closed(g.root()) && close(volume(g.root()), 42.0, 0.05), "push/pull by mouse");
    g.m().undo().unwrap();
    assert!(c6(volume(g.root()), 30.0), "one undo per push/pull");

    g.set("line");
    g.click([0.0, 1.0, 2.5]);
    g.click([4.0, 1.0, 2.5]);
    g.escape();
    assert_eq!(faces_facing(g.root(), Z).len(), 2, "a line across the top face splits it");
    g.set("select");
    g.click([2.0, 0.5, 2.5]);
    assert!(g.t.host.sel.len() == 1 && g.t.host.sel[0].t == "f", "select: click a face");
    g.click([2.0, 0.5, 2.5]);
    g.time -= 900.0;
    g.click([2.0, 0.5, 2.5]);
    assert!(g.t.host.sel.len() == 5 && g.t.host.sel.iter().filter(|x| x.t == "e").count() == 4, "double-click: the face and its 4 edges");
    g.time += 2000.0;
    g.click([2.0, 0.5, 2.5]);
    g.time -= 900.0;
    g.click([2.0, 0.5, 2.5]);
    g.time -= 900.0;
    g.click([2.0, 0.5, 2.5]);
    assert_eq!(g.t.host.sel.iter().filter(|x| x.t == "f").count(), g.root().faces.len(), "triple-click: everything connected");
    let (a, b) = (g.at([-1.0, -1.0, 0.0]), g.at([5.0, 4.0, 3.0]));
    g.set("select");
    g.move_xy(a.0.min(b.0) - 200.0, 5.0);
    g.down_xy(5.0, 5.0, 1e7);
    g.move_xy(995.0, 795.0);
    g.up_xy(995.0, 795.0);
    let soft = g.root().edges.values().filter(|e| e.soft).count();
    assert_eq!(g.t.host.sel.len(), g.root().faces.len() + g.root().edges.len() - soft, "window selection over everything");
    let c = g.at([2.0, 1.5, 2.5]);
    g.down_xy(995.0, 795.0, 2e7);
    g.move_xy(c.0, c.1);
    g.up_xy(c.0, c.1);
    assert!(!g.t.host.sel.is_empty(), "crossing selection");

    let iid = g.edit(|m| {
        let sel = connected0(m.root().unwrap());
        Ok(m.make_group("root", &sel, "group", None)?.unwrap())
    });
    assert!(g.root().faces.is_empty() && g.root().insts.len() == 1, "make group");
    g.t.host.sel = vec![Sel::new("i", iid)];
    g.set("move");
    g.click(O);
    g.move_to([6.0, 0.0, 0.0]);
    g.value("6");
    let im = g.root().insts.get(iid).unwrap().m;
    assert!(c6(im[12], 6.0) && c6(im[13], 0.0), "move tool: type 6");
    g.set("move");
    g.t.host.sel = vec![Sel::new("i", iid)];
    g.click_mods([6.0, 0.0, 0.0], Ev { ctrl: true, ..Ev::default() });
    assert!(g.key("ArrowLeft") && g.t.lock.as_ref().unwrap().kind == "axis-y", "← locks green");
    g.move_to([6.0, 5.0, 0.0]);
    g.value("5");
    g.value("3x");
    assert_eq!(g.root().insts.len(), 4, "copy with Ctrl, then 3x");
    let mut ys: Vec<f64> = g.root().insts.values().map(|i| (i.m[13] * 1e6).round() / 1e6).collect();
    ys.sort_by(f64::total_cmp);
    assert_eq!(ys, vec![0.0, 5.0, 10.0, 15.0], "copies at 5 m spacing");
    assert!(g.model().instance_counts().values().any(|n| *n == 4.0), "copies share one definition");

    g.set("rotate");
    g.t.host.sel = vec![Sel::new("i", iid)];
    g.click([6.0, 0.0, 0.0]);
    g.click([8.0, 0.0, 0.0]);
    g.move_to([6.0, 2.0, 0.0]);
    g.value("90");
    let im = g.root().insts.get(iid).unwrap().m;
    assert!(close(im[0], 0.0, 1e-9) && close(im[1], 1.0, 1e-9), "rotate tool: 90°");
    g.t.host.sel = Vec::new();
    g.set("scale");
    g.t.host.sel = vec![Sel::new("i", iid)];
    g.click([6.0, 0.0, 0.0]);
    g.click([7.0, 0.0, 0.0]);
    g.move_to([8.0, 0.0, 0.0]);
    g.value("2");
    let sm = g.root().insts.get(iid).unwrap().m;
    assert!(close(cc_m3d::vec::len([sm[0], sm[1], sm[2]]), 2.0, 1e-9), "scale tool: factor 2");

    // tape measure
    g.new_model();
    g.edit(|m| add_polygon(m.root_mut().unwrap(), &rect_pts(O, R, G, 4.0, 3.0), None));
    g.set("tape");
    g.click([2.3, 0.0, 0.0]);
    g.move_to([2.3, 1.0, 0.0]);
    g.value("1.2");
    let gl = g.model().guides.values().next().cloned().unwrap();
    match gl {
        cc_m3d::model::Guide::Line { p, d } => assert!(c6(d[0].abs(), 1.0) && close(p[1], 1.2, 1e-9), "a parallel guide line 1.2 m off"),
        cc_m3d::model::Guide::Point { .. } => panic!("not a line"),
    }
    g.set("tape");
    g.click([4.0, 3.0, 0.0]);
    g.move_to([4.0, 5.0, 0.0]);
    g.value("2");
    assert!(
        g.model().guides.values().any(|x| matches!(x, cc_m3d::model::Guide::Point { p } if dist(*p, [4.0, 5.0, 0.0]) < 1e-6)),
        "a guide point 2 m away"
    );
    let sg = scene_of(g.model());
    let cam = g.t.host.cam.clone();
    let gpp = cam.project([1.0, 1.2, 0.0]).unwrap();
    let k = infer(&sg, &cam, gpp[0], gpp[1] - 2.0, &InferOpts::default()).kind;
    assert!(k == "guide" || k == "face", "inference sees the guide line");
    let gpt = cam.project([4.0, 5.0, 0.0]).unwrap();
    assert_eq!(infer(&sg, &cam, gpt[0] + 2.0, gpt[1], &InferOpts::default()).kind, "guidept");

    g.set("circle");
    g.click([8.0, 8.0, 0.0]);
    g.move_to([9.0, 8.0, 0.0]);
    g.value("1");
    let circ = g.root().faces.iter().find(|(_, f)| f.loops[0].len() == 24).map(|(id, _)| id).unwrap();
    assert!(close(area(g.root(), circ), 0.5 * 24.0 * (2.0 * std::f64::consts::PI / 24.0).sin(), 1e-9), "circle tool: 24 sides");
    g.set("push");
    g.click([8.0, 8.0, 0.0]);
    g.value("3");
    assert_eq!(g.root().edges.values().filter(|e| e.soft).count(), 24, "pulled circle: 24 soft sides");

    g.set("line");
    g.click([2.0, 0.0, 0.0]);
    g.click([2.0, 3.0, 0.0]);
    g.escape();
    let ground = |g: &Rig| {
        let d = g.root();
        faces_facing(d, Z)
            .into_iter()
            .filter(|id| d.faces.get(*id).unwrap().loops[0].len() < 24 && c6(d.v(d.faces.get(*id).unwrap().loops[0][0])[2], 0.0))
            .count()
    };
    let n_rect = ground(&g);
    g.set("erase");
    let mid = g.at([2.0, 1.5, 0.0]);
    g.down_xy(mid.0, mid.1, 9e7);
    g.up_xy(mid.0, mid.1);
    assert!(n_rect == 2 && ground(&g) == 1, "eraser: the halves heal");
}

#[test]
fn tools_on_faces_and_inside_groups() {
    let mut g = Rig::new();
    g.box_at(O, 4.0, 1.0, 3.0);
    g.cam([2.0, -12.0, 2.5], [2.0, 0.0, 1.5], 20.0);
    g.set("rect");
    g.click([1.0, 0.0, 0.8]);
    g.move_to([2.2, 0.0, 2.2]);
    g.value("1.2,1.4");
    assert_eq!(faces_facing(g.root(), [0.0, -1.0, 0.0]).len(), 2, "rectangle on a vertical face");
    assert!(faces_facing(g.root(), [0.0, -1.0, 0.0]).iter().any(|id| c6(area(g.root(), *id), 1.68)), "the window face is 1.68 m²");
    g.set("push");
    g.click([1.6, 0.0, 1.5]);
    g.move_to([1.6, 0.5, 1.5]);
    g.value("5");
    assert!(closed(g.root()) && c6(volume(g.root()), 12.0 - 1.68), "push/pull through the wall: an opening");
    let sc = scene_of(g.model());
    g.cam([2.0, -2.0, 14.0], [2.0, 2.0, 0.0], 20.0);
    let cam = g.t.host.cam.clone();
    let opts = |from| InferOpts {
        from: Some(from),
        ref_edge: Some((O, [1.0, 1.0, 0.0])),
        axes: Some(Axes3 { x: R, y: G, z: Z }),
        ..InferOpts::default()
    };
    let tq = cam.project([8.0, 2.0, 0.0]).unwrap();
    let par = infer(&sc, &cam, tq[0] + 2.0, tq[1], &opts([6.0, 0.0, 0.0]));
    assert!(par.kind == "parallel" && c6(par.p[0] - 6.0, par.p[1]), "parallel");
    let tq2 = cam.project([8.0, -2.0, 0.0]).unwrap();
    let per = infer(&sc, &cam, tq2[0] + 2.0, tq2[1], &opts([6.0, 0.0, 0.0]));
    assert!(per.kind == "perp" && c6(per.p[0] - 6.0, -per.p[1]), "perpendicular");

    g.new_model();
    let iid = g.edit(|m| {
        let r = m.root_mut().unwrap();
        add_polygon(r, &rect_pts(O, R, G, 2.0, 2.0), None)?;
        let f = r.faces.first_key().unwrap();
        push_pull(r, f, 1.0);
        let f = r.faces.first_key().unwrap();
        let sel = connected(r, &[Sel::new("f", f)]);
        let id = m.make_group("root", &sel, "component", Some("Block"))?.unwrap();
        let r = m.root_mut().unwrap();
        r.insts.get_mut(id).unwrap().m = translation([10.0, 0.0, 0.0]);
        let d = r.insts.get(id).unwrap().def.clone();
        cc_m3d::model::place(r, &d, &translation([20.0, 0.0, 0.0]));
        Ok(id)
    });
    g.t.host.open_context(vec![iid]);
    g.cam([11.0, -6.0, 8.0], [11.0, 1.0, 1.0], 20.0);
    g.set("rect");
    g.click([10.5, 0.5, 1.0]);
    g.move_to([11.5, 1.5, 1.0]);
    g.value("1,1");
    let did = g.root().insts.get(iid).unwrap().def.clone();
    let d = g.model().def(&did).unwrap();
    let inner = faces_facing(d, Z).into_iter().find(|id| c6(area(d, *id), 1.0));
    assert!(inner.is_some_and(|id| c6(d.v(d.faces.get(id).unwrap().loops[0][0])[2], 1.0)), "the rectangle lands on the component's top");
    g.set("push");
    g.click([11.0, 1.0, 1.0]);
    g.value("0.5");
    assert!(c6(volume(g.model().def(&did).unwrap()), 4.5), "pulled inside one instance: both change");
    let sc2 = scene_of(g.model());
    g.cam([16.0, -14.0, 12.0], [16.0, 1.0, 0.5], 30.0);
    let cam = g.t.host.cam.clone();
    let q = cam.project([22.0, 2.0, 1.0]).unwrap();
    let other = infer(&sc2, &cam, q[0] + 2.0, q[1] + 2.0, &InferOpts::default());
    assert!(other.kind == "end" && dist(other.p, [22.0, 2.0, 1.0]) < 1e-9, "inference reaches other instances");

    for in_group in [false, true] {
        g.new_model();
        g.edit(|m| {
            let r = m.root_mut().unwrap();
            add_polygon(r, &rect_pts(O, R, G, 3.0, 0.3), None)?;
            let f = r.faces.first_key().unwrap();
            push_pull(r, f, 0.2);
            Ok(())
        });
        if in_group {
            let gid = g.edit(|m| {
                let sel = connected0(m.root().unwrap());
                Ok(m.make_group("root", &sel, "group", None)?.unwrap())
            });
            g.t.host.open_context(vec![gid]);
        }
        let dd = |g: &Rig| g.t.host.model.context(&g.t.host.path).unwrap().def;
        g.cam([4.2, -0.9, 0.7], [3.0, 0.15, 0.1], 8.0);
        let sce = scene_of(g.model());
        let cam = g.t.host.cam.clone();
        let em = cam.project([3.0, 0.15, 0.2]).unwrap();
        let r0 = infer(&sce, &cam, em[0], em[1] + 2.0, &InferOpts::default());
        assert!(r0.kind == "mid" && r0.face.as_ref().is_some_and(|f| c6(f.n[0].abs(), 1.0)), "a midpoint on the end face carries it");
        let cm = cam.project([3.0, 0.15, 0.1]).unwrap();
        let rc = infer(&sce, &cam, cm[0] + 3.0, cm[1] - 2.0, &InferOpts::default());
        assert!(rc.kind == "center" && dist(rc.p, [3.0, 0.15, 0.1]) < 1e-9 && rc.face.is_some(), "center of face");
        for c in [[3.0, 0.15, 0.2], [3.0, 0.15, 0.1]] {
            let id = dd(&g);
            let before = faces_facing(g.model().def(&id).unwrap(), R).len();
            let vb: Vec<f64> = g.model().def(&id).unwrap().verts.keys().collect();
            g.set("circle");
            let q = g.at(c);
            let k = g.at([3.0, 0.15, 0.1]);
            let kd = ((k.0 - q.0).powi(2) + (k.1 - q.1).powi(2)).sqrt().max(1e-300);
            let kd = if kd > 0.0 { kd } else { 1.0 };
            let (qx, qy) = (q.0 + (k.0 - q.0) / kd * 2.0, q.1 + (k.1 - q.1) / kd * 2.0);
            g.move_xy(qx, qy);
            g.click_xy(qx, qy, Ev::default());
            g.move_to([3.0, 0.2, 0.1]);
            g.value("0.04");
            let d = g.model().def(&id).unwrap();
            let pts: Vec<_> = d.verts.iter().filter(|(v, _)| !vb.contains(v)).map(|(_, p)| *p).collect();
            assert!(pts.len() >= 24 && pts.iter().all(|p| close(p[0], 3.0, 1e-9)), "the circle lies in the end face's plane");
            assert!(faces_facing(d, R).len() > before, "and cuts the end face");
            g.m().undo().unwrap();
        }
        g.t.host.path = Vec::new();
    }
}

#[test]
fn arcs_and_circles() {
    let mut g = Rig::new();
    g.cam([2.0, -8.0, 12.0], [2.0, 1.0, 0.0], 30.0);
    g.set("arc");
    g.move_click(O);
    g.move_click([4.0, 0.0, 0.0]);
    g.move_to([2.0, 1.5, 0.0]);
    g.value("2");
    let ids = curves(g.root(), "arc");
    let ci = curve_info(g.root(), &ids[0]).unwrap();
    assert!(
        ids.len() == 1
            && ci.segs == 12.0
            && ci.round
            && close(ci.radius, 2.0, 1e-9)
            && close(ci.sweep, std::f64::consts::PI, 1e-9)
            && dist(ci.center.unwrap(), [2.0, 0.0, 0.0]) < 1e-9
    );
    assert!(g.root().verts.values().map(|p| p[1]).fold(f64::MIN, f64::max) > 1.9, "it bulges toward the pointer");
    g.set("line");
    g.move_click(O);
    g.move_click([4.0, 0.0, 0.0]);
    g.escape();
    let f = g.root().faces.first_key().unwrap();
    assert!(
        g.root().faces.len() == 1 && close(area(g.root(), f), 0.5 * 12.0 * 4.0 * (std::f64::consts::PI / 12.0).sin(), 1e-9),
        "a half-disc face"
    );
    g.set("select");
    let ce = curve_edges(g.root(), &ids[0]);
    let e = g.root().edges.get(ce[5]).unwrap().clone();
    let segmid = mul(add(g.root().v(e.a), g.root().v(e.b)), 0.5);
    g.move_click(segmid);
    assert!(g.t.host.sel.len() == 12, "one click selects the whole arc");
    let id0 = ids[0].clone();
    g.edit(|m| set_curve_radius(m.root_mut().unwrap(), &id0, 3.0).map(|_| ()));
    assert!(close(curve_info(g.root(), &ids[0]).unwrap().radius, 3.0, 1e-9) && g.root().faces.len() == 1);
    g.m().undo().unwrap();
    assert!(close(curve_info(g.root(), &ids[0]).unwrap().radius, 2.0, 1e-9));

    g.new_model();
    g.box_at(O, 4.0, 1.0, 3.0);
    g.cam([2.0, -12.0, 2.5], [2.0, 0.0, 1.5], 20.0);
    g.set("arc");
    g.value("6s");
    g.move_click([1.0, 0.0, 1.0]);
    g.move_click([3.0, 0.0, 1.0]);
    g.move_to([2.0, 0.0, 2.0]);
    g.value("0.5");
    let ids = curves(g.root(), "arc");
    let ci = curve_info(g.root(), &ids[0]).unwrap();
    assert!(
        ci.segs == 6.0
            && ci.verts.iter().all(|v| close(g.root().v(*v)[1], 0.0, 1e-9))
            && ci.verts.iter().map(|v| g.root().v(*v)[2]).fold(f64::MIN, f64::max) > 1.49
    );

    g.new_model();
    g.cam([2.0, -8.0, 12.0], [2.0, 1.0, 0.0], 30.0);
    g.set("arcc");
    g.move_click([10.0, 0.0, 0.0]);
    g.move_to([12.0, 0.0, 0.0]);
    g.value("2");
    g.move_to([10.0, 2.0, 0.0]);
    g.value("90");
    let ids = curves(g.root(), "arc");
    let ci = curve_info(g.root(), &ids[0]).unwrap();
    assert!(
        close(ci.radius, 2.0, 1e-9)
            && close(ci.sweep, std::f64::consts::FRAC_PI_2, 1e-9)
            && dist(ci.center.unwrap(), [10.0, 0.0, 0.0]) < 1e-9
            && ci.segs == 12.0
    );
    g.set("circle");
    g.move_click([0.0, 5.0, 0.0]);
    g.move_to([1.0, 5.0, 0.0]);
    g.value("1");
    let circ = curves(g.root(), "circle");
    let ci = curve_info(g.root(), &circ[0]).unwrap();
    assert!(ci.closed && ci.segs == 24.0 && close(ci.radius, 1.0, 1e-9) && ci.flat);
    let c0 = circ[0].clone();
    g.edit(|m| set_curve_radius(m.root_mut().unwrap(), &c0, 1.5).map(|_| ()));
    g.set("push");
    g.move_click([0.0, 5.0, 0.0]);
    g.value("1");
    let r = g.m().change("r", |m| set_curve_radius(m.root_mut().unwrap(), &c0, 2.0).map(|_| ()));
    assert!(!curve_info(g.root(), &circ[0]).unwrap().flat && r.is_err_and(|e| e.to_string().contains("pushed or pulled")));
}

#[test]
fn offset() {
    let mut g = Rig::new();
    g.box_at(O, 1.6, 0.9, 0.05);
    g.cam([0.8, -1.2, 2.2], [0.8, 0.45, 0.05], 10.0);
    g.set("offset");
    g.move_click([0.8, 0.45, 0.05]);
    g.move_to([0.8, 0.2, 0.05]);
    g.value("0.03");
    let tops: Vec<f64> = g
        .root()
        .faces
        .iter()
        .filter(|(_, f)| c6(face_normal(g.root(), f)[2], 1.0) && c6(g.root().v(f.loops[0][0])[2], 0.05))
        .map(|(id, _)| id)
        .collect();
    assert!(tops.len() == 2 && tops.iter().any(|id| close(area(g.root(), *id), 1.54 * 0.84, 1e-9)), "a glass face inside a 30 mm frame");
    g.set("push");
    g.move_click([0.8, 0.45, 0.05]);
    g.move_to([0.8, 0.45, 0.04]);
    g.value("0.005");
    assert!(closed(g.root()) && close(volume(g.root()), 1.6 * 0.9 * 0.05 - 1.54 * 0.84 * 0.005, 1e-9), "the glass pushed in 5 mm");

    g.new_model();
    g.edit(|m| add_polygon(m.root_mut().unwrap(), &rect_pts(O, R, G, 4.0, 3.0), None));
    g.cam([2.0, -6.0, 10.0], [2.0, 1.5, 0.0], 30.0);
    g.t.host.sel = vec![Sel::new("f", g.root().faces.first_key().unwrap())];
    g.set("offset");
    g.move_to([2.0, -0.5, 0.0]);
    g.value("0.5");
    let mut areas: Vec<f64> = g.root().faces.keys().map(|id| area(g.root(), id)).collect();
    areas.sort_by(f64::total_cmp);
    assert!(areas.len() == 2 && close(areas[0], 5.0 * 4.0 - 12.0, 1e-9) && close(areas[1], 12.0, 1e-9), "offset outward");
    g.set("offset");
    g.move_click([2.0, 1.5, 0.0]);
    g.move_to([2.0, 1.2, 0.0]);
    g.value("0.5");
    assert!(g.root().faces.len() == 3 && g.root().faces.keys().any(|id| close(area(g.root(), id), 6.0, 1e-9)), "inward");
    g.set("offset");
    let q = g.at([2.0, 1.5, 0.0]);
    g.move_xy(q.0, q.1);
    g.time += 5000.0;
    let t0 = g.time;
    g.down_xy(q.0, q.1, t0);
    g.up_xy(q.0, q.1);
    g.down_xy(q.0, q.1, t0 + 100.0);
    g.up_xy(q.0, q.1);
    g.time += 100.0;
    assert!(
        g.root().faces.len() == 4 && g.root().faces.keys().any(|id| close(area(g.root(), id), 2.0, 1e-9)),
        "double-click repeats the distance"
    );
    g.set("offset");
    g.move_click([2.0, 1.5, 0.0]);
    g.move_to([2.0, 1.2, 0.0]);
    g.value("3");
    assert!(g.last_msg().contains("further than this face allows") && g.root().faces.len() == 4, "too far is refused with the reason");

    g.new_model();
    g.set("circle");
    g.move_click([2.0, 1.5, 0.0]);
    g.move_to([3.0, 1.5, 0.0]);
    g.value("1");
    g.set("offset");
    g.move_click([2.0, 1.5, 0.0]);
    g.move_to([2.2, 1.5, 0.0]);
    g.value("0.25");
    let cs = curves(g.root(), "circle");
    assert!(
        cs.len() == 2
            && cs.iter().any(|c| {
                let ci = curve_info(g.root(), c).unwrap();
                ci.round && close(ci.radius, 1.0 - 0.25 / (std::f64::consts::PI / 24.0).cos(), 1e-9)
            })
    );

    g.new_model();
    g.edit(|m| {
        let r = m.root_mut().unwrap();
        line(r, O, [3.0, 0.0, 0.0]);
        line(r, [3.0, 0.0, 0.0], [3.0, 2.0, 0.0]);
        Ok(())
    });
    g.t.host.sel = g.root().edges.keys().map(|id| Sel::new("e", id)).collect();
    g.set("offset");
    g.move_to([2.0, 0.4, 0.0]);
    g.value("0.2");
    let segs: Vec<_> = g.root().edges.values().map(|e| (g.root().v(e.a), g.root().v(e.b))).collect();
    assert!(
        segs.len() == 4
            && segs.iter().any(|(a, b)| c6(a[1], 0.2) && c6(b[1], 0.2) && c6((a[0] - b[0]).abs(), 2.8))
            && segs.iter().any(|(a, b)| c6(a[0], 2.8) && c6(b[0], 2.8) && c6((a[1] - b[1]).abs(), 1.8))
    );
}

#[test]
fn smooth_curves() {
    let mut g = Rig::new();
    g.cam([4.0, -10.0, 12.0], [4.0, 2.0, 0.0], 40.0);
    let pp = [[0.0, 0.0, 0.0], [2.0, 3.0, 0.0], [5.0, 1.0, 0.0], [8.0, 4.0, 0.0]];
    g.set("curve");
    for p in pp {
        g.move_click(p);
    }
    g.t.call("finish", &[]).unwrap();
    let ids = curves(g.root(), "curve");
    let cid = ids[0].clone();
    let ci = curve_info(g.root(), &cid).unwrap();
    let passes = |d: &cc_m3d::Def, pts: &[[f64; 3]]| pts.iter().all(|p| d.verts.values().any(|q| dist(*p, *q) < 1e-9));
    assert!(
        ids.len() == 1
            && ci.segs == 24.0
            && !ci.closed
            && passes(g.root(), &pp)
            && g.root().splines.get(cid.as_str()).unwrap().pts.len() == 4
    );
    g.set("curve");
    g.move_click([0.0, 6.0, 0.0]);
    g.value("<2,0,1>");
    g.value("<2,1,1>");
    let q = g.at([4.0, 7.0, 2.0]);
    g.move_xy(q.0, q.1);
    g.click_xy(q.0, q.1, Ev::default());
    g.time -= 900.0;
    g.click_xy(q.0, q.1, Ev::default());
    let c2 = curves(g.root(), "curve").into_iter().find(|c| *c != cid).unwrap();
    assert!(
        g.root().splines.get(c2.as_str()).unwrap().pts.iter().map(|p| p[2]).fold(f64::MIN, f64::max) >= 2.0 - 1e-9,
        "a 3D curve, finished by a double-click"
    );
    g.set("curve");
    g.move_click([10.0, 0.0, 0.0]);
    g.move_click([12.0, 0.0, 0.0]);
    g.move_click([13.0, 9.0, 0.0]);
    g.t.call("back", &[]).unwrap();
    g.move_click([12.0, 2.0, 0.0]);
    g.move_click([10.0, 0.0, 0.0]);
    let c3 = curves(g.root(), "curve").into_iter().find(|c| *c != cid && *c != c2).unwrap();
    assert!(curve_info(g.root(), &c3).unwrap().closed && g.root().faces.len() == 1, "clicking the first point closes it");
    g.set_arg("curve", ToolArg::CurveEdit(cid.clone()));
    g.move_click([5.0, 1.0, 0.0]);
    g.move_click([5.0, -1.0, 0.0]);
    assert!(
        passes(g.root(), &[pp[0], pp[1], [5.0, -1.0, 0.0], pp[3]])
            && !passes(g.root(), &[[5.0, 1.0, 0.0]])
            && curve_edges(g.root(), &cid).len() == 24
    );
    g.m().undo().unwrap();
    assert!(passes(g.root(), &pp), "one undo puts it back");
}

#[test]
fn follow_me() {
    let mut g = Rig::new();
    g.cam([3.0, -8.0, 6.0], [3.0, 1.0, 0.5], 30.0);
    let cp = [[0.0, 0.0, 0.5], [2.0, 1.0, 0.5], [4.0, -0.5, 1.0], [6.0, 1.0, 0.5]];
    g.edit(|m| {
        let r = m.root_mut().unwrap();
        add_spline(r, &cp, false, 8.0, None)?;
        let sp = smooth_pts(&cp, false, 8.0);
        add_polygon(r, &ngon_pts(cp[0], norm(sub(sp[1], cp[0])), Z, 0.05, 16.0), None)?;
        Ok(())
    });
    let cid = g.root().splines.keys().next().unwrap().clone();
    let ce = curve_edges(g.root(), &cid);
    g.t.host.sel = ce.iter().map(|id| Sel::new("e", *id)).collect();
    g.set("follow");
    let disc = g.root().faces.first_key().unwrap();
    let path: Vec<serde_json::Value> = ce.iter().map(|x| serde_json::json!(x)).collect();
    g.t.call("run", &[serde_json::json!(disc), serde_json::Value::Array(path)]).unwrap();
    assert!(closed(g.root()) && volume(g.root()) > 0.0 && g.root().edges.values().filter(|e| e.soft).count() > 100, "a closed smooth tube");
    g.m().undo().unwrap();
    g.t.host.sel = Vec::new();
    g.set("follow");
    g.cam([-0.6, -0.4, 0.7], [0.0, 0.0, 0.5], 10.0);
    g.move_click([0.0, 0.0, 0.53]);
    assert!(g.t.tool_state()["face"].is_number(), "a click on the face picks it");
    g.cam([3.0, -8.0, 6.0], [3.0, 1.0, 0.5], 30.0);
    let e = g.root().edges.get(ce[12]).unwrap().clone();
    g.move_click(mul(add(g.root().v(e.a), g.root().v(e.b)), 0.5));
    assert!(closed(g.root()), "then an edge of the path: the same tube");
}

#[test]
fn protractor() {
    let mut g = Rig::new();
    g.cam([1.0, -6.0, 9.0], [1.0, 1.0, 0.0], 30.0);
    let last = |g: &Rig| match g.model().guides.values().last().cloned() {
        Some(cc_m3d::model::Guide::Line { p, d }) => (p, d),
        _ => panic!("no guide line"),
    };
    g.set("protractor");
    g.move_click(O);
    g.move_click([1.0, 0.0, 0.0]);
    g.move_to([2.0, 2.05, 0.0]);
    g.move_click([2.0, 2.05, 0.0]);
    let (p, d) = last(&g);
    let h = std::f64::consts::FRAC_1_SQRT_2;
    assert!(dist(p, O) < 1e-9 && (d[0].abs() - h).abs() < 1e-9 && (d[1].abs() - h).abs() < 1e-9 && d[2].abs() < 1e-12, "45° (15° snap)");
    g.set("protractor");
    g.move_click(O);
    g.move_click([1.0, 0.0, 0.0]);
    g.move_to([1.0, 0.3, 0.0]);
    g.value("30");
    let (_, d) = last(&g);
    assert!((d[1] / d[0] - (std::f64::consts::PI / 6.0).tan()).abs() < 1e-9, "a typed 30");
    g.box_at([4.0, 0.0, 0.0], 2.0, 1.0, 2.0);
    g.cam([5.0, -8.0, 1.0], [5.0, 0.0, 1.0], 30.0);
    g.set("protractor");
    g.move_click([5.0, 0.0, 1.0]);
    g.move_click([5.8, 0.0, 1.0]);
    g.move_to([5.5, 0.0, 1.6]);
    g.value("60");
    let (_, d) = last(&g);
    assert!(d[1].abs() < 1e-12 && (d[2].abs() - (std::f64::consts::PI / 3.0).sin()).abs() < 1e-9, "on the front face");
    g.cam([1.0, -6.0, 9.0], [1.0, 1.0, 0.0], 30.0);
    g.set("protractor");
    g.move_click(O);
    g.key("ArrowRight");
    g.move_click([0.0, 1.0, 0.0]);
    g.move_to([0.0, 1.0, 1.0]);
    g.value("90");
    let (_, d) = last(&g);
    assert!(d[0].abs() < 1e-12 && (d[2].abs() - 1.0).abs() < 1e-9, "→ stands it square to red");
}

#[test]
fn flip() {
    let mut g = Rig::new();
    let lp = [[0.0, 0.0, 0.0], [3.0, 0.0, 0.0], [3.0, 1.0, 0.0], [1.0, 1.0, 0.0], [1.0, 2.0, 0.0], [0.0, 2.0, 0.0]];
    g.edit(|m| {
        let r = m.root_mut().unwrap();
        add_polygon(r, &lp, None)?;
        let f = r.faces.first_key().unwrap();
        push_pull(r, f, 1.0);
        Ok(())
    });
    let v0 = volume(g.root());
    g.edit(|m| {
        let r = m.root_mut().unwrap();
        let all = connected0(r);
        flip_sel(r, &all, [1.5, 1.0, 0.5], R, false);
        Ok(())
    });
    let xs: Vec<f64> = g.root().verts.values().map(|p| p[0]).collect();
    assert!(
        closed(g.root())
            && close(volume(g.root()), v0, 1e-9)
            && c6(xs.iter().copied().fold(f64::MAX, f64::min), 0.0)
            && c6(xs.iter().copied().fold(f64::MIN, f64::max), 3.0)
    );
    assert!(g.root().verts.values().any(|p| c6(p[0], 2.0) && c6(p[1], 2.0)), "the arm now at x = 2…3");
    assert!(det3(&mirror_matrix([1.0, 2.0, 3.0], G)) < 0.0, "mirrorMatrix is a reflection");
}

#[test]
fn scale_grips() {
    let mut g = Rig::new();
    g.box_at(O, 2.0, 1.0, 1.0);
    g.cam([6.0, -6.0, 5.0], [1.0, 0.5, 0.5], 30.0);
    let bbox = |g: &Rig| g.model().def_bbox("root").unwrap();
    let resel = |g: &mut Rig| g.t.host.sel = connected0(g.root());
    let cm = |g: &mut Rig, p, mods: Ev| {
        let (x, y) = g.at(p);
        g.t.mv(&Ev { x, y, ..mods }).unwrap();
        g.click_xy(x, y, mods);
    };
    resel(&mut g);
    g.set("scale");
    assert_eq!(g.t.tool_state()["grips"].as_array().unwrap().len(), 26, "26 grips");
    cm(&mut g, [2.0, 0.5, 0.5], Ev::default());
    g.move_to([3.0, 0.5, 0.5]);
    g.value("2");
    let b = bbox(&g);
    assert!(c6(b.min[0], 0.0) && c6(b.max[0], 4.0) && c6(b.max[1], 1.0) && c6(b.max[2], 1.0), "the +red grip, 2");
    resel(&mut g);
    g.set("scale");
    cm(&mut g, [4.0, 1.0, 1.0], Ev::default());
    g.move_to([3.0, 0.7, 0.7]);
    g.value("0.5");
    let b = bbox(&g);
    assert!(c6(b.max[0], 2.0) && c6(b.max[1], 0.5) && c6(b.max[2], 0.5) && c6(b.min[0], 0.0), "a corner grip, 0.5");
    resel(&mut g);
    g.set("scale");
    cm(&mut g, [2.0, 0.5, 0.5], Ev::default());
    g.value("2,3,4");
    let b = bbox(&g);
    assert!(c6(b.max[0], 4.0) && c6(b.max[1], 1.5) && c6(b.max[2], 2.0), "three factors");
    resel(&mut g);
    g.set("scale");
    cm(&mut g, [4.0, 0.75, 1.0], Ev { ctrl: true, ..Ev::default() });
    g.value("2");
    let b = bbox(&g);
    assert!(c6(b.min[0], -2.0) && c6(b.max[0], 6.0), "Ctrl: about the centre");
    assert!(closed(g.root()) && close(volume(g.root()), 8.0 * 1.5 * 2.0, 1e-9));
    let gid = g.edit(|m| {
        let sel = connected0(m.root().unwrap());
        Ok(m.make_group("root", &sel, "group", None)?.unwrap())
    });
    g.t.host.sel = vec![Sel::new("i", gid)];
    g.set("scale");
    cm(&mut g, [2.0, 1.5, 1.0], Ev::default());
    g.value("3");
    let im = g.root().insts.get(gid).unwrap().m;
    assert!(
        close(cc_m3d::vec::len([im[4], im[5], im[6]]), 3.0, 1e-9) && close(cc_m3d::vec::len([im[0], im[1], im[2]]), 1.0, 1e-9),
        "a group stretches 3× along green"
    );
}

#[test]
fn snapping_to_a_closed_group() {
    let mut g = Rig::new();
    let _ga = g.group_box(O, 2.0, 1.0, 1.5);
    let gb = g.group_box([4.0, 0.0, 0.0], 1.0, 1.0, 1.0);
    let mut ok = 0;
    let mut n = 0;
    for ctx in [vec![], vec![gb]] {
        g.t.host.path = ctx;
        for eye in [[6.0, -6.0, 5.0], [-4.0, -5.0, 3.0], [7.0, 3.0, 4.0], [1.0, 0.5, 9.0]] {
            g.cam(eye, [2.0, 0.5, 0.7], 30.0);
            let sc = g.t.host.scene();
            for x in [0.0, 2.0] {
                for y in [0.0, 1.0] {
                    for z in [0.0, 1.5] {
                        let p = [x, y, z];
                        let s2 = g.at(p);
                        let ray = g.t.host.cam.ray(s2.0, s2.1);
                        let tp = dist(ray.o, p);
                        let hidden = sc.faces().iter().any(|f| {
                            f.tris.iter().any(|t| cc_m3d::vec::ray_tri(ray.o, ray.d, t[0], t[1], t[2]).is_some_and(|tt| tt < tp - 1e-6))
                        });
                        if hidden {
                            continue;
                        }
                        g.set("line");
                        g.move_xy(s2.0 + 2.0, s2.1 - 1.0);
                        n += 1;
                        if g.t.cur().is_some_and(|c| c.kind == "end" && dist(c.p, p) < 1e-9) {
                            ok += 1;
                        }
                    }
                }
            }
        }
    }
    assert!(n >= 40 && ok == n, "line snaps to every visible corner ({ok}/{n})");
    g.t.host.path = vec![gb];
    g.cam([6.0, -6.0, 5.0], [2.0, 0.5, 0.7], 30.0);
    let q = g.at([2.0, 0.0, 1.5]);
    g.set("line");
    g.move_xy(q.0 + 2.0, q.1 - 1.0);
    g.click_xy(q.0 + 2.0, q.1 - 1.0, Ev::default());
    g.value("<0,0,1>");
    g.escape();
    let inst = g.root().insts.get(gb).unwrap().clone();
    let d = g.model().def(&inst.def).unwrap();
    let found = d.edges.values().any(|e| {
        let (a, b) = (cc_m3d::vec::apply(&inst.m, d.v(e.a)), cc_m3d::vec::apply(&inst.m, d.v(e.b)));
        (dist(a, [2.0, 0.0, 1.5]) < 1e-9 && dist(b, [2.0, 0.0, 2.5]) < 1e-9)
            || (dist(b, [2.0, 0.0, 1.5]) < 1e-9 && dist(a, [2.0, 0.0, 2.5]) < 1e-9)
    });
    assert!(found, "inside group B, a line from A's corner starts exactly there");
}

#[test]
fn touch_reach() {
    let mut g = Rig::new();
    g.edit(|m| add_polygon(m.root_mut().unwrap(), &rect_pts(O, R, G, 4.0, 3.0), None));
    g.cam([2.0, -6.0, 10.0], [2.0, 1.5, 0.0], 30.0);
    let c = g.at([4.0, 3.0, 0.0]);
    g.set("line");
    g.move_xy(c.0 + 16.0, c.1 - 4.0);
    let mouse = g.t.cur().unwrap().kind.clone();
    g.t.touch = true;
    g.set("line");
    g.move_xy(c.0 + 16.0, c.1 - 4.0);
    assert!(mouse != "end" && g.t.cur().unwrap().kind == "end", "a finger 16 px away snaps");
    let e = g.at([2.0, 3.0, 0.0]);
    g.t.touch = false;
    let pm = g.t.pick_at(&Ev::at(e.0, e.1 - 11.0));
    g.t.touch = true;
    let pt = g.t.pick_at(&Ev::at(e.0, e.1 - 11.0));
    assert!(pm.is_none_or(|s| s.t != "e") && pt.is_some_and(|s| s.t == "e"), "a tap 11 px off an edge picks it on touch");
}

#[test]
fn paint_tool() {
    let mut g = Rig::new();
    g.edit(|m| {
        let r = m.root_mut().unwrap();
        add_polygon(r, &rect_pts(O, R, G, 2.0, 2.0), None)?;
        let f = r.faces.first_key().unwrap();
        push_pull(r, f, 1.0);
        add_polygon(r, &rect_pts([4.0, 0.0, 0.0], R, G, 2.0, 2.0), None)?;
        let f2 = r
            .faces
            .iter()
            .find(|(_, f)| {
                let x = r.v(f.loops[0][0])[0];
                c6(x, 4.0) || c6(x, 6.0)
            })
            .map(|(id, _)| id)
            .unwrap();
        push_pull(r, f2, 1.0);
        Ok(())
    });
    g.cam([3.0, -8.0, 7.0], [3.0, 1.0, 0.5], 30.0);
    let face_at = |g: &Rig, p: [f64; 3], n: [f64; 3]| -> Option<f64> {
        let d = g.root();
        d.faces
            .iter()
            .find(|(_, f)| {
                let nn = face_normal(d, f);
                dot(nn, n) > 0.999
                    && dot(nn, sub(p, d.v(f.loops[0][0]))).abs() < 1e-9
                    && f.loops[0].iter().any(|v| (d.v(*v)[0] - p[0]).abs() < 1.01)
            })
            .map(|(id, _)| id)
    };
    g.t.paint_color = Some("#c0392b".into());
    g.set("paint");
    g.move_click([1.0, 0.0, 0.5]);
    let f1 = face_at(&g, [1.0, 0.0, 0.5], [0.0, -1.0, 0.0]).unwrap();
    assert!(
        g.root().faces.get(f1).unwrap().color.as_deref() == Some("#c0392b")
            && g.root().faces.values().filter(|f| f.color.is_some()).count() == 1
    );
    g.t.paint_color = Some("#2b5fbd".into());
    g.click_mods([1.0, 1.0, 1.0], Ev { ctrl: true, ..Ev::default() });
    assert_eq!(
        g.root().faces.values().filter(|f| f.color.as_deref() == Some("#2b5fbd")).count(),
        5,
        "Ctrl: connected faces of that colour"
    );
    g.t.paint_color = Some("#2e9d57".into());
    g.click_mods([1.0, 1.0, 1.0], Ev { shift: true, ..Ev::default() });
    assert!(
        g.root().faces.values().filter(|f| f.color.as_deref() == Some("#2e9d57")).count() == 5
            && !g.root().faces.values().any(|f| f.color.as_deref() == Some("#2b5fbd"))
    );
    g.click_mods([1.0, 0.0, 0.5], Ev { alt: true, ..Ev::default() });
    assert!(g.t.paint_color.as_deref() == Some("#c0392b") && g.t.host.paint_picked == Some(Some("#c0392b".into())), "Alt picks up");
    g.t.paint_color = None;
    g.move_click([1.0, 0.0, 0.5]);
    assert!(g.root().faces.get(face_at(&g, [1.0, 0.0, 0.5], [0.0, -1.0, 0.0]).unwrap()).unwrap().color.is_none(), "the default colour");
    let top = face_at(&g, [5.0, 1.0, 1.0], Z).unwrap();
    g.edit(|m| {
        let sel = connected(m.root().unwrap(), &[Sel::new("f", top)]);
        m.make_group("root", &sel, "group", None).map(|_| ())
    });
    let gid = g.root().insts.keys().next().unwrap();
    g.t.paint_color = Some("#f1c40f".into());
    g.move_click([5.0, 1.0, 1.0]);
    let inst = g.root().insts.get(gid).unwrap();
    assert!(
        inst.color.as_deref() == Some("#f1c40f") && !g.model().def(&inst.def).unwrap().faces.values().any(|f| f.color.is_some()),
        "a group's placement is painted"
    );
}

#[test]
fn collisions_grab_rotate_scale_copy_paste() {
    let mut g = Rig::new();
    g.t.collide = true;
    g.cam([10.0, -18.0, 14.0], [3.0, 2.0, 0.0], 30.0);
    let wall_a = g.group_box(O, 4.0, 0.2, 2.5);
    let _wall_b = g.group_box([6.0, -2.0, 0.0], 0.2, 4.0, 2.5);
    let pos = |g: &Rig, id| {
        let m = g.root().insts.get(id).unwrap().m;
        [m[12], m[13], m[14]]
    };
    g.t.host.sel = vec![Sel::new("i", wall_a)];
    let mv = |g: &mut Rig, d: [f64; 3]| {
        g.set("move");
        g.t.call("begin", &[serde_json::json!([0, 0, 0]), serde_json::json!(false)]).unwrap();
        g.t.call("apply", &[serde_json::json!(d)]).unwrap();
        let st = g.t.tool_state();
        (
            st["stopped"].as_bool().unwrap(),
            [st["delta"][0].as_f64().unwrap(), st["delta"][1].as_f64().unwrap(), st["delta"][2].as_f64().unwrap()],
        )
    };
    let (stopped, d) = mv(&mut g, [4.0, 0.0, 0.0]);
    assert!(stopped && close(d[0], 2.0, 1e-4), "a moved wall stops where it touches");
    g.t.call("finish", &[]).unwrap();
    assert!(close(pos(&g, wall_a)[0], 2.0, 1e-4), "the stop is committed");
    let (stopped, d) = mv(&mut g, [0.0, 1.0, 0.0]);
    assert!(!stopped && c6(d[1], 1.0), "sliding along the face is allowed");
    g.escape();
    let cube = g.group_box([10.0, 0.0, 0.0], 1.0, 1.0, 1.0);
    g.t.host.sel = vec![Sel::new("i", cube)];
    let (stopped, _) = mv(&mut g, [-3.8, 0.0, 2.5]);
    assert!(!stopped, "through empty air");
    g.escape();
    g.t.collide = false;
    mv(&mut g, [-4.0, -1.0, 5.0]);
    g.t.call("finish", &[]).unwrap();
    g.t.collide = true;
    let (stopped, d) = mv(&mut g, [0.0, 0.0, -10.0]);
    assert!(stopped && close(d[2], -2.5, 1e-4), "lowered onto a wall, it lands on the top");
    g.t.call("finish", &[]).unwrap();
    let (stopped, _) = mv(&mut g, [0.0, 0.5, 0.0]);
    assert!(!stopped, "it slides along the top");
    g.escape();
    g.m().undo().unwrap();
    g.m().undo().unwrap();
    let (stopped, d) = mv(&mut g, [-9.0, 1.0, 0.0]);
    assert!(stopped && d[0] > -3.81 && d[0] < -3.79, "a long move stops at the thin wall");
    g.escape();
    g.t.collide = false;
    let (stopped, d) = mv(&mut g, [-9.0, 1.0, 0.0]);
    assert!(!stopped && c6(d[0], -9.0), "collide off: passes through");
    g.escape();
    g.t.collide = true;
    let inside = g.group_box([6.05, 0.0, 0.0], 0.1, 0.5, 0.5);
    g.t.host.sel = vec![Sel::new("i", inside)];
    let (stopped, _) = mv(&mut g, [3.0, 0.0, 0.0]);
    assert!(!stopped, "already overlapping is not in the way");
    g.escape();

    // Blender-style grab
    g.t.host.sel = vec![Sel::new("i", cube)];
    let before = pos(&g, cube);
    g.t.collide = false;
    let e = g.at([10.5, 0.5, 1.0]);
    assert!(g.t.grab(&Ev::at(e.0, e.1), false).unwrap(), "grab starts");
    assert!(g.t.name == "move" && g.t.busy() && g.t.modal());
    g.key("ArrowRight");
    let e2 = g.at([12.5, 2.0, 1.0]);
    g.move_xy(e2.0, e2.1);
    g.down_xy(e2.0, e2.1, 5000.0);
    g.up_xy(e2.0, e2.1);
    let after = pos(&g, cube);
    assert!(c6(after[1], before[1]) && c6(after[2], before[2]) && after[0] > before[0] + 1.0, "grab + red lock");
    assert_eq!(g.t.name, "select", "back to Select");
    let n_inst = g.root().insts.len();
    let e3 = g.at([12.5, 0.5, 1.0]);
    g.t.grab(&Ev::at(e3.0, e3.1), true).unwrap();
    g.move_to([14.0, 0.5, 1.0]);
    g.escape();
    assert!(g.root().insts.len() == n_inst && g.t.name == "select", "duplicate + Esc");
    g.t.grab(&Ev::at(e3.0, e3.1), true).unwrap();
    g.move_to([14.0, 0.5, 1.0]);
    g.value("3");
    assert_eq!(g.root().insts.len(), n_inst + 1, "duplicate + typed distance");
    g.value("3x");
    assert_eq!(g.root().insts.len(), n_inst + 3, "then 3x");
    g.m().undo().unwrap();
    g.m().undo().unwrap();
    g.t.host.sel = vec![Sel::new("i", wall_a)];
    let ca = g.t.sel_center().unwrap();
    let e4 = g.at([ca[0] + 1.0, ca[1], 0.0]);
    g.t.turn(&Ev::at(e4.0, e4.1)).unwrap();
    assert!(g.t.name == "rotate");
    g.value("90");
    let inst = g.root().insts.get(wall_a).unwrap().clone();
    let bb = g.model().bbox(&inst.def, &inst.m).unwrap();
    assert!(
        close(bb.max[0] - bb.min[0], 0.2, 1e-6) && close(bb.max[1] - bb.min[1], 4.0, 1e-6) && dist(g.t.sel_center().unwrap(), ca) < 1e-6,
        "turned about its centre"
    );
    assert_eq!(g.t.name, "select");
    g.t.host.sel = vec![Sel::new("i", cube)];
    let cc = g.t.sel_center().unwrap();
    let e5 = g.at([cc[0] + 1.0, cc[1], cc[2]]);
    g.t.resize(&Ev::at(e5.0, e5.1)).unwrap();
    g.value("2");
    let inst = g.root().insts.get(cube).unwrap().clone();
    let bb = g.model().bbox(&inst.def, &inst.m).unwrap();
    assert!(close(bb.max[0] - bb.min[0], 2.0, 1e-6) && dist(g.t.sel_center().unwrap(), cc) < 1e-6, "twice the size about its centre");
    // paste through the tool
    let n_defs = g.model().defs.len();
    let clip = g.m().copy_clip(&[], &[Sel::new("i", cube)]).unwrap().unwrap();
    g.set_arg("paste", ToolArg::Paste(clip));
    g.move_click([20.0, 0.0, 0.0]);
    assert!(g.model().defs.len() == n_defs + 1 && g.last_msg() == "Pasted.", "a copied group pastes as a new group");
}

#[test]
fn dimensions() {
    let mut g = Rig::new();
    g.cam([8.0, -14.0, 12.0], [2.0, 2.0, 1.0], 20.0);
    g.box_at(O, 4.0, 3.0, 2.5);
    assert!(c6(volume(g.root()), 30.0));
    g.set("dim");
    g.click(O);
    g.click([4.0, 0.0, 0.0]);
    g.move_to([2.0, -1.0, 0.0]);
    g.click([2.0, -1.0, 0.0]);
    let did = g.model().dims.first_key().unwrap();
    let mut dm = g.model().dims.get(did).cloned().unwrap();
    assert!(
        matches!(&dm, cc_m3d::model::Dim::Lin { a, b, off, .. } if a.at.is_some() && b.at.is_some() && close(off[1], -1.0, 1e-6)),
        "attached to both vertices, 1 m in front"
    );
    let vp = g.t.host.cam.view_proj();
    assert!(c6(dim_layout(g.model(), &mut dm, &g.t.host.cam, &vp).l, 4.0), "it measures 4 m");
    g.value("6m");
    let bb = g.model().def_bbox("root").unwrap();
    assert!(c6(bb.max[0], 6.0) && c6(bb.min[0], 0.0) && c6(volume(g.root()), 45.0) && closed(g.root()), "typing 6m right after");
    g.set("select");
    let mut dm = g.model().dims.get(did).cloned().unwrap();
    let l = dim_layout(g.model(), &mut dm, &g.t.host.cam, &vp);
    let m = l.m.unwrap();
    g.t.click(&Ev { x: m[0], y: m[1], time: Some(5e6), ..Ev::default() }).unwrap();
    assert!(g.t.host.sel.len() == 1 && g.t.host.sel[0].t == "d" && g.t.host.sel[0].id == did, "clicking the text selects it");
    g.value("5");
    assert!(c6(g.model().def_bbox("root").unwrap().max[0], 5.0) && c6(volume(g.root()), 37.5), "selected dimension + 5");
    let e_y = g
        .root()
        .edges
        .iter()
        .find(|(_, e)| {
            let (a, b) = (g.root().v(e.a), g.root().v(e.b));
            c6(a[0], 0.0) && c6(b[0], 0.0) && c6(a[2], 0.0) && c6(b[2], 0.0)
        })
        .map(|(id, _)| id)
        .unwrap();
    g.set("select");
    g.t.host.sel = vec![Sel::new("e", e_y)];
    g.value("4");
    let bb = g.model().def_bbox("root").unwrap();
    assert!(c6(bb.max[1], 4.0) && c6(bb.min[1], 0.0) && c6(volume(g.root()), 5.0 * 4.0 * 2.5), "selected edge + 4");
    g.set("erase");
    let mut dm = g.model().dims.get(did).cloned().unwrap();
    let l = dim_layout(g.model(), &mut dm, &g.t.host.cam, &vp);
    let m = l.m.unwrap();
    g.down_xy(m[0], m[1], 9e8);
    g.up_xy(m[0], m[1]);
    assert!(!g.model().dims.has(did), "the eraser removes a dimension");
}

#[test]
fn section_planes() {
    let mut g = Rig::new();
    g.cam([2.0, -9.0, 6.0], [2.0, 1.5, 1.0], 40.0);
    g.box_at(O, 4.0, 3.0, 2.0);
    g.set("section");
    g.move_click([1.3, 0.0, 0.7]);
    let sid = g.model().sections.first_key().unwrap();
    let sp = g.model().sections.get(sid).cloned().unwrap();
    assert!(
        sp.p[1].abs() < 1e-9
            && dist(sp.n, [0.0, -1.0, 0.0]) < 1e-9
            && g.model().active_section == Some(sid)
            && g.t.name == "select"
            && g.t.host.sel[0].t == "s"
    );
    assert!(sp.name == "Section 1" && g.last_msg().contains("Section 1 placed"));
    g.edit(|m| {
        let s = m.sections.get_mut(sid).unwrap();
        s.p = [s.p[0], 1.0, s.p[2]];
        Ok(())
    });
    let sc = scene_of(g.model());
    assert!(sc.faces().len() == 5 && !sc.faces().iter().any(|f| dist(f.n, [0.0, -1.0, 0.0]) < 1e-9), "the front face is cut away");
    let cam = g.t.host.cam.clone();
    let q = cam.project([2.0, 0.0, 1.0]).unwrap();
    assert!(
        pick(&sc, &cam, q[0], q[1], &[], 7.0).is_some_and(|s| s.t == "f")
            && sc.keep.unwrap().keep([2.0, 1.5, 1.0])
            && !sc.keep.unwrap().keep([2.0, 0.0, 1.0])
    );
    let q = cam.project([4.0, 0.0, 2.0]).unwrap();
    let r = infer(&sc, &cam, q[0], q[1], &InferOpts::default());
    assert!(!(r.kind == "end" && dist(r.p, [4.0, 0.0, 2.0]) < 1e-9), "no snapping to a corner that is cut away");
    g.edit(|m| {
        let s = m.sections.get_mut(sid).unwrap();
        s.n = mul(s.n, -1.0);
        Ok(())
    });
    let sp = g.model().sections.get(sid).cloned().unwrap();
    let l = section_layout(g.model(), sp.p, sp.n);
    let q = cam.project(mul(add(l.corners[0], l.corners[1]), 0.5)).unwrap();
    assert!(
        pick_section(g.model(), &cam, q[0], q[1], 6.0).is_some_and(|s| s.id == sid)
            && g.t.pick_at(&Ev::at(q[0], q[1])).is_some_and(|s| s.t == "s"),
        "clicking its outline selects it"
    );
    assert!(
        l.corners.iter().all(|c| (c[1] - 1.0).abs() < 1e-9)
            && l.corners.iter().map(|c| c[0]).fold(f64::MAX, f64::min) < 0.0
            && l.corners.iter().map(|c| c[0]).fold(f64::MIN, f64::max) > 4.0
    );
    g.set("section");
    g.move_click([4.0, 1.5, 1.0]);
    let sid2 = g.model().active_section.unwrap();
    assert!(
        sid2 != sid && g.model().sections.len() == 2 && g.model().sections.get(sid2).unwrap().name == "Section 2",
        "a second plane takes over"
    );
}

#[test]
fn drawing_axes() {
    let mut g = Rig::new();
    g.cam([4.0, -10.0, 12.0], [4.0, 3.0, 0.0], 40.0);
    assert!(g.model().axes_m.is_none() && g.t.axes().o == [0.0, 0.0, 0.0]);
    let c45 = std::f64::consts::FRAC_1_SQRT_2;
    g.set("axes");
    g.move_click([2.0, 1.0, 0.0]);
    g.move_click([2.0 + 2.0 * c45, 1.0 + 2.0 * c45, 0.0]);
    g.move_click([2.0 - 2.0 * c45, 1.0 + 2.0 * c45, 0.0]);
    let a = g.t.axes();
    assert!(
        g.model().axes_m.is_some()
            && dist(a.o, [2.0, 1.0, 0.0]) < 1e-9
            && dist(a.x, [c45, c45, 0.0]) < 1e-9
            && dist(a.y, [-c45, c45, 0.0]) < 1e-9
            && dist(a.z, Z) < 1e-9
            && g.t.name == "select"
    );
    g.set("rect");
    g.move_click([2.0, 1.0, 0.0]);
    g.move_to([2.5, 2.5, 0.0]);
    g.value("2,1");
    let pts: Vec<_> = g.root().verts.values().copied().collect();
    assert!(
        pts.iter().any(|p| dist(*p, [2.0 + 2.0 * c45, 1.0 + 2.0 * c45, 0.0]) < 1e-9)
            && pts.iter().any(|p| dist(*p, [2.0 - c45, 1.0 + c45, 0.0]) < 1e-9),
        "the rectangle runs along the new axes"
    );
    let empty = cc_m3d::Model::new();
    let sc = scene_of(&empty);
    let (origin, ground) = g.t.ground_opts();
    let q = g.at([2.0, 1.0, 0.0]);
    let r = infer(
        &sc,
        &g.t.host.cam,
        q.0,
        q.1,
        &InferOpts { axes: Some(Axes3 { x: a.x, y: a.y, z: a.z }), origin, ground, ..InferOpts::default() },
    );
    assert!(r.kind == "origin" && dist(r.p, [2.0, 1.0, 0.0]) < 1e-9, "the origin snap moved with the axes");
}

#[test]
fn terrain() {
    use cc_m3d_tools::terrain::{delaunay, terrain_from_contours, terrain_grid};
    let sq = [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0], [1.0, 1.0]];
    let dt = delaunay(&sq);
    assert!(
        dt.len() == 4
            && dt.iter().all(|[a, b, c]| {
                let (pa, pb, pc) = (sq[*a], sq[*b], sq[*c]);
                (pb[0] - pa[0]) * (pc[1] - pa[1]) - (pb[1] - pa[1]) * (pc[0] - pa[0]) > 0.0
            })
    );
    let rnd: Vec<[f64; 2]> = (0..400)
        .map(|i| {
            let i = f64::from(i);
            [cc_m3d::jsmath::fd_sin(i * 12.9898) * 43758.5453 % 50.0, cc_m3d::jsmath::fd_cos(i * 78.233) * 12345.678 % 50.0]
        })
        .collect();
    let tr = delaunay(&rnd);
    let empty = tr.iter().all(|[a, b, c]| {
        let ([ax, ay], [bx, by], [cx, cy]) = (rnd[*a], rnd[*b], rnd[*c]);
        rnd.iter().enumerate().all(|(k, [px, py])| {
            if k == *a || k == *b || k == *c {
                return true;
            }
            let m = [
                [ax - px, ay - py, (ax - px).powi(2) + (ay - py).powi(2)],
                [bx - px, by - py, (bx - px).powi(2) + (by - py).powi(2)],
                [cx - px, cy - py, (cx - px).powi(2) + (cy - py).powi(2)],
            ];
            let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
                + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
            det <= 1e-6
        })
    });
    assert!(empty, "no point inside any circumcircle");
    let mut g = Rig::new();
    let eids = g.edit(|m| {
        let r = m.root_mut().unwrap();
        let mut ids = Vec::new();
        for (rr, z) in [(10.0, 0.0), (7.0, 1.0), (4.0, 2.0)] {
            let pts: Vec<[f64; 3]> = (0..24)
                .map(|i| {
                    let a = (f64::from(i) * std::f64::consts::PI) / 12.0;
                    [rr * a.cos(), rr * a.sin(), z]
                })
                .collect();
            for i in 0..24 {
                ids.extend(line_nf(r, pts[i], pts[(i + 1) % 24]));
            }
        }
        Ok(ids)
    });
    let td = g.edit(|m| Ok(terrain_from_contours(m, "root", &eids, "Terrain").unwrap()));
    let d = g.model().def(&td).unwrap();
    let tb = g.model().def_bbox(&td).unwrap();
    assert!(
        d.faces.len() == 2 * 72 - 2 - 24 && d.faces.values().all(|f| f.loops[0].len() == 3) && c6(tb.min[2], 0.0) && c6(tb.max[2], 2.0)
    );
    assert_eq!(d.edges.values().filter(|e| !e.soft).count(), 24, "only the rim stays hard");
    let gd = g.edit(|m| Ok(terrain_grid(m, 10.0, 6.0, 1.0, O, "Terrain")));
    let gi = g.edit(|m| Ok(cc_m3d::model::place(m.root_mut().unwrap(), &gd, &translation([20.0, 0.0, 0.0]))));
    assert!(g.model().def(&gd).unwrap().faces.len() == 120 && g.model().def(&gd).unwrap().verts.len() == 77);
    g.cam([25.0, -12.0, 14.0], [25.0, 3.0, 0.0], 60.0);
    g.t.host.path = vec![gi];
    g.set("sculpt");
    g.value("2");
    let q = g.at([25.0, 3.0, 0.0]);
    g.move_xy(q.0, q.1);
    g.down_xy(q.0, q.1, 1.0);
    g.up_xy(q.0, q.1);
    g.value("1.5");
    let z_at = |g: &Rig, x: f64, y: f64| g.model().def(&gd).unwrap().verts.values().find(|p| c6(p[0], x) && c6(p[1], y)).unwrap()[2];
    assert!(
        close(z_at(&g, 5.0, 3.0), 1.5, 1e-9)
            && close(z_at(&g, 6.0, 3.0), 0.75, 1e-9)
            && close(z_at(&g, 7.0, 3.0), 0.0, 1e-9)
            && g.last_msg().contains("Raised by 1.5 m under a 2 m brush")
    );
    assert!(g.model().def(&gd).unwrap().faces.values().all(|f| f.loops[0].len() == 3));
    let before = z_at(&g, 4.0, 2.0);
    let n0 = g.model().undo_stack.len();
    let q2 = g.at([24.0, 2.0, before]);
    g.move_xy(q2.0, q2.1);
    g.down_xy(q2.0, q2.1, 1e6);
    g.move_xy(q2.0, q2.1 - 40.0);
    g.up_xy(q2.0, q2.1 - 40.0);
    assert!(z_at(&g, 4.0, 2.0) > before + 0.05 && g.model().undo_stack.len() == n0 + 1, "dragging up raises the ground, one undo step");
    g.m().undo().unwrap();
    assert!(close(z_at(&g, 4.0, 2.0), before, 1e-12), "undo puts it back");
}

#[test]
fn components_that_stretch_or_repeat() {
    use cc_m3d_tools::dynamic::{parts_of, stretch_def};
    use cc_m3d_tools::starter::build_starter;
    let mut g = Rig::new();
    let tbl = g.edit(|m| Ok(build_starter(m, "table", Some("Table"))?.unwrap()));
    let ti = g.edit(|m| Ok(cc_m3d::model::place(m.root_mut().unwrap(), &tbl, &IDENT)));
    let big = g.edit(|m| {
        let d = stretch_def(m, &tbl, [2.4, 1.0, 0.76], "Table")?;
        m.root_mut().unwrap().insts.get_mut(ti).unwrap().def = d.clone();
        m.purge();
        Ok(d)
    });
    let parts = parts_of(g.model(), &big);
    let top = parts.iter().find(|p| p.bx.max[2] > 0.75).unwrap();
    assert!(close(top.bx.max[0] - top.bx.min[0], 2.4, 1e-9) && close(top.bx.max[1] - top.bx.min[1], 1.0, 1e-9), "the top is 2.4 × 1 m");
    let legs: Vec<_> = parts.iter().filter(|p| p.bx.max[2] <= 0.72 + 1e-9).collect();
    assert!(
        legs.len() == 4
            && legs.iter().all(|p| close(p.bx.max[0] - p.bx.min[0], 0.06, 1e-9))
            && legs.iter().any(|p| close(p.bx.min[0], 0.05, 1e-9))
            && legs.iter().any(|p| close(p.bx.max[0], 2.35, 1e-9))
    );
    let bd = g.model().def(&big).unwrap();
    assert!(
        g.model().def(&tbl).is_some()
            && bd.dyn_.as_ref().is_some_and(|d| d.src == tbl)
            && bd.insts.len() == g.model().def(&tbl).unwrap().insts.len()
    );
    let sh = g.edit(|m| Ok(build_starter(m, "bookshelf", Some("Shelf"))?.unwrap()));
    let tall = g.edit(|m| stretch_def(m, &sh, [0.8, 0.3, 3.0], "Tall"));
    let mut zs: Vec<f64> =
        parts_of(g.model(), &tall).iter().filter(|p| close(p.bx.max[2] - p.bx.min[2], 0.02, 1e-9)).map(|p| p.bx.min[2]).collect();
    zs.sort_by(f64::total_cmp);
    assert!(zs.len() == 6 && close(zs[5], 2.94, 0.02) && zs.windows(2).all(|w| w[1] - w[0] > 0.5), "6 shelves spread over 3 m");
    let st = g.edit(|m| Ok(build_starter(m, "stair", Some("Stair"))?.unwrap()));
    let st2 = g.edit(|m| stretch_def(m, &st, [1.2, 5.4, 3.6], "Stair"));
    let sb = g.model().def_bbox(&st2).unwrap();
    assert!(close(sb.max[1], 5.4, 1e-9) && close(sb.max[2], 3.6, 1e-9) && close(sb.max[0], 1.2, 1e-9), "a stair scales");
    let rail = g.edit(|m| {
        let c = m.new_def("component", Some("Railing"));
        let bar = m.new_def("group", Some("Bar"));
        let bd = m.def_mut(&bar).unwrap();
        add_polygon(bd, &[[0.0, 0.0, 0.95], [4.0, 0.0, 0.95], [4.0, 0.05, 0.95], [0.0, 0.05, 0.95]], None)?;
        let f = bd.faces.first_key().unwrap();
        push_pull(bd, f, 0.05);
        cc_m3d::model::place(m.def_mut(&c).unwrap(), &bar, &IDENT);
        let post = m.new_def("group", Some("Post"));
        let pd = m.def_mut(&post).unwrap();
        add_polygon(pd, &[[0.0, 0.0, 0.0], [0.05, 0.0, 0.0], [0.05, 0.05, 0.0], [0.0, 0.05, 0.0]], None)?;
        let f = pd.faces.first_key().unwrap();
        push_pull(pd, f, 0.95);
        let cd = m.def_mut(&c).unwrap();
        let pi = cc_m3d::model::place(cd, &post, &IDENT);
        cd.insts.get_mut(pi).unwrap().rule =
            Some(cc_m3d::def::Rule { x: Some("repeat".into()), sx: Some(1.0), ..cc_m3d::def::Rule::default() });
        cc_m3d::model::place(cd, &post, &translation([3.95, 0.0, 0.0]));
        Ok(c)
    });
    let r10 = g.edit(|m| stretch_def(m, &rail, [10.0, 0.05, 1.0], "Railing"));
    let posts = parts_of(g.model(), &r10).iter().filter(|p| close(p.bx.max[2], 0.95, 1e-9) && close(p.bx.min[2], 0.0, 1e-9)).count();
    assert_eq!(posts, 11, "posts repeat every metre");
}
