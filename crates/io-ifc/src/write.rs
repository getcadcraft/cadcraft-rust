//! The writing half of model.js and step.js: lines written over an open file (`set` / `add` / `remove`), every live id
//! (`ids`), and `save` — the header kept (FILE_NAME's time stamp renewed), untouched lines copied byte for byte, written
//! ones formatted anew (`formatLine`, `formatValue`, `formatString`, `formatReal`), new lines last in id order.

use crate::model::{Edit, Id, IfcFile, id_key};
use crate::schema::Schema;
use crate::step::StepError;
use crate::value::{JsStr, Value};

/// `String(x)` for a finite number (JS shortest round trip, exponent as `e+21` / `e-7`).
pub fn js_num(x: f64) -> String {
    if x == 0.0 {
        return "0".into();
    }
    let a = x.abs();
    if (1e-6..1e21).contains(&a) {
        format!("{x}")
    } else {
        let s = format!("{x:e}");
        match s.split_once('e') {
            Some((m, e)) if !e.starts_with('-') => format!("{m}e+{e}"),
            _ => s,
        }
    }
}

/// `formatReal`: always with a ".", exponent as E.
pub fn format_real(x: f64) -> Result<String, StepError> {
    if !x.is_finite() {
        return Err(StepError("A real value must be finite".into()));
    }
    let s = js_num(if x == 0.0 { 0.0 } else { x });
    if let Some((m, ex)) = s.to_ascii_lowercase().split_once('e') {
        let m = if m.contains('.') { m.to_string() } else { format!("{m}.") };
        return Ok(format!("{m}E{}", ex.replace('+', "")));
    }
    Ok(if s.contains('.') { s } else { s + "." })
}

/// `formatInteger`.
pub fn format_integer(x: f64) -> Result<String, StepError> {
    if !x.is_finite() {
        return Err(StepError("An integer value must be finite".into()));
    }
    Ok(js_num(crate::jsmath::round(x)))
}

/// `formatString`: quotes and backslashes doubled, printable ASCII as is, other BMP code points in `\X2\…\X0\` runs,
/// code points past U+FFFF as `\X4\…\X0\` (a lone surrogate is a code point of its own).
pub fn format_string(s: &JsStr) -> String {
    let units = s.units();
    let mut out = String::from("'");
    let mut wide = String::new();
    let end_wide = |out: &mut String, wide: &mut String| {
        if !wide.is_empty() {
            out.push_str("\\X2\\");
            out.push_str(wide);
            out.push_str("\\X0\\");
            wide.clear();
        }
    };
    let mut i = 0usize;
    while let Some(&u) = units.get(i) {
        i = i.saturating_add(1);
        let mut cp = u32::from(u);
        if (0xD800..0xDC00).contains(&u)
            && let Some(&lo) = units.get(i)
            && (0xDC00..0xE000).contains(&lo)
        {
            cp = 0x10000u32.wrapping_add((u32::from(u).wrapping_sub(0xD800)) << 10).wrapping_add(u32::from(lo).wrapping_sub(0xDC00));
            i = i.saturating_add(1);
        }
        if (0x20..=0x7E).contains(&cp) {
            end_wide(&mut out, &mut wide);
            match cp {
                0x27 => out.push_str("''"),
                0x5C => out.push_str("\\\\"),
                _ => out.push(char::from_u32(cp).unwrap_or('?')),
            }
            continue;
        }
        if cp > 0xFFFF {
            end_wide(&mut out, &mut wide);
            out.push_str(&format!("\\X4\\{cp:08X}\\X0\\"));
            continue;
        }
        wide.push_str(&format!("{cp:04X}"));
    }
    end_wide(&mut out, &mut wide);
    out.push('\'');
    out
}

/// How a typed value decides whether its bare numbers are REALs.
#[derive(Clone, Copy)]
pub enum RealType<'s> {
    /// the header: always
    Always,
    /// a data line: the defined type's code is r / n
    Schema(&'s Schema),
}

