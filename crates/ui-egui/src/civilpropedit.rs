//! CivilCraft's other Properties edits (M11.13), each one `civil.prop=<field>=<value>` on the selection with the checks
//! and undo labels of 0.5's onCommit bodies: the alignment's name, start station, corridor (on, lanes, shoulders, slopes,
//! step) and superelevation (on, Δ, the rate table); the pad's name, elevation and slopes; a structure's name, rim,
//! sump and size; a pipe's name, size, inverts and Auto inverts; a feature line's name, use, closed, daylight and its
//! slopes, vertex elevations and grades, Raise, From surface, Even grade and the stepped offset; a parcel's name, point
//! of beginning and course radii / turns; one survey point's number, E, N, elevation and description (point-edit.js);
//! with nothing selected the project's name, units, longest triangle edge and the pipe network's type and rules.
//! A refused value is an error and changes nothing.

use cc_civil::i18n::t;
use cc_civil::js::{J, arr, jnum, numf, slice16_range, str_to_number, string_of, trim};
use cc_civil::tr;
use cc_m3d::jsmath::JsMath;
use serde_json::{Map, Value};

use crate::civilview::{CivilView, Sel};

fn find_mut<'a>(list: Option<&'a mut Value>, id: &str) -> Option<&'a mut Map<String, Value>> {
    list?.as_array_mut()?.iter_mut().find(|q| q.get("id").and_then(Value::as_str) == Some(id))?.as_object_mut()
}

fn find<'a>(list: Option<&'a Value>, id: &str) -> Option<&'a Value> {
    arr(list).iter().find(|q| q.get("id").and_then(Value::as_str) == Some(id))
}

/// JS `{ ...base, ...over, [k]: v }` on JSON objects (keys keep their first place, new ones go last).
fn spread(base: Value, over: Option<&Value>) -> Map<String, Value> {
    let mut m = base.as_object().cloned().unwrap_or_default();
    if let Some(o) = over.and_then(Value::as_object) {
        for (k, v) in o {
            m.insert(k.clone(), v.clone());
        }
    }
    m
}

/// JS `Number(v)` refused when not ≥ 0, or empty.
fn nonneg(v: &str) -> Option<f64> {
    let x = str_to_number(v);
    (x >= 0.0 && !v.is_empty()).then_some(x)
}

/// "i:value" → (i, value).
fn indexed(v: &str) -> Option<(usize, &str)> {
    let (i, rest) = v.split_once(':')?;
    Some((i.trim().parse().ok()?, rest))
}

/// A name as 0.5 keeps it: trimmed, at most `n` UTF-16 units, else the fallback.
fn name_of(v: &str, n: i64, fallback: &str) -> String {
    let s = slice16_range(trim(v), 0, Some(n));
    if s.is_empty() { fallback.to_string() } else { s }
}

impl CivilView {
    fn al_edit(&mut self, id: &str, label: &str, f: &mut dyn FnMut(&mut Map<String, Value>)) -> bool {
        self.project.change(label, &mut |s: &mut Value| {
            if let Some(m) = find_mut(s.get_mut("alignments"), id) {
                f(m);
            }
        })
    }

