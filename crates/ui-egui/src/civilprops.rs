//! CivilCraft's Properties edits beyond the PI / PVI tables (M11.12–M11.13): 0.5 WL.23's "why" blocks (curves that have
//! no solution, or fall under the design minimum radius, said in numbers with the radius or curve length that fits as a
//! button: one undo step, its label said) and the typed fields of civilcraft.js `alignmentProps`, `corridorProps`,
//! `padProps`, `networkBlock`, `structureProps`, `pipeProps`, features-ui.js and parcels-ui.js `props`, each through
//! `civil.prop=<field>=<value>` on the selection with 0.5's checks and undo labels.

use cc_civil::i18n::t;
use cc_civil::js::{arr, num_to_string, numf};
use cc_civil::solve_why::{WPi, curve_why, vertical_why};
use cc_civil::tr;
use cc_civil::units::fixed;
use serde_json::Value;

use crate::civilview::{CivilView, Sel};

/// One "why" item as the block shows it: what is said, notes, and its fix buttons (attr, text).
#[derive(Clone, Debug, Default)]
pub struct WhyItem {
    pub said: String,
    pub notes: Vec<String>,
    pub fixes: Vec<(String, String)>,
}

/// A fix: the radii (PI, r) or the curve length (PVI index, L) it sets and the undo label (also said).
#[derive(Clone, Debug)]
enum Fix {
    Radii(Vec<(usize, f64)>, String),
    Length(usize, f64, String),
}

/// The blocks of one alignment: the curve block (error or under-minimum head), the vertical block.
#[derive(Clone, Debug, Default)]
pub struct Why {
    pub curve: Vec<WhyItem>,
    pub curve_error: bool,
    pub vertical: Vec<WhyItem>,
    fixes: Vec<(String, Fix)>,
}

/// whyBlock's `n(v)`: two decimals, no thousands separators.
fn n2(v: f64) -> String {
    fixed(v, 2).replace(',', "")
}

fn opt(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64)
}

impl CivilView {
    /// civilcraft.js `whyBlock(a, curveWhy(a.pis, { minR }))` and profileProps' `verticalWhy(a.pvis)` block, with their
    /// buttons de-duplicated as 0.5 shows them.
    pub fn why_of(&mut self, a: &Value) -> Why {
        let t0 = web_time::Instant::now();
        let w = self.why_build(a);
        self.timings.insert("why", t0.elapsed().as_secs_f64() * 1000.0);
        w
    }

