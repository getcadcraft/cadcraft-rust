//! JSON in and out the way the JS does it: `JSON.stringify` text ([`stringify`], numbers as `String(x)`), markups
//! written with makeMarkup's keys, and markups read back from parsed JSON (the CLI's scripts, the parity harness, a
//! clip or a file's private data after it has been checked).

use crate::js::{str_or_empty, to_js_string, to_number, truthy};
use crate::model::{Markup, Punch, Reply, Src, Style, default_style, pts_of, rect_of};
use crate::units::Cal;
use cc_m3d::json::Out;
use serde_json::{Map, Value};

/// `JSON.stringify(v)`.
pub fn stringify(v: &Value) -> String {
    let mut o = Out::new(false);
    o.value(v);
    o.s
}

/// `JSON.stringify(v, null, 2)`.
pub fn stringify_pretty(v: &Value) -> String {
    fn go(o: &mut Out, v: &Value, depth: usize) {
        let pad = |o: &mut Out, d: usize| {
            o.ch('\n');
            for _ in 0..d {
                o.raw("  ");
            }
        };
        match v {
            Value::Array(a) if !a.is_empty() && depth < 256 => {
                o.ch('[');
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        o.ch(',');
                    }
                    pad(o, depth.saturating_add(1));
                    go(o, x, depth.saturating_add(1));
                }
                pad(o, depth);
                o.ch(']');
            }
            Value::Object(m) if !m.is_empty() && depth < 256 => {
                o.ch('{');
                for (i, (k, x)) in m.iter().enumerate() {
                    if i > 0 {
                        o.ch(',');
                    }
                    pad(o, depth.saturating_add(1));
                    o.str(k);
                    o.raw(": ");
                    go(o, x, depth.saturating_add(1));
                }
                pad(o, depth);
                o.ch('}');
            }
            x => o.value(x),
        }
    }
    let mut o = Out::new(false);
    go(&mut o, v, 0);
    o.s
}

/// A markup as stored (makeMarkup's shape); a store clones it on every edit.
pub fn clone_markup(m: &Markup) -> Markup {
    m.clone()
}

fn s_of(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        x => to_js_string(x),
    }
}

