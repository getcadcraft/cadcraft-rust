//! ifc-export.js — IFC4 export of a part or an assembly with every property, written by hand as ISO 10303-21 text:
//! project / site / building / storey, the part as an IfcBuildingElementProxy with an IfcTriangulatedFaceSet body
//! (Y up becomes IFC's Z up), an assembly as an IfcElementAssembly of placed occurrences typed by one proxy type per
//! part; property sets for the part, its parameters and title block, the instance and the quantities. GlobalIds come
//! from names (cyrb128), so the same file exports with the same ids.

use crate::js;
use crate::jv;
use crate::params::{self, Params};
use serde_json::Value;
use std::collections::HashMap;

const B64: &[u8; 64] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$";

/// `guidFor(seed)`.
pub fn guid_for(seed: &str) -> String {
    let (mut h1, mut h2, mut h3, mut h4): (u32, u32, u32, u32) = (1_779_033_703, 3_144_134_277, 1_013_904_242, 2_773_480_762);
    for k in seed.encode_utf16().map(u32::from) {
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
    let mut out = [b'0'; 22];
    for slot in out.iter_mut().rev() {
        *slot = B64.get(usize::try_from(n & 63).unwrap_or(0)).copied().unwrap_or(b'0');
        n >>= 6;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `R(v, digits)`.
pub fn r(v: f64, digits: usize) -> String {
    if !v.is_finite() {
        return "0.".into();
    }
    let lim = 0.5 * js::pow(10.0, -js::f(digits));
    let mut s = js::to_fixed(if v.abs() < lim { 0.0 } else { v }, digits);
    // /(\.\d*?)0+$/ → '$1'
    if let Some(dot) = s.find('.') {
        let trimmed = s.trim_end_matches('0');
        s = if trimmed.len() > dot { trimmed.to_string() } else { s.get(..dot.saturating_add(1)).unwrap_or("").to_string() };
    }
    if !s.contains('.') {
        s.push('.');
    }
    if s == "-0." { "0.".into() } else { s }
}
/// `Rq(v)`.
pub fn rq(v: f64) -> String {
    if !v.is_finite() {
        return "0.".into();
    }
    let s = js::num_str(js::prec_num(v, 12));
    if let Some((m, e)) = s.split_once('e') {
        return format!("{}E{}", if m.contains('.') { m.to_string() } else { format!("{m}.") }, e.replacen('+', "", 1));
    }
    if s.contains('.') { s } else { format!("{s}.") }
}
/// `S(str)`.
pub fn s(st: &str) -> String {
    let mut out = String::from("'");
    for ch in st.chars() {
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
            out.push_str(&format!("\\X2\\{}\\X0\\", units.join("")));
        }
    }
    out.push('\'');
    out
}

struct Step {
    lines: Vec<String>,
    cache: HashMap<String, String>,
}
impl Step {
    fn add(&mut self, st: String) -> String {
        self.lines.push(format!("#{}={st};", self.lines.len().saturating_add(1)));
        format!("#{}", self.lines.len())
    }
    fn once(&mut self, st: String) -> String {
        if let Some(x) = self.cache.get(&st) {
            return x.clone();
        }
        let id = self.add(st.clone());
        self.cache.insert(st, id.clone());
        id
    }
    fn pt(&mut self, p: [f64; 3]) -> String {
        self.once(format!("IFCCARTESIANPOINT(({}))", p.iter().map(|x| r(*x, 6)).collect::<Vec<_>>().join(",")))
    }
    fn dir(&mut self, d: [f64; 3]) -> String {
        self.once(format!("IFCDIRECTION(({}))", d.iter().map(|x| r(*x, 12)).collect::<Vec<_>>().join(",")))
    }
}
fn to_z(p: [f64; 3]) -> [f64; 3] {
    [p[0], -p[2], p[1]]
}

struct Fx {
    w: Step,
    wcs: String,
    body: String,
    storey: String,
    st_lp: String,
    prefix: String,
    counts: [usize; 4],
}
impl Fx {
    fn seed(&self, k: &str) -> String {
        guid_for(&format!("{}{k}", self.prefix))
    }
}

fn begin(name: &str, prefix: String) -> Fx {
    let mut w = Step { lines: Vec::new(), cache: HashMap::new() };
    let o = w.pt([0.0; 3]);
    let wcs = w.add(format!("IFCAXIS2PLACEMENT3D({o},$,$)"));
    let ctx = w.add(format!("IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,{wcs},$)"));
    let body = w.add(format!("IFCGEOMETRICREPRESENTATIONSUBCONTEXT('Body','Model',*,*,*,*,{ctx},$,.MODEL_VIEW.,$)"));
    let kg = w.add("IFCSIUNIT(*,.MASSUNIT.,.KILO.,.GRAM.)".into());
    let metre = w.add("IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.)".into());
    let e1 = w.add(format!("IFCDERIVEDUNITELEMENT({kg},1)"));
    let e2 = w.add(format!("IFCDERIVEDUNITELEMENT({metre},-3)"));
    let density = w.add(format!("IFCDERIVEDUNIT(({e1},{e2}),.MASSDENSITYUNIT.,$)"));
    let u1 = w.add("IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.)".into());
    let u2 = w.add("IFCSIUNIT(*,.AREAUNIT.,$,.SQUARE_METRE.)".into());
    let u3 = w.add("IFCSIUNIT(*,.VOLUMEUNIT.,$,.CUBIC_METRE.)".into());
    let u4 = w.add("IFCSIUNIT(*,.PLANEANGLEUNIT.,$,.RADIAN.)".into());
    let ua = w.add(format!("IFCUNITASSIGNMENT(({u1},{u2},{u3},{u4},{kg},{density}))"));
    let mut f = Fx { w, wcs: wcs.clone(), body, storey: String::new(), st_lp: String::new(), prefix, counts: [0; 4] };
    let project = {
        let g = s(&f.seed("project"));
        f.w.add(format!("IFCPROJECT({g},$,{},$,$,$,$,({ctx}),{ua})", s(name)))
    };
    let site_lp = f.w.add(format!("IFCLOCALPLACEMENT($,{wcs})"));
    let site = {
        let g = s(&f.seed("site"));
        f.w.add(format!("IFCSITE({g},$,'Site',$,$,{site_lp},$,$,.ELEMENT.,$,$,$,$,$)"))
    };
    let bld_lp = f.w.add(format!("IFCLOCALPLACEMENT({site_lp},{wcs})"));
    let bld = {
        let g = s(&f.seed("building"));
        f.w.add(format!("IFCBUILDING({g},$,'Building',$,$,{bld_lp},$,$,.ELEMENT.,$,$,$)"))
    };
    let st_lp = f.w.add(format!("IFCLOCALPLACEMENT({bld_lp},{wcs})"));
    let storey = {
        let g = s(&f.seed("storey"));
        f.w.add(format!("IFCBUILDINGSTOREY({g},$,'PartCraft',$,$,{st_lp},$,$,.ELEMENT.,0.)"))
    };
    for (k, a, b) in [("rel-site", &project, &site), ("rel-bld", &site, &bld), ("rel-storey", &bld, &storey)] {
        let g = s(&f.seed(k));
        f.w.add(format!("IFCRELAGGREGATES({g},$,$,$,{a},({b}))"));
    }
    f.storey = storey;
    f.st_lp = st_lp;
    f
}

fn finish(f: Fx, file_name: &str, ts: &str) -> (String, Value) {
    let mut lines: Vec<String> = vec![
        "ISO-10303-21;".into(),
        "HEADER;".into(),
        "FILE_DESCRIPTION(('ViewDefinition [ReferenceView_V1.2]'),'2;1');".into(),
        format!("FILE_NAME({},{},(''),(''),'PartCraft','PartCraft','');", s(file_name), s(ts)),
        "FILE_SCHEMA(('IFC4'));".into(),
        "ENDSEC;".into(),
        "DATA;".into(),
    ];
    lines.extend(f.w.lines);
    lines.push("ENDSEC;".into());
    lines.push("END-ISO-10303-21;".into());
    lines.push(String::new());
    let c = f.counts;
    (
        lines.join("\n"),
        jv::obj(vec![
            ("elements", jv::unum(c[0])),
            ("psets", jv::unum(c[1])),
            ("properties", jv::unum(c[2])),
            ("triangles", jv::unum(c[3])),
        ]),
    )
}

fn face_set(f: &mut Fx, positions: &[f64], indices: &[u32]) -> String {
    let pts: Vec<String> = positions
        .chunks(3)
        .map(|c| {
            let g = |i: usize| c.get(i).copied().unwrap_or(f64::NAN);
            format!("({})", to_z([g(0), g(1), g(2)]).iter().map(|x| r(*x, 6)).collect::<Vec<_>>().join(","))
        })
        .collect();
    let tris: Vec<String> = indices
        .chunks(3)
        .map(|c| {
            let g = |i: usize| c.get(i).map_or_else(|| "NaN".to_string(), |x| u64::from(*x).saturating_add(1).to_string());
            format!("({},{},{})", g(0), g(1), g(2))
        })
        .collect();
    f.counts[3] = f.counts[3].saturating_add(indices.len() / 3);
    let list = f.w.add(format!("IFCCARTESIANPOINTLIST3D(({}),$)", pts.join(",")));
    f.w.add(format!("IFCTRIANGULATEDFACESET({list},$,.T.,({}),$)", tris.join(",")))
}
fn shape_rep(f: &mut Fx, items: &[String], ty: &str) -> String {
    let body = f.body.clone();
    f.w.add(format!("IFCSHAPEREPRESENTATION({body},'Body',{},({}))", s(ty), items.join(",")))
}

/// A property value.
enum Pv {
    Bool(bool),
    Num(f64),
    Measure(&'static str, f64),
    Text(String),
}
fn prop(f: &mut Fx, name: &str, value: Option<Pv>, desc: Option<String>) -> Option<String> {
    let value = value?;
    let v = match value {
        Pv::Bool(b) => format!("IFCBOOLEAN({})", if b { ".T." } else { ".F." }),
        Pv::Num(x) => format!("IFCREAL({})", rq(x)),
        Pv::Measure(m, x) => format!("{m}({})", rq(x)),
        Pv::Text(t) => {
            if t.is_empty() {
                return None;
            }
            format!("IFCLABEL({})", s(&js::slice16(&t, 255)))
        }
    };
    f.counts[2] = f.counts[2].saturating_add(1);
    let d = desc.filter(|d| !d.is_empty()).map_or_else(|| "$".to_string(), |d| s(&js::slice16(&d, 255)));
    Some(f.w.add(format!("IFCPROPERTYSINGLEVALUE({},{d},{v},$)", s(name))))
}
fn pset(f: &mut Fx, key: &str, name: &str, props: Vec<(&str, Option<Pv>, Option<String>)>) -> Option<String> {
    let ids: Vec<String> = props.into_iter().filter_map(|(n, v, d)| prop(f, n, v, d)).collect();
    if ids.is_empty() {
        return None;
    }
    f.counts[1] = f.counts[1].saturating_add(1);
    let g = s(&f.seed(&format!("{key}|{name}")));
    Some(f.w.add(format!("IFCPROPERTYSET({g},$,{},$,({}))", s(name), ids.join(","))))
}
fn rel_props(f: &mut Fx, key: &str, objects: &[String], set: &str) -> String {
    let g = s(&f.seed(&format!("rdp|{key}")));
    f.w.add(format!("IFCRELDEFINESBYPROPERTIES({g},$,$,$,({}),{set})", objects.join(",")))
}

fn text_pv(v: Option<&Value>) -> Option<Pv> {
    match v {
        None | Some(Value::Null) => None,
        Some(Value::Bool(b)) => Some(Pv::Bool(*b)),
        Some(Value::Number(n)) => Some(Pv::Num(n.as_f64().unwrap_or(f64::NAN))),
        Some(x) => Some(Pv::Text(js::string_of(Some(x)))),
    }
}

fn quantities(f: &mut Fx, key: &str, stats: Option<&Value>, mat: Option<&Value>) -> Vec<String> {
    let mut out = Vec::new();
    let Some(st) = stats.filter(|x| jv::truthy(Some(x))) else { return out };
    let n = |k: &str| jv::as_num(st.get(k)).unwrap_or(f64::NAN);
    let q = |f: &mut Fx, ty: &str, name: &str, v: f64| -> Option<String> {
        v.is_finite().then(|| f.w.add(format!("IFCQUANTITY{ty}({},$,$,{},$)", s(name), rq(v))))
    };
    let std: Vec<String> =
        [q(f, "VOLUME", "NetVolume", n("volume") * 1e-9), q(f, "AREA", "NetSurfaceArea", n("area") * 1e-6)].into_iter().flatten().collect();
    if !std.is_empty() {
        let g = s(&f.seed(&format!("qto|{key}")));
        out.push(f.w.add(format!("IFCELEMENTQUANTITY({g},$,'Qto_BuildingElementProxyQuantities',$,$,({}))", std.join(","))));
    }
    let p3 = |x: Option<&Value>| {
        let a = jv::arr(x);
        let g = |i: usize| jv::as_num(a.get(i)).unwrap_or(f64::NAN);
        [g(0), g(1), g(2)]
    };
    let bb = st.get("bbox");
    let lo = to_z(p3(bb.and_then(|b| b.get("min"))));
    let hi = to_z(p3(bb.and_then(|b| b.get("max"))));
    let size = [(hi[0] - lo[0]).abs(), (hi[1] - lo[1]).abs(), (hi[2] - lo[2]).abs()];
    let weight = mat
        .filter(|m| jv::truthy(Some(m)))
        .and_then(|m| q(f, "WEIGHT", "NetWeight", n("volume") * jv::as_num(m.get("density")).unwrap_or(f64::NAN) * 1e-6));
    let own: Vec<String> = [
        weight,
        q(f, "LENGTH", "OverallLength", size[0]),
        q(f, "LENGTH", "OverallWidth", size[1]),
        q(f, "LENGTH", "OverallDepth", size[2]),
    ]
    .into_iter()
    .flatten()
    .collect();
    if !own.is_empty() {
        let g = s(&f.seed(&format!("qpc|{key}")));
        out.push(f.w.add(format!("IFCELEMENTQUANTITY({g},$,'PartCraft_Quantities',$,$,({}))", own.join(","))));
    }
    f.counts[1] = f.counts[1].saturating_add(out.len());
    out
}

fn parameter_props(part: &Value) -> Vec<(String, Option<Pv>, Option<String>)> {
    let vars = jv::arr(part.get("params").and_then(|p| p.get("vars")));
    if vars.is_empty() {
        return Vec::new();
    }
    let p = part.get("params").map(Params::from_json).unwrap_or_default();
    let ev = params::evaluate_vars(&p, None);
    vars.iter()
        .map(|v| {
            let name = js::string_of(v.get("name"));
            let val = ev.values.get(&name).copied();
            let val_s = val.map_or_else(|| "undefined".to_string(), js::num_str);
            let expr = js::string_of(v.get("expr"));
            let mut parts = Vec::new();
            if expr != val_s {
                parts.push(format!("= {expr}"));
            }
            if jv::truthy(v.get("note")) {
                parts.push(js::string_of(v.get("note")));
            }
            (name, val.filter(|x| x.is_finite()).map(Pv::Num), Some(parts.join("; ")))
        })
        .collect()
}

fn part_psets(f: &mut Fx, key: &str, part: &Value, extra: &Value) -> Vec<String> {
    let mat =
        extra.get("material").filter(|m| jv::truthy(Some(m))).or_else(|| part.get("material").filter(|m| jv::truthy(Some(m)))).cloned();
    let mut sets = Vec::new();
    let name_v = extra.get("name").filter(|x| jv::truthy(Some(x))).or_else(|| part.get("name")).cloned();
    let bodies = jv::finite(extra.get("bodies"));
    let density = mat.as_ref().map(|m| Pv::Measure("IFCMASSDENSITYMEASURE", jv::as_num(m.get("density")).unwrap_or(f64::NAN) * 1000.0));
    let config = part.get("configs").and_then(|c| c.get("active")).filter(|x| jv::truthy(Some(x))).cloned();
    sets.push(pset(
        f,
        key,
        "PartCraft_Part",
        vec![
            ("Name", text_pv(name_v.as_ref()), None),
            ("PartNumber", text_pv(extra.get("number")), None),
            ("Description", text_pv(extra.get("description")), None),
            ("Material", text_pv(mat.as_ref().and_then(|m| m.get("name"))), None),
            ("Density", density, None),
            ("Configuration", text_pv(config.as_ref()), None),
            ("Bodies", bodies.map(Pv::Num), None),
        ],
    ));
    let pp = parameter_props(part);
    sets.push(pset(
        f,
        key,
        "PartCraft_Parameters",
        pp.iter()
            .map(|(n, v, d)| {
                (
                    n.as_str(),
                    v.as_ref().map(|x| match x {
                        Pv::Num(y) => Pv::Num(*y),
                        _ => Pv::Num(f64::NAN),
                    }),
                    d.clone(),
                )
            })
            .collect(),
    ));
    let tb = part.get("drawing").and_then(|d| d.get("title")).filter(|x| jv::truthy(Some(x))).cloned().unwrap_or(Value::Null);
    let keys = [
        ("Title", "title"),
        ("DrawingNumber", "number"),
        ("Revision", "rev"),
        ("Company", "company"),
        ("DrawnBy", "drawnBy"),
        ("CheckedBy", "checkedBy"),
        ("ApprovedBy", "approvedBy"),
        ("Date", "date"),
        ("Finish", "finish"),
        ("GeneralTolerance", "tolerance"),
    ];
    let mut tbp: Vec<(&str, Option<Pv>, Option<String>)> = keys.iter().map(|(n, k)| (*n, text_pv(tb.get(*k)), None)).collect();
    let sheet = part
        .get("drawing")
        .and_then(|d| d.get("sheet"))
        .filter(|x| jv::truthy(Some(x)))
        .map(|x| Pv::Text(js::upper(&js::string_of(Some(x)))));
    tbp.push(("SheetSize", sheet, None));
    sets.push(pset(f, key, "PartCraft_TitleBlock", tbp));
    sets.into_iter().flatten().collect()
}

fn material(f: &mut Fx, mats: &mut Vec<(String, String)>, mat: Option<&Value>, objects: &[String], key: &str) {
    let Some(m) = mat.filter(|m| jv::truthy(Some(m))) else { return };
    if objects.is_empty() {
        return;
    }
    let name = js::string_of(m.get("name"));
    let id = match mats.iter().find(|(k, _)| *k == name) {
        Some((_, id)) => id.clone(),
        None => {
            let id = f.w.add(format!("IFCMATERIAL({},$,$)", s(&name)));
            mats.push((name, id.clone()));
            id
        }
    };
    let g = s(&f.seed(&format!("mat|{key}")));
    f.w.add(format!("IFCRELASSOCIATESMATERIAL({g},$,$,$,({}),{id})", objects.join(",")));
}

/// `partIfc(part, regen, { now, fileName })`; regen = { mesh: { positions, indices }, stats }; ts = now ISO (19 chars).
pub fn part_ifc(
    part: &Value,
    positions: &[f64],
    indices: &[u32],
    stats: Option<&Value>,
    ts: &str,
    file_name: Option<&str>,
) -> (String, Value) {
    let name = if jv::truthy(part.get("name")) { js::string_of(part.get("name")) } else { "Part".to_string() };
    let mut f = begin(&name, format!("partcraft|{name}|"));
    let fs = face_set(&mut f, positions, indices);
    let rep = shape_rep(&mut f, &[fs], "Tessellation");
    let pds = f.w.add(format!("IFCPRODUCTDEFINITIONSHAPE($,$,({rep}))"));
    let (st_lp, wcs) = (f.st_lp.clone(), f.wcs.clone());
    let lp = f.w.add(format!("IFCLOCALPLACEMENT({st_lp},{wcs})"));
    let tag = part
        .get("drawing")
        .and_then(|d| d.get("title"))
        .and_then(|t| t.get("number"))
        .filter(|x| jv::truthy(Some(x)))
        .map(|x| s(&js::string_of(Some(x))));
    let g = s(&f.seed("part"));
    let el = f.w.add(format!(
        "IFCBUILDINGELEMENTPROXY({g},$,{},$,{},{lp},{pds},{},.USERDEFINED.)",
        s(&name),
        s("Part"),
        tag.unwrap_or_else(|| "$".into())
    ));
    f.counts[0] = f.counts[0].saturating_add(1);
    let (gc, storey) = (s(&f.seed("contained")), f.storey.clone());
    f.w.add(format!("IFCRELCONTAINEDINSPATIALSTRUCTURE({gc},$,$,$,({el}),{storey})"));
    let bodies = stats.and_then(|st| st.get("bodies")).cloned().unwrap_or(Value::Null);
    let extra = jv::obj(vec![("bodies", bodies)]);
    for (i, set) in part_psets(&mut f, "part", part, &extra).iter().enumerate() {
        rel_props(&mut f, &format!("part|{i}"), std::slice::from_ref(&el), set);
    }
    for (i, set) in quantities(&mut f, "part", stats, part.get("material")).iter().enumerate() {
        rel_props(&mut f, &format!("partq|{i}"), std::slice::from_ref(&el), set);
    }
    let mut mats = Vec::new();
    material(&mut f, &mut mats, part.get("material"), &[el], "part");
    let fname = file_name.map_or_else(|| format!("{name}.ifc"), str::to_string);
    finish(f, &fname, ts)
}

fn placement_of(f: &mut Fx, m: &[f64; 16]) -> String {
    let ex = to_z([m[0], m[1], m[2]]);
    let ez = to_z([m[4], m[5], m[6]]);
    let o = to_z([m[12], m[13], m[14]]);
    let p = f.w.pt(o);
    let dz = f.w.dir(ez);
    let dx = f.w.dir(ex);
    f.w.add(format!("IFCAXIS2PLACEMENT3D({p},{dz},{dx})"))
}

/// A built part for the assembly export.
pub struct BuiltPart {
    pub positions: Vec<f64>,
    pub indices: Vec<u32>,
    pub stats: Option<Value>,
}

/// `assemblyIfc(asm, builds, world, { now, readPart, fileName, bom })`.
pub fn assembly_ifc(
    asm: &Value,
    builds: &HashMap<String, BuiltPart>,
    world: &[(String, [f64; 16])],
    bom: &[Value],
    ts: &str,
    file_name: Option<&str>,
) -> Result<(String, Value), String> {
    let name = if jv::truthy(asm.get("name")) { js::string_of(asm.get("name")) } else { "Assembly".to_string() };
    let mut f = begin(&name, format!("partcraft-asm|{name}|"));
    let (st_lp, wcs) = (f.st_lp.clone(), f.wcs.clone());
    let lp = f.w.add(format!("IFCLOCALPLACEMENT({st_lp},{wcs})"));
    let g = s(&f.seed("assembly"));
    let assembly = f.w.add(format!("IFCELEMENTASSEMBLY({g},$,{},$,{},{lp},$,$,.FACTORY.,.USERDEFINED.)", s(&name), s("Assembly")));
    f.counts[0] = f.counts[0].saturating_add(1);
    let (gc, storey) = (s(&f.seed("contained")), f.storey.clone());
    f.w.add(format!("IFCRELCONTAINEDINSPATIALSTRUCTURE({gc},$,$,$,({assembly}),{storey})"));
    struct Ty {
        ty: String,
        map: String,
        stats: Option<Value>,
        mat: Option<Value>,
    }
    let mut types: Vec<(String, Ty)> = Vec::new();
    let mut mats: Vec<(String, String)> = Vec::new();
    let mut by_type: Vec<(String, Vec<String>)> = Vec::new();
    let mut parts_out: Vec<String> = Vec::new();
    let item_of = |pid: &str| bom.iter().rev().find(|r| jv::is_str(r.get("pid"), pid)).and_then(|r| r.get("item").cloned());
    if let Some(ps) = asm.get("parts").and_then(Value::as_object) {
        for (pid, p) in ps {
            let Some(b) = builds.get(pid) else { continue };
            let part = match p.get("part") {
                Some(Value::String(st)) => crate::part::read_part(&Value::String(st.clone()))?,
                Some(x) => x.clone(),
                None => Value::Null,
            };
            let fs = face_set(&mut f, &b.positions, &b.indices);
            let rep = shape_rep(&mut f, &[fs], "Tessellation");
            let wcs = f.wcs.clone();
            let map = f.w.add(format!("IFCREPRESENTATIONMAP({wcs},{rep})"));
            let extra = jv::obj(vec![
                ("name", p.get("name").cloned().unwrap_or(Value::Null)),
                ("number", p.get("number").cloned().unwrap_or(Value::Null)),
                ("description", p.get("description").cloned().unwrap_or(Value::Null)),
                ("material", p.get("material").cloned().unwrap_or(Value::Null)),
                ("bodies", b.stats.as_ref().and_then(|st| st.get("bodies")).cloned().unwrap_or(Value::Null)),
            ]);
            let sets = part_psets(&mut f, &format!("type|{pid}"), &part, &extra);
            let tname = if jv::truthy(p.get("name")) {
                js::string_of(p.get("name"))
            } else if jv::truthy(part.get("name")) {
                js::string_of(part.get("name"))
            } else {
                pid.clone()
            };
            let num = if jv::truthy(p.get("number")) { s(&js::string_of(p.get("number"))) } else { "$".into() };
            let g = s(&f.seed(&format!("type|{pid}")));
            let ty = f.w.add(format!(
                "IFCBUILDINGELEMENTPROXYTYPE({g},$,{},$,$,{},({map}),{num},$,.USERDEFINED.)",
                s(&tname),
                if sets.is_empty() { "$".to_string() } else { format!("({})", sets.join(",")) }
            ));
            let mat =
                p.get("material").filter(|m| jv::truthy(Some(m))).or_else(|| part.get("material").filter(|m| jv::truthy(Some(m)))).cloned();
            types.push((pid.clone(), Ty { ty, map, stats: b.stats.clone(), mat }));
            by_type.push((pid.clone(), Vec::new()));
        }
    }
    for (i, inst) in jv::arr(asm.get("instances")).iter().enumerate() {
        let ipart = js::str_of(inst.get("part"));
        let iid = js::str_of(inst.get("id"));
        let Some((_, tdef)) = types.iter().find(|(k, _)| *k == ipart) else { continue };
        let Some((_, m)) = world.iter().find(|(k, _)| *k == iid) else { continue };
        let (tmap, tstats, tmat) = (tdef.map.clone(), tdef.stats.clone(), tdef.mat.clone());
        let pl = placement_of(&mut f, m);
        let place = f.w.add(format!("IFCLOCALPLACEMENT({lp},{pl})"));
        let o = f.w.pt([0.0; 3]);
        let op = f.w.once(format!("IFCCARTESIANTRANSFORMATIONOPERATOR3D($,$,{o},1.,$)"));
        let mapped = f.w.add(format!("IFCMAPPEDITEM({tmap},{op})"));
        let rep = shape_rep(&mut f, &[mapped], "MappedRepresentation");
        let pds = f.w.add(format!("IFCPRODUCTDEFINITIONSHAPE($,$,({rep}))"));
        let p = asm.get("parts").and_then(|ps| ps.get(&ipart)).cloned().unwrap_or(Value::Null);
        let iname = if jv::truthy(inst.get("name")) { js::string_of(inst.get("name")) } else { js::string_of(p.get("name")) };
        let ptype = if jv::truthy(p.get("name")) { js::string_of(p.get("name")) } else { "Part".to_string() };
        let num = if jv::truthy(p.get("number")) { s(&js::string_of(p.get("number"))) } else { "$".into() };
        let g = s(&f.seed(&format!("inst|{iid}")));
        let occ = f.w.add(format!("IFCBUILDINGELEMENTPROXY({g},$,{},$,{},{place},{pds},{num},.USERDEFINED.)", s(&iname), s(&ptype)));
        f.counts[0] = f.counts[0].saturating_add(1);
        parts_out.push(occ.clone());
        if let Some((_, list)) = by_type.iter_mut().find(|(k, _)| *k == ipart) {
            list.push(occ.clone());
        }
        let item = item_of(&ipart);
        let ins = pset(
            &mut f,
            &format!("inst|{iid}"),
            "PartCraft_Instance",
            vec![
                ("InstanceName", text_pv(inst.get("name")), None),
                ("InstanceId", text_pv(inst.get("id")), None),
                ("Item", text_pv(item.as_ref()), None),
                ("Grounded", Some(Pv::Bool(i == 0)), None),
            ],
        );
        if let Some(ins) = ins {
            rel_props(&mut f, &format!("inst|{iid}"), std::slice::from_ref(&occ), &ins);
        }
        for (k, set) in quantities(&mut f, &format!("inst|{iid}"), tstats.as_ref(), tmat.as_ref()).iter().enumerate() {
            rel_props(&mut f, &format!("instq|{iid}|{k}"), std::slice::from_ref(&occ), set);
        }
    }
    if !parts_out.is_empty() {
        let g = s(&f.seed("rel-asm"));
        f.w.add(format!("IFCRELAGGREGATES({g},$,$,$,{assembly},({}))", parts_out.join(",")));
    }
    for (pid, occs) in &by_type {
        if occs.is_empty() {
            continue;
        }
        let Some((_, tdef)) = types.iter().find(|(k, _)| k == pid) else { continue };
        let (ty, mat) = (tdef.ty.clone(), tdef.mat.clone());
        let g = s(&f.seed(&format!("rdt|{pid}")));
        f.w.add(format!("IFCRELDEFINESBYTYPE({g},$,$,$,({}),{ty})", occs.join(",")));
        material(&mut f, &mut mats, mat.as_ref(), occs, &format!("type|{pid}"));
    }
    let fname = file_name.map_or_else(|| format!("{name}.ifc"), str::to_string);
    Ok(finish(f, &fname, ts))
}
