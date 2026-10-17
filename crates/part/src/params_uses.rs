//! params-uses.js — where each parameter is used: equations that name it (direct), equations that use it through other
//! parameters (via, with the chain), parameters built on it, configurations that set it, and scripts that read it
//! (`param('name')` / `params()`, found in the text — never run).

use crate::expr;
use crate::js;
use crate::jv;
use crate::omap::OMap;
use crate::params::Params;
use crate::params_part;
use serde_json::Value;

fn word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}
fn ws_len(s: &str, i: usize) -> usize {
    s.get(i..).map_or(0, |r| r.chars().take_while(|c| js::is_ws(*c)).map(char::len_utf8).sum())
}

/// `scriptParams(text)` → (names in order, all).
pub fn script_params(text: &str) -> (Vec<String>, bool) {
    let b = text.as_bytes();
    let mut names: Vec<String> = Vec::new();
    let mut all = false;
    let mut i = 0usize;
    while let Some(off) = text.get(i..).and_then(|r| r.find("param")) {
        let at = i.saturating_add(off);
        i = at.saturating_add(1);
        if at > 0 && b.get(at.saturating_sub(1)).copied().is_some_and(word) {
            continue;
        }
        let mut j = at.saturating_add(5);
        // params() (PARAMS_CALL)
        if b.get(j) == Some(&b's') {
            let mut k = j.saturating_add(1);
            k = k.saturating_add(ws_len(text, k));
            if b.get(k) == Some(&b'(') {
                k = k.saturating_add(1);
                k = k.saturating_add(ws_len(text, k));
                if b.get(k) == Some(&b')') {
                    all = true;
                }
            }
            continue;
        }
        // param('name') (PARAM_CALL)
        j = j.saturating_add(ws_len(text, j));
        if b.get(j) != Some(&b'(') {
            continue;
        }
        j = j.saturating_add(1);
        j = j.saturating_add(ws_len(text, j));
        let Some(&q) = b.get(j).filter(|c| **c == b'\'' || **c == b'"') else { continue };
        let start = j.saturating_add(1);
        let nlen = b.iter().skip(start).take_while(|c| word(**c)).count();
        let Some(name) = text.get(start..start.saturating_add(nlen)) else { continue };
        if nlen == 0 || nlen > 40 || !name.as_bytes().first().is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_') {
            continue;
        }
        let mut k = start.saturating_add(nlen);
        if b.get(k) != Some(&q) {
            continue;
        }
        k = k.saturating_add(1);
        k = k.saturating_add(ws_len(text, k));
        if b.get(k) != Some(&b')') {
            continue;
        }
        if !names.iter().any(|n| n == name) {
            names.push(name.to_string());
        }
        i = k.saturating_add(1);
    }
    (names, all)
}

/// One parameter's uses.
#[derive(Clone, Debug, Default)]
pub struct Uses {
    pub direct: Vec<Value>,
    pub via: Vec<Value>,
    pub vars: Vec<String>,
    pub configs: Vec<String>,
    pub scripts: Vec<(String, bool)>,
}
impl Uses {
    pub fn to_json(&self) -> Value {
        jv::obj(vec![
            ("direct", Value::Array(self.direct.clone())),
            ("via", Value::Array(self.via.clone())),
            ("vars", jv::strs(&self.vars)),
            ("configs", jv::strs(&self.configs)),
            (
                "scripts",
                Value::Array(self.scripts.iter().map(|(n, a)| jv::obj(vec![("name", n.as_str().into()), ("all", (*a).into())])).collect()),
            ),
        ])
    }
    /// `usesCount(u)`.
    pub fn count(&self) -> usize {
        self.direct
            .len()
            .saturating_add(self.via.len())
            .saturating_add(self.vars.len())
            .saturating_add(self.configs.len())
            .saturating_add(self.scripts.len())
    }
}

