//! MULTILEADER and MLEADERSTYLE encoders (port of lib/dwg/mleader-write.js; ODA spec 20.4.48, 20.4.86, 20.4.87).
//! Every field has a default so a program can build a multileader from a few points and a text.
//! Spec gap: up to R2004 the multileader's strings carry no closing NUL (the length is the character count).
//! Spec gap: R2013+ MLEADERSTYLE has one more bit at the end (DXF 298).

use crate::obj;
use crate::util::usize_to_f64;
use crate::value::{Obj, V};
use crate::w_bits::{BitWriter, encode1252};
use crate::w_js::{arr, field, hex_key, int32, js_string, nn, num, str_or, to_number, truthy};
use crate::w_objects::{Cx, Enc, HARD_PTR, Rec, SOFT_PTR, begin, finish, nnum, object_common};
use crate::w_passthrough::raw_entity_common;
use crate::w_write::{Conv, W};

/// A multileader record's resolvers (rec.R): the blocks found by name at convert time, the style once
/// ensureMleaderStyle gave it one.
#[derive(Clone, Debug)]
pub struct MlRec {
    pub src: Obj,
    pub arrow_b: Option<usize>,
    pub content_b: Option<usize>,
    pub style_override: Option<f64>,
}
/// A MLEADERSTYLE (copied: refs through the written handles; made by ensureMleaderStyle: `defaults`).
#[derive(Clone, Debug)]
pub struct MlsRec {
    pub src: Obj,
    pub defaults: Option<(f64, f64)>,
}

fn col(c: Option<&V>, d: V) -> V {
    match nn(c) {
        None => d,
        Some(V::Num(n)) => V::Obj(obj! { "index" => *n }),
        Some(x) => x.clone(),
    }
}
fn idx(i: f64) -> V {
    V::Obj(obj! { "index" => i })
}
fn pt(w: &mut BitWriter, p: Option<&V>, d: (f64, f64, f64)) {
    if truthy(p) {
        w.bd3(num(field(p, "x"), 0.0), num(field(p, "y"), 0.0), num(field(p, "z"), 0.0));
    } else {
        w.bd3(d.0, d.1, d.2);
    }
}
const O: (f64, f64, f64) = (0.0, 0.0, 0.0);
const Z: (f64, f64, f64) = (0.0, 0.0, 1.0);
const X1: (f64, f64, f64) = (1.0, 0.0, 0.0);
const Y1: (f64, f64, f64) = (0.0, 1.0, 0.0);
const ONE: (f64, f64, f64) = (1.0, 1.0, 1.0);
fn txt(w: &mut BitWriter, s: &str) {
    if w.r2007 {
        w.tv(s);
        return;
    }
    let b = encode1252(s);
    w.bs(usize_to_f64(b.len()));
    for c in b {
        w.rc8(c);
    }
}

