//! The desktop app's control channel (AGENTS.md § 9, § 15; desktop-shell control.rs): the HTTP server runs on its own
//! threads (cc_mcp::http); each JSON-RPC message is queued here and answered on the UI thread, where the window's
//! tools act on `UiApp` (cc_mcp::live). Started with `--control [port]` (or CADCRAFT_CONTROL_PORT / APP_CONTROL_PORT),
//! off otherwise; the token is printed on stderr and written 0600 to `control.token` in the config folder.

use std::collections::VecDeque;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use cc_mcp::live::{Control, Window, app_title};
use cc_mcp::{CallCtx, ToolDef, ToolError, ToolOut, Tools};
use cc_ui_egui::UiApp;
use serde_json::{Value, json};

/// How long a request waits for the window (control.rs ANSWER_WAIT).
const ANSWER_WAIT: Duration = Duration::from_secs(120);

struct Job {
    msg: Value,
    session: String,
    reply: Sender<Option<Value>>,
}

type Queue = Arc<Mutex<VecDeque<Job>>>;

/// The running channel.
pub struct Channel {
    pub server: cc_mcp::http::Server,
    queue: Queue,
    /// the UI's context (to wake it when a request arrives)
    pub ctx: Arc<Mutex<Option<egui::Context>>>,
    pub control: Control,
    pub token_path: Option<std::path::PathBuf>,
}

impl Channel {
    /// Start on 127.0.0.1:`port` (or the next free one) with the permission groups the person allowed; the token file
    /// goes into `config` (the config folder).
    pub fn start(port: u16, groups: Vec<String>, config: Option<std::path::PathBuf>) -> Result<Channel, String> {
        let token = cc_mcp::token::random_token()?;
        let queue: Queue = Arc::new(Mutex::new(VecDeque::new()));
        let ctx: Arc<Mutex<Option<egui::Context>>> = Arc::new(Mutex::new(None));
        let (q2, c2) = (queue.clone(), ctx.clone());
        let forward = Arc::new(move |msg: &Value, session: &str| -> Result<Option<Value>, ()> {
            let (tx, rx) = channel();
            q2.lock().unwrap_or_else(PoisonError::into_inner).push_back(Job { msg: msg.clone(), session: session.to_string(), reply: tx });
            if let Some(c) = c2.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
                c.request_repaint();
            }
            match rx.recv_timeout(ANSWER_WAIT) {
                Ok(a) => Ok(a),
                Err(_) => Ok(msg.get("id").map(|id| json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32003, "message": "The app window did not answer in time."}}))),
            }
        });
        let server = cc_mcp::http::start(port, token.clone(), forward)?;
        let token_path = config.and_then(|d| cc_mcp::token::write_token_file(&d, &token).ok());
        // AGENTS.md § 9: the token goes to stderr (for the person setting up an assistant) and to the token file
        eprintln!("CadCraft control channel: {}  (allowed: {})", server.url(), groups.join(", "));
        eprintln!("Token: {token}");
        match &token_path {
            Some(p) => eprintln!("Token file: {}", p.display()),
            None => eprintln!("Token file: not written (no config folder)"),
        }
        let control = Control::new(groups, env!("CARGO_PKG_VERSION"));
        Ok(Channel { server, queue, ctx, control, token_path })
    }

    /// The line the status bar shows while the channel is on (never the token itself).
    pub fn status_line(&self) -> String {
        match &self.token_path {
            Some(p) => format!("Assistants can connect at {} (token in {})", self.server.url(), p.display()),
            None => format!("Assistants can connect at {} (token on the terminal)", self.server.url()),
        }
    }

    /// Answer every queued request (call once per frame, on the UI thread).
    pub fn pump(&mut self, app: &mut UiApp) {
        loop {
            let job = self.queue.lock().unwrap_or_else(PoisonError::into_inner).pop_front();
            let Some(job) = job else { break };
            let groups = self.control.groups.clone();
            let mut win = Win { app: &mut *app, groups };
            // a tool must never take the window down: a panic becomes an error answer
            let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.control.handle(&job.msg, &job.session, &mut win)))
                .unwrap_or_else(|_| {
                    job.msg.get("id").map(|id| json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32603, "message": "Internal error"}}))
                });
            let _ = job.reply.send(answer);
        }
    }
}

/// The window as the control channel sees it.
pub struct Win<'a> {
    pub app: &'a mut UiApp,
    pub groups: Vec<String>,
}

impl Tools for Win<'_> {
    fn tools(&self) -> Vec<ToolDef> {
        let slug = self.app.active_app();
        let app = app_title(slug);
        vec![ToolDef::new(
            "app_info",
            "read",
            "About this window",
            &format!("{app}'s window: what is open, whether it has unsaved changes, and the permissions the person allowed."),
            None,
        )]
    }
    fn run(&mut self, name: &str, _args: Value, _ctx: &CallCtx) -> Result<ToolOut, ToolError> {
        match name {
            "app_info" => {
                let slug = self.app.active_app();
                Ok(ToolOut::Json(json!({"app": slug, "title": app_title(slug), "allowed": self.groups})))
            }
            other => Err(ToolError::new(format!("{other} is not available in this window."))),
        }
    }
    fn info(&mut self) -> Option<Value> {
        let slug = self.app.active_app();
        Some(json!({"app": slug, "title": app_title(slug), "version": env!("CARGO_PKG_VERSION"), "enabled": true, "groups": self.groups}))
    }
}

impl Window for Win<'_> {
    fn app(&self) -> &'static str {
        self.app.active_app()
    }
    fn title(&self) -> String {
        app_title(self.app.active_app()).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole path: an assistant's HTTP request → the queue → the UI thread's pump → the answer.
    #[test]
    fn requests_reach_the_window() {
        let dir = std::env::temp_dir().join(format!("cc-control-app-{}", std::process::id()));
        let mut ch = Channel::start(0, vec!["read".into()], Some(dir.clone())).unwrap();
        assert_eq!(cc_mcp::token::read_token_file(&dir.join("control.token")).unwrap(), *ch.server.run.token.lock().unwrap());
        let port = ch.server.port();
        let token = ch.server.run.token.lock().unwrap().clone();
        let auth = format!("Bearer {token}");
        let t = std::thread::spawn(move || {
            let init = cc_mcp::http::request(
                port,
                "POST",
                "/mcp",
                &[("Authorization", &auth), ("Content-Type", "application/json")],
                br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"test"}}}"#,
            )
            .unwrap();
            let sid = init.1.iter().find(|(k, _)| k == "Mcp-Session-Id").map(|(_, v)| v.clone()).unwrap();
            let call = cc_mcp::http::request(
                port,
                "POST",
                "/mcp",
                &[("Authorization", &auth), ("Content-Type", "application/json"), ("Mcp-Session-Id", &sid)],
                br#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"windows_list"}}"#,
            )
            .unwrap();
            serde_json::from_slice::<Value>(&call.2).unwrap()
        });
        let mut app = UiApp::default();
        let t0 = std::time::Instant::now();
        while !t.is_finished() && t0.elapsed() < Duration::from_secs(20) {
            ch.pump(&mut app);
            std::thread::sleep(Duration::from_millis(5));
        }
        let v = t.join().unwrap();
        assert_eq!(v["result"]["structuredContent"]["windows"][0]["app"], "studio");
        assert_eq!(v["result"]["structuredContent"]["windows"][0]["tools"], 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