/// `paramUses(part)`.
pub fn param_uses(part: &Value) -> OMap<Uses> {
    let p = part.get("params").filter(|v| jv::truthy(Some(v))).map(Params::from_json).unwrap_or_default();
    let mut out: OMap<Uses> = OMap::new();
    for v in &p.vars {
        out.set(&v.name, Uses::default());
    }
    if out.is_empty() {
        return out;
    }
    let names = out.key_vec();
    let uses_of_expr = |src: &str, out: &OMap<Uses>| -> Vec<String> {
        expr::parse(src, None).map(|r| r.uses.into_iter().filter(|n| out.has(n)).collect()).unwrap_or_default()
    };
    let mut deps: OMap<Vec<String>> = OMap::new();
    for v in &p.vars {
        deps.set(&v.name, uses_of_expr(&v.expr, &out));
    }
    for (n, ds) in deps.iter() {
        for d in ds {
            if d != n
                && let Some(u) = out.get_mut(d)
                && !u.vars.iter().any(|x| x == n)
            {
                u.vars.push(n.to_string());
            }
        }
    }
    let above = |name: &str, out: &OMap<Uses>| -> OMap<Vec<String>> {
        let mut seen: OMap<Vec<String>> = OMap::new();
        let mut stack: Vec<(String, Vec<String>)> = vec![(name.to_string(), Vec::new())];
        let mut guard = 0usize;
        while let Some((n, path)) = stack.pop() {
            guard = guard.saturating_add(1);
            if guard > 1_000_000 {
                break;
            }
            let ups = out.get(&n).map(|u| u.vars.clone()).unwrap_or_default();
            for up in ups {
                if up != name && !seen.has(&up) {
                    let mut np = path.clone();
                    np.push(up.clone());
                    seen.set(&up, np.clone());
                    stack.push((up, np));
                }
            }
        }
        seen
    };
    let tg = params_part::targets(part);
    let describe = |target: &str| -> Value {
        let t = tg.get(target);
        jv::obj(vec![
            ("target", target.into()),
            ("label", t.map_or(target, |x| x.label.as_str()).into()),
            ("feature", t.map_or(Value::Null, |x| x.feature.as_str().into())),
            ("kind", if t.is_some_and(|x| x.constraint.is_some()) { "dimension" } else { "feature" }.into()),
        ])
    };
    let link_uses: Vec<(String, Vec<String>)> = p.links.iter().map(|l| (l.target.clone(), uses_of_expr(&l.expr, &out))).collect();
    for name in &names {
        let up = above(name, &out);
        let mut direct = Vec::new();
        let mut via = Vec::new();
        for (target, uses) in &link_uses {
            if uses.iter().any(|u| u == name) {
                direct.push(describe(target));
            }
        }
        for (target, uses) in &link_uses {
            if uses.iter().any(|u| u == name) {
                continue;
            }
            if let Some(v) = uses.iter().find(|n| up.has(n)) {
                let mut d = describe(target);
                jv::set(&mut d, "through", jv::strs(up.get(v).map_or(&[][..], Vec::as_slice)));
                via.push(d);
            }
        }
        if let Some(u) = out.get_mut(name) {
            u.direct = direct;
            u.via = via;
        }
    }
    if let Some(rows) = part.get("configs").and_then(|c| c.get("rows")).and_then(Value::as_array) {
        for row in rows {
            if let Some(vals) = row.get("values").and_then(Value::as_object) {
                for n in vals.keys() {
                    if let Some(u) = out.get_mut(n) {
                        u.configs.push(js::str_of(row.get("name")));
                    }
                }
            }
        }
    }
    if let Some(scripts) = part.get("studies").and_then(|s| s.get("script")).and_then(|s| s.get("scripts")).and_then(Value::as_array) {
        for sc in scripts {
            let (rn, all) = script_params(&js::str_of(sc.get("text")));
            for n in &names {
                if all || rn.iter().any(|x| x == n) {
                    let flag = all && !rn.iter().any(|x| x == n);
                    if let Some(u) = out.get_mut(n) {
                        u.scripts.push((js::str_of(sc.get("name")), flag));
                    }
                }
            }
        }
    }
    out
}
