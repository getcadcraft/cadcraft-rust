//! A small backtracking regular-expression matcher with the JavaScript semantics the value box relies on (units.js):
//! leftmost match, alternatives tried in order, greedy `? * + {m,n}` and lazy `+? *? ??`, capture groups that fall
//! back to "undefined" when the group is backtracked out of, `^ $`, `.` (any character but a line terminator),
//! `\d` = [0-9] and `\s` = the JS white-space set, classes with ranges and negation, and the `i` flag (ASCII
//! letters, which is all the value box's patterns need). Patterns are constants of this crate; the subject is hostile
//! (typed text): matching runs on an explicit stack with a step budget, so no input can overflow the stack or hang.
//! Spec gap: past the budget (1,000,000 steps: far beyond anything a value box receives) the match is reported as no
//! match, where V8 would keep going.

const BUDGET: usize = 1_000_000;

#[derive(Clone, Debug)]
enum Item {
    Ch(char),
    Any,
    Class(Vec<(char, char)>, bool),
}

#[derive(Clone, Debug)]
enum Inst {
    Item(Item),
    /// try `.0` first, then `.1`
    Split(usize, usize),
    Jmp(usize),
    Save(usize),
    Bol,
    Eol,
    Match,
}

/// A compiled pattern.
#[derive(Clone, Debug)]
pub struct Re {
    prog: Vec<Inst>,
    groups: usize,
    icase: bool,
}

// ---- parsing (patterns are this crate's constants; a malformed one compiles to a program that matches nothing) ----

#[derive(Clone, Debug)]
enum Node {
    Empty,
    Item(Item),
    Bol,
    Eol,
    Group(Option<usize>, Box<Node>),
    Cat(Vec<Node>),
    Alt(Vec<Node>),
    Rep(Box<Node>, usize, Option<usize>, bool),
}

struct Parser<'a> {
    s: &'a [char],
    i: usize,
    groups: usize,
    bad: bool,
}

