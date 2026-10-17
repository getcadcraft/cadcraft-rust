//! A minimal SQLite 3 file reader and writer — just what GeoPackages need (gpkg.rs): the schema (`sqlite_master`),
//! `PRAGMA table_info` from the CREATE TABLE text, full table scans in rowid order, and writing a fresh database of
//! rowid tables with their automatic indexes (PRIMARY KEY / UNIQUE) and `sqlite_sequence`, from the published file
//! format ("Database File Format", sqlite.org/fileformat2.html). No SQL engine: callers name tables and columns.
//!
//! Untrusted input: every page number, offset, size and varint is checked; b-tree walks keep a visited set and a depth
//! cap (a page cycle or a too-deep tree is an error, never a hang); overflow chains are bounded by the file size.

use crate::gjs::{put, u16be_at, u32be_at};
use std::collections::HashSet;

/// A stored value.
#[derive(Clone, Debug, PartialEq)]
pub enum SVal {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

/// A read or format error.
#[derive(Clone, Debug)]
pub struct SqlError(pub String);
type Res<T> = Result<T, SqlError>;
fn bad<T>(w: &str) -> Res<T> {
    Err(SqlError(w.to_string()))
}

/// A row of `sqlite_master`.
#[derive(Clone, Debug)]
pub struct SchemaRow {
    pub ty: String,
    pub name: String,
    pub tbl_name: String,
    pub rootpage: u32,
    pub sql: String,
}

/// A column from `PRAGMA table_info`.
#[derive(Clone, Debug)]
pub struct Col {
    pub name: String,
    pub ty: String,
    pub pk: u32,
}

/// An open database (the bytes are kept).
pub struct Db<'a> {
    b: &'a [u8],
    page_size: usize,
    usable: usize,
    enc: u8,
    pub schema: Vec<SchemaRow>,
}

const MAX_DEPTH: usize = 40;

/// A varint at `p` → (value, length).
fn varint(b: &[u8], p: usize) -> Option<(i64, usize)> {
    let mut v: u64 = 0;
    for i in 0..9usize {
        let c = *b.get(p.checked_add(i)?)?;
        if i == 8 {
            v = (v << 8) | u64::from(c);
            return Some((i64::from_ne_bytes(v.to_ne_bytes()), 9));
        }
        v = (v << 7) | u64::from(c & 0x7f);
        if c & 0x80 == 0 {
            return Some((i64::from_ne_bytes(v.to_ne_bytes()), i.saturating_add(1)));
        }
    }
    None
}

impl<'a> Db<'a> {
    /// Open the bytes of a database.
    pub fn open(b: &'a [u8]) -> Res<Db<'a>> {
        if b.len() < 100 || b.get(..16) != Some(b"SQLite format 3\0") {
            return bad("not sqlite");
        }
        let ps = usize::from(u16be_at(b, 16));
        let page_size = if ps == 1 { 65536 } else { ps };
        if !(512..=65536).contains(&page_size) || !page_size.is_power_of_two() {
            return bad("page size");
        }
        let reserved = usize::from(b.get(20).copied().unwrap_or(0));
        let usable = page_size.saturating_sub(reserved);
        if usable < 480 {
            return bad("usable size");
        }
        let enc = match u32be_at(b, 56) {
            0 | 1 => 1u8,
            2 => 2,
            3 => 3,
            _ => return bad("encoding"),
        };
        // SQLite refuses a file whose header (when its size field is valid) promises more pages than the file holds
        let in_header = u32be_at(b, 28);
        // the pager counts a page cut short as a page (its missing bytes read as zeros)
        let file_pages = b.len().div_ceil(page_size);
        if in_header != 0 && u32be_at(b, 24) == u32be_at(b, 92) && usize::try_from(in_header).unwrap_or(usize::MAX) > file_pages {
            return bad("database disk image is malformed");
        }
        let mut db = Db { b, page_size, usable, enc, schema: Vec::new() };
        let rows = db.scan(1, 5)?;
        for r in rows {
            let t = |i: usize| match r.get(i) {
                Some(SVal::Text(s)) => s.clone(),
                _ => String::new(),
            };
            let root = match r.get(3) {
                Some(SVal::Int(n)) => u32::try_from(*n).unwrap_or(0),
                _ => 0,
            };
            db.schema.push(SchemaRow { ty: t(0), name: t(1), tbl_name: t(2), rootpage: root, sql: t(4) });
        }
        // the schema is parsed when it loads: a table whose CREATE statement does not read is a malformed schema
        for r in &db.schema {
            if r.ty == "table" && !r.name.starts_with("sqlite_") && parse_create(&r.sql).is_err() {
                return bad("malformed database schema");
            }
        }
        Ok(db)
    }

    fn page(&self, n: u32) -> Res<std::borrow::Cow<'a, [u8]>> {
        if n == 0 {
            return bad("page 0");
        }
        let start = usize::try_from(n.saturating_sub(1)).unwrap_or(usize::MAX).saturating_mul(self.page_size);
        if let Some(p) = self.b.get(start..start.saturating_add(self.page_size)) {
            return Ok(std::borrow::Cow::Borrowed(p));
        }
        // the last page cut short: what is there, then zeros (as SQLite's pager reads it)
        match self.b.get(start..) {
            Some(rest) if !rest.is_empty() => {
                let mut v = rest.to_vec();
                v.resize(self.page_size, 0);
                Ok(std::borrow::Cow::Owned(v))
            }
            _ => bad("page out of the file"),
        }
    }

