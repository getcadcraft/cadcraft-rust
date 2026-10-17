//! The words V8's `JSON.parse` uses when it refuses a text (Node 22+: "Expected property name or '}' in JSON at
//! position 1 (line 1 column 2)", "Unexpected token 'x', \"…\" is not valid JSON", "Unexpected end of JSON input"), so
//! a script's broken `json …` value is refused with the same reason the 0.5 app gives. Positions and context are in
//! UTF-16 units. Checked against node on random broken JSON (pcdiff `json` generator).

const CTX: usize = 10;

struct P<'a> {
    s: &'a [u16],
    i: usize,
    src: &'a str,
}

type R = Result<(), String>;

enum Tok {
    Str,
    Num,
    Eos,
    Other,
}

impl P<'_> {
    fn at(&self, i: usize) -> Option<u16> {
        self.s.get(i).copied()
    }
    fn cur(&self) -> Option<u16> {
        self.at(self.i)
    }
    fn skip_ws(&mut self) {
        while matches!(self.cur(), Some(0x20 | 0x09 | 0x0a | 0x0d)) {
            self.i = self.i.saturating_add(1);
        }
    }
    fn loc(&self, pos: usize) -> (usize, usize) {
        let mut line = 1usize;
        let mut last = 0usize;
        let mut k = 0usize;
        while k < pos {
            let c = self.at(k);
            if c == Some(0x0d) && k.saturating_add(1) < pos && self.at(k.saturating_add(1)) == Some(0x0a) {
                k = k.saturating_add(1);
            }
            if matches!(self.at(k), Some(0x0d | 0x0a)) {
                line = line.saturating_add(1);
                last = k.saturating_add(1);
            }
            k = k.saturating_add(1);
        }
        (line, k.saturating_sub(last).saturating_add(1))
    }
    fn at_pos(&self, what: &str) -> String {
        let (l, c) = self.loc(self.i);
        format!("{what} in JSON at position {} (line {l} column {c})", self.i)
    }
    fn tok_of(c: Option<u16>) -> Tok {
        match c {
            None => Tok::Eos,
            Some(0x22) => Tok::Str,
            Some(0x2d | 0x30..=0x39) => Tok::Num,
            _ => Tok::Other,
        }
    }
    /// `ReportUnexpectedToken(token)` without a message.
    fn unexpected(&self) -> String {
        match Self::tok_of(self.cur()) {
            Tok::Eos => "Unexpected end of JSON input".into(),
            Tok::Num => self.at_pos("Unexpected number"),
            Tok::Str => self.at_pos("Unexpected string"),
            Tok::Other => {
                if matches!(self.src, "undefined" | "NaN" | "Infinity" | "[object Object]") {
                    return format!("\"{}\" is not valid JSON", self.src);
                }
                let ch = String::from_utf16_lossy(self.s.get(self.i..self.i.saturating_add(1)).unwrap_or(&[]));
                let n = self.s.len();
                let pos = self.i;
                let sub = |a: usize, b: usize| String::from_utf16_lossy(self.s.get(a..b.min(n)).unwrap_or(&[]));
                if n < CTX.saturating_mul(2).saturating_add(1) {
                    format!("Unexpected token '{ch}', \"{}\" is not valid JSON", self.src)
                } else if pos < CTX {
                    format!("Unexpected token '{ch}', \"{}\"... is not valid JSON", sub(0, pos.saturating_add(CTX)))
                } else if pos < n.saturating_sub(CTX) {
                    format!(
                        "Unexpected token '{ch}', ...\"{}\"... is not valid JSON",
                        sub(pos.saturating_sub(CTX), pos.saturating_add(CTX))
                    )
                } else {
                    format!("Unexpected token '{ch}', ...\"{}\" is not valid JSON", sub(pos.saturating_sub(CTX), n))
                }
            }
        }
    }
    fn expect(&mut self, c: u16, msg: &str) -> R {
        self.skip_ws();
        if self.cur() == Some(c) {
            self.i = self.i.saturating_add(1);
            Ok(())
        } else {
            Err(self.at_pos(msg))
        }
    }
    fn string(&mut self) -> R {
        // cursor on the opening quote
        self.i = self.i.saturating_add(1);
        loop {
            let Some(c) = self.cur() else { return Err(self.at_pos("Unterminated string")) };
            match c {
                0x22 => {
                    self.i = self.i.saturating_add(1);
                    return Ok(());
                }
                0x5c => {
                    self.i = self.i.saturating_add(1);
                    match self.cur() {
                        Some(0x22 | 0x5c | 0x2f | 0x62 | 0x66 | 0x6e | 0x72 | 0x74) => self.i = self.i.saturating_add(1),
                        Some(0x75) => {
                            for _ in 0..4 {
                                self.i = self.i.saturating_add(1);
                                if !self.cur().is_some_and(|h| matches!(h, 0x30..=0x39 | 0x41..=0x46 | 0x61..=0x66)) {
                                    return Err(self.at_pos("Bad Unicode escape"));
                                }
                            }
                            self.i = self.i.saturating_add(1);
                        }
                        None => return Err("Unexpected end of JSON input".into()),
                        Some(_) => return Err(self.at_pos("Bad escaped character")),
                    }
                }
                0..=0x1f => return Err(self.at_pos("Bad control character in string literal")),
                _ => self.i = self.i.saturating_add(1),
            }
        }
    }
    fn digits(&mut self) -> usize {
        let st = self.i;
        while self.cur().is_some_and(|c| (0x30..=0x39).contains(&c)) {
            self.i = self.i.saturating_add(1);
        }
        self.i.saturating_sub(st)
    }
    fn number(&mut self) -> R {
        if self.cur() == Some(0x2d) {
            self.i = self.i.saturating_add(1);
            if !self.cur().is_some_and(|c| (0x30..=0x39).contains(&c)) {
                return Err(self.at_pos("No number after minus sign"));
            }
        }
        if self.cur() == Some(0x30) {
            self.i = self.i.saturating_add(1);
            if self.cur().is_some_and(|c| (0x30..=0x39).contains(&c)) {
                return Err(self.at_pos("Unexpected number"));
            }
        } else {
            self.digits();
        }
        if self.cur() == Some(0x2e) {
            self.i = self.i.saturating_add(1);
            if self.digits() == 0 {
                return Err(self.at_pos("Unterminated fractional number"));
            }
        }
        if matches!(self.cur(), Some(0x65 | 0x45)) {
            self.i = self.i.saturating_add(1);
            if matches!(self.cur(), Some(0x2b | 0x2d)) {
                self.i = self.i.saturating_add(1);
            }
            if self.digits() == 0 {
                return Err(self.at_pos("Exponent part is missing a number"));
            }
        }
        Ok(())
    }
    fn literal(&mut self, w: &[u16]) -> R {
        self.i = self.i.saturating_add(1);
        for &x in w.iter().skip(1) {
            if self.cur() != Some(x) {
                return Err(self.unexpected());
            }
            self.i = self.i.saturating_add(1);
        }
        Ok(())
    }
    fn value(&mut self) -> R {
        // containers: true = object
        let mut stack: Vec<bool> = Vec::new();
        loop {
            self.skip_ws();
            match self.cur() {
                Some(0x22) => self.string()?,
                Some(0x2d | 0x30..=0x39) => self.number()?,
                Some(0x7b) => {
                    self.i = self.i.saturating_add(1);
                    self.skip_ws();
                    if self.cur() == Some(0x7d) {
                        self.i = self.i.saturating_add(1);
                    } else {
                        if self.cur() != Some(0x22) {
                            return Err(self.at_pos("Expected property name or '}'"));
                        }
                        self.string()?;
                        self.expect(0x3a, "Expected ':' after property name")?;
                        if stack.len() >= 10_000 {
                            return Err("Maximum call stack size exceeded".into());
                        }
                        stack.push(true);
                        continue;
                    }
                }
                Some(0x5b) => {
                    self.i = self.i.saturating_add(1);
                    self.skip_ws();
                    if self.cur() == Some(0x5d) {
                        self.i = self.i.saturating_add(1);
                    } else {
                        if stack.len() >= 10_000 {
                            return Err("Maximum call stack size exceeded".into());
                        }
                        stack.push(false);
                        continue;
                    }
                }
                Some(0x74) => self.literal(&[0x74, 0x72, 0x75, 0x65])?,
                Some(0x66) => self.literal(&[0x66, 0x61, 0x6c, 0x73, 0x65])?,
                Some(0x6e) => self.literal(&[0x6e, 0x75, 0x6c, 0x6c])?,
                _ => return Err(self.unexpected()),
            }
            // a value is done: close containers
            loop {
                let Some(&obj) = stack.last() else { return Ok(()) };
                self.skip_ws();
                if self.cur() == Some(0x2c) {
                    self.i = self.i.saturating_add(1);
                    if obj {
                        self.skip_ws();
                        if self.cur() != Some(0x22) {
                            return Err(self.at_pos("Expected double-quoted property name"));
                        }
                        self.string()?;
                        self.expect(0x3a, "Expected ':' after property name")?;
                    }
                    break;
                }
                if obj {
                    self.expect(0x7d, "Expected ',' or '}' after property value")?;
                } else {
                    self.expect(0x5d, "Expected ',' or ']' after array element")?;
                }
                stack.pop();
            }
        }
    }
}

/// Why `JSON.parse(src)` refuses `src` (None: V8 reads it).
pub fn parse_error(src: &str) -> Option<String> {
    let s: Vec<u16> = src.encode_utf16().collect();
    let mut p = P { s: &s, i: 0, src };
    if let Err(e) = p.value() {
        return Some(e);
    }
    p.skip_ws();
    if p.cur().is_some() {
        let (l, c) = p.loc(p.i);
        return Some(format!("Unexpected non-whitespace character after JSON at position {} (line {l} column {c})", p.i));
    }
    None
}
