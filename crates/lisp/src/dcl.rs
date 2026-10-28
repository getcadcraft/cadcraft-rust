//! Dialog files (DCL) for LISP routines (dcl.js): the parser, references and derived types expanded, and the dialog
//! functions. The window shows a tree as a dialog (the drawing side's `dialog_*`); an event runs the tile's
//! action_tile text with $key $value $reason $data bound. "accept" and "cancel" without an action close the dialog
//! with 1 and 0, as DCL's ok_cancel does.

use std::collections::HashMap;
use std::rc::Rc;

use crate::builtins::{arg, arity, intv, strv};
use crate::host::{DclVal, DlgEvent, DlgOp, HostErr, Tile};
use crate::interp::{Bf, Ctl, Lisp, await_with};
use crate::reader::read_all;
use crate::syms::S_T;
use crate::value::{LErr, R, V, from_vec, rstr};

/// The deepest tile nesting (dcl.js: references deeper than 30 are refused).
const MAX_NEST: u32 = 30;

#[derive(Clone, Debug, PartialEq)]
enum Tk {
    Str(String),
    Num(f64),
    Id(String),
    At(String),
    P(char),
}

/// What a `name : type { … }` holds before references are expanded.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Def {
    pub kind: String,
    pub name: Option<String>,
    pub attrs: Vec<(String, DclVal)>,
    pub children: Vec<Def>,
    /// a bare reference (`ok_cancel;`)
    pub reference: Option<String>,
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn tokens(text: &str) -> R<Vec<Tk>> {
    let s: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    while let Some(&c) = s.get(i) {
        if cc_cad::js::is_ws(c) {
            i = i.saturating_add(1);
            continue;
        }
        if c == '/' && s.get(i.saturating_add(1)) == Some(&'/') {
            while s.get(i).is_some_and(|&c| c != '\n') {
                i = i.saturating_add(1);
            }
            continue;
        }
        if c == '/' && s.get(i.saturating_add(1)) == Some(&'*') {
            let from = i.saturating_add(2);
            let end = (from..s.len().saturating_sub(1)).find(|&k| s.get(k) == Some(&'*') && s.get(k.saturating_add(1)) == Some(&'/'));
            i = end.map_or(s.len(), |e| e.saturating_add(2));
            continue;
        }
        if c == '"' {
            let mut v = String::new();
            i = i.saturating_add(1);
            while let Some(&d) = s.get(i) {
                if d == '"' {
                    break;
                }
                if d == '\\' && i.saturating_add(1) < s.len() {
                    let e = s.get(i.saturating_add(1)).copied().unwrap_or(' ');
                    v.push(match e {
                        'n' => '\n',
                        't' => '\t',
                        x => x,
                    });
                    i = i.saturating_add(2);
                } else {
                    v.push(d);
                    i = i.saturating_add(1);
                }
            }
            i = i.saturating_add(1);
            out.push(Tk::Str(v));
            continue;
        }
        if ":{};=,".contains(c) {
            out.push(Tk::P(c));
            i = i.saturating_add(1);
            continue;
        }
        if c == '@' {
            let mut j = i.saturating_add(1);
            while s.get(j).copied().is_some_and(is_word) {
                j = j.saturating_add(1);
            }
            out.push(Tk::At(s.get(i.saturating_add(1)..j).unwrap_or(&[]).iter().collect()));
            i = j;
            continue;
        }
        let mut j = i;
        while s.get(j).copied().is_some_and(|c| is_word(c) || matches!(c, '.' | '+' | '-')) {
            j = j.saturating_add(1);
        }
        if j == i {
            return Err(LErr::new(format!("DCL: unexpected \"{c}\"")));
        }
        let w: String = s.get(i..j).unwrap_or(&[]).iter().collect();
        out.push(if crate::builtins::plain_number(&w) && !w.contains(['e', 'E']) {
            Tk::Num(cc_cad::js::str_to_number(&w))
        } else {
            Tk::Id(w)
        });
        i = j;
    }
    Ok(out)
}

