//! The writer copies the source drawing's non-graphical objects (port of lib/dwg/nongraphic-write.js, Apache-2.0;
//! ODA spec 20.1, 20.4.44/45, 20.4.72, 20.4.74, 20.4.77, 20.4.79, 20.4.83, 20.4.84, 20.4.92–94, 20.4.104, ch. 9, 10).
//!
//! What is copied: every entry of the source's named-object dictionary except those the writer makes itself, with
//! everything they own; the extension dictionary (and what it owns) of every written entity, table entry, block record
//! and layout; any object owned by a copied object. In the file's own version an object the reader kept raw is copied
//! bit for bit with its handles mapped — except dictionaries, groups, sort tables, xrecords, ID buffers, layer indexes
//! and field lists, whose lists can lose entries and are always encoded from their fields. In another version an object
//! is encoded from its decoded fields; one that cc-dwg does not decode goes as a proxy (passthrough.js) or is left out.
//!
//! Spec gap: SORTENTSTABLE's sort handles are handles of the source file. Renumbered, the draw order is kept by giving
//! every entity of the block a sort handle: the block's new entity handles, ascending, dealt out in the source's draw
//! order.

use std::collections::{HashMap, HashSet};

use crate::obj;
use crate::util::{unb64, usize_to_f64};
use crate::value::{Obj, V};
use crate::w_bits::{BitWriter, version_num};
use crate::w_js::{arr, field, hex_key, hex_upper, int32, js_entries, js_string, nn, num, parse_int16, str_or, to_number, truthy};
use crate::w_objects::{ClassProps, Cx, Enc, HARD_OWN, HARD_PTR, Rec, SOFT_PTR, begin, finish, nnum, object_common, xrecord_bytes};
use crate::w_passthrough::{Plan, RawRec, convert_plan, never_proxy, raw_usable};
use crate::w_write::{Conv, W, js_sort};

/// The tables' lists in Object.values order: (table index, list)
fn table_lists(model: &Obj) -> Vec<(usize, &[V])> {
    let Some(t) = model.obj("tables") else { return Vec::new() };
    js_entries(t).into_iter().enumerate().filter_map(|(i, (_, v))| v.as_arr().map(|a| (i, a.as_slice()))).collect()
}

/// Identity of a model object (JS Map keys are the objects themselves).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThingKey {
    Obj(usize),
    Table(usize, usize),
    Other,
}

#[derive(Default)]
pub struct XState {
    pub header_handles: Vec<(&'static str, f64)>,
    pub eed_cache: HashMap<ThingKey, V>,
    pub raw_entities: Obj,
    pub dropped_entities: Obj,
    pub proxied: Obj,
    pub src_maint: f64,
    pub mleader_eed: Option<V>,
    /// model.classes by number (the last of a number wins)
    pub cls_by_num: HashMap<u64, ClassProps>,
    /// classNumOf: dxfName → the number of the first class (in Map order) with that name
    pub num_by_dxf: HashMap<String, f64>,
}
/// A number as a Map key (SameValueZero: -0 is 0).
pub fn num_key(n: f64) -> u64 {
    if n == 0.0 { 0 } else { n.to_bits() }
}

/// A copied XRECORD (encodeXrecord).
#[derive(Clone, Debug)]
pub struct XrCopy {
    pub src: Obj,
    /// ACDB_RECOMPOSE_DATA: per data item, recomposeKeep(v) of a 330 item (decided from the model)
    pub keep: Option<Vec<bool>>,
}
/// A copy encoded from its fields (FIELD_ENCODERS).
#[derive(Clone, Debug)]
pub struct FieldCopy {
    pub kind: String,
    pub src: Obj,
    pub list_h: Vec<f64>,
    pub list_li: Vec<(Obj, f64)>,
    pub list_pairs: Vec<(f64, f64)>,
    pub owner_block: f64,
}

const WRITER_NOD: [&str; 7] =
    ["ACAD_GROUP", "ACAD_LAYOUT", "ACAD_MLINESTYLE", "ACAD_PLOTSTYLENAME", "ACAD_IMAGE_DICT", "ACAD_IMAGE_VARS", "ACAD_WIPEOUT_VARS"];
fn writer_type(t: &str) -> bool {
    matches!(t, "LAYOUT" | "MLINESTYLE" | "IMAGEDEF" | "IMAGEDEF_REACTOR" | "VPENT_HEADER")
}
fn always_fields(t: &str) -> bool {
    matches!(
        t,
        "DICTIONARY"
            | "ACDBDICTIONARYWDFLT"
            | "DICTIONARYWDFLT"
            | "GROUP"
            | "SORTENTSTABLE"
            | "XRECORD"
            | "IDBUFFER"
            | "LAYER_INDEX"
            | "FIELDLIST"
    )
}
/// FIELD_ENCODERS has an encoder for this field type
pub fn has_field_encoder(t: &str) -> bool {
    matches!(
        t,
        "GROUP"
            | "DICTIONARYVAR"
            | "SCALE"
            | "IDBUFFER"
            | "FIELDLIST"
            | "LAYER_INDEX"
            | "SPATIAL_FILTER"
            | "SORTENTSTABLE"
            | "PLOTSETTINGS"
            | "MLEADERSTYLE"
            | "RASTERVARIABLES"
            | "WIPEOUTVARIABLES"
            | "ACDBPLACEHOLDER"
            | "FIELD"
    )
}
fn field_type(t: &str) -> &str {
    if t == "ACDBDICTIONARYWDFLT" { "DICTIONARYWDFLT" } else { t }
}
fn type_of(o: &Obj) -> String {
    js_string(o.get("type"))
}
fn inc(o: &mut Obj, k: &str, d: f64) {
    let n = num(o.get(k), 0.0) + d;
    o.set(k.to_string(), n);
}
fn list_str(o: &Obj) -> String {
    js_entries(o).iter().map(|(t, n)| format!("{} {t}", crate::w_js::num_to_string(num(Some(n), 0.0)))).collect::<Vec<_>>().join(", ")
}

impl W<'_> {
    /// The tables' lists in Object.values order: (table index, list)
    /// clsOf(n): the model class of that number (`new Map(classes.map((c) => [c.num, c]))`: the last one wins)
    fn cls_of(&self, n: Option<f64>) -> Option<ClassProps> {
        self.x.cls_by_num.get(&num_key(n?)).cloned()
    }
    /// classNumOf(o): the first class (in Map order) whose dxfName is the object's type
    fn class_num_of(&self, ty: &str) -> Option<f64> {
        self.x.num_by_dxf.get(ty).copied()
    }
    fn plan_for(&self, raw: Option<&Obj>, is_proxy: bool, type_num: Option<f64>) -> Option<Plan> {
        let mv = self.model.str("version").unwrap_or("").to_string();
        let f = |id: Option<f64>| -> Option<ClassProps> {
            let c = self.cls_of(if is_proxy { id } else { type_num })?;
            if never_proxy(&js_string(c.dxf.as_ref())) { None } else { Some(c) }
        };
        convert_plan(raw, is_proxy, &f, &mv, self.x.src_maint, self.version)
    }

