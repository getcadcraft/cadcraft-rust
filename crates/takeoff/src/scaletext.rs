//! `scaletext.js` — a sheet's scale from its own text (T20): the notations drawings use (1/4" = 1'-0", 1" = 20',
//! 1:100), only those that are a preset; the most frequent wins, one written after "SCALE" counts double. The JS
//! regular expressions are matched by a small backtracking matcher over UTF-16 code units with the JS semantics
//! (leftmost match, greedy quantifiers, ordered alternation, ASCII `\d` / `\b`, JS `\s`, `i` = ASCII case folding).

use crate::jv;
use crate::units::{PRESETS, Preset};

// ------------------------------------------------------------------------------------------- a tiny regex matcher

#[derive(Debug, Clone)]
enum Re {
    /// literal text (case-insensitive for ASCII letters when the flag is set)
    Lit(Vec<u16>),
    Digit,
    Space,
    /// any one of these code units
    Set(Vec<u16>),
    /// min, max (None = unbounded), greedy
    Rep(Box<Re>, usize, Option<usize>),
    Seq(Vec<Re>),
    Alt(Vec<Re>),
    Cap(usize, Box<Re>),
    /// `\b`
    Wb,
    End,
    /// `(?!x)`
    NotAhead(Box<Re>),
    /// `(?<!set)`: the code unit before is not in the set
    NotBehindSet(Vec<u16>, bool),
}

fn lit(s: &str) -> Re {
    Re::Lit(s.encode_utf16().collect())
}
fn seq(v: Vec<Re>) -> Re {
    Re::Seq(v)
}
fn alt(v: Vec<Re>) -> Re {
    Re::Alt(v)
}
fn star(r: Re) -> Re {
    Re::Rep(Box::new(r), 0, None)
}
fn plus(r: Re) -> Re {
    Re::Rep(Box::new(r), 1, None)
}
fn opt(r: Re) -> Re {
    Re::Rep(Box::new(r), 0, Some(1))
}
fn cap(i: usize, r: Re) -> Re {
    Re::Cap(i, Box::new(r))
}

struct M<'a> {
    s: &'a [u16],
    ci: bool,
    caps: Vec<Option<(usize, usize)>>,
    steps: usize,
}

const MAX_STEPS: usize = 2_000_000;

fn is_digit(u: u16) -> bool {
    (u16::from(b'0')..=u16::from(b'9')).contains(&u)
}
fn is_word(u: u16) -> bool {
    is_digit(u)
        || (u16::from(b'a')..=u16::from(b'z')).contains(&u)
        || (u16::from(b'A')..=u16::from(b'Z')).contains(&u)
        || u == u16::from(b'_')
}
fn is_sp(u: u16) -> bool {
    char::from_u32(u32::from(u)).is_some_and(jv::is_space)
}
fn fold(u: u16, ci: bool) -> u16 {
    if ci && (u16::from(b'A')..=u16::from(b'Z')).contains(&u) { u.saturating_add(32) } else { u }
}

