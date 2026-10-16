//! Editable tables: ACAD_TABLE, TABLESTYLE and FIELD written in every version R2000–R2018 (port of
//! lib/dwg/table-write.js; ODA spec 20.4.96–101, 20.4.76). The layouts are the ones table-read.js decodes (its "Spec
//! gap:" notes apply here too): R2000 table objects carry R2004-form colours, the cell alignment is a BS, an R2007+ text
//! cell keeps its text only in its value, a string value from R2007 is BL byte count + UTF-16 with NUL, each R24
//! table-style cell style is preceded by its id.
//! Spec gap: R2000–R2007 round-trip copies (TABLECONTENT, TABLEGEOMETRY, DATATABLE, CELLSTYLEMAP, ACAD_XREC_ROUNDTRIP)
//! are not written: the ODA File Converter prefers stale round-trip data over the entity's when both exist.
//! Spec gap: AutoCAD leaves a "general" value's bytes empty and keeps the text in the value string; in an R2007 table
//! cell the text goes as UTF-16 + NUL in the bytes (still type 512) so the ODA File Converter shows it.

use std::collections::HashSet;

use crate::obj;
use crate::util::usize_to_f64;
use crate::value::{Obj, V};
use crate::w_bits::BitWriter;
use crate::w_js::{arr, field, hex_key, int32, js_string, nn, num, parse_int16, str_or, to_number, truthy, units};
use crate::w_objects::{Cx, HARD_OWN, HARD_PTR, Rec, SOFT_PTR, entity_common, insert_scale, nnum, object_common, ornum};
use crate::w_write::{Conv, Kind, W};

const FIELD_KEY: &str = "\u{1}fieldH";
/// The largest table grid written as a table (rows × columns).
const MAX_CELLS: usize = 4_000_000;