    /// extendWriter(env): the object's extended data, resolved now (new applications must exist before the APPID table).
    pub fn x_init(&mut self) {
        let model = self.model;
        let classes = arr(model.get("classes"));
        let mut order: Vec<u64> = Vec::new();
        for c in classes {
            let Some(V::Num(n)) = field(Some(c), "num") else { continue };
            let k = num_key(*n);
            if !self.x.cls_by_num.contains_key(&k) {
                order.push(k);
            }
            self.x.cls_by_num.insert(k, crate::w_passthrough::class_props_of(Some(c)));
        }
        let mut nums: HashMap<u64, f64> = HashMap::new();
        for c in classes {
            if let Some(V::Num(n)) = field(Some(c), "num") {
                nums.entry(num_key(*n)).or_insert(*n);
            }
        }
        for k in order {
            let last = classes.iter().rev().find(|x| matches!(field(Some(x), "num"), Some(V::Num(m)) if num_key(*m) == k));
            if let Some(V::Str(d)) = field(last, "dxfName") {
                let n = nums.get(&k).copied().unwrap_or(0.0);
                self.x.num_by_dxf.entry(d.clone()).or_insert(n);
            }
        }
        let mm = model.get("maintenance");
        self.x.src_maint = if truthy(mm) {
            to_number(mm)
        } else if model.str("version") == Some("AC1032") {
            29.0
        } else {
            0.0
        };
        let old = self.ver < 24;
        let objs = arr(model.get("objects"));
        let has_ml = objs.iter().any(|o| matches!(field(Some(o), "type"), Some(V::Str(s)) if s == "MLEADERSTYLE"))
            || arr(model.get("blocks")).iter().any(|b| {
                arr(field(Some(b), "entities"))
                    .iter()
                    .any(|e| matches!(field(Some(e), "type"), Some(V::Str(s)) if s == "MULTILEADER" || s == "MLEADER"))
            });
        if has_ml && old {
            let x = V::Obj(obj! { "ACAD_MLEADERVER" => vec![V::Arr(vec![V::Num(1070.0), V::Num(2.0)])] });
            self.x.mleader_eed = self.eed_of(Some(&x));
        }
        for (i, o) in objs.iter().enumerate() {
            let mut x = field(Some(o), "xdata").cloned();
            if matches!(field(Some(o), "type"), Some(V::Str(s)) if s == "MLEADERSTYLE")
                && !raw_usable(field(Some(o), "raw"), self.version, None)
            {
                let mut xo = x.as_ref().and_then(V::as_obj).cloned().unwrap_or_default();
                if old {
                    xo.set("ACAD_MLEADERVER", vec![V::Arr(vec![V::Num(1070.0), V::Num(2.0)])]);
                } else {
                    xo.remove("ACAD_MLEADERVER");
                }
                x = Some(V::Obj(xo));
            }
            if let Some(e) = self.eed_of(x.as_ref()) {
                self.x.eed_cache.insert(ThingKey::Obj(i), e);
            }
        }
        // table entries and block records keep their extended data too
        let tables: Vec<(usize, &[V])> = table_lists(model);
        for (ti, list) in tables {
            for (j, t) in list.iter().enumerate() {
                if truthy(field(Some(t), "xdata"))
                    && let Some(e) = self.eed_of(field(Some(t), "xdata"))
                {
                    self.x.eed_cache.insert(ThingKey::Table(ti, j), e);
                }
            }
        }
    }

    /// X.convert: MULTILEADER and raw / proxy entities.
    pub fn x_convert(&mut self, e: &Obj, ty: &str, owner: f64) -> Option<Conv> {
        if ty == "MULTILEADER" || ty == "MLEADER" {
            return Some(self.convert_mleader(e, owner));
        }
        if (ty == "UNKNOWN" || ty == "ACAD_PROXY_ENTITY") && truthy(e.get("raw")) {
            let name = if ty == "UNKNOWN" {
                if truthy(e.get("dxfName")) { js_string(e.get("dxfName")) } else { "UNKNOWN".into() }
            } else {
                ty.to_string()
            };
            let type_num = if ty == "ACAD_PROXY_ENTITY" { Some(498.0) } else { e.get("typeNum").map(|v| to_number(Some(v))) };
            let tn = type_num.unwrap_or(f64::NAN);
            let cls = if tn >= 500.0 { self.cls_of(type_num) } else { None };
            let raw = e.obj("raw").cloned().unwrap_or_default();
            if !raw_usable(e.get("raw"), self.version, None) || (tn >= 500.0 && cls.is_none()) {
                let conv = self.plan_for(Some(&raw), ty == "ACAD_PROXY_ENTITY", type_num);
                if let Some(conv) = conv {
                    inc(&mut self.x.proxied, &name, 1.0);
                    let mut r = self.common(e, &name, owner);
                    let ccls = conv.cls.clone();
                    r.enc = Enc::Raw(Box::new(RawRec {
                        type_num,
                        cls: Some(ccls),
                        raw,
                        is_entity: true,
                        proxy_class: false,
                        conv: Some(conv),
                    }));
                    return Some(Conv::Recs(vec![r]));
                }
                inc(&mut self.x.dropped_entities, &name, 1.0);
                return Some(Conv::Skip(format!("{name} (kept only when the drawing is written in its own version)")));
            }
            inc(&mut self.x.raw_entities, &name, 1.0);
            let mut r = self.common(e, &name, owner);
            r.enc = Enc::Raw(Box::new(RawRec { type_num, cls, raw, is_entity: true, proxy_class: true, conv: None }));
            return Some(Conv::Recs(vec![r]));
        }
        None
    }