/// One key of a markup object (a patch or a whole markup read from JSON).
pub fn set_key(m: &mut Markup, k: &str, v: &Value) {
    match k {
        "id" => m.id = s_of(v),
        "sheet" => m.sheet = v.as_str().map(str::to_string),
        "type" => m.kind = s_of(v),
        "pts" => m.pts = if v.is_null() { None } else { Some(pts_of(v)) },
        "rect" => m.rect = if v.is_null() { None } else { Some(rect_of(v)) },
        "rot" => m.rot = to_number(v),
        "style" => {
            if let Some(o) = v.as_object() {
                let mut st = default_style(&m.kind);
                st.merge(o);
                m.style = st;
            }
        }
        "text" => m.text = s_of(v),
        "label" => m.label = s_of(v),
        "img" => m.img = s_of(v),
        "legend" => m.legend = v.as_object().map(|o| str_or_empty(o.get("scope"))),
        "holes" => m.holes = v.as_array().map(|h| h.iter().map(pts_of).collect()),
        "layer" => m.layer = s_of(v),
        "fields" => {
            m.fields = v.as_object().map(|o| o.iter().map(|(k, x)| (k.clone(), s_of(x))).collect()).unwrap_or_default();
        }
        "xd" => m.xd = if v.is_null() { None } else { Some(v.clone()) },
        "link" => m.link = s_of(v),
        "subject" => m.subject = s_of(v),
        "comment" => m.comment = s_of(v),
        "author" => m.author = s_of(v),
        "created" => m.created = s_of(v),
        "modified" => m.modified = s_of(v),
        "status" => m.status = s_of(v),
        "punch" => m.punch = v.as_object().map(punch_of),
        "locked" => m.locked = truthy(Some(v)),
        "src" => m.src = v.as_object().map(|o| Src { r#ref: str_or_empty(o.get("ref")), subtype: str_or_empty(o.get("subtype")) }),
        "replies" => m.replies = v.as_array().map(|a| a.iter().filter_map(reply_of).collect()).unwrap_or_default(),
        _ => {
            m.extra.insert(k.to_string(), v.clone());
        }
    }
}

fn punch_of(o: &Map<String, Value>) -> Punch {
    Punch {
        n: o.get("n").map_or(f64::NAN, to_number),
        assignee: str_or_empty(o.get("assignee")),
        due: str_or_empty(o.get("due")),
        status: str_or_empty(o.get("status")),
    }
}

/// A reply object as stored (no cleaning).
pub fn reply_of(v: &Value) -> Option<Reply> {
    let o = v.as_object()?;
    Some(Reply {
        id: str_or_empty(o.get("id")),
        author: str_or_empty(o.get("author")),
        date: str_or_empty(o.get("date")),
        text: str_or_empty(o.get("text")),
        kind: str_or_empty(o.get("kind")),
        status: str_or_empty(o.get("status")),
        photo: o.get("photo").and_then(Value::as_str).map(str::to_string),
    })
}

/// A whole markup from parsed JSON (one makeMarkup made: every key present).
pub fn markup_from_value(v: &Value) -> Markup {
    let o = v.as_object();
    let kind = o.and_then(|o| o.get("type")).map(s_of).unwrap_or_default();
    let mut m = Markup {
        id: String::new(),
        sheet: None,
        kind: kind.clone(),
        pts: None,
        rect: None,
        rot: 0.0,
        style: default_style(&kind),
        text: String::new(),
        label: String::new(),
        img: String::new(),
        legend: None,
        holes: None,
        layer: String::new(),
        fields: Default::default(),
        xd: None,
        link: String::new(),
        subject: String::new(),
        comment: String::new(),
        author: String::new(),
        created: String::new(),
        modified: String::new(),
        status: "Open".into(),
        punch: None,
        locked: false,
        src: None,
        replies: Vec::new(),
        extra: Map::new(),
    };
    if let Some(o) = o {
        for (k, x) in o {
            set_key(&mut m, k, x);
        }
    }
    m
}

/// Write a style object (makeMarkup's key order: the six defaults, then the rest).
pub fn write_style(o: &mut Out, s: &Style) {
    o.ch('{');
    o.key("color");
    o.str(&s.color);
    o.ch(',');
    o.key("width");
    if s.nulls & 1 != 0 {
        o.raw("null");
    } else {
        o.num(s.width);
    }
    o.ch(',');
    o.key("fill");
    match &s.fill {
        Some(f) => o.str(f),
        None => o.raw("null"),
    }
    o.ch(',');
    o.key("fillOpacity");
    if s.nulls & 2 != 0 {
        o.raw("null");
    } else {
        o.num(s.fill_opacity);
    }
    o.ch(',');
    o.key("opacity");
    if s.nulls & 4 != 0 {
        o.raw("null");
    } else {
        o.num(s.opacity);
    }
    o.ch(',');
    o.key("fontSize");
    if s.nulls & 8 != 0 {
        o.raw("null");
    } else {
        o.num(s.font_size);
    }
    if let Some(a) = s.arc {
        o.ch(',');
        o.key("arc");
        o.num(a);
    }
    for (k, v) in &s.extra {
        o.ch(',');
        o.key(k);
        o.value(v);
    }
    o.ch('}');
}

fn write_pts(o: &mut Out, pts: &[[f64; 2]]) {
    o.ch('[');
    for (i, p) in pts.iter().enumerate() {
        if i > 0 {
            o.ch(',');
        }
        o.nums(p);
    }
    o.ch(']');
}

/// Write a punch item.
pub fn write_punch(o: &mut Out, p: Option<&Punch>) {
    match p {
        None => o.raw("null"),
        Some(p) => {
            o.raw("{\"n\":");
            o.num(p.n);
            o.raw(",\"assignee\":");
            o.str(&p.assignee);
            o.raw(",\"due\":");
            o.str(&p.due);
            o.raw(",\"status\":");
            o.str(&p.status);
            o.ch('}');
        }
    }
}

/// Write a reply.
pub fn write_reply(o: &mut Out, r: &Reply) {
    o.raw("{\"id\":");
    o.str(&r.id);
    o.raw(",\"author\":");
    o.str(&r.author);
    o.raw(",\"date\":");
    o.str(&r.date);
    o.raw(",\"text\":");
    o.str(&r.text);
    o.raw(",\"kind\":");
    o.str(&r.kind);
    o.raw(",\"status\":");
    o.str(&r.status);
    if let Some(p) = &r.photo {
        o.raw(",\"photo\":");
        o.str(p);
    }
    o.ch('}');
}

/// Write a markup with makeMarkup's keys (absent ones — legend, holes, xd when undefined — left out).
pub fn write_markup(o: &mut Out, m: &Markup) {
    o.raw("{\"id\":");
    o.str(&m.id);
    if let Some(s) = &m.sheet {
        o.raw(",\"sheet\":");
        o.str(s);
    }
    o.raw(",\"type\":");
    o.str(&m.kind);
    o.raw(",\"pts\":");
    match &m.pts {
        Some(p) => write_pts(o, p),
        None => o.raw("null"),
    }
    o.raw(",\"rect\":");
    match &m.rect {
        Some(r) => o.nums(r),
        None => o.raw("null"),
    }
    o.raw(",\"rot\":");
    o.num(m.rot);
    o.raw(",\"style\":");
    write_style(o, &m.style);
    for (k, v) in [("text", &m.text), ("label", &m.label), ("img", &m.img)] {
        o.ch(',');
        o.key(k);
        o.str(v);
    }
    if let Some(sc) = &m.legend {
        o.raw(",\"legend\":{\"scope\":");
        o.str(sc);
        o.ch('}');
    }
    if let Some(h) = &m.holes {
        o.raw(",\"holes\":[");
        for (i, r) in h.iter().enumerate() {
            if i > 0 {
                o.ch(',');
            }
            write_pts(o, r);
        }
        o.ch(']');
    }
    o.raw(",\"layer\":");
    o.str(&m.layer);
    o.raw(",\"fields\":{");
    for (i, (k, v)) in m.fields.iter().enumerate() {
        if i > 0 {
            o.ch(',');
        }
        o.key(k);
        o.str(v);
    }
    o.ch('}');
    if let Some(x) = &m.xd {
        o.raw(",\"xd\":");
        o.value(x);
    }
    for (k, v) in [
        ("link", &m.link),
        ("subject", &m.subject),
        ("comment", &m.comment),
        ("author", &m.author),
        ("created", &m.created),
        ("modified", &m.modified),
        ("status", &m.status),
    ] {
        o.ch(',');
        o.key(k);
        o.str(v);
    }
    o.raw(",\"punch\":");
    write_punch(o, m.punch.as_ref());
    o.raw(",\"locked\":");
    o.bool(m.locked);
    o.raw(",\"src\":");
    match &m.src {
        Some(s) => {
            o.raw("{\"ref\":");
            o.str(&s.r#ref);
            o.raw(",\"subtype\":");
            o.str(&s.subtype);
            o.ch('}');
        }
        None => o.raw("null"),
    }
    o.raw(",\"replies\":[");
    for (i, r) in m.replies.iter().enumerate() {
        if i > 0 {
            o.ch(',');
        }
        write_reply(o, r);
    }
    o.ch(']');
    for (k, v) in &m.extra {
        o.ch(',');
        o.key(k);
        o.value(v);
    }
    o.ch('}');
}

/// A markup as JSON text.
pub fn markup_json(m: &Markup, canon: bool) -> String {
    let mut o = Out::new(canon);
    write_markup(&mut o, m);
    o.s
}

/// A markup as a serde value (through its JSON text).
pub fn markup_value(m: &Markup) -> Value {
    serde_json::from_str(&markup_json(m, false)).unwrap_or(Value::Null)
}

/// Write a calibration `{mmPerPt, unit, label}`.
pub fn write_cal(o: &mut Out, c: &Cal) {
    o.raw("{\"mmPerPt\":");
    o.num(c.mm_per_pt);
    o.raw(",\"unit\":");
    o.str(&c.unit);
    o.raw(",\"label\":");
    o.str(&c.label);
    o.ch('}');
}

/// A calibration from parsed JSON (the fields as they are; checks are the caller's).
pub fn cal_of(v: &Value) -> Option<Cal> {
    let o = v.as_object()?;
    Some(Cal {
        mm_per_pt: o.get("mmPerPt").map_or(f64::NAN, to_number),
        unit: str_or_empty(o.get("unit")),
        label: str_or_empty(o.get("label")),
    })
}
