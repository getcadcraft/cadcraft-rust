//! schema.js — the IFC schemas cc-ifc knows (tables generated from buildingSMART's EXPRESS files; `data/schema-*.txt`)
//! as lookups: entity → every explicit attribute in file order (supertypes first), subtype tests, and the numeric type
//! codes (CRC-32 of the upper-case name, the numbers the apps use as constants).
//!
//! `schema_for("IFC4")` → IFC2X3 | IFC4 | IFC4X3 (any IFC4X3_* name); an unknown name → IFC4 (guessed).

use std::collections::HashMap;
use std::sync::OnceLock;

const IFC2X3: &str = include_str!("../data/schema-ifc2x3.txt");
const IFC4: &str = include_str!("../data/schema-ifc4.txt");
const IFC4X3: &str = include_str!("../data/schema-ifc4x3.txt");

/// An attribute's type code parsed: aggregate levels, base (E S e s r i n b l x), defined type name, optional.
#[derive(Clone, Debug, PartialEq)]
pub struct AttrCode {
    pub agg: u32,
    pub base: Box<str>,
    pub tname: Option<Box<str>>,
    pub opt: bool,
}

/// `parseAttrCode('[[r:IFCLENGTHMEASURE?')`
pub fn parse_attr_code(t: &str) -> AttrCode {
    let agg = t.bytes().take_while(|&c| c == b'[').count();
    let opt = t.ends_with('?');
    let body = t.get(agg..t.len().saturating_sub(usize::from(opt)).max(agg)).unwrap_or("");
    let (base, tname) = match body.find(':') {
        Some(k) => (body.get(..k).unwrap_or(""), body.get(k.saturating_add(1)..).map(Box::from)),
        None => (body, None),
    };
    AttrCode { agg: u32::try_from(agg).unwrap_or(u32::MAX), base: base.into(), tname, opt }
}

#[derive(Clone, Debug)]
pub struct Attr {
    pub name: Box<str>,
    pub t: AttrCode,
}

/// One entity: pretty name, supertype, every explicit attribute (supertypes first).
#[derive(Debug)]
pub struct Entity {
    pub name: Box<str>,
    pub upper: Box<str>,
    pub parent: Option<Box<str>>,
    pub is_abstract: bool,
    pub attrs: Vec<Attr>,
    pub index: crate::step::FnvMap<Box<str>, usize>,
    pub derived: Vec<Box<str>>,
}

impl Entity {
    /// Position of an attribute by name.
    pub fn attr_index(&self, name: &str) -> Option<usize> {
        // most entities have a handful of attributes: a scan from the end (the last of a repeated name wins, as in the
        // JS Map) beats hashing the name
        if self.attrs.len() <= 16 {
            return self.attrs.iter().rposition(|a| &*a.name == name);
        }
        self.index.get(name).copied()
    }
}

/// A defined type / select / enumeration row.
#[derive(Debug)]
pub struct TypeRow {
    pub name: Box<str>,
    pub code: Box<str>,
    pub members: Box<str>,
}

#[derive(Debug)]
pub struct Schema {
    pub name: &'static str,
    ents: Vec<Entity>,
    by_name: HashMap<Box<str>, usize>,
    children: Vec<Vec<usize>>,
    parent_ix: Vec<Option<usize>>,
    typs: HashMap<Box<str>, TypeRow>,
    typ_order: Vec<Box<str>>,
    sub_cache: HashMap<Box<str>, Vec<Box<str>>>,
}

struct Row<'a> {
    upper: &'a str,
    pretty: &'a str,
    parent: &'a str,
    abs: bool,
    own: &'a str,
    derived: &'a str,
}

