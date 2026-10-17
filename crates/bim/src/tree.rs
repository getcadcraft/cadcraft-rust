//! ifc-tree.js — the model tree's data (0.4 B13): project → site → building → storey → space → elements, elements
//! grouped by class under each spatial node, parts under their element, elements in nothing in a group at the end.

use std::collections::HashMap;

use cc_io_ifc::model::{Id, IfcFile, id_key};
use serde_json::{Map, Value as J};

use crate::api::class_name;
use crate::js::{OMap, jn, locale_cmp_plain, num_str};
use crate::model::{Model, TNode};
use crate::vis::{HIDDEN, Visibility};

const SPATIAL: [&str; 17] = [
    "IFCPROJECT",
    "IFCSITE",
    "IFCBUILDING",
    "IFCBUILDINGSTOREY",
    "IFCSPACE",
    "IFCFACILITY",
    "IFCFACILITYPART",
    "IFCBRIDGE",
    "IFCBRIDGEPART",
    "IFCROAD",
    "IFCROADPART",
    "IFCRAILWAY",
    "IFCRAILWAYPART",
    "IFCMARINEFACILITY",
    "IFCMARINEPART",
    "IFCSPATIALZONE",
    "IFCEXTERNALSPATIALELEMENT",
];

/// `isSpatial(cls)`
pub fn is_spatial(cls: &str) -> bool {
    let u = cls.to_uppercase();
    !u.is_empty() && SPATIAL.contains(&u.as_str())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Spatial,
    Group,
    Element,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Spatial => "spatial",
            Kind::Group => "group",
            Kind::Element => "element",
        }
    }
}

/// A node's key: a number (keyBase + expressID) or a group's text key.
#[derive(Clone, Debug, PartialEq)]
pub enum Key {
    Num(f64),
    Str(String),
}