fn class_d() -> Vec<(char, char)> {
    vec![('0', '9')]
}
fn class_s() -> Vec<(char, char)> {
    vec![
        ('\u{09}', '\u{0D}'),
        (' ', ' '),
        ('\u{A0}', '\u{A0}'),
        ('\u{1680}', '\u{1680}'),
        ('\u{2000}', '\u{200A}'),
        ('\u{2028}', '\u{2029}'),
        ('\u{202F}', '\u{202F}'),
        ('\u{205F}', '\u{205F}'),
        ('\u{3000}', '\u{3000}'),
        ('\u{FEFF}', '\u{FEFF}'),
    ]
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }
    fn next(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.i = self.i.saturating_add(1);
        }
        c
    }
    fn alt(&mut self) -> Node {
        let mut alts = vec![self.cat()];
        while self.peek() == Some('|') {
            self.next();
            alts.push(self.cat());
        }
        if alts.len() == 1 { alts.pop().unwrap_or(Node::Empty) } else { Node::Alt(alts) }
    }
    fn cat(&mut self) -> Node {
        let mut list = Vec::new();
        while let Some(c) = self.peek() {
            if c == '|' || c == ')' {
                break;
            }
            let atom = self.atom();
            let atom = self.quant(atom);
            list.push(atom);
        }
        Node::Cat(list)
    }
    fn escape(&mut self) -> Item {
        match self.next() {
            Some('d') => Item::Class(class_d(), false),
            Some('D') => Item::Class(class_d(), true),
            Some('s') => Item::Class(class_s(), false),
            Some('S') => Item::Class(class_s(), true),
            Some('n') => Item::Ch('\n'),
            Some('t') => Item::Ch('\t'),
            Some(c) => Item::Ch(c),
            None => {
                self.bad = true;
                Item::Ch('\\')
            }
        }
    }
    fn class(&mut self) -> Item {
        let neg = self.peek() == Some('^');
        if neg {
            self.next();
        }
        let mut ranges = Vec::new();
        loop {
            let Some(c) = self.next() else {
                self.bad = true;
                break;
            };
            if c == ']' {
                break;
            }
            let lo = if c == '\\' {
                match self.escape() {
                    Item::Ch(x) => x,
                    Item::Class(r, false) => {
                        ranges.extend(r);
                        continue;
                    }
                    _ => {
                        self.bad = true;
                        continue;
                    }
                }
            } else {
                c
            };
            if self.peek() == Some('-') && self.s.get(self.i.saturating_add(1)).is_some_and(|x| *x != ']') {
                self.next();
                let hi = match self.next() {
                    Some('\\') => match self.escape() {
                        Item::Ch(x) => x,
                        _ => {
                            self.bad = true;
                            lo
                        }
                    },
                    Some(x) => x,
                    None => {
                        self.bad = true;
                        lo
                    }
                };
                ranges.push((lo, hi));
            } else {
                ranges.push((lo, lo));
            }
        }
        Item::Class(ranges, neg)
    }
    fn atom(&mut self) -> Node {
        match self.next() {
            Some('(') => {
                let cap = if self.peek() == Some('?') {
                    self.next();
                    if self.next() != Some(':') {
                        self.bad = true;
                    }
                    None
                } else {
                    self.groups = self.groups.saturating_add(1);
                    Some(self.groups)
                };
                let inner = self.alt();
                if self.next() != Some(')') {
                    self.bad = true;
                }
                Node::Group(cap, Box::new(inner))
            }
            Some('[') => Node::Item(self.class()),
            Some('\\') => Node::Item(self.escape()),
            Some('.') => Node::Item(Item::Any),
            Some('^') => Node::Bol,
            Some('$') => Node::Eol,
            Some(c) => Node::Item(Item::Ch(c)),
            None => Node::Empty,
        }
    }
    fn number(&mut self) -> Option<usize> {
        let mut n: Option<usize> = None;
        while let Some(c) = self.peek().filter(char::is_ascii_digit) {
            self.next();
            let d = usize::try_from(c.to_digit(10).unwrap_or(0)).unwrap_or(0);
            n = Some(n.unwrap_or(0).saturating_mul(10).saturating_add(d));
        }
        n
    }
    fn quant(&mut self, atom: Node) -> Node {
        let (min, max) = match self.peek() {
            Some('?') => (0, Some(1)),
            Some('*') => (0, None),
            Some('+') => (1, None),
            Some('{') => {
                self.next();
                let lo = self.number().unwrap_or(0);
                let hi = if self.peek() == Some(',') {
                    self.next();
                    self.number()
                } else {
                    Some(lo)
                };
                if self.peek() != Some('}') {
                    self.bad = true;
                }
                (lo, hi)
            }
            _ => return atom,
        };
        self.next();
        let lazy = self.peek() == Some('?');
        if lazy {
            self.next();
        }
        Node::Rep(Box::new(atom), min, max, lazy)
    }
}

// ---- compiling ----