impl W<'_> {
    /// convertMleader(e, env, c)
    pub fn convert_mleader(&mut self, e: &Obj, owner: f64) -> Conv {
        let ctx = e.get("context");
        if !truthy(ctx) || field(ctx, "roots").and_then(V::as_arr).is_none() {
            return Conv::Skip("MULTILEADER without its leaders and content".into());
        }
        let block_of = |name: Option<&V>| -> Option<usize> {
            if truthy(name) { self.block_by_name.get(&js_string(name).to_uppercase()).copied() } else { None }
        };
        let arrow_b = block_of(e.get("arrowheadName"));
        let cname = nn(field(field(ctx, "block"), "name")).or_else(|| e.get("contentBlock"));
        let content_b = block_of(cname);
        let mut r = self.common(e, "MULTILEADER", owner);
        r.enc = Enc::Mleader(Box::new(MlRec { src: e.clone(), arrow_b, content_b, style_override: None }));
        Conv::Recs(vec![r])
    }

    /// ensureMleaderStyle: multileaders whose style is not written get a Standard one (in ACAD_MLEADERSTYLE).
    pub fn ensure_mleader_style(&mut self, text_style: f64, lt_by_block: f64) {
        let mut mls: Vec<usize> = Vec::new();
        for (i, r) in self.recs.iter().enumerate() {
            if r.t() != "MULTILEADER" {
                continue;
            }
            if let Enc::Mleader(m) = &r.enc
                && ml_style(m, &self.src_map) == 0.0
            {
                mls.push(i);
            }
        }
        if mls.is_empty() {
            return;
        }
        let nod_h = self.h.nod;
        let nod_i = self.recs.iter().position(|r| r.h() == nod_h && r.t() == "DICTIONARY");
        let have = nod_i
            .and_then(|i| self.recs.get(i))
            .and_then(|n| arr(n.g("items")).iter().find(|x| js_string(x.as_arr().and_then(|a| a.first())) == "ACAD_MLEADERSTYLE").cloned());
        let mut dict_i = have.and_then(|x| {
            let h = num(x.as_arr().and_then(|a| a.get(1)), f64::NAN);
            self.recs.iter().position(|r| r.h() == h)
        });
        let style_h = self.next();
        if dict_i.is_none() {
            let dh = self.next();
            self.recs.push(Rec::new(obj! { "t" => "DICTIONARY", "h" => dh, "owner" => nod_h, "reactors" => vec![V::Num(nod_h)], "items" => Vec::<V>::new(), "cloning" => 1 }));
            dict_i = Some(self.recs.len().saturating_sub(1));
            if let Some(n) = nod_i.and_then(|i| self.recs.get_mut(i)) {
                let mut items = arr(n.g("items")).to_vec();
                items.push(V::Arr(vec![V::from("ACAD_MLEADERSTYLE"), V::Num(dh)]));
                crate::w_nongraphic::sort_items(&mut items);
                n.o.set("items", items);
            }
        }
        let Some(di) = dict_i else { return };
        let (dict_h, name) = {
            let Some(d) = self.recs.get_mut(di) else { return };
            let mut items = arr(d.g("items")).to_vec();
            let mut name = "Standard".to_string();
            let mut k = 2u32;
            while items.iter().any(|x| js_string(x.as_arr().and_then(|a| a.first())) == name) {
                name = format!("Standard_{k}");
                k = k.saturating_add(1);
            }
            items.push(V::Arr(vec![V::from(name.as_str()), V::Num(style_h)]));
            d.o.set("items", items);
            (d.h(), name)
        };
        let mut o = obj! { "t" => "MLEADERSTYLE", "h" => style_h, "owner" => dict_h, "reactors" => vec![V::Num(dict_h)] };
        o.set_opt("eed", self.x.mleader_eed.clone());
        o.set("newFormat", true);
        let mut r = Rec::new(o);
        r.enc = Enc::MleaderStyle(Box::new(MlsRec {
            src: obj! { "description" => name, "textStyle" => "*TS", "leaderLinetype" => "*LT" },
            defaults: Some((text_style, lt_by_block)),
        }));
        self.recs.push(r);
        for i in mls {
            if let Some(Rec { enc: Enc::Mleader(m), .. }) = self.recs.get_mut(i) {
                m.style_override = Some(style_h);
            }
        }
    }
}

/// `h(hex)`: `hex ? srcMap.get(hexKey(hex)) || 0 : 0`
fn hh(map: &std::collections::HashMap<String, f64>, hex: Option<&V>) -> f64 {
    if !truthy(hex) {
        return 0.0;
    }
    hex_key(hex).and_then(|k| map.get(&k).copied()).unwrap_or(0.0)
}
fn ml_style(m: &MlRec, map: &std::collections::HashMap<String, f64>) -> f64 {
    if let Some(s) = m.style_override {
        return s;
    }
    hh(map, m.src.get("styleHandle"))
}