impl Key {
    pub fn json(&self) -> J {
        match self {
            Key::Num(n) => jn(*n),
            Key::Str(s) => J::String(s.clone()),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Node {
    pub kind: Kind,
    pub id: Option<Id>,
    pub key: Key,
    pub cls: String,
    pub kids: Vec<usize>,
    pub parent: Option<usize>,
    pub geo: bool,
    pub size: usize,
    pub loose: bool,
}

/// `buildTree(model, { className })`.
#[derive(Clone, Debug, Default)]
pub struct Tree {
    pub nodes: Vec<Node>,
    pub root: Option<usize>,
    pub loose: Option<usize>,
    pub by_key: HashMap<u64, usize>,
    pub key_base: f64,
}

pub fn build_tree(f: &IfcFile<'_>, model: &Model) -> Tree {
    let mut t = Tree { key_base: model.key_base, ..Tree::default() };
    let kb = model.key_base;
    let mut class_cache: HashMap<u64, String> = HashMap::new();
    let mut cls_of = |n: &TNode| -> String {
        if n.ty.starts_with("IFC") { class_cache.entry(id_key(n.id)).or_insert_with(|| class_name(f, n.id)).clone() } else { n.ty.clone() }
    };
    // depth-first, building each node after its children (the tree is at most 65 deep: recursion is bounded)
    fn node(t: &mut Tree, n: &TNode, parent: Option<usize>, model: &Model, kb: f64, cls_of: &mut dyn FnMut(&TNode) -> String) -> usize {
        let c = cls_of(n);
        let me = t.nodes.len();
        t.nodes.push(Node {
            kind: if is_spatial(&c) { Kind::Spatial } else { Kind::Element },
            id: Some(n.id),
            key: Key::Num(kb + n.id),
            cls: c.clone(),
            kids: Vec::new(),
            parent,
            geo: model.class(n.id).is_some(),
            size: 0,
            loose: false,
        });
        t.by_key.insert(id_key(kb + n.id), me);
        let mut spatial_kids = Vec::new();
        let mut elements = Vec::new();
        for ch in &n.children {
            let k = node(t, ch, Some(me), model, kb, cls_of);
            if t.nodes.get(k).is_some_and(|x| x.kind == Kind::Spatial) {
                spatial_kids.push(k);
            } else {
                elements.push(k);
            }
        }
        let spatial = t.nodes.get(me).is_some_and(|x| x.kind == Kind::Spatial);
        let kids = if spatial {
            let mut groups: OMap<String, Vec<usize>> = OMap::new();
            for &e in &elements {
                let ec = t.nodes.get(e).map(|x| x.cls.clone()).unwrap_or_default();
                if let Some(g) = groups.entry(ec, Vec::new) {
                    g.push(e);
                }
            }
            let mut gs: Vec<(String, Vec<usize>)> = groups.keys.into_iter().zip(groups.vals).collect();
            gs.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| locale_cmp_plain(&a.0, &b.0)));
            let mut kids = spatial_kids;
            for (gc, list) in gs {
                let gi = t.nodes.len();
                let size = list.iter().map(|&e| t.nodes.get(e).map_or(0, |x| x.size)).sum();
                for &e in &list {
                    if let Some(x) = t.nodes.get_mut(e) {
                        x.parent = Some(gi);
                    }
                }
                t.nodes.push(Node {
                    kind: Kind::Group,
                    id: None,
                    key: Key::Str(format!("g:{}:{gc}", num_str(kb + n.id))),
                    cls: gc,
                    kids: list,
                    parent: Some(me),
                    geo: false,
                    size,
                    loose: false,
                });
                kids.push(gi);
            }
            kids
        } else {
            elements
        };
        let own = usize::from(t.nodes.get(me).is_some_and(|x| x.geo) && c != "IfcSpace");
        let size = kids.iter().map(|&k| t.nodes.get(k).map_or(0, |x| x.size)).fold(own, usize::saturating_add);
        if let Some(x) = t.nodes.get_mut(me) {
            x.kids = kids;
            x.size = size;
        }
        me
    }
    if let Some(tree) = &model.tree {
        let r = node(&mut t, tree, None, model, kb, &mut cls_of);
        t.root = Some(r);
    }
    let lost: Vec<(f64, String)> = model
        .class_ids
        .iter()
        .filter(|id| !t.by_key.contains_key(&id_key(kb + **id)))
        .map(|id| (*id, model.class(*id).unwrap_or("").to_string()))
        .collect();
    if !lost.is_empty() {
        let li = t.nodes.len();
        t.nodes.push(Node {
            kind: Kind::Group,
            id: None,
            key: Key::Str("g:loose".into()),
            cls: String::new(),
            kids: Vec::new(),
            parent: None,
            geo: false,
            size: lost.len(),
            loose: true,
        });
        for (id, cls) in lost {
            let e = t.nodes.len();
            t.nodes.push(Node {
                kind: Kind::Element,
                id: Some(id),
                key: Key::Num(kb + id),
                cls,
                kids: Vec::new(),
                parent: Some(li),
                geo: true,
                size: 1,
                loose: false,
            });
            t.by_key.insert(id_key(kb + id), e);
            if let Some(l) = t.nodes.get_mut(li) {
                l.kids.push(e);
            }
        }
        t.loose = Some(li);
    }
    t
}

impl Tree {
    pub fn node(&self, i: usize) -> Option<&Node> {
        self.nodes.get(i)
    }
    /// `leaves(node)`: keys of every element with geometry at or under it (pre-order).
    pub fn leaves(&self, i: usize) -> Vec<f64> {
        let mut out = Vec::new();
        let mut stack = vec![i];
        while let Some(n) = stack.pop() {
            let Some(x) = self.nodes.get(n) else { continue };
            if x.geo
                && let Key::Num(k) = x.key
            {
                out.push(k);
            }
            stack.extend(x.kids.iter().rev());
        }
        out
    }
    /// `checkState(node, vis)`
    pub fn check_state(&self, i: usize, vis: &Visibility) -> &'static str {
        let (mut on, mut off) = (0usize, 0usize);
        let mut stack = vec![i];
        while let Some(n) = stack.pop() {
            let Some(x) = self.nodes.get(n) else { continue };
            if x.geo
                && let Key::Num(k) = x.key
            {
                if vis.hidden.has(k) {
                    off = off.saturating_add(1);
                } else {
                    on = on.saturating_add(1);
                }
            }
            stack.extend(x.kids.iter().rev());
        }
        if off == 0 {
            "on"
        } else if on == 0 {
            "off"
        } else {
            "mixed"
        }
    }
    /// `pathTo(tree, key)`: root … node (node indexes), or empty.
    pub fn path_to(&self, key: f64) -> Vec<usize> {
        let Some(&n) = self.by_key.get(&id_key(key)) else { return Vec::new() };
        let mut path = Vec::new();
        let mut x = Some(n);
        while let Some(i) = x {
            if path.len() > self.nodes.len() {
                break;
            }
            path.push(i);
            x = self.nodes.get(i).and_then(|y| y.parent);
        }
        path.reverse();
        path
    }
    pub fn node_json(&self, i: usize) -> J {
        let Some(x) = self.nodes.get(i) else { return J::Null };
        let mut o = Map::new();
        o.insert("kind".into(), J::String(x.kind.name().into()));
        o.insert("id".into(), x.id.map_or(J::Null, jn));
        o.insert("key".into(), x.key.json());
        o.insert("cls".into(), J::String(x.cls.clone()));
        o.insert("geo".into(), J::Bool(x.geo));
        o.insert("size".into(), J::from(x.size));
        if x.loose {
            o.insert("loose".into(), J::Bool(true));
        }
        o.insert("kids".into(), J::Array(x.kids.iter().map(|&k| self.node_json(k)).collect()));
        J::Object(o)
    }
}

/// `shown(vis, key)`
pub fn shown(vis: &Visibility, key: f64) -> bool {
    vis.state(key) != HIDDEN
}
