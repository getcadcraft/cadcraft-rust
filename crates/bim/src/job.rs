//! A parity job: models to open and cases to run on them, as JSON in and JSON out (`cadcraft-cli bim <job.json>`;
//! `forks/tools/bimdiff.mjs` runs the same job on the frozen JS). Each case's result is its module function's output
//! in the JS shape (Maps as `[key, value]` pairs in Map order, large typed arrays as digests), or `{ "$error": text }`.

use std::collections::HashMap;

use cc_io_ifc::geo::bool_ops::SolidOps;
use cc_io_ifc::model::IfcFile;
use serde_json::{Map, Value as J};

use crate::digest;
use crate::js::{PV, jn};
use crate::model::{Batch, Model};
use crate::props::{Index, idf};
use crate::{bcf, checks, clash, compare, elements, find, model, props, qto, tree, util, vis};

/// One opened model: its file and load result (or the load error).
pub struct Opened<'a> {
    pub file: Option<IfcFile<'a>>,
    pub model: Result<Model, String>,
}

/// Open and load each model of a job (`models: [{ keyBase }]`, bytes given by the caller in the same order).
pub fn open_models<'a>(bytes: &'a [Vec<u8>], specs: &[J], ops: &dyn SolidOps) -> Vec<Opened<'a>> {
    bytes
        .iter()
        .zip(specs.iter())
        .map(|(b, s)| {
            let kb = s.get("keyBase").and_then(J::as_f64).unwrap_or(0.0);
            match IfcFile::open(b) {
                Ok(f) => {
                    f.parse_all();
                    let m = model::load(&f, ops, kb).map_err(|e| e.0);
                    Opened { file: Some(f), model: m }
                }
                // ifc-model.js: t('ifc.open.failed')
                Err(_) => Opened { file: None, model: Err("ifc.open.failed".into()) },
            }
        })
        .collect()
}

fn err_json(e: &str) -> J {
    let mut o = Map::new();
    o.insert("$error".into(), J::String(e.to_string()));
    J::Object(o)
}

fn keys_json(v: &[f64]) -> J {
    J::Array(v.iter().map(|k| jn(*k)).collect())
}

fn nums(v: Option<&J>) -> Vec<f64> {
    v.and_then(J::as_array).map(|a| a.iter().filter_map(J::as_f64).collect()).unwrap_or_default()
}

fn strs(v: Option<&J>) -> Vec<String> {
    v.and_then(J::as_array)
        .map(|a| a.iter().map(|x| x.as_str().map_or_else(|| cc_cad::js::to_js_string(x), str::to_string)).collect())
        .unwrap_or_default()
}

fn m16(v: &J) -> Option<[f64; 16]> {
    let a = v.as_array()?;
    let mut m = [0.0; 16];
    for (i, slot) in m.iter_mut().enumerate() {
        *slot = a.get(i)?.as_f64()?;
    }
    Some(m)
}

fn batch_json(b: &Batch) -> J {
    let mut o = Map::new();
    o.insert("storey".into(), jn(b.storey));
    o.insert("space".into(), J::Bool(b.space));
    o.insert("color".into(), J::Array(b.color.iter().map(|x| jn(*x)).collect()));
    o.insert("positions".into(), digest::f32s(&b.positions));
    o.insert("normals".into(), digest::f32s(&b.normals));
    o.insert("indices".into(), digest::u32s(&b.indices));
    o.insert("triIds".into(), digest::u32s(&b.tri_ids));
    let mut e = Map::new();
    e.insert("ids".into(), J::Array(b.elems.ids.iter().map(|x| J::from(*x)).collect()));
    e.insert("t0".into(), digest::u32s(&b.elems.t0));
    e.insert("tn".into(), digest::u32s(&b.elems.tn));
    e.insert("v0".into(), digest::u32s(&b.elems.v0));
    e.insert("vn".into(), digest::u32s(&b.elems.vn));
    o.insert("elems".into(), J::Object(e));
    J::Object(o)
}