fn emit(prog: &mut Vec<Inst>, n: &Node) {
    match n {
        Node::Empty => {}
        Node::Item(it) => prog.push(Inst::Item(it.clone())),
        Node::Bol => prog.push(Inst::Bol),
        Node::Eol => prog.push(Inst::Eol),
        Node::Group(cap, inner) => {
            if let Some(g) = cap {
                prog.push(Inst::Save(g.saturating_mul(2)));
            }
            emit(prog, inner);
            if let Some(g) = cap {
                prog.push(Inst::Save(g.saturating_mul(2).saturating_add(1)));
            }
        }
        Node::Cat(list) => {
            for x in list {
                emit(prog, x);
            }
        }
        Node::Alt(alts) => {
            // split L1, next; L1: a; jmp end; next: split L2 … ; last: z; end:
            let mut jumps = Vec::new();
            let k = alts.len();
            for (i, a) in alts.iter().enumerate() {
                if i.saturating_add(1) < k {
                    let at = prog.len();
                    prog.push(Inst::Split(0, 0));
                    emit(prog, a);
                    jumps.push(prog.len());
                    prog.push(Inst::Jmp(0));
                    let next = prog.len();
                    if let Some(s) = prog.get_mut(at) {
                        *s = Inst::Split(at.saturating_add(1), next);
                    }
                } else {
                    emit(prog, a);
                }
            }
            let end = prog.len();
            for j in jumps {
                if let Some(s) = prog.get_mut(j) {
                    *s = Inst::Jmp(end);
                }
            }
        }
        Node::Rep(inner, min, max, lazy) => {
            for _ in 0..*min {
                emit(prog, inner);
            }
            match max {
                Some(mx) => {
                    // nested optionals: (x(x(x)?)?)?
                    let extra = mx.saturating_sub(*min);
                    let mut splits = Vec::new();
                    for _ in 0..extra {
                        let at = prog.len();
                        prog.push(Inst::Split(0, 0));
                        splits.push(at);
                        emit(prog, inner);
                    }
                    let end = prog.len();
                    for at in splits {
                        let body = at.saturating_add(1);
                        if let Some(s) = prog.get_mut(at) {
                            *s = if *lazy { Inst::Split(end, body) } else { Inst::Split(body, end) };
                        }
                    }
                }
                None => {
                    // L: split body, end; body: x; jmp L; end:
                    let at = prog.len();
                    prog.push(Inst::Split(0, 0));
                    emit(prog, inner);
                    prog.push(Inst::Jmp(at));
                    let end = prog.len();
                    let body = at.saturating_add(1);
                    if let Some(s) = prog.get_mut(at) {
                        *s = if *lazy { Inst::Split(end, body) } else { Inst::Split(body, end) };
                    }
                }
            }
        }
    }
}

impl Re {
    /// Compile `pattern` (`icase`: the `i` flag).
    pub fn new(pattern: &str, icase: bool) -> Re {
        let chars: Vec<char> = pattern.chars().collect();
        let mut p = Parser { s: &chars, i: 0, groups: 0, bad: false };
        let node = p.alt();
        if p.i < chars.len() {
            p.bad = true;
        }
        let mut prog = Vec::new();
        if p.bad {
            // matches nothing
            prog.push(Inst::Item(Item::Class(Vec::new(), false)));
        } else {
            emit(&mut prog, &node);
        }
        prog.push(Inst::Match);
        Re { prog, groups: p.groups, icase }
    }

    fn item_ok(&self, it: &Item, c: char) -> bool {
        let fold = |x: char| if self.icase { x.to_ascii_lowercase() } else { x };
        match it {
            Item::Ch(x) => fold(*x) == fold(c),
            Item::Any => !matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'),
            Item::Class(ranges, neg) => {
                let inr = |x: char| ranges.iter().any(|(lo, hi)| *lo <= x && x <= *hi);
                let hit = if self.icase { inr(c) || inr(c.to_ascii_lowercase()) || inr(c.to_ascii_uppercase()) } else { inr(c) };
                hit != *neg
            }
        }
    }

    /// `s.match(re)` for a non-global pattern: the captures (index 0 = the whole match) or None.
    pub fn exec(&self, s: &str) -> Option<Vec<Option<String>>> {
        let chars: Vec<char> = s.chars().collect();
        let slots = self.groups.saturating_add(1).saturating_mul(2);
        let mut budget = BUDGET;
        for start in 0..=chars.len() {
            if let Some(caps) = self.run(&chars, start, slots, &mut budget) {
                let mut out = Vec::with_capacity(self.groups.saturating_add(1));
                for g in 0..=self.groups {
                    let a = caps.get(g.saturating_mul(2)).copied().flatten();
                    let b = caps.get(g.saturating_mul(2).saturating_add(1)).copied().flatten();
                    out.push(match (a, b) {
                        (Some(a), Some(b)) if a <= b => Some(chars.get(a..b).map(|x| x.iter().collect()).unwrap_or_default()),
                        _ => None,
                    });
                }
                return Some(out);
            }
            if budget == 0 {
                return None;
            }
        }
        None
    }

