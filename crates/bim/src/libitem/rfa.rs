//! A Revit family (.rfa) → an item, from what can honestly be read without Autodesk software (rvt-reader.js, the
//! part libitem.js's `itemFromRfa` uses): the OLE compound file ([MS-CFB], an open specification), its
//! BasicFileInfo stream (that it is a Revit file) and the PartAtom stream (Atom XML: title, categories, family
//! parameters). Geometry is not read: a box from Width / Depth / Height, else an outline, else data only.

use serde_json::Value as J;

use super::{Attrs, Read, attrs_info, base_item, box_item_geometry, clip500, msg, parse_length, s, source_attrs, suggest_category};

const ENDOFCHAIN: u32 = 0xffff_fffe;
const FREESECT: u32 = 0xffff_ffff;

fn u16le(b: &[u8], o: usize) -> u32 {
    b.get(o..o.saturating_add(2)).and_then(|x| x.try_into().ok()).map_or(0, |x: [u8; 2]| u32::from(u16::from_le_bytes(x)))
}
fn u32le(b: &[u8], o: usize) -> u32 {
    b.get(o..o.saturating_add(4)).and_then(|x| x.try_into().ok()).map_or(0, u32::from_le_bytes)
}

fn truncated(size: usize, ss: usize, sector: u32) -> String {
    let need = (u64::from(sector) + 1).saturating_mul(ss as u64).saturating_add(ss as u64);
    let mb = |n: f64| format!("{} MB", crate::js::to_fixed(n / 1_048_576.0, 1));
    format!(
        "This file is incomplete: it is {}, but its own index points to data at least {} in. It was probably cut off while being copied, uploaded or synced (a cloud-drive \"online-only\" file does this too). Copy the original again and reopen it.",
        mb(size as f64),
        mb(need as f64)
    )
}

struct Entry {
    name: String,
    ty: u8,
    start: u32,
    size: u32,
}

/// The streams of a compound file by name (`openCompoundFile`). Every chain walk is bounded.
struct Cfb<'a> {
    b: &'a [u8],
    ss: usize,
    mini_size: usize,
    fat: Vec<u32>,
    entries: Vec<Entry>,
    cutoff: u32,
    first_mini_fat: u32,
    n_mini_fat: u32,
    max_sectors: usize,
}

impl Cfb<'_> {
    fn off(&self, sec: u32) -> usize {
        (sec as usize).saturating_add(1).saturating_mul(self.ss)
    }
    fn read_sectors(&self, list: &[u32]) -> Vec<u8> {
        let mut out = vec![0u8; list.len().saturating_mul(self.ss)];
        for (i, s) in list.iter().enumerate() {
            let o = self.off(*s);
            let src = self.b.get(o..o.saturating_add(self.ss).min(self.b.len())).unwrap_or(&[]);
            let st = i.saturating_mul(self.ss);
            if let Some(dst) = out.get_mut(st..st.saturating_add(src.len())) {
                dst.copy_from_slice(src);
            }
        }
        out
    }
    fn chain(start: u32, table: &[u32], limit: usize) -> Result<Vec<u32>, String> {
        let mut out = Vec::new();
        let mut s = start;
        while s != ENDOFCHAIN && s != FREESECT {
            if s as usize >= table.len() || out.len() > limit {
                return Err("Broken sector chain in the compound file.".into());
            }
            out.push(s);
            s = table.get(s as usize).copied().unwrap_or(ENDOFCHAIN);
        }
        Ok(out)
    }
    fn read_chain(&self, start: u32, size: usize) -> Result<Vec<u8>, String> {
        let mut sectors = Self::chain(start, &self.fat, self.max_sectors)?;
        sectors.truncate(size.div_ceil(self.ss));
        let mut v = self.read_sectors(&sectors);
        v.truncate(size);
        Ok(v)
    }
    fn stream(&self, name: &str) -> Result<Option<Vec<u8>>, String> {
        let Some(d) = self.entries.iter().find(|e| e.ty == 2 && e.name == name) else { return Ok(None) };
        if d.size == 0 {
            return Ok(None);
        }
        if d.size >= self.cutoff {
            return self.read_chain(d.start, d.size as usize).map(Some);
        }
        let mini_fat: Vec<u32> = if self.n_mini_fat > 0 {
            self.read_chain(self.first_mini_fat, (self.n_mini_fat as usize).saturating_mul(self.ss))?
                .chunks(4)
                .map(|c| u32le(c, 0))
                .collect()
        } else {
            Vec::new()
        };
        let root = self.entries.first();
        let root_chain = match root {
            Some(r) if r.ty == 5 => Self::chain(r.start, &self.fat, self.max_sectors)?,
            _ => Vec::new(),
        };
        let per = self.ss.checked_div(self.mini_size).unwrap_or(1).max(1);
        let mut minis = Self::chain(d.start, &mini_fat, root_chain.len().saturating_mul(per).saturating_add(1))?;
        minis.truncate((d.size as usize).div_ceil(self.mini_size.max(1)));
        let mut out = vec![0u8; d.size as usize];
        let mut w = 0usize;
        for m in minis {
            let Some(&sec) = root_chain.get((m as usize).checked_div(per).unwrap_or(usize::MAX)) else {
                return Err("Broken mini stream in the compound file.".into());
            };
            let base = self.off(sec).saturating_add((m as usize).checked_rem(per).unwrap_or(0).saturating_mul(self.mini_size));
            let n = self.mini_size.min((d.size as usize).saturating_sub(w));
            let src = self.b.get(base..base.saturating_add(n).min(self.b.len())).unwrap_or(&[]);
            if let Some(dst) = out.get_mut(w..w.saturating_add(src.len())) {
                dst.copy_from_slice(src);
            }
            w = w.saturating_add(n);
        }
        Ok(Some(out))
    }
}

