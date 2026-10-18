# hayro-syntax 0.8.0, patched for CadCraft

Upstream: https://crates.io/crates/hayro-syntax 0.8.0 (Apache-2.0 OR MIT, licences alongside), unchanged except:

- `src/object/array.rs`, `ArrayIter::next`: `read_with_context(..).unwrap()` returns the `Option` instead, so a damaged
  array ends early rather than panicking. Found by the `markup_pdf_read` fuzz target (2026-10-18); regression test
  `cc-io-markup` `array_that_does_not_read_back` (tests/fuzz_regress.rs), seed `fuzz/seeds/markup_pdf_read/hayro-array-unwrap`.

Drop this copy and the `[patch.crates-io]` entry when an upstream release has the fix.
