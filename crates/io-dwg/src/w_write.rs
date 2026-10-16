//! writeDwg(model, { version }) → (bytes, report) (port of lib/dwg/write.js, Apache-2.0).
//!
//! Turns the version-neutral model into DWG objects and lays them out for AC1015 (R2000), AC1018 (R2004), AC1021
//! (R2007), AC1024 (R2010), AC1027 (R2013) or AC1032 (R2018). Handles are renumbered: every object gets a fresh handle
//! in the order it is written (controls and tables, then each block: header, BLOCK, contents, ENDBLK).
//! report.handles maps the model's handles to the new ones. What cannot be written is skipped and listed in
//! report.notes (and counted in report.skipped).
//!
//! Inputs the JS takes from the environment, made explicit (WriteOpts): the creation time (JS: `opts.created`, else
//! `new Date()`) and the GUIDs (JS: `crypto.getRandomValues`): FINGERPRINTGUID / VERSIONGUID in the header and the
//! revision GUID of a restored NURBSURFACE. `GuidMode::Sequence` is the deterministic source the parity harness also
//! installs in the JS (`getRandomValues` filling call k with bytes `(16k + i) & 255`).

use std::collections::{HashMap, HashSet};

use crate::obj;
use crate::util::{to_i32, to_u32, usize_to_f64};
use crate::value::{Obj, V};
use crate::w_bits::{LOST, at_least, version_num};
use crate::w_container::{Jds, Parts, layout_r2000, layout_r2004, version_info};
use crate::w_js::{arr, field, hex_key, hex_upper, int32, js_entries, js_string, js_trim, nn, num, ok_p, p3_of, str_or, to_number, truthy};
use crate::w_objects::{Classes, Cx, Env, Rec, encode_classes, encode_header, encode_object, julian, ltype_ref, style_ref};

/// Where GUIDs come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuidMode {
    /// Pseudo-random (seeded from the clock and the process).
    Random,
    /// Call k fills its 16 bytes with `(16k + i) & 255` (the parity harness's stub of `getRandomValues`).
    Sequence,
}
#[derive(Clone, Debug)]
pub struct WriteOpts {
    pub version: String,
    /// Creation time, ms since 1970 (None: now).
    pub created_ms: Option<f64>,
    pub guid: GuidMode,
}
impl Default for WriteOpts {
    fn default() -> Self {
        WriteOpts { version: String::new(), created_ms: None, guid: GuidMode::Random }
    }
}

pub struct Guids {
    mode: GuidMode,
    k: u32,
    state: u64,
}
impl Guids {
    pub fn new(mode: GuidMode) -> Self {
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        if mode == GuidMode::Random {
            use std::hash::{BuildHasher, Hasher};
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u64(crate::util::f64_to_u64(now_ms()));
            state = h.finish() | 1;
        }
        Guids { mode, k: 0, state }
    }
    /// 16 bytes (getRandomValues).
    pub fn bytes(&mut self) -> [u8; 16] {
        let mut r = [0u8; 16];
        match self.mode {
            GuidMode::Sequence => {
                for (i, b) in r.iter_mut().enumerate() {
                    *b = crate::util::u8_of(self.k.wrapping_mul(16).wrapping_add(crate::util::u32_of_usize(i)));
                }
            }
            GuidMode::Random => {
                for b in r.iter_mut() {
                    // xorshift64*
                    self.state ^= self.state >> 12;
                    self.state ^= self.state << 25;
                    self.state ^= self.state >> 27;
                    *b = crate::util::u8_of(crate::util::low_u32(self.state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56));
                }
            }
        }
        self.k = self.k.wrapping_add(1);
        r
    }
    /// write.js guid(): uppercase.
    pub fn guid_upper(&mut self) -> String {
        fmt_guid(&self.bytes(), true)
    }
    /// surface-write.js newGuid(): lowercase.
    pub fn guid_lower(&mut self) -> String {
        fmt_guid(&self.bytes(), false)
    }
}
fn fmt_guid(b: &[u8; 16], upper: bool) -> String {
    let h: String = b.iter().map(|x| if upper { format!("{x:02X}") } else { format!("{x:02x}") }).collect();
    let s = |a: usize, z: usize| h.get(a..z).unwrap_or("");
    format!("{{{}-{}-{}-{}-{}}}", s(0, 8), s(8, 12), s(12, 16), s(16, 20), s(20, 32))
}
pub fn now_ms() -> f64 {
    web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).map_or(0.0, f64::floor)
}

/// DXF lineweights (1/100 mm) in the order of the DWG lineweight index.
const LW_TABLE: [f64; 24] = [
    0.0, 5.0, 9.0, 13.0, 15.0, 18.0, 20.0, 25.0, 30.0, 35.0, 40.0, 50.0, 53.0, 60.0, 70.0, 80.0, 90.0, 100.0, 106.0, 120.0, 140.0, 158.0,
    200.0, 211.0,
];
/// write.js lwIndex.
pub fn lw_index(lw: Option<&V>) -> f64 {
    match lw {
        None | Some(V::Null) => return 29.0,
        Some(V::Num(n)) if *n == -1.0 => return 29.0,
        Some(V::Num(n)) if *n == -2.0 => return 30.0,
        Some(V::Num(n)) if *n == -3.0 => return 31.0,
        _ => {}
    }
    let x = to_number(lw);
    if x < 0.0 {
        return 31.0;
    }
    let mut best = 0usize;
    for i in 1..LW_TABLE.len() {
        let a = LW_TABLE.get(i).copied().unwrap_or(0.0);
        let b = LW_TABLE.get(best).copied().unwrap_or(0.0);
        if (a - x).abs() < (b - x).abs() {
            best = i;
        }
    }
    usize_to_f64(best)
}

/// The AutoCAD colour index palette (write.js ACI_RGB).
fn aci_rgb() -> Vec<[f64; 3]> {
    let mut t = vec![[0.0f64; 3]; 256];
    let named = [
        [255.0, 0.0, 0.0],
        [255.0, 255.0, 0.0],
        [0.0, 255.0, 0.0],
        [0.0, 255.0, 255.0],
        [0.0, 0.0, 255.0],
        [255.0, 0.0, 255.0],
        [255.0, 255.0, 255.0],
        [128.0, 128.0, 128.0],
        [192.0, 192.0, 192.0],
    ];
    for (i, c) in named.iter().enumerate() {
        if let Some(x) = t.get_mut(i.saturating_add(1)) {
            *x = *c;
        }
    }
    let vv = [255.0, 204.0, 153.0, 127.0, 76.0];
    for i in 10..250usize {
        let hue = ((usize_to_f64(i) - 10.0) / 10.0).floor() * 15.0;
        let k = i.saturating_sub(10) % 10;
        let v = vv.get(k >> 1).copied().unwrap_or(0.0);
        let pastel = k & 1;
        let h6 = hue / 60.0;
        let f = h6 - h6.floor();
        let lo = if pastel != 0 { v / 2.0 } else { 0.0 };
        let q = v - (v - lo) * f;
        let u = lo + (v - lo) * f;
        let rgb = match crate::util::to_usize(h6.floor()) % 6 {
            0 => [v, u, lo],
            1 => [q, v, lo],
            2 => [lo, v, u],
            3 => [lo, q, v],
            4 => [u, lo, v],
            _ => [v, lo, q],
        };
        if let Some(x) = t.get_mut(i) {
            *x = [js_round(rgb[0]), js_round(rgb[1]), js_round(rgb[2])];
        }
    }
    for (i, g) in [51.0, 91.0, 132.0, 173.0, 214.0, 255.0].iter().enumerate() {
        if let Some(x) = t.get_mut(250usize.saturating_add(i)) {
            *x = [*g, *g, *g];
        }
    }
    t
}
/// Math.round
pub fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}
/// write.js aciFromRgb
pub fn aci_from_rgb(rgb: f64) -> f64 {
    let rgb = to_i32(rgb);
    let r = f64::from((rgb >> 16) & 255);
    let g = f64::from((rgb >> 8) & 255);
    let b = f64::from(rgb & 255);
    let t = aci_rgb();
    let mut best = 7usize;
    let mut bd = f64::INFINITY;
    for (i, c) in t.iter().enumerate().skip(1) {
        let d = (c[0] - r).powi(2) + (c[1] - g).powi(2) + (c[2] - b).powi(2);
        if d < bd {
            bd = d;
            best = i;
        }
    }
    usize_to_f64(best)
}

/// `/[<>/\\":;?*|,=`\x00-\x1f]/`
fn bad_name_char(c: char) -> bool {
    matches!(c, '<' | '>' | '/' | '\\' | '"' | ':' | ';' | '?' | '*' | '|' | ',' | '=' | '`') || (c as u32) < 0x20
}
/// JS `s.slice(0, n)` on UTF-16 units (a pair cut in two is dropped).
pub fn slice_units(s: &str, n: usize) -> String {
    let mut out = String::new();
    let mut k = 0usize;
    for c in s.chars() {
        let l = c.len_utf16();
        if k.saturating_add(l) > n {
            break;
        }
        k = k.saturating_add(l);
        out.push(c);
    }
    out
}
/// JS string `<` (UTF-16 code unit order).
pub fn js_lt(a: &str, b: &str) -> bool {
    a.encode_utf16().cmp(b.encode_utf16()) == std::cmp::Ordering::Less
}
/// `/^\*paper_space\d*$/i`
fn is_ps_name(s: &str) -> bool {
    let lower = s.to_lowercase();
    match lower.strip_prefix("*paper_space") {
        Some(rest) => rest.bytes().all(|b| b.is_ascii_digit()),
        None => false,
    }
}
fn eq_ci(s: &str, t: &str) -> bool {
    s.to_lowercase() == t
}