struct P {
    tk: Vec<Tk>,
    i: usize,
}

impl P {
    fn peek(&self, k: usize) -> Option<&Tk> {
        self.tk.get(self.i.saturating_add(k))
    }
    fn near(&self) -> String {
        match self.tk.get(self.i) {
            None => "the end".into(),
            Some(Tk::Str(s) | Tk::Id(s)) => s.clone(),
            Some(Tk::Num(n)) => cc_cad::js::num_to_string(*n),
            Some(Tk::At(s)) => s.clone(),
            Some(Tk::P(c)) => c.to_string(),
        }
    }
    fn take_p(&mut self, c: char) -> R<()> {
        if self.peek(0) == Some(&Tk::P(c)) {
            self.i = self.i.saturating_add(1);
            return Ok(());
        }
        Err(LErr::new(format!("DCL: expected {c} near {}", self.near())))
    }
    fn take_id(&mut self) -> R<String> {
        if let Some(Tk::Id(s)) = self.peek(0) {
            let s = s.clone();
            self.i = self.i.saturating_add(1);
            return Ok(s);
        }
        Err(LErr::new(format!("DCL: expected id near {}", self.near())))
    }
    fn value(&mut self) -> R<DclVal> {
        let Some(x) = self.peek(0).cloned() else { return Err(LErr::new("DCL: expected undefined near the end")) };
        self.i = self.i.saturating_add(1);
        match x {
            Tk::Str(s) => Ok(DclVal::Str(s)),
            Tk::Num(n) => Ok(DclVal::Num(n)),
            Tk::Id(s) => Ok(if s.eq_ignore_ascii_case("true") {
                DclVal::Bool(true)
            } else if s.eq_ignore_ascii_case("false") {
                DclVal::Bool(false)
            } else {
                DclVal::Id(s)
            }),
            _ => Err(LErr::new("DCL: bad value")),
        }
    }
    fn body(&mut self, depth: u32) -> R<Def> {
        if depth > 1000 {
            return Err(LErr::new("DCL: tiles nested too deep"));
        }
        let mut d = Def::default();
        self.take_p('{')?;
        while self.peek(0).is_some_and(|t| *t != Tk::P('}')) {
            let x = self.peek(0).cloned();
            match x {
                Some(Tk::P(';')) => self.i = self.i.saturating_add(1),
                Some(Tk::P(':')) => {
                    self.i = self.i.saturating_add(1);
                    let kind = self.take_id()?;
                    let mut kid = if self.peek(0) == Some(&Tk::P('{')) { self.body(depth.saturating_add(1))? } else { Def::default() };
                    kid.kind = kind;
                    d.children.push(kid);
                }
                Some(Tk::Id(name)) if self.peek(1) == Some(&Tk::P('=')) => {
                    self.i = self.i.saturating_add(2);
                    let v = self.value()?;
                    let k = name.to_lowercase();
                    match d.attrs.iter_mut().find(|(n, _)| *n == k) {
                        Some(slot) => slot.1 = v,
                        None => d.attrs.push((k, v)),
                    }
                    if self.peek(0) == Some(&Tk::P(';')) {
                        self.i = self.i.saturating_add(1);
                    }
                }
                Some(Tk::Id(name)) if self.peek(1) == Some(&Tk::P(':')) => {
                    self.i = self.i.saturating_add(2);
                    let kind = self.take_id()?;
                    let mut kid = if self.peek(0) == Some(&Tk::P('{')) { self.body(depth.saturating_add(1))? } else { Def::default() };
                    kid.kind = kind;
                    kid.name = Some(name);
                    d.children.push(kid);
                }
                Some(Tk::Id(name)) => {
                    self.i = self.i.saturating_add(1);
                    d.children.push(Def { reference: Some(name), ..Def::default() });
                    if self.peek(0) == Some(&Tk::P(';')) {
                        self.i = self.i.saturating_add(1);
                    }
                }
                _ => return Err(LErr::new(format!("DCL: unexpected {}", self.near()))),
            }
        }
        self.take_p('}')?;
        Ok(d)
    }
}

