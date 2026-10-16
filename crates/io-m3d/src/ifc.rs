//! IFC4 export: io.js `exportIFC`, `ifcGuid`, `stepString`. One IfcBuildingElementProxy per top-level group or
//! component (+ "Loose geometry"), placed in its storey (the model's storeys, or one at 0); closed solids as faceted
//! breps, the rest as shell-based surface models. GlobalIds are deterministic (a hash of the model name and a key).

use cc_m3d::Model;
use cc_m3d::js;
use cc_m3d::solids::solid_info;
use cc_m3d::vec::{IDENT, M4, P3, apply, det3, dot, len, norm};

use crate::num::{exponential, fixed, fixed_num};

const IFC64: &[u8; 64] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$";

/// `ifcGuid(seed)`: a deterministic IFC GlobalId (22 characters) from a string.
pub fn ifc_guid(seed: &str) -> String {
    let mut n: u128 = 0;
    for r in 0u32..4 {
        let mut h: u32 = 0x811c_9dc5 ^ r.wrapping_mul(0x9e37_79b9);
        let s = format!("{r}:{seed}");
        for c in s.encode_utf16() {
            h ^= u32::from(c);
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= h >> 15;
        h = h.wrapping_mul(0x2c1b_3c6d);
        h ^= h >> 12;
        n = (n << 32) | u128::from(h);
    }
    let ch = |k: u128| char::from(IFC64.get(usize::try_from(k & 63).unwrap_or(0)).copied().unwrap_or(b'0'));
    let mut out = String::with_capacity(22);
    out.push(ch(n >> 126));
    for j in (0u32..=20).rev() {
        out.push(ch(n >> j.saturating_mul(6)));
    }
    out
}

/// `stepString(s)`: an ISO-10303-21 string literal (control characters → spaces, ' doubled, \ doubled, non-ASCII as
/// \X2\…\X0\ UTF-16 units).
pub fn step_string(s: &str) -> String {
    let mut out = String::from("'");
    for ch in s.chars() {
        let c = if (ch as u32) < 0x20 || ch == '\u{7f}' { ' ' } else { ch };
        match c {
            '\'' => out.push_str("''"),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 128 => out.push(c),
            c => {
                out.push_str("\\X2\\");
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf).iter() {
                    out.push_str(&format!("{u:04X}"));
                }
                out.push_str("\\X0\\");
            }
        }
    }
    out.push('\'');
    out
}

/// `real(x)`: an IFC REAL (nine decimals; exponent form for what String() writes with one; always a point or E).
pub fn real(x: f64) -> String {
    let mut v = fixed_num(x, 9);
    if v == 0.0 || v.abs() < 1e-12 {
        v = 0.0;
    }
    let mut s = js::num_str(v);
    if s.contains('e') {
        s = exponential(x, 9).replacen('e', "E", 1);
    }
    if !s.contains('.') && !s.contains('E') {
        s.push('.');
    }
    s
}

fn rigid(m: &M4) -> bool {
    let cols = [[m[0], m[1], m[2]], [m[4], m[5], m[6]], [m[8], m[9], m[10]]];
    if m[3].abs() + m[7].abs() + m[11].abs() > 1e-12 || (m[15] - 1.0).abs() > 1e-12 {
        return false;
    }
    for a in 0..3usize {
        let ca = cols.get(a).copied().unwrap_or([0.0; 3]);
        if (len(ca) - 1.0).abs() > 1e-9 {
            return false;
        }
        for b in a.saturating_add(1)..3 {
            if dot(ca, cols.get(b).copied().unwrap_or([0.0; 3])).abs() > 1e-9 {
                return false;
            }
        }
    }
    det3(m) > 0.0
}

struct Level {
    name: String,
    elevation: f64,
    pl: String,
    id: String,
    items: Vec<String>,
}

struct Step {
    lines: Vec<String>,
    n: u64,
    salt: String,
}

