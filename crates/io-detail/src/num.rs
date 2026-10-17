//! Fast forms of `x.toFixed(f)` and `String(Number(x.toFixed(f)))` for the writers: Rust's `{:.f}` formatting is the
//! exactly rounded decimal of the double (as `toFixed` is), except on exact ties, where the JS takes the larger digit
//! and Rust the even one; ties are found exactly (an FMA residual) and handed to cc-cad's exact `to_fixed`.

pub use cc_detail::js::{fixed_num_str, to_fixed as fixed};

#[cfg(test)]
mod tests {
    use super::*;
    use cc_cad::js::{num_to_string, str_to_number, to_fixed};
    #[test]
    fn same_as_exact() {
        let mut x = 0.123_456_789_f64;
        for i in 0..200_000 {
            let v = x * f64::from(i % 1000) - 333.0;
            for f in [1, 2, 6] {
                assert_eq!(fixed(v, f), to_fixed(v, f), "{v} {f}");
                assert_eq!(fixed_num_str(v, f), num_to_string(str_to_number(&to_fixed(v, f))), "{v} {f}");
            }
            x = (x * 1.000_37 + 0.000_013).fract();
        }
        for v in [0.0078125, -0.0078125, 2.5, 0.125, -2.5, 1e20, 123_456_789_012.123_45, -1e-7, 0.000_000_5, 1e-300] {
            for f in [1, 2, 6] {
                assert_eq!(fixed(v, f), to_fixed(v, f), "{v} {f}");
                assert_eq!(fixed_num_str(v, f), num_to_string(str_to_number(&to_fixed(v, f))), "{v} {f}");
            }
        }
    }
}