impl Schema {
    fn load(name: &'static str, text: &str) -> Schema {
        let mut rows: Vec<Row<'_>> = Vec::new();
        let mut typs = HashMap::new();
        let mut typ_order: Vec<Box<str>> = Vec::new();
        for line in text.lines() {
            let mut f = line.split('\t');
            match f.next() {
                Some("E") => {
                    let mut g = || f.next().unwrap_or("");
                    let (upper, pretty, parent, abs, own, derived) = (g(), g(), g(), g() == "1", g(), g());
                    rows.push(Row { upper, pretty, parent, abs, own, derived });
                }
                Some("T") => {
                    let mut g = || f.next().unwrap_or("");
                    let (upper, pretty, code, members) = (g(), g(), g(), g());
                    typ_order.push(Box::from(upper));
                    typs.insert(Box::from(upper), TypeRow { name: pretty.into(), code: code.into(), members: members.into() });
                }
                _ => {}
            }
        }
        let by_name: HashMap<Box<str>, usize> = rows.iter().enumerate().map(|(k, r)| (Box::from(r.upper), k)).collect();
        let parent_ix: Vec<Option<usize>> =
            rows.iter().map(|r| if r.parent.is_empty() { None } else { by_name.get(r.parent).copied() }).collect();
        let mut children: Vec<Vec<usize>> = vec![Vec::new(); rows.len()];
        for (k, p) in parent_ix.iter().enumerate() {
            if let Some(p) = p
                && let Some(c) = children.get_mut(*p)
            {
                c.push(k);
            }
        }
        // attributes: the supertypes' first (the chain is short; bounded anyway)
        let mut ents = Vec::with_capacity(rows.len());
        for (k, r) in rows.iter().enumerate() {
            let mut chain = vec![k];
            let mut cur = parent_ix.get(k).copied().flatten();
            while let Some(p) = cur {
                if chain.len() > 64 || chain.contains(&p) {
                    break;
                }
                chain.push(p);
                cur = parent_ix.get(p).copied().flatten();
            }
            let mut attrs = Vec::new();
            let mut derived = Vec::new();
            for &c in chain.iter().rev() {
                let Some(row) = rows.get(c) else { continue };
                if !row.own.is_empty() {
                    for s in row.own.split(';') {
                        let mut it = s.splitn(2, '|');
                        let n = it.next().unwrap_or("");
                        let t = it.next().unwrap_or("");
                        attrs.push(Attr { name: n.into(), t: parse_attr_code(t) });
                    }
                }
                if !row.derived.is_empty() {
                    derived.extend(row.derived.split(';').map(Box::from));
                }
            }
            // a later attribute of the same name wins in the JS Map built from the list
            let index = attrs.iter().enumerate().map(|(k, a)| (a.name.clone(), k)).collect();
            ents.push(Entity {
                name: r.pretty.into(),
                upper: r.upper.into(),
                parent: if r.parent.is_empty() { None } else { Some(r.parent.into()) },
                is_abstract: r.abs,
                attrs,
                index,
                derived,
            });
        }
        let mut s = Schema { name, ents, by_name, children, parent_ix, typs, typ_order, sub_cache: HashMap::new() };
        // every subtype list the geometry asks for, ready (the cache is read-only afterwards)
        for root in [
            "IFCPRODUCT",
            "IFCOPENINGELEMENT",
            "IFCSPACE",
            "IFCPROJECT",
            "IFCCONTEXT",
            "IFCSTYLEDITEM",
            "IFCMAPCONVERSION",
            "IFCRIGIDOPERATION",
        ] {
            let list = s.compute_subtypes(root);
            s.sub_cache.insert(root.into(), list);
        }
        s
    }

    pub fn has(&self, up: &str) -> bool {
        self.by_name.contains_key(up)
    }

    pub fn entity(&self, up: &str) -> Option<&Entity> {
        self.by_name.get(up).and_then(|&k| self.ents.get(k))
    }

    /// Is `up` the entity `ancestor` or one of its subtypes?
    pub fn is_a(&self, up: &str, ancestor: &str) -> bool {
        let mut cur = self.by_name.get(up).copied();
        let mut guard = 0u32;
        while let Some(k) = cur {
            if self.ents.get(k).is_some_and(|e| &*e.upper == ancestor) {
                return true;
            }
            guard = guard.saturating_add(1);
            if guard > 64 {
                break;
            }
            cur = self.parent_ix.get(k).copied().flatten();
        }
        false
    }

    fn compute_subtypes(&self, up: &str) -> Vec<Box<str>> {
        let mut out = Vec::new();
        let Some(&root) = self.by_name.get(up) else { return out };
        // pre-order, children in table order (an explicit stack: the tree is shallow, but bounded anyway)
        let mut stack = vec![root];
        while let Some(k) = stack.pop() {
            if out.len() > self.ents.len() {
                break;
            }
            if let Some(e) = self.ents.get(k) {
                out.push(e.upper.clone());
            }
            if let Some(ch) = self.children.get(k) {
                stack.extend(ch.iter().rev());
            }
        }
        out
    }

    /// `up` and every entity under it (pre-order).
    pub fn subtypes(&self, up: &str) -> std::borrow::Cow<'_, [Box<str>]> {
        match self.sub_cache.get(up) {
            Some(v) => std::borrow::Cow::Borrowed(v),
            None => std::borrow::Cow::Owned(self.compute_subtypes(up)),
        }
    }

    /// A defined type's underlying code ('r', 's', '[r', 'e', 'S', …).
    pub fn type_code(&self, up: &str) -> Option<&str> {
        self.typs.get(up).map(|t| &*t.code)
    }

    /// "IfcWall" for "IFCWALL" (entities and types); None when unknown.
    pub fn pretty(&self, up: &str) -> Option<&str> {
        if let Some(e) = self.entity(up) {
            return Some(&e.name);
        }
        self.typs.get(up).map(|t| &*t.name)
    }

    /// Entity and type names (upper case), entities first, in table order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.ents.iter().map(|e| &*e.upper).chain(self.typ_order.iter().map(|k| &**k))
    }
}