    fn why_build(&mut self, a: &Value) -> Why {
        let u = cc_civil::units::unit_of(self.project.units()).short;
        let pis: Vec<WPi> =
            arr(a.get("pis")).iter().map(|p| WPi { p: cc_civil::geo::read_p2(p), r: cc_civil::js::number(p.get("r")) }).collect();
        let min_r = numf(a.get("minR"));
        let why = curve_why(&pis, if min_r > 0.0 { min_r } else { 0.0 });
        let n_pis = pis.len();
        let mut out = Why::default();
        let mut shown = std::collections::HashSet::new();
        let mut btn = |out: &mut Why, item: &mut WhyItem, text: String, attr: String, fix: Fix| {
            if shown.insert(attr.clone()) {
                item.fixes.push((attr.clone(), text));
                out.fixes.push((attr, fix));
            }
        };
        for w in &why {
            let kind = w.get("kind").and_then(Value::as_str).unwrap_or("");
            let mut item = WhyItem::default();
            match kind {
                "overlap" => {
                    let a_ = numf(w.get("a")) as usize;
                    let b_ = numf(w.get("b")) as usize;
                    let (fa, fb) = (opt(w.get("fixA")), opt(w.get("fixB")));
                    let (ra, rb) = (numf(w.get("both").and_then(|x| x.get("ra"))), numf(w.get("both").and_then(|x| x.get("rb"))));
                    if let Some(f) = fa {
                        let label = tr!("why.undo.r", pi = a_ + 1, r = n2(f));
                        btn(
                            &mut out,
                            &mut item,
                            tr!("why.fix.r", pi = a_ + 1, r = n2(f), u = u),
                            format!("{a_}:{}", num_to_string(f)),
                            Fix::Radii(vec![(a_, f)], label),
                        );
                    }
                    if let Some(f) = fb {
                        let label = tr!("why.undo.r", pi = b_ + 1, r = n2(f));
                        btn(
                            &mut out,
                            &mut item,
                            tr!("why.fix.r", pi = b_ + 1, r = n2(f), u = u),
                            format!("{b_}:{}", num_to_string(f)),
                            Fix::Radii(vec![(b_, f)], label),
                        );
                    }
                    if ra > 0.0 && rb > 0.0 {
                        let label = tr!("why.undo.both", a = a_ + 1, b = b_ + 1);
                        btn(
                            &mut out,
                            &mut item,
                            tr!("why.fix.both", a = a_ + 1, b = b_ + 1, ra = n2(ra), rb = n2(rb), u = u),
                            "both".into(),
                            Fix::Radii(vec![(a_, ra), (b_, rb)], label),
                        );
                    }
                    if fa.is_none() && a_ > 0 {
                        item.notes.push(tr!("why.none.at", pi = a_ + 1, other = b_ + 1));
                    }
                    if fb.is_none() && b_ + 1 < n_pis {
                        item.notes.push(tr!("why.none.at", pi = b_ + 1, other = a_ + 1));
                    }
                    let (ta, tb) = (numf(w.get("Ta")), numf(w.get("Tb")));
                    let (need, dist) = (n2(numf(w.get("need"))), n2(numf(w.get("dist"))));
                    item.said = if !(ta > 0.0 && tb > 0.0) {
                        let (pi, other) = if ta > 0.0 { (a_, b_) } else { (b_, a_) };
                        tr!("why.overlap.one", pi = pi + 1, other = other + 1, need = need.as_str(), dist = dist.as_str(), u = u)
                    } else {
                        tr!(
                            "why.overlap",
                            a = a_ + 1,
                            b = b_ + 1,
                            need = need.as_str(),
                            ta = n2(ta),
                            tb = n2(tb),
                            dist = dist.as_str(),
                            u = u
                        )
                    };
                    out.curve_error = true;
                }
                "back" => {
                    let pi = numf(w.get("pi")) as usize;
                    item.said = tr!("why.back", pi = pi + 1);
                    let label = tr!("why.undo.r", pi = pi + 1, r = "0");
                    btn(&mut out, &mut item, tr!("why.fix.nocurve", pi = pi + 1), format!("{pi}:0"), Fix::Radii(vec![(pi, 0.0)], label));
                    out.curve_error = true;
                }
                _ => {
                    let pi = numf(w.get("pi")) as usize;
                    let (r, minr) = (numf(w.get("r")), numf(w.get("minR")));
                    item.said = tr!("why.min", pi = pi + 1, r = n2(r), min = n2(minr), u = u);
                    match opt(w.get("fix")) {
                        Some(f) => {
                            let label = tr!("why.undo.r", pi = pi + 1, r = n2(f));
                            btn(
                                &mut out,
                                &mut item,
                                tr!("why.fix.min", pi = pi + 1, r = n2(f), u = u),
                                format!("{pi}:{}", num_to_string(f)),
                                Fix::Radii(vec![(pi, f)], label),
                            );
                        }
                        None => item.notes.push(tr!("why.min.none", pi = pi + 1, max = n2(numf(w.get("maxR"))), min = n2(minr), u = u)),
                    }
                }
            }
            out.curve.push(item);
        }
        let al = self.project.alignment(a);
        let pvis = arr(a.get("pvis"));
        if al.ok() && !pvis.is_empty() {
            let pv: Vec<cc_civil::profile::Pvi> = pvis.iter().map(cc_civil::profile::read_pvi).collect();
            for w in vertical_why(&pv) {
                let pvi = numf(w.get("pvi")) as usize;
                let index = numf(w.get("index")) as usize;
                let before = w.get("side").and_then(Value::as_str) == Some("before");
                let mut item = WhyItem {
                    said: tr!(
                        if before { "why.vc.before" } else { "why.vc.past" },
                        pvi = pvi + 1,
                        l = n2(numf(w.get("L"))),
                        room = n2(numf(w.get("room")) / 2.0),
                        u = u
                    ),
                    ..WhyItem::default()
                };
                if let Some(f) = opt(w.get("fix")).filter(|f| *f != 0.0) {
                    let attr = format!("L{index}:{}", num_to_string(f));
                    let text = tr!("why.fix.l", pvi = pvi + 1, l = n2(f), k = fixed(numf(w.get("K")), 1), u = u);
                    item.fixes.push((attr.clone(), text));
                    out.fixes.push((attr, Fix::Length(index, f, tr!("why.undo.l", pvi = pvi + 1, l = n2(f)))));
                }
                out.vertical.push(item);
            }
        }
        out
    }

