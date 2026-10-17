//! csv.js — CSV text for the bill of materials and the bending schedule: RFC 4180 quoting, CRLF line ends; a cell
//! that starts with = + - @ (and is not a plain number) gets a leading ' so a spreadsheet never runs it.

/// `cell(v)`.
pub fn cell(v: &str) -> String {
    let mut s = v.to_string();
    let starts = s.starts_with(['=', '+', '-', '@', '\t', '\r']);
    if starts && !is_plain_number(&s) {
        s.insert(0, '\'');
    }
    if s.contains(['"', ',', '\r', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s }
}
/// /^-?\d+(\.\d+)?$/
fn is_plain_number(s: &str) -> bool {
    let b = s.strip_prefix('-').unwrap_or(s);
    let (int, frac) = match b.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (b, None),
    };
    !int.is_empty()
        && int.bytes().all(|c| c.is_ascii_digit())
        && frac.is_none_or(|f| !f.is_empty() && f.bytes().all(|c| c.is_ascii_digit()))
}
/// `toCsv(rows)`.
pub fn to_csv(rows: &[Vec<String>]) -> String {
    let mut out = String::new();
    for (i, r) in rows.iter().enumerate() {
        if i > 0 {
            out.push_str("\r\n");
        }
        out.push_str(&r.iter().map(|c| cell(c)).collect::<Vec<_>>().join(","));
    }
    out.push_str("\r\n");
    out
}
/// `sectionsCsv(sections)`.
pub fn sections_csv(sections: &[(String, Vec<Vec<String>>)]) -> String {
    sections
        .iter()
        .map(|(title, rows)| {
            let mut all = vec![vec![title.clone()]];
            all.extend(rows.iter().cloned());
            to_csv(&all)
        })
        .collect::<Vec<_>>()
        .join("\r\n")
}
/// `parseCsv(text)` (tests): quotes, doubled quotes, CRLF.
pub fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let cs: Vec<char> = text.chars().collect();
    let (mut rows, mut row, mut cur, mut q) = (Vec::new(), Vec::new(), String::new(), false);
    let mut i = 0usize;
    while i < cs.len() {
        let Some(&ch) = cs.get(i) else { break };
        if q {
            if ch == '"' {
                if cs.get(i.saturating_add(1)) == Some(&'"') {
                    cur.push('"');
                    i = i.saturating_add(1);
                } else {
                    q = false;
                }
            } else {
                cur.push(ch);
            }
        } else if ch == '"' {
            q = true;
        } else if ch == ',' {
            row.push(std::mem::take(&mut cur));
        } else if ch == '\n' || ch == '\r' {
            if ch == '\r' && cs.get(i.saturating_add(1)) == Some(&'\n') {
                i = i.saturating_add(1);
            }
            row.push(std::mem::take(&mut cur));
            rows.push(std::mem::take(&mut row));
        } else {
            cur.push(ch);
        }
        i = i.saturating_add(1);
    }
    if !cur.is_empty() || !row.is_empty() {
        row.push(cur);
        rows.push(row);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cells() {
        assert_eq!(cell("=1+1"), "'=1+1");
        assert_eq!(cell("-12.5"), "-12.5");
        assert_eq!(cell("a,b"), "\"a,b\"");
        assert_eq!(parse_csv(&to_csv(&[vec!["a\"b".into(), "c".into()]])), vec![vec!["a\"b".to_string(), "c".to_string()]]);
    }
}
