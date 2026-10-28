//! The symbol table: names in upper case (AutoLISP's case-insensitive symbols), interned to small numbers.

use std::collections::HashMap;
use std::rc::Rc;

use crate::value::{LErr, R, Sym};

/// The most symbols one interpreter makes (a routine reading symbols in a loop cannot fill the memory).
pub const MAX_SYMS: usize = 2_000_000;

/// Symbols every interpreter knows, in this order (their numbers are fixed).
pub const FIXED: &[&str] = &["T", "QUOTE", "LAMBDA", "/", "*ERROR*", ":VLAX-TRUE", ":VLAX-FALSE", ":VLAX-NULL"];
pub const S_T: Sym = Sym(0);
pub const S_QUOTE: Sym = Sym(1);
pub const S_LAMBDA: Sym = Sym(2);
pub const S_SLASH: Sym = Sym(3);
pub const S_ERROR: Sym = Sym(4);
pub const S_VTRUE: Sym = Sym(5);
pub const S_VFALSE: Sym = Sym(6);
pub const S_VNULL: Sym = Sym(7);

#[derive(Debug)]
pub struct Syms {
    names: Vec<Rc<str>>,
    ix: HashMap<Rc<str>, u32>,
}

impl Default for Syms {
    fn default() -> Self {
        Self::new()
    }
}

impl Syms {
    pub fn new() -> Self {
        let mut s = Syms { names: Vec::new(), ix: HashMap::new() };
        for n in FIXED {
            let _ = s.intern(n);
        }
        s
    }
    /// The symbol with this name (upper-cased first).
    pub fn intern(&mut self, name: &str) -> R<Sym> {
        let has_lower = name.chars().any(|c| !c.is_ascii() || c.is_ascii_lowercase());
        let up: std::borrow::Cow<'_, str> = if has_lower { name.to_uppercase().into() } else { name.into() };
        if let Some(&i) = self.ix.get(up.as_ref()) {
            return Ok(Sym(i));
        }
        if self.names.len() >= MAX_SYMS {
            return Err(LErr::new("too many symbols"));
        }
        let rc: Rc<str> = Rc::from(up.as_ref());
        let i = u32::try_from(self.names.len()).map_err(|_| LErr::new("too many symbols"))?;
        self.names.push(Rc::clone(&rc));
        self.ix.insert(rc, i);
        Ok(Sym(i))
    }
    /// A name that is upper case already (built-in names).
    pub fn fixed(&mut self, name: &str) -> Sym {
        self.intern(name).unwrap_or(S_T)
    }
    pub fn name(&self, s: Sym) -> &str {
        self.names.get(s.0 as usize).map_or("", |n| n.as_ref())
    }
    pub fn name_rc(&self, s: Sym) -> Rc<str> {
        self.names.get(s.0 as usize).cloned().unwrap_or_else(|| Rc::from(""))
    }
    pub fn get(&self, name: &str) -> Option<Sym> {
        self.ix.get(name).map(|&i| Sym(i))
    }
    pub fn quote(&self) -> Sym {
        S_QUOTE
    }
    pub fn len(&self) -> usize {
        self.names.len()
    }
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}