/// DCL text → its named definitions, in the order written (a name written twice keeps the later one).
pub fn parse_dcl(text: &str) -> R<Vec<(String, Def)>> {
    let mut p = P { tk: tokens(text)?, i: 0 };
    let mut defs: Vec<(String, Def)> = Vec::new();
    while p.i < p.tk.len() {
        match p.peek(0) {
            Some(Tk::At(_)) => {
                p.i = p.i.saturating_add(1);
                if matches!(p.peek(0), Some(Tk::Str(_))) {
                    p.i = p.i.saturating_add(1);
                }
                if p.peek(0) == Some(&Tk::P(';')) {
                    p.i = p.i.saturating_add(1);
                }
                continue;
            }
            Some(Tk::P(';')) => {
                p.i = p.i.saturating_add(1);
                continue;
            }
            _ => {}
        }
        let name = p.take_id()?;
        p.take_p(':')?;
        let kind = p.take_id()?;
        let mut b = if p.peek(0) == Some(&Tk::P('{')) { p.body(0)? } else { Def::default() };
        b.kind = kind;
        match defs.iter_mut().find(|(n, _)| *n == name) {
            Some(slot) => slot.1 = b,
            None => defs.push((name, b)),
        }
    }
    Ok(defs)
}

fn button(key: &str, label: &str, extra: &[(&str, DclVal)]) -> Tile {
    let mut attrs = vec![("key".to_string(), DclVal::Str(key.into())), ("label".to_string(), DclVal::Str(label.into()))];
    for (k, v) in extra {
        attrs.push(((*k).to_string(), v.clone()));
    }
    Tile { kind: "button".into(), attrs, children: Vec::new() }
}
fn predefined(name: &str) -> Option<Tile> {
    let t = DclVal::Bool(true);
    let row =
        |kids: Vec<Tile>| Tile { kind: "row".into(), attrs: vec![("alignment".into(), DclVal::Str("centered".into()))], children: kids };
    Some(match name {
        "ok_only" => row(vec![button("accept", "OK", &[("is_default", t.clone()), ("is_cancel", t)])]),
        "ok_cancel" => row(vec![button("accept", "OK", &[("is_default", t.clone())]), button("cancel", "Cancel", &[("is_cancel", t)])]),
        "ok_cancel_help" => row(vec![
            button("accept", "OK", &[("is_default", t.clone())]),
            button("cancel", "Cancel", &[("is_cancel", t)]),
            button("help", "Help...", &[]),
        ]),
        "ok_cancel_help_info" => row(vec![
            button("accept", "OK", &[("is_default", t.clone())]),
            button("cancel", "Cancel", &[("is_cancel", t)]),
            button("help", "Help...", &[]),
            button("info", "Info...", &[]),
        ]),
        "ok_cancel_help_errtile" => Tile {
            kind: "column".into(),
            attrs: Vec::new(),
            children: vec![
                predefined("ok_cancel_help")?,
                Tile { kind: "errtile".into(), attrs: vec![("key".into(), DclVal::Str("error".into()))], children: Vec::new() },
            ],
        },
        "ok_button" => button("accept", "OK", &[("is_default", t)]),
        "cancel_button" => button("cancel", "Cancel", &[("is_cancel", t)]),
        "help_button" => button("help", "Help...", &[]),
        "info_button" => button("info", "Info...", &[]),
        "errtile" => Tile { kind: "errtile".into(), attrs: vec![("key".into(), DclVal::Str("error".into()))], children: Vec::new() },
        "spacer" | "spacer_0" | "spacer_1" => Tile { kind: "spacer".into(), attrs: Vec::new(), children: Vec::new() },
        _ => return None,
    })
}
const BASE_TYPES: [&str; 23] = [
    "dialog",
    "row",
    "column",
    "boxed_row",
    "boxed_column",
    "radio_row",
    "radio_column",
    "boxed_radio_row",
    "boxed_radio_column",
    "button",
    "edit_box",
    "list_box",
    "popup_list",
    "toggle",
    "radio_button",
    "text",
    "text_part",
    "paragraph",
    "concatenation",
    "slider",
    "image",
    "image_button",
    "spacer",
];