/// V8's Array.prototype.sort for up to 64 items (one binary-insertion run: exact for any comparator), a stable sort
/// beyond (exact for consistent comparators).
pub fn js_sort<T, F: FnMut(&T, &T) -> f64>(a: &mut [T], mut cmp: F) {
    let n = a.len();
    if n < 2 {
        return;
    }
    if n > 64 {
        a.sort_by(|x, y| {
            let c = cmp(x, y);
            if c < 0.0 {
                std::cmp::Ordering::Less
            } else if c > 0.0 {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        return;
    }
    // CountAndMakeRun
    let mut run = 2usize;
    let desc = a.get(1).zip(a.first()).is_some_and(|(x, y)| cmp(x, y) < 0.0);
    let mut idx = 2usize;
    while idx < n {
        let o = match (a.get(idx), a.get(idx.saturating_sub(1))) {
            (Some(x), Some(y)) => cmp(x, y),
            _ => 0.0,
        };
        if desc {
            if o >= 0.0 || o.is_nan() {
                break;
            }
        } else if o < 0.0 {
            break;
        }
        run = run.saturating_add(1);
        idx = idx.saturating_add(1);
    }
    if desc && let Some(s) = a.get_mut(..run) {
        s.reverse();
    }
    // BinaryInsertionSort(0, run, n)
    let mut start = run;
    while start < n {
        let (mut left, mut right) = (0usize, start);
        while left < right {
            let mid = left.saturating_add(right.saturating_sub(left) >> 1);
            let o = match (a.get(start), a.get(mid)) {
                (Some(p), Some(m)) => cmp(p, m),
                _ => 0.0,
            };
            if o < 0.0 {
                right = mid;
            } else {
                left = mid.saturating_add(1);
            }
        }
        if left < start
            && let Some(s) = a.get_mut(left..=start)
        {
            s.rotate_right(1);
        }
        start = start.saturating_add(1);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Model,
    Paper,
    Paper2,
    Block,
}
#[derive(Clone, Debug)]
pub struct Block<'m> {
    /// the model's block without its entities
    pub src: Obj,
    /// the model's entities (None for a block the writer adds)
    pub ents_src: Option<&'m V>,
    pub name: String,
    pub kind: Kind,
    pub hdr: f64,
    pub block: f64,
    pub endblk: f64,
    pub ents: Vec<Vec<Rec>>,
    pub inserts: Vec<f64>,
    pub rec_src: Option<Obj>,
    pub layout: f64,
}
#[derive(Clone, Debug)]
pub struct Layout {
    pub src: Obj,
    pub name: String,
    pub tab: f64,
    pub b: usize,
    pub h: f64,
}

/// The fixed handles (write.js `H`).
#[derive(Clone, Debug, Default)]
pub struct Fixed {
    pub block_ctl: f64,
    pub layer_ctl: f64,
    pub style_ctl: f64,
    pub ltype_ctl: f64,
    pub view_ctl: f64,
    pub ucs_ctl: f64,
    pub vport_ctl: f64,
    pub appid_ctl: f64,
    pub dimstyle_ctl: f64,
    pub vpent_ctl: f64,
    pub nod: f64,
    pub group_dict: f64,
    pub mls_dict: f64,
    pub layout_dict: f64,
    pub plot_style_dict: f64,
    pub plot_style_normal: f64,
    pub lt_by_block: f64,
    pub lt_by_layer: f64,
    pub lt_continuous: f64,
    pub layer0: f64,
    pub mlstyle: f64,
    pub vport: f64,
}

/// One write: all of write.js's state (the other modules add `impl W` blocks).
pub struct W<'m> {
    pub model: &'m Obj,
    pub version: &'static str,
    pub ver: u32,
    pub r2004: bool,
    pub r2007: bool,
    // report
    pub notes: Vec<String>,
    pub counts: Obj,
    pub skipped: Obj,
    pub report_layers: Vec<V>,
    /// report.handles: (model handle, new handle hex) in insertion order (a JS object: a key set again keeps its place)
    pub handles: Vec<(String, String)>,
    pub handles_at: HashMap<String, usize>,
    pub renamed_layers: Obj,
    pub renamed_blocks: Obj,
    pub renamed_layouts: Obj,
    pub report_tables: Option<V>,
    pub report_copied: Option<V>,
    pub lost_rgb: u32,
    pub created: f64,
    pub seed: f64,
    pub recs: Vec<Rec>,
    pub h: Fixed,
    pub src_map: HashMap<String, f64>,
    /// srcMap's keys in first-insertion order (a JS Map iterates so)
    pub src_order: Vec<String>,
    pub used_names: HashMap<String, HashSet<String>>,
    /// clean_name: the next suffix to try per (table, name)
    pub name_k: HashMap<(String, String), u32>,
    pub renamed_layers_n: u32,
    pub renamed_blocks_n: u32,
    pub lt_by_name: HashMap<String, f64>,
    pub ltypes: Vec<Rec>,
    pub styles: Vec<Rec>,
    pub style_by_name: HashMap<String, f64>,
    pub xref_block_names: HashSet<String>,
    pub layers: Vec<Rec>,
    pub layer_by_name: HashMap<String, usize>,
    pub layer_by_new: HashMap<String, usize>,
    pub appids: Vec<Rec>,
    pub app_by_name: HashMap<String, f64>,
    pub dimstyles: Vec<Rec>,
    pub dim_by_name: HashMap<String, f64>,
    pub views: Vec<Rec>,
    pub ucs_list: Vec<Rec>,
    pub actives: Vec<(Option<Obj>, f64)>,
    pub blocks: Vec<Block<'m>>,
    pub block_by_name: HashMap<String, usize>,
    pub ms: usize,
    pub pss: Vec<usize>,
    pub layouts: Vec<Layout>,
    pub image_defs: HashMap<String, Obj>,
    pub written_defs: Vec<(String, Rec)>,
    pub def_names: HashSet<String>,
    pub need_wipeout_vars: bool,
    pub skipped_n: Obj,
    pub guids: Guids,
    pub x: crate::w_nongraphic::XState,
    pub tbl: crate::w_table::TState,
    pub sol: crate::w_solid::SState,
    pub surf: crate::w_surface::SfState,
}

fn alias(v: &str) -> &str {
    match v {
        "R2000" | "2000" => "AC1015",
        "R2004" | "2004" => "AC1018",
        "R2007" | "2007" => "AC1021",
        "R2010" | "2010" => "AC1024",
        "R2013" | "2013" => "AC1027",
        "R2018" | "2018" => "AC1032",
        x => x,
    }
}
pub const VERSIONS: [&str; 6] = ["AC1015", "AC1018", "AC1021", "AC1024", "AC1027", "AC1032"];

/// write.js `writeDwg(model, { version })` → (bytes, report), with the JS's own inputs: the time now and random GUIDs.
/// `version`: AC1015 … AC1032 or an alias (R2000 … R2018, 2000 … 2018); empty → the model's version → AC1015.
pub fn write_dwg(model: &Obj, version: &str) -> Result<(Vec<u8>, Obj), String> {
    write_dwg_opts(model, &WriteOpts { version: version.to_string(), ..WriteOpts::default() })
}

/// writeDwg with the creation time and the GUID source given (deterministic output: see the module docs).
pub fn write_dwg_opts(model: &Obj, opts: &WriteOpts) -> Result<(Vec<u8>, Obj), String> {
    let mv = model.str("version").unwrap_or("");
    let v0 = if !opts.version.is_empty() {
        alias(&opts.version).to_string()
    } else if !mv.is_empty() {
        mv.to_string()
    } else {
        "AC1015".to_string()
    };
    let Some(version) = VERSIONS.iter().copied().find(|x| *x == v0) else {
        return Err(format!("writeDwg: version {v0} is not supported (use one of {})", VERSIONS.join(", ")));
    };
    LOST.with(|l| l.set(0));
    let created = opts.created_ms.unwrap_or_else(now_ms);
    let mut w = W::new(model, version, created, opts.guid);
    w.run()
}

impl<'m> W<'m> {
    fn new(model: &'m Obj, version: &'static str, created: f64, guid: GuidMode) -> Self {
        W {
            model,
            version,
            ver: version_num(version),
            r2004: at_least(version, "AC1018"),
            r2007: at_least(version, "AC1021"),
            notes: Vec::new(),
            counts: Obj::new(),
            skipped: Obj::new(),
            report_layers: Vec::new(),
            handles: Vec::new(),
            handles_at: HashMap::new(),
            renamed_layers: Obj::new(),
            renamed_blocks: Obj::new(),
            renamed_layouts: Obj::new(),
            report_tables: None,
            report_copied: None,
            lost_rgb: 0,
            created,
            seed: 1.0,
            recs: Vec::new(),
            h: Fixed::default(),
            src_map: HashMap::new(),
            src_order: Vec::new(),
            used_names: HashMap::new(),
            name_k: HashMap::new(),
            renamed_layers_n: 0,
            renamed_blocks_n: 0,
            lt_by_name: HashMap::new(),
            ltypes: Vec::new(),
            styles: Vec::new(),
            style_by_name: HashMap::new(),
            xref_block_names: HashSet::new(),
            layers: Vec::new(),
            layer_by_name: HashMap::new(),
            layer_by_new: HashMap::new(),
            appids: Vec::new(),
            app_by_name: HashMap::new(),
            dimstyles: Vec::new(),
            dim_by_name: HashMap::new(),
            views: Vec::new(),
            ucs_list: Vec::new(),
            actives: Vec::new(),
            blocks: Vec::new(),
            block_by_name: HashMap::new(),
            ms: 0,
            pss: Vec::new(),
            layouts: Vec::new(),
            image_defs: HashMap::new(),
            written_defs: Vec::new(),
            def_names: HashSet::new(),
            need_wipeout_vars: false,
            skipped_n: Obj::new(),
            guids: Guids::new(guid),
            x: crate::w_nongraphic::XState::default(),
            tbl: crate::w_table::TState::default(),
            sol: crate::w_solid::SState::default(),
            surf: crate::w_surface::SfState::default(),
        }
    }

    // ---------------------------------------------------------------- handles
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> f64 {
        let s = self.seed;
        self.seed += 1.0;
        s
    }
    /// mapSrc(src, h)
    pub fn map_src(&mut self, src: Option<&V>, h: f64) {
        if let Some(k) = hex_key(src).filter(|k| !k.is_empty()) {
            self.map_key(k, h);
        }
    }
    pub fn map_key(&mut self, k: String, h: f64) {
        match self.handles_at.get(&k) {
            Some(&i) => {
                if let Some(x) = self.handles.get_mut(i) {
                    x.1 = hex_upper(h);
                }
            }
            None => {
                self.handles_at.insert(k.clone(), self.handles.len());
                self.handles.push((k.clone(), hex_upper(h)));
            }
        }
        if self.src_map.insert(k.clone(), h).is_none() {
            self.src_order.push(k);
        }
    }
    /// srcMap.get(hexKey(v)) (0 = none)
    pub fn src(&self, v: Option<&V>) -> f64 {
        hex_key(v).and_then(|k| self.src_map.get(&k).copied()).unwrap_or(0.0)
    }
    pub fn src_has(&self, v: Option<&V>) -> bool {
        hex_key(v).is_some_and(|k| self.src_map.contains_key(&k))
    }

    // ---------------------------------------------------------------- names
    pub fn clean_name(&mut self, table: &str, name: Option<&V>, fallback: &str) -> String {
        let s = str_or(name, "").replace('|', "$0$");
        let s: String = s.chars().map(|c| if bad_name_char(c) { '_' } else { c }).collect();
        let s = slice_units(js_trim(&s), 255);
        let clean = if s.is_empty() { fallback.to_string() } else { s };
        // the JS counts up from 2 each time; names are only ever added, so every candidate up to the last one found for
        // this name is still taken: start after it (the same result, without the quadratic search)
        let ck = (table.to_string(), clean.clone());
        let (mut n, mut k) = match self.name_k.get(&ck) {
            Some(&k0) => (format!("{clean}_{k0}"), k0.saturating_add(1)),
            None => (clean.clone(), 2u32),
        };
        let used = self.used_names.entry(table.to_string()).or_default();
        while used.contains(&n.to_uppercase()) {
            n = format!("{clean}_{k}");
            k = k.saturating_add(1);
        }
        used.insert(n.to_uppercase());
        self.name_k.insert(ck, k);
        n
    }
    pub fn reserve(&mut self, table: &str, name: &str) {
        self.used_names.entry(table.to_string()).or_default().insert(name.to_uppercase());
    }

    // ---------------------------------------------------------------- references
    pub fn ltype_ref(&self, name: Option<&V>) -> (u32, f64) {
        ltype_ref(&self.lt_by_name, &self.src_map, name)
    }
    pub fn ltype_ref_v(&self, name: Option<&V>) -> V {
        let (flag, h) = self.ltype_ref(name);
        if flag == 3 { V::Obj(obj! { "flag" => 3, "h" => h }) } else { V::Obj(obj! { "flag" => flag }) }
    }
    pub fn style_ref(&self, name: Option<&V>) -> f64 {
        style_ref(&self.style_by_name, &self.src_map, name)
    }
    pub fn dim_ref(&self, name: Option<&V>) -> f64 {
        let up = str_or(name, "Standard").to_uppercase();
        if let Some(h) = self.dim_by_name.get(&up).copied().filter(|h| *h != 0.0) {
            return h;
        }
        let s = self.src(name);
        if s != 0.0 {
            return s;
        }
        self.dim_by_name.get("STANDARD").copied().unwrap_or(0.0)
    }
    pub fn layer_ref(&mut self, name: Option<&V>) -> f64 {
        let s = str_or(name, "0");
        if let Some(i) = self.layer_by_name.get(&s).copied() {
            return self.layers.get(i).map_or(0.0, Rec::h);
        }
        let l = obj! { "name" => s };
        let i = self.add_layer(&l);
        self.layers.get(i).map_or(0.0, Rec::h)
    }

    // ---------------------------------------------------------------- tables
    fn add_style(&mut self, s: &Obj) -> f64 {
        let up = if truthy(s.get("name")) { js_string(s.get("name")) } else { "Standard".into() }.to_uppercase();
        if let Some(h) = self.style_by_name.get(&up).copied() {
            self.map_src(s.get("handle"), h);
            return h;
        }
        let name = if up == "STANDARD" { "Standard".to_string() } else { self.clean_name("style", s.get("name"), "Style") };
        if up == "STANDARD" {
            self.reserve("style", "Standard");
        }
        let h = self.next();
        self.map_src(s.get("handle"), h);
        self.style_by_name.insert(up, h);
        self.style_by_name.insert(name.to_uppercase(), h);
        let flags = int32(s.get("flags"));
        let mut o = obj! {
            "t" => "STYLE", "h" => h, "owner" => self.h.style_ctl, "name" => name, "height" => num(s.get("height"), 0.0),
            "widthFactor" => num(s.get("widthFactor"), 1.0), "oblique" => num(s.get("oblique"), 0.0), "generation" => int32(s.get("generation")),
            "lastHeight" => num(s.get("lastHeight"), 0.2),
        };
        o.set("font", nn(s.get("font")).cloned().unwrap_or_else(|| V::from("txt")));
        o.set("bigFont", nn(s.get("bigFont")).cloned().unwrap_or_else(|| V::from("")));
        o.set("vertical", flags & 4 != 0);
        o.set("shapeFile", flags & 1 != 0);
        self.styles.push(Rec::new(o));
        h
    }
    pub fn add_layer(&mut self, l: &Obj) -> usize {
        let orig = str_or(l.get("name"), "0");
        if let Some(i) = self.layer_by_name.get(&orig).copied() {
            return i;
        }
        let keep = orig == "0" || {
            let lo = orig.to_lowercase();
            lo == "*layernamefordynamicconstraint" || lo == "*adsk_constraints" || lo == "*adsk_system_lights"
        };
        let bar = orig.find('|');
        let xref_of = match bar {
            Some(b) if b > 0 => orig.get(..b).unwrap_or("").to_string(),
            _ => String::new(),
        };
        let dep = !xref_of.is_empty()
            && self.xref_block_names.contains(&xref_of.to_uppercase())
            && !orig.chars().any(|c| c != '|' && bad_name_char(c));
        let name = if keep || dep {
            orig.clone()
        } else {
            let n = if orig.is_empty() { V::from("Unnamed") } else { V::from(orig.as_str()) };
            self.clean_name("layer", Some(&n), "Unnamed")
        };
        if keep || dep {
            self.reserve("layer", &orig);
        }
        if name != orig {
            self.renamed_layers_n = self.renamed_layers_n.saturating_add(1);
            self.renamed_layers.set(orig.clone(), name.clone());
        }
        let mut color: V = match l.get("color") {
            Some(V::Num(n)) => V::Obj(obj! { "index" => *n }),
            Some(c) => c.clone(),
            None => V::Null,
        };
        let idx_abs = to_number(field(Some(&color), "index")).abs();
        if !color.truthy() || (field(Some(&color), "rgb").is_none() && !((1.0..=255.0).contains(&idx_abs))) {
            color = V::Obj(obj! { "index" => 7 });
        }
        let idx = field(Some(&color), "index");
        let idx_n = to_number(idx);
        let off =
            matches!(l.get("off"), Some(V::Bool(true))) || matches!(l.get("on"), Some(V::Bool(false))) || (idx.is_some() && idx_n < 0.0);
        if idx_n < 0.0
            && let V::Obj(o) = &mut color
        {
            o.set("index", -idx_n);
        }
        if let Some(rgb) = field(Some(&color), "rgb").cloned()
            && !self.r2004
        {
            self.lost_rgb = self.lost_rgb.saturating_add(1);
            color = V::Obj(obj! { "index" => aci_from_rgb(to_number(Some(&rgb))) });
        }
        let h = if orig == "0" { self.h.layer0 } else { self.next() };
        self.map_src(l.get("handle"), h);
        let mut o = obj! { "t" => "LAYER", "h" => h, "owner" => self.h.layer_ctl, "name" => name.clone() };
        o.set("color", color.clone());
        o.set("off", off);
        o.set("frozen", truthy(l.get("frozen")));
        o.set("locked", truthy(l.get("locked")));
        o.set("plot", !matches!(l.get("plot"), Some(V::Bool(false))));
        o.set("frozenNew", truthy(l.get("frozenInNewViewports")));
        if dep {
            o.set("xdep", true);
            o.set("xrefOf", xref_of);
        }
        o.set("lwIndex", if l.get("lineweight").is_none() { 31.0 } else { lw_index(l.get("lineweight")) });
        let lt = {
            let a = self.ltype_ref(l.get("linetype")).1;
            if a != 0.0 {
                a
            } else {
                let b = self.src(l.get("linetype"));
                if b != 0.0 { b } else { self.h.lt_continuous }
            }
        };
        o.set("ltype", lt);
        o.set("plotstyle", self.h.plot_style_normal);
        let i = self.layers.len();
        self.layers.push(Rec::new(o));
        self.layer_by_name.insert(orig, i);
        self.layer_by_new.insert(name.clone(), i);
        let rc = if field(Some(&color), "rgb").is_some() { color } else { field(Some(&color), "index").cloned().unwrap_or(V::Null) };
        self.report_layers.push(V::Obj(obj! { "name" => name, "color" => rc }));
        i
    }
    /// addApp(name, src) → handle
    pub fn add_app(&mut self, name: &str, src: Option<&V>) -> f64 {
        let up = name.to_uppercase();
        if let Some(h) = self.app_by_name.get(&up).copied() {
            self.map_src(src, h);
            return h;
        }
        let clean = self.clean_name("appid", Some(&V::from(name)), "APP");
        let h = self.next();
        self.map_src(src, h);
        self.app_by_name.insert(up, h);
        self.appids.push(Rec::new(obj! { "t" => "APPID", "h" => h, "owner" => self.h.appid_ctl, "name" => clean }));
        h
    }
    fn add_dim(&mut self, d: &Obj) {
        let up = if truthy(d.get("name")) { js_string(d.get("name")) } else { "Standard".into() }.to_uppercase();
        if let Some(h) = self.dim_by_name.get(&up).copied() {
            self.map_src(d.get("handle"), h);
            return;
        }
        let name = if up == "STANDARD" { "Standard".to_string() } else { self.clean_name("dimstyle", d.get("name"), "Dimstyle") };
        let h = self.next();
        self.map_src(d.get("handle"), h);
        self.dim_by_name.insert(up, h);
        self.dim_by_name.insert(name.to_uppercase(), h);
        let vars = d.obj("vars").cloned().unwrap_or_default();
        let txsty = self.style_ref(vars.get("DIMTXSTY"));
        let mut o = obj! { "t" => "DIMSTYLE", "h" => h, "owner" => self.h.dimstyle_ctl, "name" => name };
        o.set("vars", vars);
        o.set("txsty", txsty);
        self.dimstyles.push(Rec::new(o));
    }

    /// eedOf(x): [{app, items}] or None
    pub fn eed_of(&mut self, x: Option<&V>) -> Option<V> {
        let o = match x {
            Some(V::Obj(o)) => o.clone(),
            Some(V::Arr(a)) => {
                let mut o = Obj::new();
                for (i, v) in a.iter().enumerate() {
                    o.set(i.to_string(), v.clone());
                }
                o
            }
            _ => return None,
        };
        let mut out = Vec::new();
        for (app, items) in js_entries(&o) {
            if let V::Arr(a) = items
                && !a.is_empty()
            {
                let h = self.add_app(app, None);
                out.push(V::Obj(obj! { "app" => h, "items" => items.clone() }));
            }
        }
        if out.is_empty() { None } else { Some(V::Arr(out)) }
    }

    /// write.js color(c)
    pub fn color(&mut self, c: Option<&V>) -> V {
        let c = match nn(c) {
            None => V::Obj(obj! { "index" => 256 }),
            Some(V::Num(n)) => V::Obj(obj! { "index" => *n }),
            Some(x) => x.clone(),
        };
        if let Some(rgb) = field(Some(&c), "rgb").cloned()
            && !self.r2004
        {
            self.lost_rgb = self.lost_rgb.saturating_add(1);
            return V::Obj(obj! { "index" => aci_from_rgb(to_number(Some(&rgb))) });
        }
        c
    }
    /// common(e, t, owner): the common entity fields of a record.
    pub fn common(&mut self, e: &Obj, t: &str, owner: f64) -> Rec {
        let h = self.next();
        let layer = self.layer_ref(e.get("layer"));
        let ltype = self.ltype_ref_v(e.get("linetype"));
        let color = self.color(e.get("color"));
        let tr = transparency(e.get("transparency"));
        let ltscale = num(e.get("ltscale"), 1.0);
        let lw = lw_index(e.get("lineweight"));
        let invisible = truthy(e.get("invisible"));
        let eed = self.eed_of(e.get("xdata"));
        let mut o = obj! { "t" => t, "h" => h, "owner" => owner, "layer" => layer };
        o.set("ltype", ltype);
        o.set("color", color);
        o.set_opt("transparency", tr.map(V::Num));
        o.set("ltscale", ltscale);
        o.set("lw", lw);
        o.set("invisible", invisible);
        o.set_opt("eed", eed);
        Rec::new(o)
    }

    fn skip(&mut self, ty: &str) {
        let n = num(self.skipped_n.get(ty), 0.0) + 1.0;
        self.skipped_n.set(ty.to_string(), n);
    }
    fn count(&mut self, ty: &str) {
        let n = num(self.counts.get(ty), 0.0) + 1.0;
        self.counts.set(ty.to_string(), n);
    }

    // ---------------------------------------------------------------- the run
    fn run(&mut self) -> Result<(Vec<u8>, Obj), String> {
        let model = self.model;
        let hdr_in = model.obj("header").cloned().unwrap_or_default();
        // handles
        self.h.block_ctl = self.next();
        self.h.layer_ctl = self.next();
        self.h.style_ctl = self.next();
        self.h.ltype_ctl = self.next();
        self.h.view_ctl = self.next();
        self.h.ucs_ctl = self.next();
        self.h.vport_ctl = self.next();
        self.h.appid_ctl = self.next();
        self.h.dimstyle_ctl = self.next();
        self.h.vpent_ctl = self.next();
        self.h.nod = self.next();
        self.h.group_dict = self.next();
        self.h.mls_dict = self.next();
        self.h.layout_dict = self.next();
        self.h.plot_style_dict = self.next();
        self.h.plot_style_normal = self.next();
        self.h.lt_by_block = self.next();
        self.h.lt_by_layer = self.next();
        self.h.lt_continuous = self.next();
        self.h.layer0 = self.next();
        self.h.mlstyle = self.next();
        self.h.vport = self.next();

        // linetypes
        let tables = model.obj("tables").cloned().unwrap_or_default();
        self.reserve("ltype", "ByBlock");
        self.reserve("ltype", "ByLayer");
        self.reserve("ltype", "Continuous");
        self.lt_by_name.insert("BYBLOCK".into(), self.h.lt_by_block);
        self.lt_by_name.insert("BYLAYER".into(), self.h.lt_by_layer);
        self.lt_by_name.insert("CONTINUOUS".into(), self.h.lt_continuous);
        for lt in arr(tables.get("linetypes")) {
            let lto = lt.as_obj().cloned().unwrap_or_default();
            let up = (if truthy(lto.get("name")) { js_string(lto.get("name")) } else { String::new() }).to_uppercase();
            if let Some(h) = self.lt_by_name.get(&up).copied() {
                self.map_src(lto.get("handle"), h);
                continue;
            }
            let name = self.clean_name("ltype", lto.get("name"), "Linetype");
            let h = self.next();
            self.map_src(lto.get("handle"), h);
            self.lt_by_name.insert(up, h);
            self.lt_by_name.insert(name.to_uppercase(), h);
            let elements: Vec<V> = arr(lto.get("elements"))
                .iter()
                .map(|e| {
                    let e = Some(e);
                    let mut x = obj! { "length" => num(crate::w_js::length_of(e).as_ref(), 0.0), "shape" => int32(field(e, "shape")) };
                    x.set_opt("offset", field(e, "offset").cloned());
                    x.set_opt("scale", field(e, "scale").cloned());
                    x.set_opt("rotation", field(e, "rotation").cloned());
                    x.set("flags", 0);
                    V::Obj(x)
                })
                .collect();
            let mut o = obj! { "t" => "LTYPE", "h" => h, "owner" => self.h.ltype_ctl, "name" => name };
            o.set(
                "description",
                if truthy(lto.get("description")) { lto.get("description").cloned().unwrap_or(V::Null) } else { V::from("") },
            );
            o.set_opt("patternLength", lto.get("patternLength").cloned());
            o.set("elements", elements);
            self.ltypes.push(Rec::new(o));
        }

        // text styles
        let styles_in = arr(tables.get("styles"));
        let std_style = styles_in
            .iter()
            .find(|s| eq_ci(&js_string(field(Some(s), "name")), "standard"))
            .and_then(V::as_obj)
            .cloned()
            .unwrap_or_else(|| obj! { "name" => "Standard" });
        self.add_style(&std_style);
        for s in styles_in {
            if truthy(field(Some(s), "name"))
                && let Some(so) = s.as_obj()
            {
                self.add_style(so);
            }
        }

        // layers
        for b in arr(model.get("blocks")) {
            if int32(field(Some(b), "flags")) & 4 != 0 && truthy(field(Some(b), "name")) {
                self.xref_block_names.insert(js_string(field(Some(b), "name")).to_uppercase());
            }
        }
        let layers_in = arr(tables.get("layers"));
        let l0 = layers_in
            .iter()
            .find(|l| matches!(field(Some(l), "name"), Some(V::Str(s)) if s == "0"))
            .and_then(V::as_obj)
            .cloned()
            .unwrap_or_else(|| obj! { "name" => "0", "color" => obj! { "index" => 7 } });
        self.add_layer(&l0);
        for l in layers_in {
            let lo = l.as_obj().cloned().unwrap_or_default();
            self.add_layer(&lo);
        }

        // registered applications
        self.add_app("ACAD", None);
        for a in arr(tables.get("appids")) {
            if truthy(field(Some(a), "name")) {
                let n = js_string(field(Some(a), "name"));
                self.add_app(&n, field(Some(a), "handle"));
            }
        }

        // dimension styles
        let dims_in = arr(tables.get("dimstyles"));
        let std_dim = dims_in
            .iter()
            .find(|d| eq_ci(&js_string(field(Some(d), "name")), "standard"))
            .and_then(V::as_obj)
            .cloned()
            .unwrap_or_else(|| obj! { "name" => "Standard" });
        self.add_dim(&std_dim);
        for d in dims_in {
            if truthy(field(Some(d), "name"))
                && let Some(dobj) = d.as_obj()
            {
                self.add_dim(dobj);
            }
        }

        // named views (20.4.60)
        for v in arr(tables.get("views")) {
            let Some(vo) = v.as_obj() else { continue };
            if !truthy(vo.get("name")) {
                continue;
            }
            let h = self.next();
            self.map_src(vo.get("handle"), h);
            let name = self.clean_name("view", vo.get("name"), "View");
            let mut o = vo.clone();
            o.set("t", "VIEW");
            o.set("h", h);
            o.set("owner", self.h.view_ctl);
            o.set("name", name);
            o.remove("reactors");
            o.remove("xdic");
            o.remove("eed");
            self.views.push(Rec::new(o));
        }
        // named UCSs (20.4.62)
        for u in arr(tables.get("ucs")) {
            let p3 = |p: Option<&V>| if ok_p(p) { Some(p3_of(p)) } else { None };
            let (o, xa, ya) = (p3(field(Some(u), "origin")), p3(field(Some(u), "xAxis")), p3(field(Some(u), "yAxis")));
            let (Some(o), Some(xa), Some(ya)) = (o, xa, ya) else { continue };
            if !u.truthy() || !truthy(field(Some(u), "name")) {
                continue;
            }
            let h = self.next();
            self.map_src(field(Some(u), "handle"), h);
            let name = self.clean_name("ucs", field(Some(u), "name"), "UCS");
            let mut r = obj! { "t" => "UCS", "h" => h, "owner" => self.h.ucs_ctl, "name" => name };
            r.set("origin", o);
            r.set("xAxis", xa);
            r.set("yAxis", ya);
            r.set("elevation", num(field(Some(u), "elevation"), 0.0));
            self.ucs_list.push(Rec::new(r));
        }
        // the *Active viewports
        let corner = |p: Option<&V>| {
            ok_p(p) && {
                let x = num(field(p, "x"), -1.0);
                let y = num(field(p, "y"), -1.0);
                (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y)
            }
        };
        let act_in: Vec<Obj> = arr(tables.get("vports"))
            .iter()
            .filter(|v| {
                let v = Some(*v);
                v.is_some_and(V::truthy)
                    && eq_ci(&(if truthy(field(v, "name")) { js_string(field(v, "name")) } else { String::new() }), "*active")
                    && corner(field(v, "lowerLeft"))
                    && corner(field(v, "upperRight"))
                    && num(field(field(v, "upperRight"), "x"), 0.0) > num(field(field(v, "lowerLeft"), "x"), 0.0)
                    && num(field(field(v, "upperRight"), "y"), 0.0) > num(field(field(v, "lowerLeft"), "y"), 0.0)
            })
            .filter_map(|v| v.as_obj().cloned())
            .collect();
        if (2..=4).contains(&act_in.len()) {
            for (i, v) in act_in.iter().enumerate() {
                let h = if i > 0 { self.next() } else { self.h.vport };
                self.actives.push((Some(v.clone()), h));
            }
        } else if act_in.len() == 1 && num(act_in.first().and_then(|v| v.get("twist")), 0.0) != 0.0 {
            self.actives.push((act_in.first().cloned(), self.h.vport));
        } else {
            self.actives.push((None, self.h.vport));
        }

        // blocks (records + their entities)
        // the blocks without their entities (those stay in the model: no copy of the drawing)
        let shallow = |b: &'m V| -> (Obj, Option<&'m V>) {
            let mut o = Obj::new();
            let mut ents = None;
            if let Some(bo) = b.as_obj() {
                for (k, v) in bo.iter() {
                    if k == "entities" {
                        ents = Some(v);
                    } else {
                        o.0.push((std::borrow::Cow::Owned(k.to_string()), v.clone()));
                    }
                }
            }
            (o, ents)
        };
        let mut blocks_in: Vec<(Obj, Option<&V>)> = arr(model.get("blocks")).iter().map(shallow).collect();
        let name_of = |b: &Obj| if truthy(b.get("name")) { js_string(b.get("name")) } else { String::new() };
        let is_ms = |b: &(Obj, Option<&V>)| eq_ci(&name_of(&b.0), "*model_space");
        let is_ps = |b: &(Obj, Option<&V>)| is_ps_name(&name_of(&b.0));
        if !blocks_in.iter().any(is_ms) {
            blocks_in.insert(0, (obj! { "name" => "*Model_Space" }, None));
        }
        if !blocks_in.iter().any(is_ps) {
            let at = 1.min(blocks_in.len());
            blocks_in.insert(at, (obj! { "name" => "*Paper_Space" }, None));
        }
        let ms_b = blocks_in.iter().find(|b| is_ms(b)).cloned().unwrap_or_default();
        let mut ps_bs: Vec<(Obj, Option<&V>)> = blocks_in.iter().filter(|b| is_ps(b)).cloned().collect();
        js_sort(&mut ps_bs, |a, b| {
            if eq_ci(&js_string(a.0.get("name")), "*paper_space") {
                -1.0
            } else if eq_ci(&js_string(b.0.get("name")), "*paper_space") {
                1.0
            } else {
                0.0
            }
        });
        let others: Vec<(Obj, Option<&V>)> = blocks_in.iter().filter(|b| !is_ms(b) && !is_ps(b)).cloned().collect();
        let mut block_recs: HashMap<Option<String>, Obj> = HashMap::new();
        for r in arr(tables.get("blockRecords")) {
            block_recs.insert(hex_key(field(Some(r), "handle")), r.as_obj().cloned().unwrap_or_default());
        }
        self.ms = self.add_block(ms_b.0, ms_b.1, Kind::Model, Some("*Model_Space"));
        for (i, b) in ps_bs.into_iter().enumerate() {
            let forced = if i > 0 { format!("*Paper_Space{}", i.saturating_sub(1)) } else { "*Paper_Space".to_string() };
            let k = self.add_block(b.0, b.1, if i > 0 { Kind::Paper2 } else { Kind::Paper }, Some(&forced));
            self.pss.push(k);
        }
        for b in others {
            if !self.block_by_name.contains_key(&js_string(b.0.get("name")).to_uppercase()) {
                self.add_block(b.0, b.1, Kind::Block, None);
            }
        }
        for b in &mut self.blocks {
            if let Some(rr) = block_recs.get(&hex_key(b.src.get("record"))) {
                b.rec_src = Some(rr.clone());
            }
        }

        // layouts: from the model, else Model + Layout1
        self.build_layouts();

        // builders M, T, S (nongraphic-write.js, table-write.js, solid-write.js)
        self.x_init();
        self.tbl_init();

        // raster image definitions (model.objects IMAGEDEF)
        for o in arr(model.get("objects")) {
            if matches!(field(Some(o), "type"), Some(V::Str(s)) if s == "IMAGEDEF")
                && let (Some(k), Some(oo)) = (hex_key(field(Some(o), "handle")), o.as_obj())
            {
                self.image_defs.insert(k, oo.clone());
            }
        }

        // allocate block handles and convert their entities
        let mut invalid = 0u32;
        for bi in 0..self.blocks.len() {
            let hdr = self.next();
            let blk = self.next();
            // the entities leave the block's source (nothing reads them there afterwards): no copy of the drawing
            let (kind, src, ents) = match self.blocks.get_mut(bi) {
                Some(b) => {
                    b.hdr = hdr;
                    b.block = blk;
                    (b.kind, b.src.clone(), b.ents_src)
                }
                None => continue,
            };
            self.map_src(src.get("record"), hdr);
            self.map_src(src.get("handle"), blk);
            let mode = match kind {
                Kind::Model => 2.0,
                Kind::Paper => 1.0,
                _ => 0.0,
            };
            let empty = Obj::new();
            for e in arr(ents) {
                let eo = e.as_obj().unwrap_or(&empty);
                let cv = self.convert(eo, hdr);
                match cv {
                    Conv::Invalid => invalid = invalid.saturating_add(1),
                    Conv::Skip(s) => self.skip(&s),
                    Conv::Recs(mut out) => {
                        if let Some(first) = out.first_mut() {
                            first.o.set("mode", mode);
                            if mode == 0.0 {
                                first.o.set("owner", hdr);
                            }
                        }
                        let h0 = out.first().map_or(0.0, Rec::h);
                        let t0 = out.first().map(|r| r.t().to_string()).unwrap_or_default();
                        self.map_src(eo.get("handle"), h0);
                        if let Some(b) = self.blocks.get_mut(bi) {
                            b.ents.push(out);
                        }
                        self.count(&t0);
                    }
                }
            }
            let end = self.next();
            if let Some(b) = self.blocks.get_mut(bi) {
                b.endblk = end;
            }
            self.map_src(src.get("endHandle"), end);
        }
        if invalid > 0 {
            self.notes.push(format!("{invalid} item{} left out (empty or not drawable).", if invalid == 1 { " was" } else { "s were" }));
        }
        let skipped = self.skipped_n.clone();
        for (t, n) in js_entries(&skipped) {
            let n = num(Some(n), 0.0);
            let ns = crate::w_js::num_to_string(n);
            if t.contains(' ') {
                self.notes.push(format!("{ns} × {t}: not written."));
            } else {
                self.notes.push(format!(
                    "{ns} {t} {} not written (not supported by the writer yet).",
                    if n == 1.0 { "entity was" } else { "entities were" }
                ));
            }
        }
        self.skipped = skipped;
        if self.renamed_layers_n > 0 {
            let n = self.renamed_layers_n;
            self.notes.push(format!("{n} layer name{} changed to fit DWG naming rules (a linked drawing's \"Xref|Layer\" becomes \"Xref$0$Layer\", as when AutoCAD binds it).", if n == 1 { " was" } else { "s were" }));
        }
        if self.renamed_blocks_n > 0 {
            let n = self.renamed_blocks_n;
            self.notes.push(format!("{n} block name{} changed to fit DWG naming rules.", if n == 1 { " was" } else { "s were" }));
        }

        // extents (model space)
        let ext = self.extents(&hdr_in);

        // records for the fixed objects
        self.push_fixed(ext);

        self.tbl_finish();
        self.x_finish();
        self.sol_finish();
        self.surf_finish();
        let handseed = self.seed;

        // encode
        let (code, maint) = version_info(self.version);
        let insunits = int32(hdr_in.get("INSUNITS"));
        let metric = if hdr_in.get("MEASUREMENT").is_some() {
            truthy(hdr_in.get("MEASUREMENT"))
        } else {
            ![1, 2, 3, 8, 9, 10].contains(&insunits) && insunits != 0
        };
        // LEADER annotations
        for i in 0..self.recs.len() {
            let Some(r) = self.recs.get(i) else { continue };
            if r.t() == "LEADER" && truthy(r.g("annotationSrc")) {
                let a = self.src(r.g("annotationSrc"));
                if let Some(r) = self.recs.get_mut(i) {
                    r.o.set("annotation", a);
                }
            }
        }
        // xdata handle references (1005 objects, 1003 layers) point into the source drawing: map them to the written
        // handles. An application's block that refers to something not written is dropped.
        let mut dropped_eed = 0u32;
        for i in 0..self.recs.len() {
            let Some(eed) = self.recs.get(i).and_then(|r| r.g("eed")).cloned() else { continue };
            let mut kept = Vec::new();
            for blk in arr(Some(&eed)) {
                let mut items = Vec::new();
                let mut ok = true;
                for it in arr(field(Some(blk), "items")) {
                    let pair = it.as_arr().map(Vec::as_slice).unwrap_or(&[]);
                    let code = pair.first().cloned().unwrap_or(V::Null);
                    let v = pair.get(1);
                    let c = match &code {
                        V::Num(n) => *n,
                        _ => f64::NAN,
                    };
                    if c == 1005.0 || c == 1003.0 {
                        if hex_key(v).as_deref() == Some("0") {
                            items.push(V::Arr(vec![code.clone(), V::from("0")]));
                            continue;
                        }
                        let n = self.src(v);
                        if n == 0.0 {
                            dropped_eed = dropped_eed.saturating_add(1);
                            ok = false;
                            break;
                        }
                        items.push(V::Arr(vec![code.clone(), V::Str(hex_upper(n))]));
                    } else {
                        let mut p = vec![code.clone()];
                        p.push(v.cloned().unwrap_or(V::Null));
                        items.push(V::Arr(p));
                    }
                }
                if ok {
                    let mut b = blk.as_obj().cloned().unwrap_or_default();
                    b.set("items", items);
                    kept.push(V::Obj(b));
                }
            }
            if let Some(r) = self.recs.get_mut(i) {
                if kept.is_empty() {
                    r.o.remove("eed");
                } else {
                    r.o.set("eed", kept);
                }
            }
        }
        if dropped_eed > 0 {
            self.notes.push(crate::w_msg::msg("ccdwg.write.eed_dropped", &[("n", &dropped_eed.to_string())]));
        }
        // the environment for the encoders
        let block_hdr: Vec<f64> = self.blocks.iter().map(|b| b.hdr).collect();
        let model_classes = self.x.cls_by_num.clone();
        let mut recs = std::mem::take(&mut self.recs);
        let mut objects: Vec<(f64, Vec<u8>)> = Vec::with_capacity(recs.len());
        let mut entities = 0u32;
        let mut internal = Vec::new();
        let classes;
        {
            let env = Env {
                src_map: &self.src_map,
                block_by_name: &self.block_by_name,
                block_hdr: &block_hdr,
                style_by_name: &self.style_by_name,
                lt_by_name: &self.lt_by_name,
                model_classes: &model_classes,
            };
            let mut cx = Cx { version: self.version, ver: self.ver, classes: Classes::new(), env };
            for r in &mut recs {
                match encode_object(r, &mut cx) {
                    Some(data) => {
                        objects.push((r.h(), data));
                        if !is_table_type(r.t()) {
                            entities = entities.saturating_add(1);
                        }
                    }
                    None => internal.push(format!("internal: no encoder for {}", r.t())),
                }
            }
            classes = cx.classes;
        }
        self.notes.extend(internal);
        objects.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut hv = hdr_in.clone();
        if hv.get("INSUNITS").is_none() {
            hv.set("INSUNITS", 0);
        }
        if hv.get("LUPREC").is_none() {
            hv.set("LUPREC", if metric { 3 } else { 2 });
        }
        let hh = self.header_handles(&hdr_in);
        let guid1 = self.guids.guid_upper();
        let guid2 = self.guids.guid_upper();
        let hctx =
            crate::w_objects::HeaderCtx { version: self.version, metric, created: self.created, handseed, extents: ext, guid1, guid2 };
        let header_bytes = encode_header(&hh, &hv, &hctx);
        let class_bytes = encode_classes(&classes, self.version, code, maint);
        let jd = Jds { created: julian(self.created), updated: julian(self.created) };
        let parts = Parts {
            header: header_bytes,
            classes: class_bytes,
            objects,
            measurement: metric,
            jd,
            handseed,
            keys: vec![
                handseed,
                self.h.block_ctl,
                self.h.layer_ctl,
                self.h.style_ctl,
                self.h.ltype_ctl,
                self.h.view_ctl,
                self.h.ucs_ctl,
                self.h.vport_ctl,
                self.h.appid_ctl,
                self.h.dimstyle_ctl,
                self.h.vpent_ctl,
                self.h.nod,
                self.h.mls_dict,
                self.h.group_dict,
            ],
            acds: self.sol_data_storage(),
        };
        let n_objects = parts.objects.len();
        let bytes = match self.version {
            "AC1015" => layout_r2000(&parts)?,
            "AC1021" => crate::w_r2007::layout_r2007(&parts)?,
            v => layout_r2004(&parts, v)?,
        };
        if self.lost_rgb > 0 {
            let n = self.lost_rgb;
            self.notes.push(crate::w_msg::msg("ccdwg.write.true_colours", &[("n", &n.to_string())]));
        }
        let lost = LOST.with(std::cell::Cell::get);
        if lost > 0 {
            self.notes.push(crate::w_msg::msg("ccdwg.write.chars_lost", &[("n", &lost.to_string())]));
        }
        let mut report = obj! { "version" => self.version };
        report.set("notes", self.notes.iter().map(|s| V::from(s.as_str())).collect::<Vec<V>>());
        report.set("counts", std::mem::take(&mut self.counts));
        report.set("skipped", std::mem::take(&mut self.skipped));
        report.set("layers", std::mem::take(&mut self.report_layers));
        let handles = Obj(std::mem::take(&mut self.handles).into_iter().map(|(k, v)| (std::borrow::Cow::Owned(k), V::Str(v))).collect());
        report.set("handles", handles);
        report.set(
            "renamed",
            obj! {
                "layers" => std::mem::take(&mut self.renamed_layers),
                "blocks" => std::mem::take(&mut self.renamed_blocks),
                "layouts" => std::mem::take(&mut self.renamed_layouts),
            },
        );
        report.set("objects", n_objects);
        report.set("entities", entities);
        if let Some(t) = self.report_tables.take() {
            report.set("tables", t);
        }
        if let Some(c) = self.report_copied.take() {
            report.set("copied", c);
        }
        Ok((bytes, report))
    }

    fn add_block(&mut self, src: Obj, ents_src: Option<&'m V>, kind: Kind, forced: Option<&str>) -> usize {
        let src_name_s = js_string(src.get("name"));
        let name = if let Some(f) = forced {
            self.reserve("block", f);
            f.to_string()
        } else if (if truthy(src.get("name")) { js_string(src.get("name")) } else { String::new() }).starts_with('*') {
            let rest: String = src_name_s.chars().skip(1).collect();
            let n = format!("*{}", self.clean_name("anonymous", Some(&V::Str(rest)), "U"));
            if n != src_name_s {
                self.renamed_blocks.set(src_name_s.clone(), n.clone());
            }
            n
        } else {
            let n = self.clean_name("block", src.get("name"), "Block");
            if Some(&V::Str(n.clone())) != src.get("name") {
                self.renamed_blocks_n = self.renamed_blocks_n.saturating_add(1);
                self.renamed_blocks.set(src_name_s.clone(), n.clone());
            }
            n
        };
        let i = self.blocks.len();
        self.block_by_name.insert(src_name_s.to_uppercase(), i);
        self.block_by_name.insert(name.to_uppercase(), i);
        self.blocks.push(Block {
            src,
            ents_src,
            name,
            kind,
            hdr: 0.0,
            block: 0.0,
            endblk: 0.0,
            ents: Vec::new(),
            inserts: Vec::new(),
            rec_src: None,
            layout: 0.0,
        });
        i
    }

    fn build_layouts(&mut self) {
        let model = self.model;
        let layouts_in: Vec<Obj> = arr(model.get("layouts")).iter().map(|l| l.as_obj().cloned().unwrap_or_default()).collect();
        let is_model = |l: &Obj| eq_ci(&js_string(l.get("name")), "model");
        let model_l = layouts_in.iter().find(|l| is_model(l)).cloned().unwrap_or_else(|| obj! { "name" => "Model" });
        let ms = self.ms;
        self.layouts.push(Layout { src: model_l, name: "Model".into(), tab: 0.0, b: ms, h: 0.0 });
        let mut paper_ls: Vec<Obj> = layouts_in.iter().filter(|l| !is_model(l)).cloned().collect();
        js_sort(&mut paper_ls, |a, b| num(a.get("tabOrder"), 1.0) - num(b.get("tabOrder"), 1.0));
        let mut used_b: Vec<usize> = Vec::new();
        for (i, l) in paper_ls.into_iter().enumerate() {
            let k = hex_key(l.get("block")).filter(|k| !k.is_empty());
            let mut b = k.and_then(|k| {
                self.pss
                    .iter()
                    .copied()
                    .find(|&bi| self.blocks.get(bi).is_some_and(|x| hex_key(x.src.get("record")).as_deref() == Some(k.as_str())))
            });
            if b.is_none() || b.is_some_and(|x| used_b.contains(&x)) {
                b = self.pss.iter().copied().find(|x| !used_b.contains(x));
            }
            let bi = match b {
                Some(x) => x,
                None => {
                    let nm = format!("*Paper_Space{}", self.pss.len().saturating_sub(1));
                    let x = self.add_block(obj! { "name" => nm.clone() }, None, Kind::Paper2, Some(&nm));
                    self.pss.push(x);
                    x
                }
            };
            used_b.push(bi);
            let lname = self.clean_name("layout", l.get("name"), &format!("Layout{}", i.saturating_add(1)));
            if Some(&V::Str(lname.clone())) != l.get("name") {
                self.renamed_layouts.set(js_string(l.get("name")), lname.clone());
            }
            self.layouts.push(Layout { src: l, name: lname, tab: usize_to_f64(i.saturating_add(1)), b: bi, h: 0.0 });
        }
        if self.layouts.len() == 1 {
            let b = self.pss.first().copied().unwrap_or(0);
            self.layouts.push(Layout { src: obj! { "name" => "Layout1" }, name: "Layout1".into(), tab: 1.0, b, h: 0.0 });
        }
        // make sure the active paper space is the first paper layout's block
        let first = self.layouts.get(1).map_or(0, |l| l.b);
        if self.pss.first() != Some(&first)
            && let Some(i) = self.pss.iter().position(|x| *x == first)
        {
            self.pss.swap(0, i);
        }
        for (i, &bi) in self.pss.clone().iter().enumerate() {
            if let Some(b) = self.blocks.get_mut(bi) {
                b.kind = if i > 0 { Kind::Paper2 } else { Kind::Paper };
                b.name = if i > 0 { format!("*Paper_Space{}", i.saturating_sub(1)) } else { "*Paper_Space".into() };
            }
        }
        for i in 0..self.layouts.len() {
            let h = self.next();
            if let Some(l) = self.layouts.get_mut(i) {
                l.h = h;
            }
        }
        // paper spaces with no layout get one, so every *Paper_Space block is reachable
        for bi in self.pss.clone() {
            if !self.layouts.iter().any(|l| l.b == bi) {
                let n = self.layouts.len();
                let nm = format!("Layout{n}");
                let name = self.clean_name("layout", Some(&V::from(nm.as_str())), "Layout");
                let h = self.next();
                self.layouts.push(Layout { src: obj! { "name" => nm }, name, tab: usize_to_f64(n), b: bi, h });
            }
        }
    }

    fn extents(&self, hdr_in: &Obj) -> Option<crate::w_objects::Bounds> {
        let mut mn = (f64::INFINITY, f64::INFINITY, f64::INFINITY);
        let mut mx = (f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        let mut g = |p: Option<&V>, r: f64| {
            if !ok_p(p) {
                return;
            }
            let x = num(field(p, "x"), 0.0);
            let y = num(field(p, "y"), 0.0);
            let z = num(field(p, "z"), 0.0);
            mn.0 = js_min(mn.0, x - r);
            mx.0 = js_max(mx.0, x + r);
            mn.1 = js_min(mn.1, y - r);
            mx.1 = js_max(mx.1, y + r);
            mn.2 = js_min(mn.2, z);
            mx.2 = js_max(mx.2, z);
        };
        let he = hdr_in.get("EXTMIN");
        let hx = hdr_in.get("EXTMAX");
        let fin = |v: Option<&V>, k: &str| matches!(field(v, k), Some(V::Num(n)) if n.is_finite());
        if truthy(he)
            && truthy(hx)
            && fin(he, "x")
            && fin(he, "y")
            && fin(hx, "x")
            && fin(hx, "y")
            && num(field(he, "x"), 0.0) <= num(field(hx, "x"), 0.0)
            && num(field(he, "y"), 0.0) <= num(field(hx, "y"), 0.0)
            && num(field(he, "x"), 0.0).abs() < 1e19
        {
            g(he, 0.0);
            g(hx, 0.0);
        } else if let Some(msb) = self.blocks.get(self.ms) {
            for list in &msb.ents {
                let Some(r) = list.first() else { continue };
                match r.t() {
                    "LINE" => {
                        g(r.g("start"), 0.0);
                        g(r.g("end"), 0.0);
                    }
                    "CIRCLE" | "ARC" => g(r.g("center"), to_number(r.g("radius"))),
                    "ELLIPSE" => {
                        let m = r.g("majorAxis");
                        g(r.g("center"), num(field(m, "x"), 0.0).hypot(num(field(m, "y"), 0.0)));
                    }
                    "LWPOLYLINE" => {
                        for p in arr(r.g("vertices")) {
                            let mut q = obj! {};
                            q.set_opt("x", field(Some(p), "x").cloned());
                            q.set_opt("y", field(Some(p), "y").cloned());
                            q.set_opt("z", r.g("elevation").cloned());
                            g(Some(&V::Obj(q)), 0.0);
                        }
                    }
                    "POLYLINE_2D" | "POLYLINE_3D" | "POLYLINE_PFACE" | "POLYLINE_MESH" => {
                        for v in list.iter().skip(1) {
                            if truthy(v.g("point")) {
                                g(v.g("point"), 0.0);
                            }
                        }
                    }
                    "POINT" => g(r.g("position"), 0.0),
                    "TEXT" | "MTEXT" | "INSERT" | "MINSERT" => g(r.g("insert"), 0.0),
                    "SOLID" | "TRACE" | "3DFACE" => {
                        for p in arr(r.g("points")) {
                            g(Some(p), 0.0);
                        }
                    }
                    "HATCH" => {
                        for l in arr(r.g("loops")) {
                            if truthy(field(Some(l), "polyline")) {
                                for p in arr(field(field(Some(l), "polyline"), "vertices")) {
                                    g(Some(p), 0.0);
                                }
                            }
                        }
                    }
                    "SPLINE" => {
                        let c = arr(r.g("control"));
                        let list = if c.is_empty() { arr(r.g("fit")) } else { c };
                        for p in list {
                            g(Some(p), 0.0);
                        }
                    }
                    _ => {
                        if truthy(r.g("defPoint")) {
                            g(r.g("defPoint"), 0.0);
                        }
                    }
                }
            }
        }
        if mn.0.is_finite() { Some((mn, mx)) } else { None }
    }

    fn push_fixed(&mut self, ext: Option<crate::w_objects::Bounds>) {
        let hf = self.h.clone();
        // fix block names now that paper spaces are settled
        for i in 0..self.blocks.len() {
            if let Some(b) = self.blocks.get(i) {
                self.block_by_name.insert(b.name.to_uppercase(), i);
            }
        }
        let hs = |v: Vec<f64>| V::Arr(v.into_iter().map(V::Num).collect());
        let user_blocks: Vec<f64> = self.blocks.iter().filter(|b| matches!(b.kind, Kind::Block | Kind::Paper2)).map(|b| b.hdr).collect();
        let ms_hdr = self.blocks.get(self.ms).map_or(0.0, |b| b.hdr);
        let ps0_hdr = self.pss.first().and_then(|&i| self.blocks.get(i)).map_or(0.0, |b| b.hdr);
        let ctl = |t: &str, h: f64, entries: Vec<f64>| obj! { "t" => t, "h" => h, "owner" => 0, "entries" => hs(entries) };
        let mut bc = ctl("BLOCK_CONTROL", hf.block_ctl, user_blocks);
        bc.set("hardEntries", hs(vec![ms_hdr, ps0_hdr]));
        self.push(bc);
        let layer_hs: Vec<f64> = self.layers.iter().map(Rec::h).collect();
        self.push(ctl("LAYER_CONTROL", hf.layer_ctl, layer_hs));
        let style_hs: Vec<f64> = self.styles.iter().map(Rec::h).collect();
        self.push(ctl("STYLE_CONTROL", hf.style_ctl, style_hs));
        let mut lt_entries = vec![hf.lt_continuous];
        lt_entries.extend(self.ltypes.iter().map(Rec::h));
        let mut ltc = ctl("LTYPE_CONTROL", hf.ltype_ctl, lt_entries);
        ltc.set("count", usize_to_f64(self.ltypes.len()) + 1.0);
        ltc.set("hardEntries", hs(vec![hf.lt_by_layer, hf.lt_by_block]));
        // JS key order: t, h, owner, count, entries, hardEntries (the encoder reads by name)
        self.push(ltc);
        let view_hs: Vec<f64> = self.views.iter().map(Rec::h).collect();
        self.push(ctl("VIEW_CONTROL", hf.view_ctl, view_hs));
        let ucs_hs: Vec<f64> = self.ucs_list.iter().map(Rec::h).collect();
        self.push(ctl("UCS_CONTROL", hf.ucs_ctl, ucs_hs));
        let act_hs: Vec<f64> = self.actives.iter().map(|a| a.1).collect();
        self.push(ctl("VPORT_CONTROL", hf.vport_ctl, act_hs));
        let app_hs: Vec<f64> = self.appids.iter().map(Rec::h).collect();
        self.push(ctl("APPID_CONTROL", hf.appid_ctl, app_hs));
        let dim_hs: Vec<f64> = self.dimstyles.iter().map(Rec::h).collect();
        self.push(ctl("DIMSTYLE_CONTROL", hf.dimstyle_ctl, dim_hs));
        if !self.r2004 {
            self.push(ctl("VPENT_CONTROL", hf.vpent_ctl, Vec::new()));
        }
        // dictionaries
        let mut layout_items: Vec<(String, f64)> = self.layouts.iter().map(|l| (l.name.clone(), l.h)).collect();
        js_sort(&mut layout_items, |a, b| if js_lt(&a.0, &b.0) { -1.0 } else { 1.0 });
        let mut nod_items: Vec<(String, f64)> = vec![
            ("ACAD_GROUP".into(), hf.group_dict),
            ("ACAD_LAYOUT".into(), hf.layout_dict),
            ("ACAD_MLINESTYLE".into(), hf.mls_dict),
            ("ACAD_PLOTSTYLENAME".into(), hf.plot_style_dict),
        ];
        let mut extra: Vec<Rec> = Vec::new();
        if !self.written_defs.is_empty() {
            let dict_h = self.next();
            let vars_h = self.next();
            nod_items.push(("ACAD_IMAGE_DICT".into(), dict_h));
            nod_items.push(("ACAD_IMAGE_VARS".into(), vars_h));
            let items: Vec<V> =
                self.written_defs.iter().map(|(_, d)| V::Arr(vec![d.g("name").cloned().unwrap_or(V::Null), V::Num(d.h())])).collect();
            extra.push(Rec::new(obj! { "t" => "DICTIONARY", "h" => dict_h, "owner" => hf.nod, "reactors" => hs(vec![hf.nod]), "items" => items, "hardOwner" => true }));
            extra.push(Rec::new(obj! { "t" => "RASTERVARIABLES", "h" => vars_h, "owner" => hf.nod, "reactors" => hs(vec![hf.nod]) }));
            for (_, d) in std::mem::take(&mut self.written_defs) {
                let mut d = d;
                d.o.set("owner", dict_h);
                let mut rs = arr(d.g("reactors")).to_vec();
                rs.push(V::Num(dict_h));
                d.o.set("reactors", rs);
                extra.push(d);
            }
        }
        if self.need_wipeout_vars {
            let h = self.next();
            nod_items.push(("ACAD_WIPEOUT_VARS".into(), h));
            extra.push(Rec::new(obj! { "t" => "WIPEOUTVARIABLES", "h" => h, "owner" => hf.nod, "reactors" => hs(vec![hf.nod]) }));
        }
        js_sort(&mut nod_items, |a, b| if js_lt(&a.0, &b.0) { -1.0 } else { 1.0 });
        let items = |v: &[(String, f64)]| V::Arr(v.iter().map(|(n, h)| V::Arr(vec![V::from(n.as_str()), V::Num(*h)])).collect());
        self.push(obj! { "t" => "DICTIONARY", "h" => hf.nod, "owner" => 0, "items" => items(&nod_items) });
        for r in extra {
            self.recs.push(r);
        }
        self.push(obj! { "t" => "DICTIONARY", "h" => hf.group_dict, "owner" => hf.nod, "reactors" => hs(vec![hf.nod]), "items" => Vec::<V>::new() });
        self.push(obj! { "t" => "DICTIONARY", "h" => hf.layout_dict, "owner" => hf.nod, "reactors" => hs(vec![hf.nod]), "items" => items(&layout_items) });
        self.push(obj! { "t" => "DICTIONARY", "h" => hf.mls_dict, "owner" => hf.nod, "reactors" => hs(vec![hf.nod]), "items" => items(&[("Standard".into(), hf.mlstyle)]) });
        self.push(obj! { "t" => "DICTIONARYWDFLT", "h" => hf.plot_style_dict, "owner" => hf.nod, "reactors" => hs(vec![hf.nod]), "items" => items(&[("Normal".into(), hf.plot_style_normal)]), "defaultEntry" => hf.plot_style_normal });
        self.push(obj! { "t" => "PLACEHOLDER", "h" => hf.plot_style_normal, "owner" => hf.plot_style_dict, "reactors" => hs(vec![hf.plot_style_dict]) });
        self.push(obj! { "t" => "MLINESTYLE", "h" => hf.mlstyle, "owner" => hf.mls_dict, "reactors" => hs(vec![hf.mls_dict]), "name" => "Standard", "ltByLayer" => hf.lt_by_layer });
        // linetypes
        self.push(
            obj! { "t" => "LTYPE", "h" => hf.lt_by_block, "owner" => hf.ltype_ctl, "name" => "ByBlock", "elements" => Vec::<V>::new() },
        );
        self.push(
            obj! { "t" => "LTYPE", "h" => hf.lt_by_layer, "owner" => hf.ltype_ctl, "name" => "ByLayer", "elements" => Vec::<V>::new() },
        );
        self.push(obj! { "t" => "LTYPE", "h" => hf.lt_continuous, "owner" => hf.ltype_ctl, "name" => "Continuous", "description" => "Solid line", "elements" => Vec::<V>::new() });
        for r in self.ltypes.clone() {
            self.recs.push(r);
        }
        for i in 0..self.layers.len() {
            let xref = self.layers.get(i).filter(|l| truthy(l.g("xdep"))).map(|l| js_string(l.g("xrefOf")).to_uppercase());
            if let Some(x) = xref {
                let hdr = self.block_by_name.get(&x).and_then(|&bi| self.blocks.get(bi)).map_or(0.0, |b| b.hdr);
                if let Some(l) = self.layers.get_mut(i) {
                    l.o.set("xrefBlock", hdr);
                }
            }
            if let Some(l) = self.layers.get(i).cloned() {
                self.recs.push(l);
            }
        }
        for r in self.styles.clone() {
            self.recs.push(r);
        }
        for r in self.appids.clone() {
            self.recs.push(r);
        }
        for r in self.dimstyles.clone() {
            self.recs.push(r);
        }
        for r in self.views.clone() {
            self.recs.push(r);
        }
        for r in self.ucs_list.clone() {
            self.recs.push(r);
        }
        // the *Active viewport
        {
            let (emn, emx) = match ext {
                Some((a, b)) => ((a.0, a.1), (b.0, b.1)),
                None => ((0.0, 0.0), (12.0, 9.0)),
            };
            let aspect = 1.5;
            let vh0 = js_max(js_max(emx.1 - emn.1, (emx.0 - emn.0) / aspect), 1e-6) * 1.05;
            let vh = if vh0 == 0.0 || vh0.is_nan() { 1.0 } else { vh0 };
            let c0 = crate::value::p2((emn.0 + emx.0) / 2.0, (emn.1 + emx.1) / 2.0);
            for (v, h) in self.actives.clone() {
                let Some(v) = v else {
                    self.push(obj! { "t" => "VPORT", "h" => h, "owner" => hf.vport_ctl, "name" => "*Active", "viewHeight" => vh, "aspect" => aspect, "center" => c0.clone() });
                    continue;
                };
                let c = if truthy(v.get("center")) { v.get("center") } else { v.get("viewCenter") };
                let hgt = v.get("viewHeight");
                let asp = nn(v.get("aspect")).or_else(|| v.get("aspectRatio"));
                let center = if truthy(c)
                    && matches!(field(c, "x"), Some(V::Num(x)) if x.is_finite())
                    && matches!(field(c, "y"), Some(V::Num(y)) if y.is_finite())
                {
                    crate::value::p2(num(field(c, "x"), 0.0), num(field(c, "y"), 0.0))
                } else {
                    c0.clone()
                };
                let eed = self.eed_of(v.get("xdata"));
                let mut o = obj! { "t" => "VPORT", "h" => h, "owner" => hf.vport_ctl, "name" => "*Active" };
                o.set("viewHeight", if num(hgt, 0.0) > 0.0 { hgt.cloned().unwrap_or(V::Null) } else { V::Num(vh) });
                o.set("aspect", if num(asp, 0.0) > 0.0 { asp.cloned().unwrap_or(V::Null) } else { V::Num(aspect) });
                o.set("center", center);
                o.set_opt("lowerLeft", v.get("lowerLeft").cloned());
                o.set_opt("upperRight", v.get("upperRight").cloned());
                o.set("twist", num(v.get("twist"), 0.0));
                o.set_opt("eed", eed);
                self.push(o);
            }
        }
        // layouts
        for li in 0..self.layouts.len() {
            let Some(l) = self.layouts.get(li).cloned() else { continue };
            let s = &l.src;
            let bh = self.blocks.get(l.b).map_or(0.0, |b| b.hdr);
            let mut o = obj! { "t" => "LAYOUT", "h" => l.h, "owner" => hf.layout_dict, "reactors" => hs(vec![hf.layout_dict]), "name" => l.name.clone(), "tab" => l.tab, "block" => bh };
            o.set_opt("paper", s.get("paper").cloned());
            o.set_opt("plotFlags", s.get("plotFlags").cloned());
            o.set_opt("limits", s.get("limits").cloned());
            o.set_opt("insbase", s.get("insbase").cloned());
            let e = if l.tab == 0.0 {
                ext.map_or(V::Null, |(a, b)| {
                    V::Obj(obj! { "min" => crate::value::p3(a.0, a.1, a.2), "max" => crate::value::p3(b.0, b.1, b.2) })
                })
            } else {
                V::Null
            };
            o.set("extents", e);
            o.set("viewports", Vec::<V>::new());
            o.set("lastViewport", if l.tab == 0.0 { hf.vport } else { 0.0 });
            self.push(o);
            if let Some(b) = self.blocks.get_mut(l.b) {
                b.layout = l.h;
            }
        }
        // blocks and their entities
        for bi in 0..self.blocks.len() {
            let ents = self.blocks.get_mut(bi).map(|x| std::mem::take(&mut x.ents)).unwrap_or_default();
            let Some(b) = self.blocks.get(bi).cloned() else { continue };
            let top_h: Vec<f64> = ents.iter().filter_map(|l| l.first()).map(Rec::h).collect();
            let has_attrs = ents.iter().filter_map(|l| l.first()).any(|r| r.t() == "ATTDEF");
            let kind = b.kind;
            let src = b.rec_src.clone().unwrap_or_default();
            let flags = int32(b.src.get("flags"));
            let is_block = kind == Kind::Block;
            let eed = self.eed_of(src.get("xdata"));
            let mut o = obj! { "t" => "BLOCK_HEADER", "h" => b.hdr, "owner" => hf.block_ctl, "name" => b.name.clone() };
            o.set("anonymous", b.name.starts_with('*') && is_block);
            o.set("hasAttrs", has_attrs);
            o.set("isXref", is_block && flags & 4 != 0);
            o.set("overlaid", is_block && flags & 4 != 0 && flags & 8 != 0);
            o.set_opt("base", b.src.get("base").cloned());
            o.set(
                "xrefPath",
                if is_block && flags & 4 != 0 {
                    if truthy(b.src.get("xrefPath")) { js_string(b.src.get("xrefPath")) } else { String::new() }
                } else {
                    String::new()
                },
            );
            o.set(
                "description",
                if truthy(b.src.get("description")) { b.src.get("description").cloned().unwrap_or(V::Null) } else { V::from("") },
            );
            o.set("owned", hs(top_h.clone()));
            o.set("inserts", hs(if is_block { b.inserts.clone() } else { Vec::new() }));
            o.set("block", b.block);
            o.set("endblk", b.endblk);
            o.set("layout", b.layout);
            o.set("insertUnits", int32(src.get("insertUnits")));
            o.set_opt("explodable", src.get("explodable").cloned());
            o.set_opt("scalable", src.get("scalable").cloned());
            o.set_opt("eed", eed);
            self.push(o);
            let block_mode = match kind {
                Kind::Model => 2.0,
                Kind::Paper => 1.0,
                _ => 0.0,
            };
            let mut bo = obj! { "t" => "BLOCK", "h" => b.block, "mode" => block_mode, "owner" => if block_mode != 0.0 { 0.0 } else { b.hdr }, "layer" => hf.layer0 };
            bo.set("ltype", obj! { "flag" => 0 });
            bo.set("color", obj! { "index" => 256 });
            bo.set("lw", 29);
            bo.set("name", b.name.clone());
            if !self.r2004 {
                bo.set("prev", 0);
                bo.set("next", 0);
            }
            self.push(bo);
            let n_top = top_h.len();
            let block_hdrs: Vec<f64> = self.blocks.iter().map(|x| x.hdr).collect();
            for (i, list) in ents.into_iter().enumerate() {
                let mut it = list.into_iter();
                let Some(mut head) = it.next() else { continue };
                let subs: Vec<Rec> = it.collect();
                if !self.r2004 {
                    head.o.set("prev", if i > 0 { top_h.get(i.saturating_sub(1)).copied().unwrap_or(0.0) } else { 0.0 });
                    head.o.set(
                        "next",
                        if i.saturating_add(1) < n_top { top_h.get(i.saturating_add(1)).copied().unwrap_or(0.0) } else { 0.0 },
                    );
                }
                match head.block_ref {
                    Some(Some(bi2)) => head.o.set("block", block_hdrs.get(bi2).copied().unwrap_or(0.0)),
                    Some(None) => head.o.set("block", 0),
                    None => {}
                }
                self.recs.push(head);
                // sub-entities: vertices / attribs chain among themselves, SEQEND last
                let chain_h: Vec<f64> = subs.iter().filter(|s| s.t() != "SEQEND").map(Rec::h).collect();
                let mut k = 0usize;
                let mut seqs = Vec::new();
                for mut s in subs {
                    if s.t() == "SEQEND" {
                        seqs.push(s);
                        continue;
                    }
                    if !self.r2004 {
                        s.o.set("prev", if k > 0 { chain_h.get(k.saturating_sub(1)).copied().unwrap_or(0.0) } else { 0.0 });
                        s.o.set(
                            "next",
                            if k.saturating_add(1) < chain_h.len() {
                                chain_h.get(k.saturating_add(1)).copied().unwrap_or(0.0)
                            } else {
                                0.0
                            },
                        );
                    }
                    k = k.saturating_add(1);
                    self.recs.push(s);
                }
                for mut s in seqs {
                    if !self.r2004 {
                        s.o.set("prev", 0);
                        s.o.set("next", 0);
                    }
                    self.recs.push(s);
                }
            }
            let mut eo = obj! { "t" => "ENDBLK", "h" => b.endblk, "mode" => block_mode, "owner" => if block_mode != 0.0 { 0.0 } else { b.hdr }, "layer" => hf.layer0 };
            eo.set("ltype", obj! { "flag" => 0 });
            eo.set("color", obj! { "index" => 256 });
            eo.set("lw", 29);
            if !self.r2004 {
                eo.set("prev", 0);
                eo.set("next", 0);
            }
            self.push(eo);
        }
    }
    pub fn push(&mut self, o: Obj) {
        self.recs.push(Rec::new(o));
    }

    fn header_handles(&self, hdr_in: &Obj) -> HashMap<&'static str, f64> {
        let hf = &self.h;
        let mut m: HashMap<&'static str, f64> = HashMap::new();
        for (k, v) in [
            ("blockCtl", hf.block_ctl),
            ("layerCtl", hf.layer_ctl),
            ("styleCtl", hf.style_ctl),
            ("ltypeCtl", hf.ltype_ctl),
            ("viewCtl", hf.view_ctl),
            ("ucsCtl", hf.ucs_ctl),
            ("vportCtl", hf.vport_ctl),
            ("appidCtl", hf.appid_ctl),
            ("dimstyleCtl", hf.dimstyle_ctl),
            ("vpentCtl", hf.vpent_ctl),
            ("nod", hf.nod),
            ("groupDict", hf.group_dict),
            ("mlsDict", hf.mls_dict),
            ("layoutDict", hf.layout_dict),
            ("plotStyleDict", hf.plot_style_dict),
            ("plotStyleNormal", hf.plot_style_normal),
            ("ltByBlock", hf.lt_by_block),
            ("ltByLayer", hf.lt_by_layer),
            ("ltContinuous", hf.lt_continuous),
            ("layer0", hf.layer0),
            ("mlstyle", hf.mlstyle),
            ("vport", hf.vport),
        ] {
            m.insert(k, v);
        }
        for (k, v) in &self.x.header_handles {
            m.insert(k, *v);
        }
        m.insert("msBlockHdr", self.blocks.get(self.ms).map_or(0.0, |b| b.hdr));
        m.insert("psBlockHdr", self.pss.first().and_then(|&i| self.blocks.get(i)).map_or(0.0, |b| b.hdr));
        let cl = hdr_in.get("CLAYER");
        let clayer = {
            let a = self.layer_by_name.get(&str_or(cl, "0")).and_then(|&i| self.layers.get(i)).map_or(0.0, Rec::h);
            if a != 0.0 {
                a
            } else {
                let b = self.src(cl);
                if b != 0.0 {
                    b
                } else {
                    let c = self.layer_by_new.get(&js_string(cl)).and_then(|&i| self.layers.get(i)).map_or(0.0, Rec::h);
                    if c != 0.0 { c } else { hf.layer0 }
                }
            }
        };
        m.insert("clayer", clayer);
        m.insert("textstyle", self.style_ref(hdr_in.get("TEXTSTYLE")));
        let ct = self.ltype_ref(hdr_in.get("CELTYPE")).1;
        m.insert("celtype", if ct != 0.0 { ct } else { hf.lt_by_layer });
        m.insert("dimstyleCur", self.dim_ref(hdr_in.get("DIMSTYLE")));
        m.insert("cmlstyle", hf.mlstyle);
        m
    }
}

/// Math.max (NaN wins)
pub fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) }
}
/// Math.min (NaN wins)
pub fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) }
}

