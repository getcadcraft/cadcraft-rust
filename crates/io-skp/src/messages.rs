//! Every sentence cc-skp says to a person (port of `lib/skp/messages.js`): keys `ccskp.*`, the same English, the same
//! fill rules (`{name}`, `{{` / `}}`, plural `{one, other}` chosen by the fill `n`, English rules). The apps translate
//! by key through their own catalogs; this crate returns the English, exactly like the JS in Node.

enum Text {
    Plain(&'static str),
    Plural(&'static str, &'static str),
}

fn text(key: &str) -> Option<Text> {
    use Text::{Plain as P, Plural as N};
    Some(match key {
        // errors (SkpError messages: read.js, vff.js, zip.js, legacy.js)
        "ccskp.not_skp" => P("This is not a SketchUp file."),
        "ccskp.damaged" => P("This SketchUp file is damaged: {reason}."),
        "ccskp.too_new" => P("This file was saved by SketchUp {version}, newer than cc-skp reads (up to {newest})."),
        "ccskp.unsupported.pre2021" => P("SketchUp {version} files (saved before SketchUp 2021) cannot be read yet."),
        "ccskp.unsupported.no_decompress" => P("This browser cannot unpack SketchUp 2021+ files (no DecompressionStream)."),
        "ccskp.unsupported.zip64" => P("This SketchUp file uses a ZIP64 archive, which cc-skp does not read."),
        "ccskp.unsupported.multipart" => P("This SketchUp file is a multi-part archive, which cc-skp does not read."),
        "ccskp.unsupported.encrypted" => P("This SketchUp file is encrypted, which cc-skp does not read."),
        "ccskp.unsupported.method" => P("This SketchUp file packs \"{name}\" with a method cc-skp does not read ({method})."),
        "ccskp.too_big.file" => P("This SketchUp file is larger than cc-skp reads ({size})."),
        "ccskp.too_big.part" => P("A part of this SketchUp file is larger than cc-skp reads ({mb} MB)."),
        "ccskp.too_big.part_unpacks" => P("This SketchUp file unpacks to more than cc-skp reads ({mb} MB in one part)."),
        "ccskp.too_big.unpacked" => P("This SketchUp file unpacks to more than cc-skp reads."),
        "ccskp.too_big.nesting" => P("Components in this SketchUp file nest deeper than cc-skp reads ({levels} levels)."),
        "ccskp.too_big.points" => P("This SketchUp file has more points than cc-skp reads ({max})."),
        "ccskp.too_big.objects" => P("This SketchUp file has more objects than cc-skp reads."),
        "ccskp.too_big.object_depth" => P("This SketchUp file nests objects deeper than cc-skp reads ({levels} levels)."),
        "ccskp.too_big.vertices" => P("This SketchUp file has more than {max} vertices."),
        "ccskp.too_big.stack" => P("This SketchUp file nests too deeply to read."),
        "ccskp.legacy.no_version" => P("This file has no SketchUp version in its header."),
        "ccskp.legacy.too_old" => P("SketchUp {major} files are older than cc-skp reads (SketchUp 3 and later)."),
        "ccskp.legacy.unknown_major" => P("A SketchUp {major} file in the classic layout is not something cc-skp knows."),
        "ccskp.legacy.failed" => P("This SketchUp file could not be read: {reason}."),
        "ccskp.legacy.failed_internal" => P("This SketchUp file could not be read ({reason})."),

        // what is damaged: the {reason} of ccskp.damaged
        "ccskp.damaged.past_end" => P("a record runs past the end of the file"),
        "ccskp.damaged.header_cut" => P("it ends inside its header"),
        "ccskp.damaged.no_version" => P("its version is missing"),
        "ccskp.damaged.no_version_map" => P("its class list (version map) is missing"),
        "ccskp.damaged.record_header" => P("a record header is cut short"),
        "ccskp.damaged.record_past" => P("a record runs past its container"),
        "ccskp.damaged.id_size" => P("an id has an impossible size"),
        "ccskp.damaged.no_archive" => P("the model archive is missing"),
        "ccskp.damaged.no_model_dat" => P("model.dat is missing from the archive"),
        "ccskp.damaged.no_geometry" => P("the model has no geometry list"),
        "ccskp.damaged.def_id_twice" => P("two definitions share one id"),
        "ccskp.damaged.self_nested" => P("a component contains itself"),
        "ccskp.damaged.placement_short" => P("an instance placement is cut short"),
        "ccskp.damaged.placement_scale" => P("an instance placement has no scale"),
        "ccskp.damaged.placement_numbers" => P("an instance placement has impossible numbers"),
        "ccskp.damaged.vertex_position" => P("a vertex has no position"),
        "ccskp.damaged.vertex_coords" => P("a vertex has impossible coordinates"),
        "ccskp.damaged.face_edge" => P("a face edge reference is incomplete"),
        "ccskp.damaged.inflate" => P("a compressed part does not unpack"),
        "ccskp.damaged.no_end" => P("the end of the archive is missing (the file may be cut off)"),
        "ccskp.damaged.dir_outside" => P("the archive directory points outside the file"),
        "ccskp.damaged.dir_broken" => P("the archive directory is broken"),
        "ccskp.damaged.entry_missing" => P("\"{name}\" is missing from the archive"),
        "ccskp.damaged.entry_outside" => P("\"{name}\" points outside the archive"),
        "ccskp.damaged.entry_past_end" => P("\"{name}\" runs past the end of the file"),
        "ccskp.damaged.entry_size" => P("\"{name}\" has the wrong size"),
        "ccskp.damaged.entry_unpacked" => P("\"{name}\" unpacks to the wrong size"),
        "ccskp.damaged.entry_crc" => P("\"{name}\" fails its checksum"),

        // classic files (legacy.js): the {reason} of ccskp.legacy.failed
        "ccskp.legacy.text_record" => P("expected a text record at byte {at}"),
        "ccskp.legacy.text_long" => P("text record longer than the file at byte {at}"),
        "ccskp.legacy.class_name" => P("implausible class name length at byte {at}"),
        "ccskp.legacy.class_unknown" => P("reference to an unknown class at byte {at}"),
        "ccskp.legacy.ref_as_class" => P("object reference used as a class at byte {at}"),
        "ccskp.legacy.type_unknown" => P("unknown object type {type} at byte {at}"),
        "ccskp.legacy.ref_unread" => P("reference to an object not read yet at byte {at}"),
        "ccskp.legacy.ref_class_record" => P("reference to a class record at byte {at}"),
        "ccskp.legacy.vertex_range" => P("vertex coordinates out of range at byte {at}"),
        "ccskp.legacy.edge_curve" => P("edge curve pointer is a {type} at byte {at}"),
        "ccskp.legacy.edgeuse_parent" => P("edge use parent is a new object at byte {at}"),
        "ccskp.legacy.edgeuse_loop" => P("edge use does not belong to its loop at byte {at}"),
        "ccskp.legacy.loop_count" => P("implausible loop count at byte {at}"),
        "ccskp.legacy.attr_array" => P("implausible attribute array at byte {at}"),
        "ccskp.legacy.attr_type" => P("unknown attribute value type {type} at byte {at}"),
        "ccskp.legacy.texture_image" => P("texture without an image at byte {at}"),
        "ccskp.legacy.texture_size" => P("texture size block misaligned at byte {at}"),
        "ccskp.legacy.image_long" => P("image longer than the file at byte {at}"),
        "ccskp.legacy.texture_pins" => P("implausible texture pin count at byte {at}"),
        "ccskp.legacy.entity_ref" => P("entity reference is a new object at byte {at}"),
        "ccskp.legacy.dim_path" => P("implausible dimension path at byte {at}"),
        "ccskp.legacy.text_label" => P("text label layout not recognised at byte {at}"),
        "ccskp.legacy.def_layers" => P("implausible definition layer count at byte {at}"),
        "ccskp.legacy.def_entities" => P("implausible definition entity count at byte {at}"),
        "ccskp.legacy.def_misaligned" => P("definition record misaligned at byte {at}"),
        "ccskp.legacy.def_end" => P("definition record end not found at byte {at}"),
        "ccskp.legacy.inst_not_def" => P("instance of something that is not a definition at byte {at}"),
        "ccskp.legacy.material_count" => P("implausible material count"),
        "ccskp.legacy.no_lists" => P("no materials or layers found"),
        "ccskp.legacy.numbering" => P("cannot find where the model data is numbered from"),
        "ccskp.legacy.layer_count" => P("implausible layer count"),
        "ccskp.legacy.layer_end" => P("the layer list did not end where expected"),
        "ccskp.legacy.def_list" => P("the definition list does not start where expected"),
        "ccskp.legacy.def_count" => P("implausible definition count"),
        "ccskp.legacy.entity_count" => P("implausible model entity count"),
        "ccskp.legacy.no_numbering" => P("no way to number the model data was found"),

        // notes: SketchUp 2021+ files (vff.js)
        "ccskp.note.material_xml" => N("{n} material description could not be read", "{n} material descriptions could not be read"),
        "ccskp.note.material_no_xml" => {
            N("{n} material without a description (shown with no color)", "{n} materials without a description (shown with no color)")
        }
        "ccskp.note.texture_missing" => N(
            "{n} texture missing from the file (its material keeps its color)",
            "{n} textures missing from the file (their materials keep their color)",
        ),
        "ccskp.note.texture_no_size" => N(
            "{n} texture without a tile size skipped (its material keeps its color)",
            "{n} textures without a tile size skipped (their materials keep their color)",
        ),
        "ccskp.note.unit_unknown" => P("Display unit \"{unit}\" not recognised"),
        "ccskp.note.styles" => N("{n} style not read", "{n} styles not read"),
        "ccskp.note.material_no_id" => N("{n} material without an id skipped", "{n} materials without an id skipped"),
        "ccskp.note.tag_no_id" => N("{n} tag without an id skipped", "{n} tags without an id skipped"),
        "ccskp.note.scenes" => N("{n} scene not read", "{n} scenes not read"),
        "ccskp.note.def_no_geometry" => N("{n} definition without geometry skipped", "{n} definitions without geometry skipped"),
        "ccskp.note.def_no_id" => N("{n} definition without an id skipped", "{n} definitions without an id skipped"),
        "ccskp.note.inst_missing_def" => N("{n} instance of a missing definition skipped", "{n} instances of a missing definition skipped"),
        "ccskp.note.material_ref" => N("{n} reference to a missing material ignored", "{n} references to a missing material ignored"),
        "ccskp.note.tag_ref" => N("{n} reference to a missing tag ignored", "{n} references to a missing tag ignored"),
        "ccskp.note.inst_no_def" => N("{n} instance without a definition skipped", "{n} instances without a definition skipped"),
        "ccskp.note.inst_no_place" => {
            N("{n} instance without a placement put at the origin", "{n} instances without a placement put at the origin")
        }
        "ccskp.note.edge_no_ends" => N("{n} edge with missing ends skipped", "{n} edges with missing ends skipped"),
        "ccskp.note.guide_lines" => N("{n} guide line not read", "{n} guide lines not read"),
        "ccskp.note.guide_points" => N("{n} guide point not read", "{n} guide points not read"),
        "ccskp.note.annotations" => N("{n} dimension or text not read", "{n} dimensions and texts not read"),
        "ccskp.note.other_lists" => N(
            "{n} list of other drawing elements (record {record}) not read",
            "{n} lists of other drawing elements (record {record}) not read",
        ),
        "ccskp.note.face_open" => N("{n} face whose outline does not close skipped", "{n} faces whose outline does not close skipped"),
        "ccskp.note.face_turned" => {
            N("{n} face outline turned to face its front side", "{n} face outlines turned to face their front side")
        }
        "ccskp.note.edge_missing_end" => N("{n} edge with a missing end point skipped", "{n} edges with a missing end point skipped"),

        // notes: classic files (legacy.js)
        "ccskp.note.text_labels" => N("{n} text label not read.", "{n} text labels not read."),
        "ccskp.note.dimensions" => N("{n} dimension not read.", "{n} dimensions not read."),
        "ccskp.note.section_planes" => N("{n} section plane not read.", "{n} section planes not read."),
        "ccskp.note.guides" => N("{n} guide line or point not read.", "{n} guide lines and points not read."),
        "ccskp.note.face_corners" => {
            N("{n} face with fewer than three corners left out.", "{n} faces with fewer than three corners left out.")
        }
        "ccskp.note.inst_broken" => N(
            "{n} instance with an unknown definition or a broken transform left out.",
            "{n} instances with an unknown definition or a broken transform left out.",
        ),
        "ccskp.note.material_unknown" => N(
            "{n} material reference to a material not in the file read as default.",
            "{n} material references to a material not in the file read as default.",
        ),
        "ccskp.note.texture_format" => N(
            "{n} texture stored in a format browsers can't show (e.g. Photoshop): use the material color.",
            "{n} textures stored in a format browsers can't show (e.g. Photoshop): use the material color.",
        ),
        "ccskp.note.texture_projected" => N(
            "{n} face with a projected texture: texture coordinates are approximate.",
            "{n} faces with a projected texture: texture coordinates are approximate.",
        ),
        "ccskp.note.face_flipped" => N(
            "{n} face whose outline runs against its stored front side (kept as stored).",
            "{n} faces whose outline runs against its stored front side (kept as stored).",
        ),
        "ccskp.note.root_cut" => N(
            "The model's entity list declared {n} more entry than could be read; reading stopped at the first one it could not.",
            "The model's entity list declared {n} more entries than could be read; reading stopped at the first one it could not.",
        ),
        "ccskp.note.v3" => P(
            "SketchUp 3 file: edge flags (soft, smooth, hidden) and entity materials other than the front face color are not stored in a way cc-skp can read.",
        ),
        "ccskp.note.not_read" => P("Scenes, styles and the camera are not read."),
        _ => return None,
    })
}

/// True when `key` is in the table (tests).
pub fn has(key: &str) -> bool {
    text(key).is_some()
}

/// `msg(key, fills)`: the English text for `key` with its fills; an unknown key returns the key itself.
/// Fills are `(name, value)` pairs, values already turned into text the way JS `String(v)` would.
pub fn msg(key: &str, fills: &[(&str, &str)]) -> String {
    let fill = |name: &str| fills.iter().find(|(k, _)| *k == name).map(|(_, v)| *v);
    let s = match text(key) {
        None => return key.to_string(),
        Some(Text::Plain(s)) => s,
        // s['=' + n] ?? (Number(n) === 1 ? s.one : s.other): the table has no '=n' forms.
        Some(Text::Plural(one, other)) => {
            if fill("n").is_some_and(|n| js_number(n) == Some(1.0)) {
                one
            } else {
                other
            }
        }
    };
    replace_fills(s, &fill)
}

/// `Number(text)` for the plural choice: the fills cc-skp passes are plain integers; anything else is "not 1".
fn js_number(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return Some(0.0);
    }
    t.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// One pass of `/\{\{|\}\}|\{([A-Za-z_][A-Za-z0-9_]*)\}/g`: `{{` → `{`, `}}` → `}`, `{name}` → its fill (kept as is
/// when the fill is missing). Inserted values are never scanned again.
fn replace_fills<'a>(s: &str, fill: &dyn Fn(&str) -> Option<&'a str>) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len().saturating_add(16));
    let mut i = 0usize;
    let mut lit = 0usize; // start of the pending literal run
    while i < b.len() {
        let c = b.get(i).copied().unwrap_or(0);
        let next = b.get(i.saturating_add(1)).copied();
        if c == b'{' && next == Some(b'{') {
            out.push_str(s.get(lit..i).unwrap_or(""));
            out.push('{');
            i = i.saturating_add(2);
            lit = i;
            continue;
        }
        if c == b'}' && next == Some(b'}') {
            out.push_str(s.get(lit..i).unwrap_or(""));
            out.push('}');
            i = i.saturating_add(2);
            lit = i;
            continue;
        }
        if c == b'{' && next.is_some_and(|n| n.is_ascii_alphabetic() || n == b'_') {
            let mut j = i.saturating_add(2);
            while b.get(j).is_some_and(|x| x.is_ascii_alphanumeric() || *x == b'_') {
                j = j.saturating_add(1);
            }
            if b.get(j) == Some(&b'}') {
                let name = s.get(i.saturating_add(1)..j).unwrap_or("");
                out.push_str(s.get(lit..i).unwrap_or(""));
                match fill(name) {
                    Some(v) => out.push_str(v),
                    None => out.push_str(s.get(i..=j).unwrap_or("")),
                }
                i = j.saturating_add(1);
                lit = i;
                continue;
            }
        }
        i = i.saturating_add(1);
    }
    out.push_str(s.get(lit..).unwrap_or(""));
    out
}

/// `msg('ccskp.damaged', { reason: msg(reason_key, fills) })`.
pub fn damaged_text(reason_key: &str, fills: &[(&str, &str)]) -> String {
    let reason = msg(reason_key, fills);
    msg("ccskp.damaged", &[("reason", &reason)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_plurals_and_escapes() {
        assert_eq!(damaged_text("ccskp.damaged.no_version", &[]), "This SketchUp file is damaged: its version is missing.");
        assert_eq!(msg("ccskp.note.styles", &[("n", "1")]), "1 style not read");
        assert_eq!(msg("ccskp.note.styles", &[("n", "3")]), "3 styles not read");
        assert_eq!(msg("ccskp.note.unit_unknown", &[("unit", "{n}")]), "Display unit \"{n}\" not recognised");
        assert_eq!(msg("ccskp.legacy.type_unknown", &[("at", "5")]), "unknown object type {type} at byte 5");
        assert_eq!(msg("nope.key", &[]), "nope.key");
        assert_eq!(replace_fills("a{{b}}c{x}{1}", &|k| (k == "x").then_some("X")), "a{b}cX{1}");
    }
}