fn load(slot: &'static OnceLock<Schema>, name: &'static str, text: &'static str) -> &'static Schema {
    slot.get_or_init(|| Schema::load(name, text))
}

static S2X3: OnceLock<Schema> = OnceLock::new();
static S4: OnceLock<Schema> = OnceLock::new();
static S4X3: OnceLock<Schema> = OnceLock::new();

/// The schema of a file's FILE_SCHEMA name (and whether it was guessed).
pub fn schema_for(file_schema: &str) -> (&'static Schema, bool) {
    let raw = crate::util::js_trim(&file_schema.to_uppercase()).to_string();
    if raw.starts_with("IFC2X3") {
        (load(&S2X3, "IFC2X3", IFC2X3), false)
    } else if raw.starts_with("IFC4X3") {
        (load(&S4X3, "IFC4X3", IFC4X3), false)
    } else if raw.starts_with("IFC4") {
        (load(&S4, "IFC4", IFC4), false)
    } else {
        (load(&S4, "IFC4", IFC4), true)
    }
}

/// `GetNameFromTypeCode(GetLineType(id)).toUpperCase()` for a line of type `up`: the code's first registered name
/// (every schema's entities and types, then the header entities, as constants.js registers them), its pretty name in
/// IFC4X3 / IFC4 / IFC2X3, upper case.
pub fn api_type_name(up: &str) -> String {
    static FIRST: OnceLock<HashMap<u32, String>> = OnceLock::new();
    let first = FIRST.get_or_init(|| {
        let mut m: HashMap<u32, String> = HashMap::new();
        for s in [schema_for("IFC2X3").0, schema_for("IFC4").0, schema_for("IFC4X3").0] {
            for n in s.names() {
                m.entry(crc32(n)).or_insert_with(|| n.to_string());
            }
        }
        for h in ["FILE_DESCRIPTION", "FILE_NAME", "FILE_SCHEMA"] {
            m.entry(crc32(h)).or_insert_with(|| h.to_string());
        }
        m
    });
    let name = first.get(&crc32(up)).cloned().unwrap_or_else(|| up.to_string());
    for s in ["IFC4X3", "IFC4", "IFC2X3"] {
        if let Some(p) = schema_for(s).0.pretty(&name) {
            return p.to_uppercase();
        }
    }
    name.to_uppercase()
}

/// The CRC-32 of a name's UTF-16 code units, low byte each (`typeCode`: the numbers web-ifc's constants use).
pub fn crc32(s: &str) -> u32 {
    static T: OnceLock<[u32; 256]> = OnceLock::new();
    let t = T.get_or_init(|| {
        let mut t = [0u32; 256];
        for (n, slot) in t.iter_mut().enumerate() {
            let mut c = u32::try_from(n).unwrap_or(0);
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *slot = c;
        }
        t
    });
    let mut c = 0xFFFF_FFFFu32;
    for u in s.encode_utf16() {
        let k = usize::from(u8::try_from((c ^ u32::from(u)) & 0xFF).unwrap_or(0));
        c = t.get(k).copied().unwrap_or(0) ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

/// `typeCode(name)`: CRC-32 of the upper-case name.
pub fn type_code(name: &str) -> u32 {
    crc32(&name.to_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_load() {
        let (s, guessed) = schema_for(" ifc4 ");
        assert!(!guessed);
        assert_eq!(s.name, "IFC4");
        let w = s.entity("IFCWALL").unwrap();
        assert_eq!(&*w.name, "IfcWall");
        assert_eq!(w.attr_index("GlobalId"), Some(0));
        assert_eq!(w.attr_index("Representation"), Some(6));
        assert!(s.is_a("IFCWALLSTANDARDCASE", "IFCPRODUCT"));
        assert!(!s.is_a("IFCWALL", "IFCSLAB"));
        let sub = s.subtypes("IFCWALL");
        assert_eq!(sub.iter().map(|x| &**x).collect::<Vec<_>>(), ["IFCWALL", "IFCWALLELEMENTEDCASE", "IFCWALLSTANDARDCASE"]);
        assert_eq!(schema_for("IFC2X3_TC1").0.name, "IFC2X3");
        assert_eq!(schema_for("IFC4X3_ADD2").0.name, "IFC4X3");
        assert!(schema_for("FOO").1);
        assert_eq!(s.type_code("IFCLENGTHMEASURE"), Some("r"));
    }

    #[test]
    fn codes_match_the_constants() {
        // constants.js: IFCWALL = 2391406946, IFCZONE = 1033361043
        assert_eq!(type_code("IfcWall"), 2391406946);
        assert_eq!(type_code("IFCZONE"), 1033361043);
        assert_eq!(
            parse_attr_code("[[r:IFCLENGTHMEASURE?"),
            AttrCode { agg: 2, base: "r".into(), tname: Some("IFCLENGTHMEASURE".into()), opt: true }
        );
    }
}
