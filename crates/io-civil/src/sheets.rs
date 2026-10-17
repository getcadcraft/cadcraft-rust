//! pp-sheets.js (plan and profile sheets, 0.4 V15), section-sheets.js (cross-section sheets, 0.4 V17) and the staking
//! table pages of staking.js (0.4 V23): page layouts for plot.js `toPdfPages`.

use crate::plot::{Fill, MARGIN, PText, Scale, Sheet, Stroke, TITLE_H, View, day_of, orient, paper_by, rgb_of, width_of};
use cc_civil::alignment::Alignment;
use cc_civil::i18n::t;
use cc_civil::js::{J, JsMath, hypot, max, max_of, min, num_to_string, round, round_fixed};
use cc_civil::profile::{Profile, StaZ};
use cc_civil::sections::{SectionSet, z_at};
use cc_civil::staking::{Row, cut_fill_text, offset_text};
use cc_civil::tr;
use cc_civil::units::{fixed, format_station, pct, unit_of};
use serde_json::{Value, json};

const INK: &str = "#000000";

fn st(color: &str, width: f64, pts: Vec<(f64, f64)>) -> Stroke {
    Stroke { color: color.into(), width, pts, closed: None, arc: None, dash: None }
}
#[allow(clippy::too_many_arguments)]
fn tx(color: &str, x: f64, y: f64, h: f64, rot: f64, align: &str, middle: Option<bool>, text: String) -> PText {
    PText { color: color.into(), x, y, h, rot, text, align: align.into(), middle }
}

// ---------------------------------------------------------------------------------------------------------------
// plan and profile sheets

struct Layout {
    pw: f64,
    ph: f64,
    plan: (f64, f64, f64, f64),
    profile: (f64, f64, f64, f64),
}
fn layout(paper: &str, landscape: bool) -> Layout {
    let (pw, ph) = orient(&paper_by(paper), landscape);
    let ix0 = MARGIN + 16.0;
    let ix1 = pw - MARGIN - 16.0;
    let top = ph - MARGIN - 3.0;
    let bottom = MARGIN + TITLE_H + 9.0;
    let span = top - bottom;
    let profile = (ix0, bottom, ix1, bottom + span * 0.4);
    let plan = (ix0, profile.3 + 10.0, ix1, top);
    Layout { pw, ph, plan, profile }
}

/// The design profile across [a, b].
fn design_in(profile: Option<&Profile>, a: f64, b: f64) -> Vec<(f64, f64)> {
    let Some(p) = profile.filter(|p| p.ok()) else { return Vec::new() };
    let mut st: Vec<f64> = vec![a];
    if b != a {
        st.push(b);
    }
    for q in p.sample(max((b - a) / 300.0, 0.05)) {
        if q.sta > a && q.sta < b && !st.contains(&q.sta) {
            st.push(q.sta);
        }
    }
    cc_civil::js::sort_by(&mut st, |m, n| m - n);
    st.into_iter().filter_map(|sta| p.elevation_at(sta).map(|z| (sta, z))).collect()
}

pub const VEXES: [f64; 5] = [10.0, 5.0, 4.0, 2.0, 1.0];

/// `sheetPlan(alignment, { paper, landscape, scale, from, to, units })` → (length, stations, layout).
pub fn sheet_plan(al: &Alignment, paper: &str, landscape: bool, scale: &Scale, from: f64, to: f64, units: &str) -> (f64, Vec<(f64, f64)>) {
    let l = layout(paper, landscape);
    let a0 = max(al.start, if from.is_finite() { from } else { al.start });
    let a1 = min(al.end, if to.is_finite() { to } else { al.end });
    let fits = (l.plan.2 - l.plan.0) / scale.mm_per_unit;
    let r = if units == "m" {
        if fits > 200.0 {
            20.0
        } else if fits > 25.0 {
            5.0
        } else {
            0.0
        }
    } else if fits > 500.0 {
        50.0
    } else if fits > 50.0 {
        10.0
    } else {
        0.0
    };
    let length = if r != 0.0 { (fits / r).floor() * r } else { fits };
    let mut stations = Vec::new();
    let mut a = a0;
    while a < a1 - 1e-6 && stations.len() < 500 {
        stations.push((a, min(a1, a + length)));
        a += length;
    }
    (length, stations)
}

/// `rotation(al, a, b)`.
pub fn rotation(al: &Alignment, a: f64, b: f64) -> f64 {
    match (al.point_at(a), al.point_at(b)) {
        (Some(p), Some(q)) => (q.y - p.y).jatan2(q.x - p.x),
        _ => f64::NAN,
    }
}