pub fn encode_mleader(m: &MlRec, r: &Rec, cx: &mut Cx) -> Vec<u8> {
    let e = &m.src;
    let v = cx.ver;
    let env = &cx.env;
    let h = |x: Option<&V>| hh(env.src_map, x);
    let block_h = |hex: Option<&V>, b: Option<usize>| {
        let a = h(hex);
        if a != 0.0 { a } else { b.and_then(|i| env.block_hdr.get(i).copied()).unwrap_or(0.0) }
    };
    let style_text = |hex: Option<&V>, name: Option<&V>| {
        let a = h(hex);
        if a != 0.0 {
            a
        } else if truthy(name) {
            env.style_ref(name)
        } else {
            0.0
        }
    };
    let ctx = e.get("context");
    let style = ml_style(m, env.src_map);
    let leader_lt = {
        let a = h(e.get("leaderLinetypeHandle"));
        if a != 0.0 {
            a
        } else if truthy(e.get("leaderLinetype")) {
            env.ltype_h(e.get("leaderLinetype"))
        } else {
            0.0
        }
    };
    let arrowhead = block_h(e.get("arrowheadHandle"), m.arrow_b);
    let st_text = style_text(e.get("styleTextStyleHandle"), None);
    let style_block = block_h(e.get("styleBlockHandle"), None);
    let text_style = style_text(field(field(ctx, "text"), "styleHandle"), e.get("textStyle"));
    let content_block = block_h(field(field(ctx, "block"), "blockHandle"), m.content_b);
    let label_hs: Vec<(V, f64)> = arr(e.get("blockLabels")).iter().map(|l| (l.clone(), h(field(Some(l), "attdefHandle")))).collect();
    let arrow_hs: Vec<(V, f64)> = arr(e.get("arrowheads")).iter().map(|a| (a.clone(), h(field(Some(a), "handle")))).collect();
    // per-line handles (R2010+)
    let line_hs: Vec<Vec<(f64, f64)>> = arr(field(ctx, "roots"))
        .iter()
        .map(|root| {
            arr(field(Some(root), "lines"))
                .iter()
                .map(|ln| (h(field(Some(ln), "linetypeHandle")), h(field(Some(ln), "arrowheadHandle"))))
                .collect()
        })
        .collect();

    let mut w = BitWriter::obj_writer(cx.version, false);
    let ty = cx.classes.class_num("MULTILEADER");
    begin(&mut w, ty, r);
    let gfx = e.get("proxyGraphics").filter(|g| g.truthy());
    raw_entity_common(&mut w, r, gfx);
    if v >= 24 {
        w.bs(nnum(e.get("version"), 2.0));
    }
    write_context(&mut w, ctx, v, text_style, content_block, &line_hs);
    w.h(HARD_PTR, style);
    w.bl(nnum(e.get("overrideFlags"), 0.0));
    w.bs(nnum(e.get("leaderType"), 1.0));
    w.cmc(Some(&col(e.get("leaderColor"), idx(0.0))));
    w.h(HARD_PTR, leader_lt);
    w.bl(nnum(e.get("leaderLineweight"), -2.0));
    w.b(!matches!(e.get("landingEnabled"), Some(V::Bool(false))));
    w.b(!matches!(e.get("doglegEnabled"), Some(V::Bool(false))));
    w.bd(num(e.get("landingDistance"), 0.36));
    w.h(HARD_PTR, arrowhead);
    w.bd(num(e.get("arrowSize"), 0.18));
    let ct = match nn(e.get("contentType")) {
        Some(x) => to_number(Some(x)),
        None => {
            if truthy(field(ctx, "hasText")) {
                2.0
            } else if truthy(field(ctx, "hasBlock")) {
                1.0
            } else {
                0.0
            }
        }
    };
    w.bs(ct);
    w.h(HARD_PTR, st_text);
    w.bs(nnum(e.get("textLeftAttach"), 1.0));
    w.bs(nnum(e.get("textRightAttach"), 1.0));
    w.bs(nnum(e.get("textAngleType"), 1.0));
    w.bs(nnum(e.get("unknown175"), 0.0));
    w.cmc(Some(&col(e.get("textColor"), idx(0.0))));
    w.b(truthy(e.get("textFrame")));
    w.h(HARD_PTR, style_block);
    w.cmc(Some(&col(e.get("blockColor"), idx(0.0))));
    pt(&mut w, e.get("blockScale"), ONE);
    w.bd(num(e.get("blockRotation"), 0.0));
    w.bs(nnum(e.get("blockAttach"), 0.0));
    w.b(truthy(e.get("annotative")));
    if v < 24 {
        w.bl(usize_to_f64(arrow_hs.len()));
        for (a, ah) in &arrow_hs {
            w.b(truthy(field(Some(a), "isDefault")));
            w.h(HARD_PTR, *ah);
        }
    }
    let bl: Vec<&(V, f64)> = label_hs.iter().filter(|x| x.1 != 0.0).collect();
    w.bl(usize_to_f64(bl.len()));
    for (l, lh) in bl {
        w.h(SOFT_PTR, *lh);
        txt(&mut w, &str_or(field(Some(l), "text"), ""));
        w.bs(f64::from(int32(field(Some(l), "index"))));
        w.bd(num(field(Some(l), "width"), 0.0));
    }
    w.b(truthy(e.get("textDirectionNegative")));
    w.bs(nnum(e.get("ipeAlign"), 0.0));
    w.bs(nnum(e.get("justification"), 1.0));
    w.bd(num(e.get("scale"), 1.0));
    if v >= 24 {
        w.bs(nnum(e.get("attachDirection"), 0.0));
        w.bs(nnum(e.get("textTopAttach"), 9.0));
        w.bs(nnum(e.get("textBottomAttach"), 9.0));
    }
    if v >= 27 {
        w.b(truthy(e.get("extendToText")));
    }
    finish(&mut w)
}