fn open(b: &[u8]) -> Result<Cfb<'_>, String> {
    let head = b.get(..512.min(b.len())).unwrap_or(&[]);
    let shift = u16le(head, 0x1e);
    let mini_shift = u16le(head, 0x20);
    if shift != 9 && shift != 12 {
        return Err("Unsupported compound file sector size.".into());
    }
    let ss = 1usize << shift;
    let mini_size = if mini_shift < 16 { 1usize << mini_shift } else { 64 };
    let max_sectors = b.len().div_ceil(ss).saturating_add(1);
    let fits = |sec: u32| (sec as usize).saturating_add(1).saturating_mul(ss).saturating_add(ss) <= b.len();
    let mut fat_sectors: Vec<u32> = Vec::new();
    for i in 0..109usize {
        let s = u32le(head, 0x4c_usize.saturating_add(i.saturating_mul(4)));
        if s == FREESECT || s == ENDOFCHAIN {
            break;
        }
        fat_sectors.push(s);
    }
    let first_mini_fat = u32le(head, 0x3c);
    let mut header_index = fat_sectors.clone();
    header_index.push(u32le(head, 0x30));
    if u32le(head, 0x40) != 0 && first_mini_fat != ENDOFCHAIN {
        header_index.push(first_mini_fat);
    }
    let furthest = header_index.iter().copied().max().unwrap_or(0);
    if !fits(furthest) {
        return Err(truncated(b.len(), ss, furthest));
    }
    let mut dif = u32le(head, 0x44);
    let mut guard = 0usize;
    while dif != ENDOFCHAIN && dif != FREESECT && guard < max_sectors {
        guard = guard.saturating_add(1);
        if !fits(dif) {
            return Err(truncated(b.len(), ss, dif));
        }
        let o = (dif as usize).saturating_add(1).saturating_mul(ss);
        let sec = b.get(o..o.saturating_add(ss)).unwrap_or(&[]);
        for i in 0..(ss / 4).saturating_sub(1) {
            let s = u32le(sec, i.saturating_mul(4));
            if s == FREESECT || s == ENDOFCHAIN {
                continue;
            }
            if !fits(s) {
                return Err(truncated(b.len(), ss, s));
            }
            fat_sectors.push(s);
        }
        dif = u32le(sec, ss.saturating_sub(4));
    }
    let mut c = Cfb {
        b,
        ss,
        mini_size,
        fat: Vec::new(),
        entries: Vec::new(),
        cutoff: 4096,
        first_mini_fat,
        n_mini_fat: u32le(head, 0x40),
        max_sectors,
    };
    c.fat = c.read_sectors(&fat_sectors).chunks(4).map(|x| u32le(x, 0)).collect();
    for s in (0..c.fat.len()).rev() {
        if c.fat.get(s) == Some(&FREESECT) {
            continue;
        }
        let s32 = u32::try_from(s).unwrap_or(u32::MAX);
        if !fits(s32) {
            return Err(truncated(b.len(), ss, s32));
        }
        break;
    }
    let dir = c.read_sectors(&Cfb::chain(u32le(head, 0x30), &c.fat, max_sectors)?);
    let mut o = 0usize;
    while o.saturating_add(128) <= dir.len() {
        let name_len = u16le(&dir, o.saturating_add(0x40)) as usize;
        let ty = dir.get(o.saturating_add(0x42)).copied().unwrap_or(0);
        if ty == 0 || !(2..=64).contains(&name_len) {
            c.entries.push(Entry { name: String::new(), ty: 0, start: 0, size: 0 });
        } else {
            let raw = dir.get(o..o.saturating_add(name_len).saturating_sub(2)).unwrap_or(&[]);
            let units: Vec<u16> =
                raw.chunks(2).map(|x| u16::from_le_bytes([x.first().copied().unwrap_or(0), x.get(1).copied().unwrap_or(0)])).collect();
            c.entries.push(Entry {
                name: String::from_utf16_lossy(&units),
                ty,
                start: u32le(&dir, o.saturating_add(0x74)),
                size: u32le(&dir, o.saturating_add(0x78)),
            });
        }
        o = o.saturating_add(128);
    }
    let co = u32le(head, 0x38);
    c.cutoff = if co == 0 { 4096 } else { co };
    Ok(c)
}