/// `planProfileSheets(scene, {...})` → (sheets, vex used, notes, (length, stations)).
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
#[allow(clippy::arithmetic_side_effects)] // bounded counters and indices (page numbers, padding, digit math)
pub fn plan_profile_sheets(
    items: &[Value],
    al: &Alignment,
    eg: &[StaZ],
    profile: Option<&Profile>,
    paper: &str,
    landscape: bool,
    scale: &Scale,
    vex: f64,
    units: &str,
    title: &str,
    name: &str,
    from: f64,
    to: f64,
    made_ms: f64,
) -> (Vec<Sheet>, f64, Vec<String>, (f64, Vec<(f64, f64)>)) {
    let (length, stations) = sheet_plan(al, paper, landscape, scale, from, to, units);
    let lay = layout(paper, landscape);
    let (pw_, ph_) = (lay.pw, lay.ph);
    let pwin = lay.plan;
    let vw = lay.profile;
    let s = scale.mm_per_unit;
    let mut notes = Vec::new();
    let n_sheets = stations.len();
    let h_avail = vw.3 - vw.1 - 8.0;
    let z_range = |(a, b): (f64, f64)| -> Option<(f64, f64)> {
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for p in eg {
            if let Some(z) = p.z
                && p.sta >= a - 1e-9
                && p.sta <= b + 1e-9
            {
                lo = min(lo, z);
                hi = max(hi, z);
            }
        }
        for (_, z) in design_in(profile, a, b) {
            lo = min(lo, z);
            hi = max(hi, z);
        }
        lo.is_finite().then_some((lo, hi))
    };
    let ranges: Vec<Option<(f64, f64)>> = stations.iter().map(|&r| z_range(r)).collect();
    let need = max_of(std::iter::once(1e-9).chain(ranges.iter().flatten().map(|(lo, hi)| hi - lo)));
    let want = if VEXES.contains(&vex) { vex } else { 10.0 };
    let used = VEXES.iter().copied().filter(|v| *v <= want).find(|v| need * s * v <= h_avail).unwrap_or(1.0);
    if used != want {
        notes.push(tr!("pp.vex.lowered", want = want, used = used));
    }
    if need * s * used > h_avail {
        notes.push(t("pp.vex.clipped", &[]));
    }
    let sv = s * used;
    let aci_of = |layer: &str| crate::scene::LAYERS.iter().find(|l| l.0 == layer).map(|l| l.2);
    let n = |v: &Value, k: &str| cc_civil::js::numf(v.get(k));
    let mut sheets = Vec::new();
    for (k, &(a, b)) in stations.iter().enumerate() {
        let th = rotation(al, a, b);
        let c = (-th).jcos();
        let sn = (-th).jsin();
        let Some(pa) = al.point_at(a) else { continue };
        let rot = |x: f64, y: f64| ((x - pa.x) * c - (y - pa.y) * sn, (x - pa.x) * sn + (y - pa.y) * c);
        let (mut ylo, mut yhi) = (f64::INFINITY, f64::NEG_INFINITY);
        for i in 0..=40 {
            if let Some(p) = al.point_at(a + ((b - a) * f64::from(i)) / 40.0) {
                let q = rot(p.x, p.y);
                ylo = min(ylo, q.1);
                yhi = max(yhi, q.1);
            }
        }
        let ymid = (ylo + yhi) / 2.0;
        let cy = (pwin.1 + pwin.3) / 2.0;
        let txf = |x: f64, y: f64| {
            let q = rot(x, y);
            (pwin.0 + q.0 * s, cy + (q.1 - ymid) * s)
        };
        let inv = |xx: f64, yy: f64| {
            let qx = (xx - pwin.0) / s;
            let qy = (yy - cy) / s + ymid;
            (pa.x + qx * th.jcos() - qy * th.jsin(), pa.y + qx * th.jsin() + qy * th.jcos())
        };
        let corners = [inv(pwin.0, pwin.1), inv(pwin.2, pwin.1), inv(pwin.2, pwin.3), inv(pwin.0, pwin.3)];
        let wb = (
            corners.iter().map(|q| q.0).fold(f64::INFINITY, min),
            corners.iter().map(|q| q.0).fold(f64::NEG_INFINITY, max),
            corners.iter().map(|q| q.1).fold(f64::INFINITY, min),
            corners.iter().map(|q| q.1).fold(f64::NEG_INFINITY, max),
        );
        let hit = |pts: &[(f64, f64)]| {
            let (mut x0, mut x1, mut y0, mut y1) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
            for p in pts {
                x0 = min(x0, p.0);
                x1 = max(x1, p.0);
                y0 = min(y0, p.1);
                y1 = max(y1, p.1);
            }
            !(x1 < wb.0 || x0 > wb.1 || y1 < wb.2 || y0 > wb.3)
        };
        let mut plan_v = View { clip: pwin, ..View::default() };
        for it in items {
            let layer = cc_civil::js::string_of(it.get("layer"));
            let color = aci_of(&layer).and_then(rgb_of).unwrap_or(INK).to_string();
            let width = width_of(&layer).unwrap_or(0.18);
            match it.get("kind").and_then(Value::as_str) {
                Some("poly") => {
                    let pts: Vec<(f64, f64)> = cc_civil::js::arr(it.get("pts")).iter().map(|p| (n(p, "x"), n(p, "y"))).collect();
                    if !pts.is_empty() && hit(&pts) {
                        plan_v.strokes.push(Stroke {
                            color,
                            width,
                            pts: pts.iter().map(|p| txf(p.0, p.1)).collect(),
                            closed: Some(cc_civil::js::truthy(it.get("closed"))),
                            arc: None,
                            dash: None,
                        });
                    }
                }
                Some("arc") => {
                    let cc = it.get("c").cloned().unwrap_or(Value::Null);
                    let (cxx, cyy, r) = (n(&cc, "x"), n(&cc, "y"), n(it, "r"));
                    if hit(&[(cxx - r, cyy - r), (cxx + r, cyy + r)]) {
                        let (x, y) = txf(cxx, cyy);
                        plan_v.strokes.push(Stroke {
                            color,
                            width,
                            pts: Vec::new(),
                            closed: None,
                            arc: Some((x, y, r * s, n(it, "a0") - th, n(it, "a1") - th)),
                            dash: None,
                        });
                    }
                }
                Some("point") => {
                    let (x, y) = (n(it, "x"), n(it, "y"));
                    if hit(&[(x, y)]) {
                        let (px, py) = txf(x, y);
                        plan_v.points.push((color, px, py));
                    }
                }
                Some("text") => {
                    let (x, y) = (n(it, "x"), n(it, "y"));
                    if hit(&[(x, y)]) {
                        let (px, py) = txf(x, y);
                        let r0 = n(it, "rot");
                        let r0 = if r0 != 0.0 && !r0.is_nan() { r0 } else { 0.0 };
                        plan_v.texts.push(PText {
                            color,
                            x: px,
                            y: py,
                            h: n(it, "mm"),
                            rot: r0 - th,
                            text: cc_civil::js::string_of(it.get("text")),
                            align: cc_civil::js::string_of(it.get("align")),
                            middle: it.get("middle").and_then(Value::as_bool),
                        });
                    }
                }
                _ => {}
            }
        }
        // match lines
        let half = (pwin.3 - pwin.1) / 2.0 / s;
        for (sta, other) in [(a, k), (b, k + 2)] {
            if other < 1 || other > n_sheets || (sta == a && k == 0) || (sta == b && k + 1 == n_sheets) {
                continue;
            }
            let Some(p) = al.point_at(sta) else { continue };
            let nx = p.dir.jsin();
            let ny = -p.dir.jcos();
            let e1 = txf(p.x - nx * half, p.y - ny * half);
            let e2 = txf(p.x + nx * half, p.y + ny * half);
            plan_v.strokes.push(Stroke {
                color: INK.into(),
                width: 0.7,
                pts: vec![e1, e2],
                closed: None,
                arc: None,
                dash: Some(vec![6.0, 1.5, 1.0, 1.5]),
            });
            let (up, dn) = if e1.1 > e2.1 { (e1, e2) } else { (e2, e1) };
            let ang = (up.1 - dn.1).jatan2(up.0 - dn.0);
            let at = (dn.0 + (up.0 - dn.0) * 0.95, dn.1 + (up.1 - dn.1) * 0.95);
            plan_v.texts.push(tx(
                INK,
                at.0 + if sta == a { 2.2 } else { -2.2 },
                at.1,
                2.2,
                ang,
                "right",
                Some(true),
                tr!("pp.match", sta = format_station(sta, units, None), sheet = other),
            ));
        }
        // the profile
        let mut pv = View { clip: (vw.0 - 15.0, vw.1 - 8.0, vw.2 + 15.0, vw.3 + 2.0), ..View::default() };
        let r = ranges.get(k).copied().flatten().unwrap_or((0.0, 1.0));
        let zmid = (r.0 + r.1) / 2.0;
        let z_span = (vw.3 - vw.1) / sv;
        let z_step_raw = 6.0 / sv;
        let z_step = [0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0].iter().copied().find(|v| *v >= z_step_raw).unwrap_or(100.0);
        let z0 = ((zmid - z_span / 2.0) / z_step).floor() * z_step;
        let xf = |sta: f64| vw.0 + (sta - a) * s;
        let yf = |z: f64| vw.1 + (z - z0) * sv;
        let x_end = xf(b);
        let s_step_raw = 12.0 / s;
        let s_step =
            [5.0, 10.0, 20.0, 25.0, 50.0, 100.0, 200.0, 500.0, 1000.0].iter().copied().find(|v| *v >= s_step_raw).unwrap_or(1000.0);
        let mut stn = (a / s_step - 1e-9).ceil() * s_step;
        let mut guard = 0u32;
        while stn <= b + 1e-9 && guard < 100_000 {
            pv.strokes.push(st("#b8b8b8", 0.1, vec![(xf(stn), vw.1), (xf(stn), vw.3)]));
            pv.texts.push(tx(INK, xf(stn), vw.1 - 4.0, 1.8, 0.0, "center", None, format_station(stn, units, Some(0))));
            stn += s_step;
            guard = guard.saturating_add(1);
        }
        let mut z = z0;
        guard = 0;
        while z <= z0 + z_span + 1e-9 && guard < 100_000 {
            let y = yf(z);
            if y > vw.3 + 1e-6 {
                break;
            }
            pv.strokes.push(st("#b8b8b8", 0.1, vec![(vw.0, y), (x_end, y)]));
            let lbl = fixed(z, if z_step < 1.0 { 1 } else { 0 }).replace(',', "");
            pv.texts.push(tx(INK, vw.0 - 1.5, y, 1.8, 0.0, "right", Some(true), lbl.clone()));
            pv.texts.push(tx(INK, x_end + 1.5, y, 1.8, 0.0, "left", Some(true), lbl));
            z += z_step;
            guard = guard.saturating_add(1);
        }
        let mut frame = st(INK, 0.35, vec![(vw.0, vw.1), (x_end, vw.1), (x_end, vw.3), (vw.0, vw.3)]);
        frame.closed = Some(true);
        pv.strokes.push(frame);
        let mut run: Vec<(f64, f64)> = Vec::new();
        let flush = |run: &mut Vec<(f64, f64)>, pv: &mut View| {
            if run.len() >= 2 {
                pv.strokes.push(Stroke {
                    color: "#606060".into(),
                    width: 0.3,
                    pts: std::mem::take(run),
                    closed: None,
                    arc: None,
                    dash: Some(vec![2.0, 1.0]),
                });
            } else {
                run.clear();
            }
        };
        for p in eg {
            if p.sta < a - 1e-9 || p.sta > b + 1e-9 {
                continue;
            }
            match p.z {
                None => flush(&mut run, &mut pv),
                Some(z) => run.push((xf(p.sta), yf(z))),
            }
        }
        flush(&mut run, &mut pv);
        if let Some(pr) = profile.filter(|p| p.ok()) {
            let q = design_in(Some(pr), a, b);
            if q.len() >= 2 {
                pv.strokes.push(st("#c00000", 0.6, q.iter().map(|v| (xf(v.0), yf(v.1))).collect()));
            }
            for g in &pr.grades {
                let m = (max(g.from, a) + min(g.to, b)) / 2.0;
                if min(g.to, b) - max(g.from, a) < 30.0 / s {
                    continue;
                }
                let Some(zm) = pr.elevation_at(m) else { continue };
                pv.texts.push(tx("#c00000", xf(m), yf(zm) + 2.5, 1.8, (g.g * used).jatan(), "center", None, pct(g.g, 2)));
            }
            for cv in &pr.curves {
                if cv.pvi < a || cv.pvi > b {
                    continue;
                }
                pv.texts.push(tx(
                    INK,
                    xf(cv.pvi),
                    yf(cv.z_pvi) + if cv.crest { 5.0 } else { -6.0 },
                    1.8,
                    0.0,
                    "center",
                    None,
                    tr!("pp.vc", sta = format_station(cv.pvi, units, None), z = fixed(cv.z_pvi, 2), L = fixed(cv.l, 2), K = fixed(cv.k, 1)),
                ));
            }
        }
        // frame and title strip
        let mut chrome = Vec::new();
        let mut tt = Vec::new();
        let mut f0 = st(INK, 0.5, vec![(MARGIN, MARGIN), (pw_ - MARGIN, MARGIN), (pw_ - MARGIN, ph_ - MARGIN), (MARGIN, ph_ - MARGIN)]);
        f0.closed = Some(true);
        chrome.push(f0);
        chrome.push(st(INK, 0.35, vec![(MARGIN, MARGIN + TITLE_H), (pw_ - MARGIN, MARGIN + TITLE_H)]));
        let mut f1 = st(INK, 0.25, vec![(pwin.0, pwin.1), (pwin.2, pwin.1), (pwin.2, pwin.3), (pwin.0, pwin.3)]);
        f1.closed = Some(true);
        chrome.push(f1);
        let h_label = scale.label.clone();
        let v_label = if units == "m" {
            format!("1:{}", num_to_string(round(1000.0 / sv)))
        } else {
            format!("1\" = {}'", num_to_string(round_fixed(25.4 / sv, 1)))
        };
        let ttl = if title.is_empty() { name.to_string() } else { title.to_string() };
        tt.push(tx(INK, MARGIN + 4.0, MARGIN + TITLE_H - 6.0, 4.0, 0.0, "left", None, ttl.clone()));
        tt.push(tx(
            INK,
            MARGIN + 4.0,
            MARGIN + 3.5,
            2.2,
            0.0,
            "left",
            None,
            tr!(
                "pp.strip",
                name = name,
                a = format_station(a, units, None),
                b = format_station(b, units, None),
                h = h_label,
                v = v_label,
                vex = used
            ),
        ));
        tt.push(tx(INK, pw_ - MARGIN - 4.0, MARGIN + TITLE_H - 6.0, 4.0, 0.0, "right", None, tr!("pp.sheet", n = k + 1, N = n_sheets)));
        tt.push(tx(INK, pw_ - MARGIN - 4.0, MARGIN + 3.5, 2.2, 0.0, "right", None, tr!("pp.made", date = day_of(made_ms))));
        let na = (pwin.2 - 10.0, pwin.3 - 14.0);
        let nd = (th.jsin(), th.jcos());
        let (nx, ny) = (-nd.1, nd.0);
        chrome.push(st(INK, 0.35, vec![(na.0 - nd.0 * 6.0, na.1 - nd.1 * 6.0), (na.0 + nd.0 * 8.0, na.1 + nd.1 * 8.0)]));
        chrome.push(st(
            INK,
            0.35,
            vec![
                (na.0 + nd.0 * 3.0 + nx * 3.0, na.1 + nd.1 * 3.0 + ny * 3.0),
                (na.0 + nd.0 * 8.0, na.1 + nd.1 * 8.0),
                (na.0 + nd.0 * 3.0 - nx * 3.0, na.1 + nd.1 * 3.0 - ny * 3.0),
            ],
        ));
        tt.push(tx(INK, na.0 + nd.0 * 11.0, na.1 + nd.1 * 11.0, 3.0, 0.0, "center", Some(true), t("pp.north", &[])));
        sheets.push(Sheet {
            w: pw_,
            h: ph_,
            name: format!("{} {}", ttl, k + 1),
            views: Some(vec![plan_v, pv]),
            single: View::default(),
            chrome,
            chrome_texts: tt,
            extra: vec![("stations".into(), json!([a.j(), b.j()])), ("theta".into(), th.j())],
        });
    }
    let _ = hypot;
    (sheets, used, notes, (length, stations))
}