    /// The fields M11.13 adds (civilprops.rs `prop_set` hands everything else here).
    pub fn prop_set_more(&mut self, field: &str, v: &str) -> Result<(), String> {
        let bad = || format!("civil.prop: {field} = {v} is refused");
        let units = self.project.units().to_string();
        if field.starts_with("dr.") {
            return self.dr_hydro_prop(field, v);
        }
        match self.sel.clone() {
            Some(Sel::Catchment(id)) => self.dr_catchment_prop(&id, field, v),
            Some(Sel::Alignment(id)) => self.alignment_prop(&id, field, v, &units),
            Some(Sel::Pad(id)) => {
                let label;
                let (key, val) = match field {
                    "name" => {
                        label = t("cv.undo.pad.rename", &[]);
                        ("name", Value::String(name_of(v, 120, &t("cv.pad", &[]))))
                    }
                    "z" => {
                        let x = str_to_number(v);
                        if !x.is_finite() || v.is_empty() {
                            return Err(bad());
                        }
                        label = t("cv.undo.pad.z", &[]);
                        ("z", jnum(x))
                    }
                    "cut" | "fill" => {
                        let x = str_to_number(v);
                        if x.is_nan() || x <= 0.0 {
                            return Err(bad());
                        }
                        label = t(if field == "cut" { "fl.props.cut" } else { "fl.props.fill" }, &[]);
                        (field, jnum(x))
                    }
                    _ => return Err(format!("civil.prop: a pad has no field {field}")),
                };
                self.project.change(&label, &mut |s: &mut Value| {
                    if let Some(m) = find_mut(s.get_mut("pads"), &id) {
                        m.insert(key.into(), val.clone());
                    }
                });
                Ok(())
            }
            Some(Sel::Structure(id)) => {
                let (label, key, val) = match field {
                    "name" => (t("cv.undo.st.rename", &[]), "name", Value::String(name_of(v, 60, &t("cv.st", &[])))),
                    "rim" if v.is_empty() => (t("cv.undo.st.rim.surface", &[]), "rim", Value::Null),
                    "rim" => {
                        let z = str_to_number(v);
                        if !z.is_finite() {
                            return Err(bad());
                        }
                        (t("cv.undo.st.rim", &[]), "rim", jnum(z))
                    }
                    "sump" => (t("cv.st.sump", &[]), "sumpDepth", jnum(nonneg_or_empty0(v).ok_or_else(bad)?)),
                    "dia" => {
                        let z = str_to_number(v);
                        if z.is_nan() || z <= 0.0 {
                            return Err(bad());
                        }
                        (t("cv.undo.st.size", &[]), "dia", jnum(z))
                    }
                    _ => return Err(format!("civil.prop: a structure has no field {field}")),
                };
                self.project.change(&label, &mut |s: &mut Value| {
                    if let Some(m) = find_mut(s.get_mut("network").and_then(|n| n.get_mut("structures")), &id) {
                        m.insert(key.into(), val.clone());
                    }
                });
                Ok(())
            }
            Some(Sel::Pipe(id)) => {
                if field == "auto" {
                    self.set_inverts(Some(vec![id]));
                    return Ok(());
                }
                let (label, key, val) = match field {
                    "name" => (t("cv.undo.pp.rename", &[]), "name", Value::String(name_of(v, 60, &t("dr.r.pipe", &[])))),
                    "dia" => {
                        let z = str_to_number(v);
                        if z.is_nan() || z <= 0.0 {
                            return Err(bad());
                        }
                        (t("cv.undo.pp.size", &[]), "dia", jnum(z))
                    }
                    "invUp" | "invDn" => {
                        let z = str_to_number(v);
                        if !z.is_finite() || v.is_empty() {
                            return Err(bad());
                        }
                        (t(if field == "invUp" { "cv.pp.up" } else { "cv.pp.dn" }, &[]), field, jnum(z))
                    }
                    _ => return Err(format!("civil.prop: a pipe has no field {field}")),
                };
                self.project.change(&label, &mut |s: &mut Value| {
                    if let Some(m) = find_mut(s.get_mut("network").and_then(|n| n.get_mut("pipes")), &id) {
                        m.insert(key.into(), val.clone());
                    }
                });
                Ok(())
            }
            Some(Sel::Feature(id)) => self.feature_prop(&id, field, v, &units),
            Some(Sel::Parcel(id)) => self.parcel_prop(&id, field, v),
            Some(Sel::Point(n)) => self.point_prop(&n, field, v),
            None if self.points.picked.len() == 1 => {
                let n = self.points.picked.iter().next().cloned().unwrap_or_default();
                self.point_prop(&n, field, v)
            }
            None => self.project_prop(field, v),
        }
    }

