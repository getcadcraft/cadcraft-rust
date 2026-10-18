//! new-model.js — a new, empty building as IFC4 text: owner history (BimCraft), units (mm), the 3D context with Body /
//! FootPrint / Axis sub-contexts, IfcProject → IfcSite → IfcBuilding → one IfcBuildingStorey per level, each level's
//! height in a CadCraft_Level property set. Names, the clock and the GlobalIds come from the caller.

use crate::draw::m;
use crate::draw::tr::t;
use crate::js::num_str;
use crate::wapi::{Rand, guid_from_bytes};

/// A level of a new building (mm).
#[derive(Clone, Debug, PartialEq)]
pub struct NewLevel {
    pub name: String,
    pub elevation: f64,
    pub height: f64,
}

/// `ifcGuid(rand)`: 16 random bytes as a version-4 GlobalId.
pub fn ifc_guid(r: &mut Rand) -> String {
    r.guid()
}

/// Kept for callers that have the bytes already.
pub fn guid_of(b: &[u8; 16]) -> String {
    let mut c = *b;
    c[6] = (c[6] & 0x0f) | 0x40;
    c[8] = (c[8] & 0x3f) | 0x80;
    guid_from_bytes(&c)
}

/// `stepString(s)`: quotes and backslashes doubled, U+00A0 and up as \X2\…\X0\ (past U+FFFF as surrogate pairs),
/// control characters dropped.
pub fn step_string(s: &str) -> String {
    let mut out = String::new();
    let mut wide = String::new();
    let flush = |out: &mut String, wide: &mut String| {
        if !wide.is_empty() {
            out.push_str("\\X2\\");
            out.push_str(wide);
            out.push_str("\\X0\\");
            wide.clear();
        }
    };
    for ch in s.chars() {
        let c = u32::from(ch);
        if (0x20..=0x7e).contains(&c) {
            flush(&mut out, &mut wide);
            match ch {
                '\'' => out.push_str("''"),
                '\\' => out.push_str("\\\\"),
                _ => out.push(ch),
            }
        } else if c > 0xffff {
            let mut b = [0u16; 2];
            for u in ch.encode_utf16(&mut b) {
                wide.push_str(&format!("{u:04X}"));
            }
        } else if c >= 0xa0 {
            wide.push_str(&format!("{c:04X}"));
        }
    }
    flush(&mut out, &mut wide);
    format!("'{out}'")
}

/// A STEP real: always with a decimal point (rounded to 6 decimals).
fn real(n: f64) -> String {
    let s = num_str(m::round(n * 1e6) / 1e6);
    if s.contains(['.', 'e', 'E']) { s } else { s + "." }
}

/// `stackLevels(count, heightMm, nameOf)`
pub fn stack_levels(count: usize, height: f64) -> Vec<NewLevel> {
    (0..count)
        .map(|i| NewLevel { name: t("new.level", &[("n", &(i.saturating_add(1)).to_string())]), elevation: i as f64 * height, height })
        .collect()
}

/// `levelsProblem(levels)` → '' or the problem's key.
pub fn levels_problem(levels: &[NewLevel]) -> &'static str {
    if levels.is_empty() || levels.len() > 200 {
        return "new.err.levels";
    }
    for l in levels {
        if !l.height.is_finite() || l.height < 1000.0 || l.height > 100_000.0 {
            return "new.err.height";
        }
        if !l.elevation.is_finite() || l.elevation.abs() > 1e6 {
            return "new.err.elevation";
        }
    }
    ""
}

/// What a new building is.
#[derive(Clone, Debug, PartialEq)]
pub struct NewBuilding {
    pub name: String,
    pub site_name: String,
    pub levels: Vec<NewLevel>,
    pub author: String,
    pub file_name: String,
    /// the clock (ms since 1970)
    pub now_ms: f64,
    pub version: String,
}