    /// `civil.why=<attr>`: press a fix button of the selected alignment's why blocks (attr as 0.5's `data-why-fix`:
    /// "<PI>:<radius>", "both", "L<PVI index>:<length>"; or "#k", the k-th button from 0 as shown).
    pub fn why_fix(&mut self, attr: &str) -> Result<(), String> {
        let Some(Sel::Alignment(id)) = self.sel.clone() else { return Err("civil.why: select an alignment first".into()) };
        let a = arr(self.project.state.get("alignments"))
            .iter()
            .find(|x| x.get("id").and_then(Value::as_str) == Some(id.as_str()))
            .cloned()
            .ok_or("civil.why: no such alignment")?;
        let why = self.why_of(&a);
        let attr = attr.trim();
        let fix = match attr.strip_prefix('#').and_then(|k| k.parse::<usize>().ok()) {
            Some(k) => why.fixes.into_iter().nth(k).map(|(_, f)| f),
            None => why.fixes.into_iter().find(|(k, _)| k == attr).map(|(_, f)| f),
        };
        let fix = fix.ok_or_else(|| format!("civil.why: no fix {attr} is offered"))?;
        let label = match &fix {
            Fix::Radii(_, l) | Fix::Length(_, _, l) => l.clone(),
        };
        self.project.change(&label, &mut |s: &mut Value| {
            let Some(al) = s
                .get_mut("alignments")
                .and_then(Value::as_array_mut)
                .and_then(|l| l.iter_mut().find(|q| q.get("id").and_then(Value::as_str) == Some(id.as_str())))
            else {
                return;
            };
            match &fix {
                Fix::Radii(pairs, _) => {
                    for (i, r) in pairs {
                        if let Some(p) = al.get_mut("pis").and_then(|x| x.get_mut(*i)).and_then(Value::as_object_mut) {
                            p.insert("r".into(), cc_civil::js::jnum(*r));
                        }
                    }
                }
                Fix::Length(i, l, _) => {
                    if let Some(v) = al.get_mut("pvis").and_then(|x| x.get_mut(*i)).and_then(Value::as_object_mut) {
                        v.insert("L".into(), cc_civil::js::jnum(*l));
                        v.shift_remove("K");
                    }
                }
            }
        });
        self.message = label;
        Ok(())
    }
}

impl CivilView {
    /// Change the selected alignment (`f` on its JSON) as one undo step.
    fn al_change(&mut self, id: &str, label: &str, f: &mut dyn FnMut(&mut serde_json::Map<String, Value>)) -> bool {
        self.project.change(label, &mut |s: &mut Value| {
            if let Some(m) = s
                .get_mut("alignments")
                .and_then(Value::as_array_mut)
                .and_then(|l| l.iter_mut().find(|q| q.get("id").and_then(Value::as_str) == Some(id)))
                .and_then(Value::as_object_mut)
            {
                f(m);
            }
        })
    }

    /// `civil.prop=<field>=<value>`: one typed Properties field of the selection, with civilcraft.js's checks (a value
    /// it refuses is an error and changes nothing) and its undo label.
    pub fn prop_set(&mut self, field: &str, v: &str) -> Result<(), String> {
        let bad = || format!("civil.prop: {field} = {v} is refused");
        match self.sel.clone() {
            Some(Sel::Alignment(id)) => match field {
                // 0.5 WL.23: the design minimum radius (empty = none)
                "minR" => {
                    let x = if cc_civil::js::trim(v).is_empty() { 0.0 } else { cc_civil::js::str_to_number(v) };
                    if x.is_nan() || x < 0.0 {
                        return Err(bad());
                    }
                    self.al_change(&id, &t("why.undo.minr", &[]), &mut |m| {
                        m.insert("minR".into(), cc_civil::js::jnum(x));
                    });
                    Ok(())
                }
                _ => Err(format!("civil.prop: an alignment has no field {field}")),
            },
            _ => Err(format!("civil.prop: nothing selected has a field {field}")),
        }
    }
}

/// The why blocks in the alignment's Properties; a pressed fix → `civil.why`.
pub fn why_ui(
    ui: &mut egui::Ui,
    tk: &crate::theme::Tokens,
    items: &[WhyItem],
    head: &str,
    error: bool,
    acts: &mut Vec<(&'static str, String)>,
) {
    if items.is_empty() {
        return;
    }
    let col = if error { ui.visuals().error_fg_color } else { tk.text_strong };
    egui::Frame::new().stroke(egui::Stroke::new(1.0, col)).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
        ui.label(egui::RichText::new(head).strong().color(col));
        for it in items {
            ui.add(egui::Label::new(egui::RichText::new(format!("• {}", it.said)).color(tk.text)).wrap());
            for n in &it.notes {
                ui.add(egui::Label::new(egui::RichText::new(n).small().color(tk.text_dim)).wrap());
            }
            if !it.fixes.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    for (attr, text) in &it.fixes {
                        if ui.small_button(text).clicked() {
                            acts.push(("civil.why", attr.clone()));
                        }
                    }
                });
            }
        }
    });
}

/// whyBlock's head for the curve block.
pub fn curve_head(error: bool) -> String {
    t(if error { "why.head.error" } else { "why.head.min" }, &[])
}