/// `/CONTROL|^LAYER$|^STYLE$|^LTYPE$|^APPID$|^DIMSTYLE$|^VPORT$|^VIEW$|DICTIONARY|PLACEHOLDER|MLINESTYLE|LAYOUT|BLOCK_HEADER/`
fn is_table_type(t: &str) -> bool {
    t.contains("CONTROL")
        || matches!(t, "LAYER" | "STYLE" | "LTYPE" | "APPID" | "DIMSTYLE" | "VPORT" | "VIEW")
        || t.contains("DICTIONARY")
        || t.contains("PLACEHOLDER")
        || t.contains("MLINESTYLE")
        || t.contains("LAYOUT")
        || t.contains("BLOCK_HEADER")
}

/// write.js transparency(t)
pub fn transparency(t: Option<&V>) -> Option<f64> {
    let t = nn(t)?;
    let v = to_u32(to_number(Some(t)));
    if v & 0xFF00_0000 == 0x0200_0000 {
        return Some(f64::from(0x0300_0000 | (v & 0xFF)));
    }
    if v == 0x0100_0000 || v == 0 {
        return Some(f64::from(0x0100_0000u32));
    }
    None
}

/// What convert() returns.
pub enum Conv {
    Invalid,
    Skip(String),
    Recs(Vec<Rec>),
}

