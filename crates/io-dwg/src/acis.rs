//! The ACIS data of REGION / 3DSOLID / BODY entities: SAT text and SAB bytes as a DWG stores them (port of the read
//! helpers of lib/dwg/acis.js). Written from the ODA "Open Design Specification for .dwg files" v5.4.1, 20.4.41
//! (version 1: SAT text in blocks with the character substitution 0x9F − c; version 2: a SAB or SAT file with no
//! length, read up to its end mark) and ch. 24 (SAB streams in the data storage section). The inside of SAT/SAB is not
//! interpreted. Facts found by experiment are marked "Spec gap:".

use crate::value::V;

/// Bytes → a string of the same char codes (JS `String.fromCharCode` per byte).
pub fn latin1_string(b: &[u8]) -> String {
    b.iter().map(|&c| char::from(c)).collect()
}

/// 20.4.41 version 1: "if value is between 0x20 and 0x7E, calculate 0x9F − the value to get the real character. If
/// it's a tab, we convert to a space." Spec gap: a space is stored as 0x20 − nothing maps onto it (0x9F − 0x20 = 0x7F)
/// — AutoCAD and the ODA File Converter store a space as itself; the substitution is its own inverse on 0x21–0x7E.
pub fn sat_decrypt(b: &[u8]) -> String {
    b.iter()
        .map(|&c| {
            char::from(match c {
                0x21..=0x7E => 0x9Fu8.wrapping_sub(c),
                9 => 32,
                _ => c,
            })
        })
        .collect()
}

const MARKS: [&[u8]; 4] =
    [b"End\x0E\x02of\x0E\x04ACIS\x0D\x04data", b"End\x0E\x02of\x0E\x03ASM\x0D\x04data", b"End-of-ACIS-data", b"End-of-ASM-data"];

/// Version 2 has no length: 20.4.41 "SAB files will end with End\x0E\x02of\x0E\x04ACIS\x0D\x04data". Spec gap: ASM
/// (ACIS ≥ 21800) writes "…\x0E\x03ASM\x0D\x04data", and the ODA File Converter ends its SAB with the identifier
/// "\x0D\x10End-of-ACIS-data" (found in the converter's own R2004–R2010 files). SAT text ends with "End-of-ACIS-data"
/// / "End-of-ASM-data". The length up to and including the first end mark, or None.
pub fn find_acis_end(b: &[u8]) -> Option<usize> {
    let mut best: Option<usize> = None;
    for m in MARKS {
        if let Some(at) = b.windows(m.len()).position(|w| w == m) {
            let e = at.saturating_add(m.len());
            if best.is_none_or(|x| e < x) {
                best = Some(e);
            }
        }
    }
    best
}

/// "ACIS BinaryFile" + Int32 version, or (ASM ≥ 223) "ASM BinaryFile4" + Int64 version — spec gap, found by experiment.
pub fn sab_version(b: &[u8]) -> Option<u32> {
    let head = b.get(..15)?;
    let min = match head {
        b"ACIS BinaryFile" => 19,
        b"ASM BinaryFile4" => 23,
        _ => return None,
    };
    if b.len() < min {
        return None;
    }
    let v = b.get(15..19)?;
    Some(u32::from_le_bytes([*v.first()?, *v.get(1)?, *v.get(2)?, *v.get(3)?]))
}

pub fn is_sab(b: &[u8]) -> bool {
    matches!(b.get(..15), Some(b"ACIS BinaryFile") | Some(b"ASM BinaryFile4"))
}

/// JS `\s` on a Latin-1 string (the code points that can occur: tab … CR, space, NBSP).
fn js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n'
            | '\u{0B}'
            | '\u{0C}'
            | '\r'
            | ' '
            | '\u{A0}'
            | '\u{FEFF}'
            | '\u{1680}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202F}'
            | '\u{205F}'
            | '\u{3000}'
    ) || ('\u{2000}'..='\u{200A}').contains(&c)
}

/// The ACIS release number of SAT text: `/^\s*(\d+)/` → `+m[1]`, else undefined.
pub fn sat_version(text: &str) -> Option<f64> {
    let t = text.trim_start_matches(js_space);
    let digits: String = t.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<f64>().ok()
}

/// `{ format, version, data }` for ACIS bytes as they are stored (SAB → base64, else SAT text).
pub fn acis_value(b: &[u8]) -> crate::value::Obj {
    let mut o = crate::value::Obj::with_capacity(3);
    if is_sab(b) {
        o.set("format", "sab");
        o.set_opt("version", sab_version(b).map(V::from));
        o.set("data", crate::util::b64(b));
    } else {
        let t = latin1_string(b);
        o.set("format", "sat");
        o.set_opt("version", sat_version(&t).map(V::Num));
        o.set("data", t);
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decrypt_and_versions() {
        assert_eq!(sat_decrypt(&[0x9F - b'A', 9, b' ', 0x80]), "A  \u{80}");
        assert_eq!(sat_version("  700 0 1 0"), Some(700.0));
        assert_eq!(sat_version("x700"), None);
        let mut sab = b"ACIS BinaryFile".to_vec();
        sab.extend_from_slice(&[0x10, 0x55, 0, 0]);
        assert!(is_sab(&sab));
        assert_eq!(sab_version(&sab), Some(0x5510));
        assert_eq!(sab_version(&sab[..18]), None);
        assert_eq!(find_acis_end(b"abc End-of-ASM-data zz End-of-ACIS-data"), Some(19));
        assert_eq!(find_acis_end(b"nothing"), None);
    }
}
