//! The parity kit's `ifc-step` engine (tools/fork-parity/lib/engines.mjs): scanStep + parseParams / parseComplex of
//! every instance, as NDJSON. Line 1 = `{schema, header, count, maxId, types, errors}`; then one
//! `[id, "IFCTYPE", params]` per instance in file order (`[id, "", [{type, params}…]]` for a complex one; a line that
//! does not parse carries `{"$error": message}` as its params).

use crate::step::{Parsed, StepError, StepIndex, parse_record, scan_step};
use crate::util::{push_json_num, push_json_str};
use crate::value::push_list_json;

/// The head line (no trailing newline).
pub fn head_json(ix: &StepIndex) -> String {
    let mut o = String::new();
    o.push_str("{\"count\":");
    push_json_num(&mut o, crate::util::usize_f64(ix.count));
    o.push_str(",\"errors\":[");
    for (k, e) in ix.errors.iter().enumerate() {
        if k > 0 {
            o.push(',');
        }
        o.push_str("{\"message\":");
        push_json_str(&mut o, &e.message);
        o.push_str(",\"offset\":");
        push_json_num(&mut o, crate::util::i64_f64(e.offset));
        o.push('}');
    }
    o.push_str("],\"header\":{\"description\":");
    match &ix.header.description {
        Some(l) => push_list_json(&mut o, l),
        None => o.push_str("null"),
    }
    o.push_str(",\"name\":");
    match &ix.header.name {
        Some(l) => push_list_json(&mut o, l),
        None => o.push_str("null"),
    }
    o.push_str(",\"raw\":{");
    for (k, (name, params)) in ix.header.raw.iter().enumerate() {
        if k > 0 {
            o.push(',');
        }
        push_json_str(&mut o, name);
        o.push(':');
        push_list_json(&mut o, params);
    }
    o.push_str("},\"schema\":[");
    for (k, s) in ix.header.schema.iter().enumerate() {
        if k > 0 {
            o.push(',');
        }
        s.push_json(&mut o);
    }
    o.push_str("]},\"maxId\":");
    push_json_num(&mut o, ix.max_id);
    o.push_str(",\"schema\":");
    ix.schema.push_json(&mut o);
    o.push_str(",\"types\":{");
    // counts per type name ("(complex)" for complex instances), in first-seen order
    let mut counts: Vec<u64> = vec![0; ix.names.len()];
    for &t in &ix.types {
        if let Some(c) = counts.get_mut(usize::from(t)) {
            *c = c.saturating_add(1);
        }
    }
    let mut first = true;
    for (t, &c) in counts.iter().enumerate() {
        if c == 0 {
            continue;
        }
        if !first {
            o.push(',');
        }
        first = false;
        let name = ix.names.get(t).map_or("", |s| s);
        push_json_str(&mut o, if name.is_empty() { "(complex)" } else { name });
        o.push(':');
        push_json_num(&mut o, u64_f64(c));
    }
    o.push_str("}}");
    o
}

fn u64_f64(c: u64) -> f64 {
    crate::util::usize_f64(usize::try_from(c).unwrap_or(usize::MAX))
}

/// One instance line (with its trailing newline) into `o`.
pub fn push_line(o: &mut String, b: &[u8], ix: &StepIndex, i: usize) {
    o.push('[');
    push_json_num(o, f64::from(ix.ids.get(i).copied().unwrap_or(0)));
    o.push(',');
    push_json_str(o, ix.type_name(i));
    o.push(',');
    match parse_record(b, ix, i) {
        Parsed::Params(p) => push_list_json(o, &p),
        Parsed::Complex(parts) => {
            o.push('[');
            for (k, part) in parts.iter().enumerate() {
                if k > 0 {
                    o.push(',');
                }
                o.push_str("{\"params\":");
                push_list_json(o, &part.params);
                o.push_str(",\"type\":");
                push_json_str(o, &part.ty);
                o.push('}');
            }
            o.push(']');
        }
        Parsed::Error(msg) => {
            o.push_str("{\"$error\":");
            push_json_str(o, &msg);
            o.push('}');
        }
    }
    o.push_str("]\n");
}

/// The whole engine output written to `w` in blocks (lines are built in parallel natively, written in file order).
/// Err(StepError) when the file is refused (nothing written then).
pub fn write_ndjson(b: &[u8], w: &mut dyn std::io::Write) -> Result<std::io::Result<()>, StepError> {
    let ix = scan_step(b)?;
    Ok(write_index(b, &ix, w))
}

const BLOCK: usize = 4096;

fn write_index(b: &[u8], ix: &StepIndex, w: &mut dyn std::io::Write) -> std::io::Result<()> {
    let mut head = head_json(ix);
    head.push('\n');
    w.write_all(head.as_bytes())?;
    let blocks: Vec<usize> = (0..ix.count.div_ceil(BLOCK)).collect();
    let render = |&blk: &usize| {
        let a = blk.saturating_mul(BLOCK);
        let z = a.saturating_add(BLOCK).min(ix.count);
        let mut s = String::with_capacity(BLOCK.saturating_mul(64));
        for i in a..z {
            push_line(&mut s, b, ix, i);
        }
        s
    };
    // a wave of blocks at a time keeps memory bounded on huge files
    for wave in blocks.chunks(256) {
        #[cfg(not(target_arch = "wasm32"))]
        let parts: Vec<String> = {
            use rayon::prelude::*;
            wave.par_iter().map(render).collect()
        };
        #[cfg(target_arch = "wasm32")]
        let parts: Vec<String> = wave.iter().map(render).collect();
        for p in parts {
            w.write_all(p.as_bytes())?;
        }
    }
    Ok(())
}

/// The engine run without output (bench): scan + parse every instance; → the number of instances.
pub fn run_all(b: &[u8]) -> Result<usize, StepError> {
    let ix = scan_step(b)?;
    let n = ix.count;
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        (0..n).into_par_iter().for_each(|i| drop(parse_record(b, &ix, i)));
    }
    #[cfg(target_arch = "wasm32")]
    for i in 0..n {
        drop(parse_record(b, &ix, i));
    }
    Ok(n)
}
