# The control channel and MCP

Last reviewed 2026-10-18 · Last updated 2026-10-18 · Change: M16.1, the local control channel (Studio's rules) and the
dispatcher.

| Date | Change |
|---|---|
| 2026-10-18 | M16.1: `cc-mcp` dispatcher (core.mjs), HTTP control channel (control.rs), token, confinement; `cadcraft --control`. |
| 2026-10-18 | M16.2: the headless MCP server `cadcraft-cli mcp` with 0.5's 22 tools; parity harness `forks/tools/mcpdiff.mjs`. |

The apps contain no AI. They give the person's own assistant (or script) **tools**, and the person decides which
permission groups are allowed: `read`, `edit`, `save`, `export` (0.5 `docs/AGENTS-SPEC.md` § 3). Tool names, titles,
descriptions and argument schemas are 0.5's, so an assistant set up for CadCraft 0.5 works unchanged (AGENTS.md § 15).

## Surfaces

| Surface | Command | Transport | Code |
|---|---|---|---|
| Headless MCP | `cadcraft-cli mcp [--root DIR]… [--automation-read-root DIR]… [--automation-write-root DIR]… [--allow …] [--log f.jsonl] [--verbose]` (or `CADCRAFT_MCP_ROOTS` / `CADCRAFT_MCP_ALLOW`) | MCP over stdio (newline-delimited JSON-RPC 2.0; stdout is protocol only; lines ≤ 4 MB) | `cc_mcp::headless`, `cc_mcp::{cad, takeoff, markups, ifc, defs}`, `apps/cadcraft-cli/src/mcp.rs` |
| Relay to a window | `cadcraft-cli mcp --connect PORT --token-file control.token` | stdio → the window's `/mcp` | `apps/cadcraft-cli/src/mcp.rs` |
| Live control (the open window) | `cadcraft --control [PORT] [--allow read,edit,save,export]` (or `CADCRAFT_CONTROL_PORT` / `APP_CONTROL_PORT`) | MCP over Streamable HTTP (`POST /mcp`) and the JSON control channel (`POST /control`) on 127.0.0.1 | `cc_mcp::http`, `cc_mcp::live`, `apps/cadcraft/src/control.rs` |

## The headless tools (0.5 `tools/cli/headless/tools.mjs`, AGENTS-SPEC § 5.1)

`files_list`, `drawing_open`, `drawing_new`, `drawing_describe`, `objects_list`, `object_get`, `drawing_check`,
`command_run`, `script_run`, `undo`, `drawing_save`, `drawing_convert`, `drawing_plot`, `doc_close`, `ifc_open`,
`ifc_query`, `ifc_properties`, `takeoff_open`, `takeoff_quantities`, `takeoff_estimate`, `takeoff_export`, `markups_list`
— names, titles, descriptions, schemas and annotations byte for byte 0.5's. The drawing tools run on the Rust `Doc` and
command line (`cc_mcp::cad` = the app's agents.js: describeDrawing, listObjects, checkDrawing, runInput with one undo
step labelled `Assistant: …`); IFC on cc-io-ifc + cc-bim (ifc.mjs), takeoffs on cc-takeoff (agents.js + takeoff.mjs),
markups on cc-io-markup (markup.mjs). Two answers are "not available yet": `drawing_plot` (CadCraft's plot.js is not
ported) and `script_run` with `language: "lisp"` ("This CadCraft build has no LISP.", 0.5's own sentence). Only the
commands the Rust command line has ported run (LINE PLINE CIRCLE ARC RECTANG POLYGON ELLIPSE POINT TEXT ERASE MOVE COPY
ROTATE SCALE MIRROR OFFSET TRIM EXTEND UNDO REDO ZOOM); others answer "Unknown command" as the app's command line does.

Confinement (AGENTS.md § 9, paths.mjs): on by default — with no folder option the current folder is the only root.
`--root` (0.5's flag) is a read and write root; `--automation-read-root` / `--automation-write-root` split them. Paths are
canonicalised (links followed, `..` folded) and must stay inside; device paths, NUL bytes, and on Windows UNC paths,
alternate data streams and device names are refused first; a file that exists is replaced only when this session opened or
wrote it, or with `overwrite: true`; a link is never written through.

Parity: `node forks/tools/mcpdiff.mjs` runs the frozen 0.5 server (`tools/mcp/server.mjs` at 594ab3e on the frozen
js-0.5 app build) and `cadcraft-cli mcp` side by side on one script (initialize, tools/list, 240 tool calls incl.
refusals, confinement, saves read back, every element of the IFC house, the takeoff fixture, two marked-up PDFs; then a
read-only server). Written DXF/CSV files compared byte for byte, XLSX entry by entry.

## Rules of the HTTP channel (desktop-shell `control.rs`, exactly)

- Listens on **127.0.0.1 only**, port 47615 by default, else the next free one of 20. Off unless started for this run.
- **Token**: 32 random bytes from the OS random source (getrandom), base64url without padding (43 characters).
  Printed on stderr and written to `control.token` in the config folder (`APP_CONFIG_DIR`, else
  `$XDG_CONFIG_HOME/cadcraft-rust`, `~/Library/Application Support/cadcraft-rust`, `%APPDATA%\cadcraft-rust`), the file
  `0600` and its folder `0700` on Unix. Every request needs `Authorization: Bearer <token>`, compared in constant time,
  else **401** with `WWW-Authenticate: Bearer realm="CadCraft Studio"`. Never in a URL: a query string is **400**.
- **Host** other than `127.0.0.1:<port>` / `localhost:<port>` → **403** (DNS rebinding). **Any `Origin` header → 403**
  (browsers send one; MCP clients do not).
- `POST /mcp`: JSON answers, no SSE. `initialize` issues `Mcp-Session-Id` (32 hex); every later request needs it
  (missing 400, unknown or ended 404); a notification or a response → **202**; `GET` → **405**; `DELETE` with the session
  ends it (**204**); at most 32 sessions (the oldest goes). An answer with error -32002 (the person disconnected) ends the
  session. `POST /control`: the same token; optional `X-CadCraft-Session` (64 safe characters).
- **JSON only** (`Content-Type: application/json`, else 415), **≤ 4 MB** (413; only 4 MB + 1 bytes are read), no
  batches (400), anything else 404.
- On top of control.rs (AGENTS.md § 9): every refusal closes the connection; a request that is not HTTP (a bare JSON
  line, a probe) is closed without a byte of answer; at most 16 connections; 30 s idle timeout; request heads ≤ 64 KB,
  lines ≤ 8 KB, ≤ 100 headers; `Content-Length` with `Transfer-Encoding`, or two different lengths, is refused.
- Requests are answered on the UI thread (the window's state is not shared across threads): the server queues each
  JSON-RPC message, wakes the window, and waits ≤ 120 s (then -32003 "The app window did not answer in time.").

## Messages (0.5 `tools/mcp/core.mjs`)

JSON-RPC 2.0, one object per message. MCP: `initialize` (protocol versions 2025-11-25, 2025-06-18, 2025-03-26,
2024-11-05: the client's when it is one of them, else the newest), `notifications/initialized`, `ping`, `tools/list`,
`tools/call`, `logging/setLevel`; anything else -32601. Control channel: `control.hello`, `tools.list`, `tools.call`,
`app.info`. Arguments are checked against the tool's schema before it runs (type, enum, const, minimum, maximum,
minLength, maxLength, pattern, items, min/maxItems, properties, required, additionalProperties, anyOf/oneOf; defaults
filled); a refusal is a tool result with `isError: true` and 0.5's sentence. Every call is logged (last 500 in the
window): time, client, tool, group, arguments (strings over 200 characters cut), outcome, undo label, ms.

## The window (host.js `createControlRelay` + `makeControl`)

The fork has one window, `main`. `tools/list` = Studio's `windows_list` + the window's tools (each with an optional
`window` argument); a tool call naming another window is refused with 0.5's sentence. Edit, save and export calls are
refused while the person is in the middle of a command. Which app the window is follows what is open in it.

## Fuzzing

`fuzz/fuzz_targets/control_request.rs` (the HTTP reader + the rules) and `fuzz/fuzz_targets/mcp_message.rs` (the
dispatcher, the window routing and argument checks on any JSON).