    fn alignment_prop(&mut self, id: &str, field: &str, v: &str, units: &str) -> Result<(), String> {
        let bad = || format!("civil.prop: {field} = {v} is refused");
        let a = find(self.project.state.get("alignments"), id).cloned().ok_or("civil.prop: no such alignment")?;
        let corridor_on = a.get("corridor").and_then(|c| c.get("on")) == Some(&Value::Bool(true));
        match field {
            "name" => {
                let name = name_of(v, 120, &t("sec.alignment", &[]));
                self.al_edit(id, &t("cv.undo.al.rename", &[]), &mut |m| {
                    m.insert("name".into(), Value::String(name.clone()));
                });
            }
            "start" => {
                let x = cc_civil::units::parse_station(v, units).ok_or_else(bad)?;
                self.al_edit(id, &t("cv.al.start", &[]), &mut |m| {
                    m.insert("start".into(), jnum(x));
                });
            }
            "corridor" => {
                let on = match v {
                    "on" | "true" => true,
                    "off" | "false" => false,
                    _ => return Err(bad()),
                };
                let base = cc_civil::corridor::default_assembly(units).j();
                let label = t(if on { "cv.undo.co.on" } else { "cv.undo.co.off" }, &[]);
                self.al_edit(id, &label, &mut |m| {
                    let mut c = spread(base.clone(), m.get("corridor"));
                    c.insert("on".into(), Value::Bool(on));
                    m.insert("corridor".into(), Value::Object(c));
                });
            }
            "lane" | "laneSlope" | "shoulder" | "shoulderSlope" | "cut" | "fill" | "step" if corridor_on => {
                let (undo, scale) = match field {
                    "lane" => ("cv.undo.co.lane", 1.0),
                    "laneSlope" => ("cv.undo.co.ls", 100.0),
                    "shoulder" => ("cv.undo.co.sh", 1.0),
                    "shoulderSlope" => ("cv.undo.co.ss", 100.0),
                    "cut" => ("cv.undo.co.cut", 1.0),
                    "fill" => ("cv.undo.co.fill", 1.0),
                    _ => ("cv.undo.co.step", 1.0),
                };
                let x = nonneg(v).ok_or_else(bad)?;
                if matches!(field, "cut" | "fill" | "step") && x <= 0.0 {
                    return Err(bad());
                }
                let key = field.to_string();
                self.al_edit(id, &t(undo, &[]), &mut |m| {
                    if let Some(c) = m.get_mut("corridor").and_then(Value::as_object_mut) {
                        c.insert(key.clone(), jnum(x / scale));
                    }
                });
            }
            f if f.starts_with("se") && corridor_on => self.super_prop(id, &a, field, v, units)?,
            "lane" | "laneSlope" | "shoulder" | "shoulderSlope" | "cut" | "fill" | "step" => {
                return Err(format!("civil.prop: turn the corridor on first ({field})"));
            }
            _ => return Err(format!("civil.prop: an alignment has no field {field}")),
        }
        Ok(())
    }