/// `newBuilding(spec)` → the IFC4 text, or the problem's key.
pub fn new_building(b: &NewBuilding, rand: &mut Rand) -> Result<String, &'static str> {
    let bad = levels_problem(&b.levels);
    if !bad.is_empty() {
        return Err(bad);
    }
    let mut lines: Vec<String> = Vec::new();
    let mut add = |s: String| -> String {
        lines.push(format!("#{}={s};", lines.len().saturating_add(1)));
        format!("#{}", lines.len())
    };
    let s = step_string;
    let ts = num_str((b.now_ms / 1000.0).floor());
    let person = add(format!("IFCPERSON($,{},$,$,$,$,$,$)", s(&b.author)));
    let org = add(format!("IFCORGANIZATION($,{},$,$,$)", s("CadCraft")));
    let po = add(format!("IFCPERSONANDORGANIZATION({person},{org},$)"));
    let appl = add(format!(
        "IFCAPPLICATION({org},{},{},{})",
        s(if b.version.is_empty() { "dev" } else { &b.version }),
        s("BimCraft"),
        s("BimCraft")
    ));
    let oh = add(format!("IFCOWNERHISTORY({po},{appl},$,.ADDED.,$,$,$,{ts})"));
    let units = [
        add("IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.)".into()),
        add("IFCSIUNIT(*,.AREAUNIT.,$,.SQUARE_METRE.)".into()),
        add("IFCSIUNIT(*,.VOLUMEUNIT.,$,.CUBIC_METRE.)".into()),
        add("IFCSIUNIT(*,.PLANEANGLEUNIT.,$,.RADIAN.)".into()),
    ];
    let unit_assign = add(format!("IFCUNITASSIGNMENT(({}))", units.join(",")));
    let origin = add("IFCCARTESIANPOINT((0.,0.,0.))".into());
    let world = add(format!("IFCAXIS2PLACEMENT3D({origin},$,$)"));
    let north = add("IFCDIRECTION((0.,1.))".into());
    let ctx = add(format!("IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,{world},{north})"));
    add(format!("IFCGEOMETRICREPRESENTATIONSUBCONTEXT('Body','Model',*,*,*,*,{ctx},$,.MODEL_VIEW.,$)"));
    add(format!("IFCGEOMETRICREPRESENTATIONSUBCONTEXT('FootPrint','Model',*,*,*,*,{ctx},$,.MODEL_VIEW.,$)"));
    add(format!("IFCGEOMETRICREPRESENTATIONSUBCONTEXT('Axis','Model',*,*,*,*,{ctx},$,.GRAPH_VIEW.,$)"));
    let project = add(format!("IFCPROJECT({},{oh},{},$,$,$,$,({ctx}),{unit_assign})", s(&rand.guid()), s(&b.name)));
    let site_pl = add(format!("IFCLOCALPLACEMENT($,{world})"));
    let site_name = if b.site_name.is_empty() { &b.name } else { &b.site_name };
    let site = add(format!("IFCSITE({},{oh},{},$,$,{site_pl},$,$,.ELEMENT.,$,$,$,$,$)", s(&rand.guid()), s(site_name)));
    let bldg_pl = add(format!("IFCLOCALPLACEMENT({site_pl},{world})"));
    let building = add(format!("IFCBUILDING({},{oh},{},$,$,{bldg_pl},$,$,.ELEMENT.,$,$,$)", s(&rand.guid()), s(&b.name)));
    let mut storeys = Vec::new();
    for l in &b.levels {
        let pt = add(format!("IFCCARTESIANPOINT((0.,0.,{}))", real(l.elevation)));
        let ax = add(format!("IFCAXIS2PLACEMENT3D({pt},$,$)"));
        let pl = add(format!("IFCLOCALPLACEMENT({bldg_pl},{ax})"));
        let st = add(format!("IFCBUILDINGSTOREY({},{oh},{},$,$,{pl},$,$,.ELEMENT.,{})", s(&rand.guid()), s(&l.name), real(l.elevation)));
        let h = add(format!("IFCPROPERTYSINGLEVALUE('Height',$,IFCPOSITIVELENGTHMEASURE({}),$)", real(l.height)));
        let ps = add(format!("IFCPROPERTYSET({},{oh},'CadCraft_Level',$,({h}))", s(&rand.guid())));
        add(format!("IFCRELDEFINESBYPROPERTIES({},{oh},$,$,({st}),{ps})", s(&rand.guid())));
        storeys.push(st);
    }
    add(format!("IFCRELAGGREGATES({},{oh},$,$,{project},({site}))", s(&rand.guid())));
    add(format!("IFCRELAGGREGATES({},{oh},$,$,{site},({building}))", s(&rand.guid())));
    add(format!("IFCRELAGGREGATES({},{oh},$,$,{building},({}))", s(&rand.guid()), storeys.join(",")));
    let iso = crate::draw::editor::iso_seconds(b.now_ms);
    let mut out = vec![
        "ISO-10303-21;".to_string(),
        "HEADER;".into(),
        "FILE_DESCRIPTION(('ViewDefinition [DesignTransferView]'),'2;1');".into(),
        format!("FILE_NAME({},'{iso}',({}),(''),'BimCraft','BimCraft','');", s(&b.file_name), s(&b.author)),
        "FILE_SCHEMA(('IFC4'));".into(),
        "ENDSEC;".into(),
        "DATA;".into(),
    ];
    out.extend(lines);
    out.extend(["ENDSEC;".to_string(), "END-ISO-10303-21;".into(), String::new()]);
    Ok(out.join("\n"))
}