impl M<'_> {
    /// Match `r` at `i`, then the continuation `k`; true when the whole succeeds.
    fn m(&mut self, r: &Re, i: usize, k: &mut dyn FnMut(&mut Self, usize) -> bool) -> bool {
        self.steps = self.steps.saturating_add(1);
        if self.steps > MAX_STEPS {
            return false;
        }
        match r {
            Re::Lit(l) => {
                let end = i.saturating_add(l.len());
                if end > self.s.len() {
                    return false;
                }
                let ok = l
                    .iter()
                    .enumerate()
                    .all(|(j, c)| self.s.get(i.saturating_add(j)).is_some_and(|u| fold(*u, self.ci) == fold(*c, self.ci)));
                ok && k(self, end)
            }
            Re::Digit => self.s.get(i).is_some_and(|u| is_digit(*u)) && k(self, i.saturating_add(1)),
            Re::Space => self.s.get(i).is_some_and(|u| is_sp(*u)) && k(self, i.saturating_add(1)),
            Re::Set(set) => self.s.get(i).is_some_and(|u| set.contains(u)) && k(self, i.saturating_add(1)),
            Re::Wb => {
                let a = i > 0 && self.s.get(i.saturating_sub(1)).is_some_and(|u| is_word(*u));
                let b = self.s.get(i).is_some_and(|u| is_word(*u));
                a != b && k(self, i)
            }
            Re::End => i == self.s.len() && k(self, i),
            Re::NotAhead(x) => {
                let saved = self.caps.clone();
                let hit = self.m(x, i, &mut |_, _| true);
                self.caps = saved;
                !hit && k(self, i)
            }
            Re::NotBehindSet(set, digit_too) => {
                let prev = if i > 0 { self.s.get(i.saturating_sub(1)).copied() } else { None };
                let bad = prev.is_some_and(|u| set.contains(&u) || (*digit_too && is_digit(u)));
                !bad && k(self, i)
            }
            Re::Seq(v) => self.seq(v, 0, i, k),
            Re::Alt(v) => {
                for x in v {
                    let saved = self.caps.clone();
                    if self.m(x, i, k) {
                        return true;
                    }
                    self.caps = saved;
                }
                false
            }
            Re::Cap(n, x) => {
                let n = *n;
                let saved = self.caps.get(n).copied().flatten();
                let ok = self.m(x, i, &mut |me: &mut Self, j| {
                    let before = me.caps.get(n).copied().flatten();
                    if let Some(c) = me.caps.get_mut(n) {
                        *c = Some((i, j));
                    }
                    if k(me, j) {
                        return true;
                    }
                    if let Some(c) = me.caps.get_mut(n) {
                        *c = before;
                    }
                    false
                });
                if !ok && let Some(c) = self.caps.get_mut(n) {
                    *c = saved;
                }
                ok
            }
            Re::Rep(x, min, max) => self.rep(x, *min, *max, 0, i, k),
        }
    }
    fn seq(&mut self, v: &[Re], idx: usize, i: usize, k: &mut dyn FnMut(&mut Self, usize) -> bool) -> bool {
        match v.get(idx) {
            None => k(self, i),
            Some(r) => self.m(r, i, &mut |me: &mut Self, j| me.seq(v, idx.saturating_add(1), j, k)),
        }
    }
    fn rep(&mut self, x: &Re, min: usize, max: Option<usize>, count: usize, i: usize, k: &mut dyn FnMut(&mut Self, usize) -> bool) -> bool {
        // greedy: one more first (never an empty iteration), then stop
        if max.is_none_or(|mx| count < mx) {
            let tried = self.m(x, i, &mut |me: &mut Self, j| j != i && me.rep(x, min, max, count.saturating_add(1), j, k));
            if tried {
                return true;
            }
        }
        count >= min && k(self, i)
    }
}

/// One match: start, end, captures (UTF-16 ranges).
type Found = (usize, usize, Vec<Option<(usize, usize)>>);

fn exec(re: &Re, s: &[u16], from: usize, ci: bool, ncaps: usize) -> Option<Found> {
    for start in from..=s.len() {
        let mut m = M { s, ci, caps: vec![None; ncaps], steps: 0 };
        let mut end = None;
        if m.m(re, start, &mut |_, j| {
            end = Some(j);
            true
        }) {
            return end.map(|e| (start, e, m.caps));
        }
    }
    None
}
fn match_all(re: &Re, s: &[u16], ci: bool, ncaps: usize) -> Vec<Found> {
    let mut out = Vec::new();
    let mut from = 0usize;
    while from <= s.len() {
        let Some(f) = exec(re, s, from, ci, ncaps) else { break };
        from = if f.1 > f.0 { f.1 } else { f.1.saturating_add(1) };
        out.push(f);
    }
    out
}

// ------------------------------------------------------------------------------------------- the patterns

