//! Writing an edited visibility parameter of a dynamic block (port of lib/dwg/dynblock-write.js, 0.5 DB.9). Not in the
//! ODA spec: the BLOCKVISIBILITYPARAMETER layout is a `Spec gap:` found by experiment on AutoCAD's own files
//! (docs/DWG-DYNAMIC-BLOCKS.md; dynblock_read.rs reads the same layout).
//!
//! Spec gap: element header (BL −1, BL release, BL maintenance, BL a 32-bit value, TV name) · B B · BS ×4 · BD3 grip
//! point · BS BS · BL grip node · TV label · TV description · B B · BL n + n handles (every entity) · BL states, each
//! TV name · BL a + a handles (entities shown) · BL b + b handles (parameters and grips shown).
//!
//! Only the states of a parameter already in the file can change (renamed, added, deleted, which entities each shows);
//! every other field is written back as it came. `rewrite_visibility` fails with 'not-exact' when the parameter does not
//! re-encode bit for bit from its own fields first, 'handles' / 'states' when the edit is not valid, 'layout' when the
//! data does not have this layout.

use crate::bits::{BitReader, Codepage};
use crate::obj;
use crate::util::{b64, unb64, usize_to_f64};
use crate::value::{Obj, V};
use crate::w_bits::BitWriter;
use crate::w_js::{arr, field, js_string, nn, num, to_number};

const LIMIT_LIST: i32 = 100_000;
const LIMIT_STATES: i32 = 1000;
const LIMIT_NAME: usize = 255;

