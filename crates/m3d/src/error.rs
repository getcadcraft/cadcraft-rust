//! The kernel's error: what the JS throws. `Msg` is an `Error` with the kernel's own message (`t('m3d.…')`);
//! `Type` stands for a V8 TypeError the JS would raise on the same input (reading a property of `undefined`), which
//! the parity harness compares by kind only.

use crate::i18n;

#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    Msg(String),
    Type,
}

pub type Res<T> = Result<T, Error>;

/// `new Error(t(key))`.
pub fn msg(key: &str) -> Error {
    Error::Msg(i18n::t(key, &[]))
}
/// `new Error(t(key, fills))`.
pub fn msg_with(key: &str, fills: &[(&str, &str)]) -> Error {
    Error::Msg(i18n::t(key, fills))
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Msg(m) => f.write_str(m),
            Error::Type => f.write_str("$TypeError"),
        }
    }
}
impl std::error::Error for Error {}
