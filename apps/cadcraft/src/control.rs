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

/// The apps whose 0.5 windows have live tools (agents.js); the others list only `app_info`.
const LIVE_APPS: [&str; 5] = ["cadcraft", "takeoffcraft", "markupcraft", "modelcraft", "detailcraft"];

impl Win<'_> {
    /// CadCraft's tools work on the drawing open for editing: a drawing only viewed is opened for editing first.
    fn cad_engine(&mut self) -> Result<&mut cc_engine::Engine, String> {
        if self.app.editor.is_none() {
            if self.app.source.is_none() {
                return Err("No drawing is open in CadCraft.".into());
            }
            self.app.run("file.edit", &cc_ui_egui::Params::default())?;
        }
        self.app.editor.as_mut().map(|e| &mut e.eng).ok_or_else(|| "No drawing is open in CadCraft.".to_string())
    }

    fn view_info(&self) -> Option<cc_mcp::cad::ViewInfo> {
        let v = self.app.views.get(self.app.space).copied().flatten()?;
        let corners = [v.to_world(0.0, 0.0), v.to_world(v.width, 0.0), v.to_world(0.0, v.height), v.to_world(v.width, v.height)];
        let (mut a, mut b) = ((f64::INFINITY, f64::INFINITY), (f64::NEG_INFINITY, f64::NEG_INFINITY));
        for (x, y) in corners {
            a = (a.0.min(x), a.1.min(y));
            b = (b.0.max(x), b.1.max(y));
        }
        Some(cc_mcp::cad::ViewInfo { min_x: a.0, min_y: a.1, max_x: b.0, max_y: b.1, zoom: v.zoom })
    }

    /// CadCraft's own Save: in place when the drawing has a file (desktop), else through the app's save service.
    fn cad_save(&mut self, format: Option<&str>) -> Result<Value, String> {
        let busy = self.app.editor.as_ref().is_some_and(|e| e.eng.busy());
        if busy {
            return Err("A command is running; try again when it is done.".into());
        }
        self.cad_engine()?;
        let src = self.app.source.clone();
        let ext = src.as_ref().map(|f| f.name.rsplit_once('.').map_or("", |(_, e)| e).to_ascii_lowercase()).unwrap_or_default();
        let last = self.app.save_format.clone();
        let fmt = match format {
            Some("dxf") => "dxf12".to_string(),
            Some("dxf2000") => "dxf2000".to_string(),
            // dwg: the version saved last, else 2018
            Some(_) => {
                if last.starts_with("dxf") {
                    "2018".to_string()
                } else {
                    last
                }
            }
            None if ext == "dxf" => "dxf2000".to_string(),
            None => last,
        };
        let ed = self.app.editor.as_mut().ok_or("No drawing is open in CadCraft.")?;
        let (bytes, out_ext, _report) = cc_io_cad_save(&ed.eng.doc(), &fmt)?;
        let printed;
        match src.as_ref().and_then(|f| f.dir.as_ref().map(|d| (d.clone(), f.name.clone()))) {
            Some((dir, name)) if !name.is_empty() => {
                let stem = name.rsplit_once('.').map_or(name.as_str(), |(s, _)| s);
                let path = std::path::Path::new(&dir).join(format!("{stem}.{out_ext}"));
                cc_mcp::confine::write_file_atomic(&path, &bytes)?;
                ed.mark_saved();
                ed.eng.doc_mut().mark_saved();
                printed = vec![format!("Saved {}", path.display())];
            }
            _ => {
                let name = format!("{}.{out_ext}", ed.name);
                let save = self.app.services.save.as_mut().ok_or("Saving is not available here.")?;
                match save(&name, bytes)? {
                    Some(to) => {
                        if let Some(ed) = self.app.editor.as_mut() {
                            ed.mark_saved();
                            ed.eng.doc_mut().mark_saved();
                        }
                        printed = vec![format!("Saved {to}")];
                    }
                    None => printed = vec!["Not saved.".to_string()],
                }
            }
        }
        let saved = self.app.editor.as_ref().is_some_and(|e| !e.dirty());
        self.app.sync_editor();
        Ok(json!({"saved": saved, "printed": printed}))
    }

    /// agents.js `drawing_plot` in the window: plotPdf on the drawing, saved where the person chooses (the 0.5 page
    /// downloads it), "Plotted <name>" on the command line.
    fn cad_plot(&mut self, a: &Value) -> Result<Value, String> {
        self.cad_engine()?;
        let ed = self.app.editor.as_mut().ok_or("No drawing is open in CadCraft.")?;
        let opts = cc_mcp::cad::plot_opts(a, cc_mcp::rpc::now_ms());
        let r = cc_io_cad::plot::plot_pdf(&ed.eng.doc(), &opts)?;
        let doc_name = {
            let d = ed.eng.doc();
            match d.meta.get("name") {
                Some(v) if cc_cad::js::truthy(Some(v)) => cc_cad::js::to_js_string(v),
                _ => "Drawing1".into(),
            }
        };
        let base = match a.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()) {
            Some(n) => n.to_string(),
            None => format!("{doc_name}{}", opts.layout.as_ref().map(|l| format!("-{l}")).unwrap_or_default()),
        };
        // [\\/:*?"<>|]+ → "_", then a ".pdf" ending dropped and put back
        let mut clean = String::new();
        let mut run = false;
        for ch in base.chars() {
            if matches!(ch, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                if !run {
                    clean.push('_');
                }
                run = true;
            } else {
                clean.push(ch);
                run = false;
            }
        }
        if clean.to_ascii_lowercase().ends_with(".pdf") {
            clean.truncate(clean.len().saturating_sub(4));
        }
        let name = format!("{clean}.pdf");
        let bytes_len = r.pdf.bytes.len();
        let save = self.app.services.save.as_mut().ok_or("Saving is not available here.")?;
        let _ = save(&name, r.pdf.bytes.clone())?;
        if let Some(ed) = self.app.editor.as_mut() {
            let msg = cc_ui_egui::edit::lang_text("agents.plotted", &[("name", name.clone())]);
            ed.hs.borrow_mut().log.push_back(msg);
        }
        let sheet = r.sheet_value();
        Ok(json!({
            "name": name, "bytes": bytes_len,
            "sheetMm": { "w": sheet.get("w").cloned().unwrap_or(Value::Null), "h": sheet.get("h").cloned().unwrap_or(Value::Null) },
            "scale": r.scale, "fitsOnPaper": r.fits, "unprintableCharacters": r.pdf.missing,
        }))
    }

    fn pdf_sheets(&self) -> Vec<cc_mcp::markups::LiveSheet> {
        let Some(p) = self.app.pdf.as_ref() else { return Vec::new() };
        // every PDF of the plan set (M9.15), in file order f1, f2, …; sheets are "<file id>:<page index>"
        let mut files: Vec<&cc_ui_egui::pdfview::PdfFile> = std::iter::once(&p.cur).chain(p.others.iter()).collect();
        files.sort_by_key(|f| f.id.trim_start_matches('f').parse::<u64>().unwrap_or(u64::MAX));
        files
            .into_iter()
            .flat_map(|f| {
                let stem = f.name.rsplit_once('.').map_or(f.name.as_str(), |(s, _)| s).to_string();
                f.pages.iter().map(move |pg| {
                    let title =
                        if pg.number.is_empty() { format!("{stem} — page {}", pg.index.saturating_add(1)) } else { pg.number.clone() };
                    cc_mcp::markups::LiveSheet {
                        key: format!("{}:{}", f.id, pg.index),
                        title,
                        width: pg.width,
                        height: pg.height,
                        view: pg.view,
                        file: Some(f.name.clone()),
                    }
                })
            })
            .collect()
    }
}

