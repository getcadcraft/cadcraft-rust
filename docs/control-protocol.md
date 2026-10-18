# The control channel and MCP

Last reviewed 2026-10-18 · Last updated 2026-10-18 · Change: M16.1, the local control channel (Studio's rules) and the
dispatcher.

| Date | Change |
|---|---|
| 2026-10-18 | M16.1: `cc-mcp` dispatcher (core.mjs), HTTP control channel (control.rs), token, confinement; `cadcraft --control`. |

The apps contain no AI. They give the person's own assistant (or script) **tools**, and the person decides which
permission groups are allowed: `read`, `edit`, `save`, `export` (0.5 `docs/AGENTS-SPEC.md` § 3). Tool names, titles,
descriptions and argument schemas are 0.5's, so an assistant set up for CadCraft 0.5 works unchanged (AGENTS.md § 15).

## Surfaces

| Surface | Command | Transport | Code |
|---|---|---|---|
| Live control (the open window) | `cadcraft --control [PORT] [--allow read,edit,save,export]` (or `CADCRAFT_CONTROL_PORT` / `APP_CONTROL_PORT`) | MCP over Streamable HTTP (`POST /mcp`) and the JSON control channel (`POST /control`) on 127.0.0.1 | `cc_mcp::http`, `cc_mcp::live`, `apps/cadcraft/src/control.rs` |

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