    fn text(&self, raw: &[u8]) -> String {
        match self.enc {
            2 => encoding_rs::UTF_16LE.decode_without_bom_handling(raw).0.into_owned(),
            3 => encoding_rs::UTF_16BE.decode_without_bom_handling(raw).0.into_owned(),
            _ => String::from_utf8_lossy(raw).into_owned(),
        }
    }

    /// The payload of a cell: local part and overflow chain.
    fn payload(&self, page: &[u8], p: usize, total: usize, index: bool) -> Res<Vec<u8>> {
        let u = self.usable;
        let x = if index { (u.saturating_sub(12).saturating_mul(64) / 255).saturating_sub(23) } else { u.saturating_sub(35) };
        let local = if total <= x {
            total
        } else {
            let m = (u.saturating_sub(12).saturating_mul(32) / 255).saturating_sub(23);
            let k = m.saturating_add(total.saturating_sub(m).checked_rem(u.saturating_sub(4).max(1)).unwrap_or(0));
            if k <= x { k } else { m }
        };
        let mut out = page.get(p..p.saturating_add(local)).ok_or_else(|| SqlError("cell".into()))?.to_vec();
        if local < total {
            let mut next = u32be_at(page, p.saturating_add(local));
            let mut seen = HashSet::new();
            while out.len() < total {
                if !seen.insert(next) {
                    return bad("overflow cycle");
                }
                let pg = self.page(next)?;
                next = u32be_at(&pg, 0);
                let take = (total.saturating_sub(out.len())).min(u.saturating_sub(4));
                out.extend_from_slice(pg.get(4..4usize.saturating_add(take)).ok_or_else(|| SqlError("overflow".into()))?);
            }
        }
        Ok(out)
    }

    /// A record → values.
    fn record(&self, rec: &[u8]) -> Res<Vec<SVal>> {
        if rec.is_empty() {
            return Ok(Vec::new()); // an empty payload: every column NULL
        }
        let (hsize, n) = varint(rec, 0).ok_or_else(|| SqlError("record".into()))?;
        let hsize = usize::try_from(hsize).map_err(|_| SqlError("record".into()))?;
        if hsize > rec.len() {
            return bad("record header");
        }
        let mut p = n;
        let mut data = hsize;
        let mut out = Vec::new();
        while p < hsize {
            let (t, l) = varint(rec, p).ok_or_else(|| SqlError("record".into()))?;
            p = p.saturating_add(l);
            let take = |len: usize, data: &mut usize| -> Res<&[u8]> {
                let s = rec.get(*data..data.saturating_add(len)).ok_or_else(|| SqlError("record body".into()))?;
                *data = data.saturating_add(len);
                Ok(s)
            };
            let int = |s: &[u8]| -> i64 {
                let mut a = if s.first().is_some_and(|c| c & 0x80 != 0) { [0xffu8; 8] } else { [0u8; 8] };
                let n = s.len().min(8);
                if let Some(d) = a.get_mut(8usize.saturating_sub(n)..) {
                    d.copy_from_slice(s.get(..n).unwrap_or(&[]));
                }
                i64::from_be_bytes(a)
            };
            let v = match t {
                0 => SVal::Null,
                1 => SVal::Int(int(take(1, &mut data)?)),
                2 => SVal::Int(int(take(2, &mut data)?)),
                3 => SVal::Int(int(take(3, &mut data)?)),
                4 => SVal::Int(int(take(4, &mut data)?)),
                5 => SVal::Int(int(take(6, &mut data)?)),
                6 => SVal::Int(int(take(8, &mut data)?)),
                7 => {
                    let s = take(8, &mut data)?;
                    let mut a = [0u8; 8];
                    a.copy_from_slice(s);
                    SVal::Real(f64::from_be_bytes(a))
                }
                8 => SVal::Int(0),
                9 => SVal::Int(1),
                t if t >= 12 && t % 2 == 0 => {
                    SVal::Blob(take(usize::try_from(t.saturating_sub(12) / 2).unwrap_or(usize::MAX), &mut data)?.to_vec())
                }
                t if t >= 13 => SVal::Text(self.text(take(usize::try_from(t.saturating_sub(13) / 2).unwrap_or(usize::MAX), &mut data)?)),
                _ => return bad("serial type"),
            };
            out.push(v);
        }
        Ok(out)
    }