fn merge_attrs(base: &[(String, DclVal)], over: &[(String, DclVal)]) -> Vec<(String, DclVal)> {
    let mut out = base.to_vec();
    for (k, v) in over {
        match out.iter_mut().find(|(n, _)| n == k) {
            Some(slot) => slot.1 = v.clone(),
            None => out.push((k.clone(), v.clone())),
        }
    }
    out
}

fn find<'a>(defs: &'a [(String, Def)], name: &str) -> Option<&'a Def> {
    defs.iter().find(|(n, _)| n == name).map(|(_, d)| d)
}

/// The tile named `name` with every reference and derived type expanded into base tiles.
pub fn resolve_tile(defs: &[(String, Def)], name: &str) -> R<Option<Tile>> {
    let Some(def) = find(defs, name) else { return Ok(None) };
    Ok(Some(expand(defs, def, 0)?))
}
fn expand(defs: &[(String, Def)], node: &Def, depth: u32) -> R<Tile> {
    if depth > MAX_NEST {
        return Err(LErr::new("DCL: tiles nested too deep"));
    }
    if let Some(r) = &node.reference {
        if let Some(d) = find(defs, r) {
            return expand(defs, d, depth.saturating_add(1));
        }
        if let Some(p) = predefined(r) {
            return Ok(p);
        }
        return Err(LErr::new(format!("DCL: unknown tile {r}")));
    }
    let mut kind = node.kind.clone();
    let mut attrs = node.attrs.clone();
    let mut pre: Vec<Tile> = Vec::new();
    let mut pre_defs: Vec<Def> = Vec::new();
    let mut guard = 0_u32;
    while !BASE_TYPES.contains(&kind.as_str()) && "errtile" != kind && guard < 30 {
        guard = guard.saturating_add(1);
        if let Some(p) = predefined(&kind) {
            kind = p.kind.clone();
            attrs = merge_attrs(&p.attrs, &attrs);
            let mut kids = p.children;
            kids.extend(pre);
            pre = kids;
            continue;
        }
        let Some(proto) = find(defs, &kind) else {
            kind = "text".into();
            break;
        };
        kind = proto.kind.clone();
        attrs = merge_attrs(&proto.attrs, &attrs);
        let mut kids = proto.children.clone();
        kids.extend(pre_defs);
        pre_defs = kids;
    }
    // children: the predefined ones (already tiles), the prototypes' and the node's own (expanded)
    let mut children = pre;
    for c in pre_defs.iter().chain(node.children.iter()) {
        children.push(expand(defs, c, depth.saturating_add(1))?);
    }
    Ok(Tile { kind, attrs, children })
}

/// One loaded dialog file.
#[derive(Clone, Debug)]
pub struct DclFile {
    pub name: String,
    pub defs: Vec<(String, Def)>,
}

/// The dialog open now (new_dialog … start_dialog).
#[derive(Clone, Debug, Default)]
pub struct DclCur {
    pub actions: HashMap<String, String>,
    pub dflt: Option<String>,
    pub data: HashMap<String, String>,
    pub status: Option<i32>,
    pub last_key: Option<String>,
}

#[derive(Clone, Debug)]
pub struct DclList {
    pub key: String,
    pub op: i32,
    pub index: i32,
    pub items: Vec<String>,
}

/// The dialogs' state of one interpreter.
#[derive(Clone, Debug)]
pub struct DclState {
    pub files: HashMap<i32, DclFile>,
    pub next: i32,
    pub cur: Option<DclCur>,
    pub list: Option<DclList>,
}

