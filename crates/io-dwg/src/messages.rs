//! Every sentence cc-dwg says to a person (port of lib/dwg/messages.js; English source, keys `ccdwg.*`).
//! `msg("ccdwg.read.section_failed", &[("name", ...), ("reason", ...)])`. A plural picks `one` when the fill `n` is 1.

enum Text {
    One(&'static str),
    Plural(&'static str, &'static str),
}

fn text(key: &str) -> Option<Text> {
    Some(match key {
        "ccdwg.open.failed" => Text::One("Could not open the file: {reason}"),
        "ccdwg.open.too_short" => Text::Plural("too short to be a DWG file ({n} byte)", "too short to be a DWG file ({n} bytes)"),
        "ccdwg.open.version" => Text::One("DWG version {version} is not supported (only R13 to R2018: AC1012–AC1032)"),
        "ccdwg.open.not_dwg" => Text::One("not a DWG file (it starts \"{start}\")"),
        "ccdwg.open.r13_header" => Text::One("damaged R13–R15 file header ({n} section locators)"),
        "ccdwg.open.r13_locators" => Text::One("damaged R13–R15 file: section locators point outside the file"),
        "ccdwg.open.no_id" => Text::One("damaged {version} file header (no AcFssFcAJMB id)"),
        "ccdwg.open.no_section_map" => Text::One("damaged {version} file: section map page {id} is not in the page map"),
        "ccdwg.open.past_end" => Text::One("damaged file: the {what} lies past the end of the file"),
        "ccdwg.open.page_type" => Text::One("damaged file: the {what} page has type 0x{type}"),
        "ccdwg.open.runs_past" => Text::One("damaged file: the {what} runs past the end of the file"),
        "ccdwg.open.r2007_short" => Text::One("R2007 file too short"),
        "ccdwg.open.r2007_no_section_map" => Text::One("damaged R2007 file: no section map page"),
        "ccdwg.open.r2007_past_end" => Text::One("damaged R2007 file: the {what} lies past the end"),
        "ccdwg.part.page_map" => Text::One("page map"),
        "ccdwg.part.section_map" => Text::One("section map"),
        "ccdwg.read.password" => Text::One("The drawing is password-protected (encrypted data); its sections cannot be read."),
        "ccdwg.read.codepage" => Text::One("Code page {num} ({name}) is decoded as ANSI_1252."),
        "ccdwg.read.file_header_crc" => Text::One("File header CRC does not match (file may be damaged)."),
        "ccdwg.read.file_header_crc32" => Text::One("File header CRC-32 does not match."),
        "ccdwg.read.r2007_header_bad" => Text::One("The R2007 file header has uncorrectable errors."),
        "ccdwg.read.r2007_corrected" => {
            Text::Plural("Corrected {n} damaged byte in the file header.", "Corrected {n} damaged bytes in the file header.")
        }
        "ccdwg.read.uncorrectable" => Text::One("The {what} has uncorrectable errors."),
        "ccdwg.read.section_map_damaged" => Text::One("The section map is damaged ({n} pages for one section)."),
        "ccdwg.read.section_encrypted" => Text::One("Section {name} is encrypted and was skipped."),
        "ccdwg.read.section_failed" => Text::One("Section {name} could not be read: {reason}"),
        "ccdwg.read.system_checksum" => Text::One("Checksum of the {what} does not match."),
        "ccdwg.read.page_checksum" => Text::One("Checksum of a page of {name} does not match."),
        "ccdwg.read.pages_bad" => {
            Text::Plural("{n} page of {name} could not be found or decoded.", "{n} pages of {name} could not be found or decoded.")
        }
        "ccdwg.read.pages_missing" => Text::Plural("{n} page of {name} could not be found.", "{n} pages of {name} could not be found."),
        "ccdwg.read.header_failed" => Text::One("Header variables could not be read: {reason}"),
        "ccdwg.read.header_sentinel" => Text::One("Header section sentinel is missing."),
        "ccdwg.read.header_crc" => Text::One("Header section CRC does not match."),
        "ccdwg.read.header_early" => Text::One("Header variables stop early: {reason}"),
        "ccdwg.read.classes_failed" => Text::One("Classes could not be read: {reason}"),
        "ccdwg.read.classes_sentinel" => Text::One("Classes section sentinel is missing."),
        "ccdwg.read.classes_crc" => Text::One("Classes section CRC does not match."),
        "ccdwg.read.class_number" => Text::One("Class list stops at an unexpected class number {num}."),
        "ccdwg.read.map_failed" => Text::One("Object map could not be read: {reason}"),
        "ccdwg.read.map_short" => Text::One("Object map is cut short."),
        "ccdwg.read.map_crc" => Text::Plural("{n} object map page failed the CRC check.", "{n} object map pages failed the CRC check."),
        "ccdwg.read.map_entry" => Text::One("Object map entry {handle} points at object {other}."),
        "ccdwg.read.object_failed" => Text::One("Object {handle} could not be read: {reason}"),
        "ccdwg.read.no_objects" => Text::One("The file has no objects section."),
        "ccdwg.read.more_failed" => Text::Plural("… {n} more object could not be read.", "… {n} more objects could not be read."),
        "ccdwg.read.crc_failed" => {
            Text::Plural("{n} object failed the CRC check (read anyway).", "{n} objects failed the CRC check (read anyway).")
        }
        "ccdwg.read.partial_type" => {
            Text::Plural("{n} {type} object could only be read in part.", "{n} {type} objects could only be read in part.")
        }
        "ccdwg.read.assembled_part" => Text::One("The drawing could only be assembled in part: {reason}"),
        "ccdwg.read.kept_common" => {
            Text::Plural("{n} damaged object kept with only its common data.", "{n} damaged objects kept with only their common data.")
        }
        "ccdwg.read.orphans" => Text::Plural(
            "{n} entity was not linked from its block and was added at its end.",
            "{n} entities were not linked from their block and were added at its end.",
        ),
        "ccdwg.read.acds_empty" => Text::One("The data storage section is empty."),
        "ccdwg.read.acds_no_sig" => Text::One("The data storage section has no \"jard\" signature."),
        "ccdwg.read.acds_entry" => Text::One("Data storage entry points at a \"{name}\" segment."),
        "ccdwg.read.acds_record_large" => Text::One("Data storage record of {handle} is larger than its segment."),
        "ccdwg.read.acds_partial" => Text::One("The data storage section could only be read in part: {reason}"),
        "ccdwg.read.acis_no_storage" => Text::Plural(
            "{n} ACIS entity says its data is in the data storage section, which the file does not have.",
            "{n} ACIS entities say their data is in the data storage section, which the file does not have.",
        ),
        "ccdwg.read.acis_no_record" => Text::Plural(
            "{n} ACIS entity has no record in the data storage section (kept empty).",
            "{n} ACIS entities have no record in the data storage section (kept empty).",
        ),
        "ccdwg.err.no_header" => Text::One("no AcDb:Header section"),
        "ccdwg.err.no_classes" => Text::One("no AcDb:Classes section"),
        "ccdwg.err.no_handles" => Text::One("no AcDb:Handles section"),
        "ccdwg.err.read_past_end" => Text::One("read past the end ({at} > {end} bits)"),
        "ccdwg.err.bitlong" => Text::One("bad BITLONG code 11"),
        "ccdwg.err.bitdouble" => Text::One("bad BITDOUBLE code 11"),
        "ccdwg.err.lz_before_start" => Text::One("LZ back-reference before the start of the page"),
        "ccdwg.err.lz_opcode" => Text::One("LZ opcode 0x{op} is not used"),
        "ccdwg.err.r2007_lz" => Text::One("R2007 LZ back-reference before the start"),
        "ccdwg.err.header_type" => Text::One("unknown header type {type}"),
        "ccdwg.err.offset_outside" => Text::One("object offset outside the section"),
        "ccdwg.err.object_past_end" => Text::One("object of {n} bytes runs past the section end"),
        "ccdwg.err.size_fields" => Text::One("object size fields do not agree (damaged object)"),
        "ccdwg.err.eed_code" => Text::One("EED code {code}"),
        "ccdwg.err.handle_count" => Text::One("handle count {n} is larger than the handle stream"),
        "ccdwg.err.count_large" => Text::One("count {n} is larger than the data"),
        "ccdwg.err.dictionary_size" => Text::One("dictionary of {n} entries"),
        "ccdwg.err.hatch_edge" => Text::One("hatch edge type {type}"),
        "ccdwg.err.xrecord_code" => Text::One("XRECORD group code {code}"),
        "ccdwg.err.value_type" => Text::One("value data type {type}"),
        "ccdwg.err.surface_code" => Text::One("surface field code {code}"),
        "ccdwg.err.acds_no_segment" => Text::One("no file segment at {offset}"),
        "ccdwg.err.acds_index" => Text::One("the segment index is a \"{name}\" segment"),
        "ccdwg.err.acds_segment" => Text::One("segment {n} is not in the index"),
        "ccdwg.err.acis_block" => Text::One("ACIS block of {n} bytes is larger than the data"),
        "ccdwg.err.acis_no_end" => Text::One("ACIS data (version 2) has no end mark"),
        "ccdwg.err.acis_version" => Text::One("ACIS data version {version}"),
        "ccdwg.err.wire_large" => Text::One("wire of {n} points is larger than the data"),
        "ccdwg.err.wires_large" => Text::One("{n} wires are more than the data holds"),
        "ccdwg.err.silhouettes_large" => Text::One("{n} silhouettes are more than the data holds"),
        _ => return None,
    })
}

/// The message for `key` with its `{name}` fills (unknown key → the key itself, as msg() does).
pub fn msg(key: &str, fills: &[(&str, &str)]) -> String {
    let fill = |k: &str| fills.iter().find(|(n, _)| *n == k).map(|(_, v)| *v);
    let s = match text(key) {
        None => return key.to_string(),
        Some(Text::One(s)) => s,
        Some(Text::Plural(one, other)) => {
            if fill("n").and_then(|n| n.parse::<f64>().ok()) == Some(1.0) {
                one
            } else {
                other
            }
        }
    };
    let mut out = String::with_capacity(s.len().saturating_add(16));
    let mut rest = s;
    while let Some(i) = rest.find('{') {
        let (head, tail) = rest.split_at(i);
        out.push_str(head);
        if let Some(t) = tail.strip_prefix("{{") {
            out.push('{');
            rest = t;
            continue;
        }
        match tail.find('}') {
            Some(j) => {
                let name = tail.get(1..j).unwrap_or_default();
                match fill(name) {
                    Some(v) if !name.is_empty() => out.push_str(v),
                    _ => out.push_str(tail.get(..=j).unwrap_or_default()),
                }
                rest = tail.get(j.saturating_add(1)..).unwrap_or_default(); // j < tail.len()
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fills_and_plurals() {
        assert_eq!(
            msg("ccdwg.read.section_failed", &[("name", "AcDb:Header"), ("reason", "x")]),
            "Section AcDb:Header could not be read: x"
        );
        assert_eq!(msg("ccdwg.open.too_short", &[("n", "1")]), "too short to be a DWG file (1 byte)");
        assert_eq!(msg("ccdwg.open.too_short", &[("n", "5")]), "too short to be a DWG file (5 bytes)");
        assert_eq!(msg("nope", &[]), "nope");
    }
}