// ---------------------------------------------------------------------------------------------------------------
// cross-section sheets

pub const SHEET_VEXES: [f64; 4] = [10.0, 5.0, 2.0, 1.0];

/// `sectionSheets(set, {...})` → (sheets, cell (w, h, cols, rows) | None, perPage, vex, notes).
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
#[allow(clippy::arithmetic_side_effects)] // bounded counters and indices (page numbers, padding, digit math)
pub fn section_sheets(
    set: &SectionSet,
    paper: &str,
    landscape: bool,
    scale: &Scale,
    vex: f64,
    units: &str,
    title: &str,
    name: &str,
    from: f64,
    to: f64,
    made_ms: f64,
) -> (Vec<Sheet>, Option<(f64, f64, f64, f64)>, f64, f64, Vec<String>) {
    let secs: Vec<&cc_civil::sections::Sec> = set.sections.iter().filter(|x| x.sta >= from - 1e-9 && x.sta <= to + 1e-9).collect();
    let mut notes = Vec::new();
    if secs.is_empty() {
        return (Vec::new(), None, 0.0, vex, vec![t("xs.none", &[])]);
    }
    let (pw, ph) = orient(&paper_by(paper), landscape);
    let area = (MARGIN + 4.0, MARGIN + TITLE_H + 4.0, pw - MARGIN - 4.0, ph - MARGIN - 4.0);
    let s = scale.mm_per_unit;
    let left = secs.first().map_or(0.0, |x| x.left);
    let right = secs.first().map_or(0.0, |x| x.right);
    let relief = |x: &cc_civil::sections::Sec| -> Option<(f64, f64)> {
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for r in x.ground.iter().chain(x.design.iter()) {
            for q in r {
                lo = min(lo, q.z);
                hi = max(hi, q.z);
            }
        }
        lo.is_finite().then_some((lo, hi))
    };
    let rel: Vec<Option<(f64, f64)>> = secs.iter().map(|x| relief(x)).collect();
    let max_rel = max_of(std::iter::once(1.0).chain(rel.iter().flatten().map(|(lo, hi)| hi - lo)));
    let (pad_x, pad_top, pad_bot) = (14.0, 6.0, 13.0);
    let cw = (left + right) * s + pad_x + 4.0;
    if cw > area.2 - area.0 {
        notes.push(t("xs.wide", &[]));
    }
    let fit_v = |v: f64| max_rel * 1.2 * s * v + pad_top + pad_bot <= area.3 - area.1;
    let want = if SHEET_VEXES.contains(&vex) { vex } else { 5.0 };
    let used = SHEET_VEXES.iter().copied().filter(|v| *v <= want).find(|v| fit_v(*v)).unwrap_or(1.0);
    if used != want {
        notes.push(tr!("xs.vex", want = want, used = used));
    }
    let sv = s * used;
    let ch = max_rel * 1.2 * sv + pad_top + pad_bot;
    let cols = max(1.0, ((area.2 - area.0) / cw).floor());
    let rows = max(1.0, ((area.3 - area.1) / ch).floor());
    let per_page = cols * rows;
    let pages = (secs.len() as f64 / per_page).ceil();
    let o_step_raw = 10.0 / s;
    let o_step = [1.0, 2.0, 5.0, 10.0, 20.0, 25.0, 50.0, 100.0, 200.0].iter().copied().find(|v| *v >= o_step_raw).unwrap_or(200.0);
    let z_step_raw = 5.0 / sv;
    let z_step = [0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0].iter().copied().find(|v| *v >= z_step_raw).unwrap_or(50.0);
    let h_label = scale.label.clone();
    let v_label = if units == "m" {
        format!("1:{}", num_to_string(round(1000.0 / sv)))
    } else {
        format!("1\" = {}'", num_to_string(round_fixed(25.4 / sv, 1)))
    };
    let area_u = unit_of(units).area;
    let ppu = cc_civil::js::usize_of(per_page).unwrap_or(1).max(1);
    let colsu = cc_civil::js::usize_of(cols).unwrap_or(1).max(1);
    let pages_u = cc_civil::js::usize_of(pages).unwrap_or(0);
    let mut sheets = Vec::new();
    for pg in 0..pages_u {
        let mut strokes = Vec::new();
        let mut texts = Vec::new();
        let mut fills = Vec::new();
        let here: Vec<&&cc_civil::sections::Sec> = secs.iter().skip(pg * ppu).take(ppu).collect();
        for (k, x) in here.iter().enumerate() {
            let col = (k % colsu) as f64;
            let row = (k / colsu) as f64;
            let cx0 = area.0 + col * cw + pad_x;
            let cy1 = area.3 - row * ch - pad_top;
            let cy0 = cy1 - (ch - pad_top - pad_bot);
            let r = rel.get(pg * ppu + k).copied().flatten().unwrap_or((0.0, 1.0));
            let zmid = (r.0 + r.1) / 2.0;
            let z_span = (cy1 - cy0) / sv;
            let z0 = ((zmid - z_span / 2.0) / z_step).floor() * z_step;
            let xf = |o: f64| cx0 + (o + left) * s;
            let yf = |z: f64| cy0 + (z - z0) * sv;
            let xr = xf(right);
            let mut o = (-left / o_step).ceil() * o_step;
            let mut guard = 0u32;
            while o <= right + 1e-9 && guard < 100_000 {
                strokes.push(st("#c4c4c4", 0.08, vec![(xf(o), cy0), (xf(o), cy1)]));
                texts.push(tx(
                    INK,
                    xf(o),
                    cy0 - 2.6,
                    1.3,
                    0.0,
                    "center",
                    None,
                    format!("{}{}", if o > 0.0 { "+" } else { "" }, fixed(o, 0)),
                ));
                o += o_step;
                guard = guard.saturating_add(1);
            }
            let mut z = z0;
            guard = 0;
            while z <= z0 + z_span + 1e-9 && guard < 100_000 {
                let y = yf(z);
                if y > cy1 + 1e-6 {
                    break;
                }
                strokes.push(st("#c4c4c4", 0.08, vec![(cx0, y), (xr, y)]));
                texts.push(tx(
                    INK,
                    cx0 - 1.0,
                    y,
                    1.3,
                    0.0,
                    "right",
                    Some(true),
                    fixed(z, if z_step < 1.0 { 1 } else { 0 }).replace(',', ""),
                ));
                z += z_step;
                guard = guard.saturating_add(1);
            }
            let mut fr = st(INK, 0.25, vec![(cx0, cy0), (xr, cy0), (xr, cy1), (cx0, cy1)]);
            fr.closed = Some(true);
            strokes.push(fr);
            strokes.push(Stroke {
                color: "#c00000".into(),
                width: 0.15,
                pts: vec![(xf(0.0), cy0), (xf(0.0), cy1)],
                closed: None,
                arc: None,
                dash: Some(vec![3.0, 1.0, 0.5, 1.0]),
            });
            let mut os: Vec<f64> = Vec::new();
            for r2 in x.ground.iter().chain(x.design.iter()) {
                for q in r2 {
                    if !os.iter().any(|&v| v == q.o || (v.is_nan() && q.o.is_nan())) {
                        os.push(q.o);
                    }
                }
            }
            cc_civil::js::sort_by(&mut os, |a, b| a - b);
            for part in &x.parts {
                let mut xs = vec![part.o0];
                xs.extend(os.iter().copied().filter(|&o| o > part.o0 && o < part.o1));
                xs.push(part.o1);
                // zAt may give null: the JS then computes with 0 (null in arithmetic)
                let top: Vec<(f64, f64)> = xs.iter().map(|&o| (xf(o), yf(z_at(&x.design, o).unwrap_or(0.0)))).collect();
                let bot: Vec<(f64, f64)> = xs.iter().rev().map(|&o| (xf(o), yf(z_at(&x.ground, o).unwrap_or(0.0)))).collect();
                if top.iter().chain(bot.iter()).all(|q| q.1.is_finite()) {
                    let mut pts = top;
                    pts.extend(bot);
                    fills.push(Fill { color: if part.cut { "#f3c9a6".into() } else { "#bfe3c4".into() }, pts });
                }
            }
            for run in &x.ground {
                strokes.push(Stroke {
                    color: "#606060".into(),
                    width: 0.25,
                    pts: run.iter().map(|q| (xf(q.o), yf(q.z))).collect(),
                    closed: None,
                    arc: None,
                    dash: Some(vec![1.5, 0.8]),
                });
            }
            if !x.same {
                for run in &x.design {
                    strokes.push(st("#c00000", 0.4, run.iter().map(|q| (xf(q.o), yf(q.z))).collect()));
                }
            }
            for m in &x.marks {
                if let Some(mz) = m.z.filter(|z| z.is_finite()) {
                    let q = (xf(m.o), yf(mz));
                    strokes.push(st(
                        "#8a6200",
                        0.25,
                        vec![(q.0 - 0.6, q.1 - 0.6), (q.0 + 0.6, q.1 + 0.6), (q.0 - 0.6, q.1 + 0.6), (q.0 + 0.6, q.1 - 0.6)],
                    ));
                }
            }
            texts.push(tx(
                INK,
                (cx0 + xr) / 2.0,
                cy0 - 7.5,
                1.8,
                0.0,
                "center",
                None,
                tr!("xs.cell", sta = format_station(x.sta, units, None), cut = fixed(x.cut, 2), fill = fixed(x.fill, 2), u = area_u),
            ));
        }
        let mut chrome = Vec::new();
        let mut tt = Vec::new();
        let mut f0 = st(INK, 0.5, vec![(MARGIN, MARGIN), (pw - MARGIN, MARGIN), (pw - MARGIN, ph - MARGIN), (MARGIN, ph - MARGIN)]);
        f0.closed = Some(true);
        chrome.push(f0);
        chrome.push(st(INK, 0.35, vec![(MARGIN, MARGIN + TITLE_H), (pw - MARGIN, MARGIN + TITLE_H)]));
        let ttl = if title.is_empty() { name.to_string() } else { title.to_string() };
        let (s0, s1) = (here.first().map_or(f64::NAN, |x| x.sta), here.last().map_or(f64::NAN, |x| x.sta));
        tt.push(tx(INK, MARGIN + 4.0, MARGIN + TITLE_H - 6.0, 4.0, 0.0, "left", None, ttl.clone()));
        tt.push(tx(
            INK,
            MARGIN + 4.0,
            MARGIN + 3.5,
            2.2,
            0.0,
            "left",
            None,
            tr!(
                "xs.strip",
                name = name,
                a = format_station(s0, units, None),
                b = format_station(s1, units, None),
                h = h_label.as_str(),
                v = v_label.as_str(),
                vex = used
            ),
        ));
        tt.push(tx(INK, pw - MARGIN - 4.0, MARGIN + TITLE_H - 6.0, 4.0, 0.0, "right", None, tr!("pp.sheet", n = pg + 1, N = pages)));
        tt.push(tx(INK, pw - MARGIN - 4.0, MARGIN + 3.5, 2.2, 0.0, "right", None, tr!("pp.made", date = day_of(made_ms))));
        sheets.push(Sheet {
            w: pw,
            h: ph,
            name: format!("{} {}", ttl, pg + 1),
            views: Some(vec![View {
                clip: (MARGIN, MARGIN + TITLE_H, pw - MARGIN, ph - MARGIN),
                strokes,
                texts,
                fills,
                ..View::default()
            }]),
            single: View::default(),
            chrome,
            chrome_texts: tt,
            extra: vec![("stations".into(), Value::Array(here.iter().map(|x| x.sta.j()).collect()))],
        });
    }
    (sheets, Some((cw, ch, cols, rows)), per_page, used, notes)
}

