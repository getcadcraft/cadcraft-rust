//! The words io.js, xml.js, solids.js and skp-import.js say: the English strings of ModelCraft 0.5's 'lang/en.json'
//! for the keys they use (copied unchanged). A key not here falls back to cc-m3d's table, then to the key itself.

const EN: &[(&str, &str)] = &[
    ("app.import.default", "Import"),
    ("app.import.skp.bad", "This is not a SketchUp model ModelCraft can read."),
    ("app.import.skp.component", "Component"),
    ("app.import.skp.empty", "This SketchUp model has nothing to show: no faces, edges, groups or components."),
    ("app.import.skp.group", "Group"),
    ("app.import.skp.skip.images", "pictures"),
    ("app.import.skp.skip.nested", "components nested too deep or inside themselves"),
    ("app.solid.a_solid", "A solid"),
    ("app.solid.empty", "Nothing is left: the result is empty."),
    ("app.solid.empty.intersect", "These solids do not overlap: nothing is shared."),
    ("app.solid.gone", "The open group is gone."),
    ("app.solid.intersect", "Intersect"),
    ("app.solid.need.subtract", "Subtract needs a solid to cut and a solid to take away."),
    ("app.solid.need.two", "This needs two solids or more."),
    (
        "app.solid.open",
        "{name} is not a solid: every edge must join exactly two faces. Select it: Entity info says why, and Fix mends the simple cases.",
    ),
    ("app.solid.subtract", "Subtract"),
    ("app.solid.union", "Union"),
    ("app.undo.import_dae", "Import Collada"),
    ("app.undo.import_gltf", "Import glTF"),
    ("app.undo.import_obj", "Import OBJ"),
    ("app.undo.import_site", "Import site plan"),
    ("app.undo.import_skp", "Import SketchUp model"),
    ("m3d.dwg.this_is_not_a_dwg_drawing", "This is not a DWG drawing (its first bytes are not a DWG version mark)."),
    ("m3d.dwg.unknown", "unknown"),
    ("m3d.dwg.unknown_reason", "unknown reason"),
    (
        "m3d.dwg.unreadable",
        "This drawing could not be read ({why}). It may be damaged, or saved in a DWG version the reader does not handle ({version}).",
    ),
    ("m3d.io.a_buffer_has_no_data", "a buffer has no data"),
    ("m3d.io.a_buffer_is_wrong", "a buffer is wrong"),
    ("m3d.io.a_buffer_view_is_missing", "a buffer view is missing"),
    ("m3d.io.a_buffer_view_points_at_a", "a buffer view points at a missing buffer"),
    ("m3d.io.a_buffer_view_runs_past_its", "a buffer view runs past its buffer"),
    ("m3d.io.a_buffer_view_stride_is_wrong", "a buffer view stride is wrong"),
    ("m3d.io.a_chunk_runs_past_the_end", "a chunk runs past the end of the file"),
    ("m3d.io.a_node_transform_is_not_a", "a node transform is not a number"),
    ("m3d.io.a_position_source_has_no_data", "a position source has no data"),
    ("m3d.io.a_vertex_is_not_a_number", "a vertex is not a number or is more than 10,000 km out"),
    ("m3d.io.an_accessor_has_an_unknown_number", "an accessor has an unknown number type"),
    ("m3d.io.an_accessor_has_an_unknown_shape", "an accessor has an unknown shape"),
    ("m3d.io.an_accessor_is_missing", "an accessor is missing"),
    ("m3d.io.an_accessor_is_too_big", "an accessor is too big"),
    ("m3d.io.an_accessor_runs_past_its_buffer", "an accessor runs past its buffer view"),
    ("m3d.io.an_index_points_past_the_vertices", "an index points past the vertices"),
    ("m3d.io.bad_face", "bad face"),
    ("m3d.io.cameras_and_lights", "cameras and lights"),
    ("m3d.io.character_no_glyph_drawn_as_a", "character (no glyph, drawn as a box)"),
    ("m3d.io.convex_meshes", "convex meshes"),
    ("m3d.io.curves_and_surfaces", "curves and surfaces"),
    ("m3d.io.dae_bad", "This Collada file could not be read: {why}."),
    ("m3d.io.dae_root", "This is not a Collada (.dae) file (its root element is <{name}>)."),
    ("m3d.io.dae_short_source", "a position source in \"{name}\" is shorter than it says"),
    ("m3d.io.dae_vcount", "a polylist's vcount runs past its indices"),
    ("m3d.io.degenerate_face", "degenerate face"),
    ("m3d.io.dimension_missing_block", "DIMENSION (missing block)"),
    ("m3d.io.dimension_nested_too_deep", "DIMENSION (nested too deep)"),
    ("m3d.io.dxf_code", "This file is not a readable DXF (bad group code on line {line})."),
    ("m3d.io.gltf_bad", "This glTF file could not be read: {why}."),
    ("m3d.io.gltf_external", "This glTF file keeps its data in a separate file (\"{uri}\"). Export it as a single .glb and open that."),
    ("m3d.io.insert_missing_block", "INSERT (missing block)"),
    ("m3d.io.insert_nested_too_deep", "INSERT (nested too deep)"),
    ("m3d.io.it_has_no_json_part", "it has no JSON part"),
    ("m3d.io.it_is_neither_a_glb_nor", "it is neither a .glb nor .gltf JSON"),
    ("m3d.io.its_json_part_is_broken", "its JSON part is broken"),
    ("m3d.io.lines", "lines"),
    ("m3d.io.meshes_without_positions", "meshes without positions"),
    ("m3d.io.missing", "missing <{kind}> elements"),
    ("m3d.io.nodes_nested_too_deep_or_inside", "nodes nested too deep or inside themselves"),
    ("m3d.io.obj_vertex", "This OBJ file has a vertex that is not a number (line \"{line}\")."),
    ("m3d.io.only_gltf_2_0_is_supported", "only glTF 2.0 is supported"),
    ("m3d.io.points", "points"),
    ("m3d.io.positions_are_not_3d", "positions are not 3D"),
    ("m3d.io.primitive_without_positions", "primitive without positions"),
    ("m3d.io.references_to_other_files", "references to other files"),
    ("m3d.io.skinned_or_morphed_meshes", "skinned or morphed meshes"),
    ("m3d.io.sparse_accessors_are_not_supported", "sparse accessors are not supported"),
    ("m3d.io.spline_no_points", "SPLINE (no points)"),
    ("m3d.io.text_over_the_text_limit", "TEXT (over the text limit)"),
    ("m3d.io.textures_faces_keep_no_colour", "textures (faces keep no colour)"),
    ("m3d.io.this_collada_file_has_more_than", "This Collada file has more than 2 million faces: too big to model with here."),
    ("m3d.io.this_collada_file_has_no_geometry", "This Collada file has no geometry ModelCraft can import."),
    ("m3d.io.this_collada_file_is_too_big", "This Collada file is too big to model with here (over 2 million vertices)."),
    ("m3d.io.this_drawing_has_more_than_2", "This drawing has more than 2 million line pieces: too big to bring in here."),
    ("m3d.io.this_drawing_has_more_than_200", "This drawing has more than 200,000 hatches and 3D faces: too big to bring in here."),
    (
        "m3d.io.this_drawing_reaches_more_than_10",
        "This drawing reaches more than 10,000 km from its origin: move it closer to 0,0 in the CAD program and try again.",
    ),
    ("m3d.io.this_dwg_could_not_be_read", "This DWG could not be read."),
    ("m3d.io.this_dxf_file_could_not_be", "This DXF file could not be read as text."),
    ("m3d.io.this_gltf_file_is_too_big", "This glTF file is too big to model with here (over 2 million vertices)."),
    ("m3d.io.this_is_a_binary_dxf_save", "This is a binary DXF. Save it as ASCII DXF and try again."),
    ("m3d.io.this_model_is_too_big_to", "This model is too big to export as DXF or DWG (more than 2 million objects)."),
    ("m3d.io.this_obj_file_could_not_be", "This OBJ file could not be read as text."),
    ("m3d.io.this_obj_file_has_more_than", "This OBJ file has more than 2 million vertices: too big to model with here."),
    ("m3d.io.this_obj_file_has_more_than_2", "This OBJ file has more than 2 million faces: too big to model with here."),
    ("m3d.io.transforms", "{name} transforms"),
    ("m3d.io.without_vertices", "{kind} without vertices"),
    ("m3d.xml.a_tag_without_a_name", "a tag without a name"),
    ("m3d.xml.an_unclosed_cdata_section", "an unclosed CDATA section"),
    ("m3d.xml.an_unclosed_comment", "an unclosed comment"),
    ("m3d.xml.an_unclosed_declaration", "an unclosed declaration"),
    ("m3d.xml.an_unclosed_end_tag", "an unclosed end tag"),
    ("m3d.xml.an_unclosed_processing_instruction", "an unclosed processing instruction"),
    ("m3d.xml.an_unclosed_start_tag", "an unclosed start tag"),
    ("m3d.xml.bad", "This file is not well-formed XML ({why})."),
    ("m3d.xml.more_than_one_root_element", "more than one root element"),
    ("m3d.xml.no_root_element", "no root element"),
    ("m3d.xml.this_file_is_not_text", "This file is not text."),
    ("m3d.xml.toobig", "This file has more than {n} XML elements: too big to read here."),
];

