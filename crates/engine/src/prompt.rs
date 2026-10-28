//! Commands that ask questions (the JS Engine's `await c.point(...)`), in safe Rust.
//!
//! A command is an `async` block that awaits [`Prompter::ask`]; [`Runner`] polls it with a no-op waker whenever
//! input arrives (single-threaded, no runtime, works on wasm). The document lives behind `Rc<RefCell<…>>` so a command
//! borrows it only between questions, exactly as the JS commands touch `doc` between `await`s.
//! Esc answers the waiting question with [`CmdError::Cancelled`]; the command's `?` unwinds it like the JS `Cancel`.

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

/// What a question wants (c.point / distance / angle / number / string / keyword / select / entity).
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Point,
    Distance,
    Angle,
    Number,
    Text,
    Keyword,
    Select,
    Entity,
    /// a file the window's picker is fetching (XATTACH, IMAGEATTACH … when the host's picker answers later: the web)
    File,
}

/// One question.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub kind: Kind,
    /// the prompt shown on the command line
    pub text: String,
    /// keyword options ("Close", "Undo", …): typed as their capital letters or in full
    pub options: Vec<String>,
    /// rubber-band base point (ortho, direct distance entry and previews use it)
    pub base: Option<[f64; 2]>,
    /// Enter on an empty line answers [`Answer::None`] instead of repeating
    pub allow_none: bool,
}

impl Request {
    pub fn new(kind: Kind, text: impl Into<String>) -> Self {
        Request { kind, text: text.into(), options: Vec::new(), base: None, allow_none: false }
    }
    pub fn options(mut self, o: &[&str]) -> Self {
        self.options = o.iter().map(|s| (*s).to_string()).collect();
        self
    }
    pub fn base(mut self, p: [f64; 2]) -> Self {
        self.base = Some(p);
        self
    }
    pub fn allow_none(mut self) -> Self {
        self.allow_none = true;
        self
    }
}

/// The answer to a question.
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    Point([f64; 2]),
    Value(f64),
    Text(String),
    Keyword(String),
    Selection(Vec<u64>),
    Entity {
        id: u64,
        at: [f64; 2],
        shift: bool,
    },
    /// Enter on an empty line (only when the request allows it)
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CmdError {
    /// Esc
    Cancelled,
    /// the command stopped with a message for the command line
    Msg(String),
}

impl std::fmt::Display for CmdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CmdError::Cancelled => f.write_str("*Cancel*"),
            CmdError::Msg(m) => f.write_str(m),
        }
    }
}

#[derive(Default, Debug)]
struct Slot {
    request: Option<Request>,
    answer: Option<Result<Answer, CmdError>>,
}

/// The command's side of the question channel (cheap to clone into a command's async block).
#[derive(Clone, Default, Debug)]
pub struct Prompter(Rc<RefCell<Slot>>);

impl Prompter {
    /// Ask a question; resolves when the runner delivers an answer (or Esc).
    pub fn ask(&self, req: Request) -> Ask {
        Ask { p: self.clone(), req: Some(req) }
    }
    /// Put an answer in the slot (a routine answering the command it runs: the command's next poll takes it).
    pub fn deliver(&self, a: Result<Answer, CmdError>) {
        self.0.borrow_mut().answer = Some(a);
    }
    /// Forget the question and any answer (a command dropped).
    pub fn clear(&self) {
        let mut s = self.0.borrow_mut();
        s.request = None;
        s.answer = None;
    }
}

/// The future of one question.
pub struct Ask {
    p: Prompter,
    req: Option<Request>,
}

impl Future for Ask {
    type Output = Result<Answer, CmdError>;
    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut slot = self.p.0.borrow_mut();
        if let Some(a) = slot.answer.take() {
            slot.request = None;
            return Poll::Ready(a);
        }
        drop(slot);
        if let Some(r) = self.req.take() {
            self.p.0.borrow_mut().request = Some(r);
        }
        Poll::Pending
    }
}