impl RealType<'_> {
    fn is_real(&self, name: &str) -> bool {
        match self {
            RealType::Always => true,
            RealType::Schema(s) => s.type_code(name).is_some_and(|c| matches!(c.trim_start_matches('['), "r" | "n")),
        }
    }
}

/// `formatValue(v, real, realType)`: `real` = whether a bare number here is a REAL.
pub fn format_value(out: &mut String, v: &Value, real: bool, rt: RealType<'_>) -> Result<(), StepError> {
    match v {
        Value::Null => out.push('$'),
        Value::Derived => out.push('*'),
        Value::Num(n) => {
            let s = if real || n.fract() != 0.0 || !n.is_finite() { format_real(*n)? } else { format_integer(*n)? };
            out.push_str(&s);
        }
        Value::Str(s) => out.push_str(&format_string(s)),
        Value::List(l) => {
            out.push('(');
            for (k, x) in l.iter().enumerate() {
                if k > 0 {
                    out.push(',');
                }
                format_value(out, x, real, rt)?;
            }
            out.push(')');
        }
        Value::Ref(r) => {
            out.push('#');
            out.push_str(&js_num(*r));
        }
        Value::Enum(e) => {
            out.push('.');
            out.push_str(e);
            out.push('.');
        }
        Value::Bin(b) => {
            out.push('"');
            out.push_str(b);
            out.push('"');
        }
        Value::Typed(t) => {
            let is_real = rt.is_real(&t.0);
            out.push_str(&t.0);
            out.push('(');
            format_value(out, &t.1, is_real, rt)?;
            out.push(')');
        }
    }
    Ok(())
}