fn tnode_json(n: &model::TNode, depth: usize) -> J {
    let mut o = Map::new();
    o.insert("expressID".into(), jn(n.id));
    o.insert("type".into(), J::String(n.ty.clone()));
    o.insert(
        "children".into(),
        J::Array(if depth < 80 { n.children.iter().map(|c| tnode_json(c, depth.saturating_add(1))).collect() } else { Vec::new() }),
    );
    J::Object(o)
}

/// The load result as JSON (batches as digests).
pub fn model_json(m: &Model) -> J {
    let mut o = Map::new();
    o.insert("keyBase".into(), jn(m.key_base));
    o.insert("schema".into(), J::String(m.schema.clone()));
    o.insert(
        "storeys".into(),
        J::Array(
            m.storeys
                .iter()
                .map(|s| {
                    let mut x = Map::new();
                    x.insert("id".into(), jn(s.id));
                    x.insert("name".into(), s.name.json());
                    x.insert("elevation".into(), s.elevation.map_or(J::Null, jn));
                    J::Object(x)
                })
                .collect(),
        ),
    );
    o.insert("storeyOf".into(), J::Array(m.storey_of.iter().map(|(k, v)| J::Array(vec![jn(idf(*k)), jn(*v)])).collect()));
    o.insert(
        "classOf".into(),
        J::Array(m.class_ids.iter().map(|id| J::Array(vec![jn(*id), J::String(m.class(*id).unwrap_or("").into())])).collect()),
    );
    o.insert("tree".into(), m.tree.as_ref().map_or(J::Null, |t| tnode_json(t, 0)));
    o.insert("batches".into(), J::Array(m.batches.iter().map(batch_json).collect()));
    o.insert("elements".into(), J::from(m.elements));
    o.insert("spaces".into(), J::from(m.spaces));
    o.insert("triangles".into(), jn(m.triangles));
    o.insert(
        "types".into(),
        J::Array(
            m.types
                .iter()
                .map(|(n, c)| {
                    let mut x = Map::new();
                    x.insert("name".into(), J::String(n.clone()));
                    x.insert("count".into(), J::from(*c));
                    J::Object(x)
                })
                .collect(),
        ),
    );
    o.insert("coord".into(), J::Array(m.coord.iter().map(|x| jn(*x)).collect()));
    J::Object(o)
}

struct Ctx<'o, 'a> {
    opened: &'o [Opened<'a>],
    ix: HashMap<(usize, bool), Result<Index, String>>,
}

impl<'o, 'a> Ctx<'o, 'a> {
    fn model(&self, i: usize) -> Result<(&'o IfcFile<'a>, &'o Model), String> {
        let o = self.opened.get(i).ok_or("no such model")?;
        let m = o.model.as_ref().map_err(String::clone)?;
        let f = o.file.as_ref().ok_or("no file")?;
        Ok((f, m))
    }
    fn index(&mut self, i: usize, spaces: bool) -> Result<&Index, String> {
        if !self.ix.contains_key(&(i, spaces)) {
            let r = self.model(i).and_then(|(f, m)| props::property_index(f, m, spaces).map_err(|e| e.0));
            self.ix.insert((i, spaces), r);
        }
        match self.ix.get(&(i, spaces)) {
            Some(Ok(ix)) => Ok(ix),
            Some(Err(e)) => Err(e.clone()),
            None => Err("no index".into()),
        }
    }
}

fn mi(c: &J) -> usize {
    c.get("model").and_then(J::as_u64).and_then(|x| usize::try_from(x).ok()).unwrap_or(0)
}

/// A side's matrix: null | { ref, offset } → placementMatrix(coord(ref), coord(model), offset).
fn side_matrix(ctx: &Ctx<'_, '_>, model: usize, spec: Option<&J>) -> Result<Option<[f64; 16]>, String> {
    let Some(s) = spec.filter(|s| !s.is_null()) else { return Ok(None) };
    if let Some(m) = s.as_array().and_then(|_| m16(s)) {
        return Ok(Some(m));
    }
    let r = s.get("ref").and_then(J::as_u64).and_then(|x| usize::try_from(x).ok()).unwrap_or(0);
    let off = nums(s.get("offset"));
    let offset = [off.first().copied().unwrap_or(0.0), off.get(1).copied().unwrap_or(0.0), off.get(2).copied().unwrap_or(0.0)];
    let (_, rm) = ctx.model(r)?;
    let (_, mm) = ctx.model(model)?;
    Ok(Some(util::placement_matrix(Some(&rm.coord), &mm.coord, offset)))
}

