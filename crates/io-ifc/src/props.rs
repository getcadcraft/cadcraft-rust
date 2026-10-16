//! What BimCraft's Properties panel lists for a product: its attributes, its property sets and quantity sets
//! (IfcRelDefinesByProperties), and its type's property sets (IfcRelDefinesByType → HasPropertySets). Text only; values
//! as the file writes them (a typed value's number or text, booleans as Yes / No, enumerations by name).

use crate::model::{Id, IfcFile, id_key};
use crate::value::Value;

/// A titled list of name → value rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropGroup {
    pub title: String,
    pub rows: Vec<(String, String)>,
}

/// Groups shown at most / rows per group at most (a hostile file cannot make the panel unbounded).
const MAX_GROUPS: usize = 200;
const MAX_ROWS: usize = 500;

/// A number as JavaScript writes it.
fn num_text(n: f64) -> String {
    let mut s = String::new();
    crate::util::push_json_num(&mut s, n);
    s
}

fn text(v: Option<&Value>) -> Option<String> {
    let v = v?;
    let s = match v {
        Value::Null | Value::Derived => return None,
        Value::Str(s) => s.lossy().into_owned(),
        Value::Num(n) => num_text(*n),
        Value::Enum(e) => match &**e {
            "T" => "Yes".into(),
            "F" => "No".into(),
            "U" => "Unknown".into(),
            other => other.to_string(),
        },
        Value::Ref(r) => format!("#{}", num_text(*r)),
        Value::Bin(b) => b.to_string(),
        Value::Typed(t) => return text(Some(&t.1)),
        Value::List(l) => l.iter().filter_map(|x| text(Some(x))).collect::<Vec<_>>().join(", "),
    };
    (!s.is_empty()).then_some(s)
}

fn refs(v: Option<&Value>) -> Vec<Id> {
    match v {
        Some(Value::List(l)) => l.iter().filter_map(Value::as_ref_id).collect(),
        Some(x) => x.as_ref_id().into_iter().collect(),
        None => Vec::new(),
    }
}

fn attr_text(f: &IfcFile<'_>, id: Id, name: &str) -> Option<String> {
    text(f.attr(id, name).ok().flatten())
}

/// One property definition (a property set or a quantity set) → a group.
fn definition(f: &IfcFile<'_>, def: Id, suffix: &str) -> Option<PropGroup> {
    let ty = f.ty(def)?.to_string();
    let name = attr_text(f, def, "Name").unwrap_or_else(|| ty.clone());
    let mut rows = Vec::new();
    match ty.as_str() {
        "IFCPROPERTYSET" => {
            for p in refs(f.attr(def, "HasProperties").ok().flatten()) {
                if rows.len() >= MAX_ROWS {
                    break;
                }
                let pn = attr_text(f, p, "Name").unwrap_or_default();
                let val = match f.ty(p) {
                    Some("IFCPROPERTYSINGLEVALUE") => attr_text(f, p, "NominalValue"),
                    Some("IFCPROPERTYENUMERATEDVALUE") => attr_text(f, p, "EnumerationValues"),
                    Some("IFCPROPERTYLISTVALUE") => attr_text(f, p, "ListValues"),
                    Some("IFCPROPERTYBOUNDEDVALUE") => {
                        let lo = attr_text(f, p, "LowerBoundValue").unwrap_or_default();
                        let hi = attr_text(f, p, "UpperBoundValue").unwrap_or_default();
                        Some(format!("{lo} – {hi}"))
                    }
                    _ => None,
                };
                rows.push((pn, val.unwrap_or_default()));
            }
        }
        "IFCELEMENTQUANTITY" => {
            for q in refs(f.attr(def, "Quantities").ok().flatten()) {
                if rows.len() >= MAX_ROWS {
                    break;
                }
                let qn = attr_text(f, q, "Name").unwrap_or_default();
                let field = match f.ty(q) {
                    Some("IFCQUANTITYLENGTH") => "LengthValue",
                    Some("IFCQUANTITYAREA") => "AreaValue",
                    Some("IFCQUANTITYVOLUME") => "VolumeValue",
                    Some("IFCQUANTITYCOUNT") => "CountValue",
                    Some("IFCQUANTITYWEIGHT") => "WeightValue",
                    Some("IFCQUANTITYTIME") => "TimeValue",
                    _ => "",
                };
                rows.push((qn, attr_text(f, q, field).unwrap_or_default()));
            }
        }
        _ => return None,
    }
    Some(PropGroup { title: format!("{name}{suffix}"), rows })
}