    /// X.finish: copies the objects, links extension dictionaries and reactors, extends the writer's dictionaries.
    pub fn x_finish(&mut self) {
        let model = self.model;
        let objs = arr(model.get("objects"));
        let hf = self.h.clone();
        for i in 0..self.layouts.len() {
            let Some(l) = self.layouts.get(i) else { continue };
            let (sh, lh) = (l.src.get("handle").cloned(), l.h);
            if truthy(sh.as_ref()) && lh != 0.0 && !self.src_has(sh.as_ref()) {
                self.map_src(sh.as_ref(), lh);
            }
        }
        // records write.js makes but does not map: the attributes of a reference, the *Active viewport
        let mut src_ent: HashMap<Option<String>, &Obj> = HashMap::new();
        for b in arr(model.get("blocks")) {
            for e in arr(field(Some(b), "entities")) {
                if let Some(eo) = e.as_obj() {
                    src_ent.insert(hex_key(eo.get("handle")), eo);
                }
            }
        }
        let mut back: HashMap<u64, String> = HashMap::new();
        for k in &self.src_order {
            if let Some(v) = self.src_map.get(k) {
                back.insert(v.to_bits(), k.clone());
            }
        }
        let mut att_maps: Vec<(Option<V>, f64)> = Vec::new();
        for r in &self.recs {
            if (r.t() != "INSERT" && r.t() != "MINSERT") || arr(r.g("attribs")).is_empty() {
                continue;
            }
            let k = back.get(&r.h().to_bits()).cloned();
            let e = src_ent.get(&k).copied();
            let atts: Vec<&V> = arr(e.and_then(|e| e.get("attribs")))
                .iter()
                .filter(|a| {
                    let a = Some(*a);
                    let ins = field(a, "insert");
                    truthy(ins)
                        && matches!(field(ins, "x"), Some(V::Num(x)) if x.is_finite())
                        && matches!(field(ins, "y"), Some(V::Num(y)) if y.is_finite())
                        && to_number(field(a, "height")) > 0.0
                        && truthy(field(a, "tag"))
                })
                .collect();
            let ra = arr(r.g("attribs"));
            if atts.len() == ra.len() {
                for (i, a) in atts.iter().enumerate() {
                    att_maps.push((field(Some(a), "handle").cloned(), num(ra.get(i), 0.0)));
                }
            }
        }
        for (h, n) in att_maps {
            if !self.src_has(h.as_ref()) {
                self.map_src(h.as_ref(), n);
            }
        }
        let act = arr(model.obj("tables").and_then(|t| t.get("vports")))
            .iter()
            .find(|v| {
                (if truthy(field(Some(v), "name")) { js_string(field(Some(v), "name")) } else { String::new() }).to_lowercase() == "*active"
            })
            .cloned();
        if let Some(a) = &act
            && hf.vport != 0.0
            && !self.src_has(field(Some(a), "handle"))
        {
            let h = field(Some(a), "handle").cloned();
            self.map_src(h.as_ref(), hf.vport);
        }
        let mut by_h: HashMap<Option<String>, usize> = HashMap::new();
        for (i, o) in objs.iter().enumerate() {
            by_h.insert(hex_key(field(Some(o), "handle")), i);
        }
        let obj_at = |i: usize| objs.get(i).and_then(V::as_obj);
        let mut kids: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, o) in objs.iter().enumerate() {
            if let Some(k) = hex_key(field(Some(o), "owner")).filter(|k| !k.is_empty()) {
                kids.entry(k).or_default().push(i);
            }
        }
        let version = self.version;
        let same_ver = |o: &Obj| raw_usable(o.get("raw"), version, Some(&type_of(o)));
        let mut conv_cache: HashMap<usize, Option<Plan>> = HashMap::new();
        let ml_handles: HashSet<Option<String>> =
            src_ent.values().filter(|e| e.str("type") == Some("MULTILEADER")).map(|e| hex_key(e.get("handle"))).collect();
        let wver = self.ver;
        let stale_round_trip = |o: &Obj| -> bool {
            if wver < 24 {
                return false;
            }
            let is_x = o.str("type") == Some("XRECORD");
            let d: Option<&Obj> = if is_x { by_h.get(&hex_key(o.get("owner"))).and_then(|&i| obj_at(i)) } else { Some(o) };
            let Some(d) = d else { return false };
            if d.str("type") != Some("DICTIONARY") || !ml_handles.contains(&hex_key(d.get("owner"))) {
                return false;
            }
            if !is_x {
                let ents = d.obj("entries");
                return ents.is_some_and(|en| !en.is_empty() && en.keys().all(|n| n == "ACAD_XREC_ROUNDTRIP"));
            }
            hex_key(field(d.get("entries"), "ACAD_XREC_ROUNDTRIP")) == hex_key(o.get("handle"))
        };
        let hdr = model.obj("header");
        let nod_src: Option<usize> = by_h.get(&hex_key(hdr.and_then(|h| h.get("DICTIONARY_NAMED_OBJECTS")))).copied().or_else(|| {
            objs.iter().position(|o| {
                matches!(field(Some(o), "type"), Some(V::Str(s)) if s == "DICTIONARY") && !truthy_key(field(Some(o), "owner"))
            })
        });
        let recompose_h = nod_src.and_then(obj_at).and_then(|n| hex_key(field(n.get("entries"), "ACDB_RECOMPOSE_DATA")));
        let recompose_keep = |v: Option<&V>| -> bool {
            let k = hex_key(v);
            let t = by_h
                .get(&k)
                .and_then(|&i| obj_at(i))
                .and_then(|o| o.get("type"))
                .filter(|t| t.truthy())
                .or_else(|| src_ent.get(&k).and_then(|e| e.get("type")))
                .map(|t| js_string(Some(t)));
            t.is_some_and(|t| !t.is_empty() && t != "TABLESTYLE" && t != "ACAD_TABLE")
        };
        // the plan
        let mut plan: Vec<(String, usize, POwner)> = Vec::new();
        let mut plan_idx: HashMap<String, usize> = HashMap::new();
        let mut dropped = Obj::new();
        let mut dropped_h: HashSet<Option<String>> = HashSet::new();
        let mut queue: std::collections::VecDeque<String> = std::collections::VecDeque::new();
        let skip_handles = self.tbl.skip_handles.clone();
        let empty_o = Obj::new();
        macro_rules! want {
            ($hex:expr, $owner:expr) => {{
                let k = hex_key($hex);
                if let Some(k) = k.filter(|k| !k.is_empty()) {
                    if !plan_idx.contains_key(&k) && !self.src_map.contains_key(&k) {
                        if let Some(&oi) = by_h.get(&Some(k.clone())) {
                            if !dropped_h.contains(&Some(k.clone())) {
                                let o: &Obj = obj_at(oi).unwrap_or(&empty_o);
                                let ot = type_of(o);
                                let recompose_drop = recompose_h.as_deref() == Some(k.as_str())
                                    && !arr(o.get("data")).iter().any(|it| {
                                        let p = it.as_arr().map(Vec::as_slice).unwrap_or(&[]);
                                        matches!(p.first(), Some(V::Num(c)) if *c == 330.0) && recompose_keep(p.get(1))
                                    });
                                if stale_round_trip(o) || recompose_drop {
                                    dropped_h.insert(Some(k.clone()));
                                    inc(&mut dropped, &ot, 1.0);
                                } else {
                                    let skip = skip_handles.contains(&js_string(o.get("handle")).to_uppercase());
                                    let can = {
                                        let tt = ot.as_str();
                                        if writer_type(tt) || tt.ends_with("_CONTROL") {
                                            false
                                        } else if always_fields(tt) {
                                            !truthy(o.get("damaged"))
                                        } else if same_ver(o) {
                                            true
                                        } else if has_field_encoder(field_type(tt)) && !truthy(o.get("undecoded")) && !truthy(o.get("damaged")) {
                                            true
                                        } else {
                                            let c = conv_cache.entry(oi).or_insert_with(|| self.conv_of(o));
                                            c.is_some()
                                        }
                                    };
                                    if skip || !can {
                                        dropped_h.insert(Some(k.clone()));
                                        inc(&mut dropped, &ot, 1.0);
                                    } else {
                                        plan_idx.insert(k.clone(), plan.len());
                                        plan.push((k.clone(), oi, $owner));
                                        queue.push_back(k);
                                    }
                                }
                            }
                        }
                    }
                }
            }};
        }
        // roots: the named-object dictionary
        let mut nod_add: Vec<(String, String)> = Vec::new();
        let mut group_add: Vec<(String, String)> = Vec::new();
        let mut plot_add: Vec<(String, String)> = Vec::new();
        let writer_nod: HashSet<String> = self
            .recs
            .iter()
            .find(|r| r.h() == hf.nod && r.t() == "DICTIONARY")
            .map(|r| arr(r.g("items")).iter().map(|x| js_string(x.as_arr().and_then(|a| a.first()))).collect())
            .unwrap_or_default();
        if let Some(ns) = nod_src.and_then(obj_at) {
            let entries = ns.obj("entries").cloned().unwrap_or_default();
            for (name, hex) in js_entries(&entries) {
                let hk = hex_key(Some(hex));
                if name.is_empty() || hk.as_deref().is_none_or(str::is_empty) {
                    continue;
                }
                if name == "ACAD_GROUP" {
                    let gd = by_h.get(&hk).and_then(|&i| obj_at(i)).and_then(|o| o.obj("entries")).cloned().unwrap_or_default();
                    for (gname, gh) in js_entries(&gd) {
                        let gk = hex_key(Some(gh));
                        if !gname.is_empty()
                            && let Some(gk) = gk.filter(|k| !k.is_empty())
                        {
                            want!(Some(gh), POwner::H(hf.group_dict));
                            group_add.push((gname.to_string(), gk));
                        }
                    }
                    continue;
                }
                if name == "ACAD_PLOTSTYLENAME" {
                    let pd = by_h.get(&hk).and_then(|&i| obj_at(i)).and_then(|o| o.obj("entries")).cloned().unwrap_or_default();
                    for (pn, ph) in js_entries(&pd) {
                        let pk = hex_key(Some(ph));
                        let Some(pk) = pk.filter(|k| !k.is_empty()) else { continue };
                        if pn.is_empty() {
                            continue;
                        }
                        if pn.to_lowercase() == "normal" {
                            if !self.src_map.contains_key(&pk) {
                                self.map_src(Some(ph), hf.plot_style_normal);
                            }
                            continue;
                        }
                        want!(Some(ph), POwner::H(hf.plot_style_dict));
                        plot_add.push((pn.to_string(), pk));
                    }
                    continue;
                }
                if WRITER_NOD.contains(&name) && !(name.ends_with("_VARS") && !writer_nod.contains(name)) {
                    continue;
                }
                want!(Some(hex), POwner::H(hf.nod));
                nod_add.push((name.to_string(), hk.unwrap_or_default()));
            }
        }
        // roots: extension dictionaries of everything written that the model describes (the model's objects are referenced,
        // not copied; the block records' and table controls' stand-ins live in `pseudo`)
        enum Th<'a> {
            M(ThingKey, &'a Obj),
            P(usize),
        }
        let empty = Obj::new();
        let mut pseudo: Vec<Obj> = Vec::new();
        let mut order: Vec<Th> = Vec::new();
        for (ti, list) in table_lists(model) {
            for (j, t) in list.iter().enumerate() {
                order.push(Th::M(ThingKey::Table(ti, j), t.as_obj().unwrap_or(&empty)));
            }
        }
        let brecs = arr(model.obj("tables").and_then(|t| t.get("blockRecords")));
        let mut brec_ext: HashMap<Option<String>, Option<&V>> = HashMap::new();
        for r in brecs {
            brec_ext.entry(hex_key(field(Some(r), "handle"))).or_insert(field(Some(r), "extDict"));
        }
        for b in arr(model.get("blocks")) {
            let rec = field(Some(b), "record");
            let ext = brec_ext.get(&hex_key(rec)).copied().flatten().cloned();
            let mut o = Obj::new();
            o.set_opt("handle", rec.cloned());
            o.set_opt("extDict", ext);
            pseudo.push(o);
            order.push(Th::P(pseudo.len().saturating_sub(1)));
            for e in arr(field(Some(b), "entities")) {
                order.push(Th::M(ThingKey::Other, e.as_obj().unwrap_or(&empty)));
                for a in arr(field(Some(e), "attribs")) {
                    order.push(Th::M(ThingKey::Other, a.as_obj().unwrap_or(&empty)));
                }
            }
        }
        for l in arr(model.get("layouts")) {
            order.push(Th::M(ThingKey::Other, l.as_obj().unwrap_or(&empty)));
        }
        // the table control objects are not in the model; their extension dictionaries are found by owner
        let ctl = [
            ("BLOCK_CONTROL_OBJECT", hf.block_ctl),
            ("LAYER_CONTROL_OBJECT", hf.layer_ctl),
            ("STYLE_CONTROL_OBJECT", hf.style_ctl),
            ("LINETYPE_CONTROL_OBJECT", hf.ltype_ctl),
            ("VIEW_CONTROL_OBJECT", hf.view_ctl),
            ("UCS_CONTROL_OBJECT", hf.ucs_ctl),
            ("VPORT_CONTROL_OBJECT", hf.vport_ctl),
            ("APPID_CONTROL_OBJECT", hf.appid_ctl),
            ("DIMSTYLE_CONTROL_OBJECT", hf.dimstyle_ctl),
            ("VPORT_ENTITY_HEADER_CONTROL_OBJECT", hf.vpent_ctl),
        ];
        let rec_hs: HashSet<u64> = self.recs.iter().map(|r| r.h().to_bits()).collect();
        for (hv, hkey) in ctl {
            let Some(k) = hex_key(hdr.and_then(|h| h.get(hv))).filter(|k| !k.is_empty()) else { continue };
            if hkey == 0.0 || !rec_hs.contains(&hkey.to_bits()) {
                continue;
            }
            if !self.src_map.contains_key(&k) {
                self.map_key(k.clone(), hkey);
            }
            let xd =
                kids.get(&k).and_then(|v| v.iter().find(|&&i| js_string(obj_at(i).and_then(|o| o.get("type"))).contains("DICTIONARY")));
            if let Some(&xi) = xd {
                let mut o = obj! { "handle" => k.clone() };
                o.set_opt("extDict", obj_at(xi).and_then(|x| x.get("handle")).cloned());
                pseudo.push(o);
                order.push(Th::P(pseudo.len().saturating_sub(1)));
            }
        }
        let src_things: Vec<(ThingKey, &Obj)> = order
            .into_iter()
            .map(|t| match t {
                Th::M(k, o) => (k, o),
                Th::P(i) => (ThingKey::Other, pseudo.get(i).unwrap_or(&empty)),
            })
            .collect();
        let mut thing_by_new: HashMap<u64, (ThingKey, &Obj)> = HashMap::new();
        for &(key, s) in &src_things {
            let nh = self.src(s.get("handle"));
            if nh == 0.0 {
                continue;
            }
            if !thing_by_new.contains_key(&nh.to_bits()) || truthy(s.get("extDict")) {
                thing_by_new.insert(nh.to_bits(), (key, s));
            }
            if truthy(s.get("extDict")) {
                want!(s.get("extDict"), POwner::H(nh));
            }
        }
        // roots: objects owned by something the writer wrote
        for o in objs {
            if let Some(ow) = hex_key(field(Some(o), "owner")).filter(|k| !k.is_empty())
                && let Some(&h) = self.src_map.get(&ow)
            {
                want!(field(Some(o), "handle"), POwner::H(h));
            }
        }
        for (i, o) in objs.iter().enumerate() {
            let nh = self.src(field(Some(o), "handle"));
            if nh != 0.0 && truthy(field(Some(o), "extDict")) && !thing_by_new.contains_key(&nh.to_bits()) {
                thing_by_new.insert(nh.to_bits(), (ThingKey::Obj(i), o.as_obj().unwrap_or(&empty)));
                want!(field(Some(o), "extDict"), POwner::H(nh));
            }
        }
        // closure: what a copied object owns
        while let Some(k) = queue.pop_front() {
            let Some(&pi) = plan_idx.get(&k) else { continue };
            let Some(oi) = plan.get(pi).map(|p| p.1) else { continue };
            let o: &Obj = obj_at(oi).unwrap_or(&empty_o);
            if truthy(o.get("extDict")) {
                want!(o.get("extDict"), POwner::Src(k.clone()));
            }
            if let Some(en) = o.obj("entries") {
                for (_, v) in js_entries(en) {
                    let c = by_h.get(&hex_key(Some(v))).and_then(|&i| obj_at(i));
                    if c.is_some_and(|c| hex_key(c.get("owner")).as_deref() == Some(k.as_str())) {
                        want!(Some(v), POwner::Src(k.clone()));
                    }
                }
            }
            for &ci in kids.get(&k).map(Vec::as_slice).unwrap_or(&[]) {
                let ch = obj_at(ci).and_then(|c| c.get("handle")).cloned();
                want!(ch.as_ref(), POwner::Src(k.clone()));
            }
        }
        // what a left-out object owns goes with it
        for o in objs {
            let k0 = hex_key(field(Some(o), "handle"));
            if k0.as_ref().is_some_and(|k| plan_idx.contains_key(k))
                || dropped_h.contains(&k0)
                || k0.as_ref().is_some_and(|k| self.src_map.contains_key(k))
            {
                continue;
            }
            let mut h = hex_key(field(Some(o), "owner")).filter(|k| !k.is_empty());
            let mut n: u32 = 0;
            while let Some(hh) = h.clone() {
                if dropped_h.contains(&Some(hh.clone())) || !by_h.contains_key(&Some(hh.clone())) || plan_idx.contains_key(&hh) || n >= 64 {
                    break;
                }
                n = n.saturating_add(1);
                h = by_h.get(&Some(hh)).and_then(|&i| obj_at(i)).and_then(|x| hex_key(x.get("owner"))).filter(|k| !k.is_empty());
            }
            if let Some(hh) = h
                && dropped_h.contains(&Some(hh))
            {
                dropped_h.insert(k0);
                inc(&mut dropped, &js_string(field(Some(o), "type")), 1.0);
            }
        }
        // handles
        let mut keys: Vec<String> = plan.iter().map(|p| p.0.clone()).collect();
        js_sort(&mut keys, |a, b| parse_int16(a) - parse_int16(b));
        let mut new_h: HashMap<String, f64> = HashMap::new();
        for k in &keys {
            let h = self.next();
            self.map_key(k.clone(), h);
            new_h.insert(k.clone(), h);
        }
        let mut raw_n = 0u32;
        let mut fields_n = 0u32;
        let mut by_type = Obj::new();
        let mut new_recs: Vec<Rec> = Vec::new();
        for k in &keys {
            let Some(&pi) = plan_idx.get(k) else { continue };
            let Some((_, oi, owner)) = plan.get(pi).cloned() else { continue };
            let o = obj_at(oi).cloned().unwrap_or_default();
            let ot = type_of(&o);
            let h = new_h.get(k).copied().unwrap_or(0.0);
            let owner_h = match owner {
                POwner::H(x) => x,
                POwner::Src(s) => self.src_map.get(&s).copied().unwrap_or(0.0),
            };
            let reactors = self.refs(o.get("reactors"));
            let xdic = if truthy(o.get("extDict")) { Some(self.src(o.get("extDict"))).filter(|x| *x != 0.0) } else { None };
            let mut ro = obj! { "t" => format!("COPY:{ot}"), "h" => h, "owner" => owner_h };
            if !reactors.is_empty() {
                ro.set("reactors", reactors.into_iter().map(V::Num).collect::<Vec<V>>());
            }
            ro.set_opt("xdic", xdic.map(V::Num));
            ro.set_opt("eed", self.x.eed_cache.get(&ThingKey::Obj(oi)).cloned());
            ro.set("newFormat", true);
            let mut rec = Rec::new(ro);
            inc(&mut by_type, &ot, 1.0);
            let ft = field_type(&ot).to_string();
            if ot == "DICTIONARY" || ot == "ACDBDICTIONARYWDFLT" || ot == "DICTIONARYWDFLT" {
                rec.o.set("t", if ot == "DICTIONARY" { "DICTIONARY" } else { "DICTIONARYWDFLT" });
                let en = o.obj("entries").cloned().unwrap_or_default();
                let items: Vec<V> = js_entries(&en)
                    .into_iter()
                    .filter_map(|(n, v)| {
                        let w = self.src(Some(v));
                        if n.is_empty() || w == 0.0 { None } else { Some(V::Arr(vec![V::from(n), V::Num(w)])) }
                    })
                    .collect();
                rec.o.set("items", items);
                rec.o.set("hardOwner", truthy(o.get("hardOwner")));
                rec.o.set("cloning", nn(o.get("cloning")).cloned().unwrap_or(V::Num(1.0)));
                if ot != "DICTIONARY" {
                    rec.o.set("defaultEntry", self.src(o.get("defaultEntry")));
                }
                fields_n = fields_n.saturating_add(1);
            } else if ot == "XRECORD" {
                let keep = if recompose_h.as_deref() == Some(k.as_str()) {
                    Some(arr(o.get("data")).iter().map(|it| recompose_keep(it.as_arr().and_then(|p| p.get(1)))).collect())
                } else {
                    None
                };
                rec.enc = Enc::XrecordCopy(Box::new(XrCopy { src: o.clone(), keep }));
                fields_n = fields_n.saturating_add(1);
            } else if !same_ver(&o)
                && !always_fields(&ot)
                && !(has_field_encoder(&ft) && !truthy(o.get("undecoded")))
                && conv_cache.entry(oi).or_insert_with(|| self.conv_of(&o)).is_some()
            {
                let conv = conv_cache.get(&oi).cloned().flatten();
                if let Some(conv) = conv {
                    let cls = conv.cls.clone();
                    rec.enc = Enc::Raw(Box::new(RawRec {
                        type_num: None,
                        cls: Some(cls),
                        raw: o.obj("raw").cloned().unwrap_or_default(),
                        is_entity: false,
                        proxy_class: false,
                        conv: Some(conv),
                    }));
                }
                inc(&mut self.x.proxied, &ot, 1.0);
                raw_n = raw_n.saturating_add(1);
            } else if always_fields(&ot) || !same_ver(&o) {
                let mut fc = FieldCopy {
                    kind: ft.clone(),
                    src: o.clone(),
                    list_h: Vec::new(),
                    list_li: Vec::new(),
                    list_pairs: Vec::new(),
                    owner_block: 0.0,
                };
                match ft.as_str() {
                    "GROUP" => fc.list_h = self.refs(o.get("entities")),
                    "IDBUFFER" => fc.list_h = self.refs(o.get("ids")),
                    "FIELDLIST" => fc.list_h = self.refs(o.get("fields")),
                    "LAYER_INDEX" => {
                        fc.list_li = arr(o.get("entries"))
                            .iter()
                            .map(|e| (e.as_obj().cloned().unwrap_or_default(), self.src(field(Some(e), "handle"))))
                            .filter(|(_, h)| *h != 0.0)
                            .collect();
                    }
                    "SORTENTSTABLE" => self.sort_list(&mut fc, &o),
                    _ => {}
                }
                rec.enc = Enc::FieldCopy(Box::new(fc));
                fields_n = fields_n.saturating_add(1);
            } else {
                let tn = if ot == "ACAD_PROXY_OBJECT" {
                    Some(499.0)
                } else {
                    nn(o.get("typeNum")).map(|v| to_number(Some(v))).or_else(|| self.class_num_of(&ot))
                };
                let cls = if tn.is_some_and(|t| t >= 500.0) { self.cls_of(tn) } else { None };
                if tn.is_none() || (tn.is_some_and(|t| t >= 500.0) && cls.is_none()) {
                    inc(&mut by_type, &ot, -1.0);
                    inc(&mut dropped, &ot, 1.0);
                    continue;
                }
                rec.enc = Enc::Raw(Box::new(RawRec {
                    type_num: tn,
                    cls,
                    raw: o.obj("raw").cloned().unwrap_or_default(),
                    is_entity: false,
                    proxy_class: true,
                    conv: None,
                }));
                raw_n = raw_n.saturating_add(1);
            }
            new_recs.push(rec);
        }
        self.recs.extend(new_recs);