/// The string of a key in the language now (es-MX: ModelCraft's Spanish file first), else English.
pub fn lookup(key: &str) -> Option<&'static str> {
    if let Some(s) = cc_m3d::i18n::ES.text(key, None) {
        return Some(s);
    }
    EN.binary_search_by(|(k, _)| (*k).cmp(key)).ok().and_then(|i| EN.get(i)).map(|(_, v)| *v)
}

/// 't(key, fills)'.
pub fn t(key: &str, vars: &[(&str, &str)]) -> String {
    match lookup(key) {
        Some(s) => cc_m3d::i18n::fill(s, vars),
        None => cc_m3d::i18n::t(key, vars),
    }
}

/// 'new Error(t(key))'.
pub fn err(key: &str) -> cc_m3d::Error {
    cc_m3d::Error::Msg(t(key, &[]))
}
/// 'new Error(t(key, fills))'.
pub fn err_with(key: &str, vars: &[(&str, &str)]) -> cc_m3d::Error {
    cc_m3d::Error::Msg(t(key, vars))
}

#[cfg(test)]
mod tests {
    #[test]
    fn sorted() {
        for w in super::EN.windows(2) {
            assert!(w[0].0 < w[1].0, "{} before {}", w[0].0, w[1].0);
        }
        assert_eq!(super::t("m3d.io.lines", &[]), "lines");
    }
}