    /// super-ui.js `superProps`: every change on `alignment.corridor.super` = { ...SUPER_DEFAULTS, ...super }.
    fn super_prop(&mut self, id: &str, a: &Value, field: &str, v: &str, units: &str) -> Result<(), String> {
        let bad = || format!("civil.prop: {field} = {v} is refused");
        let cur = a.get("corridor").and_then(|c| c.get("super")).cloned().unwrap_or(Value::Null);
        let on = cur.get("on") == Some(&Value::Bool(true));
        let rate_n = arr(cur.get("rate")).len();
        enum Op {
            On(bool),
            Delta(f64),
            R(usize, f64),
            E(usize, Option<f64>),
            Add,
            Remove(usize),
        }
        let (label, op) = match field {
            "se" => {
                let x = match v {
                    "on" | "true" => true,
                    "off" | "false" => false,
                    _ => return Err(bad()),
                };
                (t(if x { "se.undo.on" } else { "se.undo.off" }, &[]), Op::On(x))
            }
            _ if !on => return Err(format!("civil.prop: turn superelevation on first ({field})")),
            "se.delta" => {
                let x = str_to_number(v) / 100.0;
                if !(x > 0.0 && x <= 0.05) {
                    return Err(bad());
                }
                (t("se.undo.delta", &[]), Op::Delta(x))
            }
            "se.r" => {
                let (i, s) = indexed(v).filter(|(i, _)| *i < rate_n).ok_or_else(bad)?;
                (t("se.undo.rate", &[]), Op::R(i, nonneg(s).ok_or_else(bad)?))
            }
            "se.e" => {
                let (i, s) = indexed(v).filter(|(i, _)| *i < rate_n).ok_or_else(bad)?;
                let s = trim(s);
                let x = if s.is_empty() { None } else { Some(str_to_number(s) / 100.0) };
                if let Some(x) = x
                    && !(0.0..=0.2).contains(&x)
                {
                    return Err(bad());
                }
                (t("se.undo.rate", &[]), Op::E(i, x))
            }
            "se.add" => (t("se.undo.rate", &[]), Op::Add),
            "se.remove" => {
                let i: usize = v.trim().parse().map_err(|_| bad())?;
                if rate_n <= 1 || i >= rate_n {
                    return Err(bad());
                }
                (t("se.undo.rate", &[]), Op::Remove(i))
            }
            _ => return Err(format!("civil.prop: superelevation has no field {field}")),
        };
        let base = cc_civil::superelev::super_defaults(units).j();
        let sort_desc = |r: &mut Vec<Value>| cc_civil::js::sort_by(r, |m, n| numf(n.get(0)) - numf(m.get(0)));
        self.al_edit(id, &label, &mut |m| {
            let Some(c) = m.get_mut("corridor").and_then(Value::as_object_mut) else { return };
            let mut sp = spread(base.clone(), c.get("super"));
            match &op {
                Op::On(x) => {
                    sp.insert("on".into(), Value::Bool(*x));
                }
                Op::Delta(x) => {
                    sp.insert("delta".into(), jnum(*x));
                }
                Op::R(i, x) => {
                    if let Some(r) = sp.get_mut("rate").and_then(Value::as_array_mut) {
                        if let Some(row) = r.get_mut(*i).and_then(Value::as_array_mut)
                            && let Some(c0) = row.get_mut(0)
                        {
                            *c0 = jnum(*x);
                        }
                        sort_desc(r);
                    }
                }
                Op::E(i, x) => {
                    if let Some(row) = sp.get_mut("rate").and_then(|r| r.get_mut(*i)).and_then(Value::as_array_mut)
                        && let Some(c1) = row.get_mut(1)
                    {
                        *c1 = x.map_or(Value::Null, jnum);
                    }
                }
                Op::Add => {
                    if let Some(r) = sp.get_mut("rate").and_then(Value::as_array_mut) {
                        r.push(Value::Array(vec![jnum(0.0), jnum(0.06)]));
                        sort_desc(r);
                    }
                }
                Op::Remove(i) => {
                    if let Some(r) = sp.get_mut("rate").and_then(Value::as_array_mut)
                        && *i < r.len()
                    {
                        r.remove(*i);
                    }
                }
            }
            c.insert("super".into(), Value::Object(sp));
        });
        Ok(())
    }

    /// civilcraft.js `setInverts(only)`: autoInverts on the network (all pipes, or the ones given), one undo step.
    pub fn set_inverts(&mut self, only: Option<Vec<String>>) {
        let Some(n) = self.project.state.get("network").filter(|n| n.is_object()).cloned() else { return };
        let units = self.project.units().to_string();
        let g = self.project.graded();
        let eg = self.project.eg();
        let surface = g.surface.as_deref().or(eg.surface.as_ref());
        let net = cc_civil::pipes::read_network(&n);
        let (pipes, problems) = cc_civil::pipes::auto_inverts(&net, surface, &units, only.as_deref());
        self.project.change(&t("cv.undo.net.inverts", &[]), &mut |st: &mut Value| {
            if let Some(l) = st.get_mut("network").and_then(|n| n.get_mut("pipes")).and_then(Value::as_array_mut) {
                for (o, p) in l.iter_mut().zip(pipes.iter()) {
                    if let Some(m) = o.as_object_mut() {
                        m.insert("invUp".into(), p.inv_up.j());
                        m.insert("invDn".into(), p.inv_dn.j());
                    }
                }
            }
        });
        self.message = if problems.is_empty() { t("cv.net.inverts.done", &[]) } else { problems.join(" ") };
    }