impl Default for DclState {
    fn default() -> Self {
        DclState { files: HashMap::new(), next: 1, cur: None, list: None }
    }
}

fn host_err(e: HostErr) -> LErr {
    match e {
        HostErr::Missing => LErr::new("no dialogs here"),
        HostErr::Cancel => LErr::cancel(),
        HostErr::Msg(m) => LErr::new(m),
    }
}
fn op(l: &Lisp, o: DlgOp) -> R<Option<String>> {
    let Some(h) = &l.host else { return Err(LErr::new("no dialogs here")) };
    h.dialog_op(o).map_err(host_err)
}
fn cur_ok(l: &Lisp) -> R<()> {
    if l.dcl.cur.is_none() {
        return Err(LErr::new("no dialog open"));
    }
    Ok(())
}

fn f_load_dialog(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let name = strv(l, &arg(a, 0))?;
    let f = l.find_file(&name).or_else(|| {
        let mut n2 = name.to_vec();
        n2.extend(".dcl".encode_utf16());
        l.find_file(&n2)
    });
    let Some(f) = f else { return Ok(V::Int(-1)) };
    match parse_dcl(&rstr(&f.text)) {
        Ok(defs) => {
            let id = l.dcl.next;
            l.dcl.next = l.dcl.next.saturating_add(1);
            l.dcl.files.insert(id, DclFile { name: rstr(&f.name), defs });
            Ok(V::Int(id))
        }
        Err(e) => {
            l.print(&format!("; error: {}", e.msg));
            Ok(V::Int(-1))
        }
    }
}
fn f_unload_dialog(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    let id = intv(l, &arg(a, 0))?;
    l.dcl.files.remove(&id);
    Ok(V::Nil)
}
fn f_new_dialog(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 4)?;
    let id = intv(l, &arg(a, 1))?;
    let Some(file) = l.dcl.files.get(&id).cloned() else { return Ok(V::Nil) };
    let name = rstr(&strv(l, &arg(a, 0))?);
    let tree = resolve_tile(&file.defs, &name)?;
    let Some(tree) = tree.filter(|t| t.kind == "dialog") else { return Ok(V::Nil) };
    let Some(h) = &l.host else { return Err(LErr::new("no dialogs here")) };
    h.dialog_open(&tree).map_err(host_err)?;
    let dflt = if let V::Str(s) = arg(a, 2) { Some(rstr(&s)) } else { None };
    l.dcl.cur = Some(DclCur { dflt, ..DclCur::default() });
    Ok(V::Sym(S_T))
}
fn f_action_tile(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    cur_ok(l)?;
    let k = rstr(&strv(l, &arg(a, 0))?);
    let v = rstr(&strv(l, &arg(a, 1))?);
    if let Some(c) = l.dcl.cur.as_mut() {
        c.actions.insert(k, v);
    }
    Ok(V::Sym(S_T))
}
fn f_client_data(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    cur_ok(l)?;
    let k = rstr(&strv(l, &arg(a, 0))?);
    let v = rstr(&strv(l, &arg(a, 1))?);
    if let Some(c) = l.dcl.cur.as_mut() {
        c.data.insert(k, v);
    }
    Ok(V::Nil)
}
fn f_set_tile(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    cur_ok(l)?;
    let k = rstr(&strv(l, &arg(a, 0))?);
    let v = rstr(&strv(l, &arg(a, 1))?);
    op(l, DlgOp::Set(k, v))?;
    Ok(arg(a, 1))
}
fn f_get_tile(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    cur_ok(l)?;
    let k = rstr(&strv(l, &arg(a, 0))?);
    Ok(op(l, DlgOp::Get(k))?.map_or(V::Nil, |s| V::str(&s)))
}
fn f_get_attr(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    cur_ok(l)?;
    let k = rstr(&strv(l, &arg(a, 0))?);
    let n = rstr(&strv(l, &arg(a, 1))?);
    Ok(op(l, DlgOp::Attr(k, n))?.map_or(V::Nil, |s| V::str(&s)))
}
fn f_mode_tile(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 2, 2)?;
    cur_ok(l)?;
    let k = rstr(&strv(l, &arg(a, 0))?);
    let m = intv(l, &arg(a, 1))?;
    op(l, DlgOp::Mode(k, m))?;
    Ok(V::Nil)
}
fn f_start_list(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 3)?;
    cur_ok(l)?;
    let key = rstr(&strv(l, &arg(a, 0))?);
    let o = if a.len() > 1 && !arg(a, 1).is_nil() { intv(l, &arg(a, 1))? } else { 3 };
    let index = if a.len() > 2 && !arg(a, 2).is_nil() { intv(l, &arg(a, 2))? } else { 0 };
    l.dcl.list = Some(DclList { key, op: o, index, items: Vec::new() });
    Ok(arg(a, 0))
}
fn f_add_list(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 1, 1)?;
    if l.dcl.list.is_none() {
        return Ok(V::Nil);
    }
    let s = strv(l, &arg(a, 0))?;
    if let Some(li) = l.dcl.list.as_mut() {
        if li.items.len() >= 1_000_000 {
            return Err(LErr::new("Invalid array length"));
        }
        li.items.push(rstr(&s));
    }
    Ok(V::Str(s))
}
fn f_end_list(l: &mut Lisp, _: &[V]) -> R<V> {
    let Some(li) = l.dcl.list.take() else { return Ok(V::Nil) };
    op(l, DlgOp::List { key: li.key, op: li.op, index: li.index, items: li.items })?;
    Ok(V::Nil)
}
fn f_dimx(_: &mut Lisp, _: &[V]) -> R<V> {
    Ok(V::Int(200))
}
fn f_dimy(_: &mut Lisp, _: &[V]) -> R<V> {
    Ok(V::Int(100))
}
fn f_image(_: &mut Lisp, a: &[V]) -> R<V> {
    Ok(a.last().cloned().unwrap_or(V::Nil))
}
fn f_done_dialog(l: &mut Lisp, a: &[V]) -> R<V> {
    arity(a, 0, 1)?;
    cur_ok(l)?;
    let status = if !a.is_empty() && !arg(a, 0).is_nil() {
        intv(l, &arg(a, 0))?
    } else if l.dcl.cur.as_ref().and_then(|c| c.last_key.as_deref()) == Some("cancel") {
        0
    } else {
        1
    };
    if let Some(c) = l.dcl.cur.as_mut() {
        c.status = Some(status);
    }
    op(l, DlgOp::Done(status))?;
    Ok(from_vec(vec![V::Real(0.0), V::Real(0.0)], V::Nil))
}
fn f_term_dialog(l: &mut Lisp, _: &[V]) -> R<V> {
    if l.dcl.cur.is_some() {
        op(l, DlgOp::Done(0))?;
        l.dcl.cur = None;
    }
    Ok(V::Nil)
}