pub type CmdFuture = Pin<Box<dyn Future<Output = Result<(), CmdError>>>>;

/// Runs one command at a time.
#[derive(Default)]
pub struct Runner {
    fut: Option<CmdFuture>,
    pub prompter: Prompter,
    /// how the last command ended (None while one runs)
    pub last_result: Option<Result<(), CmdError>>,
}

impl std::fmt::Debug for Runner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runner").field("running", &self.fut.is_some()).field("waiting", &self.waiting()).finish()
    }
}

impl Runner {
    pub fn running(&self) -> bool {
        self.fut.is_some()
    }
    /// The question waiting for an answer.
    pub fn waiting(&self) -> Option<Request> {
        self.prompter.0.borrow().request.clone()
    }
    /// Start a command (a running one is cancelled first) and run it to its first question.
    pub fn start(&mut self, fut: CmdFuture) {
        if self.fut.is_some() {
            self.cancel();
        }
        self.fut = Some(fut);
        self.last_result = None;
        self.poll();
    }
    /// Deliver an answer to the waiting question and run on to the next one.
    pub fn answer(&mut self, a: Answer) {
        if self.fut.is_none() {
            return;
        }
        self.prompter.0.borrow_mut().answer = Some(Ok(a));
        self.poll();
    }
    /// Poll the command without an answer (it paused for the window).
    pub fn poll_now(&mut self) {
        self.poll();
    }
    /// Esc.
    pub fn cancel(&mut self) {
        if self.fut.is_none() {
            return;
        }
        self.prompter.0.borrow_mut().answer = Some(Err(CmdError::Cancelled));
        self.poll();
        // a command that swallows Cancel and asks again is stopped here
        if self.fut.is_some() {
            self.fut = None;
            self.last_result = Some(Err(CmdError::Cancelled));
        }
        let mut s = self.prompter.0.borrow_mut();
        s.request = None;
        s.answer = None;
    }
    fn poll(&mut self) {
        let Some(f) = self.fut.as_mut() else { return };
        let mut cx = Context::from_waker(Waker::noop());
        if let Poll::Ready(r) = f.as_mut().poll(&mut cx) {
            self.fut = None;
            self.last_result = Some(r);
            let mut s = self.prompter.0.borrow_mut();
            s.request = None;
            s.answer = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_command_asks_twice_then_finishes() {
        let out: Rc<RefCell<Vec<[f64; 4]>>> = Rc::default();
        let mut r = Runner::default();
        let p = r.prompter.clone();
        let o = Rc::clone(&out);
        r.start(Box::pin(async move {
            let Answer::Point(a) = p.ask(Request::new(Kind::Point, "Specify first point:")).await? else {
                return Err(CmdError::Msg("no".into()));
            };
            loop {
                match p.ask(Request::new(Kind::Point, "Specify next point:").base(a).allow_none()).await? {
                    Answer::Point(b) => o.borrow_mut().push([a[0], a[1], b[0], b[1]]),
                    _ => return Ok(()),
                }
            }
        }));
        assert_eq!(r.waiting().map(|q| q.text), Some("Specify first point:".into()));
        r.answer(Answer::Point([0.0, 0.0]));
        assert_eq!(r.waiting().and_then(|q| q.base), Some([0.0, 0.0]));
        r.answer(Answer::Point([10.0, 0.0]));
        r.answer(Answer::None);
        assert!(!r.running());
        assert_eq!(r.last_result, Some(Ok(())));
        assert_eq!(out.borrow().len(), 1);
    }

    #[test]
    fn escape_cancels() {
        let mut r = Runner::default();
        let p = r.prompter.clone();
        r.start(Box::pin(async move {
            p.ask(Request::new(Kind::Point, "x")).await?;
            Ok(())
        }));
        r.cancel();
        assert!(!r.running());
        assert_eq!(r.last_result, Some(Err(CmdError::Cancelled)));
        assert_eq!(r.waiting(), None);
    }
}
