//! Content streams are read as pdf.js reads them: numbers accumulated digit by digit in a double and divided by a
//! power of ten (regression: a pdf-lib "1088.6399999999999" is 1088.64 to pdf.js, and the find-text boxes and vector
//! segments used the correctly rounded 1088.6399999999999).

use cc_io_markup::obj::{Lexer, Obj};

fn num(text: &str, pdfjs: bool) -> f64 {
    let mut lx = if pdfjs { Lexer::pdfjs(text.as_bytes()) } else { Lexer::new(text.as_bytes()) };
    match lx.object(0) {
        Some(Obj::Num(n)) => n,
        other => panic!("not a number: {other:?}"),
    }
}

#[test]
fn pdfjs_numbers() {
    assert_eq!(num("1088.6399999999999", true), 1088.64);
    assert_eq!(num("1088.6399999999999", false), 1088.6399999999999);
    assert_eq!(num("-.5", true), -0.5);
    assert_eq!(num("--3", true), -3.0);
    assert_eq!(num("12.5", true), 12.5);
    // "1.2.3" is 1.2 then .3 for pdf.js
    let mut lx = Lexer::pdfjs(b"1.2.3");
    assert!(matches!(lx.object(0), Some(Obj::Num(n)) if n == 1.2));
    assert!(matches!(lx.object(0), Some(Obj::Num(n)) if n == 0.3));
}

/// The fast `toFixed(3)` of the writer gives the same digits as the exact one.
#[test]
fn to_fixed3_matches_to_fixed() {
    let mut s = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    let mut xs: Vec<f64> =
        vec![0.0, -0.0, 0.0005, 0.0015, 1.0005, -1.0005, 2.5e-7, 1088.6399999999999, 123456.7895, 1e20, 9.999999e20, -0.0004999, 5e-324];
    for _ in 0..200_000 {
        let r = next();
        let x = match r % 4 {
            0 => f64::from_bits(r) % 1e7,
            1 => (r % 10_000_000) as f64 / 1000.0 + 0.0005,
            2 => (r as f64 / u64::MAX as f64 - 0.5) * 3000.0,
            _ => (r % 100_000) as f64 / 8.0,
        };
        if x.is_finite() {
            xs.push(x);
        }
    }
    for x in xs {
        assert_eq!(cc_io_markup::write::to_fixed3(x), cc_markup::js::to_fixed(x, 3), "{x:e}");
    }
}