impl Step {
    fn l(&mut self, s: &str) -> String {
        self.n = self.n.saturating_add(1);
        let id = format!("#{}", self.n);
        self.lines.push(format!("{id}={s};"));
        id
    }
    fn g(&self, k: &str) -> String {
        step_string(&ifc_guid(&format!("{}|{k}", self.salt)))
    }
    fn nums(v: &[f64]) -> String {
        v.iter().map(|&x| real(x)).collect::<Vec<_>>().join(",")
    }
    fn dir(&mut self, v: P3) -> String {
        self.l(&format!("IFCDIRECTION(({}))", Self::nums(&v)))
    }
    fn pt(&mut self, p: P3) -> String {
        self.l(&format!("IFCCARTESIANPOINT(({}))", Self::nums(&p)))
    }
    fn place(&mut self, rel: &str, axes: Option<String>, origin: &str) -> String {
        let axes = match axes {
            Some(a) => a,
            None => self.l(&format!("IFCAXIS2PLACEMENT3D({origin},$,$)")),
        };
        self.l(&format!("IFCLOCALPLACEMENT({rel},{axes})"))
    }
}

/// `exportIFC(model, {name, date})` → the text. `date_iso` is `date.toISOString()`.
#[allow(clippy::too_many_lines)]
pub fn export_ifc(model: &Model, name: &str, date_iso: &str) -> String {
    let mut st = Step { lines: Vec::new(), n: 0, salt: name.to_string() };
    let origin = st.pt([0.0, 0.0, 0.0]);
    let z_dir = st.dir([0.0, 0.0, 1.0]);
    let x_dir = st.dir([1.0, 0.0, 0.0]);
    let world = st.l(&format!("IFCAXIS2PLACEMENT3D({origin},{z_dir},{x_dir})"));
    let ctx = st.l(&format!("IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,{world},$)"));
    let body = st.l(&format!("IFCGEOMETRICREPRESENTATIONSUBCONTEXT('Body','Model',*,*,*,*,{ctx},$,.MODEL_VIEW.,$)"));
    let u1 = st.l("IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.)");
    let u2 = st.l("IFCSIUNIT(*,.AREAUNIT.,$,.SQUARE_METRE.)");
    let u3 = st.l("IFCSIUNIT(*,.VOLUMEUNIT.,$,.CUBIC_METRE.)");
    let u4 = st.l("IFCSIUNIT(*,.PLANEANGLEUNIT.,$,.RADIAN.)");
    let units = st.l(&format!("IFCUNITASSIGNMENT(({u1},{u2},{u3},{u4}))"));
    let project = st.l(&format!("IFCPROJECT({},$,{},$,$,$,$,({ctx}),{units})", st.g("project"), step_string(name)));
    let site_pl = st.place("$", None, &origin);
    let bld_pl = st.place(&site_pl, None, &origin);
    let site = st.l(&format!("IFCSITE({},$,'Site',$,$,{site_pl},$,$,.ELEMENT.,$,$,$,$,$)", st.g("site")));
    let building = st.l(&format!("IFCBUILDING({},$,'Building',$,$,{bld_pl},$,$,.ELEMENT.,$,$,$)", st.g("building")));
    // storeys (0.4 M12): the model's list, or one at 0; each is placed at its elevation in the building
    let src: Vec<(String, f64)> = if model.storeys.is_empty() {
        vec![("Level 1".into(), 0.0)]
    } else {
        model.storeys.iter().map(|s| (s.name.clone(), s.elevation)).collect()
    };
    let mut levels: Vec<Level> = Vec::new();
    for (k, (sname, elev)) in src.into_iter().enumerate() {
        let p = st.pt([0.0, 0.0, elev]);
        let ax = st.l(&format!("IFCAXIS2PLACEMENT3D({p},$,$)"));
        let pl = st.place(&bld_pl, Some(ax), &origin);
        let gk = if k > 0 { format!("storey|{k}") } else { "storey".into() };
        let id = st.l(&format!("IFCBUILDINGSTOREY({},$,{},$,$,{pl},$,$,.ELEMENT.,{})", st.g(&gk), step_string(&sname), real(elev)));
        levels.push(Level { name: sname, elevation: elev, pl, id, items: Vec::new() });
    }
    let _ = levels.first().map(|l| &l.name);
    st.l(&format!("IFCRELAGGREGATES({},$,$,$,{project},({site}))", st.g("agg-project")));
    st.l(&format!("IFCRELAGGREGATES({},$,$,$,{site},({building}))", st.g("agg-site")));
    let ids = levels.iter().map(|l| l.id.clone()).collect::<Vec<_>>().join(",");
    st.l(&format!("IFCRELAGGREGATES({},$,$,$,{building},({ids}))", st.g("agg-building")));

    let level_of = |levels: &[Level], z: f64| -> usize {
        let mut s = 0usize;
        for (i, l) in levels.iter().enumerate() {
            if l.elevation <= z + 1e-6 {
                s = i;
            }
        }
        s
    };
    // faces: loops of points in the proxy's coordinates
    let proxy = |st: &mut Step, levels: &mut Vec<Level>, key2: &str, label: &str, faces: &[Vec<Vec<P3>>], closed: bool, m: Option<&M4>| {
        if faces.is_empty() {
            return;
        }
        let mut z = f64::INFINITY;
        for f in faces {
            for p in f.first().map_or(&[][..], Vec::as_slice) {
                z = js::min(
                    z,
                    match m {
                        Some(m) => apply(m, *p)[2],
                        None => p[2],
                    },
                );
            }
        }
        let li = level_of(levels, z);
        let (lv_elev, lv_pl) = levels.get(li).map(|l| (l.elevation, l.pl.clone())).unwrap_or((0.0, String::new()));
        let mut pts: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        let mut fs: Vec<String> = Vec::with_capacity(faces.len());
        for f in faces {
            let mut bounds: Vec<String> = Vec::with_capacity(f.len());
            for (j, l) in f.iter().enumerate() {
                let mut refs: Vec<String> = Vec::with_capacity(l.len());
                for p in l {
                    let k2 = p.iter().map(|&x| fixed(x, 9)).collect::<Vec<_>>().join(",");
                    let r = match pts.get(&k2) {
                        Some(r) => r.clone(),
                        None => {
                            let r = st.pt(*p);
                            pts.insert(k2, r.clone());
                            r
                        }
                    };
                    refs.push(r);
                }
                let lp = st.l(&format!("IFCPOLYLOOP(({}))", refs.join(",")));
                bounds.push(st.l(&format!("{}({lp},.T.)", if j > 0 { "IFCFACEBOUND" } else { "IFCFACEOUTERBOUND" })));
            }
            fs.push(st.l(&format!("IFCFACE(({}))", bounds.join(","))));
        }
        let (item, kind) = if closed {
            let sh = st.l(&format!("IFCCLOSEDSHELL(({}))", fs.join(",")));
            (st.l(&format!("IFCFACETEDBREP({sh})")), "Brep")
        } else {
            let sh = st.l(&format!("IFCOPENSHELL(({}))", fs.join(",")));
            (st.l(&format!("IFCSHELLBASEDSURFACEMODEL(({sh}))")), "SurfaceModel")
        };
        let rep = st.l(&format!("IFCSHAPEREPRESENTATION({body},'Body','{kind}',({item}))"));
        let shape = st.l(&format!("IFCPRODUCTDEFINITIONSHAPE($,$,({rep}))"));
        // placed relative to its storey, so it stays where it is in the world
        let axes = match m {
            Some(m) => {
                let p = st.pt([m[12], m[13], m[14] - lv_elev]);
                let d1 = st.dir(norm([m[8], m[9], m[10]]));
                let d2 = st.dir(norm([m[0], m[1], m[2]]));
                Some(st.l(&format!("IFCAXIS2PLACEMENT3D({p},{d1},{d2})")))
            }
            None if lv_elev != 0.0 && !lv_elev.is_nan() => {
                let p = st.pt([0.0, 0.0, -lv_elev]);
                Some(st.l(&format!("IFCAXIS2PLACEMENT3D({p},$,$)")))
            }
            None => None,
        };
        let pl = st.place(&lv_pl, axes, &origin);
        let el = st.l(&format!(
            "IFCBUILDINGELEMENTPROXY({},$,{},$,$,{pl},{shape},$,.NOTDEFINED.)",
            st.g(&format!("proxy|{key2}")),
            step_string(label)
        ));
        if let Some(l) = levels.get_mut(li) {
            l.items.push(el);
        }
    };
    let faces_of = |def: &str, m0: &M4| -> Vec<Vec<Vec<P3>>> {
        let mut out = Vec::new();
        model.walk(def, m0, &mut |d, wm, _, _| {
            let mirror = det3(wm) < 0.0;
            for f in d.faces.values() {
                let mut loops: Vec<Vec<P3>> = f.loops.iter().map(|l| l.iter().map(|&v| apply(wm, d.v(v))).collect()).collect();
                if mirror {
                    for l in &mut loops {
                        l.reverse();
                    }
                }
                out.push(loops);
            }
            true
        });
        out
    };
    if let Some(root) = model.root() {
        for (iid, inst) in root.insts.iter() {
            let Some(d) = model.def(&inst.def) else { continue };
            let closed = d.insts.is_empty() && solid_info(d).closed;
            let label = inst.name.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| d.name.clone());
            let key2 = format!("i{}", js::num_str(iid));
            if rigid(&inst.m) {
                let fs = faces_of(&inst.def, &IDENT);
                proxy(&mut st, &mut levels, &key2, &label, &fs, closed, Some(&inst.m));
            } else {
                let fs = faces_of(&inst.def, &inst.m);
                proxy(&mut st, &mut levels, &key2, &label, &fs, closed && det3(&inst.m) > 0.0, None);
            }
        }
        if !root.faces.is_empty() {
            let own: Vec<Vec<Vec<P3>>> =
                root.faces.values().map(|f| f.loops.iter().map(|l| l.iter().map(|&v| root.v(v)).collect()).collect()).collect();
            proxy(&mut st, &mut levels, "loose", "Loose geometry", &own, solid_info(root).closed, None);
        }
    }
    for (k, lv) in levels.iter().enumerate() {
        if !lv.items.is_empty() {
            let gk = if k > 0 { format!("contained|{k}") } else { "contained".into() };
            let g = st.g(&gk);
            st.l(&format!("IFCRELCONTAINEDINSPATIALSTRUCTURE({g},$,$,$,({}),{})", lv.items.join(","), lv.id));
        }
    }
    let ts = js::slice16(date_iso, 19);
    let mut out = vec![
        "ISO-10303-21;".to_string(),
        "HEADER;".into(),
        "FILE_DESCRIPTION(('ViewDefinition [ReferenceView]'),'2;1');".into(),
        format!("FILE_NAME({},'{ts}',(''),(''),'ModelCraft','ModelCraft','');", step_string(&format!("{name}.ifc"))),
        "FILE_SCHEMA(('IFC4'));".into(),
        "ENDSEC;".into(),
        "DATA;".into(),
    ];
    out.extend(st.lines);
    out.push("ENDSEC;".into());
    out.push("END-ISO-10303-21;".into());
    out.push(String::new());
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strings() {
        assert_eq!(step_string("it's \\ ok"), "'it''s \\\\ ok'");
        assert_eq!(step_string("Pavilion — test"), "'Pavilion \\X2\\2014\\X0\\ test'");
        assert_eq!(real(1.0), "1.");
        assert_eq!(real(-0.0), "0.");
        assert_eq!(real(1e-7), "1.000000000E-7");
        assert_eq!(ifc_guid("x").len(), 22);
    }
}