    /// features-ui.js `edit(id, label, fn)`: the line replaced by fn's (as one undo step).
    fn fl_edit(&mut self, id: &str, label: &str, f: &mut dyn FnMut(&mut Map<String, Value>)) -> bool {
        self.project.change(label, &mut |s: &mut Value| {
            if let Some(m) = find_mut(s.get_mut("features"), id) {
                f(m);
            }
        })
    }

    /// The vertex elevations of `nf` written into the line's JSON (x, y and every other key stay as they are).
    fn fl_set_z(&mut self, id: &str, label: &str, nf: &cc_civil::featureline::Feature) -> bool {
        let zs: Vec<f64> = nf.pts.iter().map(|p| p.z).collect();
        self.fl_edit(id, label, &mut |m| {
            if let Some(pts) = m.get_mut("pts").and_then(Value::as_array_mut) {
                for (p, z) in pts.iter_mut().zip(zs.iter()) {
                    if let Some(o) = p.as_object_mut() {
                        o.insert("z".into(), jnum(*z));
                    }
                }
            }
        })
    }

    fn feature_prop(&mut self, id: &str, field: &str, v: &str, units: &str) -> Result<(), String> {
        use cc_civil::featureline as fl;
        let bad = || format!("civil.prop: {field} = {v} is refused");
        let fj = find(self.project.state.get("features"), id).cloned().ok_or("civil.prop: no such feature line")?;
        let f = fl::read_feature(&fj);
        match field {
            "name" => {
                let name = name_of(v, 120, &f.name);
                self.fl_edit(id, &t("fl.undo.rename", &[]), &mut |m| {
                    m.insert("name".into(), Value::String(name.clone()));
                });
            }
            "use" => {
                if !fl::USES.contains(&v) {
                    return Err(bad());
                }
                self.fl_edit(id, &t("fl.undo.use", &[]), &mut |m| {
                    m.insert("use".into(), Value::String(v.to_string()));
                });
            }
            "closed" | "daylight" => {
                let on = match v {
                    "on" | "true" => true,
                    "off" | "false" => false,
                    _ => return Err(bad()),
                };
                if field == "closed" && f.pts.len() < 3 {
                    return Err("civil.prop: a feature line needs three vertices to close".into());
                }
                if field == "daylight" && !(f.use_ == "design" && f.closed) {
                    return Err("civil.prop: only a closed design line daylights".into());
                }
                let label = match field {
                    "closed" => t(if on { "fl.undo.close" } else { "fl.undo.open" }, &[]),
                    _ => t("fl.undo.daylight", &[]),
                };
                let key = field.to_string();
                self.fl_edit(id, &label, &mut |m| {
                    m.insert(key.clone(), Value::Bool(on));
                });
            }
            "cut" | "fill" => {
                if !(f.use_ == "design" && f.closed && f.daylight) {
                    return Err("civil.prop: the slopes are a daylighted line's".into());
                }
                let x = str_to_number(v);
                if x.is_nan() || x <= 0.0 || v.is_empty() {
                    return Err(bad());
                }
                let key = field.to_string();
                self.fl_edit(id, &t(&format!("fl.undo.{field}"), &[]), &mut |m| {
                    m.insert(key.clone(), jnum(x));
                });
            }
            "elev" => {
                let (i, s) = indexed(v).ok_or_else(bad)?;
                let z = str_to_number(s);
                if !z.is_finite() || s.is_empty() || i >= f.pts.len() {
                    return Err(bad());
                }
                let nf = fl::set_elevation(&f, i, z);
                self.fl_set_z(id, &tr!("fl.undo.elev", n = i + 1), &nf);
            }
            "grade" => {
                let (i, s) = indexed(v).ok_or_else(bad)?;
                if fl::segments(&f).get(i).is_none() {
                    return Err(bad());
                }
                let g = str_to_number(&s.replacen('%', "", 1)) / 100.0;
                if !g.is_finite() || s.is_empty() {
                    return Err(bad());
                }
                let nf = fl::set_grade(&f, i, g);
                self.fl_set_z(id, &tr!("fl.undo.grade", n = i + 1), &nf);
            }
            "raise" => {
                let x = str_to_number(v);
                if !x.is_finite() || v.is_empty() || x == 0.0 {
                    return Err(bad());
                }
                let nf = fl::raise(&f, x);
                self.fl_set_z(id, &t("fl.undo.raise", &[]), &nf);
            }
            "surface" => {
                let eg = self.project.eg();
                let (nf, missing) = fl::from_surface(&f, eg.surface.as_ref(), None);
                if missing.len() == f.pts.len() {
                    self.message = t("fl.surface.none", &[]);
                    return Ok(());
                }
                self.fl_set_z(id, &t("fl.undo.surface", &[]), &nf);
                let mut m = t("fl.surface.done", &[]);
                if !missing.is_empty() {
                    let list: Vec<String> = missing.iter().map(|k| (k + 1).to_string()).collect();
                    m.push(' ');
                    m.push_str(&tr!("fl.surface.missing", list = list.join(", ")));
                }
                self.message = m;
            }
            "even" => {
                if f.pts.len() <= 2 {
                    return Err("civil.prop: Even grade needs three vertices or more".into());
                }
                let nf = fl::even_grade(&f, 0.0, (f.pts.len() - 1) as f64);
                self.fl_set_z(id, &t("fl.undo.even", &[]), &nf);
                self.message = t("fl.even.done", &[]);
            }
            "offset" => {
                // "distance,dz|slope,amount" (the Stepped offset block's three fields)
                let parts: Vec<&str> = v.splitn(3, ',').collect();
                let (d, how, amt) = match parts.as_slice() {
                    [d, h, a] => (*d, *h, *a),
                    _ => return Err("civil.prop: offset = \"distance,dz|slope,amount\"".into()),
                };
                if how != "dz" && how != "slope" {
                    return Err(bad());
                }
                let dd = str_to_number(d);
                let aa = str_to_number(&amt.replacen('%', "", 1));
                if !dd.is_finite() || !aa.is_finite() {
                    self.message = t("fl.offset.numbers", &[]);
                    return Ok(());
                }
                let (nf, problem) = if how == "dz" {
                    fl::offset_feature(&f, dd, Some(aa), None)
                } else {
                    fl::offset_feature(&f, dd, None, Some(aa / 100.0))
                };
                let Some(nf) = nf else {
                    self.message = problem.unwrap_or_default();
                    return Ok(());
                };
                let nid = self.project.next_id("f");
                let dtext = format!("{}{}", if dd > 0.0 { "+" } else { "" }, cc_civil::js::num_to_string(dd));
                let name = tr!("fl.offset.name", name = f.name.as_str(), d = dtext);
                let mut o = fj.as_object().cloned().unwrap_or_default();
                o.insert("id".into(), Value::String(nid.clone()));
                o.insert("name".into(), Value::String(slice16_range(&name, 0, Some(120))));
                o.insert("pts".into(), nf.pts.j());
                let item = Value::Object(o);
                self.project.change(&t("fl.undo.offset", &[]), &mut |s: &mut Value| {
                    if let Some(l) = s.get_mut("features").and_then(Value::as_array_mut) {
                        l.push(item.clone());
                    }
                });
                self.sel = Some(Sel::Feature(nid));
                self.profile.sel_pvi = None;
                self.points.picked.clear();
                self.message = tr!("fl.offset.done", name = name.as_str());
                let _ = units;
            }
            _ => return Err(format!("civil.prop: a feature line has no field {field}")),
        }
        Ok(())
    }

