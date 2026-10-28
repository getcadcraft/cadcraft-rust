//! Command aliases from one text file (aliases.js): the familiar program-parameters format, one alias per line
//! (`L,  *LINE`; ";" starts a comment). Lines that run a program of the computer are skipped with a note. The
//! person's aliases sit on top of the built-in ones; REINIT (or Save in the editor) applies the file again.

use crate::engine::Shared;

/// One alias of a file.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub alias: String,
    pub command: String,
    pub line: usize,
}

/// What a file holds: the aliases, the lines skipped (programs), the errors (line, message).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Parsed {
    pub entries: Vec<Entry>,
    pub skipped: Vec<(usize, String)>,
    pub errors: Vec<(usize, String)>,
}

fn name_ok(a: &str) -> bool {
    let n = a.chars().count();
    (1..=64).contains(&n) && a.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || "_.-+?$#@~".contains(c))
}

/// Read an aliases file (lookup: the command table as it is, the person's old aliases taken off).
pub fn parse(sh: &Shared, text: &str) -> Parsed {
    let mut out = Parsed::default();
    let mut seen: Vec<(String, usize)> = Vec::new();
    let lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    for (k, raw) in lines.iter().enumerate() {
        let line = k.saturating_add(1);
        let no_comment = raw.split(';').next().unwrap_or("");
        let s = cc_cad::js::trim(no_comment);
        if s.is_empty() {
            continue;
        }
        // ^([^,\s]+)\s*,\s*(.*)$
        let first_end = s.find(|c: char| c == ',' || cc_cad::js::is_ws(c)).unwrap_or(s.len());
        let alias_raw = s.get(..first_end).unwrap_or("");
        let after = cc_cad::js::trim_start(s.get(first_end..).unwrap_or(""));
        if alias_raw.is_empty() || !after.starts_with(',') {
            out.errors.push((line, sh.t("aliases.err.format")));
            continue;
        }
        let rest = cc_cad::js::trim(after.get(1..).unwrap_or(""));
        let alias = alias_raw.to_uppercase();
        if !rest.starts_with('*') {
            out.skipped.push((line, cc_cad::js::trim(raw).to_string()));
            continue;
        }
        let command = cc_cad::js::trim(rest.get(1..).unwrap_or(""))
            .split(|c: char| c == ',' || cc_cad::js::is_ws(c))
            .next()
            .unwrap_or("")
            .to_uppercase();
        if !name_ok(&alias) {
            out.errors.push((line, sh.tv("aliases.err.name", &[("alias", alias.clone())])));
            continue;
        }
        if command.is_empty() {
            out.errors.push((line, sh.tv("aliases.err.nocommand", &[("alias", alias.clone())])));
            continue;
        }
        let Some(target) = sh.lookup(&command) else {
            out.errors.push((line, sh.tv("aliases.err.unknown", &[("alias", alias.clone()), ("command", command.clone())])));
            continue;
        };
        if let Some(own) = sh.lookup(&alias)
            && own.name == alias
            && own.name != target.name
        {
            out.errors.push((line, sh.tv("aliases.err.clash", &[("alias", alias.clone())])));
            continue;
        }
        if let Some((_, first)) = seen.iter().find(|(a, _)| *a == alias) {
            out.errors.push((line, sh.tv("aliases.err.twice", &[("alias", alias.clone()), ("first", first.to_string())])));
        }
        match seen.iter_mut().find(|(a, _)| *a == alias) {
            Some(slot) => slot.1 = line,
            None => seen.push((alias.clone(), line)),
        }
        out.entries.push(Entry { alias, command: target.name.to_string(), line });
    }
    out
}

/// What applying a file did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Applied {
    pub applied: usize,
    pub errors: Vec<(usize, String)>,
}

/// Apply a file: the previous file's aliases off, this one's on.
pub fn apply(sh: &Shared, text: &str) -> Applied {
    sh.st.borrow_mut().user_aliases.clear();
    let r = parse(sh, text);
    {
        let mut st = sh.st.borrow_mut();
        for e in &r.entries {
            match st.user_aliases.iter_mut().find(|(a, _)| *a == e.alias) {
                Some(slot) => slot.1 = e.command.clone(),
                None => st.user_aliases.push((e.alias.clone(), e.command.clone())),
            }
        }
    }
    let mut errors = r.errors.clone();
    for (line, _) in &r.skipped {
        errors.push((*line, sh.t("aliases.skipped.shell")));
    }
    Applied { applied: r.entries.len(), errors }
}

/// The built-in and the person's aliases as one file (the editor's Export).
pub fn export_pgp(sh: &Shared) -> String {
    let mut all: Vec<(String, String)> = Vec::new();
    for c in crate::cmd::COMMANDS {
        for a in c.aliases {
            match all.iter_mut().find(|(x, _)| x == a) {
                Some(slot) => slot.1 = c.name.to_string(),
                None => all.push(((*a).to_string(), c.name.to_string())),
            }
        }
    }
    for (a, c) in sh.st.borrow().user_aliases.iter() {
        match all.iter_mut().find(|(x, _)| x == a) {
            Some(slot) => slot.1 = c.clone(),
            None => all.push((a.clone(), c.clone())),
        }
    }
    all.sort_by_key(|(a, _)| a.to_lowercase());
    let mut lines = vec![sh.t("aliases.export.head"), String::new()];
    for (a, c) in all {
        if sh.lookup(&c).is_none() || c.starts_with('_') {
            continue;
        }
        lines.push(format!("{:<10}*{c}", format!("{a},")));
    }
    lines.join("\r\n") + "\r\n"
}

/// The editor's starting text.
pub fn template(sh: &Shared) -> String {
    [
        sh.t("aliases.file.head1"),
        sh.t("aliases.file.head2"),
        sh.t("aliases.file.head3"),
        ";".into(),
        sh.t("aliases.file.example"),
        ";   LL,     *LINE".into(),
        String::new(),
    ]
    .join("\n")
}
