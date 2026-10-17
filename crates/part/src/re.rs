//! The part file's regular expressions, matched by hand (each is deterministic: no backtracking needed).

fn digits(b: &[u8], i: usize) -> usize {
    b.iter().skip(i).take_while(|c| c.is_ascii_digit()).count()
}

/// ID = /^f\d{1,9}$/.
pub fn feature_id(s: &str) -> bool {
    let b = s.as_bytes();
    b.first() == Some(&b'f') && {
        let n = digits(b, 1);
        (1..=9).contains(&n) && n.saturating_add(1) == b.len()
    }
}

/// `/^<prefix>\d{1,9}$/` (assembly ids: p parts, i instances, j joints, v views, s steps).
pub fn prefixed_id(s: &str, prefix: char) -> bool {
    let b = s.as_bytes();
    u8::try_from(u32::from(prefix)).is_ok_and(|p| b.first() == Some(&p)) && {
        let n = digits(b, 1);
        (1..=9).contains(&n) && n.saturating_add(1) == b.len()
    }
}
/// `/^i\d{1,9}$/`.
pub fn inst_id(s: &str) -> bool {
    prefixed_id(s, 'i')
}

/// NAME_RE = /^f\d{1,9}\.[a-z]+(:[A-Za-z0-9_:-]{1,40})?([@~]f\d{1,9}(#(\d{1,4}|m))?){0,24}$/.
pub fn face_name(s: &str) -> bool {
    let b = s.as_bytes();
    if b.first() != Some(&b'f') {
        return false;
    }
    let n = digits(b, 1);
    if !(1..=9).contains(&n) {
        return false;
    }
    let mut i = n.saturating_add(1);
    if b.get(i) != Some(&b'.') {
        return false;
    }
    i = i.saturating_add(1);
    let l = b.iter().skip(i).take_while(|c| c.is_ascii_lowercase()).count();
    if l == 0 {
        return false;
    }
    i = i.saturating_add(l);
    if b.get(i) == Some(&b':') {
        let k = b.iter().skip(i.saturating_add(1)).take_while(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b':' | b'-')).count();
        if !(1..=40).contains(&k) {
            return false;
        }
        i = i.saturating_add(1).saturating_add(k);
    }
    let mut segs: u32 = 0;
    while i < b.len() {
        if !matches!(b.get(i), Some(b'@' | b'~')) || b.get(i.saturating_add(1)) != Some(&b'f') {
            return false;
        }
        let d = digits(b, i.saturating_add(2));
        if !(1..=9).contains(&d) {
            return false;
        }
        i = i.saturating_add(2).saturating_add(d);
        if b.get(i) == Some(&b'#') {
            i = i.saturating_add(1);
            if b.get(i) == Some(&b'm') {
                i = i.saturating_add(1);
            } else {
                let d = digits(b, i);
                if !(1..=4).contains(&d) {
                    return false;
                }
                i = i.saturating_add(d);
            }
        }
        segs = segs.saturating_add(1);
        if segs > 24 {
            return false;
        }
    }
    true
}

/// BODY_RE = /^f\d{1,9}(:\d{1,4})?(@f\d{1,9}#\d{1,4}){0,24}$/.
pub fn body_id(s: &str) -> bool {
    let b = s.as_bytes();
    if b.first() != Some(&b'f') {
        return false;
    }
    let n = digits(b, 1);
    if !(1..=9).contains(&n) {
        return false;
    }
    let mut i = n.saturating_add(1);
    if b.get(i) == Some(&b':') {
        let d = digits(b, i.saturating_add(1));
        if !(1..=4).contains(&d) {
            return false;
        }
        i = i.saturating_add(1).saturating_add(d);
    }
    let mut segs: u32 = 0;
    while i < b.len() {
        if b.get(i) != Some(&b'@') || b.get(i.saturating_add(1)) != Some(&b'f') {
            return false;
        }
        let d = digits(b, i.saturating_add(2));
        if !(1..=9).contains(&d) {
            return false;
        }
        i = i.saturating_add(2).saturating_add(d);
        if b.get(i) != Some(&b'#') {
            return false;
        }
        let d = digits(b, i.saturating_add(1));
        if !(1..=4).contains(&d) {
            return false;
        }
        i = i.saturating_add(1).saturating_add(d);
        segs = segs.saturating_add(1);
        if segs > 24 {
            return false;
        }
    }
    true
}

/// `nameFeatures(name)`: /^(f\d+)\./ then every /[@~](f\d+)/g, deduplicated in order.
pub fn name_features(name: &str) -> Vec<String> {
    let b = name.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut add = |s: &str| {
        if !out.iter().any(|x| x == s) {
            out.push(s.to_string());
        }
    };
    if b.first() == Some(&b'f') {
        let n = digits(b, 1);
        if n > 0 && b.get(n.saturating_add(1)) == Some(&b'.') {
            add(name.get(..n.saturating_add(1)).unwrap_or(""));
        }
    }
    let mut i = 0usize;
    while i < b.len() {
        if matches!(b.get(i), Some(b'@' | b'~')) && b.get(i.saturating_add(1)) == Some(&b'f') {
            let n = digits(b, i.saturating_add(2));
            if n > 0 {
                let start = i.saturating_add(1);
                add(name.get(start..start.saturating_add(1).saturating_add(n)).unwrap_or(""));
                i = start.saturating_add(1).saturating_add(n);
                continue;
            }
        }
        i = i.saturating_add(1);
    }
    out
}

/// `bodyFeatures(id)`: every /f\d+/g, deduplicated in order.
pub fn body_features(id: &str) -> Vec<String> {
    let b = id.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        if b.get(i) == Some(&b'f') {
            let n = digits(b, i.saturating_add(1));
            if n > 0 {
                let s = id.get(i..i.saturating_add(1).saturating_add(n)).unwrap_or("").to_string();
                if !out.contains(&s) {
                    out.push(s);
                }
                i = i.saturating_add(1).saturating_add(n);
                continue;
            }
        }
        i = i.saturating_add(1);
    }
    out
}

/// STUDY_KEY = /^[a-z][a-z0-9-]{0,30}$/.
pub fn study_key(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 31
        && b.first().is_some_and(u8::is_ascii_lowercase)
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// /^\d{4}-\d{2}-\d{2}$/.
pub fn ymd(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10 && b.iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names() {
        assert!(face_name("f1.side:l3"));
        assert!(face_name("f12.blend:2@f5#3~f7"));
        assert!(face_name("f1.end@f2#m"));
        assert!(!face_name("f1.Side"));
        assert!(!face_name("f1.side:"));
        assert!(!face_name("f1.side@f2#12345"));
        assert!(body_id("f5:2@f7#1"));
        assert!(!body_id("f5@f7"));
        assert_eq!(name_features("f3.side:l1@f5#2~f9"), vec!["f3", "f5", "f9"]);
        assert_eq!(body_features("f5:2@f7#1"), vec!["f5", "f7"]);
        assert!(feature_id("f123"));
        assert!(!feature_id("f1234567890"));
    }
}