// ---------------------------------------------------------------------------------------------------------------
// staking table pages

/// `stakingSheets(rows, { paper, landscape, units, title, name })` → (sheets, perPage).
#[allow(clippy::too_many_lines)]
#[allow(clippy::arithmetic_side_effects)] // bounded counters and indices (page numbers, padding, digit math)
#[allow(clippy::too_many_arguments)]
pub fn staking_sheets(rows: &[Row], paper: &str, landscape: bool, units: &str, title: &str, name: &str, made_ms: f64) -> (Vec<Sheet>, f64) {
    let (pw, ph) = orient(&paper_by(paper), landscape);
    let x0 = MARGIN + 4.0;
    let x1 = pw - MARGIN - 4.0;
    let top = ph - MARGIN - 6.0;
    let bottom = MARGIN + TITLE_H + 4.0;
    let row_h = 5.0;
    let per_page = max(1.0, ((top - bottom - row_h * 1.6) / row_h).floor());
    let u = if units == "m" { "m" } else { "ft" };
    let cols: [(&str, f64, String, &str); 8] = [
        ("id", 0.11, t("sk.c.point", &[]), "left"),
        ("sta", 0.12, t("sk.c.sta", &[]), "right"),
        ("off", 0.11, tr!("sk.c.off", u = u), "right"),
        ("n", 0.15, t("sk.c.n", &[]), "right"),
        ("e", 0.15, t("sk.c.e", &[]), "right"),
        ("design", 0.12, t("sk.c.design", &[]), "right"),
        ("ground", 0.12, t("sk.c.ground", &[]), "right"),
        ("cf", 0.12, tr!("sk.c.cf", u = u), "right"),
    ];
    let w = x1 - x0;
    let mut cx = x0;
    let mut edges: Vec<(f64, f64)> = Vec::new();
    for c in &cols {
        let c0 = cx;
        cx += c.1 * w;
        edges.push((c0, cx));
    }
    let num = |v: Option<f64>, d: usize| v.filter(|x| x.is_finite()).map_or(String::new(), |x| cc_civil::js::to_fixed(x, d));
    let cell = |r: &Row, k: &str| -> String {
        match k {
            "id" => r.id.clone(),
            "sta" => format_station(r.sta, units, None),
            "off" => offset_text(r.off),
            "n" => num(Some(r.y), 3),
            "e" => num(Some(r.x), 3),
            "design" => num(r.design, 2),
            "ground" => num(r.ground, 2),
            _ => cut_fill_text(r.cf),
        }
    };
    let ppu = cc_civil::js::usize_of(per_page).unwrap_or(1).max(1);
    let pages = max(1.0, (rows.len() as f64 / per_page).ceil());
    let pages_u = cc_civil::js::usize_of(pages).unwrap_or(1);
    let mut sheets = Vec::new();
    for pg in 0..pages_u {
        let here: Vec<&Row> = rows.iter().skip(pg * ppu).take(ppu).collect();
        let mut strokes = Vec::new();
        let mut texts = Vec::new();
        let mut y = top;
        for (c, e) in cols.iter().zip(edges.iter()) {
            texts.push(tx(INK, if c.3 == "left" { e.0 + 1.0 } else { e.1 - 1.0 }, y - 3.4, 2.2, 0.0, c.3, None, c.2.clone()));
        }
        y -= row_h * 1.2;
        strokes.push(st(INK, 0.35, vec![(x0, y), (x1, y)]));
        let nh = here.len();
        for (i, r) in here.iter().enumerate() {
            let yb = y - row_h;
            for (c, e) in cols.iter().zip(edges.iter()) {
                let s = cell(r, c.0);
                if s.is_empty() {
                    continue;
                }
                let color = if c.0 == "cf" && r.cf.is_some_and(|v| v.is_finite() && v.abs() >= 0.005) {
                    if r.cf.unwrap_or(0.0) > 0.0 { "#a84a00" } else { "#17703a" }
                } else {
                    INK
                };
                texts.push(tx(color, if c.3 == "left" { e.0 + 1.0 } else { e.1 - 1.0 }, yb + 1.5, 2.1, 0.0, c.3, None, s));
            }
            if (i + 1) % 5 == 0 && i + 1 < nh {
                strokes.push(st("#9a9a9a", 0.1, vec![(x0, yb), (x1, yb)]));
            }
            y = yb;
        }
        let mut chrome = Vec::new();
        let mut tt = Vec::new();
        let mut f0 = st(INK, 0.5, vec![(MARGIN, MARGIN), (pw - MARGIN, MARGIN), (pw - MARGIN, ph - MARGIN), (MARGIN, ph - MARGIN)]);
        f0.closed = Some(true);
        chrome.push(f0);
        chrome.push(st(INK, 0.35, vec![(MARGIN, MARGIN + TITLE_H), (pw - MARGIN, MARGIN + TITLE_H)]));
        let ttl = if title.is_empty() { name.to_string() } else { title.to_string() };
        tt.push(tx(INK, MARGIN + 4.0, MARGIN + TITLE_H - 6.0, 4.0, 0.0, "left", None, ttl.clone()));
        tt.push(tx(INK, MARGIN + 4.0, MARGIN + 3.5, 2.2, 0.0, "left", None, tr!("sk.strip", name = name, n = rows.len(), u = u)));
        tt.push(tx(INK, pw - MARGIN - 4.0, MARGIN + TITLE_H - 6.0, 4.0, 0.0, "right", None, tr!("pp.sheet", n = pg + 1, N = pages)));
        tt.push(tx(INK, pw - MARGIN - 4.0, MARGIN + 3.5, 2.2, 0.0, "right", None, tr!("pp.made", date = day_of(made_ms))));
        sheets.push(Sheet {
            w: pw,
            h: ph,
            name: format!("{} {}", ttl, pg + 1),
            views: Some(vec![View {
                clip: (MARGIN, MARGIN + TITLE_H, pw - MARGIN, ph - MARGIN),
                strokes,
                texts,
                fills: Vec::new(),
                ..View::default()
            }]),
            single: View::default(),
            chrome,
            chrome_texts: tt,
            extra: Vec::new(),
        });
    }
    (sheets, per_page)
}