/// A family's PartAtom (`parseAtom`).
#[derive(Clone, Debug, Default)]
pub struct Family {
    pub title: Option<String>,
    pub categories: Vec<String>,
    pub groups: Vec<(String, Vec<(String, String)>)>,
}

fn unxml(s: &str) -> String {
    static RX: std::sync::OnceLock<Option<regex::Regex>> = std::sync::OnceLock::new();
    let Some(rx) = RX.get_or_init(|| regex::Regex::new(r"(?i)&(#x[0-9a-f]+|#[0-9]+|\w+);").ok()) else {
        return crate::js::trim(s).to_string();
    };
    let out = rx.replace_all(s, |c: &regex::Captures<'_>| {
        let e = c.get(1).map_or("", |m| m.as_str());
        let whole = c.get(0).map_or("", |m| m.as_str()).to_string();
        if let Some(num) = e.strip_prefix('#') {
            let v = match num.strip_prefix(['x', 'X']) {
                Some(h) => u32::from_str_radix(h, 16).ok(),
                None => num.parse::<u32>().ok(),
            };
            return v.and_then(char::from_u32).map_or(whole, |c| c.to_string());
        }
        match e {
            "amp" => "&".into(),
            "lt" => "<".into(),
            "gt" => ">".into(),
            "quot" => "\"".into(),
            "apos" => "'".into(),
            _ => whole,
        }
    });
    crate::js::trim(&out).to_string()
}

