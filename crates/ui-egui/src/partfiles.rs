//! PartCraft's DXF of a sketch or a flat face and the design-table CSV import in the app (M13.8): partcraft.js
//! `exportDxf` (export.js `dxfSketch` / `dxfLoops` with faces.js `faceLoops2D` on the face picked in the view) and
//! params-panel.js `importTable` (configs.js `rowsFromTable`: each row becomes a configuration, one undo step).
//! Commands `part.export=dxf` (the open or selected sketch, else the picked flat face), `part.export=dxf:<sketch>`
//! (a sketch panel's Export DXF), `part.config=import` (the platform's picker; a CSV dropped on an open part too).

use cc_part::{configs as cfg, export as X, faces as FC, jv, part as P, vec as V};
use serde_json::Value;

use crate::partview::{PartView, tr, trv};

/// Largest design table read (params-panel.js reads the first 4 MB).
pub const MAX_TABLE: usize = 4 * 1024 * 1024;

/// `name.replace(/[^A-Za-z0-9 _.-]+/g, '_')`.
fn file_safe(s: &str) -> String {
    let mut out = String::new();
    let mut run = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '.' | '-') {
            out.push(c);
            run = false;
        } else if !run {
            out.push('_');
            run = true;
        }
    }
    out
}

impl PartView {
    /// partcraft.js `exportDxf(sf)`: (file, bytes, words), or None with the status line saying why not.
    pub fn export_dxf(&mut self, sketch: Option<&str>) -> Result<Option<(String, Vec<u8>, String)>, String> {
        let f = match sketch {
            Some(k) => Some(
                P::feature_by_id(&self.part, &self.feature_id(k).ok_or_else(|| format!("part.export: no feature {k}"))?)
                    .cloned()
                    .ok_or("gone")?,
            ),
            None => self
                .editing
                .as_deref()
                .filter(|_| self.mode == crate::partview::Mode::Sketch)
                .and_then(|id| P::feature_by_id(&self.part, id))
                .or_else(|| {
                    self.sel_feature.as_deref().and_then(|id| P::feature_by_id(&self.part, id)).filter(|f| jv::s(f, "type") == "sketch")
                })
                .cloned(),
        };
        if let Some(f) = f.filter(|f| jv::s(f, "type") == "sketch") {
            let (text, counts) = X::dxf_sketch(f.get("sketch").unwrap_or(&Value::Null));
            let file = format!("{} - {}.dxf", self.file_base(), file_safe(jv::s(&f, "name")));
            let n = |k: &str| cc_part::js::string_of(counts.get(k));
            let words = trv(
                "app.export.dxf_sketch",
                &[("file", file.clone()), ("lines", n("line")), ("arcs", n("arc")), ("circles", n("circle")), ("points", n("point"))],
            );
            return Ok(Some((file, text.into_bytes(), words)));
        }
        if let (Some(pk), Some(m)) = (self.pick.as_ref().filter(|p| p.face && p.flat), self.regen.raw_mesh.as_ref()) {
            let (_, groups) = FC::planar_groups(m, 1e-5);
            let face = i32::try_from(pk.index).ok();
            if let Some(g) = groups.iter().find(|g| g.face == face && face.is_some()) {
                let (_, loops, area) = FC::face_loops_2d(m, g);
                let (text, _) = X::dxf_loops(&loops);
                let file = format!("{} - face.dxf", self.file_base());
                let words = trv("app.export.dxf_face", &[("n", loops.len().to_string()), ("area", V::fmt(area.abs(), 3))]);
                return Ok(Some((file, text.into_bytes(), words)));
            }
        }
        self.note = tr("app.export.dxf_nothing");
        Ok(None)
    }

    /// params-panel.js `importTable(file)`: the CSV's rows become configurations (one undo step).
    pub fn import_table(&mut self, file: &str, bytes: &[u8]) {
        let p = cc_part::params_part::params_of(&self.part);
        if self.part.get("params").is_none_or(|v| !v.is_object()) || p.vars.is_empty() {
            self.note = tr("configs.need_params");
            return;
        }
        let text = String::from_utf8_lossy(bytes.get(..bytes.len().min(MAX_TABLE)).unwrap_or(bytes));
        // String(rd.result).slice(0, 4 MB): UTF-16 units; the lossy text is cut at a char boundary
        let text: String = text.chars().take(MAX_TABLE).collect();
        let rows = match cfg::rows_from_table(&text, &p) {
            Ok(r) => r,
            Err(e) => {
                self.note = e;
                return;
            }
        };
        let cur = self.configs();
        let n = rows.len();
        let r = self.param_edit_cfg(&tr("configs.undo.import"), |_, _| {
            let mut c = cur.clone();
            for row in &rows {
                c = Some(cfg::save_row(c.as_ref(), row)?);
            }
            Ok(Some(c))
        });
        match r {
            Ok(_) => {
                let name: String = file.chars().take(120).collect();
                self.note = trv("configs.imported", &[("n", n.to_string()), ("file", name)]);
            }
            Err(e) => self.note = e,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Params, UiApp};
    use std::sync::{Arc, Mutex};

    fn run(app: &mut UiApp, id: &str, name: &str) {
        app.run(id, &Params { name: Some(name.into()), ..Default::default() }).unwrap_or_else(|e| panic!("{id}={name}: {e}"));
    }

    #[test]
    fn dxf_of_a_sketch_and_a_face_and_a_design_table() {
        let saved: Arc<Mutex<Vec<(String, usize)>>> = Arc::default();
        let mut app = UiApp::default();
        let s2 = saved.clone();
        app.services.save = Some(Box::new(move |name: &str, bytes: Vec<u8>| {
            s2.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((name.to_string(), bytes.len()));
            Ok(Some(name.to_string()))
        }));
        app.services.pick_files = Some(Box::new(|_, _| {
            Some(vec![crate::ImportedFile { name: "t.csv".into(), bytes: b"name,length\r\nA,90\r\nB,140\r\n".to_vec(), dir: None }])
        }));
        run(&mut app, "part.sample", "");
        run(&mut app, "part.export", "dxf:Profile");
        run(&mut app, "part.face", "f2.end");
        run(&mut app, "part.export", "dxf");
        let names: Vec<String> = saved.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().map(|x| x.0.clone()).collect();
        assert_eq!(names, vec!["Bracket - Profile.dxf".to_string(), "Bracket - face.dxf".to_string()]);
        run(&mut app, "part.config", "import");
        let pv = app.part.as_ref().unwrap();
        assert_eq!(pv.configs().map(|c| c.rows.len()), Some(2));
        // hostile bytes: a note, no panic
        let mut pv = crate::partview::PartView::sample().unwrap();
        pv.import_table("x.csv", &[0xff, 0xfe, b',', b'\n', 0, 0]);
        assert!(!pv.note.is_empty());
    }
}
