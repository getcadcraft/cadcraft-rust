//! Fuzz CadCraft's LISP with no drawing (cc-lisp): any text must read and run to a value or an error within its caps
//! (steps, frames, string and array sizes) — never a panic, a stack overflow, unbounded memory or a hang. DCL text too.
#![no_main]

use libfuzzer_sys::fuzz_target;
use std::rc::Rc;

fuzz_target!(|data: &[u8]| {
    let src = cc_lisp::reader::decode_source(data);
    let _ = cc_lisp::reader::balanced(&src);
    let _ = cc_lisp::dcl::parse_dcl(&src);
    let mut l = cc_lisp::new_lisp(None, Rc::new(|_: &str| {}));
    l.limits = cc_lisp::Limits { steps: Some(200_000), frames: 100_000, string: 1 << 20, array: 1 << 16 };
    l.put_file(&cc_lisp::value::lstr("f.lsp"), &cc_lisp::value::lstr(&src));
    l.put_file(&cc_lisp::value::lstr("f.dcl"), &cc_lisp::value::lstr(&src));
    let units = cc_lisp::value::lstr(&src);
    if let Some(r) = cc_lisp::block_on(l.eval_text(&units)) {
        let _ = l.show(&r.value, true);
    }
    l.abort();
});