fn f_start_dialog(l: &mut Lisp, a: Vec<V>) -> Ctl {
    if let Err(e) = arity(&a, 0, 0) {
        return Ctl::Throw(e);
    }
    if let Err(e) = cur_ok(l) {
        return Ctl::Throw(e);
    }
    l.finally(|l| l.dcl.cur = None);
    dialog_turn(l)
}
/// One turn of start_dialog's loop: wait for an event, run its action, go again.
fn dialog_turn(l: &mut Lisp) -> Ctl {
    let Some(h) = l.host.clone() else { return Ctl::Throw(LErr::new("no dialogs here")) };
    await_with(h.dialog_run(), |l, r| {
        let ev = match r {
            Ok(ev) => ev,
            Err(e) => return Ctl::Throw(host_err(e)),
        };
        match ev {
            DlgEvent::Done(st) => {
                let s = st.or_else(|| l.dcl.cur.as_ref().and_then(|c| c.status)).unwrap_or(0);
                Ctl::Ret(V::Int(s))
            }
            DlgEvent::Nothing => dialog_turn(l),
            DlgEvent::Event { key, value, reason, x, y } => {
                let (action, data) = match l.dcl.cur.as_mut() {
                    Some(c) => {
                        c.last_key = Some(key.clone());
                        (c.actions.get(&key).cloned().or_else(|| c.dflt.clone()), c.data.get(&key).cloned())
                    }
                    None => (None, None),
                };
                let Some(action) = action else {
                    let close = match key.as_str() {
                        "accept" => Some(1),
                        "cancel" => Some(0),
                        _ => None,
                    };
                    if let Some(st) = close {
                        if let Some(c) = l.dcl.cur.as_mut() {
                            c.status = Some(st);
                        }
                        if let Err(e) = op(l, DlgOp::Done(st)) {
                            return Ctl::Throw(e);
                        }
                    }
                    return dialog_turn(l);
                };
                let names = ["$KEY", "$VALUE", "$REASON", "$DATA", "$X", "$Y"];
                let syms: Vec<_> = names.iter().map(|n| l.fsym(n)).collect();
                let saved: Vec<_> = syms.iter().map(|s| (*s, l.raw(*s))).collect();
                let vals = [V::str(&key), V::str(&value), V::Int(reason), data.map_or(V::Nil, |d| V::str(&d)), V::Int(x), V::Int(y)];
                for (s, v) in syms.iter().zip(vals) {
                    l.set(*s, v);
                }
                let forms = match read_all(&crate::value::lstr(&action), &mut l.syms) {
                    Ok(f) => f,
                    Err(e) => {
                        for (s, v) in saved {
                            l.restore(s, v);
                        }
                        return Ctl::Throw(e);
                    }
                };
                // the next turn, after the action has run and its $-values are back
                l.frames_push_then(|l, _| dialog_turn(l));
                l.push_restore(saved);
                let body: Rc<[V]> = forms.into();
                l.seq(body)
            }
        }
    })
}