    /// Every row of the table b-tree rooted at `root`, in rowid order; `ncols` pads short records with NULL (columns
    /// added later). The rowid comes back in front: [rowid, col0, col1, …] when `with_rowid`.
    fn scan_rows(&self, root: u32, ncols: usize, with_rowid: bool) -> Res<Vec<Vec<SVal>>> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        self.walk(root, 0, &mut seen, &mut |rowid, payload| {
            let mut r = self.record(payload)?;
            while r.len() < ncols {
                r.push(SVal::Null);
            }
            if with_rowid {
                r.insert(0, SVal::Int(rowid));
            }
            out.push(r);
            Ok(())
        })?;
        Ok(out)
    }
    fn scan(&self, root: u32, ncols: usize) -> Res<Vec<Vec<SVal>>> {
        self.scan_rows(root, ncols, false)
    }

    fn walk(&self, pg: u32, depth: usize, seen: &mut HashSet<u32>, f: &mut dyn FnMut(i64, &[u8]) -> Res<()>) -> Res<()> {
        if depth > MAX_DEPTH {
            return bad("tree too deep");
        }
        if !seen.insert(pg) {
            return bad("page cycle");
        }
        let page_cow = self.page(pg)?;
        let page: &[u8] = &page_cow;
        let h = if pg == 1 { 100 } else { 0 };
        let kind = page.get(h).copied().unwrap_or(0);
        let n = usize::from(u16be_at(page, h.saturating_add(3)));
        match kind {
            13 => {
                for i in 0..n {
                    let cp = usize::from(u16be_at(page, h.saturating_add(8).saturating_add(i.saturating_mul(2))));
                    let (len, l1) = varint(page, cp).ok_or_else(|| SqlError("cell".into()))?;
                    let (rowid, l2) = varint(page, cp.saturating_add(l1)).ok_or_else(|| SqlError("cell".into()))?;
                    let len = usize::try_from(len).map_err(|_| SqlError("cell".into()))?;
                    if len > self.b.len() {
                        return bad("payload size");
                    }
                    let pl = self.payload(page, cp.saturating_add(l1).saturating_add(l2), len, false)?;
                    f(rowid, &pl)?;
                }
                Ok(())
            }
            5 => {
                for i in 0..n {
                    let cp = usize::from(u16be_at(page, h.saturating_add(12).saturating_add(i.saturating_mul(2))));
                    let child = u32be_at(page, cp);
                    self.walk(child, depth.saturating_add(1), seen, f)?;
                }
                let right = u32be_at(page, h.saturating_add(8));
                self.walk(right, depth.saturating_add(1), seen, f)
            }
            _ => bad("not a table page"),
        }
    }

    /// `type IN ('table','view') AND name = ?`.
    pub fn has(&self, name: &str) -> bool {
        self.schema.iter().any(|r| (r.ty == "table" || r.ty == "view") && r.name == name)
    }
    fn table(&self, name: &str) -> Res<&SchemaRow> {
        self.schema.iter().find(|r| r.ty == "table" && r.name == name).ok_or_else(|| SqlError(format!("no such table: {name}")))
    }
    /// `PRAGMA table_info(name)`.
    pub fn columns(&self, name: &str) -> Res<Vec<Col>> {
        let t = self.table(name)?;
        parse_create(&t.sql).map(|(c, _)| c)
    }
    /// Every row of a table as its columns (an INTEGER PRIMARY KEY column holds the rowid), in rowid order.
    pub fn rows(&self, name: &str) -> Res<Vec<Vec<SVal>>> {
        let t = self.table(name)?;
        let (cols, without_rowid) = parse_create(&t.sql)?;
        if without_rowid {
            return bad("WITHOUT ROWID tables are not read");
        }
        let alias =
            cols.iter().position(|c| c.pk == 1 && c.ty.eq_ignore_ascii_case("INTEGER") && cols.iter().filter(|x| x.pk > 0).count() == 1);
        let mut rows = self.scan_rows(t.rootpage, cols.len(), true)?;
        for r in &mut rows {
            let rowid = r.remove(0);
            if let Some(a) = alias
                && let Some(slot) = r.get_mut(a)
                && *slot == SVal::Null
            {
                *slot = rowid;
            }
            r.truncate(cols.len());
        }
        Ok(rows)
    }
    /// `SELECT count(*) FROM name`.
    pub fn count(&self, name: &str) -> Res<usize> {
        let t = self.table(name)?;
        let mut n = 0usize;
        let mut seen = HashSet::new();
        self.walk(t.rootpage, 0, &mut seen, &mut |_, _| {
            n = n.saturating_add(1);
            Ok(())
        })?;
        Ok(n)
    }
}

// ---------------------------------------------------------------------------------------------------- CREATE TABLE

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Id(String),
    Punct(char),
    Num(String),
}