fn units16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}
fn in_re() -> Re {
    // (?:"|''|″|”|\s?in\.?|\s?inch(?:es)?)
    alt(vec![
        lit("\""),
        lit("''"),
        lit("″"),
        lit("”"),
        seq(vec![opt(Re::Space), lit("in"), opt(lit("."))]),
        seq(vec![opt(Re::Space), lit("inch"), opt(lit("es"))]),
    ])
}
fn ft_re() -> Re {
    // (?:'|′|’|\s?ft\.?|\s?feet|\s?foot)
    alt(vec![
        lit("'"),
        lit("′"),
        lit("’"),
        seq(vec![opt(Re::Space), lit("ft"), opt(lit("."))]),
        seq(vec![opt(Re::Space), lit("feet")]),
        seq(vec![opt(Re::Space), lit("foot")]),
    ])
}
fn num_re() -> Re {
    // \d+(?:\s*[- ]\s*\d+\s*\/\s*\d+|\s*\/\s*\d+|\.\d+)?
    seq(vec![
        plus(Re::Digit),
        opt(alt(vec![
            seq(vec![
                star(Re::Space),
                Re::Set(units16("- ")),
                star(Re::Space),
                plus(Re::Digit),
                star(Re::Space),
                lit("/"),
                star(Re::Space),
                plus(Re::Digit),
            ]),
            seq(vec![star(Re::Space), lit("/"), star(Re::Space), plus(Re::Digit)]),
            seq(vec![lit("."), plus(Re::Digit)]),
        ])),
    ])
}
fn dec_re() -> Re {
    // \d+(?:\.\d+)?
    seq(vec![plus(Re::Digit), opt(seq(vec![lit("."), plus(Re::Digit)]))])
}
fn imperial_re() -> Re {
    seq(vec![
        cap(1, num_re()),
        star(Re::Space),
        in_re(),
        star(Re::Space),
        lit("="),
        star(Re::Space),
        cap(2, dec_re()),
        star(Re::Space),
        ft_re(),
        opt(seq(vec![star(Re::Space), opt(lit("-")), star(Re::Space), cap(3, dec_re()), star(Re::Space), in_re()])),
    ])
}
fn metric_re() -> Re {
    // (?<![\d.:/])1\s*:\s*(\d{1,5})(?![\d:/]|\.\d)
    seq(vec![
        Re::NotBehindSet(units16(".:/"), true),
        lit("1"),
        star(Re::Space),
        lit(":"),
        star(Re::Space),
        cap(1, Re::Rep(Box::new(Re::Digit), 1, Some(5))),
        Re::NotAhead(Box::new(alt(vec![Re::Digit, Re::Set(units16(":/")), seq(vec![lit("."), Re::Digit])]))),
    ])
}
fn nts_re() -> Re {
    // \bN\.?\s?T\.?\s?S\.?(?![a-z])|\bNOT\s+TO\s+SCALE\b   (flag i: [a-z] matches A-Z too)
    let az: Vec<u16> = (b'a'..=b'z').chain(b'A'..=b'Z').map(u16::from).collect();
    alt(vec![
        seq(vec![
            Re::Wb,
            lit("n"),
            opt(lit(".")),
            opt(Re::Space),
            lit("t"),
            opt(lit(".")),
            opt(Re::Space),
            lit("s"),
            opt(lit(".")),
            Re::NotAhead(Box::new(Re::Set(az))),
        ]),
        seq(vec![Re::Wb, lit("not"), plus(Re::Space), lit("to"), plus(Re::Space), lit("scale"), Re::Wb]),
    ])
}
fn scale_word_re() -> Re {
    // \bscale\b\s*[:=]?\s*$
    seq(vec![Re::Wb, lit("scale"), Re::Wb, star(Re::Space), opt(Re::Set(units16(":="))), star(Re::Space), Re::End])
}

fn text_of(s: &[u16], r: Option<(usize, usize)>) -> Option<String> {
    r.map(|(a, b)| jv::from16(s.get(a..b).unwrap_or(&[])))
}

/// `inches(s)`: "1/4" → 0.25, "1-1/2" / "1 1/2" → 1.5, "0.25" → 0.25.
fn inches(s: &str) -> f64 {
    let t = cc_m3d::js::trim(s);
    let u = units16(t);
    // ^(\d+)\s*[- ]\s*(\d+)\s*\/\s*(\d+)$
    let mixed = seq(vec![
        cap(1, plus(Re::Digit)),
        star(Re::Space),
        Re::Set(units16("- ")),
        star(Re::Space),
        cap(2, plus(Re::Digit)),
        star(Re::Space),
        lit("/"),
        star(Re::Space),
        cap(3, plus(Re::Digit)),
        Re::End,
    ]);
    if let Some((0, _, c)) = exec(&mixed, &u, 0, false, 4).filter(|f| f.0 == 0) {
        let g = |i: usize| text_of(&u, c.get(i).copied().flatten()).map_or(f64::NAN, |x| jv::str_to_number(&x));
        return g(1) + g(2) / g(3);
    }
    let frac = seq(vec![cap(1, plus(Re::Digit)), star(Re::Space), lit("/"), star(Re::Space), cap(2, plus(Re::Digit)), Re::End]);
    if let Some((0, _, c)) = exec(&frac, &u, 0, false, 3).filter(|f| f.0 == 0) {
        let g = |i: usize| text_of(&u, c.get(i).copied().flatten()).map_or(f64::NAN, |x| jv::str_to_number(&x));
        let den = g(2);
        return if den != 0.0 { g(1) / den } else { f64::NAN };
    }
    jv::str_to_number(t)
}