fn with_index<'c>(ctx: &'c mut Ctx<'_, '_>, c: &J, owned: &'c mut Option<Index>) -> Result<&'c Index, String> {
    if let Some(s) = c.get("index") {
        *owned = Some(Index::from_json(s));
        return owned.as_ref().ok_or_else(|| "index".to_string());
    }
    let spaces = c.get("spaces").and_then(J::as_bool).unwrap_or(false);
    ctx.index(mi(c), spaces)
}

fn coloring_out(ix: &Index, c: &J) -> J {
    let field = c.get("field").and_then(J::as_str).unwrap_or("cls");
    let dark = c.get("theme").and_then(J::as_str) == Some("dark");
    props::color_by(ix, field, dark).json()
}

fn read_scheme_json(c: &J) -> J {
    let r = props::read_scheme(c.get("text").and_then(J::as_str), c.get("obj"));
    let mut o = Map::new();
    match r {
        Ok(s) => {
            o.insert("ok".into(), J::Bool(true));
            o.insert("scheme".into(), s.json());
        }
        Err(e) => {
            o.insert("ok".into(), J::Bool(false));
            o.insert("error".into(), J::String(e.into()));
        }
    }
    J::Object(o)
}

fn vis_from_ops(v: &mut vis::Visibility, op: &J) -> J {
    let keys = nums(op.get("keys"));
    match op.get("op").and_then(J::as_str).unwrap_or("") {
        "hide" => J::from(v.hide(&keys)),
        "unhide" => J::from(v.unhide(&keys)),
        "isolate" => {
            v.isolate(&keys, cc_cad::js::truthy(op.get("ghost")));
            J::Null
        }
        "showAll" => {
            v.show_all();
            J::Null
        }
        "prune" => {
            let drop: std::collections::HashSet<u64> = keys.iter().map(|k| cc_io_ifc::model::id_key(*k)).collect();
            J::from(v.prune(|k| !drop.contains(&cc_io_ifc::model::id_key(k))))
        }
        "load" => {
            let s = op.get("state");
            let hidden = nums(s.and_then(|s| s.get("hidden")));
            let iso = s.and_then(|s| s.get("isolated")).filter(|x| cc_cad::js::truthy(Some(x))).map(|x| nums(Some(x)));
            v.load(&hidden, iso.as_deref(), cc_cad::js::truthy(s.and_then(|s| s.get("ghost"))));
            J::Null
        }
        _ => J::Null,
    }
}

fn vis_state_json(v: &vis::Visibility, batches: &[Batch]) -> J {
    let mut o = Map::new();
    o.insert("json".into(), v.to_json());
    o.insert("plain".into(), J::Bool(v.plain()));
    o.insert("version".into(), J::from(v.version));
    let per: Vec<J> = batches
        .iter()
        .map(|b| {
            let (index, tri_ids, ns, ng, own) = vis::batch_index(b, Some(v));
            let edges = vis::batch_edges(b, 30.0);
            let (ei, es, eg, ea) = vis::edge_index(&edges, Some(v));
            let mut x = Map::new();
            x.insert("index".into(), digest::u32s(&index));
            x.insert("triIds".into(), digest::u32s(&tri_ids));
            x.insert("nSolid".into(), J::from(ns));
            x.insert("nGhost".into(), J::from(ng));
            x.insert("own".into(), J::Bool(own));
            x.insert("edgeIndex".into(), digest::u32s(&ei));
            x.insert("eSolid".into(), J::from(es));
            x.insert("eGhost".into(), J::from(eg));
            x.insert("eAll".into(), J::from(ea));
            J::Object(x)
        })
        .collect();
    o.insert("batches".into(), J::Array(per));
    J::Object(o)
}

type Placed<'m> = Vec<(&'m Batch, Option<[f64; 16]>)>;