    /// `re.test(s)`.
    pub fn test(&self, s: &str) -> bool {
        self.exec(s).is_some()
    }

    fn run(&self, s: &[char], start: usize, slots: usize, budget: &mut usize) -> Option<Vec<Option<usize>>> {
        enum Frame {
            Try(usize, usize),
            Restore(usize, Option<usize>),
        }
        let mut caps: Vec<Option<usize>> = vec![None; slots];
        if let Some(c) = caps.get_mut(0) {
            *c = Some(start);
        }
        let mut stack: Vec<Frame> = vec![Frame::Try(0, start)];
        while let Some(f) = stack.pop() {
            let (mut pc, mut sp) = match f {
                Frame::Restore(slot, v) => {
                    if let Some(c) = caps.get_mut(slot) {
                        *c = v;
                    }
                    continue;
                }
                Frame::Try(pc, sp) => (pc, sp),
            };
            loop {
                if *budget == 0 {
                    return None;
                }
                *budget -= 1;
                let Some(inst) = self.prog.get(pc) else { break };
                match inst {
                    Inst::Match => {
                        if let Some(c) = caps.get_mut(1) {
                            *c = Some(sp);
                        }
                        return Some(caps);
                    }
                    Inst::Item(it) => match s.get(sp) {
                        Some(&c) if self.item_ok(it, c) => {
                            pc = pc.saturating_add(1);
                            sp = sp.saturating_add(1);
                        }
                        _ => break,
                    },
                    Inst::Bol => {
                        if sp != 0 {
                            break;
                        }
                        pc = pc.saturating_add(1);
                    }
                    Inst::Eol => {
                        if sp != s.len() {
                            break;
                        }
                        pc = pc.saturating_add(1);
                    }
                    Inst::Jmp(t) => pc = *t,
                    Inst::Split(a, b) => {
                        if stack.len() > 4_000_000 {
                            return None;
                        }
                        stack.push(Frame::Try(*b, sp));
                        pc = *a;
                    }
                    Inst::Save(slot) => {
                        let old = caps.get(*slot).copied().flatten();
                        stack.push(Frame::Restore(*slot, old));
                        if let Some(c) = caps.get_mut(*slot) {
                            *c = Some(sp);
                        }
                        pc = pc.saturating_add(1);
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn backtracking_like_js() {
        let n = r"(\d+(?:\.\d*)?|\.\d+)";
        let re = Re::new(&format!(r#"^{n}\s*'\s*(?:-?\s*{n}?\s*(?:(\d+)\s*/\s*(\d+))?\s*"?)?$"#), false);
        let m = re.exec("10' 12/16").unwrap();
        assert_eq!(m[1].as_deref(), Some("10"));
        assert_eq!(m[2].as_deref(), Some("1"));
        assert_eq!(m[3].as_deref(), Some("2"));
        assert_eq!(m[4].as_deref(), Some("16"));
        let m = re.exec("10'").unwrap();
        assert_eq!(m[2], None);
        let lazy = Re::new(r"^([[<])\s*(.+?)\s*([\]>])$", false);
        let m = lazy.exec("[ 1, 2 ,3 ]").unwrap();
        assert_eq!(m[2].as_deref(), Some("1, 2 ,3"));
        let ic = Re::new(r"^(\d{1,4})\s*s$", true);
        assert!(ic.test("24S") && ic.test("1234 s") && !ic.test("12345s"));
        assert!(Re::new("^(?:(\\d{1,4})\\s*x|[*x]\\s*(\\d{1,4}))$", true).exec("*3").unwrap()[2].as_deref() == Some("3"));
    }
}