impl IfcFile<'_> {
    /// `m.set(id, type, params)`: a new line or a replacement (the type may change). Ids are positive whole numbers.
    pub fn set(&mut self, id: Id, ty: &str, params: Vec<Value>) -> Result<Id, StepError> {
        if !(id > 0.0 && id.fract() == 0.0 && id.is_finite()) {
            return Err(StepError("A line id must be a positive whole number.".into()));
        }
        let up = ty.to_ascii_uppercase();
        let k = id_key(id);
        self.gone.remove(&k);
        self.edits.insert(k, Edit { id, ty: up.into(), params: params.into_boxed_slice() });
        if id > self.max_id {
            self.max_id = id;
        }
        self.version = self.version.wrapping_add(1);
        self.clear_inverse();
        Ok(id)
    }

    /// `m.add(type, params)` → the new id (maxId + 1).
    pub fn add(&mut self, ty: &str, params: Vec<Value>) -> Result<Id, StepError> {
        self.set(self.max_id + 1.0, ty, params)
    }

    /// `m.remove(id)`: the line is gone (references to it stay as written). → whether there was one.
    pub fn remove(&mut self, id: Id) -> bool {
        if !self.has(id) {
            return false;
        }
        let k = id_key(id);
        self.edits.remove(&k);
        if self.rec(id).is_some() {
            self.gone.insert(k);
        }
        self.version = self.version.wrapping_add(1);
        self.clear_inverse();
        true
    }

    /// `m.dirty`
    pub fn dirty(&self) -> bool {
        !self.edits.is_empty() || !self.gone.is_empty()
    }

    /// Written ids that are not lines of the original file, in id order.
    pub fn new_ids(&self) -> Vec<Id> {
        let mut v: Vec<Id> = self.edits.values().filter(|e| self.rec(e.id).is_none()).map(|e| e.id).collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        v
    }

    /// `m.ids()`: every live id, file order then the new ones.
    pub fn ids(&self) -> Vec<Id> {
        let mut out = Vec::with_capacity(self.ix.count);
        for (i, &id) in self.ix.ids.iter().enumerate() {
            let f = f64::from(id);
            let k = id_key(f);
            if self.gone.contains(&k) {
                continue;
            }
            if self.edits.contains_key(&k) && self.rec(f) != Some(i) {
                continue;
            }
            out.push(f);
        }
        out.extend(self.new_ids());
        out
    }

    /// Is a bare number at attribute k of `ty` a REAL?
    fn real_at(&self, ty: &str, k: usize) -> bool {
        self.schema.entity(ty).and_then(|e| e.attrs.get(k)).is_some_and(|a| matches!(&*a.t.base, "r" | "n"))
    }

    /// `formatLine(id, type, params)`: "#id=TYPE(…);".
    pub fn format_line(&self, id: Id, ty: &str, params: &[Value]) -> Result<String, StepError> {
        let mut out = format!("#{}={ty}(", js_num(id));
        for (k, v) in params.iter().enumerate() {
            if k > 0 {
                out.push(',');
            }
            format_value(&mut out, v, self.real_at(ty, k), RealType::Schema(self.schema))?;
        }
        out.push_str(");");
        Ok(out)
    }

    /// The header section's statements (FILE_NAME's time stamp replaced when given).
    pub fn header_text(&self, time_stamp: Option<&str>) -> Result<String, StepError> {
        let raw = &self.ix.header.raw;
        let get = |k: &str| raw.iter().find(|(n, _)| n == k).map(|(_, v)| v);
        let s = |t: &str| Value::Str(JsStr::Utf8(t.into()));
        let desc = get("FILE_DESCRIPTION")
            .cloned()
            .unwrap_or_else(|| vec![Value::List(vec![s("ViewDefinition [CoordinationView]")].into()), s("2;1")]);
        // FILE_NAME as an array with possible holes (an index past its end, as `name[1] = ts` makes one)
        let mut name: Vec<Option<Value>> = get("FILE_NAME").map_or_else(
            || {
                vec![
                    Some(s("")),
                    Some(s("")),
                    Some(Value::List(vec![s("")].into())),
                    Some(Value::List(vec![s("")].into())),
                    Some(s("")),
                    Some(s("")),
                    Some(s("")),
                ]
            },
            |v| v.iter().cloned().map(Some).collect(),
        );
        if let Some(ts) = time_stamp.filter(|t| !t.is_empty()) {
            while name.len() < 2 {
                name.push(None);
            }
            if let Some(slot) = name.get_mut(1) {
                *slot = Some(s(ts));
            }
        }
        let schema = get("FILE_SCHEMA").cloned().unwrap_or_else(|| {
            let fs = self.ix.schema.lossy().into_owned();
            vec![Value::List(vec![s(if fs.is_empty() { self.schema_name } else { &fs })].into())]
        });
        let fmt = |vals: &[Option<Value>]| -> Result<String, StepError> {
            let mut parts = Vec::with_capacity(vals.len());
            for v in vals {
                let mut o = String::new();
                if let Some(v) = v {
                    format_value(&mut o, v, false, RealType::Always)?;
                }
                parts.push(o);
            }
            Ok(parts.join(","))
        };
        let some = |v: &[Value]| v.iter().cloned().map(Some).collect::<Vec<_>>();
        let mut out = format!("FILE_DESCRIPTION({});\n", fmt(&some(&desc))?);
        out.push_str(&format!("FILE_NAME({});\n", fmt(&name)?));
        out.push_str(&format!("FILE_SCHEMA({});\n", fmt(&some(&schema))?));
        for (k, v) in raw {
            if !matches!(k.as_str(), "FILE_DESCRIPTION" | "FILE_NAME" | "FILE_SCHEMA") {
                out.push_str(&format!("{k}({});\n", fmt(&some(v))?));
            }
        }
        Ok(out)
    }

    /// `m.save({ timeStamp })`: the whole file as bytes. Untouched lines are copied exactly; the header is kept with
    /// FILE_NAME's time stamp renewed (`time_stamp` = "YYYY-MM-DDTHH:MM:SS", the caller's clock).
    pub fn save(&self, time_stamp: Option<&str>) -> Result<Vec<u8>, StepError> {
        let b = self.bytes();
        let mut out: Vec<u8> = Vec::with_capacity(b.len().saturating_add(4096));
        out.extend_from_slice(b"ISO-10303-21;\nHEADER;\n");
        out.extend_from_slice(self.header_text(time_stamp)?.as_bytes());
        out.extend_from_slice(b"ENDSEC;\nDATA;\n");
        let ix = &self.ix;
        for (i, &id) in ix.ids.iter().enumerate() {
            let f = f64::from(id);
            let k = id_key(f);
            if self.gone.contains(&k) {
                continue;
            }
            if let Some(e) = self.edits.get(&k) {
                if self.rec(f) == Some(i) {
                    out.extend_from_slice(self.format_line(f, &e.ty, &e.params)?.as_bytes());
                    out.push(b'\n');
                }
                continue;
            }
            let (Some(&s), Some(&e)) = (ix.starts.get(i), ix.ends.get(i)) else { continue };
            let (Ok(s), Ok(e)) = (usize::try_from(s), usize::try_from(e)) else { continue };
            out.push(b'#');
            out.extend_from_slice(id.to_string().as_bytes());
            out.push(b'=');
            out.extend_from_slice(ix.type_name(i).as_bytes());
            if let Some(chunk) = b.get(s..=e) {
                out.extend_from_slice(chunk);
            }
            out.push(b'\n');
        }
        for id in self.new_ids() {
            if let Some(e) = self.edits.get(&id_key(id)) {
                out.extend_from_slice(self.format_line(id, &e.ty, &e.params)?.as_bytes());
                out.push(b'\n');
            }
        }
        out.extend_from_slice(b"ENDSEC;\nEND-ISO-10303-21;\n");
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reals_and_strings_as_step_js_writes_them() {
        assert_eq!(format_real(1.0).unwrap(), "1.");
        assert_eq!(format_real(-0.0).unwrap(), "0.");
        assert_eq!(format_real(1e-7).unwrap(), "1.E-7");
        assert_eq!(format_real(1.5e21).unwrap(), "1.5E21");
        assert_eq!(format_real(0.25).unwrap(), "0.25");
        assert!(format_real(f64::NAN).is_err());
        assert_eq!(format_string(&JsStr::Utf8("it's a\\b".into())), "'it''s a\\\\b'");
        assert_eq!(format_string(&JsStr::Utf8("m²".into())), "'m\\X2\\00B2\\X0\\'");
        assert_eq!(format_string(&JsStr::Utf8("😀".into())), "'\\X4\\0001F600\\X0\\'");
        assert_eq!(format_string(&JsStr::Utf16(vec![0xD800, 0x41].into())), "'\\X2\\D800\\X0\\A'");
    }

    #[test]
    fn edits_round_trip_through_save() {
        let text = "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('x'),'2;1');\nFILE_NAME('a.ifc','2020-01-01T00:00:00',(''),(''),'','','');\nFILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n#1= IFCCARTESIANPOINT((0.,0.,0.));\n#2=IFCDIRECTION((1.,0.,0.));\nENDSEC;\nEND-ISO-10303-21;\n";
        let mut f = IfcFile::open_owned(std::sync::Arc::from(text.as_bytes())).unwrap();
        f.set(2.0, "IFCDIRECTION", vec![Value::List(vec![Value::Num(0.0), Value::Num(1.0), Value::Num(0.0)].into())]).unwrap();
        let n = f.add("IFCCARTESIANPOINT", vec![Value::List(vec![Value::Num(1.5), Value::Num(2.0)].into())]).unwrap();
        assert_eq!(n, 3.0);
        assert_eq!(f.of_type("IFCCARTESIANPOINT", false), vec![1.0, 3.0]);
        let out = String::from_utf8(f.save(Some("2026-10-18T00:00:00")).unwrap()).unwrap();
        assert!(out.contains("FILE_NAME('a.ifc','2026-10-18T00:00:00',(''),(''),'','','');"), "{out}");
        assert!(
            out.contains("#1=IFCCARTESIANPOINT((0.,0.,0.));\n#2=IFCDIRECTION((0.,1.,0.));\n#3=IFCCARTESIANPOINT((1.5,2.));\n"),
            "{out}"
        );
        assert!(f.remove(1.0));
        assert_eq!(f.ids(), vec![2.0, 3.0]);
        assert!(f.ty(1.0).is_none());
    }
}
