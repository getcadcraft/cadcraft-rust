//! ifc.js — IFC4 export written by hand as ISO 10303-21 text: project / site / building / one storey per level, the
//! parts as extruded parameterized profiles with holes and copes as openings, assemblies, fasteners, footings with
//! their bars (swept disks along indexed poly curves), grids, materials and a "DetailCraft" property set. GlobalIds
//! come from the model name + ids (cyrb128), so the same model gives the same file.

use cc_detail::build::Built;
use cc_detail::geom::{V3, add, cross, mul, norm, sub};
use cc_detail::i18n::{F, t};
use cc_detail::js::{arr, cos, f as fnum, num_to_string, round, sin, string_of};
use cc_detail::marks::Marks;
use cc_detail::rebar::{Schedule, Seg, bar, centerline2d};
use serde_json::Value;
use std::collections::HashMap;
use std::f64::consts::PI;

const B64: &[u8; 64] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$";

/// `guidFor(seed)`: 128 bits from a string (cyrb128) → a 22-character IFC GlobalId.
pub fn guid_for(seed: &str) -> String {
    let (mut h1, mut h2, mut h3, mut h4): (u32, u32, u32, u32) = (1_779_033_703, 3_144_134_277, 1_013_904_242, 2_773_480_762);
    for k in seed.encode_utf16() {
        let k = u32::from(k);
        h1 = h2 ^ (h1 ^ k).wrapping_mul(597_399_067);
        h2 = h3 ^ (h2 ^ k).wrapping_mul(2_869_860_233);
        h3 = h4 ^ (h3 ^ k).wrapping_mul(951_274_213);
        h4 = h1 ^ (h4 ^ k).wrapping_mul(2_716_044_179);
    }
    h1 = (h3 ^ (h1 >> 18)).wrapping_mul(597_399_067);
    h2 = (h4 ^ (h2 >> 22)).wrapping_mul(2_869_860_233);
    h3 = (h1 ^ (h3 >> 17)).wrapping_mul(951_274_213);
    h4 = (h2 ^ (h4 >> 19)).wrapping_mul(2_716_044_179);
    h1 ^= h2 ^ h3 ^ h4;
    h2 ^= h1;
    h3 ^= h1;
    h4 ^= h1;
    let mut n: u128 = 0;
    for h in [h1, h2, h3, h4] {
        n = (n << 32) | u128::from(h);
    }
    let mut out = [0u8; 22];
    for slot in out.iter_mut().rev() {
        let i = usize::try_from(n & 63).unwrap_or(0);
        *slot = B64.get(i).copied().unwrap_or(b'0');
        n >>= 6;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `R(v)`: a STEP real, always with a decimal point.
pub fn r(v: f64) -> String {
    if !v.is_finite() {
        return "0.".into();
    }
    let x = if v.abs() < 1e-9 { 0.0 } else { v };
    let s = crate::num::fixed_num_str(x, 6);
    if let Some((m, e)) = s.split_once('e') {
        let m = if m.contains('.') { m.to_string() } else { format!("{m}.") };
        return format!("{m}E{e}");
    }
    if s.contains('.') { s } else { format!("{s}.") }
}

/// `S(str)`: a STEP string ('' for a quote, \\ for a backslash, \X2\…\X0\ outside printable ASCII).
pub fn s(text: &str) -> String {
    let mut out = String::with_capacity(text.len().saturating_add(2));
    out.push('\'');
    for ch in text.chars() {
        let c = u32::from(ch);
        if c == 39 {
            out.push_str("''");
        } else if c == 92 {
            out.push_str("\\\\");
        } else if (32..127).contains(&c) {
            out.push(ch);
        } else {
            let mut buf = [0u16; 2];
            let units: Vec<String> = ch.encode_utf16(&mut buf).iter().map(|u| format!("{u:04X}")).collect();
            out.push_str("\\X2\\");
            out.push_str(&units.concat());
            out.push_str("\\X0\\");
        }
    }
    out.push('\'');
    out
}

struct Step {
    lines: Vec<String>,
    n: usize,
    cache: HashMap<String, String>,
}
impl Step {
    fn add(&mut self, s: String) -> String {
        self.n = self.n.saturating_add(1);
        self.lines.push(format!("#{}={};", self.n, s));
        format!("#{}", self.n)
    }
    fn once(&mut self, s: String) -> String {
        if let Some(v) = self.cache.get(&s) {
            return v.clone();
        }
        let id = self.add(s.clone());
        self.cache.insert(s, id.clone());
        id
    }
    fn pt(&mut self, p: &[f64]) -> String {
        let t = p.iter().map(|v| r(*v)).collect::<Vec<_>>().join(",");
        self.once(format!("IFCCARTESIANPOINT(({t}))"))
    }
    fn dir(&mut self, d: &[f64]) -> String {
        let t = d.iter().map(|v| r(*v)).collect::<Vec<_>>().join(",");
        self.once(format!("IFCDIRECTION(({t}))"))
    }
}

fn any_perp(a: V3) -> V3 {
    norm(if a[2].abs() < 0.9 { cross([0.0, 0.0, 1.0], a) } else { cross([1.0, 0.0, 0.0], a) })
}

struct Storey {
    z: f64,
    name: String,
    lp: String,
    id: String,
    contents: Vec<String>,
}

/// `new Date(ms).toISOString().slice(0, 19)`.
pub fn iso19(ms: f64) -> String {
    let ms = if ms.is_finite() { ms } else { 0.0 };
    let days = (ms / 86_400_000.0).floor();
    let rem = ms - days * 86_400_000.0;
    let secs = (rem / 1000.0).floor();
    // days since 1970-01-01 → civil date (Howard Hinnant's algorithm), in f64 (exact for any date a file carries)
    let z = days + 719_468.0;
    let era = (z / 146_097.0).floor();
    let doe = z - era * 146_097.0;
    let yoe = ((doe - (doe / 1460.0).floor() + (doe / 36524.0).floor() - (doe / 146_096.0).floor()) / 365.0).floor();
    let y = yoe + era * 400.0;
    let doy = doe - (365.0 * yoe + (yoe / 4.0).floor() - (yoe / 100.0).floor());
    let mp = ((5.0 * doy + 2.0) / 153.0).floor();
    let d = doy - ((153.0 * mp + 2.0) / 5.0).floor() + 1.0;
    let m = if mp < 10.0 { mp + 3.0 } else { mp - 9.0 };
    let y = if m <= 2.0 { y + 1.0 } else { y };
    let (hh, mm, ss) = ((secs / 3600.0).floor(), ((secs % 3600.0) / 60.0).floor(), secs % 60.0);
    let two = |v: f64| format!("{v:02}");
    let year = if (0.0..=9999.0).contains(&y) { format!("{y:04}") } else { format!("{y:+07}") };
    format!("{year}-{}-{}T{}:{}:{}", two(m), two(d), two(hh), two(mm), two(ss))
}

fn pred(ifc: &str) -> &'static str {
    match ifc {
        "IfcBeam" => ".BEAM.",
        "IfcColumn" => ".COLUMN.",
        "IfcMember" => ".BRACE.",
        "IfcPlate" => ".SHEET.",
        _ => "undefined",
    }
}

/// `writeIfc(state, built, marks, schedule, { name, now })`; `footing_mark` is `marks.footingMark` (None = absent).
pub fn write_ifc(
    state: &Value,
    built: &Built,
    marks: &Marks,
    footing_mark: Option<&[(String, String)]>,
    schedule: Option<&Schedule>,
    name: Option<&str>,
    now_ms: f64,
) -> Result<String, String> {
    let state_name = state.get("name");
    let name: String = match name {
        Some(n) => n.to_string(),
        None => match state_name {
            Some(v) if cc_detail::js::truthy(Some(v)) => string_of(Some(v)),
            _ => "DetailCraft model".into(),
        },
    };
    let seed_base = string_of(state_name);
    let seed = |k: &str| guid_for(&format!("{seed_base}|{k}"));
    let mut w = Step { lines: vec![], n: 0, cache: HashMap::new() };
    let o = w.pt(&[0.0, 0.0, 0.0]);
    let dz = w.dir(&[0.0, 0.0, 1.0]);
    let _dx = w.dir(&[1.0, 0.0, 0.0]);
    let wcs = w.add(format!("IFCAXIS2PLACEMENT3D({o},$,$)"));
    let ctx = w.add(format!("IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,{wcs},$)"));
    let body = w.add(format!("IFCGEOMETRICREPRESENTATIONSUBCONTEXT('Body','Model',*,*,*,*,{ctx},$,.MODEL_VIEW.,$)"));
    let units = [
        w.add("IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.)".into()),
        w.add("IFCSIUNIT(*,.AREAUNIT.,$,.SQUARE_METRE.)".into()),
        w.add("IFCSIUNIT(*,.VOLUMEUNIT.,$,.CUBIC_METRE.)".into()),
        w.add("IFCSIUNIT(*,.PLANEANGLEUNIT.,$,.RADIAN.)".into()),
        w.add("IFCSIUNIT(*,.MASSUNIT.,.KILO.,.GRAM.)".into()),
    ];
    let ua = w.add(format!("IFCUNITASSIGNMENT(({}))", units.join(",")));
    let project = w.add(format!("IFCPROJECT({},$,{},$,$,$,$,({ctx}),{ua})", s(&seed("project")), s(&name)));
    let site_lp = w.add(format!("IFCLOCALPLACEMENT($,{wcs})"));
    let site = w.add(format!("IFCSITE({},$,'Site',$,$,{site_lp},$,$,.ELEMENT.,$,$,$,$,$)", s(&seed("site"))));
    let bld_lp = w.add(format!("IFCLOCALPLACEMENT({site_lp},{wcs})"));
    let bld = w.add(format!("IFCBUILDING({},$,'Building',$,$,{bld_lp},$,$,.ELEMENT.,$,$,$)", s(&seed("building"))));
    w.add(format!("IFCRELAGGREGATES({},$,$,$,{project},({site}))", s(&seed("rel-site"))));
    w.add(format!("IFCRELAGGREGATES({},$,$,$,{site},({bld}))", s(&seed("rel-bld"))));
    let mut levels: Vec<(f64, String)> = arr(state.get("levels")).iter().map(|l| (fnum(l.get("z")), string_of(l.get("name")))).collect();
    levels.sort_by(|a, b| cc_detail::js::ord(a.0 - b.0));
    let mut storeys: Vec<Storey> = Vec::new();
    for (i, (z, lname)) in levels.iter().enumerate() {
        let p = w.pt(&[0.0, 0.0, *z]);
        let ax = w.add(format!("IFCAXIS2PLACEMENT3D({p},$,$)"));
        let lp = w.add(format!("IFCLOCALPLACEMENT({bld_lp},{ax})"));
        let id =
            w.add(format!("IFCBUILDINGSTOREY({},$,{},$,$,{lp},$,$,.ELEMENT.,{})", s(&seed(&format!("storey{i}{lname}"))), s(lname), r(*z)));
        storeys.push(Storey { z: *z, name: lname.clone(), lp, id, contents: vec![] });
    }
    let ids: Vec<String> = storeys.iter().map(|s| s.id.clone()).collect();
    w.add(format!("IFCRELAGGREGATES({},$,$,$,{bld},({}))", s(&seed("rel-storeys")), ids.join(",")));
    if storeys.is_empty() {
        return Err("Cannot read properties of undefined (reading 'lv')".into());
    }
    let storey_for = |z: f64, storeys: &[Storey]| -> usize {
        let mut k = 0;
        for (i, t) in storeys.iter().enumerate() {
            if t.z <= z + 0.5 {
                k = i;
            }
        }
        k
    };
    fn place(w: &mut Step, st: &Storey, p: V3, axis: V3, rf: V3) -> String {
        let loc = w.pt(&[p[0], p[1], p[2] - st.z]);
        let a = w.dir(&axis);
        let b = w.dir(&rf);
        let ax = w.add(format!("IFCAXIS2PLACEMENT3D({loc},{a},{b})"));
        w.add(format!("IFCLOCALPLACEMENT({},{ax})", st.lp))
    }
    let shape = |w: &mut Step, items: &[String], ty: &str| -> String {
        let rep = w.add(format!("IFCSHAPEREPRESENTATION({body},{},{},({}))", s("Body"), s(ty), items.join(",")));
        w.add(format!("IFCPRODUCTDEFINITIONSHAPE($,$,({rep}))"))
    };
    fn pos2(w: &mut Step, x: f64, y: f64) -> String {
        let p = w.pt(&[x, y]);
        w.add(format!("IFCAXIS2PLACEMENT2D({p},$)"))
    }
    let extrude = |w: &mut Step, profile: &str, depth: f64| -> String {
        let p = w.pt(&[0.0, 0.0, 0.0]);
        let ax = w.add(format!("IFCAXIS2PLACEMENT3D({p},$,$)"));
        w.add(format!("IFCEXTRUDEDAREASOLID({profile},{ax},{dz},{})", r(depth)))
    };
    let rs = |v: &[f64]| v.iter().map(|x| r(*x)).collect::<Vec<_>>().join(",");

    let mut voids: Vec<(String, String, String)> = Vec::new();
    let mut materials: Vec<(String, Vec<String>)> = Vec::new();
    fn mat_push(m: &str, id: String, materials: &mut Vec<(String, Vec<String>)>) {
        match materials.iter_mut().find(|(k, _)| k == m) {
            Some(e) => e.1.push(id),
            None => materials.push((m.to_string(), vec![id])),
        }
    }
    let mut psets: Vec<(String, Vec<(&'static str, &'static str, PVal)>)> = Vec::new();
    let mut element_of: HashMap<String, (String, usize)> = HashMap::new();
    for part in &built.parts {
        let si = storey_for(cc_detail::js::min(part.start[2], part.end[2]), &storeys);
        let fr = part.frame;
        let lp = {
            let st = storeys.get(si).ok_or("storey")?;
            place(&mut w, st, fr.o, fr.x, fr.u)
        };
        let p = &part.profile;
        let pos = pos2(&mut w, 0.0, part.voff);
        let nm = s(&p.name);
        let pd = match p.kind.as_str() {
            "I" => w.add(format!(
                "IFCISHAPEPROFILEDEF(.AREA.,{nm},{pos},{},{},$,$)",
                rs(&[p.bf(), p.d(), p.tw(), p.tf()]),
                if p.r() > 0.0 { r(p.r()) } else { "$".into() }
            )),
            "C" => w.add(format!("IFCUSHAPEPROFILEDEF(.AREA.,{nm},{pos},{},$,$,$)", rs(&[p.d(), p.bf(), p.tw(), p.tf()]))),
            "L" => w.add(format!("IFCLSHAPEPROFILEDEF(.AREA.,{nm},{pos},{},$,$,$)", rs(&[p.b(), p.b(), p.t()]))),
            "RHS" => w.add(format!(
                "IFCRECTANGLEHOLLOWPROFILEDEF(.AREA.,{nm},{pos},{})",
                rs(&[p.b(), p.h.unwrap_or(f64::NAN), p.t(), p.ri(), p.ro()])
            )),
            "CHS" => w.add(format!("IFCCIRCLEHOLLOWPROFILEDEF(.AREA.,{nm},{pos},{},{})", r(p.big_d() / 2.0), r(p.t()))),
            "PL" => w.add(format!("IFCRECTANGLEPROFILEDEF(.AREA.,{nm},{pos},{},{})", r(p.b()), r(p.t()))),
            k => return Err(t("ifcout.err.profile", &[("kind", F::from(k))])),
        };
        let solid = extrude(&mut w, &pd, part.length);
        let mark = marks.part_mark(&part.id).unwrap_or("").to_string();
        let nm = match &part.name {
            Some(n) if !n.is_empty() => n.clone(),
            _ => part.role.clone().unwrap_or_default(),
        };
        let ifc = part.ifc.ok_or("Cannot read properties of undefined (reading 'toUpperCase')")?;
        let sh = shape(&mut w, &[solid], "SweptSolid");
        let id = w.add(format!(
            "{}({},$,{},$,{},{lp},{sh},{},{})",
            ifc.to_uppercase(),
            s(&seed(&format!("part{}", part.id))),
            s(&nm),
            s(&part.profile_name),
            s(&mark),
            pred(ifc)
        ));
        element_of.insert(part.id.clone(), (id.clone(), si));
        mat_push(&part.material, id.clone(), &mut materials);
        let am = marks.asm_of(&part.id).and_then(|a| marks.asm_mark(a)).unwrap_or("").to_string();
        psets.push((
            id.clone(),
            vec![
                ("PartMark", "IFCLABEL", PVal::S(mark)),
                ("AssemblyMark", "IFCLABEL", PVal::S(am)),
                ("Profile", "IFCLABEL", PVal::S(part.profile_name.clone())),
                ("Length", "IFCLENGTHMEASURE", PVal::N(part.length)),
                ("Weight", "IFCMASSMEASURE", PVal::N(part.weight)),
                ("Phase", "IFCINTEGER", PVal::N(cc_detail::js::or(part.phase, 1.0))),
            ],
        ));
        for (i, h) in part.holes.iter().enumerate() {
            let (c, axis, depth): (V3, V3, f64) = if h.face == "web" {
                ([0.0, h.y + part.voff, h.x], [1.0, 0.0, 0.0], (if p.kind == "RHS" { p.b() } else { p.tw() }) + 2.0)
            } else if h.face == "plate" {
                ([h.y, part.voff, h.x], [0.0, 1.0, 0.0], p.t() + 2.0)
            } else {
                let sg = if h.face == "top" { 1.0 } else { -1.0 };
                let tk = if p.kind == "RHS" { p.t() } else { p.tf() };
                ([h.y, part.voff + sg * (p.depth / 2.0 - tk / 2.0), h.x], [0.0, 1.0, 0.0], tk + 2.0)
            };
            let start = sub(c, mul(axis, depth / 2.0));
            let sp = w.pt(&start);
            let a = w.dir(&axis);
            let b = w.dir(&any_perp(axis));
            let ax = w.add(format!("IFCAXIS2PLACEMENT3D({sp},{a},{b})"));
            let olp = w.add(format!("IFCLOCALPLACEMENT({lp},{ax})"));
            let p0 = pos2(&mut w, 0.0, 0.0);
            let circ = w.add(format!("IFCCIRCLEPROFILEDEF(.AREA.,$,{p0},{})", r(h.d / 2.0)));
            let ex = extrude(&mut w, &circ, depth);
            let sh = shape(&mut w, &[ex], "SweptSolid");
            let key = format!("hole{}:{i}", part.id);
            let op = w.add(format!("IFCOPENINGELEMENT({},$,'Hole',$,$,{olp},{sh},$,.OPENING.)", s(&seed(&key))));
            voids.push((id.clone(), op, key));
        }
        for (i, c) in part.cuts.iter().enumerate() {
            if c.kind != "cope" {
                continue;
            }
            let sg = if c.side == "top" { 1.0 } else { -1.0 };
            let vc = part.voff + sg * (p.depth / 2.0 - c.depth / 2.0 + 0.5);
            let x0 = if c.end == "start" { -1.0 } else { part.length - c.length };
            let pp = w.pt(&[0.0, vc, x0]);
            let ax = w.add(format!("IFCAXIS2PLACEMENT3D({pp},$,$)"));
            let olp = w.add(format!("IFCLOCALPLACEMENT({lp},{ax})"));
            let p0 = pos2(&mut w, 0.0, 0.0);
            let rect = w.add(format!("IFCRECTANGLEPROFILEDEF(.AREA.,$,{p0},{},{})", r(p.width + 2.0), r(c.depth + 1.0)));
            let ex = extrude(&mut w, &rect, c.length + 1.0);
            let sh = shape(&mut w, &[ex], "SweptSolid");
            let key = format!("cope{}:{i}", part.id);
            let op = w.add(format!("IFCOPENINGELEMENT({},$,'Cope',$,$,{olp},{sh},$,.OPENING.)", s(&seed(&key))));
            voids.push((id.clone(), op, key));
        }
    }
    for (el, op, k) in &voids {
        w.add(format!("IFCRELVOIDSELEMENT({},$,$,$,{el},{op})", s(&seed(&format!("void{k}")))));
    }

    for a in &marks.assemblies {
        for main_id in &a.ids {
            let subs: Vec<&cc_detail::build::Part> =
                built.parts.iter().filter(|p| marks.asm_of(&p.id) == Some(main_id.as_str()) && p.id != *main_id).collect();
            let (mid, si) = element_of.get(main_id).cloned().ok_or("Cannot read properties of undefined (reading 'st')")?;
            if subs.is_empty() {
                if let Some(st) = storeys.get_mut(si) {
                    st.contents.push(mid);
                }
                continue;
            }
            let slp = storeys.get(si).map(|s| s.lp.clone()).unwrap_or_default();
            let lp = w.add(format!("IFCLOCALPLACEMENT({slp},{wcs})"));
            let asm = w.add(format!(
                "IFCELEMENTASSEMBLY({},$,{},$,'Assembly',{lp},$,{},.FACTORY.,.NOTDEFINED.)",
                s(&seed(&format!("asm{main_id}"))),
                s(&a.mark),
                s(&a.mark)
            ));
            let mut list = vec![mid];
            for sp in &subs {
                list.push(element_of.get(&sp.id).map(|e| e.0.clone()).unwrap_or_default());
            }
            w.add(format!("IFCRELAGGREGATES({},$,$,$,{asm},({}))", s(&seed(&format!("rel-asm{main_id}"))), list.join(",")));
            if let Some(st) = storeys.get_mut(si) {
                st.contents.push(asm);
            }
        }
    }

    for (i, b) in built.bolts.iter().enumerate() {
        let si = storey_for(cc_detail::js::min(b.head[2], b.head[2] + b.axis[2] * b.length), &storeys);
        let lp = {
            let st = storeys.get(si).ok_or("storey")?;
            place(&mut w, st, b.head, b.axis, any_perp(b.axis))
        };
        let p0 = pos2(&mut w, 0.0, 0.0);
        let circ = w.add(format!("IFCCIRCLEPROFILEDEF(.AREA.,$,{p0},{})", r(b.d / 2.0)));
        let label = format!(
            "{}{} x {} {}",
            if b.kind == "ANCHORBOLT" { "Anchor rod " } else { "Bolt " },
            num_to_string(b.d),
            num_to_string(b.length),
            b.grade
        );
        let ex = extrude(&mut w, &circ, b.length);
        let sh = shape(&mut w, &[ex], "SweptSolid");
        // `b.conn || b.loose`
        let who = match (&b.conn, &b.loose) {
            (Some(c), _) if !c.is_empty() => c.clone(),
            (_, Some(l)) => l.clone(),
            (Some(c), None) => c.clone(),
            (None, None) => "undefined".into(),
        };
        let id = w.add(format!(
            "IFCMECHANICALFASTENER({},$,{},$,{},{lp},{sh},$,{},{},.{}.)",
            s(&seed(&format!("bolt{who}:{i}"))),
            s(&label),
            s(&b.grade),
            r(b.d),
            r(b.length),
            b.kind
        ));
        if let Some(st) = storeys.get_mut(si) {
            st.contents.push(id);
        }
    }

    for ft in &built.footings {
        let f = &ft.f;
        let rot = cc_detail::js::or(f.rotation, 0.0) * PI / 180.0;
        let si = storey_for(f.top - f.h, &storeys);
        let (lp, slp, sz) = {
            let st = storeys.get(si).ok_or("storey")?;
            (place(&mut w, st, [f.x, f.y, f.top - f.h], [0.0, 0.0, 1.0], [cos(rot), sin(rot), 0.0]), st.lp.clone(), st.z)
        };
        let p0 = pos2(&mut w, 0.0, 0.0);
        let rect = w.add(format!("IFCRECTANGLEPROFILEDEF(.AREA.,$,{p0},{},{})", r(f.l), r(f.b)));
        let fmark = footing_mark
            .and_then(|m| m.iter().find(|(k, _)| *k == f.id))
            .map(|(_, v)| v.clone())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| f.id.clone());
        let ex = extrude(&mut w, &rect, f.h);
        let sh = shape(&mut w, &[ex], "SweptSolid");
        let fid = w.add(format!(
            "IFCFOOTING({},$,{},$,{},{lp},{sh},{},.PAD_FOOTING.)",
            s(&seed(&format!("footing{}", f.id))),
            s(&format!("Footing {fmark}")),
            s(&f.concrete),
            s(&fmark)
        ));
        if let Some(st) = storeys.get_mut(si) {
            st.contents.push(fid.clone());
        }
        mat_push(&format!("Concrete {}", f.concrete), fid, &mut materials);
        let mark_list: &[(cc_detail::rebar::BarGroup, String)] =
            schedule.and_then(|sc| sc.by_footing.iter().find(|(k, _)| *k == f.id)).map_or(&[], |(_, l)| l.as_slice());
        for (gi, g) in ft.bars.iter().enumerate() {
            let Some(b) = bar(&g.bar) else { return Err("Cannot read properties of undefined (reading 'db')".into()) };
            let segs = centerline2d(g.shape, &g.dims, b);
            for (si2, set) in g.sets.iter().enumerate() {
                let wp = |q: [f64; 2]| add(add(set.origin, mul(set.s_dir, q[0])), mul(set.t_dir, q[1]));
                let mut pts: Vec<V3> = Vec::new();
                if let Some(first) = segs.first() {
                    pts.push(wp(first.a()));
                }
                let mut idx = Vec::new();
                for sg in &segs {
                    let i0 = pts.len();
                    match sg {
                        Seg::L { b, .. } => {
                            pts.push(wp(*b));
                            idx.push(format!("IFCLINEINDEX(({},{}))", i0, i0.saturating_add(1)));
                        }
                        Seg::A { m, b, .. } => {
                            pts.push(wp(*m));
                            pts.push(wp(*b));
                            idx.push(format!("IFCARCINDEX(({},{},{}))", i0, i0.saturating_add(1), i0.saturating_add(2)));
                        }
                    }
                }
                let rel: Vec<String> = pts.iter().map(|p| format!("({})", rs(&[p[0], p[1], p[2] - sz]))).collect();
                let list = w.add(format!("IFCCARTESIANPOINTLIST3D(({}),$)", rel.join(",")));
                let curve = w.add(format!("IFCINDEXEDPOLYCURVE({list},({}),.F.)", idx.join(",")));
                let disk = w.add(format!("IFCSWEPTDISKSOLID({curve},{},$,$,$)", r(b.db / 2.0)));
                let ax = w.add(format!("IFCAXIS2PLACEMENT3D({o},$,$)"));
                let blp = w.add(format!("IFCLOCALPLACEMENT({slp},{ax})"));
                let mk = mark_list.get(gi).map(|x| x.1.clone()).unwrap_or_default();
                let sh = shape(&mut w, &[disk], "AdvancedSweptSolid");
                let id = w.add(format!(
                    "IFCREINFORCINGBAR({},$,{},{},{},{blp},{sh},{},$,{},{},{},.MAIN.,.TEXTURED.)",
                    s(&seed(&format!("bar{}:{gi}:{si2}", f.id))),
                    s(&format!("{mk} {}", g.bar)),
                    s(&g.role),
                    s(g.shape),
                    s(&mk),
                    r(b.db),
                    r(b.area),
                    r(g.length)
                ));
                if let Some(st) = storeys.get_mut(si) {
                    st.contents.push(id);
                }
            }
        }
    }

    let grids = state.get("grids");
    let gx = arr(grids.and_then(|g| g.get("x")));
    let gy = arr(grids.and_then(|g| g.get("y")));
    if !gx.is_empty() && !gy.is_empty() {
        let xs: Vec<f64> = gx.iter().map(|g| fnum(g.get("at"))).collect();
        let ys: Vec<f64> = gy.iter().map(|g| fnum(g.get("at"))).collect();
        let pad = 1500.0;
        let (x0, x1) = (cc_detail::js::min_of(xs.iter().copied()) - pad, cc_detail::js::max_of(xs.iter().copied()) + pad);
        let (y0, y1) = (cc_detail::js::min_of(ys.iter().copied()) - pad, cc_detail::js::max_of(ys.iter().copied()) + pad);
        let axis = |w: &mut Step, tag: &str, a: [f64; 2], b: [f64; 2]| -> String {
            let pa = w.pt(&a);
            let pb = w.pt(&b);
            let pl = w.add(format!("IFCPOLYLINE(({pa},{pb}))"));
            w.add(format!("IFCGRIDAXIS({},{pl},.T.)", s(tag)))
        };
        let mut u = Vec::new();
        for g in gx {
            let at = fnum(g.get("at"));
            u.push(axis(&mut w, &string_of(g.get("label")), [at, y0], [at, y1]));
        }
        let mut v = Vec::new();
        for g in gy {
            let at = fnum(g.get("at"));
            v.push(axis(&mut w, &string_of(g.get("label")), [x0, at], [x1, at]));
        }
        let slp = storeys.first().map(|s| s.lp.clone()).unwrap_or_default();
        let ax = w.add(format!("IFCAXIS2PLACEMENT3D({o},$,$)"));
        let glp = w.add(format!("IFCLOCALPLACEMENT({slp},{ax})"));
        let gid = w.add(format!("IFCGRID({},$,'Grid',$,$,{glp},$,({}),({}),$,.RECTANGULAR.)", s(&seed("grid")), u.join(","), v.join(",")));
        if let Some(st) = storeys.first_mut() {
            st.contents.push(gid);
        }
    }

    for st in &storeys {
        if !st.contents.is_empty() {
            w.add(format!(
                "IFCRELCONTAINEDINSPATIALSTRUCTURE({},$,$,$,({}),{})",
                s(&seed(&format!("contain{}", st.name))),
                st.contents.join(","),
                st.id
            ));
        }
    }
    for (mat, els) in &materials {
        let m = w.add(format!("IFCMATERIAL({},$,$)", s(mat)));
        w.add(format!("IFCRELASSOCIATESMATERIAL({},$,$,$,({}),{m})", s(&seed(&format!("mat{mat}"))), els.join(",")));
    }
    for (i, (el, props)) in psets.iter().enumerate() {
        let mut ps = Vec::new();
        for (n, ty, v) in props {
            let val = match (ty, v) {
                (&"IFCINTEGER", PVal::N(x)) => num_to_string(round(*x)),
                (_, PVal::N(x)) => r(*x),
                (_, PVal::S(t)) => s(t),
            };
            ps.push(w.add(format!("IFCPROPERTYSINGLEVALUE({},$,{ty}({val}),$)", s(n))));
        }
        let set = w.add(format!("IFCPROPERTYSET({},$,'DetailCraft',$,({}))", s(&seed(&format!("pset{i}"))), ps.join(",")));
        w.add(format!("IFCRELDEFINESBYPROPERTIES({},$,$,$,({el}),{set})", s(&seed(&format!("rdp{i}")))));
    }
    let stamp = iso19(now_ms);
    let mut out = vec![
        "ISO-10303-21;".to_string(),
        "HEADER;".into(),
        "FILE_DESCRIPTION(('ViewDefinition [ReferenceView_V1.2]'),'2;1');".into(),
        format!("FILE_NAME({},{},(''),(''),'DetailCraft','DetailCraft lab build','');", s(&format!("{name}.ifc")), s(&stamp)),
        "FILE_SCHEMA(('IFC4'));".into(),
        "ENDSEC;".into(),
        "DATA;".into(),
    ];
    out.extend(w.lines);
    out.push("ENDSEC;".into());
    out.push("END-ISO-10303-21;".into());
    out.push(String::new());
    Ok(out.join("\n"))
}

enum PVal {
    S(String),
    N(f64),
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn step_values() {
        // values the frozen ifc.js gives for the same inputs
        assert_eq!(guid_for("Sample frame|project"), "0vd9GmVkS9q5ZkRGliY0y6");
        assert_eq!(r(1e21), "1.E+21");
        assert_eq!(r(-1e-10), "0.");
        assert_eq!(r(2.5), "2.5");
        assert_eq!(r(f64::NAN), "0.");
        assert_eq!(s("it's ★"), "'it''s \\X2\\2605\\X0\\'");
        assert_eq!(iso19(0.0), "1970-01-01T00:00:00");
        assert_eq!(iso19(1_791_633_600_000.0), "2026-10-10T12:00:00");
    }
}
