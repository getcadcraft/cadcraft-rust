//! UNITS (commands.js, over units.js): how lengths are written and typed. `setLengthFormat` is one undo step and
//! new dimensions follow it.

use cc_cad::Doc;
use cc_cad::entity::num;
use cc_cad::js;
use cc_cad_geom::units::{format_length, is_imperial_format, lunit_code, units_of};
use serde_json::Value;

use crate::engine::{Ctx, Opts, R, Reply};
use crate::input::Def;
use crate::lang::Lang;

/// `LUNIT_OPTS`: key, code, label key.
pub const LUNIT_OPTS: [(&str, i32, &str); 5] = [
    ("S", 1, "opt.lunit.scientific"),
    ("D", 2, "opt.lunit.decimal"),
    ("E", 3, "opt.lunit.engineering"),
    ("A", 4, "opt.lunit.architectural"),
    ("F", 5, "opt.lunit.fractional"),
];

/// `setLengthFormat(doc, lunits, luprec)` → (code, prec).
pub fn set_length_format(doc: &mut Doc, lunits: f64, luprec: f64) -> Result<(i32, i32), cc_cad::DocError> {
    let code = lunit_code(lunits);
    let r = js::max(0.0, js::min(8.0, js::round(luprec)));
    #[allow(clippy::cast_possible_truncation)]
    let prec = if r.is_finite() { r as i32 } else { 0 };
    doc.change("UNITS", |d| {
        let mut patch = cc_cad::entity::Obj::new();
        patch.insert("lunits".into(), Value::from(code));
        patch.insert("luprec".into(), Value::from(prec));
        d.set_meta(&patch)
    })?;
    doc.current.dim.insert("lunit".into(), num(f64::from(code)));
    doc.current.dim.insert("dec".into(), num(f64::from(prec)));
    Ok((code, prec))
}

/// `lengthFormatName(lunits)`: "Architectural".
pub fn length_format_name(lang: &Lang, code: i32) -> String {
    let c = lunit_code(f64::from(code));
    lang.s(LUNIT_OPTS.iter().find(|o| o.1 == c).map_or("opt.lunit.decimal", |o| o.2))
}

/// `lengthFormatLabel(lunits, prec)`: "Architectural, to 1/16"" — the format as a phrase.
pub fn length_format_label(lang: &Lang, code: i32, prec: i32) -> String {
    let c = lunit_code(f64::from(code));
    let name = length_format_name(lang, c);
    if c == 4 || c == 5 {
        lang.t("cmd.units.label.frac", &[("format", name), ("den", js::num_to_string(2f64.powi(prec)))])
    } else {
        lang.t("cmd.units.label.dec", &[("format", name), ("n", prec.to_string())])
    }
}

fn example(code: i32, prec: i32) -> String {
    format_length(150.5, f64::from(code), Some(&Value::from(prec)))
}

pub async fn units(c: Ctx) -> R {
    let (cur_code, cur_prec) = units_of(Some(&c.doc().meta));
    let lang = c.sh().lang.clone();
    let cur_key = LUNIT_OPTS.iter().find(|o| o.1 == cur_code).map_or("D", |o| o.0);
    c.print(
        &c.tv("cmd.units.now", &[("format", length_format_label(&lang, cur_code, cur_prec)), ("example", example(cur_code, cur_prec))]),
    );
    let options = LUNIT_OPTS.iter().map(|(k, _, l)| c.opt(k, &c.t(l))).collect();
    let k = c.keyword(c.t("cmd.units.format"), Opts { options, default: Some(Def::Str(cur_key.into())), ..Opts::default() }).await?;
    let key = match k {
        Reply::Opt(o) | Reply::Str(o) => o,
        _ => String::new(),
    };
    let code = LUNIT_OPTS.iter().find(|o| o.0 == key).map_or(cur_code, |o| o.1);
    let prec = loop {
        if code == 4 || code == 5 {
            let d = 2f64.powi(if code == cur_code { cur_prec } else { 4 });
            let r = c.number(c.t("cmd.units.den"), Opts { default: Some(Def::Num(d)), ..Opts::default() }).await?;
            let den = if let Reply::Num(n) = r { n } else { f64::NAN };
            let p = den.log2();
            if p.fract() == 0.0 && (0.0..=8.0).contains(&p) {
                break p;
            }
            c.print(&c.t("cmd.units.den.bad"));
        } else {
            let d = f64::from(if code == cur_code { cur_prec } else { 4 });
            let r = c.number(c.t("cmd.units.dec"), Opts { default: Some(Def::Num(d)), ..Opts::default() }).await?;
            let n = if let Reply::Num(n) = r { n } else { f64::NAN };
            if n.fract() == 0.0 && (0.0..=8.0).contains(&n) {
                break n;
            }
            c.print(&c.t("cmd.units.dec.bad"));
        }
    };
    let (rc, rp) = {
        let mut d = c.sh().doc.borrow_mut();
        set_length_format(&mut d, f64::from(code), prec).map_err(crate::engine::doc_err)?
    };
    c.print(&c.tv("cmd.units.set", &[("format", length_format_label(&lang, rc, rp)), ("example", example(rc, rp))]));
    if is_imperial_format(rc) {
        c.print(&c.t("cmd.units.typing"));
        let u = c.doc().meta.get("units").filter(|v| js::truthy(Some(v))).map(js::to_js_string);
        if let Some(u) = u.filter(|u| u != "inches") {
            c.print(&c.tv("cmd.units.inches", &[("units", u)]));
        }
    }
    Ok(())
}