fn write_context(w: &mut BitWriter, c: Option<&V>, v: u32, text_style: f64, content_block: f64, line_hs: &[Vec<(f64, f64)>]) {
    let roots = arr(field(c, "roots"));
    w.bl(usize_to_f64(roots.len()));
    for (ri, root) in roots.iter().enumerate() {
        let root = Some(root);
        w.b(!matches!(field(root, "contentValid"), Some(V::Bool(false))));
        w.b(!matches!(field(root, "unknown"), Some(V::Bool(false))));
        pt(w, field(root, "connection"), O);
        pt(w, field(root, "direction"), X1);
        let br = arr(field(root, "breaks"));
        w.bl(usize_to_f64(br.len()));
        for b in br {
            let a = b.as_arr().map(Vec::as_slice).unwrap_or(&[]);
            pt(w, a.first(), O);
            pt(w, a.get(1), O);
        }
        w.bl(nnum(field(root, "index"), usize_to_f64(ri)));
        w.bd(num(field(root, "landingDistance"), 0.36));
        let lines = arr(field(root, "lines"));
        w.bl(usize_to_f64(lines.len()));
        for (li, ln) in lines.iter().enumerate() {
            let ln = Some(ln);
            let ps = arr(field(ln, "points"));
            w.bl(usize_to_f64(ps.len()));
            for p in ps {
                pt(w, Some(p), O);
            }
            let bc = int32(field(ln, "breakInfoCount"));
            w.bl(f64::from(bc));
            if bc != 0 {
                w.bl(f64::from(int32(field(ln, "segmentIndex"))));
                let b = arr(field(ln, "breaks"));
                w.bl(usize_to_f64(b.len()));
                for x in b {
                    let a = x.as_arr().map(Vec::as_slice).unwrap_or(&[]);
                    pt(w, a.first(), O);
                    pt(w, a.get(1), O);
                }
            }
            w.bl(nnum(field(ln, "index"), usize_to_f64(li)));
            if v >= 24 {
                let (lt, ah) = line_hs.get(ri).and_then(|l| l.get(li)).copied().unwrap_or((0.0, 0.0));
                w.bs(nnum(field(ln, "leaderType"), 1.0));
                w.cmc(Some(&col(field(ln, "color"), idx(0.0))));
                w.h(HARD_PTR, lt);
                w.bl(nnum(field(ln, "lineweight"), -2.0));
                w.bd(num(field(ln, "arrowSize"), 0.0));
                w.h(HARD_PTR, ah);
                w.bl(f64::from(int32(field(ln, "overrideFlags"))));
            }
        }
        if v >= 24 {
            w.bs(f64::from(int32(field(root, "attachDirection"))));
        }
    }
    w.bd(num(field(c, "scale"), 1.0));
    pt(w, field(c, "contentBase"), O);
    w.bd(num(field(c, "textHeight"), 0.18));
    w.bd(num(field(c, "arrowSize"), 0.18));
    w.bd(num(field(c, "landingGap"), 0.09));
    w.bs(nnum(field(c, "leftAttach"), 1.0));
    w.bs(nnum(field(c, "rightAttach"), 1.0));
    w.bs(f64::from(int32(field(c, "textAlign"))));
    w.bs(f64::from(int32(field(c, "attachType"))));
    let has_text = !matches!(field(c, "hasText"), Some(V::Bool(false))) && truthy(field(c, "text"));
    w.b(has_text);
    if has_text {
        let t = field(c, "text");
        txt(w, &str_or(field(t, "label"), ""));
        pt(w, field(t, "normal"), Z);
        w.h(HARD_PTR, text_style);
        pt(w, field(t, "location"), O);
        pt(w, field(t, "direction"), X1);
        w.bd(num(field(t, "rotation"), 0.0));
        w.bd(num(field(t, "width"), 0.0));
        w.bd(num(field(t, "height"), 0.0));
        w.bd(num(field(t, "lineSpacing"), 1.0));
        w.bs(nnum(field(t, "lineSpacingStyle"), 1.0));
        w.cmc(Some(&col(field(t, "color"), idx(256.0))));
        w.bs(nnum(field(t, "alignment"), 1.0));
        w.bs(nnum(field(t, "flow"), 1.0));
        w.cmc(Some(&col(field(t, "bgColor"), idx(7.0))));
        w.bd(num(field(t, "bgScale"), 1.5));
        w.bl(f64::from(int32(field(t, "bgTransparency"))));
        w.b(truthy(field(t, "bgFill")));
        w.b(truthy(field(t, "bgMask")));
        w.bs(f64::from(int32(field(t, "columnType"))));
        w.b(truthy(field(t, "autoHeight")));
        w.bd(num(field(t, "columnWidth"), 0.0));
        w.bd(num(field(t, "columnGutter"), 0.0));
        w.b(truthy(field(t, "columnFlowReversed")));
        let cs = arr(field(t, "columnSizes"));
        w.bl(usize_to_f64(cs.len()));
        for x in cs {
            w.bd(num(Some(x), 0.0));
        }
        w.b(truthy(field(t, "wordBreak")));
        w.b(truthy(field(t, "unknown")));
    } else {
        let has_block = !matches!(field(c, "hasBlock"), Some(V::Bool(false))) && truthy(field(c, "block"));
        w.b(has_block);
        if has_block {
            let b = field(c, "block");
            w.h(SOFT_PTR, content_block);
            pt(w, field(b, "normal"), Z);
            pt(w, field(b, "location"), O);
            pt(w, field(b, "scale"), ONE);
            w.bd(num(field(b, "rotation"), 0.0));
            w.cmc(Some(&col(field(b, "color"), idx(0.0))));
            let m = field(b, "matrix").and_then(V::as_arr).filter(|m| m.len() == 16).cloned();
            let m: Vec<V> = match m {
                Some(m) => m,
                None => block_matrix(b).into_iter().map(V::Num).collect(),
            };
            for i in 0..16usize {
                w.bd(num(m.get(i), 0.0));
            }
        }
    }
    pt(w, field(c, "basePoint"), O);
    pt(w, field(c, "baseDirection"), X1);
    pt(w, field(c, "baseVertical"), Y1);
    w.b(truthy(field(c, "normalReversed")));
    if v >= 24 {
        w.bs(nnum(field(c, "topAttach"), 9.0));
        w.bs(nnum(field(c, "bottomAttach"), 9.0));
    }
}

