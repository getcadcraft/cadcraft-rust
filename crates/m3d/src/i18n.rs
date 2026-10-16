//! The kernel's words: `t('m3d.…', fills)` as i18n.js does it, over the English strings of ModelCraft 0.5's
//! `lang/en.json` for the keys model.js and units.js use (copied unchanged; a key not in the table comes back as the
//! key itself, as i18n.js does for a key no catalog has). None of these strings is a plural form.

const EN: &[(&str, &str)] = &[
    ("m3d.axes.bad", "The red and green points must not lie on one line through the origin."),
    ("m3d.model.a_component_cannot_be_pasted_inside", "a component cannot be pasted inside itself"),
    ("m3d.model.a_curve_needs_two_points_or", "A curve needs two points or more."),
    ("m3d.model.a_definition_appears_twice", "a definition appears twice"),
    ("m3d.model.a_definition_id_is_wrong", "a definition id is wrong"),
    ("m3d.model.a_face_is_wrong", "a face is wrong"),
    ("m3d.model.a_face_would_bend_out_of", "a face would bend out of its plane; dimension along an edge, or along red, green or blue"),
    ("m3d.model.a_face_would_shrink_to_nothing", "a face would shrink to nothing or turn inside out"),
    ("m3d.model.a_group_contains_itself", "a group contains itself"),
    ("m3d.model.a_length_must_be_above_zero", "a length must be above zero"),
    ("m3d.model.a_size_must_be_above_zero", "a size must be above zero"),
    ("m3d.model.a_vertex_is_wrong", "a vertex is wrong"),
    ("m3d.model.an_edge_is_wrong", "an edge is wrong"),
    ("m3d.model.an_instance_is_wrong", "an instance is wrong"),
    ("m3d.model.an_instance_points_at_a_missing", "an instance points at a missing definition"),
    ("m3d.model.bad_image", "That image can't be kept: only PNG, JPEG and WebP images under 18 MB."),
    ("m3d.model.bad_light", "That light can't be kept: it needs a kind and a place."),
    ("m3d.model.choose_a_path_one_or_more", "Choose a path: one or more connected edges."),
    ("m3d.model.choose_the_face_to_sweep", "Choose the face to sweep."),
    ("m3d.model.dim_angle_only", "An angle dimension measures only: rotate the edges to change the angle."),
    ("m3d.model.dim_curve_gone", "The curve of this dimension is gone, so it can no longer resize anything."),
    ("m3d.model.dim_not_round", "Only arcs, circles and polygons have a radius to dimension."),
    ("m3d.model.file_bad", "This is not a ModelCraft file we can open: {why}."),
    ("m3d.model.file_newer", "This model was saved by a newer ModelCraft (file version {v}). Update ModelCraft to open it."),
    ("m3d.model.it_has_no_model", "it has no model"),
    ("m3d.model.it_has_no_root", "it has no root"),
    ("m3d.model.it_is_empty", "it is empty"),
    ("m3d.model.it_is_flat_that_way_push", "it is flat that way; push/pull it first"),
    ("m3d.model.move_gone", "That group is gone."),
    ("m3d.model.move_into_itself", "A group cannot go inside itself, or inside a group it holds."),
    ("m3d.model.offset_works_on_one_chain_of", "Offset works on one chain of edges: these branch."),
    ("m3d.model.offset_works_on_one_connected_chain", "Offset works on one connected chain of edges."),
    ("m3d.model.only_arcs_circles_and_polygons_have", "Only arcs, circles and polygons have a radius."),
    ("m3d.model.that_is_further_than_these_edges", "That is further than these edges allow: a piece would turn inside out."),
    ("m3d.model.that_is_further_than_this_face", "That is further than this face allows: a side would turn inside out."),
    ("m3d.model.that_is_further_than_this_face_2", "That is further than this face allows: the new outline would cross itself."),
    ("m3d.model.that_is_not_a_colour", "That is not a colour."),
    ("m3d.model.the_clipboard_does_not_hold_a", "the clipboard does not hold a ModelCraft copy"),
    ("m3d.model.the_copy_is_too_big", "the copy is too big"),
    ("m3d.model.the_dimension_is_gone", "the dimension is gone"),
    ("m3d.model.the_edge_is_gone", "the edge is gone"),
    ("m3d.model.the_end_that_moves_is_not", "the end that moves is not on a corner of the model; choose which end moves in Entity info"),
    (
        "m3d.model.the_face_lies_along_the_path",
        "The face lies along the path: turn it to cross the path (Follow me sweeps it along the path, square to it).",
    ),
    ("m3d.model.the_format_mark_is_missing", "the format mark is missing"),
    ("m3d.model.the_open_group_is_gone", "the open group is gone"),
    ("m3d.model.the_path_branches_select_one_chain", "The path branches: select one chain of edges, with no forks."),
    ("m3d.model.the_path_has_an_edge_of", "The path has an edge of no length."),
    ("m3d.model.the_path_is_in_more_than", "The path is in more than one piece: select one connected chain of edges."),
    ("m3d.model.the_path_needs_at_least_one", "The path needs at least one edge."),
    ("m3d.model.the_path_turns_straight_back_on", "The path turns straight back on itself."),
    ("m3d.model.the_path_turns_too_sharply_here", "The path turns too sharply here."),
    ("m3d.model.the_place_is_wrong", "the place is wrong"),
    ("m3d.model.the_two_ends_are_at_the", "the two ends are at the same place"),
    ("m3d.model.the_version_is_missing", "the version is missing"),
    ("m3d.model.there_is_no_such_face", "There is no such face."),
    ("m3d.model.there_is_nothing_on_that_side", "there is nothing on that side to move"),
    ("m3d.model.these_edges_are_not_all_in", "These edges are not all in one plane."),
    ("m3d.model.this_curve_bounds_a_face_erase", "This curve bounds a face: erase the face first, or draw a new curve."),
    (
        "m3d.model.this_curve_bounds_a_face_that",
        "This curve bounds a face that leaves its plane (it was pushed or pulled): its radius cannot change now.",
    ),
    ("m3d.model.this_curve_can_no_longer_be", "This curve can no longer be edited (it was stretched or split); draw it again."),
    ("m3d.model.type_a_radius_above_zero", "Type a radius above zero."),
    ("m3d.section.name", "Section {n}"),
    ("m3d.tag.default", "Tag"),
    ("m3d.tag.empty", "A tag needs a name."),
    ("m3d.tag.gone", "That tag is gone."),
    ("m3d.tag.taken", "There is already a tag called {name}."),
    ("m3d.units.cuft", "{v} cu ft"),
    ("m3d.units.cuin", "{v} cu in"),
    ("m3d.units.name.cm", "Centimeters"),
    ("m3d.units.name.ft_in", "Feet and inches"),
    ("m3d.units.name.in", "Inches"),
    ("m3d.units.name.m", "Meters"),
    ("m3d.units.name.mm", "Millimeters"),
    ("m3d.units.sqft", "{v} sq ft"),
    ("m3d.units.sqin", "{v} sq in"),
];