/// `parseAtom(xml)` (the parts an item needs)
pub fn parse_atom(xml: &str) -> Family {
    struct Rx {
        title: regex::Regex,
        cat: regex::Regex,
        param: fancy_regex::Regex,
        attr: regex::Regex,
        gtitle: regex::Regex,
    }
    static RX: std::sync::OnceLock<Option<Rx>> = std::sync::OnceLock::new();
    let Some(rx) = RX.get_or_init(|| {
        Some(Rx {
            title: regex::Regex::new(r"<title>([^<]*)</title>").ok()?,
            cat: regex::Regex::new(r"<category>\s*<term>([^<]*)</term>").ok()?,
            param: fancy_regex::Regex::new(r#"<([A-Za-z_][\w.-]*)((?:\s+[\w:-]+="[^"]*")*)\s*>([^<]*)</\1>"#).ok()?,
            attr: regex::Regex::new(r#"([\w:-]+)="([^"]*)""#).ok()?,
            gtitle: regex::Regex::new(r"<A:title>([^<]*)</A:title>").ok()?,
        })
    }) else {
        return Family::default();
    };
    let mut f = Family { title: rx.title.captures(xml).and_then(|c| c.get(1).map(|m| unxml(m.as_str()))), ..Family::default() };
    for c in rx.cat.captures_iter(xml) {
        f.categories.push(unxml(c.get(1).map_or("", |m| m.as_str())));
    }
    let (rp, ra, rt) = (&rx.param, &rx.attr, &rx.gtitle);
    for g in xml.split("<A:group>").skip(1).take(10_000) {
        let body = g.split("</A:group>").next().unwrap_or("");
        let title = rt.captures(body).and_then(|c| c.get(1)).map(|m| m.as_str().to_string());
        let mut params = Vec::new();
        for p in rp.captures_iter(body).take(100_000).flatten() {
            let attrs: Vec<(String, String)> = ra
                .captures_iter(p.get(2).map_or("", |m| m.as_str()))
                .map(|a| (a.get(1).map_or("", |m| m.as_str()).to_string(), unxml(a.get(2).map_or("", |m| m.as_str()))))
                .collect();
            let disp = attrs.iter().rev().find(|(k, _)| k == "displayName").map(|(_, v)| v.clone()).filter(|v| !v.is_empty());
            let name = disp.unwrap_or_else(|| p.get(1).map_or("", |m| m.as_str()).replace('_', " "));
            params.push((name, unxml(p.get(3).map_or("", |m| m.as_str()))));
        }
        if title.is_some() || !params.is_empty() {
            f.groups.push((title.map_or_else(|| "Other".to_string(), |t| unxml(&t)), params));
        }
    }
    f
}

/// `readRevit(bytes, fileName)` as far as `itemFromRfa` needs it → the family (None: no PartAtom). Err: not a
/// Revit file / unreadable, in the reader's words.
pub fn read_revit_family(b: &[u8]) -> Result<Option<Family>, String> {
    const MAGIC: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
    if b.len() < 8 || b.get(..8) != Some(&MAGIC[..]) {
        return Err("This is not a Revit file: it is not an OLE compound file.".into());
    }
    let c = open(b)?;
    if c.stream("BasicFileInfo")?.is_none() {
        return Err("This compound file has no BasicFileInfo stream, so it is not a Revit file.".into());
    }
    Ok(c.stream("PartAtom")?.map(|x| parse_atom(&String::from_utf8_lossy(&x))))
}

/// `itemFromRfa(info)`
pub fn item_from_rfa(family: Option<&Family>, file_name: &str) -> Read {
    let Some(fam) = family else { return Read { items: Vec::new(), notes: vec![msg("libitem.rfa.no_family", &[])] } };
    let mut notes = Vec::new();
    let title = fam.title.clone().filter(|t| !t.is_empty());
    let name = title.clone().unwrap_or_else(|| if file_name.is_empty() { "Family".to_string() } else { file_name.to_string() });
    let mut it = base_item(&name);
    let mut params: Attrs = Vec::new();
    for (_, ps) in &fam.groups {
        for (k, v) in ps {
            if !k.is_empty() && !v.is_empty() {
                match params.iter_mut().find(|(x, _)| x == k) {
                    Some(slot) => slot.1 = v.clone(),
                    None => params.push((k.clone(), v.clone())),
                }
            }
        }
    }
    it.insert("source_attrs".into(), J::Object(source_attrs(&params)));
    let find = |names: &[&str]| -> Option<f64> {
        for nn in names {
            for (k, v) in &params {
                if k.to_lowercase() == *nn {
                    let x = parse_length(v, 304.8);
                    if x.is_finite() && x > 0.0 {
                        return Some(x);
                    }
                }
            }
        }
        None
    };
    let (w, d, h) = (find(&["width", "overall width", "length"]), find(&["depth", "overall depth"]), find(&["height", "overall height"]));
    let get = |k: &str| params.iter().find(|(x, _)| x == k).map_or(String::new(), |(_, v)| v.clone());
    let info = attrs_info(&vec![("MANUFACTURER".to_string(), get("Manufacturer")), ("MODEL".to_string(), get("Model"))]);
    it.insert("maker".into(), s(&clip500(&info.maker)));
    it.insert("model".into(), s(&clip500(&info.model)));
    let cats = if fam.categories.is_empty() { String::new() } else { format!(" ({})", fam.categories.join(", ")) };
    it.insert("description".into(), s(&clip500(&format!("Family {}{cats}.", fam.title.clone().unwrap_or_default()))));
    match (w, d) {
        (Some(w), Some(d)) => {
            let label = it.get("name").and_then(J::as_str).unwrap_or("").to_string();
            box_item_geometry(&mut it, w, d, h.unwrap_or(0.0), 0.0, &label);
            notes.push(msg(if h.is_some() { "libitem.rfa.box" } else { "libitem.rfa.outline" }, &[]));
        }
        _ => {
            it.insert("body".into(), J::Null);
            notes.push(msg("libitem.rfa.no_geometry", &[]));
        }
    }
    let iname = it.get("name").and_then(J::as_str).unwrap_or("").to_string();
    let sug = suggest_category(&format!("{} {iname}", fam.categories.join(" ")));
    match sug {
        Some(c) => {
            it.insert("category".into(), s(c));
            notes.push(msg("libitem.category.suggested_family", &[("category", c.to_string())]));
        }
        None => notes.push(msg("libitem.category.set", &[("category", "other/other".to_string())])),
    }
    if fam.categories.join(" ").to_lowercase().contains("furniture") {
        it.insert("kind".into(), s("furniture"));
        it.insert("category".into(), s(sug.unwrap_or("furniture/other")));
    }
    Read { items: vec![J::Object(it)], notes }
}
