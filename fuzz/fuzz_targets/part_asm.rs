//! Fuzz PartCraft's assembly files and clipboard clips (assembly.js readAssembly, clipboard.js readClip): any text must
//! read back as a clean assembly / clip or be refused — never a panic, unbounded work or a hang.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let src = String::from_utf8_lossy(data).into_owned();
    let v = serde_json::Value::String(src);
    if let Ok(asm) = cc_part::asm::read_assembly(&v) {
        let text = cc_part::asm::to_json(&asm);
        let _ = cc_part::asm::read_assembly(&serde_json::Value::String(text));
        let _ = cc_part::asm::place_all(&asm, &cc_part::asm::Builds::new());
        let _ = cc_part::asm::free_ids(&asm);
    }
    if let Ok(c) = cc_part::clipboard::read_clip(&v) {
        let _ = cc_part::clipboard::describe_clip(&c);
    }
});