        // the *Active viewport owns its SUN (20.4.64, R2007+)
        if let Some(a) = &act
            && hf.vport != 0.0
        {
            let ah = hex_key(field(Some(a), "handle"));
            let sun = objs
                .iter()
                .find(|o| matches!(field(Some(o), "type"), Some(V::Str(s)) if s == "SUN") && hex_key(field(Some(o), "owner")) == ah);
            let sh = sun.map_or(0.0, |s| self.src(field(Some(s), "handle")));
            if sh != 0.0
                && let Some(vr) = self.recs.iter_mut().find(|r| r.h() == hf.vport && r.t() == "VPORT")
            {
                vr.o.set("sun", sh);
            }
        }
        // multileaders whose style is not written get a Standard one
        let ts = self.style_ref(Some(&V::from("Standard")));
        self.ensure_mleader_style(ts, hf.lt_by_block);

        // written entities, table entries, block records and layouts: extension dictionaries and reactors
        let mut linked = 0u32;
        for i in 0..self.recs.len() {
            let Some(r) = self.recs.get(i) else { continue };
            let Some((key, s)) = thing_by_new.get(&r.h().to_bits()) else { continue };
            if r.t().starts_with("COPY:") {
                continue;
            }
            let t = r.t().to_string();
            let xd = if truthy(s.get("extDict")) { self.src(s.get("extDict")) } else { 0.0 };
            let set_xdic = xd != 0.0 && !truthy(r.g("xdic"));
            let set_eed = !truthy(r.g("eed"))
                && self.x.eed_cache.contains_key(key)
                && matches!(t.as_str(), "LAYER" | "STYLE" | "LTYPE" | "DIMSTYLE" | "BLOCK_HEADER" | "VPORT" | "VIEW");
            let have: Vec<f64> = arr(r.g("reactors")).iter().map(|x| num(Some(x), f64::NAN)).collect();
            let rs: Vec<f64> = self.refs(s.get("reactors")).into_iter().filter(|x| !have.contains(x)).collect();
            let eed = if set_eed { self.x.eed_cache.get(key).cloned() } else { None };
            let Some(r) = self.recs.get_mut(i) else { continue };
            if set_xdic {
                r.o.set("xdic", xd);
                linked = linked.saturating_add(1);
            }
            if let Some(e) = eed {
                r.o.set("eed", e);
            }
            if !rs.is_empty() {
                let mut all = arr(r.g("reactors")).to_vec();
                all.extend(rs.into_iter().map(V::Num));
                r.o.set("reactors", all);
            }
        }
        // the writer's own dictionaries
        let nod_adds: Vec<(String, f64)> = nod_add.iter().map(|(n, k)| (n.clone(), self.src_map.get(k).copied().unwrap_or(0.0))).collect();
        if let Some(nod) = self.recs.iter_mut().find(|r| r.h() == hf.nod && r.t() == "DICTIONARY") {
            let mut items = arr(nod.g("items")).to_vec();
            let have: HashSet<String> = items.iter().map(|x| js_string(x.as_arr().and_then(|a| a.first()))).collect();
            for (name, w) in nod_adds {
                if w != 0.0 && !have.contains(&name) {
                    items.push(V::Arr(vec![V::from(name.as_str()), V::Num(w)]));
                }
            }
            sort_items(&mut items);
            nod.o.set("items", items);
        }
        let plot_adds: Vec<(String, f64)> =
            plot_add.iter().map(|(n, k)| (n.clone(), self.src_map.get(k).copied().unwrap_or(0.0))).collect();
        if let Some(pd) = self.recs.iter_mut().find(|r| r.h() == hf.plot_style_dict && r.t() == "DICTIONARYWDFLT") {
            let mut items = arr(pd.g("items")).to_vec();
            for (name, w) in plot_adds {
                if w != 0.0 && !items.iter().any(|x| js_string(x.as_arr().and_then(|a| a.first())) == name) {
                    items.push(V::Arr(vec![V::from(name.as_str()), V::Num(w)]));
                }
            }
            pd.o.set("items", items);
        }
        for i in 0..self.recs.len() {
            let Some(r) = self.recs.get(i) else { continue };
            if r.t() != "LAYER" {
                continue;
            }
            let Some((_, sl)) = thing_by_new.get(&r.h().to_bits()) else { continue };
            let ps = if truthy(sl.get("plotStyle")) { self.src(sl.get("plotStyle")) } else { 0.0 };
            let mat = if truthy(sl.get("material")) { self.src(sl.get("material")) } else { 0.0 };
            if let Some(r) = self.recs.get_mut(i) {
                if ps != 0.0 {
                    r.o.set("plotstyle", ps);
                }
                if mat != 0.0 {
                    r.o.set("material", mat);
                }
            }
        }
        let group_adds: Vec<(String, f64)> =
            group_add.iter().map(|(n, k)| (n.clone(), self.src_map.get(k).copied().unwrap_or(0.0))).collect();
        if let Some(gd) = self.recs.iter_mut().find(|r| r.h() == hf.group_dict && r.t() == "DICTIONARY") {
            let mut items = arr(gd.g("items")).to_vec();
            for (name, w) in group_adds {
                if w != 0.0 {
                    items.push(V::Arr(vec![V::from(name.as_str()), V::Num(w)]));
                }
            }
            gd.o.set("items", items);
        }
        // header handles
        for (hv, key) in [
            ("DICTIONARY_MATERIALS", "materialsDict"),
            ("DICTIONARY_COLORS", "colorsDict"),
            ("DICTIONARY_VISUALSTYLE", "visualStyleDict"),
            ("DICTIONARY_PLOTSETTINGS", "plotSettingsDict"),
            ("CMATERIAL", "cmaterial"),
        ] {
            let n = self.src(hdr.and_then(|h| h.get(hv)));
            if n != 0.0 {
                self.x.header_handles.push((key, n));
            }
        }
        // report
        let total = raw_n.saturating_add(fields_n);
        if total > 0 {
            self.notes.push(crate::w_msg::msg(
                "ccdwg.write.nongraphic",
                &[("n", &total.to_string()), ("raw", &raw_n.to_string()), ("fields", &fields_n.to_string())],
            ));
        }
        let mut dn = Obj::new();
        for (t, n) in js_entries(&dropped) {
            if num(Some(n), 0.0) > 0.0 {
                dn.set(t.to_string(), n.clone());
            }
        }
        if !dn.is_empty() {
            self.notes.push(crate::w_msg::msg("ccdwg.write.not_copied", &[("list", &list_str(&dn))]));
        }
        let de = self.x.dropped_entities.clone();
        for (t, n) in js_entries(&de) {
            let ns = crate::w_js::num_to_string(num(Some(n), 0.0));
            self.notes.push(crate::w_msg::msg("ccdwg.write.entities_dropped", &[("n", &ns), ("type", t)]));
        }
        if !self.x.proxied.is_empty() {
            self.notes.push(crate::w_msg::msg("ccdwg.write.proxied", &[("list", &list_str(&self.x.proxied))]));
        }
        if !self.x.raw_entities.is_empty() {
            self.notes.push(crate::w_msg::msg("ccdwg.write.raw", &[("list", &list_str(&self.x.raw_entities))]));
        }
        self.report_copied = Some(V::Obj(obj! {
            "objects" => by_type, "dropped" => dropped, "proxied" => self.x.proxied.clone(), "rawEntities" => self.x.raw_entities.clone(),
            "droppedEntities" => self.x.dropped_entities.clone(), "linkedExtDicts" => linked,
        }));
    }

    fn conv_of(&self, o: &Obj) -> Option<Plan> {
        let ot = type_of(o);
        if !(truthy(o.get("undecoded")) || ot == "ACAD_PROXY_OBJECT") || !truthy(o.get("raw")) {
            return None;
        }
        let tn = nn(o.get("typeNum")).map(|v| to_number(Some(v))).or_else(|| self.class_num_of(&ot));
        self.plan_for(o.obj("raw"), ot == "ACAD_PROXY_OBJECT", tn)
    }
    /// refs(list): written handles of a list (unwritten ones left out)
    pub fn refs(&self, list: Option<&V>) -> Vec<f64> {
        arr(list).iter().map(|v| self.src(Some(v))).filter(|h| *h != 0.0).collect()
    }
    /// SORTENTSTABLE: every entity of the block, in the source's draw order
    fn sort_list(&self, fc: &mut FieldCopy, o: &Obj) {
        let block_rec = hex_key(o.get("ownerBlock"));
        fc.owner_block = block_rec.as_ref().and_then(|k| self.src_map.get(k).copied()).unwrap_or(0.0);
        let b = arr(self.model.get("blocks")).iter().find(|x| hex_key(field(Some(x), "record")) == block_rec);
        let mut sort_of: HashMap<Option<String>, f64> = HashMap::new();
        let sh = arr(o.get("sortHandles"));
        for (i, e) in arr(o.get("entities")).iter().enumerate() {
            let s = sh.get(i).filter(|x| x.truthy()).unwrap_or(e);
            sort_of.insert(hex_key(Some(e)), parse_int16(&js_string(Some(s))));
        }
        let mut ents: Vec<(f64, f64, f64)> = arr(b.and_then(|b| field(Some(b), "entities")))
            .iter()
            .map(|e| {
                let hh = field(Some(e), "handle");
                let src = parse_int16(&js_string(hh));
                let key = sort_of.get(&hex_key(hh)).copied().unwrap_or(src);
                (src, key, self.src(hh))
            })
            .filter(|x| x.2 != 0.0)
            .collect();
        js_sort(&mut ents, |a, c| {
            let d = a.1 - c.1;
            if d != 0.0 && !d.is_nan() { d } else { a.0 - c.0 }
        });
        let mut new_hs: Vec<f64> = ents.iter().map(|x| x.2).collect();
        js_sort(&mut new_hs, |a, c| a - c);
        fc.list_pairs = ents.iter().enumerate().map(|(i, x)| (x.2, new_hs.get(i).copied().unwrap_or(0.0))).collect();
    }
}