/// dynblock-write.js hexKey: `String(h).toUpperCase().replace(/^0+(?=.)/, '')`, '' for undefined/null.
fn hex_key(v: Option<&V>) -> String {
    let s = match nn(v) {
        None => return String::new(),
        x => js_string(x),
    };
    let up = s.to_uppercase();
    let t = up.trim_start_matches('0');
    if t.is_empty() && !up.is_empty() { "0".into() } else { t.to_string() }
}
fn version_of(ver: u32) -> Option<&'static str> {
    Some(match ver {
        15 => "AC1015",
        18 => "AC1018",
        21 => "AC1021",
        24 => "AC1024",
        27 => "AC1027",
        32 => "AC1032",
        _ => return None,
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct VisState {
    pub name: String,
    pub entities: Vec<(f64, String)>,
    pub elements: Vec<(f64, String)>,
}
/// Every field of the parameter, in file order (handles as (code, 'HEX')).
#[derive(Clone, Debug, PartialEq)]
pub struct VisFields {
    pub ver: u32,
    pub id: i32,
    pub release: i32,
    pub maint: i32,
    pub tag: i32,
    pub name: String,
    pub b: [u32; 2],
    pub pad: [i32; 4],
    pub point: [f64; 3],
    pub z: [i32; 2],
    pub grip: i32,
    pub label: String,
    pub description: String,
    pub f2: [u32; 2],
    pub all: Vec<(f64, String)>,
    pub states: Vec<VisState>,
}

fn layout<T>() -> Result<T, String> {
    Err("layout".into())
}
fn count(n: i32, max: i32) -> Result<usize, String> {
    if !(0..=max).contains(&n) {
        return layout();
    }
    Ok(crate::util::usize_from_i32(n))
}

/// visibilityFields(raw)
pub fn visibility_fields(raw: &Obj) -> Result<VisFields, String> {
    let ver = match raw.get("ver") {
        Some(V::Num(v)) => crate::util::to_u32(*v),
        _ => return layout(),
    };
    if version_of(ver).is_none() {
        return layout();
    }
    let (Some(data), Some(hs)) = (raw.str("data"), raw.arr("h")) else { return layout() };
    let buf = unb64(data);
    let n = crate::util::to_usize(num(raw.get("n"), 0.0));
    let mut r = BitReader::range(&buf, ver, Codepage::latin1(), 0, n);
    let mut str_end = None;
    if ver >= 21 {
        match crate::objects::string_stream_start(&buf, n, 0) {
            Some((s, e)) => {
                r.set_str(s, e);
                r.end = s;
                str_end = Some(e);
            }
            None => {
                r.set_str(n, n);
                r.end = n.saturating_sub(1);
                str_end = Some(n);
            }
        }
    }
    let h: Vec<(f64, String)> = hs
        .iter()
        .map(|x| {
            let a = x.as_arr().map(Vec::as_slice).unwrap_or(&[]);
            (to_number(a.first()), hex_key(a.get(1)))
        })
        .collect();
    let mut k = 0usize;
    let mut take = |cnt: usize| -> Result<Vec<(f64, String)>, String> {
        let end = k.checked_add(cnt).filter(|e| *e <= h.len()).ok_or_else(|| "layout".to_string())?;
        let l = h.get(k..end).map(<[(f64, String)]>::to_vec).unwrap_or_default();
        k = end;
        Ok(l)
    };
    let e = |x: crate::bits::Res<i32>| x.map_err(|_| "layout".to_string());
    let eu = |x: crate::bits::Res<u32>| x.map_err(|_| "layout".to_string());
    let es = |x: crate::bits::Res<String>| x.map_err(|_| "layout".to_string());
    let id = e(r.bl())?;
    let release = e(r.bl())?;
    let maint = e(r.bl())?;
    let tag = e(r.bl())?;
    let name = es(r.tv())?;
    let b = [eu(r.b())?, eu(r.b())?];
    let pad = [e(r.bs())?, e(r.bs())?, e(r.bs())?, e(r.bs())?];
    let point = r.bd3_raw().map_err(|_| "layout".to_string())?;
    let z = [e(r.bs())?, e(r.bs())?];
    let grip = e(r.bl())?;
    let label = es(r.tv())?;
    let description = es(r.tv())?;
    let f2 = [eu(r.b())?, eu(r.b())?];
    let na = count(e(r.bl())?, LIMIT_LIST)?;
    let all = take(na)?;
    let mut states = Vec::new();
    let ns = count(e(r.bl())?, LIMIT_STATES)?;
    for _ in 0..ns {
        let name = es(r.tv())?;
        let a = count(e(r.bl())?, LIMIT_LIST)?;
        let entities = take(a)?;
        let bcnt = count(e(r.bl())?, LIMIT_LIST)?;
        let elements = take(bcnt)?;
        states.push(VisState { name, entities, elements });
    }
    let str_left = match (r.str_pos, str_end) {
        (Some((p, e)), Some(_)) => e.saturating_sub(p),
        _ => 0,
    };
    if r.end != r.pos || k != h.len() || str_left != 0 {
        return layout();
    }
    Ok(VisFields { ver, id, release, maint, tag, name, b, pad, point, z, grip, label, description, f2, all, states })
}

/// encodeVisibility(fields, ver) → raw { ver, n, data, h }
pub fn encode_visibility(f: &VisFields, ver: u32) -> Result<Obj, String> {
    let Some(vs) = version_of(ver) else { return layout() };
    let mut w = BitWriter::new(vs);
    let mut s = BitWriter::new(vs);
    let split = ver >= 21;
    let mut handles: Vec<(f64, String)> = Vec::new();
    macro_rules! tv {
        ($x:expr) => {
            if split { s.tv($x) } else { w.tv($x) }
        };
    }
    w.bl(f64::from(f.id));
    w.bl(f64::from(f.release));
    w.bl(f64::from(f.maint));
    w.bl(f64::from(f.tag));
    tv!(&f.name);
    w.b(f.b[0] != 0);
    w.b(f.b[1] != 0);
    for v in f.pad {
        w.bs(f64::from(v));
    }
    w.bd3(f.point[0], f.point[1], f.point[2]);
    for v in f.z {
        w.bs(f64::from(v));
    }
    w.bl(f64::from(f.grip));
    tv!(&f.label);
    tv!(&f.description);
    w.b(f.f2[0] != 0);
    w.b(f.f2[1] != 0);
    w.bl(usize_to_f64(f.all.len()));
    handles.extend(f.all.iter().cloned());
    w.bl(usize_to_f64(f.states.len()));
    for st in &f.states {
        tv!(&st.name);
        w.bl(usize_to_f64(st.entities.len()));
        handles.extend(st.entities.iter().cloned());
        w.bl(usize_to_f64(st.elements.len()));
        handles.extend(st.elements.iter().cloned());
    }
    if split {
        let size = s.bit;
        w.append(&s);
        if size < 0x8000 {
            w.rs_i(crate::util::i32_of_usize(size));
        } else {
            w.rs_i(crate::util::i32_of_usize(size >> 15));
            w.rs_i(crate::util::i32_of_usize((size & 0x7fff) | 0x8000));
        }
        w.b(true);
    }
    let h: Vec<V> = handles.into_iter().map(|(c, x)| V::Arr(vec![V::Num(c), V::Str(x)])).collect();
    Ok(obj! { "ver" => ver, "n" => w.bit, "data" => b64(&w.bytes()), "h" => h })
}

fn bits_of(raw: &Obj) -> Vec<u8> {
    let u = unb64(raw.str("data").unwrap_or(""));
    let n = crate::util::to_usize(num(raw.get("n"), 0.0));
    (0..n).map(|i| (u.get(i >> 3).copied().unwrap_or(0) >> (7usize.wrapping_sub(i & 7))) & 1).collect()
}
fn same_handles(a: &Obj, b: &Obj) -> bool {
    let (x, y) = (arr(a.get("h")), arr(b.get("h")));
    x.len() == y.len()
        && x.iter().zip(y).all(|(p, q)| {
            let (p, q) = (p.as_arr().map(Vec::as_slice).unwrap_or(&[]), q.as_arr().map(Vec::as_slice).unwrap_or(&[]));
            p.first() == q.first() && hex_key(p.get(1)) == hex_key(q.get(1))
        })
}
/// reencodesExactly(raw)
pub fn reencodes_exactly(raw: &Obj) -> bool {
    let Ok(f) = visibility_fields(raw) else { return false };
    let Ok(e) = encode_visibility(&f, f.ver) else { return false };
    e.get("n") == raw.get("n") && bits_of(&e) == bits_of(raw) && same_handles(&e, raw)
}

fn clean(s: Option<&V>) -> String {
    let st: String = crate::w_js::str_or(s, "").chars().filter(|&c| (c as u32) >= 0x20).collect();
    crate::w_write::slice_units(&st, LIMIT_NAME)
}

/// rewriteVisibility(raw, states) → raw with these states:
/// states = [{ name, entities: ['HEX'…], from?: a state name in the file }]
pub fn rewrite_visibility(raw: &Obj, states: &V) -> Result<Obj, String> {
    if !reencodes_exactly(raw) {
        return Err("not-exact".into());
    }
    let mut f = visibility_fields(raw)?;
    let list = match states {
        V::Arr(a) if !a.is_empty() && a.len() <= crate::util::usize_from_i32(LIMIT_STATES) => a,
        _ => return Err("states".into()),
    };
    let code_of = |x: &str| f.all.iter().rev().find(|(_, h)| h == x).map(|(c, _)| *c);
    let at_of = |x: &str| f.all.iter().rposition(|(_, h)| h == x).map_or(f64::NAN, usize_to_f64);
    let state_code = f.states.iter().flat_map(|s| s.entities.iter()).next().map(|e| e.0);
    let mut names: Vec<String> = Vec::new();
    let mut out = Vec::with_capacity(list.len());
    for st in list {
        let name = clean(field(Some(st), "name"));
        if name.is_empty() || names.contains(&name.to_lowercase()) {
            return Err("states".into());
        }
        names.push(name.to_lowercase());
        let mut ents: Vec<String> = Vec::new();
        for x in arr(field(Some(st), "entities")) {
            let k = hex_key(Some(x));
            if !ents.contains(&k) {
                ents.push(k);
            }
        }
        if ents.iter().any(|x| code_of(x).is_none()) {
            return Err("handles".into());
        }
        crate::w_write::js_sort(&mut ents, |a, b| at_of(a) - at_of(b));
        let from = crate::w_js::str_or(nn(field(Some(st), "from")).or(Some(&V::Str(name.clone()))), "").to_lowercase();
        let base = f.states.iter().rev().find(|s| s.name.to_lowercase() == from).or_else(|| f.states.first());
        out.push(VisState {
            entities: ents.iter().map(|x| (state_code.or_else(|| code_of(x)).unwrap_or(0.0), x.clone())).collect(),
            elements: base.map(|b| b.elements.clone()).unwrap_or_default(),
            name,
        });
    }
    f.states = out;
    encode_visibility(&f, f.ver)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(ver: u32) -> VisFields {
        VisFields {
            ver,
            id: -1,
            release: 27,
            maint: 8,
            tag: 4,
            name: "Visibility1".into(),
            b: [1, 0],
            pad: [0, 1, 2, 3],
            point: [1.5, -2.0, 0.0],
            z: [0, 0],
            grip: 7,
            label: "Visibility State".into(),
            description: String::new(),
            f2: [0, 1],
            all: vec![(4.0, "2A".into()), (4.0, "2B".into()), (4.0, "2C".into())],
            states: vec![
                VisState { name: "Open".into(), entities: vec![(4.0, "2A".into())], elements: vec![(4.0, "30".into())] },
                VisState { name: "Closed".into(), entities: vec![(4.0, "2B".into()), (4.0, "2C".into())], elements: vec![] },
            ],
        }
    }
    #[test]
    fn round_trips_and_rewrites() {
        for ver in [15u32, 18, 21, 24, 27, 32] {
            let f = sample(ver);
            let raw = encode_visibility(&f, ver).expect("encodes");
            assert_eq!(visibility_fields(&raw).expect("reads"), f);
            assert!(reencodes_exactly(&raw));
            let states = V::Arr(vec![
                V::Obj(obj! { "name" => "Half", "entities" => vec![V::from("2c"), V::from("2A"), V::from("2A")], "from" => "closed" }),
                V::Obj(obj! { "name" => "Open" }),
            ]);
            let out = rewrite_visibility(&raw, &states).expect("rewrites");
            let g = visibility_fields(&out).expect("reads back");
            assert_eq!(g.states.len(), 2);
            assert_eq!(g.states[0].entities, vec![(4.0, "2A".to_string()), (4.0, "2C".to_string())]);
            assert_eq!(g.states[1].elements, vec![(4.0, "30".to_string())]);
            assert_eq!(
                rewrite_visibility(&raw, &V::Arr(vec![V::Obj(obj! { "name" => "x", "entities" => vec![V::from("99")] })])),
                Err("handles".into())
            );
            assert_eq!(rewrite_visibility(&raw, &V::Arr(vec![])), Err("states".into()));
        }
    }
}