/// Install, in dcl.js's order.
pub fn install(l: &mut Lisp) {
    use Bf::{Ctl as C, Sync as S};
    for (n, f) in [
        ("LOAD_DIALOG", f_load_dialog as fn(&mut Lisp, &[V]) -> R<V>),
        ("UNLOAD_DIALOG", f_unload_dialog),
        ("NEW_DIALOG", f_new_dialog),
        ("ACTION_TILE", f_action_tile),
        ("CLIENT_DATA_TILE", f_client_data),
        ("SET_TILE", f_set_tile),
        ("GET_TILE", f_get_tile),
        ("GET_ATTR", f_get_attr),
        ("MODE_TILE", f_mode_tile),
        ("START_LIST", f_start_list),
        ("ADD_LIST", f_add_list),
        ("END_LIST", f_end_list),
        ("DIMX_TILE", f_dimx),
        ("DIMY_TILE", f_dimy),
        ("START_IMAGE", f_image),
        ("FILL_IMAGE", f_image),
        ("VECTOR_IMAGE", f_image),
        ("SLIDE_IMAGE", f_image),
        ("END_IMAGE", f_image),
        ("DONE_DIALOG", f_done_dialog),
        ("TERM_DIALOG", f_term_dialog),
    ] {
        l.def(n, S(f));
    }
    l.def("START_DIALOG", C(f_start_dialog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dialog_resolves() {
        let defs =
            parse_dcl("hello : dialog { label = \"Hi\"; : edit_box { key = \"name\"; label = \"Name\"; } ok_cancel; }").unwrap_or_default();
        let t = resolve_tile(&defs, "hello").ok().flatten();
        let t = t.unwrap_or(Tile { kind: String::new(), attrs: Vec::new(), children: Vec::new() });
        assert_eq!(t.kind, "dialog");
        assert_eq!(t.children.len(), 2);
        assert_eq!(t.children.get(1).map(|c| c.kind.as_str()), Some("row"));
    }
}