    fn parcel_prop(&mut self, id: &str, field: &str, v: &str) -> Result<(), String> {
        let bad = || format!("civil.prop: {field} = {v} is refused");
        let pj = find(self.project.state.get("parcels"), id).cloned().ok_or("civil.prop: no such parcel")?;
        let edit = |cv: &mut CivilView, label: &str, f: &mut dyn FnMut(&mut Map<String, Value>)| {
            cv.project.change(label, &mut |s: &mut Value| {
                if let Some(m) = find_mut(s.get_mut("parcels"), id) {
                    f(m);
                }
            });
        };
        match field {
            "name" => {
                let name = name_of(v, 120, &t("pc.default", &[]));
                edit(self, &t("pc.undo.rename", &[]), &mut |m| {
                    m.insert("name".into(), Value::String(name.clone()));
                });
            }
            "pob" => {
                let s = slice16_range(trim(&cc_civil::js::strip_controls(v, " ")), 0, Some(200));
                edit(self, &t("pc.undo.pob", &[]), &mut |m| {
                    if s.is_empty() {
                        m.shift_remove("pob");
                    } else {
                        m.insert("pob".into(), Value::String(s.clone()));
                    }
                });
            }
            "r" | "turn" => {
                let (i, s) = indexed(v).ok_or_else(bad)?;
                let pts = cc_civil::parcels::read_corners(pj.get("pts"));
                let cs = cc_civil::parcels::courses(&pts);
                let c = cs.get(i).ok_or_else(bad)?;
                let (from, to, curve) = match c {
                    cc_civil::parcels::Course::Line { from, to, .. } => (*from, *to, None),
                    cc_civil::parcels::Course::Curve { from, to, a } => (*from, *to, Some((a.r, a.turn))),
                };
                let chord = cc_civil::js::hypot(to.x - from.x, to.y - from.y);
                // parcels-ui.js setR: no radius = straight; else the bulge for that radius and turn
                let set = |cv: &mut CivilView, r: Option<f64>, dir: f64| {
                    let b = r.map(|r| dir * (libm::asin(chord / (2.0 * r)) / 2.0).jtan());
                    edit(cv, &t("pc.undo.curve", &[]), &mut |m| {
                        if let Some(p) = m.get_mut("pts").and_then(|x| x.get_mut(i)).and_then(Value::as_object_mut) {
                            match b {
                                None => {
                                    p.shift_remove("b");
                                }
                                Some(b) => {
                                    p.insert("b".into(), jnum(b));
                                }
                            }
                        }
                    });
                };
                let turn = curve.map_or(-1.0, |c| c.1);
                if field == "turn" {
                    let dir = match trim(s) {
                        "1" | "left" => 1.0,
                        "-1" | "right" => -1.0,
                        _ => return Err(bad()),
                    };
                    if let Some((r, _)) = curve {
                        set(self, Some(r), dir);
                    }
                    return Ok(());
                }
                let s = trim(s);
                if s.is_empty() {
                    set(self, None, turn);
                    return Ok(());
                }
                let r = str_to_number(s);
                if r.is_nan() || r < chord / 2.0 - 1e-9 {
                    return Err(bad());
                }
                set(self, Some(r.max(chord / 2.0)), turn);
            }
            _ => return Err(format!("civil.prop: a parcel has no field {field}")),
        }
        Ok(())
    }