#[cfg(test)]
mod tests {
    use super::*;
    fn opts(v: &str) -> WriteOpts {
        WriteOpts { version: v.into(), created_ms: Some(1_791_720_000_000.0), guid: GuidMode::Sequence }
    }
    #[test]
    fn writes_every_version_and_reads_back() {
        let model = obj! {
            "blocks" => vec![V::Obj(obj! { "name" => "*Model_Space", "entities" => vec![
                V::Obj(obj! { "type" => "LINE", "layer" => "Walls", "start" => crate::value::p3(0.0, 0.0, 0.0), "end" => crate::value::p3(10.0, 5.0, 0.0) }),
                V::Obj(obj! { "type" => "TEXT", "insert" => crate::value::p3(1.0, 1.0, 0.0), "height" => 2.5, "value" => "Hello Ω 😀" }),
            ] })],
        };
        for v in VERSIONS {
            let (a, report) = write_dwg_opts(&model, &opts(v)).expect("writes");
            let (b, _) = write_dwg_opts(&model, &opts(v)).expect("writes again");
            assert_eq!(a, b, "{v}: deterministic with a fixed clock and GUIDs");
            assert_eq!(report.str("version"), Some(v));
            let back = crate::read_dwg(&a, &crate::ReadOpts::default());
            assert_eq!(back.str("version"), Some(v));
            assert!(back.obj("report").and_then(|r| r.get("error")).is_none(), "{v} reads back");
            let ents: usize = back.arr("blocks").map_or(0, |bs| bs.iter().map(|b| arr(field(Some(b), "entities")).len()).sum());
            assert_eq!(ents, 2, "{v}");
        }
        assert!(write_dwg_opts(&model, &opts("AC1012")).is_err());
        assert!(write_dwg(&model, "R2018").is_ok_and(|(_, r)| r.str("version") == Some("AC1032")));
    }
    #[test]
    fn js_sort_is_v8s_for_small_arrays() {
        // an inconsistent comparator (two "first" items): V8's binary insertion keeps this order
        let mut a = vec!["b", "*P", "c", "*P", "a"];
        js_sort(&mut a, |x, y| {
            if *x == "*P" {
                -1.0
            } else if *y == "*P" {
                1.0
            } else {
                0.0
            }
        });
        assert_eq!(a, vec!["*P", "*P", "b", "c", "a"]);
        let mut n = vec![3.0, 1.0, 2.0];
        js_sort(&mut n, |x, y| x - y);
        assert_eq!(n, vec![1.0, 2.0, 3.0]);
    }
    #[test]
    fn lineweights_and_colours() {
        assert_eq!(lw_index(None), 29.0);
        assert_eq!(lw_index(Some(&V::Num(-2.0))), 30.0);
        assert_eq!(lw_index(Some(&V::Num(24.0))), 7.0);
        assert_eq!(aci_from_rgb(f64::from(0xFF_0000)), 1.0);
        assert_eq!(transparency(Some(&V::Num(f64::from(0x0200_0080u32)))), Some(f64::from(0x0300_0080u32)));
    }
}