/// Save a drawing in one of CadCraft's save formats.
fn cc_io_cad_save(doc: &cc_cad::Doc, fmt: &str) -> Result<(Vec<u8>, &'static str, Value), String> {
    cc_io_cad::save_bytes(doc, fmt, None).map_err(|e| e.0)
}

impl Tools for Win<'_> {
    fn tools(&self) -> Vec<ToolDef> {
        let slug = self.app.active_app();
        if LIVE_APPS.contains(&slug) {
            return cc_mcp::defs::live_defs(slug);
        }
        let app = app_title(slug);
        vec![ToolDef::new(
            "app_info",
            "read",
            "About this window",
            &format!("{app}'s window: what is open, whether it has unsaved changes, and the permissions the person allowed."),
            None,
        )]
    }
    fn run(&mut self, name: &str, args: Value, ctx: &CallCtx) -> Result<ToolOut, ToolError> {
        let slug = self.app.active_app();
        let groups = self.groups.clone();
        let out = match slug {
            "cadcraft" => match name {
                "drawing_save" => self.cad_save(args.get("format").and_then(Value::as_str)),
                "drawing_plot" => self.cad_plot(&args),
                _ => {
                    let view = self.view_info();
                    let r = self.cad_engine().and_then(|eng| cc_mcp::cad::live_run(eng, name, &args, &groups, view));
                    self.app.sync_editor();
                    r
                }
            },
            "takeoffcraft" => {
                let p = self.app.pdf.as_ref().ok_or("No plan set is open.")?;
                let st = p.takeoff.tk.state.clone();
                let sheets: Vec<(String, String)> =
                    (0..p.cur.pages.len()).map(|i| (p.takeoff.sheet(i), cc_mcp::takeoff::sheet_label(&st, &p.takeoff.sheet(i)))).collect();
                let current = p.takeoff.sheet(p.page);
                if name == "takeoff_export" {
                    if p.takeoff.tk.items().is_empty() {
                        return Err(ToolError::new("The takeoff has no items yet."));
                    }
                    let fmt = args.get("format").and_then(Value::as_str).unwrap_or("xlsx").to_string();
                    let id = if fmt == "csv" { "csv" } else { "xlsx" };
                    self.app
                        .run("takeoff.export", &cc_ui_egui::Params { name: Some(id.into()), ..Default::default() })
                        .map(|()| json!({"exported": fmt}))
                } else {
                    cc_mcp::takeoff::live_run(&st, &sheets, Some(&current), name, &args, &groups)
                }
            }
            "markupcraft" => {
                let sheets = self.pdf_sheets();
                let p = self.app.pdf.as_mut().ok_or("No plan set is open.")?;
                let current = format!("f1:{}", p.page);
                let r = cc_mcp::markups::live_run(&mut p.store, &sheets, Some(&current), "", ctx.client.as_deref(), name, &args, &groups);
                match r {
                    Ok((v, go)) => {
                        if let Some(k) = go
                            && let Some(i) = k.strip_prefix("f1:").and_then(|i| i.parse::<f64>().ok())
                        {
                            let _ = self.app.run("pdf.page", &cc_ui_egui::Params { factor: Some(i + 1.0), ..Default::default() });
                        }
                        Ok(v)
                    }
                    Err(e) => Err(e),
                }
            }
            "modelcraft" => match name {
                "view_screenshot" => Err("A picture of the view is not available in this version of ModelCraft yet.".into()),
                _ => {
                    let mc = self.app.mc.as_mut().ok_or("No model is open for editing in ModelCraft.")?;
                    cc_mcp::modelcraft::run(&mut mc.tools.host.model, name, &args, &groups)
                }
            },
            "detailcraft" => match name {
                "app_info" => Ok(json!({"app": "detailcraft", "title": "DetailCraft", "allowed": groups})),
                other => {
                    Err(format!("{other} is not available in this version of DetailCraft yet: its assistant tools have not been ported."))
                }
            },
            other => match name {
                "app_info" => Ok(json!({"app": other, "title": app_title(other), "allowed": groups})),
                n => Err(format!("{n} is not available in {}.", app_title(other))),
            },
        };
        out.map(ToolOut::Json).map_err(ToolError::new)
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
    fn busy(&self) -> Option<String> {
        let ed = self.app.editor.as_ref()?;
        if ed.eng.busy() { ed.eng.running().map(str::to_string) } else { None }
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

    fn ask(c: &mut Control, app: &mut UiApp, m: Value) -> Value {
        let mut w = Win { app, groups: c.groups.clone() };
        c.handle(&m, "t", &mut w).unwrap_or(Value::Null)
    }
    fn call(c: &mut Control, app: &mut UiApp, name: &str, args: Value) -> Value {
        let r = ask(c, app, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": args}}));
        r["result"].clone()
    }

    /// The live tools on the real window state: CadCraft's on the sample drawing, ModelCraft's on the sample model.
    #[test]
    fn live_tools_act_on_the_window() {
        let all: Vec<String> = ["read", "edit", "save", "export"].iter().map(|s| s.to_string()).collect();
        let mut c = Control::new(all, "t");
        let mut app = UiApp::default();
        app.run("file.sample", &cc_ui_egui::Params::default()).unwrap();
        let list = ask(&mut c, &mut app, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}));
        let names: Vec<&str> = list["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names.len(), 14, "{names:?}");
        assert!(names.contains(&"command_run") && names.contains(&"view_describe"));
        let info = call(&mut c, &mut app, "app_info", json!({}));
        assert_eq!(info["structuredContent"]["app"], "cadcraft");
        let run = call(&mut c, &mut app, "command_run", json!({"input": ["LINE", "0,0", "@100,0", ""]}));
        assert_eq!(run["structuredContent"]["undo"], "Assistant: LINE", "{run}");
        let id = run["structuredContent"]["added"][0].as_u64().unwrap();
        let sel = call(&mut c, &mut app, "selection_set", json!({"ids": [id]}));
        assert_eq!(sel["structuredContent"]["count"], 1);
        let got = call(&mut c, &mut app, "selection_get", json!({}));
        assert_eq!(got["structuredContent"]["objects"][0]["type"], "line");
        let bad = call(&mut c, &mut app, "selection_set", json!({"ids": [999999]}));
        assert_eq!(bad["content"][0]["text"], "No objects 999999 in the space shown.");
        let undo = call(&mut c, &mut app, "undo", json!({}));
        assert_eq!(undo["structuredContent"]["undone"], json!(["Assistant: LINE"]));
        let plot = call(&mut c, &mut app, "drawing_plot", json!({}));
        assert_eq!(plot["isError"], true);
        // ModelCraft
        app.run("view.model.sample", &cc_ui_egui::Params::default()).unwrap();
        let list = ask(&mut c, &mut app, json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}));
        assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 9);
        let boxed = call(&mut c, &mut app, "box_add", json!({"size": [1, 2, 3], "at": [10, 0]}));
        assert_eq!(boxed["structuredContent"]["closed"], true, "{boxed}");
        assert!(boxed["structuredContent"]["volume"].as_f64().unwrap() > 0.0, "a 1 × 2 × 3 box in the model's display unit");
        let d = call(&mut c, &mut app, "model_describe", json!({}));
        assert!(d["structuredContent"]["instances"].as_array().unwrap().iter().any(|i| i["name"] == "Box"));
        let u = call(&mut c, &mut app, "undo", json!({}));
        assert_eq!(u["structuredContent"]["undone"], json!(["Assistant: box"]));
    }
}