#[derive(Clone, Debug)]
pub enum TableX {
    Table { style_h: f64, ids: Vec<(String, f64)>, table: Obj },
    Style { style: Obj, tables: Vec<V>, owner_dict: f64 },
    Field { field: Obj, kids: Vec<f64>, parent: f64, src_owner: Option<V>, table_h: f64 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SKey {
    Default,
    Hex(Option<String>),
}
#[derive(Default)]
pub struct TState {
    pub skip_handles: HashSet<String>,
    styles: Vec<(SKey, f64)>,
    obj_recs: Vec<Rec>,
    dict: Option<Rec>,
    n_tables: u32,
    n_field_cells: u32,
}

fn round_trip(t: &str) -> bool {
    matches!(t, "TABLECONTENT" | "TABLEGEOMETRY" | "DATATABLE" | "CELLSTYLEMAP")
}
fn rt_entry(n: &str) -> bool {
    n == "ACAD_XREC_ROUNDTRIP" || n == "ACAD_ROUNDTRIP_2008_TABLESTYLE_CELLSTYLEMAP"
}
fn default_style_name(id: f64) -> Option<V> {
    let s = if id == 1.0 {
        "_TITLE"
    } else if id == 2.0 {
        "_HEADER"
    } else if id == 3.0 {
        "_DATA"
    } else if id == 4.0 {
        "Table"
    } else {
        return None;
    };
    Some(V::from(s))
}
fn idx(i: f64) -> V {
    V::Obj(obj! { "index" => i })
}
fn default_table_style() -> Obj {
    let cs = |id: f64, name: &str, sc: f64, th: f64, al: f64| {
        V::Obj(obj! {
            "id" => id, "name" => name, "styleClass" => sc, "textStyle" => "Standard", "textHeight" => th, "align" => al,
            "color" => idx(0.0), "background" => idx(257.0), "backgroundEnabled" => false,
        })
    };
    obj! {
        "name" => "Standard", "description" => "", "flowDirection" => 0, "flags" => 0, "hMargin" => 0.06, "vMargin" => 0.06,
        "titleSuppressed" => false, "headerSuppressed" => false,
        "cellStyles" => vec![cs(3.0, "_DATA", 1.0, 0.18, 2.0), cs(1.0, "_TITLE", 2.0, 0.25, 5.0), cs(2.0, "_HEADER", 2.0, 0.18, 5.0)],
    }
}

impl W<'_> {
    /// tableWriter(env): the round-trip objects to leave out (COPY_SKIP).
    pub fn tbl_init(&mut self) {
        let model = self.model;
        let objs = arr(model.get("objects"));
        let by_h = |k: &Option<String>| objs.iter().rev().find(|o| hex_key(field(Some(o), "handle")) == *k);
        let mut xdicts: Vec<V> = objs
            .iter()
            .filter(|o| matches!(field(Some(o), "type"), Some(V::Str(s)) if s == "TABLESTYLE") && truthy(field(Some(o), "extDict")))
            .filter_map(|o| field(Some(o), "extDict").cloned())
            .collect();
        for b in arr(model.get("blocks")) {
            for e in arr(field(Some(b), "entities")) {
                if js_string(field(Some(e), "type")).to_uppercase() == "ACAD_TABLE" && truthy(field(Some(e), "extDict")) {
                    xdicts.extend(field(Some(e), "extDict").cloned());
                }
            }
        }
        let mut skip = HashSet::new();
        for xh in &xdicts {
            let Some(d) = by_h(&hex_key(Some(xh))) else { continue };
            if !matches!(field(Some(d), "type"), Some(V::Str(s)) if s == "DICTIONARY") || !truthy(field(Some(d), "entries")) {
                continue;
            }
            let Some(en) = field(Some(d), "entries").and_then(V::as_obj) else { continue };
            for (name, h) in crate::w_js::js_entries(en) {
                if !rt_entry(name) {
                    continue;
                }
                skip.extend(hex_key(Some(h)));
                let x = by_h(&hex_key(Some(h)));
                if x.is_some_and(|x| matches!(field(Some(x), "type"), Some(V::Str(s)) if s == "XRECORD")) {
                    for it in arr(x.and_then(|x| field(Some(x), "data"))) {
                        let p = it.as_arr().map(Vec::as_slice).unwrap_or(&[]);
                        let c = p.first();
                        if matches!(c, Some(V::Num(n)) if *n == 360.0 || *n == 361.0)
                            && by_h(&hex_key(p.get(1))).is_some_and(|o| round_trip(&js_string(field(Some(o), "type"))))
                        {
                            skip.extend(hex_key(p.get(1)));
                        }
                    }
                }
            }
            if !en.is_empty() && en.keys().all(rt_entry) {
                skip.extend(hex_key(Some(xh)));
            }
        }
        self.tbl.skip_handles = skip;
    }

    fn ensure_dict(&mut self) {
        if self.tbl.dict.is_some() {
            return;
        }
        let model = self.model;
        let objs = arr(model.get("objects"));
        let src_style = objs.iter().find(|o| matches!(field(Some(o), "type"), Some(V::Str(s)) if s == "TABLESTYLE"));
        let src_dict = src_style.and_then(|s| {
            let k = hex_key(field(Some(s), "owner"));
            objs.iter().rev().find(|o| hex_key(field(Some(o), "handle")) == k)
        });
        let h = self.next();
        self.tbl.dict =
            Some(Rec::new(obj! { "t" => "DICTIONARY", "h" => h, "owner" => 0, "items" => Vec::<V>::new(), "hardOwner" => false }));
        if let Some(d) = src_dict {
            self.map_src(field(Some(d), "handle"), h);
        }
        for o in objs {
            if matches!(field(Some(o), "type"), Some(V::Str(s)) if s == "TABLESTYLE")
                && let Some(oo) = o.as_obj()
            {
                self.add_tstyle(Some(oo));
            }
        }
    }
    /// addStyle(o) → the style record's handle
    fn add_tstyle(&mut self, o: Option<&Obj>) -> f64 {
        let k = match o {
            Some(o) => SKey::Hex(hex_key(o.get("handle"))),
            None => SKey::Default,
        };
        if let Some((_, h)) = self.tbl.styles.iter().find(|(kk, _)| *kk == k) {
            return *h;
        }
        let st = o.cloned().unwrap_or_else(default_table_style);
        let h = self.next();
        if let Some(o) = o {
            self.map_src(o.get("handle"), h);
        }
        let name = if truthy(st.get("name")) {
            js_string(st.get("name"))
        } else if o.is_some() {
            format!("Style{}", crate::util::hex_of(h).to_lowercase())
        } else {
            "Standard".into()
        };
        let Some(dict) = self.tbl.dict.as_mut() else { return h };
        let mut items = arr(dict.g("items")).to_vec();
        let used: HashSet<String> = items.iter().map(|x| js_string(x.as_arr().and_then(|a| a.first())).to_uppercase()).collect();
        let mut n = name.clone();
        let mut i = 2u32;
        while used.contains(&n.to_uppercase()) {
            n = format!("{name}_{i}");
            i = i.saturating_add(1);
        }
        items.push(V::Arr(vec![V::from(n.as_str()), V::Num(h)]));
        dict.o.set("items", items);
        let mut rec = Rec::new(obj! { "t" => "TABLESTYLE", "h" => h, "owner" => 0 });
        rec.tx = Some(Box::new(TableX::Style { style: st, tables: Vec::new(), owner_dict: 0.0 }));
        self.tbl.styles.push((k, h));
        self.tbl.obj_recs.push(rec);
        h
    }
    fn tstyle_obj(&self, h: f64) -> Option<&Obj> {
        self.tbl.obj_recs.iter().find(|r| r.h() == h).and_then(|r| match r.tx.as_deref() {
            Some(TableX::Style { style, .. }) => Some(style),
            _ => None,
        })
    }
    /// fieldRecs(f, tableH) → the FIELD record's handle
    fn field_recs(&mut self, f: &Obj, table_h: f64, depth: u32) -> f64 {
        let k = hex_key(f.get("handle")).filter(|k| !k.is_empty());
        if let Some(h) = k.as_ref().and_then(|k| self.src_map.get(k).copied()) {
            return h;
        }
        let h = self.next();
        if let Some(k) = &k {
            self.map_key(k.clone(), h);
        }
        let mut kids = Vec::new();
        if depth < 64 {
            for c in arr(f.get("children")) {
                let co = c.as_obj().cloned().unwrap_or_default();
                let kh = self.field_recs(&co, h, depth.saturating_add(1));
                if kh != 0.0 {
                    kids.push(kh);
                }
            }
        }
        for r in &mut self.tbl.obj_recs {
            if r.t() == "FIELD"
                && kids.contains(&r.h())
                && let Some(TableX::Field { parent, .. }) = r.tx.as_deref_mut()
            {
                *parent = h;
            }
        }
        let model = self.model;
        let src_owner = k
            .as_ref()
            .and_then(|k| arr(model.get("objects")).iter().rev().find(|o| hex_key(field(Some(o), "handle")).as_deref() == Some(k.as_str())))
            .and_then(|o| field(Some(o), "owner").cloned());
        let src_obj_exists = k
            .as_ref()
            .is_some_and(|k| arr(model.get("objects")).iter().any(|o| hex_key(field(Some(o), "handle")).as_deref() == Some(k.as_str())));
        let mut rec = Rec::new(obj! { "t" => "FIELD", "h" => h, "owner" => 0 });
        rec.tx = Some(Box::new(TableX::Field {
            field: f.clone(),
            kids,
            parent: 0.0,
            src_owner: if src_obj_exists { src_owner } else { None },
            table_h,
        }));
        self.tbl.obj_recs.push(rec);
        h
    }

    /// T.convert(e, owner, c)
    pub fn tbl_convert(&mut self, e: &Obj, owner: f64) -> Conv {
        let b = {
            let a = self.block_by_name.get(&str_or(e.get("block"), "").to_uppercase()).copied();
            a.or_else(|| {
                let k = hex_key(nn(e.get("blockRecord")).or_else(|| e.get("block")));
                self.blocks.iter().position(|x| hex_key(x.src.get("record")) == k)
            })
        };
        let Some(bi) = b.filter(|&i| self.blocks.get(i).is_some_and(|x| x.kind == Kind::Block)) else {
            return Conv::Skip("ACAD_TABLE without its block".into());
        };
        let t = e.get("table");
        let mut head = self.common(e, "ACAD_TABLE", owner);
        head.o.set("insert", crate::w_convert::p3w(e.get("insert")));
        head.o.set_opt("scale", e.get("scale").cloned());
        head.o.set("rotation", num(e.get("rotation"), 0.0));
        head.o.set_opt("extrusion", e.get("extrusion").cloned());
        head.o.set_opt("horizDir", e.get("horizDir").cloned());
        head.block_ref = Some(Some(bi));
        let hh = head.h();
        if let Some(b) = self.blocks.get_mut(bi) {
            b.inserts.push(hh);
        }
        let rows = field(t, "rows").and_then(V::as_arr);
        let cols = field(t, "columns").and_then(V::as_arr);
        // hostile input: a grid of more than MAX_CELLS cells (the JS would try to write them all) is a reference too
        let too_big = rows.map_or(0, Vec::len).saturating_mul(cols.map_or(0, Vec::len)) > MAX_CELLS;
        if !truthy(t) || rows.is_none_or(Vec::is_empty) || cols.is_none_or(Vec::is_empty) || too_big {
            // no table data in the model: a reference to the block
            head.o.set("t", "INSERT");
            head.o.set("attribs", Vec::<V>::new());
            head.o.set("cols", 1);
            head.o.set("rows", 1);
            head.o.set("colSpacing", 0);
            head.o.set("rowSpacing", 0);
            return Conv::Recs(vec![head]);
        }
        self.tbl.n_tables = self.tbl.n_tables.saturating_add(1);
        self.ensure_dict();
        let model = self.model;
        let objs = arr(model.get("objects"));
        let ts = field(t, "style");
        let by_style = if truthy(ts) {
            objs.iter()
                .rev()
                .find(|o| hex_key(field(Some(o), "handle")) == hex_key(ts))
                .filter(|o| matches!(field(Some(o), "type"), Some(V::Str(s)) if s == "TABLESTYLE"))
                .and_then(V::as_obj)
        } else {
            None
        };
        let sv = by_style.map(|o| self.add_tstyle(Some(o)));
        let sv = sv
            .or_else(|| {
                let want = (if truthy(field(t, "styleName")) { js_string(field(t, "styleName")) } else { String::new() }).to_uppercase();
                self.tbl
                    .styles
                    .iter()
                    .map(|x| x.1)
                    .find(|&h| js_string(self.tstyle_obj(h).and_then(|s| s.get("name"))).to_uppercase() == want)
            })
            .or_else(|| {
                self.tbl
                    .styles
                    .iter()
                    .map(|x| x.1)
                    .find(|&h| js_string(self.tstyle_obj(h).and_then(|s| s.get("name"))).to_lowercase() == "standard")
            });
        let sv = match sv {
            Some(h) => h,
            None => self.add_tstyle(None),
        };
        let src_handle = e.get("handle").cloned().unwrap_or(V::Null);
        let style_obj = self.tstyle_obj(sv).cloned().unwrap_or_default();
        for r in &mut self.tbl.obj_recs {
            if r.h() == sv
                && let Some(TableX::Style { tables, .. }) = r.tx.as_deref_mut()
            {
                tables.push(src_handle.clone());
            }
        }
        // styleNameIds
        let mut ids: Vec<(String, f64)> =
            vec![("_TITLE".into(), 1.0), ("_HEADER".into(), 2.0), ("_DATA".into(), 3.0), ("TABLE".into(), 4.0)];
        for c in arr(style_obj.get("cellStyles")) {
            if truthy(field(Some(c), "name")) {
                let n = js_string(field(Some(c), "name")).to_uppercase();
                let id = nn(field(Some(c), "id")).map_or(0.0, |x| to_number(Some(x)));
                match ids.iter_mut().find(|x| x.0 == n) {
                    Some(x) => x.1 = id,
                    None => ids.push((n, id)),
                }
            }
        }
        // cells: shallow copies (overrides and their borders copied too)
        let mut cells: Vec<Vec<Obj>> = arr(field(t, "cells"))
            .iter()
            .map(|row| {
                arr(Some(row))
                    .iter()
                    .map(|x| {
                        let mut c = x.as_obj().cloned().unwrap_or_default();
                        if let Some(ov) = c.obj("overrides").cloned()
                            && truthy(c.get("overrides"))
                        {
                            c.set("overrides", ov);
                        }
                        c
                    })
                    .collect()
            })
            .collect();
        canonical_borders(&mut cells);
        for row in &mut cells {
            for c in row.iter_mut() {
                if truthy(c.get("field")) && !matches!(c.get("type"), Some(V::Str(s)) if s == "block") {
                    let f = c.obj("field").cloned().unwrap_or_default();
                    let fh = self.field_recs(&f, hh, 0);
                    c.set(FIELD_KEY, fh);
                    self.tbl.n_field_cells = self.tbl.n_field_cells.saturating_add(1);
                }
            }
        }
        let mut table = t.and_then(V::as_obj).cloned().unwrap_or_default();
        table.set("cells", cells.into_iter().map(|r| V::Arr(r.into_iter().map(V::Obj).collect())).collect::<Vec<V>>());
        head.tx = Some(Box::new(TableX::Table { style_h: sv, ids, table }));
        Conv::Recs(vec![head])
    }

    /// T.finish: table styles, their dictionary, cell fields.
    pub fn tbl_finish(&mut self) {
        let Some(mut dict) = self.tbl.dict.take() else { return };
        let nod_h = self.h.nod;
        dict.o.set("owner", nod_h);
        dict.o.set("reactors", vec![V::Num(nod_h)]);
        let dh = dict.h();
        let style_hs: Vec<f64> = self.tbl.styles.iter().map(|s| s.1).collect();
        for r in &mut self.tbl.obj_recs {
            if style_hs.contains(&r.h()) {
                r.o.set("owner", dh);
                if let Some(TableX::Style { owner_dict, .. }) = r.tx.as_deref_mut() {
                    *owner_dict = dh;
                }
            }
        }
        self.recs.push(dict);
        let objs = std::mem::take(&mut self.tbl.obj_recs);
        self.recs.extend(objs);
        if let Some(nod) = self.recs.iter_mut().find(|r| r.h() == nod_h && r.t() == "DICTIONARY") {
            let mut items = arr(nod.g("items")).to_vec();
            if !items.iter().any(|x| js_string(x.as_arr().and_then(|a| a.first())) == "ACAD_TABLESTYLE") {
                items.push(V::Arr(vec![V::from("ACAD_TABLESTYLE"), V::Num(dh)]));
                crate::w_nongraphic::sort_items(&mut items);
                nod.o.set("items", items);
            }
        }
        self.report_tables =
            Some(V::Obj(obj! { "tables" => self.tbl.n_tables, "styles" => self.tbl.styles.len(), "fieldCells" => self.tbl.n_field_cells }));
    }
}

/// table-read.js canonicalBorders: a top or left override moves to the neighbour (its bottom / right).
fn canonical_borders(cells: &mut [Vec<Obj>]) {
    for i in 0..cells.len() {
        let nj = cells.get(i).map_or(0, Vec::len);
        for j in 0..nj {
            let Some(mut b) = cells.get(i).and_then(|r| r.get(j)).and_then(|c| c.obj("overrides")).and_then(|o| o.obj("borders")).cloned()
            else {
                continue;
            };
            if !cells.get(i).and_then(|r| r.get(j)).is_some_and(|c| truthy(field(c.get("overrides"), "borders"))) {
                continue;
            }
            for (side, up, to) in [("top", true, "bottom"), ("left", false, "right")] {
                let Some(moved) = b.get(side).filter(|v| v.truthy()).cloned() else { continue };
                let (ni, nj2) = if up { (i.checked_sub(1), Some(j)) } else { (Some(i), j.checked_sub(1)) };
                let Some(n) = ni.zip(nj2).and_then(|(a, c)| cells.get_mut(a).and_then(|r| r.get_mut(c))) else { continue };
                if !truthy(n.get("overrides")) {
                    n.set("overrides", Obj::new());
                }
                if let Some(ov) = n.obj_mut("overrides") {
                    let mut borders = ov.obj("borders").cloned().unwrap_or_default();
                    let mut inner = borders.obj(to).cloned().unwrap_or_default();
                    if let Some(m) = moved.as_obj() {
                        for (k, v) in m.iter() {
                            inner.set(k.to_string(), v.clone());
                        }
                    }
                    borders.set(to, inner);
                    ov.set("borders", borders);
                }
                b.remove(side);
            }
            if let Some(c) = cells.get_mut(i).and_then(|r| r.get_mut(j)) {
                if b.is_empty() {
                    if let Some(ov) = c.obj_mut("overrides") {
                        ov.remove("borders");
                    }
                    if c.obj("overrides").is_some_and(Obj::is_empty) {
                        c.remove("overrides");
                    }
                } else if let Some(ov) = c.obj_mut("overrides") {
                    ov.set("borders", b);
                }
            }
        }
    }
}

// ---------------------------------------------------------------- encoding
/// Colours of table data: the R2004 CMC form in every version.
fn cmc(w: &mut BitWriter, c: Option<&V>) {
    let d = idx(256.0);
    let c = c.filter(|v| v.truthy()).unwrap_or(&d);
    if w.r2004 {
        w.cmc(Some(c));
        return;
    }
    w.r2004 = true;
    w.cmc(Some(c));
    w.r2004 = false;
}
fn cmc_or(w: &mut BitWriter, c: Option<&V>, d: V) {
    match c.filter(|v| v.truthy()) {
        Some(c) => cmc(w, Some(c)),
        None => cmc(w, Some(&d)),
    }
}
fn utf16z(s: &str) -> Vec<u8> {
    let mut b = Vec::new();
    for u in units(s) {
        b.extend_from_slice(&u.to_le_bytes());
    }
    b.extend_from_slice(&[0, 0]);
    b
}
fn hex_bytes(h: Option<&V>) -> Vec<u8> {
    let s: Vec<char> = (if truthy(h) { js_string(h) } else { String::new() }).chars().collect();
    let n = s.len() >> 1;
    (0..n)
        .map(|i| {
            let p: String = s.iter().skip(i.saturating_mul(2)).take(2).collect();
            let v = parse_int16(&p);
            if v.is_nan() { 0 } else { crate::util::u8_of(crate::util::to_u32(v)) }
        })
        .collect()
}

/// 20.4.99 value
fn write_value(w: &mut BitWriter, v: Option<&V>, cx: &Cx, general_text: bool) {
    let dv = V::Obj(obj! { "flags" => 3, "dataType" => 0 });
    let v = v.filter(|x| x.truthy()).unwrap_or(&dv);
    let g = |k: &str| field(Some(v), k);
    let mut ty = int32(g("dataType"));
    let r2007 = w.r2007;
    let flags = int32(g("flags"));
    if !r2007 && (ty == 512 || ty == 128 || ty == 256) {
        ty = if truthy(g("text")) { 4 } else { 0 };
    }
    if r2007 {
        w.bl(f64::from(flags));
    }
    w.bl(f64::from(ty));
    if !(r2007 && flags & 1 != 0) {
        let bytes = |w: &mut BitWriter, b: &[u8]| {
            w.bl(usize_to_f64(b.len()));
            for &x in b {
                w.rc8(x);
            }
        };
        match ty {
            0 | 1 => w.bl(f64::from(crate::util::to_i32(num(g("value"), 0.0)))),
            2 => w.bd(num(Some(&V::Num(to_number(g("value")))), 0.0)),
            4 => {
                let s = if g("value").is_some() && matches!(g("dataType"), Some(V::Num(n)) if *n == 4.0) {
                    g("value").cloned()
                } else {
                    Some(nn(g("text")).cloned().unwrap_or(V::from("")))
                };
                if r2007 {
                    bytes(w, &utf16z(&str_or(s.as_ref(), "")));
                } else {
                    w.tv(&js_string(s.as_ref()));
                }
            }
            512 | 8 | 128 | 256 => {
                if ty == 512 && general_text && !truthy(g("bytes")) && truthy(g("text")) {
                    bytes(w, &utf16z(&js_string(g("text"))));
                } else {
                    bytes(w, &hex_bytes(g("bytes")));
                }
            }
            16 => {
                w.bl(16.0);
                w.rd(num(field(g("value"), "x"), 0.0));
                w.rd(num(field(g("value"), "y"), 0.0));
            }
            32 => {
                w.bl(24.0);
                w.rd(num(field(g("value"), "x"), 0.0));
                w.rd(num(field(g("value"), "y"), 0.0));
                w.rd(num(field(g("value"), "z"), 0.0));
            }
            64 => w.h(SOFT_PTR, cx.env.map(g("id"))),
            _ => w.bl(0.0),
        }
    }
    if r2007 {
        w.bl(f64::from(int32(g("unitType"))));
        w.tv(&str_or(g("format"), ""));
        w.tv(&str_or(g("text"), ""));
    }
}

/// 20.4.76 FIELD
fn write_field(w: &mut BitWriter, f: &Obj, kids: &[f64], objs: &[f64], cx: &Cx) {
    w.tv(&str_or(f.get("evaluatorId"), ""));
    w.tv(&str_or(f.get("code"), ""));
    w.bl(usize_to_f64(kids.len()));
    for &h in kids {
        w.h(HARD_OWN, h);
    }
    w.bl(usize_to_f64(objs.len()));
    for &h in objs {
        w.h(SOFT_PTR, h);
    }
    if !w.r2007 {
        let fm = nn(f.get("format")).or_else(|| nn(field(f.get("value"), "format")));
        w.tv(&str_or(fm, ""));
    }
    w.bl(nnum(f.get("evalOptions"), 63.0));
    w.bl(f64::from(int32(f.get("filingOptions"))));
    w.bl(nnum(f.get("state"), 13.0));
    w.bl(nnum(f.get("evalStatus"), 2.0));
    w.bl(f64::from(int32(f.get("evalError"))));
    w.tv(&str_or(f.get("evalMessage"), ""));
    write_value(w, f.get("value"), cx, false);
    let vs = str_or(f.get("valueString"), "");
    w.tv(&vs);
    w.bl(match nn(f.get("valueStringLength")) {
        Some(x) => to_number(Some(x)),
        None => usize_to_f64(crate::w_bits::u16_len(&vs)),
    });
    let cv = arr(f.get("childValues"));
    w.bl(usize_to_f64(cv.len()));
    for c in cv {
        w.tv(&str_or(field(Some(c), "key"), ""));
        write_value(w, field(Some(c), "value"), cx, false);
    }
}
/// The copier's FIELD encoder (fields of other objects, in another version than the source's).
pub fn encode_field_copy(o: &Obj, r: &Rec, cx: &mut Cx) -> Vec<u8> {
    let mut w = BitWriter::obj_writer(cx.version, false);
    let ty = cx.classes.class_num("FIELD");
    crate::w_objects::begin(&mut w, ty, r);
    object_common(&mut w, r);
    let kids: Vec<f64> = arr(o.get("children")).iter().map(|x| cx.env.map(Some(x))).filter(|h| *h != 0.0).collect();
    let objs: Vec<f64> = arr(o.get("objects")).iter().map(|x| cx.env.map(Some(x))).filter(|h| *h != 0.0).collect();
    write_field(&mut w, o, &kids, &objs, cx);
    crate::w_objects::finish(&mut w)
}

const EDGES: [(i32, &str); 6] = [(1, "top"), (2, "right"), (4, "bottom"), (8, "left"), (16, "insideV"), (32, "insideH")];
fn r_style(cx: &Cx, name: Option<&V>) -> f64 {
    if truthy(name) { cx.env.style_ref(name) } else { cx.env.style_ref(Some(&V::from("Standard"))) }
}
fn write_content_format(w: &mut BitWriter, c: Option<&V>, cx: &Cx) {
    let g = |k: &str| field(c, k);
    w.bl(f64::from(int32(g("overrides"))));
    w.bl(f64::from(int32(g("propertyFlags"))));
    w.bl(nnum(g("dataType"), 512.0));
    w.bl(f64::from(int32(g("unitType"))));
    w.tv(&str_or(g("format"), ""));
    w.bd(num(g("rotation"), 0.0));
    w.bd(num(g("blockScale"), 1.0));
    w.bl(ornum(g("align"), 1.0));
    cmc_or(w, g("color"), idx(0.0));
    w.h(HARD_PTR, if truthy(g("textStyle")) { r_style(cx, g("textStyle")) } else { 0.0 });
    w.bd(num(g("textHeight"), 0.18));
}
fn write_cell_style(w: &mut BitWriter, s: Option<&V>, cx: &Cx) {
    let g = |k: &str| field(s, k);
    w.bl(nnum(g("type"), 1.0));
    let has = truthy(s) && g("overrides").is_some();
    w.bs(if has { 1.0 } else { 0.0 });
    if !has {
        return;
    }
    w.bl(f64::from(int32(g("overrides"))));
    w.bl(f64::from(int32(g("mergeFlags"))));
    cmc_or(w, g("background"), idx(257.0));
    w.bl(nnum(g("contentLayout"), 1.0));
    let dc = V::Obj(Obj::new());
    write_content_format(w, Some(g("content").filter(|x| x.truthy()).unwrap_or(&dc)), cx);
    match g("margins").filter(|m| m.truthy()) {
        Some(m) => {
            w.bs(1.0);
            for k in ["vertical", "horizontal", "bottom", "right", "hSpacing", "vSpacing"] {
                w.bd(num(field(Some(m), k), 0.0));
            }
        }
        None => w.bs(0.0),
    }
    let borders = g("borders");
    let bs: Vec<(i32, &str)> = EDGES.iter().copied().filter(|(_, k)| truthy(borders) && truthy(field(borders, k))).collect();
    w.bl(usize_to_f64(bs.len()));
    for (e, k) in bs {
        let b = field(borders, k);
        w.bl(f64::from(e));
        w.bl(f64::from(int32(field(b, "overrides"))));
        w.bl(nnum(field(b, "type"), 1.0));
        cmc_or(w, field(b, "color"), idx(0.0));
        w.bl(f64::from(crate::util::to_i32(num(field(b, "lineweight"), -2.0))));
        w.h(HARD_PTR, if truthy(field(b, "linetype")) { cx.env.ltype_h(field(b, "linetype")) } else { 0.0 });
        w.bl(if matches!(field(b, "visible"), Some(V::Bool(false))) { 1.0 } else { 0.0 });
        w.bd(num(field(b, "doubleSpacing"), 0.045));
    }
}
fn write_named_cell_style(w: &mut BitWriter, s: &V, cx: &Cx) {
    write_cell_style(w, Some(s), cx);
    w.bl(f64::from(int32(field(Some(s), "id"))));
    w.bl(nnum(field(Some(s), "styleClass"), 2.0));
    w.tv(&str_or(field(Some(s), "name"), ""));
}

fn border_default(color: V, lw: f64, visible: bool) -> V {
    V::Obj(obj! { "overrides" => 0, "type" => 1, "color" => color, "lineweight" => lw, "visible" => visible, "doubleSpacing" => 0.045 })
}
/// a neutral table-style cell style as an R24 cell style
fn r24_cell_style(cs: &V, st: &Obj) -> V {
    let g = |k: &str| field(Some(cs), k);
    if truthy(g("cell")) && truthy(field(g("cell"), "content")) {
        let mut o = g("cell").and_then(V::as_obj).cloned().unwrap_or_default();
        o.set_opt("id", g("id").cloned());
        o.set_opt("name", g("name").cloned());
        o.set_opt("styleClass", nn(g("styleClass")).or_else(|| field(g("cell"), "styleClass")).cloned());
        return V::Obj(o);
    }
    let mut borders = Obj::new();
    for (_, k) in EDGES {
        let b = field(g("borders"), k);
        let color = field(b, "color").filter(|c| c.truthy()).cloned().unwrap_or_else(|| idx(0.0));
        borders.set(k, border_default(color, num(field(b, "lineweight"), -2.0), !matches!(field(b, "visible"), Some(V::Bool(false)))));
    }
    let hm = num(st.get("hMargin"), 0.06);
    let vm = num(st.get("vMargin"), 0.06);
    let id = g("id");
    let id_is = |n: f64| matches!(id, Some(V::Num(x)) if *x == n);
    let mut content = obj! { "overrides" => 0, "propertyFlags" => 0 };
    content.set("dataType", nn(g("dataType")).cloned().unwrap_or(V::Num(512.0)));
    content.set("unitType", int32(g("unitType")));
    content.set("format", nn(g("format")).cloned().unwrap_or(V::from("")));
    content.set("rotation", num(g("rotation"), 0.0));
    content.set("blockScale", 1);
    content
        .set("align", if truthy(g("align")) { g("align").cloned().unwrap_or(V::Null) } else { V::Num(if id_is(3.0) { 2.0 } else { 5.0 }) });
    content.set("color", g("color").filter(|c| c.truthy()).cloned().unwrap_or_else(|| idx(0.0)));
    content.set_opt("textStyle", g("textStyle").cloned());
    content.set("textHeight", num(g("textHeight"), 0.18));
    let mut o = obj! { "type" => 5, "overrides" => 0, "mergeFlags" => if id_is(1.0) { 0x8000 } else { 0 } };
    let bg = if truthy(g("backgroundEnabled")) { g("background").cloned() } else { Some(idx(257.0)) };
    o.set_opt("background", bg);
    o.set("contentLayout", 1);
    o.set("content", content);
    o.set(
        "margins",
        obj! { "vertical" => vm, "horizontal" => hm, "bottom" => vm, "right" => hm, "hSpacing" => hm * 3.0, "vSpacing" => vm * 3.0 },
    );
    o.set("borders", borders);
    o.set_opt("id", id.cloned());
    o.set_opt("name", g("name").cloned());
    o.set("styleClass", nn(g("styleClass")).cloned().unwrap_or(V::Num(if id_is(3.0) { 1.0 } else { 2.0 })));
    V::Obj(o)
}

fn enc_tablestyle(w: &mut BitWriter, r: &mut Rec, cx: &Cx) {
    let Some(TableX::Style { style: st, tables, owner_dict }) = r.tx.as_deref().cloned() else { return };
    let mut reactors = Vec::new();
    if owner_dict != 0.0 {
        reactors.push(V::Num(owner_dict));
    }
    for t in &tables {
        let h = cx.env.map(Some(t));
        if h != 0.0 {
            reactors.push(V::Num(h));
        }
    }
    r.o.set("reactors", reactors);
    if !truthy(r.g("xdic")) && truthy(st.get("extDict")) {
        let x = cx.env.map(st.get("extDict"));
        r.o.set_opt("xdic", if x != 0.0 { Some(V::Num(x)) } else { None });
    }
    object_common(w, r);
    let cells = arr(st.get("cellStyles"));
    let by_id = |id: f64| -> V {
        cells.iter().find(|c| matches!(field(Some(c), "id"), Some(V::Num(x)) if *x == id)).cloned().unwrap_or_else(|| {
            let mut o = obj! { "id" => id };
            o.set_opt("name", default_style_name(id));
            o.set("textHeight", if id == 1.0 { 0.25 } else { 0.18 });
            o.set("align", if id == 3.0 { 2.0 } else { 5.0 });
            V::Obj(o)
        })
    };
    if w.r2010 {
        let unk = st.get("unknown");
        w.rc(f64::from(int32(field(unk, "rc"))));
        w.tv(&str_or(st.get("description"), ""));
        w.bl(nnum(st.get("flags"), 8.0));
        w.bl(nnum(field(unk, "bl2"), 101.0));
        w.h(HARD_OWN, 0.0);
        let tcs = match st.get("tableCellStyle").filter(|x| x.truthy()) {
            Some(t) => t.clone(),
            None => {
                let mut borders = Obj::new();
                for (_, k) in EDGES {
                    borders.set(k, border_default(idx(0.0), -2.0, true));
                }
                let vm = num(st.get("vMargin"), 0.06);
                let hm = num(st.get("hMargin"), 0.06);
                V::Obj(obj! {
                    "type" => 5, "overrides" => 0, "mergeFlags" => if truthy(st.get("flowDirection")) { 0x10000 } else { 0 },
                    "background" => idx(257.0), "contentLayout" => 1,
                    "content" => obj! { "overrides" => 0, "propertyFlags" => 0, "dataType" => 4, "unitType" => 0, "format" => "", "rotation" => 0, "blockScale" => 1, "align" => 1, "color" => idx(0.0), "textHeight" => 0.18 },
                    "margins" => obj! { "vertical" => vm, "horizontal" => hm, "bottom" => vm, "right" => hm, "hSpacing" => 0.18, "vSpacing" => 0.18 },
                    "borders" => borders,
                })
            }
        };
        let mut named = tcs.as_obj().cloned().unwrap_or_default();
        named.set("id", 4);
        named.set("styleClass", 2);
        named.set("name", "Table");
        write_named_cell_style(w, &V::Obj(named), cx);
        let mut list: Vec<V> = vec![by_id(1.0), by_id(2.0), by_id(3.0)];
        list.extend(cells.iter().filter(|c| to_number(field(Some(c), "id")) > 4.0).cloned());
        let list: Vec<V> = list.iter().map(|c| r24_cell_style(c, &st)).collect();
        w.bl(usize_to_f64(list.len()));
        for s in &list {
            w.bl(to_number(field(Some(s), "id")));
            write_named_cell_style(w, s, cx);
        }
        return;
    }
    w.tv(&str_or(st.get("description"), ""));
    w.bs(f64::from(int32(st.get("flowDirection"))));
    w.bs(if truthy(st.get("tableCellStyle")) { 0.0 } else { f64::from(int32(st.get("flags")) & 0x7FFF) });
    w.bd(num(st.get("hMargin"), 0.06));
    w.bd(num(st.get("vMargin"), 0.06));
    w.b(truthy(st.get("titleSuppressed")));
    w.b(truthy(st.get("headerSuppressed")));
    for id in [3.0, 1.0, 2.0] {
        let cs = by_id(id);
        let g = |k: &str| field(Some(&cs), k);
        w.h(HARD_PTR, r_style(cx, g("textStyle")));
        w.bd(num(g("textHeight"), 0.18));
        w.bs(ornum(g("align"), 5.0));
        cmc_or(w, g("color"), idx(0.0));
        cmc_or(w, g("background"), idx(257.0));
        w.b(truthy(g("backgroundEnabled")));
        for k in ["top", "insideH", "bottom", "left", "insideV", "right"] {
            let b = field(g("borders"), k);
            w.bs(num(field(b, "lineweight"), -2.0));
            w.b(!matches!(field(b, "visible"), Some(V::Bool(false))));
            cmc_or(w, field(b, "color"), idx(0.0));
        }
        if w.r2007 {
            w.bl(nnum(g("dataType"), 512.0));
            w.bl(f64::from(int32(g("unitType"))));
            w.tv(&str_or(g("format"), ""));
        }
    }
}

fn side_bit(s: &str) -> Option<u32> {
    match s {
        "top" => Some(0),
        "right" => Some(1),
        "bottom" => Some(2),
        "left" => Some(3),
        _ => None,
    }
}
#[derive(Clone, Debug, Default)]
struct Edge {
    flags: i32,
    virt: i32,
    vprops: Obj,
}
/// Border overrides per cell edge (see table-write.js edgeMap).
fn edge_map(t: &Obj) -> std::collections::HashMap<(usize, usize), Edge> {
    let rows = arr(t.get("rows")).len();
    let cols = arr(t.get("columns")).len();
    let cells = arr(t.get("cells"));
    let mut m: std::collections::HashMap<(usize, usize), Edge> = std::collections::HashMap::new();
    for i in 0..rows {
        for j in 0..cols {
            let Some(c) = cells.get(i).and_then(|r| r.as_arr()).and_then(|r| r.get(j)).filter(|c| c.truthy()) else { continue };
            let c = Some(c);
            {
                let e = m.entry((i, j)).or_default();
                e.flags |= int32(field(field(c, "edges"), "flags"));
                e.virt |= int32(field(field(c, "edges"), "virtual"));
            }
            let borders = field(field(c, "overrides"), "borders").and_then(V::as_obj).cloned().unwrap_or_default();
            for (side, b) in crate::w_js::js_entries(&borders) {
                let Some(k) = side_bit(side) else { continue };
                if !b.truthy() {
                    continue;
                }
                {
                    let e = m.entry((i, j)).or_default();
                    e.flags |= 1 << k;
                    e.virt &= !(1 << k);
                }
                let (os, di, dj): (&str, i64, i64) = match side {
                    "top" => ("bottom", -1, 0),
                    "right" => ("left", 0, 1),
                    "bottom" => ("top", 1, 0),
                    _ => ("right", 0, -1),
                };
                let ni = i64::try_from(i).unwrap_or(0).saturating_add(di);
                let nj = i64::try_from(j).unwrap_or(0).saturating_add(dj);
                if ni < 0 || nj < 0 || ni >= i64::try_from(rows).unwrap_or(0) || nj >= i64::try_from(cols).unwrap_or(0) {
                    continue;
                }
                let (ni, nj) = (usize::try_from(ni).unwrap_or(0), usize::try_from(nj).unwrap_or(0));
                let ok = side_bit(os).unwrap_or(0);
                let n = m.entry((ni, nj)).or_default();
                n.flags |= 1 << ok;
                n.virt |= 1 << ok;
                n.vprops.set(os, b.clone());
            }
        }
    }
    m
}
fn is_text(c: &V) -> bool {
    !matches!(field(Some(c), "type"), Some(V::Str(s)) if s == "block")
}
/// the value a text cell stores (R2007+): its own, or a string built from its text
fn cell_value(c: &V) -> V {
    let d = field(Some(c), "data");
    if truthy(d) && int32(field(d, "flags")) & 1 == 0 && field(d, "dataType").is_some() {
        let mut v = d.and_then(V::as_obj).cloned().unwrap_or_default();
        let cv = nn(field(Some(c), "value")).cloned();
        if matches!(v.get("dataType"), Some(V::Num(n)) if *n == 4.0) {
            let x = cv.or_else(|| v.get("value").cloned());
            v.set_opt("value", x);
        } else if matches!(v.get("dataType"), Some(V::Num(n)) if *n == 512.0) {
            let x = cv.or_else(|| v.get("text").cloned());
            v.set_opt("text", x);
        }
        return V::Obj(v);
    }
    match field(Some(c), "value") {
        None => V::Obj(obj! { "flags" => 3, "dataType" => 0 }),
        Some(V::Str(s)) if s.is_empty() => V::Obj(obj! { "flags" => 3, "dataType" => 0 }),
        x => V::Obj(obj! { "flags" => 0, "dataType" => 4, "value" => js_string(x) }),
    }
}
fn cell_field_h(c: &V) -> f64 {
    if is_text(c) && truthy(field(Some(c), "field")) { num(field(Some(c), FIELD_KEY), 0.0) } else { 0.0 }
}
fn cell_block_h(c: &V, cx: &Cx) -> f64 {
    let n = str_or(field(Some(c), "block"), "").to_uppercase();
    match cx.env.block_by_name.get(&n).and_then(|&i| cx.env.block_hdr.get(i)) {
        Some(h) => *h,
        None => cx.env.map(field(Some(c), "blockRecord")),
    }
}

fn insert_part(w: &mut BitWriter, r: &Rec) {
    w.p3(r.g("insert"));
    insert_scale(w, r.g("scale"));
    w.bd(num(r.g("rotation"), 0.0));
    let e = r.g("extrusion").filter(|x| x.truthy());
    match e {
        Some(e) => w.bd3(num(field(Some(e), "x"), 0.0), num(field(Some(e), "y"), 0.0), num(field(Some(e), "z"), 1.0)),
        None => w.bd3(0.0, 0.0, 1.0),
    }
    w.b(false);
    w.hv(HARD_PTR, r.g("block"));
}
fn horiz_dir(w: &mut BitWriter, r: &Rec) {
    match r.g("horizDir").filter(|x| x.truthy()) {
        Some(h) => w.p3(Some(h)),
        None => w.p3n(1.0, 0.0, 0.0),
    }
}
fn text_cell() -> V {
    V::Obj(obj! { "type" => "text", "value" => "" })
}

fn old_table(w: &mut BitWriter, r: &Rec, t: &Obj, style_h: f64, cx: &Cx) {
    let rows = arr(t.get("rows"));
    let cols = arr(t.get("columns"));
    let (nr, nc) = (rows.len(), cols.len());
    match nn(t.get("flags")) {
        Some(f) => w.bs(to_number(Some(f))),
        None => w.bs(f64::from(
            6 | if truthy(t.get("flowDirection")) { 0 } else { 0x10 } | if truthy(t.get("titleSuppressed")) { 0x20 } else { 0 },
        )),
    }
    horiz_dir(w, r);
    w.bl(usize_to_f64(nc));
    w.bl(usize_to_f64(nr));
    for c in cols {
        w.bd(num(field(Some(c), "width"), 1.0));
    }
    for x in rows {
        w.bd(num(field(Some(x), "height"), 0.25));
    }
    w.h(HARD_PTR, style_h);
    let merged = arr(t.get("merged"));
    let key = |a: &V| -> (f64, f64) { (to_number(field(Some(a), "top")), to_number(field(Some(a), "left"))) };
    let mut covered: HashSet<(i64, i64)> = HashSet::new();
    for m in merged {
        let (top, left) = key(m);
        let (bottom, right) = (to_number(field(Some(m), "bottom")), to_number(field(Some(m), "right")));
        let mut i = top;
        let mut guard = 0u32;
        while i <= bottom && guard < 100_000 {
            let mut j = left;
            while j <= right && guard < 100_000 {
                if i != top || j != left {
                    covered.insert((crate::util::to_i64(i), crate::util::to_i64(j)));
                }
                j += 1.0;
                guard = guard.saturating_add(1);
            }
            i += 1.0;
        }
    }
    let em = edge_map(t);
    let cells = arr(t.get("cells"));
    let dc = text_cell();
    for i in 0..nr {
        for j in 0..nc {
            let c = cells.get(i).and_then(|row| row.as_arr()).and_then(|row| row.get(j)).filter(|c| c.truthy()).unwrap_or(&dc);
            let (fi, fj) = (usize_to_f64(i), usize_to_f64(j));
            let m = merged.iter().rev().find(|m| key(m) == (fi, fj));
            let ed = em.get(&(i, j)).cloned().unwrap_or_default();
            let g = |k: &str| field(Some(c), k);
            w.bs(if is_text(c) { 1.0 } else { 2.0 });
            w.rc(f64::from(ed.flags));
            let cov = covered.contains(&(crate::util::to_i64(fi), crate::util::to_i64(fj)));
            w.b(cov || truthy(g("covered")));
            w.b(truthy(g("autoFit")));
            match m {
                Some(m) => {
                    w.bl(to_number(field(Some(m), "right")) - to_number(field(Some(m), "left")) + 1.0);
                    w.bl(to_number(field(Some(m), "bottom")) - to_number(field(Some(m), "top")) + 1.0);
                }
                None => {
                    w.bl(1.0);
                    w.bl(1.0);
                }
            }
            w.bd(num(g("rotation"), 0.0));
            let field_h = cell_field_h(c);
            if is_text(c) {
                w.h(HARD_PTR, field_h);
                if field_h == 0.0 && !w.r2007 {
                    w.tv(&str_or(g("value"), ""));
                }
            } else {
                w.h(HARD_PTR, cell_block_h(c, cx));
                w.bd(num(g("blockScale"), 1.0));
                let at: Vec<(&V, f64)> =
                    arr(g("attrs")).iter().map(|a| (a, cx.env.map(field(Some(a), "attdef")))).filter(|x| x.1 != 0.0).collect();
                w.b(!at.is_empty());
                if !at.is_empty() {
                    w.bs(usize_to_f64(at.len()));
                    for (a, h) in at {
                        w.h(SOFT_PTR, h);
                        w.bs(f64::from(int32(field(Some(a), "index"))));
                        w.tv(&str_or(field(Some(a), "value"), ""));
                    }
                }
            }
            let ov = g("overrides").filter(|x| x.truthy());
            let og = |k: &str| field(ov, k);
            let mut fl: i32 = 0;
            if og("align").is_some() {
                fl |= 0x01;
            }
            if og("fillNone").is_some() {
                fl |= 0x02;
            }
            if truthy(og("background")) {
                fl |= 0x04;
            }
            if truthy(og("color")) {
                fl |= 0x08;
            }
            if og("textStyle").is_some() {
                fl |= 0x10;
            }
            if og("textHeight").is_some() {
                fl |= 0x20;
            }
            let mut bo = ed.vprops.clone();
            if let Some(b) = og("borders").and_then(V::as_obj) {
                for (k, v) in b.iter() {
                    bo.set(k.to_string(), v.clone());
                }
            }
            for (side, k) in [("top", 0), ("right", 1), ("bottom", 2), ("left", 3)] {
                let Some(b) = bo.get(side).filter(|b| b.truthy()) else { continue };
                if truthy(field(Some(b), "color")) {
                    fl |= 0x40 << k;
                }
                if field(Some(b), "lineweight").is_some() {
                    fl |= 0x400 << k;
                }
                if field(Some(b), "visible").is_some() {
                    fl |= 0x4000 << k;
                }
            }
            // Spec gap: R2007+ sets 0x40000 on a text cell that holds a value
            let val = if is_text(c) && field_h == 0.0 { cell_value(c) } else { V::Obj(obj! { "flags" => 3, "dataType" => 0 }) };
            if w.r2007 && is_text(c) && field_h == 0.0 && int32(field(Some(&val), "flags")) & 1 == 0 {
                fl |= 0x40000;
            }
            w.b(fl != 0 || ed.virt != 0);
            if fl != 0 || ed.virt != 0 {
                w.bl(f64::from(fl));
                w.rc(f64::from(ed.virt));
                if fl & 0x01 != 0 {
                    w.bs(f64::from(int32(og("align"))));
                }
                if fl & 0x02 != 0 {
                    w.b(truthy(og("fillNone")));
                }
                if fl & 0x04 != 0 {
                    cmc(w, og("background"));
                }
                if fl & 0x08 != 0 {
                    cmc(w, og("color"));
                }
                if fl & 0x10 != 0 {
                    w.h(HARD_PTR, r_style(cx, og("textStyle")));
                }
                if fl & 0x20 != 0 {
                    w.bd(num(og("textHeight"), 0.18));
                }
                for (side, k) in [("top", 0), ("right", 1), ("bottom", 2), ("left", 3)] {
                    let Some(b) = bo.get(side).filter(|b| b.truthy()) else { continue };
                    if ed.virt & (1 << k) != 0 {
                        continue;
                    }
                    if fl & (0x40 << k) != 0 {
                        cmc(w, field(Some(b), "color"));
                    }
                    if fl & (0x400 << k) != 0 {
                        w.bs(num(field(Some(b), "lineweight"), -2.0));
                    }
                    if fl & (0x4000 << k) != 0 {
                        w.bs(if matches!(field(Some(b), "visible"), Some(V::Bool(false))) { 1.0 } else { 0.0 });
                    }
                }
            }
            if w.r2007 {
                w.bl(0.0);
                write_value(w, Some(&val), cx, true);
            }
        }
    }
    // table-wide overrides (as read from an R13–R21 table; others write none)
    let o = t.get("overrides").filter(|x| x.truthy());
    let f = if o.is_some() { int32(t.get("overrideFlags")) } else { 0 };
    w.b(f != 0);
    if f != 0 {
        let og = |k: &str| field(o, k);
        w.bl(f64::from(f));
        if f & 1 != 0 {
            w.b(truthy(og("titleSuppressed")));
        }
        if f & 4 != 0 {
            w.bs(f64::from(int32(og("flowDirection"))));
        }
        if f & 8 != 0 {
            w.bd(num(og("hMargin"), 0.0));
        }
        if f & 0x10 != 0 {
            w.bd(num(og("vMargin"), 0.0));
        }
        let rows = ["title", "header", "data"];
        for (i, n) in rows.iter().enumerate() {
            if f & (0x20 << i) != 0 {
                cmc(w, og(&format!("{n}Color")));
            }
        }
        for (i, n) in rows.iter().enumerate() {
            if f & (0x100 << i) != 0 {
                w.b(truthy(og(&format!("{n}FillNone"))));
            }
        }
        for (i, n) in rows.iter().enumerate() {
            if f & (0x800 << i) != 0 {
                cmc(w, og(&format!("{n}Fill")));
            }
        }
        for (i, n) in rows.iter().enumerate() {
            if f & (0x4000 << i) != 0 {
                w.bs(f64::from(int32(og(&format!("{n}Align")))));
            }
        }
        for (i, n) in rows.iter().enumerate() {
            if f & (0x20000 << i) != 0 {
                w.h(HARD_PTR, r_style(cx, og(&format!("{n}Style"))));
            }
        }
        for (i, n) in rows.iter().enumerate() {
            if f & (0x100000 << i) != 0 {
                w.bd(num(og(&format!("{n}Height")), 0.0));
            }
        }
    }
    for k in ["borderColors", "borderLineweights", "borderVisibility"] {
        let b = t.get(k).filter(|x| x.truthy());
        w.b(b.is_some());
        if let Some(b) = b {
            let fl = int32(field(Some(b), "flags"));
            w.bl(f64::from(fl));
            for i in 0..18 {
                if fl & (1 << i) != 0 {
                    let v = field(Some(b), &i.to_string()).or_else(|| b.as_arr().and_then(|a| a.get(usize::try_from(i).unwrap_or(0))));
                    match k {
                        "borderColors" => cmc(w, v),
                        "borderLineweights" => w.bs(num(v, -2.0)),
                        _ => w.bs(f64::from(int32(v))),
                    }
                }
            }
        }
    }
}

/// R24 cell style of a cell: its own (if the source had one) with the model's overrides applied
fn cell_style_for(c: &V, vprops: &Obj) -> V {
    let ov0 = field(Some(c), "overrides").filter(|x| x.truthy()).and_then(V::as_obj).cloned().unwrap_or_default();
    let ov = if !vprops.is_empty() {
        let mut o = ov0.clone();
        let mut b = vprops.clone();
        if let Some(ob) = ov0.obj("borders") {
            for (k, v) in ob.iter() {
                b.set(k.to_string(), v.clone());
            }
        }
        o.set("borders", b);
        o
    } else {
        ov0
    };
    let cs = field(Some(c), "cellStyle");
    let base = if truthy(cs) && field(cs, "overrides").is_some() { cs.and_then(V::as_obj).cloned() } else { None };
    let rot = num(field(Some(c), "rotation"), 0.0);
    if base.is_none() && ov.is_empty() && rot == 0.0 {
        return V::Obj(obj! { "type" => 1 });
    }
    let had_base = base.is_some();
    let mut s = base.unwrap_or_else(|| {
        let align = if truthy(field(Some(c), "align")) { field(Some(c), "align").cloned().unwrap_or(V::Null) } else { V::Num(1.0) };
        obj! {
            "type" => 1, "overrides" => 0, "mergeFlags" => 0, "background" => idx(257.0), "contentLayout" => 1,
            "content" => obj! { "overrides" => 0, "propertyFlags" => 0, "dataType" => 512, "unitType" => 0, "format" => "", "rotation" => 0, "blockScale" => 1, "align" => align, "color" => idx(0.0), "textHeight" => 0.18 },
            "borders" => Obj::new(),
        }
    });
    let _ = had_base;
    let s_ov = int32(s.get("overrides"));
    let mut fl = s_ov & !(0x4 | 0x10 | 0x20 | 0x40 | 0x80 | 0x200);
    let mut ct = s.obj("content").cloned().unwrap_or_default();
    if let Some(a) = ov.get("align") {
        fl |= 0x10;
        ct.set("align", a.clone());
    }
    if truthy(ov.get("color")) {
        fl |= 0x20;
        ct.set_opt("color", ov.get("color").cloned());
    }
    if let Some(t) = ov.get("textStyle") {
        fl |= 0x40;
        ct.set("textStyle", t.clone());
    }
    if let Some(t) = ov.get("textHeight") {
        fl |= 0x80;
        ct.set("textHeight", t.clone());
    }
    if truthy(ov.get("background")) || truthy(ov.get("fillNone")) {
        fl |= 0x200;
        let bg = if truthy(ov.get("fillNone")) { Some(idx(257.0)) } else { ov.get("background").cloned() };
        s.set_opt("background", bg);
    }
    if rot != 0.0 {
        fl |= 0x4;
        ct.set("rotation", rot);
    } else if s_ov & 0x4 == 0 {
        ct.set("rotation", 0);
    }
    ct.set("overrides", fl & 0x1FF);
    s.set("content", ct);
    s.set("overrides", fl);
    if let Some(ob) = ov.get("borders").filter(|x| x.truthy()).and_then(V::as_obj) {
        let mut borders = s.obj("borders").cloned().unwrap_or_default();
        for (side, b) in crate::w_js::js_entries(ob) {
            let mut x =
                borders.obj(side).cloned().unwrap_or_else(|| border_default(idx(0.0), -2.0, true).as_obj().cloned().unwrap_or_default());
            x.set("overrides", int32(x.get("overrides")) | 26);
            if truthy(field(Some(b), "color")) {
                x.set_opt("color", field(Some(b), "color").cloned());
            }
            if let Some(lw) = field(Some(b), "lineweight") {
                x.set("lineweight", lw.clone());
            }
            if let Some(v) = field(Some(b), "visible") {
                x.set("visible", v.truthy());
            }
            borders.set(side.to_string(), x);
        }
        s.set("borders", borders);
    }
    V::Obj(s)
}

fn new_table(w: &mut BitWriter, r: &Rec, t: &Obj, style_h: f64, ids: &[(String, f64)], cx: &Cx) {
    let rows = arr(t.get("rows"));
    let cols = arr(t.get("columns"));
    let (nr, nc) = (rows.len(), cols.len());
    let em = edge_map(t);
    let sid = |name: Option<&V>| -> f64 {
        let n = str_or(name, "").to_uppercase();
        ids.iter().find(|x| x.0 == n).map_or(0.0, |x| x.1)
    };
    w.rc8(0);
    w.h(SOFT_PTR, 0.0);
    w.bl(0.0);
    if w.r2013 {
        w.bl(0.0);
    } else {
        w.b(true);
    }
    w.tv(&str_or(t.get("name"), ""));
    w.tv(&str_or(t.get("description"), ""));
    w.bl(usize_to_f64(nc));
    for col in cols {
        let g = |k: &str| field(Some(col), k);
        w.tv(&str_or(g("name"), ""));
        w.bl(0.0);
        w.bl(0.0);
        let d = V::Obj(obj! { "type" => 3 });
        write_cell_style(w, Some(g("cellStyle").filter(|x| x.truthy()).unwrap_or(&d)), cx);
        w.bl(if truthy(g("style")) { sid(g("style")) } else { 0.0 });
        w.bd(num(g("width"), 1.0));
    }
    w.bl(usize_to_f64(nr));
    let mut fields = Vec::new();
    let cells = arr(t.get("cells"));
    let dc = text_cell();
    for (i, row) in rows.iter().enumerate() {
        w.bl(usize_to_f64(nc));
        for j in 0..nc {
            let c = cells.get(i).and_then(|row| row.as_arr()).and_then(|row| row.get(j)).filter(|c| c.truthy()).unwrap_or(&dc);
            let g = |k: &str| field(Some(c), k);
            w.bl(f64::from(int32(g("state"))));
            w.tv(&str_or(g("tooltip"), ""));
            w.bl(0.0);
            w.bl(0.0);
            w.bl(0.0);
            let field_h = cell_field_h(c);
            let mut cf = obj! { "overrides" => 0, "propertyFlags" => 0, "dataType" => 512, "unitType" => 0, "format" => "" };
            cf.set("rotation", num(g("rotation"), 0.0));
            cf.set("blockScale", num(g("blockScale"), 1.0));
            cf.set("align", if truthy(g("align")) { g("align").cloned().unwrap_or(V::Null) } else { V::Num(1.0) });
            cf.set("color", idx(0.0));
            cf.set("textHeight", 0.18);
            if let Some(cfo) = g("contentFormat").filter(|x| x.truthy()).and_then(V::as_obj) {
                for (k, v) in cfo.iter() {
                    cf.set(k.to_string(), v.clone());
                }
            }
            cf.set("rotation", num(g("rotation"), 0.0));
            if matches!(g("type"), Some(V::Str(s)) if s == "block") {
                cf.set("blockScale", num(g("blockScale"), 1.0));
            }
            if num(cf.get("rotation"), 0.0) != 0.0 {
                let o = int32(cf.get("overrides")) | 4;
                cf.set("overrides", o);
            }
            if !is_text(c) {
                w.bl(1.0);
                w.bl(4.0);
                w.h(HARD_PTR, cell_block_h(c, cx));
                let at: Vec<(&V, f64)> =
                    arr(g("attrs")).iter().map(|a| (a, cx.env.map(field(Some(a), "attdef")))).filter(|x| x.1 != 0.0).collect();
                w.bl(usize_to_f64(at.len()));
                for (a, h) in at {
                    w.h(SOFT_PTR, h);
                    w.tv(&str_or(field(Some(a), "value"), ""));
                    w.bl(f64::from(int32(field(Some(a), "index"))));
                }
                w.bs(1.0);
                write_content_format(w, Some(&V::Obj(cf)), cx);
            } else if field_h != 0.0 {
                fields.push(field_h);
                w.bl(1.0);
                w.bl(2.0);
                w.h(HARD_PTR, field_h);
                w.bl(0.0);
                w.bs(1.0);
                write_content_format(w, Some(&V::Obj(cf)), cx);
            } else {
                let v = cell_value(c);
                let has_cf = truthy(g("contentFormat"));
                if int32(field(Some(&v), "flags")) & 1 != 0 && !has_cf {
                    w.bl(0.0);
                } else {
                    if !has_cf {
                        cf.set("overrides", 1);
                        cf.set_opt("dataType", field(Some(&v), "dataType").cloned());
                    }
                    w.bl(1.0);
                    w.bl(1.0);
                    write_value(w, Some(&v), cx, false);
                    w.bl(0.0);
                    w.bs(1.0);
                    write_content_format(w, Some(&V::Obj(cf)), cx);
                }
            }
            let vp = em.get(&(i, j)).map(|e| e.vprops.clone()).unwrap_or_default();
            write_cell_style(w, Some(&cell_style_for(c, &vp)), cx);
            let cs = g("style");
            let rs = field(Some(row), "style");
            w.bl(if truthy(cs) && cs != rs { sid(cs) } else { 0.0 });
            w.bl(0.0);
        }
        w.bl(0.0);
        w.bl(0.0);
        let d = V::Obj(obj! { "type" => 2 });
        write_cell_style(w, Some(field(Some(row), "cellStyle").filter(|x| x.truthy()).unwrap_or(&d)), cx);
        w.bl(sid(field(Some(row), "style")));
        w.bd(num(field(Some(row), "height"), 0.25));
    }
    w.bl(usize_to_f64(fields.len()));
    for h in fields {
        w.h(HARD_OWN, h);
    }
    let d = V::Obj(obj! { "type" => 4 });
    write_cell_style(w, Some(t.get("tableCellStyle").filter(|x| x.truthy()).unwrap_or(&d)), cx);
    let merged = arr(t.get("merged"));
    w.bl(usize_to_f64(merged.len()));
    for m in merged {
        for k in ["top", "left", "bottom", "right"] {
            w.bl(to_number(field(Some(m), k)));
        }
    }
    w.h(HARD_PTR, style_h);
    // the table entity part after the content
    w.bs(nnum(t.get("tableBits"), 38.0));
    horiz_dir(w, r);
    let br = t.get("breaks").filter(|x| x.truthy());
    w.bl(if br.is_some() { 1.0 } else { 0.0 });
    if let Some(br) = br {
        let g = |k: &str| field(Some(br), k);
        w.bl(f64::from(int32(g("options"))));
        w.bl(nnum(g("flow"), 1.0));
        w.bd(num(g("spacing"), 0.0));
        w.bl(f64::from(int32(g("unknown1"))));
        w.bl(f64::from(int32(g("unknown2"))));
        let hs = arr(g("heights"));
        w.bl(usize_to_f64(hs.len()));
        for x in hs {
            w.p3(field(Some(x), "position"));
            w.bd(num(field(Some(x), "height"), 0.0));
            w.bl(f64::from(int32(field(Some(x), "flags"))));
        }
    }
    let rr = arr(t.get("rowRanges"));
    let ranges: Vec<V> = if !rr.is_empty() {
        rr.to_vec()
    } else {
        vec![V::Obj(obj! { "position" => crate::value::p3(0.0, 0.0, 0.0), "start" => 0, "end" => usize_to_f64(nr) - 1.0 })]
    };
    w.bl(usize_to_f64(ranges.len()));
    for x in &ranges {
        w.p3(field(Some(x), "position"));
        w.bl(f64::from(int32(field(Some(x), "start"))));
        w.bl(f64::from(int32(field(Some(x), "end"))));
    }
}

/// ENC.FIELD / ENC.TABLESTYLE / ENC.ACAD_TABLE
pub fn enc_table(w: &mut BitWriter, r: &mut Rec, cx: &mut Cx) -> bool {
    match r.t() {
        "FIELD" => {
            let Some(TableX::Field { field: f, kids, parent, src_owner, table_h }) = r.tx.as_deref().cloned() else { return true };
            if parent != 0.0 {
                r.o.set("owner", parent);
                r.o.set("reactors", vec![V::Num(parent)]);
            } else {
                let o = if src_owner.is_some() { cx.env.map(src_owner.as_ref()) } else { 0.0 };
                r.o.set("owner", if o != 0.0 { o } else { table_h });
                r.o.set("reactors", if o != 0.0 { vec![V::Num(o)] } else { Vec::new() });
            }
            object_common(w, r);
            let objs: Vec<f64> = arr(f.get("objects")).iter().map(|x| cx.env.map(Some(x))).filter(|h| *h != 0.0).collect();
            write_field(w, &f, &kids, &objs, cx);
        }
        "TABLESTYLE" => enc_tablestyle(w, r, cx),
        "ACAD_TABLE" => {
            let Some(TableX::Table { style_h, ids, table }) = r.tx.as_deref().cloned() else { return false };
            let bh = match r.block_ref {
                Some(Some(i)) => cx.env.block_hdr.get(i).copied().unwrap_or(0.0),
                _ => 0.0,
            };
            r.o.set("block", bh);
            entity_common(w, r);
            insert_part(w, r);
            if w.r2010 {
                new_table(w, r, &table, style_h, &ids, cx);
            } else {
                old_table(w, r, &table, style_h, cx);
            }
        }
        _ => return false,
    }
    true
}
