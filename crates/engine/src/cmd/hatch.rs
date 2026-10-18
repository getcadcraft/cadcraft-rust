//! HATCH (commands.js): pick inside areas (hatch.js `boundaryAt`) or select closed objects; one hatch at the end.

use std::rc::Rc;

use cc_cad::entity::{Entity, Geo, Hatch};
use cc_cad_geom::geom::{deg, norm_angle};
use cc_cad_geom::hatch::{boundary_at, hatch_is_metric_doc, hatch_lines, loops_of, pattern_names};
use serde_json::Value;

use crate::cmd::HatchMemo;
use crate::cmd::draw::ent;
use crate::engine::{Ctx, Opts, R, Reply};
use crate::input::{Def, fmt};

fn memo(c: &Ctx) -> HatchMemo {
    c.st().memo.hatch.get_or_insert_with(|| HatchMemo { pattern: "ANSI31".into(), scale: 1.0, angle: 0.0 }).clone()
}
fn set_memo(c: &Ctx, m: HatchMemo) {
    c.st().memo.hatch = Some(m);
}

/// `make(loops)`: the hatch with the current settings.
fn make(c: &Ctx, s: &HatchMemo, loops: Vec<Vec<f64>>) -> Entity {
    let solid = s.pattern == "SOLID";
    let mut e = ent(c, Geo::Hatch(Hatch { loops, solid: Some(solid) }));
    if !solid {
        let iso = hatch_is_metric_doc(None, &c.doc());
        e.extra.insert("pattern".into(), Value::String(s.pattern.clone()));
        e.extra.insert("pscale".into(), cc_cad::entity::num(s.scale));
        e.extra.insert("pangle".into(), cc_cad::entity::num(s.angle));
        e.extra.insert("piso".into(), Value::Bool(iso));
    }
    e
}

fn pattern_known(n: &str) -> bool {
    pattern_names().contains(&n)
}

pub async fn hatch(c: Ctx) -> R {
    let s = memo(&c);
    let label = if s.pattern == "SOLID" {
        c.t("cmd.hatch.solid")
    } else {
        format!("{}, {}", s.pattern, c.tv("cmd.hatch.scaleangle", &[("scale", fmt(s.scale)), ("angle", fmt(deg(s.angle)))]))
    };
    c.print(&c.tv("cmd.hatch.current", &[("what", label)]));
    let mut picks: Vec<Vec<Vec<f64>>> = Vec::new();
    loop {
        {
            let all: Vec<Vec<f64>> = picks.iter().flatten().cloned().collect();
            let has = !picks.is_empty();
            let s = memo(&c);
            let h = Rc::new(if has { Some(make(&c, &s, all)) } else { None });
            c.preview(Some(Rc::new(move |_, _| h.as_ref().clone().into_iter().collect())));
        }
        let mut options = vec![
            c.opt("S", &c.t("opt.selectobjects")),
            c.opt("P", &c.t("opt.pattern")),
            c.opt("SC", &c.t("opt.scale")),
            c.opt("A", &c.t("opt.angle")),
        ];
        if !picks.is_empty() {
            options.push(c.opt("U", &c.t("opt.undo")));
        }
        let p = c.point(c.t("cmd.hatch.pick"), Opts { none: true, options, ..Opts::default() }).await?;
        let pt = match p {
            Reply::Null => break,
            Reply::Opt(k) => {
                match k.as_str() {
                    "U" => {
                        picks.pop();
                    }
                    "P" => loop {
                        let mut s = memo(&c);
                        let r = c
                            .string(c.t("cmd.hatch.pattern"), Opts { default: Some(Def::Str(s.pattern.clone())), ..Opts::default() })
                            .await?;
                        let v = match r {
                            Reply::Str(t) => cc_cad::js::trim(&t).to_uppercase(),
                            _ => String::new(),
                        };
                        if v.is_empty() {
                            break;
                        }
                        if v == "?" {
                            let mut lines = vec![format!("SOLID — {}", c.t("cmd.hatch.solid"))];
                            for n in pattern_names() {
                                lines.push(format!("{n} — {}", c.t(&format!("hatch.pat.{}", n.to_lowercase()))));
                            }
                            c.print(&lines.join("\n"));
                            continue;
                        }
                        if v == "SOLID" || v == "S" || pattern_known(&v) {
                            s.pattern = if v == "S" { "SOLID".into() } else { v };
                            set_memo(&c, s);
                            break;
                        }
                        c.print(&c.tv("cmd.hatch.unknown", &[("name", v)]));
                    },
                    "SC" => {
                        let mut s = memo(&c);
                        let r = c.number(c.t("cmd.hatch.scale"), Opts { default: Some(Def::Num(s.scale)), ..Opts::default() }).await?;
                        if let Reply::Num(v) = r
                            && v > 0.0
                        {
                            s.scale = v;
                            set_memo(&c, s);
                        }
                    }
                    "A" => {
                        let mut s = memo(&c);
                        let r = c
                            .angle(c.t("cmd.hatch.angle"), Opts { default: Some(Def::Num(s.angle)), ucs: true, ..Opts::default() })
                            .await?;
                        let v = match r {
                            Reply::Num(v) if v != 0.0 && !v.is_nan() => v,
                            _ => 0.0,
                        };
                        s.angle = norm_angle(v);
                        set_memo(&c, s);
                    }
                    "S" => {
                        c.preview(None);
                        let ids = c.select(Some(c.t("cmd.hatch.select")), true).await?;
                        let mut loops = Vec::new();
                        let mut skipped = 0usize;
                        {
                            let d = c.doc();
                            for id in &ids.0 {
                                match d.get(*id).and_then(loops_of) {
                                    Some(l) => loops.extend(l),
                                    None => skipped = skipped.saturating_add(1),
                                }
                            }
                        }
                        if skipped > 0 {
                            c.print(&c.tv("cmd.hatch.notclosed", &[("n", skipped.to_string())]));
                        }
                        if !loops.is_empty() {
                            picks.push(loops);
                        }
                    }
                    _ => {}
                }
                continue;
            }
            Reply::Point(q, _) => q,
            _ => break,
        };
        let within = c.sh().host.borrow().view_extents();
        let r = {
            let d = c.doc();
            let ents: Vec<&Entity> = d.entities().filter(|e| d.visible(e)).collect();
            boundary_at(&ents, pt, Some(&d.blocks), within.as_ref())
        };
        match r {
            Err(e) => c.print(&c.t(&format!("cmd.hatch.err.{}", e.code()))),
            Ok(loops) => picks.push(loops),
        }
    }
    c.preview(None);
    if picks.is_empty() {
        return Ok(());
    }
    let s = memo(&c);
    let h = make(&c, &s, picks.into_iter().flatten().collect());
    let solid = matches!(&h.geo, Geo::Hatch(x) if x.solid == Some(true));
    if !solid && hatch_lines(&h, &c.doc()).is_none() {
        c.print(&c.t("cmd.hatch.dense"));
    }
    c.change("HATCH", |d| d.add(h))?;
    Ok(())
}