/// 20.4.86: rotation, then scale, then translation (normal Z).
fn block_matrix(b: Option<&V>) -> [f64; 16] {
    let s = if truthy(field(b, "scale")) {
        let p = field(b, "scale");
        (num(field(p, "x"), 0.0), num(field(p, "y"), 0.0), num(field(p, "z"), 0.0))
    } else {
        ONE
    };
    let r = num(field(b, "rotation"), 0.0);
    let l = if truthy(field(b, "location")) {
        let p = field(b, "location");
        (num(field(p, "x"), 0.0), num(field(p, "y"), 0.0), num(field(p, "z"), 0.0))
    } else {
        O
    };
    let (cs, sn) = (r.cos(), r.sin());
    [cs * s.0, -sn * s.1, 0.0, l.0, sn * s.0, cs * s.1, 0.0, l.1, 0.0, 0.0, s.2, l.2, 0.0, 0.0, 0.0, 1.0]
}

/// MLEADERSTYLE from the model (mleader-read.js fields).
pub fn encode_mleader_style(m: &MlsRec, r: &Rec, cx: &mut Cx) -> Vec<u8> {
    let o = &m.src;
    let v = cx.ver;
    let env = &cx.env;
    let rf = |k: &str| -> f64 {
        let x = o.get(k);
        match m.defaults {
            Some((ts, lt)) => match x {
                Some(V::Str(s)) if s == "*TS" => ts,
                Some(V::Str(s)) if s == "*LT" => lt,
                _ => 0.0,
            },
            None => env.map(x),
        }
    };
    let (lt, ah, ts, blk) = (rf("leaderLinetype"), rf("arrowhead"), rf("textStyle"), rf("block"));
    let mut w = BitWriter::obj_writer(cx.version, false);
    let ty = cx.classes.class_num("MLEADERSTYLE");
    begin(&mut w, ty, r);
    object_common(&mut w, r);
    let new_format = v >= 24 || truthy(r.g("newFormat"));
    let g = |k: &str| o.get(k);
    if v >= 24 {
        w.bs(nnum(g("styleVersion"), 2.0));
    }
    w.bs(nnum(g("contentType"), 2.0));
    w.bs(nnum(g("drawMLeaderOrder"), 1.0));
    w.bs(f64::from(int32(g("drawLeaderOrder"))));
    w.bl(nnum(g("maxPoints"), 2.0));
    w.bd(num(g("firstAngle"), 0.0));
    w.bd(num(g("secondAngle"), 0.0));
    w.bs(nnum(g("leaderType"), 1.0));
    w.cmc(Some(&col(g("leaderColor"), idx(0.0))));
    w.h(HARD_PTR, lt);
    w.bl(nnum(g("leaderLineweight"), -2.0));
    w.b(!matches!(g("landingEnabled"), Some(V::Bool(false))));
    w.bd(num(g("landingGap"), 0.09));
    w.b(!matches!(g("doglegEnabled"), Some(V::Bool(false))));
    w.bd(num(g("landingDistance"), 0.36));
    w.tv(&str_or(g("description"), ""));
    w.h(HARD_PTR, ah);
    w.bd(num(g("arrowSize"), 0.18));
    w.tv(&str_or(g("textDefault"), ""));
    w.h(HARD_PTR, ts);
    w.bs(nnum(g("leftAttach"), 1.0));
    w.bs(nnum(g("rightAttach"), 1.0));
    if new_format {
        w.bs(nnum(g("textAngleType"), 1.0));
    }
    w.bs(f64::from(int32(g("textAlign"))));
    w.cmc(Some(&col(g("textColor"), idx(0.0))));
    w.bd(num(g("textHeight"), 0.18));
    w.b(truthy(g("textFrame")));
    if new_format {
        w.b(truthy(g("alwaysAlignLeft")));
    }
    w.bd(num(g("alignSpace"), 4.0));
    w.h(HARD_PTR, blk);
    w.cmc(Some(&col(g("blockColor"), idx(0.0))));
    pt(&mut w, g("blockScale"), ONE);
    w.b(!matches!(g("blockScaleEnabled"), Some(V::Bool(false))));
    w.bd(num(g("blockRotation"), 0.0));
    w.b(!matches!(g("blockRotationEnabled"), Some(V::Bool(false))));
    w.bs(f64::from(int32(g("blockConnection"))));
    w.bd(num(g("scale"), 1.0));
    w.b(truthy(g("changed")));
    w.b(truthy(g("annotative")));
    w.bd(num(g("breakSize"), 0.125));
    if v >= 24 {
        w.bs(f64::from(int32(g("attachDirection"))));
        w.bs(nnum(g("topAttach"), 9.0));
        w.bs(nnum(g("bottomAttach"), 9.0));
    }
    if v >= 27 {
        w.b(truthy(g("unknown298")));
    }
    finish(&mut w)
}