    /// civilcraft.js `pointProps` commit: point-edit.js `editPoint` with one field.
    fn point_prop(&mut self, n: &str, field: &str, v: &str) -> Result<(), String> {
        if !matches!(field, "n" | "x" | "y" | "z" | "d") {
            return Err(format!("civil.prop: a point has no field {field} (n, x, y, z, d)"));
        }
        let mut patch = Map::new();
        patch.insert(field.into(), Value::String(v.to_string()));
        let r = self.project.edit_point(n, &patch, None);
        let msg = string_of(r.get("message"));
        if r.get("ok") != Some(&Value::Bool(true)) {
            if msg.is_empty() {
                return Ok(());
            }
            self.message = msg.clone();
            return Err(format!("civil.prop: {msg}"));
        }
        let nn = r.get("point").and_then(|p| p.get("n")).and_then(Value::as_str).unwrap_or(n).to_string();
        if nn != n {
            self.points.picked.clear();
            self.points.picked.insert(nn.clone());
            if matches!(self.sel, Some(Sel::Point(_))) {
                self.sel = Some(Sel::Point(nn));
            }
        }
        self.message = msg;
        Ok(())
    }

    pub(crate) fn project_prop(&mut self, field: &str, v: &str) -> Result<(), String> {
        let bad = || format!("civil.prop: {field} = {v} is refused");
        let has_net = self.project.state.get("network").is_some_and(Value::is_object);
        match field {
            "name" => {
                let name = name_of(v, 120, &t("project.untitled", &[]));
                self.project.change(&t("cv.undo.project.rename", &[]), &mut |s: &mut Value| {
                    if let Some(m) = s.as_object_mut() {
                        m.insert("name".into(), Value::String(name.clone()));
                    }
                });
            }
            "units" => {
                let u = if v == "m" { "m" } else { "ft" };
                self.project.change(&t("cv.undo.units", &[]), &mut |s: &mut Value| {
                    if let Some(m) = s.as_object_mut() {
                        m.insert("units".into(), Value::String(u.into()));
                    }
                });
                let name = cc_civil::units::unit_of(self.project.units()).name().to_lowercase();
                self.message = tr!("cv.units.now", units = name);
            }
            "maxEdge" => {
                let x = str_to_number(v);
                if x.is_nan() || x < 0.0 {
                    return Err(bad());
                }
                self.project.change(&t("cv.undo.maxedge", &[]), &mut |s: &mut Value| {
                    if let Some(m) = s.get_mut("surface").and_then(Value::as_object_mut) {
                        m.insert("maxEdge".into(), jnum(x));
                    }
                });
            }
            "net.type" if has_net => {
                if v != "storm" && v != "sanitary" {
                    return Err(bad());
                }
                self.project.change(&t("cv.undo.net.type", &[]), &mut |s: &mut Value| {
                    if let Some(m) = s.get_mut("network").and_then(Value::as_object_mut) {
                        m.insert("type".into(), Value::String(v.into()));
                    }
                });
            }
            "net.minSlope" | "net.minCover" | "net.minDrop" if has_net => {
                let (k, label, scale) = match field {
                    "net.minSlope" => ("minSlope", "cv.net.minslope", 100.0),
                    "net.minCover" => ("minCover", "cv.net.mincover", 1.0),
                    _ => ("minDrop", "cv.net.mindrop", 1.0),
                };
                let x = nonneg(v).ok_or_else(bad)?;
                self.project.change(&t(label, &[]), &mut |s: &mut Value| {
                    if let Some(m) = s.get_mut("network").and_then(|n| n.get_mut("rules")).and_then(Value::as_object_mut) {
                        m.insert(k.into(), jnum(x / scale));
                    }
                });
            }
            "net.auto" if has_net => self.set_inverts(None),
            f if f.starts_with("net.") => return Err("civil.prop: the project has no pipe network".into()),
            _ => return Err(format!("civil.prop: the project has no field {field}")),
        }
        Ok(())
    }
}