/// `p.owner`: a new handle or a source hex resolved when the copy is written
#[derive(Clone, Debug)]
enum POwner {
    H(f64),
    Src(String),
}
fn truthy_key(v: Option<&V>) -> bool {
    hex_key(v).is_some_and(|k| !k.is_empty())
}
/// `items.sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0))`
pub fn sort_items(items: &mut [V]) {
    js_sort(items, |a, b| {
        let x = js_string(a.as_arr().and_then(|v| v.first()));
        let y = js_string(b.as_arr().and_then(|v| v.first()));
        if crate::w_write::js_lt(&x, &y) {
            -1.0
        } else if crate::w_write::js_lt(&y, &x) {
            1.0
        } else {
            0.0
        }
    });
}

/// XRECORD (20.4.104) with handle values and object ids mapped.
pub fn encode_xrecord(x: &XrCopy, r: &Rec, cx: &mut Cx) -> Vec<u8> {
    let o = &x.src;
    let is_h = |c: f64| (320.0..=369.0).contains(&c) || (390.0..=399.0).contains(&c) || c == 480.0 || c == 481.0 || c == 1005.0;
    // rec.keep = ([c, v]) => c !== 330 || (recomposeKeep(v) && !!written(v))
    let keep = |i: usize, it: &V| -> bool {
        let Some(k) = &x.keep else { return true };
        let p = it.as_arr().map(Vec::as_slice).unwrap_or(&[]);
        if !matches!(p.first(), Some(V::Num(n)) if *n == 330.0) {
            return true;
        }
        k.get(i).copied().unwrap_or(false) && cx.env.map(p.get(1)) != 0.0
    };
    let data: Vec<V> = arr(o.get("data"))
        .iter()
        .enumerate()
        .filter(|(i, it)| keep(*i, it))
        .map(|(_, it)| it)
        .map(|it| {
            let p = it.as_arr().map(Vec::as_slice).unwrap_or(&[]);
            let c = p.first().cloned().unwrap_or(V::Null);
            let cn = match &c {
                V::Num(n) => *n,
                _ => f64::NAN,
            };
            if is_h(cn) {
                V::Arr(vec![c, V::Str(hex_upper(cx.env.map(p.get(1))))])
            } else {
                V::Arr(vec![c, p.get(1).cloned().unwrap_or(V::Null)])
            }
        })
        .collect();
    let mut w = BitWriter::obj_writer(cx.version, false);
    let ty = cx.classes.class_num("XRECORD");
    begin(&mut w, ty, r);
    object_common(&mut w, r);
    let mut b = xrecord_bytes(&data, w.r2007);
    // Spec gap: group codes outside the DXF ranges stop the reader's data early; in the same string form the source's
    // bytes are written back as they are.
    if let Some(raw) = o.obj("raw")
        && let Some(rb) = raw_xrecord_bytes(raw)
        && x.keep.is_none()
        && (num(raw.get("ver"), 0.0) >= 21.0) == w.r2007
        && rb.len() != b.len()
    {
        b = rb;
    }
    w.bl(usize_to_f64(b.len()));
    for v in b {
        w.rc8(v);
    }
    if cx.ver >= 15 {
        w.bs(nnum(o.get("cloning"), 1.0));
    }
    for h in arr(o.get("objids")) {
        w.h(SOFT_PTR, cx.env.map(Some(h)));
    }
    finish(&mut w)
}
/// The data bytes of an XRECORD as the reader kept them raw: BL count, then the bytes.
fn raw_xrecord_bytes(raw: &Obj) -> Option<Vec<u8>> {
    let u = unb64(raw.str("data")?);
    let n = crate::util::to_usize(num(raw.get("n"), 0.0));
    let mut r = crate::bits::BitReader::range(&u, crate::util::to_u32(num(raw.get("ver"), 0.0)), crate::bits::Codepage::latin1(), 0, n);
    let cnt = r.bl().ok()?;
    if cnt < 0 || f64::from(cnt) * 8.0 > num(raw.get("n"), 0.0) {
        return None;
    }
    r.bytes(crate::util::usize_from_i32(cnt)).ok()
}

