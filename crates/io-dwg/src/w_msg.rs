//! The writer's report notes (lib/dwg/messages.js, the `ccdwg.write.*` keys, English), with msg()'s fills and plurals.

fn text(key: &str) -> Option<(&'static str, &'static str)> {
    Some(match key {
        "ccdwg.write.eed_dropped" => (
            "{n} extended-data block referring to objects this writer does not copy (materials, groups, …) was left out.",
            "{n} extended-data blocks referring to objects this writer does not copy (materials, groups, …) were left out.",
        ),
        "ccdwg.write.true_colours" => (
            "{n} true colour became the nearest AutoCAD colour index (R2000 stores colour indexes only).",
            "{n} true colours became the nearest AutoCAD colour index (R2000 stores colour indexes only).",
        ),
        "ccdwg.write.chars_lost" => (
            "{n} character outside the Basic Multilingual Plane (emoji and the like) became \"?\".",
            "{n} characters outside the Basic Multilingual Plane (emoji and the like) became \"?\".",
        ),
        "ccdwg.write.nongraphic" => (
            "{n} non-graphical object was copied from the source ({raw} bit for bit in the drawing's own version, {fields} from their fields).",
            "{n} non-graphical objects were copied from the source ({raw} bit for bit in the drawing's own version, {fields} from their fields).",
        ),
        "ccdwg.write.not_copied" => {
            let s = "Not copied (cc-dwg cannot write them in this version, or they depend on them): {list}.";
            (s, s)
        }
        "ccdwg.write.entities_dropped" => (
            "{n} {type} entity was left out: undecoded data can only be written back in the drawing's own version.",
            "{n} {type} entities were left out: undecoded data can only be written back in the drawing's own version.",
        ),
        "ccdwg.write.proxied" => {
            let s = "Written as proxies of their own class (undecoded data in another version, as AutoCAD saves them down): {list}.";
            (s, s)
        }
        "ccdwg.write.raw" => {
            let s = "Copied bit for bit (custom or proxy entities): {list}.";
            (s, s)
        }
        "ccdwg.write.acis_empty" => {
            ("{n} ACIS entity has no ACIS data (written empty).", "{n} ACIS entities have no ACIS data (written empty).")
        }
        "ccdwg.write.acis_sat" => (
            "{n} ACIS entity keeps its SAT text (as the data storage record; R2013+ files normally hold SAB there — SAT is not converted).",
            "{n} ACIS entities keep their SAT text (as the data storage record; R2013+ files normally hold SAB there — SAT is not converted).",
        ),
        "ccdwg.write.acis_sab" => (
            "{n} ACIS entity keeps its SAB data as ACIS version 2 in an R2000 file (AutoCAD 2000 itself writes SAT).",
            "{n} ACIS entities keep their SAB data as ACIS version 2 in an R2000 file (AutoCAD 2000 itself writes SAT).",
        ),
        "ccdwg.write.nurbs_down" => (
            "{n} NURBS surface was written as plain SURFACE ({version} has no NURBSURFACE; as AutoCAD and the ODA File Converter save it down, with the ACAD_XREC_ROUNDTRIP marker that turns it back into a NURBS surface in R2013+).",
            "{n} NURBS surfaces were written as plain SURFACE ({version} has no NURBSURFACE; as AutoCAD and the ODA File Converter save it down, with the ACAD_XREC_ROUNDTRIP marker that turns it back into a NURBS surface in R2013+).",
        ),
        "ccdwg.write.nurbs_up" => (
            "{n} surface saved down from NURBSURFACE was written as NURBSURFACE again (its NURBS fields were not kept by the save-down).",
            "{n} surfaces saved down from NURBSURFACE were written as NURBSURFACE again (its NURBS fields were not kept by the save-down).",
        ),
        "ccdwg.write.embedded" => (
            "{n} embedded profile/path entity of a surface (an encoding cc-dwg does not know) was left out: it is kept only in the drawing's own version.",
            "{n} embedded profile/path entities of a surface (an encoding cc-dwg does not know) were left out: it is kept only in the drawing's own version.",
        ),
        "ccdwg.write.table_fields" => (
            "{n} table cell field could not be written and kept its text.",
            "{n} table cell fields could not be written and kept their text.",
        ),
        _ => return None,
    })
}

/// msg(key, fills) for the writer's keys (unknown → the reader's catalog).
pub fn msg(key: &str, fills: &[(&str, &str)]) -> String {
    let Some((one, other)) = text(key) else { return crate::messages::msg(key, fills) };
    let fill = |k: &str| fills.iter().find(|(n, _)| *n == k).map(|(_, v)| *v);
    let s = if fill("n").map(crate::w_js::str_to_number) == Some(1.0) { one } else { other };
    let mut out = String::with_capacity(s.len().saturating_add(16));
    let mut rest = s;
    while let Some(i) = rest.find(['{', '}']) {
        let (head, tail) = rest.split_at(i);
        out.push_str(head);
        if let Some(t) = tail.strip_prefix("{{") {
            out.push('{');
            rest = t;
            continue;
        }
        if let Some(t) = tail.strip_prefix("}}") {
            out.push('}');
            rest = t;
            continue;
        }
        if tail.starts_with('}') {
            out.push('}');
            rest = tail.get(1..).unwrap_or("");
            continue;
        }
        match tail.find('}') {
            Some(j) => {
                let name = tail.get(1..j).unwrap_or_default();
                let ok = !name.is_empty()
                    && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                    && !name.starts_with(|c: char| c.is_ascii_digit());
                match fill(name) {
                    Some(v) if ok => out.push_str(v),
                    _ => out.push_str(tail.get(..=j).unwrap_or_default()),
                }
                rest = tail.get(j.saturating_add(1)..).unwrap_or_default();
            }
            None => {
                out.push_str(tail);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}