fn parts_batches<'m>(ctx: &Ctx<'m, '_>, parts: Option<&J>) -> Result<Placed<'m>, String> {
    let mut out = Vec::new();
    for p in parts.and_then(J::as_array).map(Vec::as_slice).unwrap_or(&[]) {
        let i = mi(p);
        let (_, m) = ctx.model(i)?;
        let mx = side_matrix(ctx, i, p.get("matrix"))?;
        for b in &m.batches {
            out.push((b, mx));
        }
    }
    Ok(out)
}

fn elements_from(ctx: &Ctx<'_, '_>, spec: Option<&J>) -> Result<Vec<clash::Element>, String> {
    let s = spec.ok_or("no set")?;
    let bs = parts_batches(ctx, s.get("parts"))?;
    let refs: Vec<(&Batch, Option<&[f64; 16]>)> = bs.iter().map(|(b, m)| (*b, m.as_ref())).collect();
    Ok(clash::elements_of(&refs, &nums(s.get("keys"))))
}

fn run_case(ctx: &mut Ctx<'_, '_>, c: &J) -> Result<J, String> {
    let op = c.get("op").and_then(J::as_str).unwrap_or("");
    let mut owned = None;
    Ok(match op {
        "load" => model_json(ctx.model(mi(c))?.1),
        "index" => props::index_json(with_index(ctx, c, &mut owned)?),
        "colorBy" => coloring_out(with_index(ctx, c, &mut owned)?, c),
        "readScheme" => read_scheme_json(c),
        "applyScheme" => {
            let ix = with_index(ctx, c, &mut owned)?;
            let s = props::Scheme::from_json(c.get("scheme").unwrap_or(&J::Null));
            props::apply_scheme(ix, &s).json()
        }
        "scheme" => {
            // schemeOf(colorBy(ix, field), ix field) → readScheme(JSON) → applyScheme(ix, scheme)
            let ix = with_index(ctx, c, &mut owned)?;
            let field = c.get("field").and_then(J::as_str).unwrap_or("cls");
            let dark = c.get("theme").and_then(J::as_str) == Some("dark");
            let col = props::color_by(ix, field, dark);
            // schemeOf(c, undefined) reads `field.id` of undefined: a field the index lacks throws in the JS
            let fj = c
                .get("fieldObj")
                .cloned()
                .or_else(|| ix.field(field).map(props::Field::json))
                .ok_or("TypeError: Cannot read properties of undefined (reading 'id')")?;
            let sc = props::scheme_of(&col, &fj);
            let text = sc.to_string();
            let mut o = Map::new();
            o.insert("scheme".into(), sc);
            match props::read_scheme(Some(&text), None) {
                Ok(s) => {
                    o.insert("read".into(), s.json());
                    o.insert("applied".into(), props::apply_scheme(ix, &s).json());
                }
                Err(e) => {
                    o.insert("readError".into(), J::String(e.into()));
                }
            }
            J::Object(o)
        }
        "find" => {
            let ix = with_index(ctx, c, &mut owned)?;
            let rules: Vec<find::Rule> =
                c.get("rules").and_then(J::as_array).map(|a| a.iter().filter_map(find::Rule::from_json).collect()).unwrap_or_default();
            keys_json(&find::find(ix, &rules, c.get("mode").and_then(J::as_str) == Some("any")))
        }
        "suggestions" => {
            let ix = with_index(ctx, c, &mut owned)?;
            let limit = c.get("limit").and_then(J::as_u64).and_then(|x| usize::try_from(x).ok()).unwrap_or(200);
            J::Array(find::suggestions(ix, c.get("field").and_then(J::as_str).unwrap_or(""), limit).into_iter().map(J::String).collect())
        }
        "cleanSets" => {
            J::Array(find::clean_sets(c.get("text").and_then(J::as_str), c.get("obj")).iter().map(find::SearchSet::json).collect())
        }
        "totals" => {
            let ix = with_index(ctx, c, &mut owned)?;
            let keys = c.get("keys").filter(|k| !k.is_null()).map(|k| nums(Some(k)));
            let gb = c.get("groupBy").map_or_else(|| vec!["cls".to_string()], |g| strs(Some(g)));
            let t = qto::totals(ix, keys.as_deref(), &gb, &strs(c.get("sum")));
            let csv = qto::totals_csv(&t, &qto::CsvWords::from_json(c.get("words")));
            let mut o = Map::new();
            o.insert("res".into(), t.json());
            o.insert("csv".into(), J::String(csv));
            J::Object(o)
        }
        "totalsCsv" => {
            J::String(qto::totals_csv(&qto::Totals::from_json(c.get("res").unwrap_or(&J::Null)), &qto::CsvWords::from_json(c.get("words"))))
        }
        "elementRows" => {
            let ix = with_index(ctx, c, &mut owned)?;
            let keys = c.get("keys").filter(|k| !k.is_null()).map(|k| nums(Some(k)));
            let cols = c.get("columns").map_or_else(|| vec!["cls".to_string(), "name".to_string()], |g| strs(Some(g)));
            let rows = qto::element_rows(ix, keys.as_deref(), &cols);
            let mut o = Map::new();
            o.insert("columns".into(), J::Array(cols.iter().map(|s| J::String(s.clone())).collect()));
            o.insert(
                "rows".into(),
                J::Array(
                    rows.iter()
                        .map(|(k, v)| {
                            let mut r = Map::new();
                            r.insert("key".into(), jn(*k));
                            r.insert("values".into(), J::Array(v.iter().map(qto::Cell::json).collect()));
                            J::Object(r)
                        })
                        .collect(),
                ),
            );
            J::Object(o)
        }
        "spaceSchedule" => {
            let i = mi(c);
            let keys = c.get("keys").filter(|k| !k.is_null()).map(|k| nums(Some(k)));
            let spaces = c.get("spaces").and_then(J::as_bool).unwrap_or(true);
            ctx.index(i, spaces)?;
            let (_, m) = ctx.model(i)?;
            let ix = ctx.index(i, spaces)?;
            let (rows, n, area, volume) = qto::space_schedule(ix, &m.batches, keys.as_deref());
            let mut t = Map::new();
            t.insert("count".into(), J::from(n));
            t.insert("area".into(), jn(area));
            t.insert("volume".into(), jn(volume));
            let mut o = Map::new();
            o.insert("rows".into(), J::Array(rows.iter().map(qto::SpaceRow::json).collect()));
            o.insert("total".into(), J::Object(t));
            J::Object(o)
        }
        "quantityFields" => {
            J::Array(qto::quantity_fields(with_index(ctx, c, &mut owned)?).into_iter().map(|f| J::String(f.id.clone())).collect())
        }
        "tableCsv" => {
            let head: Vec<qto::Cell> =
                c.get("head").and_then(J::as_array).map(|a| a.iter().map(|x| qto::Cell::from_json(Some(x))).collect()).unwrap_or_default();
            let rows: Vec<Vec<qto::Cell>> = c
                .get("rows")
                .and_then(J::as_array)
                .map(|a| {
                    a.iter()
                        .map(|r| r.as_array().map(|x| x.iter().map(|v| qto::Cell::from_json(Some(v))).collect()).unwrap_or_default())
                        .collect()
                })
                .unwrap_or_default();
            J::String(qto::table_csv(&head, &rows))
        }
        "checks" => {
            let i = mi(c);
            ctx.index(i, false)?;
            let (_, m) = ctx.model(i)?;
            let ix = ctx.index(i, false)?;
            let req: Vec<Option<checks::Required>> = match c.get("required") {
                Some(J::Array(a)) => a.iter().map(|r| Some(checks::Required::from_json(r))).collect(),
                _ => vec![Some(checks::Required::from_json(&serde_json::json!({ "cls": "", "field": "name" })))],
            };
            let tol = c.get("tolerance").and_then(J::as_f64).unwrap_or(0.001);
            J::Array(checks::run_checks(ix, m, &req, tol).iter().map(checks::Check::json).collect())
        }
        "cleanRules" => match checks::clean_rules(c.get("text").and_then(J::as_str), c.get("obj")) {
            None => J::Null,
            Some(v) => J::Array(
                v.into_iter()
                    .map(|(cls, f)| {
                        let mut o = Map::new();
                        o.insert("cls".into(), J::String(cls));
                        o.insert("field".into(), J::String(f));
                        J::Object(o)
                    })
                    .collect(),
            ),
        },
        "compare" => {
            let sa = c.get("a").ok_or("a")?;
            let sb = c.get("b").ok_or("b")?;
            let (ia, ib) = (mi(sa), mi(sb));
            ctx.index(ia, false)?;
            ctx.index(ib, false)?;
            let ma = side_matrix(ctx, ia, sa.get("matrix"))?;
            let mb = side_matrix(ctx, ib, sb.get("matrix"))?;
            let (_, mda) = ctx.model(ia)?;
            let (_, mdb) = ctx.model(ib)?;
            let ixa = ctx.ix.get(&(ia, false)).and_then(|r| r.as_ref().ok()).ok_or("index a")?;
            let ixb = ctx.ix.get(&(ib, false)).and_then(|r| r.as_ref().ok()).ok_or("index b")?;
            let tol = c.get("tolerance").and_then(J::as_f64).unwrap_or(0.001);
            let r = compare::compare_models(
                compare::Side { ix: ixa, model: mda, matrix: ma.as_ref() },
                compare::Side { ix: ixb, model: mdb, matrix: mb.as_ref() },
                tol,
            );
            let w = c.get("csv");
            let words = compare::CompareWords {
                words: w
                    .and_then(|w| w.get("words"))
                    .and_then(J::as_object)
                    .map(|o| o.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string())).collect())
                    .unwrap_or_default(),
                labels: w
                    .and_then(|w| w.get("labels"))
                    .and_then(J::as_object)
                    .map(|o| o.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string())).collect())
                    .unwrap_or_default(),
                use_index: w.and_then(|w| w.get("useIndex")).and_then(J::as_bool).unwrap_or(false),
            };
            let mut o = Map::new();
            o.insert("res".into(), r.json());
            o.insert("csv".into(), J::String(compare::compare_csv(&r, &words, ixa, ixb)));
            J::Object(o)
        }
        "elementsOf" => {
            let els = elements_from(ctx, Some(c))?;
            J::Array(els.iter().map(|e| e.json(true)).collect())
        }
        "clash" => {
            let a = elements_from(ctx, c.get("a"))?;
            let b = elements_from(ctx, c.get("b"))?;
            let tol = c.get("tolerance").and_then(J::as_f64).unwrap_or(0.01);
            J::Array(clash::clashes(&a, &b, tol).iter().map(clash::Clash::json).collect())
        }
        "triTri" => {
            let t: Vec<f32> = nums(c.get("t")).into_iter().map(crate::js::f32_of).collect();
            let u: Vec<f32> = nums(c.get("u")).into_iter().map(crate::js::f32_of).collect();
            clash::tri_tri(&t, 0, &u, 0).map_or(J::Null, |p| J::Array(p.iter().map(|x| jn(*x)).collect()))
        }
        "vis" => {
            let (_, m) = ctx.model(mi(c))?;
            let mut v = vis::Visibility::default();
            let mut steps = vec![vis_state_json(&v, &m.batches)];
            for op in c.get("ops").and_then(J::as_array).map(Vec::as_slice).unwrap_or(&[]) {
                let r = vis_from_ops(&mut v, op);
                let mut s = vis_state_json(&v, &m.batches);
                if let J::Object(o) = &mut s {
                    o.insert("ret".into(), r);
                    let probe = nums(op.get("probe"));
                    o.insert("states".into(), J::Array(probe.iter().map(|k| J::from(v.state(*k))).collect()));
                }
                steps.push(s);
            }
            J::Array(steps)
        }
        "edges" => {
            let (_, m) = ctx.model(mi(c))?;
            let angle = c.get("angle").and_then(J::as_f64).unwrap_or(30.0);
            J::Array(
                m.batches
                    .iter()
                    .map(|b| {
                        let e = vis::batch_edges(b, angle);
                        let mut o = Map::new();
                        o.insert("positions".into(), digest::f32s(&e.positions));
                        o.insert("ids".into(), J::Array(e.ids.iter().map(|x| J::from(*x)).collect()));
                        o.insert("s0".into(), digest::u32s(&e.s0));
                        o.insert("sn".into(), J::Array(e.sn.iter().map(|x| J::from(*x)).collect()));
                        J::Object(o)
                    })
                    .collect(),
            )
        }
        "bounds" => {
            let (_, m) = ctx.model(mi(c))?;
            let refs: Vec<&Batch> = m.batches.iter().collect();
            match vis::bounds_of(&refs, &nums(c.get("keys"))) {
                None => J::Null,
                Some((lo, hi)) => {
                    let mut o = Map::new();
                    o.insert("min".into(), J::Array(lo.iter().map(|x| jn(*x)).collect()));
                    o.insert("max".into(), J::Array(hi.iter().map(|x| jn(*x)).collect()));
                    J::Object(o)
                }
            }
        }
        "keysOf" => {
            let (_, m) = ctx.model(mi(c))?;
            let refs: Vec<&Batch> = m.batches.iter().collect();
            keys_json(&vis::keys_of(&refs))
        }
        "tree" => {
            let (f, m) = ctx.model(mi(c))?;
            let t = tree::build_tree(f, m);
            let mut v = vis::Visibility::default();
            for op in c.get("visOps").and_then(J::as_array).map(Vec::as_slice).unwrap_or(&[]) {
                vis_from_ops(&mut v, op);
            }
            let mut o = Map::new();
            o.insert("root".into(), t.root.map_or(J::Null, |r| t.node_json(r)));
            o.insert("loose".into(), t.loose.map_or(J::Null, |r| t.node_json(r)));
            o.insert("leavesRoot".into(), t.root.map_or(J::Null, |r| keys_json(&t.leaves(r))));
            o.insert("leavesLoose".into(), t.loose.map_or(J::Null, |r| keys_json(&t.leaves(r))));
            // checkState of every node, pre-order from the root then the loose group
            let mut states = Vec::new();
            for start in [t.root, t.loose].into_iter().flatten() {
                let mut stack = vec![start];
                while let Some(n) = stack.pop() {
                    let Some(x) = t.node(n) else { continue };
                    states.push(J::Array(vec![x.key.json(), J::String(t.check_state(n, &v).into())]));
                    stack.extend(x.kids.iter().rev());
                }
            }
            o.insert("states".into(), J::Array(states));
            let paths: Vec<J> = nums(c.get("paths"))
                .iter()
                .map(|k| J::Array(t.path_to(*k).into_iter().filter_map(|i| t.node(i).map(|x| x.key.json())).collect()))
                .collect();
            o.insert("paths".into(), J::Array(paths));
            o.insert("shown".into(), J::Array(nums(c.get("paths")).iter().map(|k| J::Bool(tree::shown(&v, *k))).collect()));
            J::Object(o)
        }
        "whyNot" => {
            let (f, _) = ctx.model(mi(c))?;
            J::Array(nums(c.get("ids")).iter().map(|id| J::String(elements::why_not(f, *id).into())).collect())
        }
        "relations" => {
            let (f, _) = ctx.model(mi(c))?;
            elements::relations(f).map_err(|e| e.0)?.json()
        }
        "expand" => {
            let (f, _) = ctx.model(mi(c))?;
            let r = elements::relations(f).map_err(|e| e.0)?;
            let out = elements::expand(&nums(c.get("list")), &r, cc_cad::js::truthy(c.get("withFilledOpenings")));
            J::Array(out.iter().map(elements::RV::json).collect())
        }
        "bcfWrite" => {
            let topics = c.get("topics").and_then(J::as_array).cloned().unwrap_or_default();
            let file = c.get("file").filter(|f| !f.is_null());
            let bytes = bcf::write_bcf(&topics, file)?;
            let mut o = Map::new();
            o.insert("zip".into(), digest::bytes(&bytes));
            o.insert("markup".into(), J::Array(topics.iter().map(|t| J::String(bcf::markup_xml(t, file))).collect()));
            o.insert(
                "viewpoints".into(),
                J::Array(
                    topics
                        .iter()
                        .flat_map(|t| t.get("viewpoints").and_then(J::as_array).cloned().unwrap_or_default())
                        .map(|vp| J::String(bcf::viewpoint_xml(&vp)))
                        .collect(),
                ),
            );
            if let Some(p) = c.get("out").and_then(J::as_str) {
                std::fs::write(p, &bytes).map_err(|e| e.to_string())?;
            }
            J::Object(o)
        }
        "coords" => {
            let cm = c.get("C").and_then(m16).ok_or("C")?;
            let v = nums(c.get("v"));
            let v3 = [v.first().copied().unwrap_or(f64::NAN), v.get(1).copied().unwrap_or(f64::NAN), v.get(2).copied().unwrap_or(f64::NAN)];
            let r = match c.get("fn").and_then(J::as_str).unwrap_or("") {
                "toIfcPoint" => bcf::to_ifc_point(&cm, &v3),
                "toIfcDir" => bcf::to_ifc_dir(&cm, &v3),
                "fromIfcPoint" => bcf::from_ifc_point(&cm, &v3),
                _ => bcf::from_ifc_dir(&cm, &v3),
            };
            J::Array(r.iter().map(|x| jn(*x)).collect())
        }
        "placementMatrix" => {
            let r = c.get("refC").and_then(m16);
            let cm = c.get("C").and_then(m16).ok_or("C")?;
            let off = nums(c.get("offset"));
            let o = [off.first().copied().unwrap_or(0.0), off.get(1).copied().unwrap_or(0.0), off.get(2).copied().unwrap_or(0.0)];
            J::Array(util::placement_matrix(r.as_ref(), &cm, o).iter().map(|x| jn(*x)).collect())
        }
        _ => return Err(format!("unknown op {op}")),
    })
}