/// Everything the panel shows for one product.
pub fn properties(f: &IfcFile<'_>, id: Id) -> Vec<PropGroup> {
    let mut out = Vec::new();
    let mut el = PropGroup { title: "Element".into(), rows: Vec::new() };
    if let Some(t) = f.ty(id) {
        el.rows.push(("Class".into(), crate::schema::api_type_name(t)));
    }
    for (label, attr) in [
        ("Name", "Name"),
        ("Description", "Description"),
        ("Object type", "ObjectType"),
        ("Tag", "Tag"),
        ("Predefined type", "PredefinedType"),
        ("GlobalId", "GlobalId"),
    ] {
        if let Some(v) = attr_text(f, id, attr) {
            el.rows.push((label.into(), v));
        }
    }
    el.rows.push(("Line".into(), format!("#{}", num_text(id))));
    out.push(el);
    if let Ok(by_props) = f.inverse("IFCRELDEFINESBYPROPERTIES", "RelatedObjects") {
        for rel in by_props.get(&id_key(id)).into_iter().flatten() {
            if out.len() >= MAX_GROUPS {
                break;
            }
            for def in refs(f.attr(*rel, "RelatingPropertyDefinition").ok().flatten()) {
                out.extend(definition(f, def, ""));
            }
        }
    }
    if let Ok(by_type) = f.inverse("IFCRELDEFINESBYTYPE", "RelatedObjects") {
        for rel in by_type.get(&id_key(id)).into_iter().flatten() {
            for ty in refs(f.attr(*rel, "RelatingType").ok().flatten()) {
                let tn = attr_text(f, ty, "Name").unwrap_or_default();
                let mut g = PropGroup { title: "Type".into(), rows: vec![("Name".into(), tn)] };
                if let Some(c) = f.ty(ty) {
                    g.rows.push(("Class".into(), crate::schema::api_type_name(c)));
                }
                out.push(g);
                for def in refs(f.attr(ty, "HasPropertySets").ok().flatten()) {
                    if out.len() >= MAX_GROUPS {
                        break;
                    }
                    out.extend(definition(f, def, " (type)"));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wall_with_a_property_set_and_a_type() {
        let ifc = b"ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\nFILE_NAME('','',(''),(''),'','','');\nFILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n\
#1=IFCWALL('2O2Fr$t4X7Zf8NOew3FLOH',$,'Wall 1',$,$,$,$,$,.STANDARD.);\n\
#2=IFCPROPERTYSINGLEVALUE('IsExternal',$,IFCBOOLEAN(.T.),$);\n\
#3=IFCPROPERTYSINGLEVALUE('FireRating',$,IFCLABEL('2HR'),$);\n\
#4=IFCPROPERTYSET('0x',$,'Pset_WallCommon',$,(#2,#3));\n\
#5=IFCRELDEFINESBYPROPERTIES('1x',$,$,$,(#1),#4);\n\
#6=IFCQUANTITYLENGTH('Height',$,$,3.,$);\n\
#7=IFCELEMENTQUANTITY('2x',$,'Qto_WallBaseQuantities',$,$,(#6));\n\
#8=IFCRELDEFINESBYPROPERTIES('3x',$,$,$,(#1),#7);\n\
#9=IFCWALLTYPE('4x',$,'Basic 200',$,$,$,$,$,$,.STANDARD.);\n\
#10=IFCRELDEFINESBYTYPE('5x',$,$,$,(#1),#9);\n\
ENDSEC;\nEND-ISO-10303-21;\n";
        let f = IfcFile::open(ifc).unwrap();
        let g = properties(&f, 1.0);
        let titles: Vec<&str> = g.iter().map(|x| x.title.as_str()).collect();
        assert_eq!(titles, ["Element", "Pset_WallCommon", "Qto_WallBaseQuantities", "Type"], "{g:?}");
        assert!(g[0].rows.contains(&("Name".into(), "Wall 1".into())));
        assert!(g[0].rows.contains(&("Predefined type".into(), "STANDARD".into())));
        assert_eq!(g[1].rows, vec![("IsExternal".into(), "Yes".into()), ("FireRating".into(), "2HR".into())]);
        assert_eq!(g[2].rows, vec![("Height".into(), "3".into())]);
        assert!(g[3].rows.contains(&("Name".into(), "Basic 200".into())));
    }
}