/// FIELD_ENCODERS for copies (`simple(t, body)`, MLEADERSTYLE, FIELD, ACDBPLACEHOLDER).
pub fn encode_field_copy(fc: &FieldCopy, r: &Rec, cx: &mut Cx) -> Option<Vec<u8>> {
    let o = &fc.src;
    match fc.kind.as_str() {
        "MLEADERSTYLE" => {
            let m = crate::w_mleader::MlsRec { src: o.clone(), defaults: None };
            return Some(crate::w_mleader::encode_mleader_style(&m, r, cx));
        }
        "FIELD" => return Some(crate::w_table::encode_field_copy(o, r, cx)),
        _ => {}
    }
    if !has_field_encoder(&fc.kind) {
        return None;
    }
    let mut w = BitWriter::obj_writer(cx.version, false);
    let ty = match fc.kind.as_str() {
        "GROUP" => 72.0,
        "ACDBPLACEHOLDER" => cx.classes.class_num("PLACEHOLDER"),
        k => cx.classes.class_num(k),
    };
    begin(&mut w, ty, r);
    object_common(&mut w, r);
    let g = |k: &str| o.get(k);
    match fc.kind.as_str() {
        "GROUP" => {
            w.tv(&str_or(g("name"), ""));
            w.bs(nnum(g("unnamed"), 1.0));
            w.bs(nnum(g("selectable"), 1.0));
            w.bl(usize_to_f64(fc.list_h.len()));
            for &h in &fc.list_h {
                w.h(HARD_PTR, h);
            }
        }
        "DICTIONARYVAR" => {
            w.rc(f64::from(int32(g("intval"))));
            w.tv(&str_or(g("value"), ""));
        }
        "SCALE" => {
            w.bs(0.0);
            w.tv(&str_or(g("name"), ""));
            w.bd(num(g("paperUnits"), 1.0));
            w.bd(num(g("drawingUnits"), 1.0));
            w.b(truthy(g("unitScale")));
        }
        "IDBUFFER" => {
            w.rc8(0);
            w.bl(usize_to_f64(fc.list_h.len()));
            for &h in &fc.list_h {
                w.h(SOFT_PTR, h);
            }
        }
        "FIELDLIST" => {
            w.bl(usize_to_f64(fc.list_h.len()));
            w.b(truthy(g("unknown")));
            for &h in &fc.list_h {
                w.h(HARD_OWN, h);
            }
        }
        "LAYER_INDEX" => {
            w.bl(0.0);
            w.bl(0.0);
            w.bl(usize_to_f64(fc.list_li.len()));
            for (e, _) in &fc.list_li {
                w.bl(f64::from(int32(e.get("index"))));
                w.tv(&str_or(e.get("name"), ""));
            }
            for (_, h) in &fc.list_li {
                w.h(HARD_PTR, *h);
            }
        }
        "SPATIAL_FILTER" => {
            let pts = arr(g("points"));
            w.bs(usize_to_f64(pts.len()));
            for p in pts {
                w.rd2(num(field(Some(p), "x"), 0.0), num(field(Some(p), "y"), 0.0));
            }
            let e = g("extrusion").filter(|v| v.truthy());
            match e {
                Some(e) => w.bd3(num(field(Some(e), "x"), 0.0), num(field(Some(e), "y"), 0.0), num(field(Some(e), "z"), 1.0)),
                None => w.bd3(0.0, 0.0, 1.0),
            }
            let og = g("origin").filter(|v| v.truthy());
            w.bd3(num(field(og, "x"), 0.0), num(field(og, "y"), 0.0), num(field(og, "z"), 0.0));
            w.bs(f64::from(int32(g("displayBoundary"))));
            for k in ["frontClip", "backClip"] {
                match nn(g(k)) {
                    Some(v) => {
                        w.bs(1.0);
                        w.bd(num(Some(v), 0.0));
                    }
                    None => w.bs(0.0),
                }
            }
            for k in ["inverseBlockTransform", "clipTransform"] {
                let m = g(k);
                for i in 0..12usize {
                    let d = if i % 4 == 0 { 1.0 } else { 0.0 };
                    w.bd(num(m.and_then(V::as_arr).and_then(|a| a.get(i)), d));
                }
            }
        }
        "SORTENTSTABLE" => {
            w.bl(usize_to_f64(fc.list_pairs.len()));
            for &(_, s) in &fc.list_pairs {
                w.hd(0, s);
            }
            w.h(SOFT_PTR, fc.owner_block);
            for &(e, _) in &fc.list_pairs {
                w.h(SOFT_PTR, e);
            }
        }
        "PLOTSETTINGS" => {
            let p = g("plot").filter(|v| v.truthy());
            let pg = |k: &str| field(p, k);
            w.tv(&str_or(pg("pageSetupName"), ""));
            w.tv(&str_or(pg("printer"), "none_device"));
            w.bs(nnum(pg("flags"), 688.0));
            let m = pg("margins").filter(|v| v.truthy());
            for k in ["left", "bottom", "right", "top"] {
                w.bd(num(field(m, k), 6.35));
            }
            w.bd(num(pg("paperWidth"), 215.9));
            w.bd(num(pg("paperHeight"), 279.4));
            w.tv(&str_or(pg("paperSize"), ""));
            w.bd2(num(field(pg("origin"), "x"), 0.0), num(field(pg("origin"), "y"), 0.0));
            w.bs(f64::from(int32(pg("paperUnits"))));
            w.bs(f64::from(int32(pg("rotation"))));
            w.bs(f64::from(int32(pg("plotType"))));
            w.bd2(num(field(pg("windowMin"), "x"), 0.0), num(field(pg("windowMin"), "y"), 0.0));
            w.bd2(num(field(pg("windowMax"), "x"), 0.0), num(field(pg("windowMax"), "y"), 0.0));
            if !w.r2004 {
                w.t8(&str_or(pg("viewName"), ""));
            }
            w.bd(num(pg("realWorldUnits"), 1.0));
            w.bd(num(pg("drawingUnits"), 1.0));
            w.tv(&str_or(pg("styleSheet"), ""));
            w.bs(f64::from(int32(pg("scaleType"))));
            w.bd(num(pg("scaleFactor"), 1.0));
            w.bd2(num(field(pg("imageOrigin"), "x"), 0.0), num(field(pg("imageOrigin"), "y"), 0.0));
            if w.r2004 {
                w.bs(f64::from(int32(pg("shadePlotMode"))));
                w.bs(nnum(pg("shadePlotRes"), 2.0));
                w.bs(nnum(pg("shadePlotDpi"), 300.0));
                w.h(HARD_PTR, 0.0);
            }
            // Spec gap: R2007+ plot settings end with one more handle, the shade-plot visual style (soft pointer).
            if w.r2007 {
                w.h(SOFT_PTR, 0.0);
            }
        }
        "RASTERVARIABLES" => {
            w.bl(f64::from(int32(g("classVersion"))));
            w.bs(nnum(g("displayFrame"), 1.0));
            w.bs(nnum(g("displayQuality"), 1.0));
            w.bs(f64::from(int32(g("units"))));
        }
        "WIPEOUTVARIABLES" => w.bs(nnum(g("displayFrame"), 1.0)),
        _ => {}
    }
    let _ = version_num;
    Some(finish(&mut w))
}