/// The English string of a key.
pub fn lookup(key: &str) -> Option<&'static str> {
    EN.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

/// `fill(text, vars)`: `{name}` → its value (unknown fills stay as written), `{{` / `}}` → literal braces.
pub fn fill(text: &str, vars: &[(&str, &str)]) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find(['{', '}']) {
        out.push_str(rest.get(..i).unwrap_or(""));
        let tail = rest.get(i..).unwrap_or("");
        if tail.starts_with("{{") {
            out.push('{');
            rest = tail.get(2..).unwrap_or("");
            continue;
        }
        if tail.starts_with("}}") {
            out.push('}');
            rest = tail.get(2..).unwrap_or("");
            continue;
        }
        if tail.starts_with('{') {
            let name: String = tail.chars().skip(1).take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            let valid = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
            let n1 = name.len().saturating_add(1);
            let n2 = name.len().saturating_add(2);
            if valid && tail.get(n1..).is_some_and(|t| t.starts_with('}')) {
                match vars.iter().find(|(k, _)| *k == name) {
                    Some((_, v)) => out.push_str(v),
                    None => out.push_str(tail.get(..n2).unwrap_or("")),
                }
                rest = tail.get(n2..).unwrap_or("");
                continue;
            }
        }
        out.push_str(tail.get(..1).unwrap_or(""));
        rest = tail.get(1..).unwrap_or("");
    }
    out.push_str(rest);
    out
}

/// `t(key, vars)`.
pub fn t(key: &str, vars: &[(&str, &str)]) -> String {
    match lookup(key) {
        Some(s) => fill(s, vars),
        None => key.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strings() {
        assert_eq!(t("m3d.section.name", &[("n", "3")]), "Section 3");
        assert_eq!(t("m3d.nope", &[]), "m3d.nope");
        assert_eq!(fill("{a} {{x}} {b}", &[("a", "1")]), "1 {x} {b}");
    }
}