fn by_ratio(k: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| format!("{}{}", if p.group == "Metric" { "m" } else { "i" }, crate::jv::num_str(p.ratio)) == k)
}

/// A scale notation found in the text.
#[derive(Debug, Clone, PartialEq)]
pub struct Found1 {
    pub preset: &'static str,
    pub label: &'static str,
    pub index: usize,
    pub scale_word: bool,
}

/// Every scale notation, in order.
pub fn scales_in_text(text: &str) -> Vec<Found1> {
    let s = units16(text);
    let before = |i: usize| -> bool {
        let w = s.get(i.saturating_sub(16)..i).unwrap_or(&[]);
        exec(&scale_word_re(), w, 0, true, 1).is_some()
    };
    let mut out = Vec::new();
    for (start, _, c) in match_all(&imperial_re(), &s, true, 4) {
        let paper = inches(&text_of(&s, c.get(1).copied().flatten()).unwrap_or_default());
        let real = jv::str_to_number(&text_of(&s, c.get(2).copied().flatten()).unwrap_or_default()) * 12.0
            + c.get(3).copied().flatten().map_or(0.0, |r| jv::str_to_number(&text_of(&s, Some(r)).unwrap_or_default()));
        if !(paper > 0.0) || !(real > 0.0) {
            continue;
        }
        let ratio = cc_m3d::js::round(real / paper * 1000.0) / 1000.0;
        let Some(p) = by_ratio(&format!("i{}", crate::jv::num_str(ratio))) else { continue };
        if p.group == "Metric" {
            continue;
        }
        out.push(Found1 { preset: p.key, label: p.label, index: start, scale_word: before(start) });
    }
    for (start, _, c) in match_all(&metric_re(), &s, false, 2) {
        let v = jv::str_to_number(&text_of(&s, c.get(1).copied().flatten()).unwrap_or_default());
        let Some(p) = by_ratio(&format!("m{}", crate::jv::num_str(v))) else { continue };
        out.push(Found1 { preset: p.key, label: p.label, index: start, scale_word: before(start) });
    }
    out.sort_by_key(|f| f.index);
    out
}

/// What to apply to a sheet, or why not.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetScale {
    pub preset: Option<&'static str>,
    pub label: Option<&'static str>,
    pub n: usize,
    pub others: Vec<&'static str>,
    pub nts: bool,
    pub why: &'static str,
}

pub fn sheet_scale(text: &str) -> SheetScale {
    let found = scales_in_text(text);
    let s = units16(text);
    let nts = exec(&nts_re(), &s, 0, true, 1).is_some();
    if found.is_empty() {
        return SheetScale { preset: None, label: None, n: 0, others: Vec::new(), nts, why: if nts { "nts" } else { "none" } };
    }
    let mut score: Vec<(&'static str, f64)> = Vec::new();
    for f in &found {
        let add = if f.scale_word { 2.0 } else { 1.0 };
        match score.iter_mut().find(|(k, _)| *k == f.preset) {
            Some(e) => e.1 += add,
            None => score.push((f.preset, add)),
        }
    }
    let first_ix = |k: &str| found.iter().position(|f| f.preset == k).unwrap_or(usize::MAX);
    let mut ranked = score.clone();
    cc_m3d::js::sort_by(&mut ranked, |a, b| {
        let d = b.1 - a.1;
        if d != 0.0 { d } else { first_ix(a.0) as f64 - first_ix(b.0) as f64 }
    });
    let preset = ranked.first().map_or("", |r| r.0);
    SheetScale {
        preset: Some(preset),
        label: crate::units::preset(preset).map(|p| p.label),
        n: found.iter().filter(|f| f.preset == preset).count(),
        others: ranked.iter().skip(1).map(|r| r.0).collect(),
        nts,
        why: if ranked.len() > 1 { "several" } else { "one" },
    }
}