/// civilcraft.js structureProps sump: `Number(v)` ≥ 0 (empty = 0, as Number("") is).
fn nonneg_or_empty0(v: &str) -> Option<f64> {
    let x = str_to_number(v);
    (x >= 0.0).then_some(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn properties_are_edited_as_single_steps_and_refuse_bad_values() {
        let mut cv = CivilView::demo().unwrap();
        cv.sel = Some(Sel::Alignment("a1".into()));
        cv.prop_set("name", "  Elm Avenue ").unwrap();
        assert_eq!(cv.project.state["alignments"][0]["name"], "Elm Avenue");
        assert!(cv.prop_set("start", "nope").is_err());
        cv.prop_set("corridor", "on").unwrap();
        cv.prop_set("laneSlope", "2.5").unwrap();
        assert_eq!(cv.project.state["alignments"][0]["corridor"]["laneSlope"], serde_json::json!(0.025));
        assert!(cv.prop_set("step", "0").is_err());
        cv.prop_set("se", "on").unwrap();
        cv.prop_set("se.add", "").unwrap();
        cv.prop_set("se.e", "0:").unwrap();
        assert!(cv.prop_set("se.delta", "9").is_err());
        let n = cv.project.undo_stack.len();
        assert!(n >= 5, "{n}");
        cv.sel = None;
        cv.prop_set("name", "").unwrap();
        assert_eq!(cv.project.state["name"], t("project.untitled", &[]));
        cv.prop_set("net.minSlope", "1").unwrap();
        assert_eq!(cv.project.state["network"]["rules"]["minSlope"], serde_json::json!(0.01));
        cv.prop_set("net.auto", "").unwrap();
        assert!(!cv.message.is_empty());
    }
}