/// Raw-JSON cases (results with UTF-16 strings that serde_json cannot hold).
fn run_raw(c: &J) -> Option<String> {
    let op = c.get("op").and_then(J::as_str)?;
    let read_file = |c: &J| -> Result<Vec<u8>, String> {
        let p = c.get("file").and_then(J::as_str).ok_or("file")?;
        std::fs::read(p).map_err(|e| e.to_string())
    };
    match op {
        "bcfRead" => Some(match read_file(c) {
            Err(e) => err_json(&e).to_string(),
            Ok(b) => match bcf::read_bcf(&b) {
                Ok(r) => r.json(),
                Err(e) => err_json(&e).to_string(),
            },
        }),
        "unzip" => Some(match read_file(c) {
            Err(e) => err_json(&e).to_string(),
            Ok(b) => match bcf::unzip(&b) {
                Err(e) => err_json(&e).to_string(),
                Ok(m) => {
                    let mut o = String::from("[");
                    for (i, (k, v)) in m.iter().enumerate() {
                        if i > 0 {
                            o.push(',');
                        }
                        o.push('[');
                        cc_io_ifc::util::push_json_str16(&mut o, k);
                        o.push(',');
                        o.push_str(&digest::bytes(v).to_string());
                        o.push(']');
                    }
                    o.push(']');
                    o
                }
            },
        }),
        "xml" => {
            let text = c.get("text").and_then(J::as_str).unwrap_or("");
            Some(bcf::xml_json(&crate::xml::u16s(text)))
        }
        _ => None,
    }
}

/// Run a job → the JSON text `{ "models": [load error | null], "results": [...] }`.
pub fn run_job(job: &J, opened: &[Opened<'_>]) -> String {
    let mut ctx = Ctx { opened, ix: HashMap::new() };
    let mut out = String::from("{\"models\":[");
    for (i, o) in opened.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        match &o.model {
            Ok(_) => out.push_str("null"),
            Err(e) => out.push_str(&J::String(e.clone()).to_string()),
        }
    }
    out.push_str("],\"results\":[");
    for (i, c) in job.get("cases").and_then(J::as_array).map(Vec::as_slice).unwrap_or(&[]).iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        if let Some(raw) = run_raw(c) {
            out.push_str(&raw);
            continue;
        }
        let r = run_case(&mut ctx, c).unwrap_or_else(|e| err_json(&e));
        out.push_str(&r.to_string());
    }
    out.push_str("]}");
    out
}

/// `PV` as a plain JSON value (helper for callers).
pub fn pv_json(v: &PV) -> J {
    v.json()
}