fn tokens(sql: &str) -> Res<Vec<Tok>> {
    let cs: Vec<char> = sql.chars().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    let at = |i: usize| cs.get(i).copied();
    while let Some(c) = at(i) {
        if c.is_whitespace() {
            i = i.saturating_add(1);
            continue;
        }
        if c == '-' && at(i.saturating_add(1)) == Some('-') {
            while at(i).is_some_and(|c| c != '\n') {
                i = i.saturating_add(1);
            }
            continue;
        }
        if c == '/' && at(i.saturating_add(1)) == Some('*') {
            i = i.saturating_add(2);
            while at(i).is_some() && !(at(i) == Some('*') && at(i.saturating_add(1)) == Some('/')) {
                i = i.saturating_add(1);
            }
            i = i.saturating_add(2);
            continue;
        }
        if c == '"' || c == '`' || c == '\'' || c == '[' {
            let close = if c == '[' { ']' } else { c };
            let mut s = String::new();
            i = i.saturating_add(1);
            loop {
                match at(i) {
                    None => return bad("quote"),
                    Some(x) if x == close => {
                        if close != ']' && at(i.saturating_add(1)) == Some(close) {
                            s.push(close);
                            i = i.saturating_add(2);
                            continue;
                        }
                        i = i.saturating_add(1);
                        break;
                    }
                    Some(x) => {
                        s.push(x);
                        i = i.saturating_add(1);
                    }
                }
            }
            out.push(Tok::Id(s));
            continue;
        }
        if c.is_alphanumeric() || c == '_' || c == '$' || (c as u32) > 127 {
            let mut s = String::new();
            let numeric = c.is_ascii_digit();
            while let Some(x) = at(i) {
                if x.is_alphanumeric()
                    || x == '_'
                    || x == '$'
                    || (x as u32) > 127
                    || (numeric && (x == '.' || x == '+' || x == '-') && false)
                {
                    s.push(x);
                    i = i.saturating_add(1);
                } else {
                    break;
                }
            }
            out.push(if numeric { Tok::Num(s) } else { Tok::Id(s) });
            continue;
        }
        out.push(Tok::Punct(c));
        i = i.saturating_add(1);
    }
    Ok(out)
}

const CONSTRAINT_WORDS: [&str; 11] =
    ["CONSTRAINT", "PRIMARY", "NOT", "NULL", "UNIQUE", "CHECK", "DEFAULT", "COLLATE", "REFERENCES", "GENERATED", "AS"];

/// Columns of a CREATE TABLE statement (names, declared types, primary key positions) and whether it is WITHOUT ROWID.
pub fn parse_create(sql: &str) -> Res<(Vec<Col>, bool)> {
    let t = tokens(sql)?;
    let open = t.iter().position(|x| *x == Tok::Punct('(')).ok_or_else(|| SqlError("create".into()))?;
    // the matching close
    let mut depth = 0i32;
    let mut close = None;
    for (k, x) in t.iter().enumerate().skip(open) {
        match x {
            Tok::Punct('(') => depth = depth.saturating_add(1),
            Tok::Punct(')') => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    close = Some(k);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close.ok_or_else(|| SqlError("create".into()))?;
    let without_rowid = t.get(close.saturating_add(1)..).is_some_and(|rest| {
        rest.windows(2)
            .any(|w| matches!(w, [Tok::Id(a), Tok::Id(b)] if a.eq_ignore_ascii_case("WITHOUT") && b.eq_ignore_ascii_case("ROWID")))
    });
    let body = t.get(open.saturating_add(1)..close).unwrap_or(&[]);
    // split on top-level commas
    let mut defs: Vec<&[Tok]> = Vec::new();
    let mut start = 0usize;
    let mut d = 0i32;
    for (k, x) in body.iter().enumerate() {
        match x {
            Tok::Punct('(') => d = d.saturating_add(1),
            Tok::Punct(')') => d = d.saturating_sub(1),
            Tok::Punct(',') if d == 0 => {
                defs.push(body.get(start..k).unwrap_or(&[]));
                start = k.saturating_add(1);
            }
            _ => {}
        }
    }
    defs.push(body.get(start..).unwrap_or(&[]));
    let word = |x: Option<&Tok>| match x {
        Some(Tok::Id(s)) => s.to_ascii_uppercase(),
        _ => String::new(),
    };
    let mut cols: Vec<Col> = Vec::new();
    let mut table_pk: Vec<String> = Vec::new();
    for def in defs {
        let w0 = word(def.first());
        let is_table_constraint = matches!(w0.as_str(), "CONSTRAINT" | "PRIMARY" | "UNIQUE" | "CHECK" | "FOREIGN")
            && !matches!(def.get(1), Some(Tok::Punct(_)) if false);
        // "CONSTRAINT x PRIMARY KEY (…)" / "PRIMARY KEY (…)" at table level: the next words decide
        let table_level = is_table_constraint && {
            let mut k = 0usize;
            if w0 == "CONSTRAINT" {
                k = 2;
            }
            matches!(word(def.get(k)).as_str(), "PRIMARY" | "UNIQUE" | "CHECK" | "FOREIGN")
        };
        if table_level {
            let mut k = 0usize;
            if w0 == "CONSTRAINT" {
                k = 2;
            }
            if word(def.get(k)) == "PRIMARY" {
                // PRIMARY KEY ( a, b )
                let mut inside = false;
                for x in def.iter().skip(k) {
                    match x {
                        Tok::Punct('(') => inside = true,
                        Tok::Punct(')') => break,
                        Tok::Id(s) if inside && !["ASC", "DESC", "COLLATE"].contains(&s.to_ascii_uppercase().as_str()) => {
                            table_pk.push(s.clone());
                        }
                        _ => {}
                    }
                }
            }
            continue;
        }
        let Some(Tok::Id(name)) = def.first() else { continue };
        let mut k = 1usize;
        let mut ty: Vec<String> = Vec::new();
        while let Some(x) = def.get(k) {
            match x {
                Tok::Id(s) if !CONSTRAINT_WORDS.contains(&s.to_ascii_uppercase().as_str()) => {
                    ty.push(s.clone());
                    k = k.saturating_add(1);
                }
                Tok::Punct('(') => {
                    let mut s = String::from("(");
                    k = k.saturating_add(1);
                    while let Some(y) = def.get(k) {
                        k = k.saturating_add(1);
                        match y {
                            Tok::Punct(')') => break,
                            Tok::Num(n) | Tok::Id(n) => s.push_str(n),
                            Tok::Punct(c) => s.push(*c),
                        }
                    }
                    s.push(')');
                    if let Some(last) = ty.last_mut() {
                        last.push_str(&s);
                    } else {
                        ty.push(s);
                    }
                }
                _ => break,
            }
        }
        let mut pk = 0u32;
        let rest = def.get(k..).unwrap_or(&[]);
        if rest
            .windows(2)
            .any(|w| matches!(w, [Tok::Id(a), Tok::Id(b)] if a.eq_ignore_ascii_case("PRIMARY") && b.eq_ignore_ascii_case("KEY")))
        {
            pk = 1;
        }
        cols.push(Col { name: name.clone(), ty: ty.join(" "), pk });
    }
    for (i, n) in table_pk.iter().enumerate() {
        if let Some(c) = cols.iter_mut().find(|c| c.name.eq_ignore_ascii_case(n)) {
            c.pk = u32::try_from(i.saturating_add(1)).unwrap_or(1);
        }
    }
    Ok((cols, without_rowid))
}

// ---------------------------------------------------------------------------------------------------- writing
// The writer lays out our own data (sizes bounded by the page arithmetic below), never input bytes.

#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::indexing_slicing)]
fn push_varint(out: &mut Vec<u8>, v: i64) {
    let u = v as u64;
    if u <= 0x7f {
        out.push(u as u8);
        return;
    }
    if u > 0x00ff_ffff_ffff_ffff {
        // 9 bytes: 8 × 7 bits then a full byte
        let mut b = [0u8; 9];
        b[8] = (u & 0xff) as u8;
        let mut x = u >> 8;
        for i in (0..8).rev() {
            b[i] = ((x & 0x7f) as u8) | 0x80;
            x >>= 7;
        }
        out.extend_from_slice(&b);
        return;
    }
    let mut tmp = Vec::new();
    let mut x = u;
    while x > 0 {
        tmp.push((x & 0x7f) as u8);
        x >>= 7;
    }
    let n = tmp.len();
    for (i, c) in tmp.iter().rev().enumerate() {
        out.push(if i + 1 < n { c | 0x80 } else { *c });
    }
}

/// A record of values.
pub fn record(vals: &[SVal]) -> Vec<u8> {
    let mut types: Vec<u8> = Vec::new();
    let mut body: Vec<u8> = Vec::new();
    for v in vals {
        match v {
            SVal::Null => push_varint(&mut types, 0),
            SVal::Int(0) => push_varint(&mut types, 8),
            SVal::Int(1) => push_varint(&mut types, 9),
            SVal::Int(n) => {
                let (t, len) = match *n {
                    -128..=127 => (1, 1),
                    -32768..=32767 => (2, 2),
                    -8_388_608..=8_388_607 => (3, 3),
                    -2_147_483_648..=2_147_483_647 => (4, 4),
                    -140_737_488_355_328..=140_737_488_355_327 => (5, 6),
                    _ => (6, 8),
                };
                push_varint(&mut types, t);
                let be = n.to_be_bytes();
                body.extend_from_slice(be.get(8usize.saturating_sub(len)..).unwrap_or(&[]));
            }
            SVal::Real(f) => {
                push_varint(&mut types, 7);
                body.extend_from_slice(&f.to_be_bytes());
            }
            SVal::Text(s) => {
                push_varint(&mut types, i64::try_from(s.len()).unwrap_or(0).saturating_mul(2).saturating_add(13));
                body.extend_from_slice(s.as_bytes());
            }
            SVal::Blob(b) => {
                push_varint(&mut types, i64::try_from(b.len()).unwrap_or(0).saturating_mul(2).saturating_add(12));
                body.extend_from_slice(b);
            }
        }
    }
    // the header size counts itself
    let mut hs = types.len().saturating_add(1);
    loop {
        let mut v = Vec::new();
        push_varint(&mut v, i64::try_from(hs).unwrap_or(0));
        if v.len().saturating_add(types.len()) == hs {
            break;
        }
        hs = v.len().saturating_add(types.len());
    }
    let mut out = Vec::new();
    push_varint(&mut out, i64::try_from(hs).unwrap_or(0));
    out.extend_from_slice(&types);
    out.extend_from_slice(&body);
    out
}

#[allow(clippy::cast_precision_loss)]
/// SQLite's BINARY comparison of two records' leading values (for index order).
fn cmp_vals(a: &[SVal], b: &[SVal]) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let rank = |v: &SVal| match v {
        SVal::Null => 0,
        SVal::Int(_) | SVal::Real(_) => 1,
        SVal::Text(_) => 2,
        SVal::Blob(_) => 3,
    };
    for (x, y) in a.iter().zip(b) {
        let o = match (x, y) {
            (SVal::Int(p), SVal::Int(q)) => p.cmp(q),
            (SVal::Int(p), SVal::Real(q)) => (*p as f64).partial_cmp(q).unwrap_or(Ordering::Equal),
            (SVal::Real(p), SVal::Int(q)) => p.partial_cmp(&(*q as f64)).unwrap_or(Ordering::Equal),
            (SVal::Real(p), SVal::Real(q)) => p.partial_cmp(q).unwrap_or(Ordering::Equal),
            (SVal::Text(p), SVal::Text(q)) => p.as_bytes().cmp(q.as_bytes()),
            (SVal::Blob(p), SVal::Blob(q)) => p.cmp(q),
            _ => rank(x).cmp(&rank(y)),
        };
        if o.is_ne() {
            return o;
        }
    }
    a.len().cmp(&b.len())
}

/// A database being written: page 1 is kept for the schema.
pub struct Writer {
    ps: usize,
    pages: Vec<Vec<u8>>,
    schema: Vec<(String, String, String, u32, Option<String>)>,
}

#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]
impl Writer {
    pub fn new() -> Writer {
        let ps = 4096;
        Writer { ps, pages: vec![vec![0u8; ps]], schema: Vec::new() }
    }
    fn alloc(&mut self) -> u32 {
        self.pages.push(vec![0u8; self.ps]);
        u32::try_from(self.pages.len()).unwrap_or(u32::MAX)
    }
    fn set_page(&mut self, n: u32, data: Vec<u8>) {
        if let Some(p) = self.pages.get_mut(usize::try_from(n).unwrap_or(0).saturating_sub(1)) {
            *p = data;
        }
    }
    fn max_local(&self, index: bool) -> (usize, usize) {
        let u = self.ps;
        let x = if index { (u - 12) * 64 / 255 - 23 } else { u - 35 };
        let m = (u - 12) * 32 / 255 - 23;
        (x, m)
    }
    /// The cell's local bytes (with its overflow chain written).
    fn cell_payload(&mut self, payload: &[u8], index: bool) -> Vec<u8> {
        let (x, m) = self.max_local(index);
        let total = payload.len();
        if total <= x {
            return payload.to_vec();
        }
        let u = self.ps;
        let k = m + (total - m) % (u - 4);
        let local = if k <= x { k } else { m };
        let mut out = payload.get(..local).unwrap_or(&[]).to_vec();
        let rest = payload.get(local..).unwrap_or(&[]);
        let chunks: Vec<&[u8]> = rest.chunks(u - 4).collect();
        let first = self.pages.len() as u32 + 1;
        let n = chunks.len() as u32;
        for (i, ch) in chunks.iter().enumerate() {
            let pg = self.alloc();
            let mut data = vec![0u8; u];
            let next = if (i as u32) + 1 < n { first + i as u32 + 1 } else { 0 };
            put(&mut data, 0, &next.to_be_bytes());
            put(&mut data, 4, ch);
            self.set_page(pg, data);
        }
        out.extend_from_slice(&first.to_be_bytes());
        out
    }
    /// Lay cells into one page (`kind` 13 / 10 leaf, 5 / 2 interior) at page `pg`.
    fn write_page(&mut self, pg: u32, kind: u8, cells: &[Vec<u8>], right: u32) {
        let h0 = if pg == 1 { 100 } else { 0 };
        let hl = if kind == 5 || kind == 2 { 12 } else { 8 };
        let mut data = if pg == 1 { self.pages.first().cloned().unwrap_or_default() } else { vec![0u8; self.ps] };
        data.resize(self.ps, 0);
        let mut end = self.ps;
        let mut ptrs = Vec::new();
        for c in cells {
            end -= c.len();
            put(&mut data, end, c);
            ptrs.push(end as u16);
        }
        if let Some(s) = data.get_mut(h0) {
            *s = kind;
        }
        put(&mut data, h0 + 1, &0u16.to_be_bytes());
        put(&mut data, h0 + 3, &(cells.len() as u16).to_be_bytes());
        put(&mut data, h0 + 5, &((if end == 65536 { 0 } else { end }) as u16).to_be_bytes());
        if hl == 12 {
            put(&mut data, h0 + 8, &right.to_be_bytes());
        }
        for (i, p) in ptrs.iter().enumerate() {
            put(&mut data, h0 + hl + 2 * i, &p.to_be_bytes());
        }
        self.set_page(pg, data);
    }
    fn room(&self, pg_is_1: bool, interior: bool) -> usize {
        self.ps - if pg_is_1 { 100 } else { 0 } - if interior { 12 } else { 8 }
    }

    /// A table b-tree of (rowid, record) rows in rowid order; `root` = Some(1) puts the root on page 1.
    fn table_tree(&mut self, rows: &[(i64, Vec<u8>)], root: Option<u32>) -> u32 {
        // leaves
        let mut leaves: Vec<(Vec<Vec<u8>>, i64)> = Vec::new();
        let mut cur: Vec<Vec<u8>> = Vec::new();
        let mut used = 0usize;
        let mut last = 0i64;
        let room = self.room(false, false);
        for (rowid, rec) in rows {
            let mut cell = Vec::new();
            push_varint(&mut cell, rec.len() as i64);
            push_varint(&mut cell, *rowid);
            let local = self.cell_payload(rec, false);
            cell.extend_from_slice(&local);
            if !cur.is_empty() && used + cell.len() + 2 > room {
                leaves.push((std::mem::take(&mut cur), last));
                used = 0;
            }
            used += cell.len() + 2;
            cur.push(cell);
            last = *rowid;
        }
        leaves.push((cur, last));
        // single leaf: it is the root
        if leaves.len() == 1
            && root.is_none_or(|r| {
                r == 1 && leaves.first().is_some_and(|(c, _)| c.iter().map(|x| x.len() + 2).sum::<usize>() <= self.room(true, false))
            })
        {
            let pg = root.unwrap_or_else(|| self.alloc());
            let cells = leaves.pop().map(|l| l.0).unwrap_or_default();
            self.write_page(pg, 13, &cells, 0);
            return pg;
        }
        let mut level: Vec<(u32, i64)> = Vec::new();
        for (cells, max) in leaves {
            let pg = self.alloc();
            self.write_page(pg, 13, &cells, 0);
            level.push((pg, max));
        }
        loop {
            let at_root = level.len() <= 1;
            if at_root {
                break;
            }
            // pack children into interior pages
            let room = self.room(false, true);
            let mut groups: Vec<Vec<(u32, i64)>> = Vec::new();
            let mut cur: Vec<(u32, i64)> = Vec::new();
            let mut used = 0usize;
            for ch in &level {
                let mut cell = Vec::new();
                cell.extend_from_slice(&ch.0.to_be_bytes());
                push_varint(&mut cell, ch.1);
                if cur.len() >= 2 && used + cell.len() + 2 > room {
                    groups.push(std::mem::take(&mut cur));
                    used = 0;
                }
                used += cell.len() + 2;
                cur.push(*ch);
            }
            groups.push(cur);
            let last_level = groups.len() == 1;
            let mut next = Vec::new();
            for g in groups {
                let pg = if last_level && root.is_some() { root.unwrap_or(1) } else { self.alloc() };
                let (body, right) = g.split_at(g.len().saturating_sub(1));
                let cells: Vec<Vec<u8>> = body
                    .iter()
                    .map(|(p, k)| {
                        let mut c = p.to_be_bytes().to_vec();
                        push_varint(&mut c, *k);
                        c
                    })
                    .collect();
                let (rp, rmax) = right.first().copied().unwrap_or((0, 0));
                self.write_page(pg, 5, &cells, rp);
                next.push((pg, rmax));
            }
            level = next;
        }
        level.first().map_or(0, |l| l.0)
    }

    /// An index b-tree of sorted records.
    fn index_tree(&mut self, entries: &[Vec<u8>]) -> u32 {
        // build bottom-up: (pages of the level, separators between them)
        let room_leaf = self.room(false, false);
        let mut leaves: Vec<Vec<Vec<u8>>> = Vec::new();
        let mut seps: Vec<Vec<u8>> = Vec::new();
        let mut cur: Vec<Vec<u8>> = Vec::new();
        let mut used = 0usize;
        let cells: Vec<Vec<u8>> = entries
            .iter()
            .map(|rec| {
                let mut c = Vec::new();
                push_varint(&mut c, rec.len() as i64);
                let local = self.cell_payload(rec, true);
                c.extend_from_slice(&local);
                c
            })
            .collect();
        let mut i = 0usize;
        while i < cells.len() {
            let c = cells.get(i).cloned().unwrap_or_default();
            if !cur.is_empty() && used + c.len() + 2 > room_leaf && i + 1 < cells.len() {
                // this entry goes up as the separator; a new leaf starts after it
                leaves.push(std::mem::take(&mut cur));
                seps.push(c);
                used = 0;
                i += 1;
                continue;
            }
            used += c.len() + 2;
            cur.push(c);
            i += 1;
        }
        leaves.push(cur);
        let mut pages: Vec<u32> = Vec::new();
        for l in &leaves {
            let pg = self.alloc();
            self.write_page(pg, 10, l, 0);
            pages.push(pg);
        }
        while pages.len() > 1 {
            let room = self.room(false, true);
            let mut np: Vec<u32> = Vec::new();
            let mut nseps: Vec<Vec<u8>> = Vec::new();
            let mut gp: Vec<u32> = vec![pages.first().copied().unwrap_or(0)];
            let mut gs: Vec<Vec<u8>> = Vec::new();
            let mut used = 0usize;
            for k in 0..seps.len() {
                let s = seps.get(k).cloned().unwrap_or_default();
                let child = pages.get(k + 1).copied().unwrap_or(0);
                if !gs.is_empty() && used + s.len() + 6 > room && k + 1 < seps.len() {
                    // close this interior page: s goes up
                    let pg = self.alloc();
                    let cells: Vec<Vec<u8>> = gs.iter().zip(&gp).map(|(s, p)| [p.to_be_bytes().to_vec(), s.clone()].concat()).collect();
                    let right = gp.last().copied().unwrap_or(0);
                    self.write_page(pg, 2, &cells, right);
                    np.push(pg);
                    nseps.push(s);
                    gp = vec![child];
                    gs = Vec::new();
                    used = 0;
                    continue;
                }
                used += s.len() + 6;
                gs.push(s);
                gp.push(child);
            }
            let pg = self.alloc();
            let cells: Vec<Vec<u8>> = gs.iter().zip(&gp).map(|(s, p)| [p.to_be_bytes().to_vec(), s.clone()].concat()).collect();
            let right = gp.last().copied().unwrap_or(0);
            self.write_page(pg, 2, &cells, right);
            np.push(pg);
            pages = np;
            seps = nseps;
        }
        pages.first().copied().unwrap_or(0)
    }

    /// Add a rowid table: `rows` in rowid order (rowid, values).
    pub fn table(&mut self, name: &str, sql: &str, rows: &[(i64, Vec<SVal>)]) {
        let recs: Vec<(i64, Vec<u8>)> = rows.iter().map(|(r, v)| (*r, record(v))).collect();
        let root = self.table_tree(&recs, None);
        self.schema.push(("table".into(), name.into(), name.into(), root, Some(sql.into())));
    }
    /// Add an automatic index over `cols` (positions in the table's rows) of a table added before.
    pub fn auto_index(&mut self, table: &str, n: usize, rows: &[(i64, Vec<SVal>)], cols: &[usize]) {
        let mut keys: Vec<Vec<SVal>> = rows
            .iter()
            .map(|(rowid, v)| {
                let mut k: Vec<SVal> = cols.iter().map(|&i| v.get(i).cloned().unwrap_or(SVal::Null)).collect();
                k.push(SVal::Int(*rowid));
                k
            })
            .collect();
        keys.sort_by(|a, b| cmp_vals(a, b));
        let recs: Vec<Vec<u8>> = keys.iter().map(|k| record(k)).collect();
        let root = self.index_tree(&recs);
        self.schema.push(("index".into(), format!("sqlite_autoindex_{table}_{n}"), table.into(), root, None));
    }

    /// The file: the schema on page 1, the header, `user_version` and `application_id` set.
    pub fn finish(mut self, user_version: u32, application_id: u32) -> Vec<u8> {
        let rows: Vec<(i64, Vec<u8>)> = self
            .schema
            .iter()
            .enumerate()
            .map(|(i, (ty, name, tbl, root, sql))| {
                let vals = vec![
                    SVal::Text(ty.clone()),
                    SVal::Text(name.clone()),
                    SVal::Text(tbl.clone()),
                    SVal::Int(i64::from(*root)),
                    sql.clone().map_or(SVal::Null, SVal::Text),
                ];
                (i as i64 + 1, record(&vals))
            })
            .collect();
        self.table_tree(&rows, Some(1));
        let n = self.pages.len() as u32;
        let mut out: Vec<u8> = self.pages.concat();
        put(&mut out, 0, b"SQLite format 3\0");
        put(&mut out, 16, &(self.ps as u16).to_be_bytes());
        put(&mut out, 18, &[1, 1, 0, 64, 32, 32]);
        put(&mut out, 24, &1u32.to_be_bytes());
        put(&mut out, 28, &n.to_be_bytes());
        put(&mut out, 40, &1u32.to_be_bytes());
        put(&mut out, 44, &4u32.to_be_bytes());
        put(&mut out, 56, &1u32.to_be_bytes());
        put(&mut out, 60, &user_version.to_be_bytes());
        put(&mut out, 68, &application_id.to_be_bytes());
        put(&mut out, 92, &1u32.to_be_bytes());
        put(&mut out, 96, &3_046_001u32.to_be_bytes());
        out
    }
}
impl Default for Writer {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
/// NUMERIC affinity for a text value (what SQLite stores when text goes into a NUMERIC / DATE / BOOLEAN column).
pub fn numeric_affinity(s: &str) -> SVal {
    let t = s.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\u{b}' | '\u{c}' | '\r'));
    let b = t.as_bytes();
    let mut i = 0usize;
    if matches!(b.first(), Some(b'+' | b'-')) {
        i = 1;
    }
    let ds = i;
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
    }
    let int_digits = i - ds;
    let mut frac = 0usize;
    let mut is_real = false;
    if b.get(i) == Some(&b'.') {
        is_real = true;
        i += 1;
        let fs = i;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        frac = i - fs;
    }
    if int_digits == 0 && frac == 0 {
        return SVal::Text(s.to_string());
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        let mut k = i + 1;
        if matches!(b.get(k), Some(b'+' | b'-')) {
            k += 1;
        }
        let es = k;
        while b.get(k).is_some_and(u8::is_ascii_digit) {
            k += 1;
        }
        if k == es {
            return SVal::Text(s.to_string());
        }
        is_real = true;
        i = k;
    }
    if i != b.len() {
        return SVal::Text(s.to_string());
    }
    if !is_real && let Ok(n) = t.parse::<i64>() {
        return SVal::Int(n);
    }
    let f: f64 = t.parse().unwrap_or(f64::NAN);
    if f.is_finite() && f.fract() == 0.0 && f.abs() < 9.2e18 {
        return SVal::Int(f as i64);
    }
    SVal::Real(f)
}
