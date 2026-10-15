# Building a pure-Rust desktop + web creative app

Drop this file into any empty repo (as `AGENTS.md` or `CLAUDE.md`) and tell the agent what to build.
It needs no other context. It defines the stack, the architecture, the rules, the UI approach, the design
language and the working loop. The *product* (what the app does, what it looks like) comes from the person's
request, their reference images and their reference app.

If a rule here conflicts with the request, follow the request and say so in one line. **Exception:** the safety rules
(section 4 "Never crash" and "Everything from outside is hostile", section 9 "Security") and the project rules in
section 15 are never relaxed by a request; say what conflicts and ask.

> Last reviewed 2026-10-15 · Change: hardened from the pasted draft for the CadCraft Rust fork (see `docs/forks/README.md`).

---

## 1. What you are building

A native Windows / macOS / Linux application **and** a browser build (WASM), from **one Rust codebase**:

- **Pure Rust.** No handwritten JS/TS, no C/C++ dependencies unless unavoidable (and then say so and isolate them behind a trait).
- **GUI in Rust** with egui, rendered through wgpu. No webview, no Electron.
- **Headless-capable.** Every feature works without a window, from a CLI, a script, or an AI agent.
- **Never loses user work.** Crash-proof by construction (section 4), on desktop **and** web.
- **Fast.** Multithreaded where it pays, cached, damage-based repainting.

---

## 2. How to start a task

1. **Read the request and any reference material first.**
   - Reference images: look at every one. Note layout (regions and proportions), density, colours, type, icon style, control shapes. Write what you measured into `docs/ui-notes.md` (pixel sizes, hex values) *before* coding the UI. Measure, do not guess.
   - Reference app or files: use it black-box only (section 4, "Clean-room"). List its features into `docs/gaps.md`, ranked.
2. **If the request is large, write a short plan** (`docs/roadmap.md`: milestones, each ending in something runnable) and start on milestone 1. Do not stop to ask unless the decision is genuinely the user's (product name, licence, paid services, destructive actions, anything published or released).
3. **Build vertically.** Model + command + test first, then UI on top of it. A feature is done when it works headless, has a test, and can be seen in a screenshot.
4. **Run the quality gate** (section 11) before every commit. One task per commit, with a task id in the message.
5. **Look at your result.** After any UI change, render a screenshot (section 9) and read it. Compare against the reference images. Fix what is visibly off. Do not claim UI work is done on the strength of compiling.

---

## 3. Stack (pin these; bump together)

The versions below were current when this file was written. On day one, check each against crates.io, pin the
newest stable that compiles together, and write the pins into `docs/architecture.md`. Never guess an API from this
file: read the pinned crate's docs (`cargo doc --open`) before writing against it.

| Concern | Choice |
|---|---|
| Language | Rust, edition 2024, resolver 3, `rust-version` 1.95 or newer, toolchain pinned in `rust-toolchain.toml` |
| UI | `egui` + `eframe` (same minor version, e.g. 0.36), `egui_extras` with `svg` feature |
| GPU | `wgpu` through eframe. Backends: DX12 (Windows), Metal (macOS), Vulkan (Linux), OpenGL fallback natively; **web: WebGPU only** (section 10, Web performance) |
| UI tests | `egui_kittest` with `wgpu` + `snapshot` features |
| Math / paths | `kurbo` (2D), `glam` (DVec, f64) or `nalgebra` (3D) |
| 2D CPU raster (if the app is 2D) | `vello_cpu` |
| Images | `image` (disable default features; enable only the codecs needed; **always set `image::Limits`**) |
| Text | `skrifa` (outlines, metrics) + `harfrust` (shaping); `unicode-bidi`, `unicode-segmentation` |
| Serialisation | `serde`, `serde_json` with `float_roundtrip` |
| Errors | `thiserror` |
| Parallelism | `rayon` (native only) |
| Archives / XML | `zip` (deflate only), `quick-xml` |
| File dialogs / clipboard | `rfd`, `arboard` (native only) |
| Web build | `trunk` (`apps/<name>-web`, `Trunk.toml`, `index.html`) |
| Property tests / fuzzing | `proptest`; `cargo-fuzz` (libFuzzer) targets in `fuzz/` |
| Supply chain | `cargo-deny` (licences, advisories, bans, sources) with `deny.toml` |

Before adding any dependency: permissive licence (MIT / Apache-2.0 / BSD / ISC / Zlib / Unicode-3.0 / OFL for fonts),
compiles for `wasm32-unknown-unknown` if it sits below the UI layer, no `unsafe` leaking into your crates, maintained
(a release in the last two years or a clear reason), and its `build.rs` and proc-macros read before it goes in (they run
on every developer's machine). Record it in `THIRD_PARTY.md`. `deny.toml` allows only crates.io plus git sources pinned
by `rev`. `Cargo.lock` is committed; CI builds with `--locked`.

Dev profile: `opt-level = 1`, and `[profile.dev.package."*"] opt-level = 2`, otherwise debug builds are unusably slow.
Release: `lto = "thin"`, `codegen-units = 16`, **`panic = "unwind"`** (never `abort`: the last-resort guard in
section 4 depends on unwinding), `overflow-checks = true` in crates that parse files (the cost is small; a silent
wrap in a size calculation is a memory-sized bug).

---

## 4. Non-negotiable rules

### Never crash

A crash loses someone's work. This outranks feature work: fix a panic before building on top of it.

- Workspace: `[workspace.lints.rust] unsafe_code = "forbid"`.
- Every non-test crate starts with
  `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable, clippy::indexing_slicing)]`.
  Crates that read files or network data (L1 model loaders, L2 parsers, all `io-*`, `format`, `engine` param
  validation, `mcp`) also deny `clippy::arithmetic_side_effects`, `clippy::cast_possible_truncation`,
  `clippy::cast_sign_loss` and `clippy::cast_possible_wrap`.
  Put `allow-unwrap-in-tests = true`, `allow-expect-in-tests = true`, `allow-indexing-slicing-in-tests = true` in `clippy.toml`.
- No `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!`, `unreachable!`, slice indexing on untrusted data. The only exception is a provably infallible literal (constant regex, `write!` to a `String`): `#[allow(clippy::expect_used)]` plus `.expect("why it cannot fail")`.
- Errors are `Result<T, E>` with the crate's error type and `?`. An unfinished feature returns an "unsupported" error. Never `unwrap_or_default()` where a silent default would corrupt a document.
- **Everything from outside is hostile**: files, command params, network, settings, scripts, add-ons, clipboard, dropped files, agent input. Use `.get()`; slice strings at char boundaries; checked/saturating arithmetic for sizes and offsets; no division by zero or NaN-to-int casts; cap every allocation derived from input; bound recursion with depth limits or seen-sets (cyclic documents exist).
- **Some failures are not panics and cannot be caught**: stack overflow and out-of-memory abort the process. That is why recursion limits and allocation caps are mandatory, not style. Deeply nested formats are parsed iteratively or with an explicit depth counter.
- Locks: `lock().unwrap_or_else(PoisonError::into_inner)`. Thread joins return `Result`s.
- **Last-resort guard (desktop):** wrap command execution and every import/export in `catch_unwind`. On a caught panic, **restore the document to the snapshot taken before the command** (a half-applied edit is a corrupt document, not a survivor), turn the panic into an error, log it with the command id and a backtrace, and show it. A safety net, not a licence.
- **Last-resort guard (web):** `wasm32-unknown-unknown` aborts on panic; `catch_unwind` does nothing there. The web build therefore installs a panic hook that tells the page the instance died, and relies on recovery snapshots written to browser storage (section 6, Recovery) so the reload offers the work back. Test it: a forced panic in the web build, then reload, recovers the document.
- **Every crash fix lands with a small synthetic regression test that panicked before the fix.** Fuzz finds go into the corpus of that fuzz target as well.

### Everything is a command

User-visible behaviour is a command in the engine, never logic inside UI code:

- Menus, shortcuts, the command palette, the CLI, the control channel and the agent server all call the **same** `Session::execute(id, params)`.
- A tool gesture is a sequence of commands (section 6).
- UI-only commands (toggle panel, zoom, open dialog) live in the UI crate in a `UI_COMMANDS` table and are reachable the same way.
- Every journaled command is replayable as a script. Commands are deterministic: no wall clock, no random numbers and no thread-order effects inside `run` except through values the session passes in (a seed, a timestamp), which are journaled with the command.

### The UI is thin

Panels read engine state and act through `app.run(id, params)`. Panels contain layout and widgets, nothing else. All colours come from the theme tokens; widgets never hard-code a colour.

### Layering is enforced

Crates are numbered layers; a crate may depend only on strictly lower layers (section 5). **Nothing below the UI layer may depend on egui, eframe, winit or rfd.** The UI crate must be swappable. Enforce it with a `cargo xtask layers` check (read `cargo metadata`, compare each crate's layer from its `[package.metadata.layer]`) that fails CI.

### Clean-room

When reimplementing the behaviour of an existing product:

- Observe it black-box only: run it, use its UI with synthetic data, take screenshots, measure. Never read, disassemble or copy its binaries, resources, icons, artwork, presets or wording beyond feature names.
- Never copy GPL/AGPL/LGPL/MPL code. Copyleft tools may be used **only as test oracles run as programs** (give them files, compare output); never link, ship or read their source.
- If the spec is silent, find the answer by experiment on real files and write it down in a comment ("Spec gap: …").
- Do not use another product's name in titles or branding; mention it only to say what a format or workflow is compatible with.
- Do not copy another vendor's design-system values either (their published colour tokens, icon set, spacing scale). Measure the person's references for the *feel*; the values in `Tokens` are your own.

### Assets

- No third-party brand icons or images. Every asset is original, OSI-licensed or CC0, and has a row in `ASSETS.md` (author, source, licence). `cargo xtask assets` fails if a file in `assets/` has no row.
- **Prefer art generated in code** (icons, backgrounds, sample documents). It sidesteps licensing and scales perfectly.
- Fonts: bundle only OFL fonts (e.g. Inter, Source Sans 3, JetBrains Mono). Large or CJK font sets live in a separate repo/dir used as an optional build input (`FONTS_DIR=… cargo build`); the app must work, and tests must pass, with that input unset.
- Large binary test fixtures stay out of the repo: fetch them pinned by commit and sha256-verified from a corpus repo (`cargo xtask corpus`). Customer or user files never go in any repo, public or private.

### Rust-only and wasm

No handwritten JS/TS. Never break the web build: `cargo xtask wasm` (check all crates up to the UI layer for `wasm32-unknown-unknown`) is part of CI. Anything native-only (threads, sockets, file dialogs, clipboard) sits behind `#[cfg(not(target_arch = "wasm32"))]` with a web alternative.

---

## 5. Workspace layout

```
repo/
  Cargo.toml  Cargo.lock  rust-toolchain.toml  deny.toml
  AGENTS.md             # this file (+ product-specific notes appended)
  ROADMAP.md            # one page: stage, headline numbers, progress log
  ASSETS.md  THIRD_PARTY.md  LICENSE-*  NOTICE  SECURITY.md
  clippy.toml  rustfmt.toml
  assets/               # fonts, icons, app icon, only attributed files
  crates/
    geom/               # L0  math, units, transforms, hit testing, tolerances
    <domain>/           # L1  the document model (pure data + serde)
    ...                 # L2  algorithms (layout, geometry kernels, solvers, text)
    render/             # L3  document -> pixels / GPU scene description
    io-*/               # L3  every import/export, one crate per format family
    format/             # L3  the native file format (versioned)
    tools/              # L4  tool state machines (pointer events -> commands)
    engine/             # L5  Session, command registry, history, recovery, scripts
    ui-egui/            # L6  the whole egui frontend
    mcp/                # L6  MCP server
  apps/
    <name>/             # L7  desktop binary (eframe)
    <name>-cli/         # L7  headless CLI
    <name>-web/         # L7  WASM entry (trunk)
  fuzz/                 # cargo-fuzz targets, one per reader
  xtask/                # ci, layers, wasm, assets, deny, parity, corpus, fuzz
  docs/                 # architecture.md, control-protocol.md, gaps.md, roadmap.md, ui-notes.md
```

Rules of thumb:

- A crate exists when it has one reason to change. Keep files under about 3,000 lines; split by area.
- Each crate: `#![forbid(unsafe_code)]`, the deny list above, `[lints] workspace = true`.
- **Layers** (an example; rename to fit):

| Layer | Holds | May depend on |
|---|---|---|
| L0 | `geom`, `color`, `units` | std + math crates |
| L1 | document model, fonts | L0 |
| L2 | algorithms (kernels, text/layout, solvers) | L0–L1 |
| L3 | render, io-*, native format | L0–L2 |
| L4 | tools | L0–L3 |
| L5 | engine (Session, commands, history) | L0–L4 |
| L6 | ui-egui, mcp | L0–L5 |
| L7 | apps | everything |

---

## 6. The core architecture

### Document model

- Pure data + `serde`. No UI types, no I/O.
- Large parts are held in `Arc` so an edit clones only what it touches and **an undo snapshot is O(1)** (copy-on-write). Local undo is a stack of snapshots, not inverse operations.
- **Every journaled edit also yields an op record** `{command, params, base_revision, result_revision}`. The journal, scripts, and any co-editing or sync protocol use op records; undo uses snapshots. Do not design co-editing around snapshots: they cannot be merged.
- Stable ids (u64 newtypes) for everything referenced across edits. Never store indices that edits can invalidate. Ids that travel between machines (co-editing, sync) carry a client/origin part so two clients never mint the same id.
- Units: pick one internal unit and coordinate convention and document it at the top of the model crate (for example metres, Z up; or points, y down). Convert only at the edges (UI fields, import/export).
- Each layered format change bumps a `version`; the loader refuses files from a newer version with a clear message, and migrates older ones. Every migration has a fixture from the old version and a test.
- A native format is a zip with `document.json`, `assets/`, `meta.json`, written through an atomic-write helper (stage in the same directory, fsync, rename; fsync the directory on Unix; on macOS use `F_FULLFSYNC`; on Windows the rename replaces in place and there is no directory fsync). Keep the previous file as `name.bak` until the new one is verified readable.
- Reading any zip (native or imported): cap entry count, per-entry and total uncompressed size, and compression ratio (zip bombs); reject entry names with `..`, absolute paths, drive letters or backslashes before using them anywhere (zip slip).

### Commands

```rust
pub struct CommandSpec {
    pub id: &'static str,               // "guide.add"
    pub label: &'static str,            // translation key for "New Guide"
    pub menu: &'static [&'static str],  // ["View", "Grids & Guides"]; empty = not in menus
    pub shortcut: Option<&'static str>,
    pub params: &'static str,           // human/agent-readable params doc incl. return shape
    pub enabled: fn(&Session) -> Result<(), String>,   // Err(reason) = disabled, reason shown
    pub run: fn(&mut Session, &serde_json::Value) -> Result<serde_json::Value>,
    pub journal: bool,                  // recorded for script replay
    pub undoable: bool,                 // pushes an undo step if the document changed
    pub touches_files: bool,            // reads or writes paths: subject to confinement (section 9)
}
```

- Declare with a `cmd!` macro; variants for read-only queries (`journal: false, undoable: false`) and non-undoable actions.
- One module per area exposes `pub fn specs() -> Vec<CommandSpec>`; a `command_specs()` aggregates them in a `OnceLock`. A test asserts ids are unique.
- Params and results are JSON `Value`s, validated strictly (type, range, finite, size caps, unknown fields rejected) with a clear error naming the command and field.
- Mutation goes through one helper (`session.edit(|doc, ctx| …)`) that snapshots, runs, and records the undo step only if something changed; on error or caught panic it restores the snapshot.
- Commands list their params in the `params` doc string. The control channel's `engine.commands` returns the registry, which is how agents learn the app. Keep those strings accurate; a test parses every `params` string.
- There is no command that runs a shell, loads native code, or fetches a URL chosen by a document or script.

### Tools and gestures

Tools are state machines in their own crate. **They never mutate the document.** Input: pointer events (`Down | Drag | Up | Move | DoubleClick`, canvas coordinates, modifiers, click count) and keys. Output: `Action`s.

```rust
pub enum Action {
    Begin(String),                    // snapshot the document
    Preview(String, serde_json::Value), // re-apply one command on top of the snapshot
    Commit,                           // last preview becomes ONE undo step
    Cancel,
    Exec(String, serde_json::Value),  // run a command now
    Dialog(String, serde_json::Value),
    SwitchTool(String),
    View(serde_json::Value),          // ask the UI to pan/zoom
}
```

A drag is `Begin`, then many `Preview`, then `Commit`: one undo step, one op record, replayable, testable without a UI. Snap settings are a plain struct (`SnapView`) with one bool per category and a pixel zone (default 4); each category is an independent switch.

### Session

`Session` owns the open documents, history, selection, caches, preferences, the command registry, recovery state, and a scripting facility. It is the only thing the UI, CLI and agent servers talk to.

- **Recovery:** every N seconds (30), write dirty documents to a per-instance locked recovery folder (created `0700` on Unix); on start offer to reopen them. On orderly exit, preserve any remaining dirty documents. Report when recovery is unavailable instead of silently skipping it. **Web:** the same snapshots go to browser storage (OPFS, else IndexedDB) through `Services`; quota errors are reported, never swallowed.
- **Compose/derived caches** key on revision counters of their inputs. Invalidate by changing the key, never by manual clearing.
- **Scripts** are hostile input: they call commands through `Session::execute` only, under the same validation and file confinement as agents, with an instruction/time budget so an infinite loop cannot hang the app.

### Rendering

- Rendering is a pure function of `(document, view, options)`. The render crate has no UI dependency, so it also powers headless export and test screenshots.
- **2D apps:** rasterise on a background thread (`vello_cpu`), upload the result as one egui texture; the GPU only presents. The worker keeps only the newest job and calls `ctx.request_repaint()` when done; the UI shows the previous texture shifted/scaled meanwhile. Render with ~25 % overscan so panning only moves the texture. After an edit, repaint **only damaged regions** and patch them into the texture (must be pixel-identical to a full render; test that).
- **3D apps:** do the 3D draw on the GPU with an `egui_wgpu` paint callback inside the viewport rect; keep model, picking, snapping and hit-testing on the CPU in the lower layers so they stay wasm-compatible and testable. Use one GPU mesh per shared definition with instance transforms; render on demand, not every frame.
- **Overlays (selection, handles, guides, snap markers, measurements, previews) are drawn as crisp vectors by the UI over the rendered view**, not baked into the raster. They stay sharp at any zoom and are cheap to update.
- Cap texture sizes at the GPU limit (`max_texture_side`) and sample down rather than failing.

### Files

One crate per format family, each with `read(&[u8], &Limits) -> Result<(Doc, Vec<Warning>)>` / `write(&Doc, &Options) -> Result<Vec<u8>>`. Every reader enforces size, count and depth limits and reports what was dropped (a list of warnings returned to the user, never silently). Round-trip tests for every format that has a writer. **Every reader has a fuzz target** (section 9). Prefer fixtures made for the test, not user files.

---

## 7. The UI, in pure Rust

### App skeleton

```rust
struct App { app: UiApp /* wraps Session + UiState */, startup: Option<gpu::Startup>, frames: u32 }

impl eframe::App for App {
    // Shape for orientation only: match the trait of the pinned eframe version exactly.
    fn logic(&mut self, ctx: &egui::Context, _f: &mut eframe::Frame) { self.app.logic(ctx); }
    fn raw_input_hook(&mut self, _c: &egui::Context, raw: &mut egui::RawInput) { self.app.raw_input_hook(raw); }
    fn ui(&mut self, ui: &mut egui::Ui, _f: &mut eframe::Frame) { self.app.ui(ui); }
    fn on_exit(&mut self) { /* preserve dirty docs, save prefs */ }
}
```

- `UiApp::run(id, params)` is the single entry: UI commands are handled in the UI crate, everything else is forwarded to `Session::execute`.
- `UiState` is serde (camelCase, `#[serde(default)]` everywhere so old prefs files load) and persisted as JSON in the platform config dir (`ui.json`, `prefs.json`). Env vars `APP_CONFIG_DIR` and `APP_NO_PREFS` isolate tests. A prefs file that fails to parse is kept as `.bad` and replaced by defaults, with a message.

### Platform services are injected

The UI crate never calls `rfd`, `std::fs` or the clipboard directly. The host passes a `Services` struct of boxed closures:

```rust
#[derive(Default)]
pub struct Services {
    pub pick_open: Option<Box<dyn FnMut(&str) -> Option<String>>>,   // purpose -> path
    pub pick_save: Option<Box<dyn FnMut(&str) -> Option<String>>>,   // suggested name -> path
    pub read:  Option<Box<dyn Fn(&str) -> Result<Vec<u8>, String>>>,
    pub write: Option<Box<dyn FnMut(&str, &[u8]) -> Result<(), String>>>,
    pub open_async: Option<Box<dyn FnMut(ImportRequest)>>,           // web: result arrives later
    pub inbox: Option<Arc<Mutex<Vec<ImportedFile>>>>,                // drained every frame
    pub download: Option<Box<dyn FnMut(&str, &[u8])>>,               // web Save/Export
    pub clipboard_text: Option<Box<dyn FnMut() -> Option<String>>>,
    pub recovery: Option<Box<dyn FnMut(RecoveryOp) -> Result<Option<Vec<u8>>, String>>>, // desktop folder / web OPFS
}
```

Desktop fills the path-based ones; web fills `open_async`, `inbox`, `download` and `recovery`. Capture the operation context (purpose, target document) **before** any async picker so a late result applies to the right thing. Size-check every inbox item before reading it.

### Window anatomy

Build the layout the reference images show. The default for a professional tool is:

1. **App bar** (about 36 px + a thin lower band): home button, menu bar, workspace switcher, search/command palette.
2. **Contextual control bar** under it: shows fields relevant to the current selection and tool.
3. **Tools strip** on the left: icon-only buttons in groups, with flyouts for tool variants, an active-tool well, shortcuts in tooltips.
4. **Document tabs**, then the **canvas** with rulers or axes, scrollbars, and overlays.
5. **Dock** on the right: an icon rail plus tab groups of panels; panels can be re-ordered, split, floated, closed and reopened; layouts are saved as named **workspaces**.
6. **Status bar** at the bottom: tool hint (what the tool wants next), zoom, messages, warnings/errors indicator, and a numeric input where relevant.
7. Optional movable **contextual task bar** near the selection.

Use `egui::Panel` / `SidePanel` / `TopBottomPanel` (whichever the pinned egui provides) for the regions and `ScrollArea` for panel bodies. Let the window be narrow without breaking: make bars scroll horizontally rather than overlap.

### Panels, dialogs, menus

- One file per panel. Panel IDs are stable strings, independent of theme, because workspaces and agents refer to them.
- Menus are a data table (`MENUS`) built from the command registry's `menu` paths plus UI commands. Disabled items show the `enabled` reason.
- Dialogs: write the important ones by hand; **generate the long tail from the command `params` doc**, so every command is reachable from the UI.
- Dockable panels: if you can, use a transactional docking model: every move (tab, split, float, resize, close) is a validated action on a layout value; invalid input returns an error and changes nothing; the layout serialises into the workspace. Also expose it as commands (`window.panel.move/float/dock/activate/close`).
- Modal dialogs are draggable and restore focus; keyboard: Enter confirms, Esc cancels.

### Custom widgets (do not ship stock egui look)

Write a small `widgets` module and measure it from the references. Typical values for a dense pro tool: field height 21, button height 20, label size 11, 1 px borders, 1.5 px section rules, icon toggles in a darker "well".

- **Numeric field:** drag-to-scrub, optional spinner and preset chevron segments, min/max, step, mixed-value state, and **accepts units and arithmetic** (`12+3`, `50%*2`, `3m`, `10'`, `1p6`, `3in+2p`). Parse through a shared `parse_measure` (input length capped, no recursion deeper than a small fixed depth, results checked finite); format through one `format_measure`. Invalid text reverts and flashes the field; it never panics. `parse_measure` has a proptest and a fuzz target.
- **Colour field/picker, dropdown with search, tab strip, section header, link label, toggle, segmented control, tooltip with shortcut.**
- Hit areas at least 18–20 px even for small icons; 40 px+ on touch screens.
- Everything keyboard reachable; set accessibility labels (egui accesskit) on icon buttons where it is free. Full screen-reader support is a **known issue, skipped for now** (owner, 2026-10-15): list it in `docs/gaps.md`; don't spend time on it.

### Icons

Draw them in code or compile in your own SVGs; never download a pack.

- **Painter icons:** a `Pen { painter, rect, color, width }` helper with `line`, `closed`, `fill`, `rect`, `circle`, `arc` working on a fixed **20×20 design grid** scaled to the target rect. Name → draw function in a `match`. Stroke width scales with the icon.
- **SVG icons:** keep them monochrome with `currentColor`; prepare them once, cache by URI (distinct URI prefix per catalog/style), render with `egui_extras` image loaders, tint from the theme. Only compiled-in SVGs go through the SVG loader; never render an SVG from a document or the network with it.
- Provide an app icon as SVG + PNG sizes + `.ico` + `.icns`; embed the `.ico` and version info on Windows with `winresource`.

### Theme

One flat `Tokens` struct of `Color32` roles; widgets read `Tokens::get(ctx)`. Roles to define at minimum:

`app_bar, panel, panel_darker, input, input_border, divider, border, text, text_strong, text_dim, text_disabled, icon, hover, tool_active, well, accent, accent_strong, row_selected, canvas_bg, ruler, ruler_tick, tab_inactive, tab_strip, button, field_border, smart_guide, measure_bg, measure_text, focus_ring, radius`.

- Ship presets: **Dark, Medium Dark, Medium Light, Light, High Contrast** (black chrome, white text, yellow selection and focus). `Tokens::for_brightness(b)` returns them; start from the product's own brand values (or the measured *feel* of the references, re-picked as your own values) in one preset and derive the others. A test checks text/background contrast of every preset (WCAG 4.5:1 for body text, 3:1 for icons and borders).
- Apply once per theme change: fill egui `Visuals` from tokens; set `bg_fill`, `weak_bg_fill`, `corner_radius` and `fg_stroke` for the five widget states (noninteractive, inactive, hovered, active, open); set text styles and fonts (`ctx.set_fonts`, with fallback fonts for scripts you support); UI scaling from a preference.
- Follow the system light/dark setting when asked to.

### Localisation and text direction

Strings go through `tr(&lang, "key")` against JSON catalogs (`lang/<locale>.json`, English as the fallback catalog); a test fails on a key used in code but missing from English, and lists keys missing per locale. Translations are original work. Support right-to-left layouts (mirror panels, use bidi shaping) if Arabic/Hebrew are in scope. Do not concatenate translated fragments; use format strings with named fills. Test IME (CJK) input through the control channel on each OS before claiming a language.

### The canvas widget

- Own a view transform (`pan`, `zoom`, `rotation`); one `to_screen`/`to_canvas` pair used by everything. Zoom about the pointer. Pan with Space-drag, middle drag, and scroll; wheel zoom with a trackpad/mouse setting. Clamp zoom to a range where `to_screen` stays finite.
- Convert egui input to the tool's `PointerEvent`s here, once. Clicks: count presses within a time/distance threshold.
- Draw order: rendered texture or GPU callback, then overlays, then HUD (measure labels, tips).
- Cursors per tool and per hover target.

---

## 8. Design language (defaults; override from the user's images or the product's brand)

Unless the references or the product's brand say otherwise, build a **dense, flat, dark, professional-tool** interface:

- **Colour:** neutral grey chrome (dark presets in the `#1e1e1e`–`#4a4a4a` range, canvas darker than panels), white or near-white text, one accent (the product's brand colour; pick your own if there is none, never another vendor's design-system value), selection rows a darkened accent, smart guides a clear green, measurement labels dark translucent with white text. Colour-code axes red / green / blue (X / Y / Z) in 3D apps.
- **Shape:** nearly square. Radius 1–4 px. 1 px borders. **No shadows**; hierarchy comes from fill steps and line weight.
- **Density:** 11 px labels, 20–21 px controls, tight padding, tabular numerals in numeric fields.
- **Type:** one humanist sans for UI (Inter or Source Sans 3), a monospace (JetBrains Mono) for code/values, document fonts separate from UI fonts.
- **Icons:** thin monochrome line icons on the 20 px grid; the active tool sits in a darker well; the fill/stroke or colour proxy follows the tool-panel convention of the field.
- **Feedback:** every tool shows what it wants next in the status bar; modifier keys change the tip live; snapping shows a marker plus a text label; destructive things are undoable rather than confirmed (except actions outside the document: deleting files, overwriting a file on disk, sending anything over the network).
- **Motion:** none, or under 100 ms. No animations that delay input.
- **Start screen:** recent files, new-document presets, sample document, links to help.
- **Accessibility:** contrast presets, visible focus rings, UI scale 75–200 %, no information by colour alone.

If the user gives images: extract the palette (sample pixels), the grid of regions (measure them as % of the window), control sizes, icon stroke weight, font size ratios. Put them in `docs/ui-notes.md`. Build one screen at a time and compare screenshots side by side with the reference until the layout and density match; colours follow the brand rule above.

---

## 9. Agent control, headless use, testing, security

Build these from day one; they are how you (and users' agents) verify the app.

### Control channel (desktop only)

`app --control <port>` (or `APP_CONTROL_PORT`) starts a **loopback-only** JSON-lines server: one JSON object per line in, one per line out: `{"id":1,"method":"…","params":{…}}` to `{"id":1,"ok":true,"result":…}` or `{"id":1,"ok":false,"error":"…"}`.

Security (every rule is required; the channel can read the screen and write files as the user):

- Bind `127.0.0.1` only. Off unless started for this run.
- **Token:** at start, make 32 random bytes (OS RNG), base64url; print it to stderr and write it to `control.token` in the config dir (`0600` on Unix; per-user ACL on Windows). The first line of every connection must be `{"method":"auth","params":{"token":"…"}}`; compare in constant time; a wrong or missing token gets one error reply and the connection closes. Never put the token in a URL, a log line or a screenshot.
- A line that is not a JSON object with a string `method` gets one error reply and **closes the connection** (stops HTTP/cross-origin smuggling: an HTTP request line is not JSON).
- Line cap 4 MiB; at most 16 connections; idle timeout; cap queued synthetic input events.
- **File confinement applies here too:** every path in `app.open/save/export`, `ui.screenshot`, `ui.render` and any `touches_files` command must lie inside the read/write roots (default: the app's documents folder and a per-run temp folder; widened only by `--automation-read-root` / `--automation-write-root`), checked after canonicalising (no `..`, no symlink or junction escapes; on Windows also reject `\\?\`, UNC and device paths, and alternate data streams).

Methods to provide:

| Method | Purpose |
|---|---|
| `engine.execute {command, params}` | Run any command |
| `engine.commands` | The registry: id, label, menu, shortcut, params doc, enabled and why not |
| `document.inspect` | Structured state of the open document |
| `ui.inspect`, `ui.menu.list`, `ui.menu.invoke`, `ui.tool.list`, `ui.tool.select` | UI state and navigation |
| `ui.pointer {events, mods}` | Drive the active tool through the real tool code |
| `ui.key`, `ui.text` | Keyboard and typing |
| `ui.move`, `ui.click`, `ui.drag` | Real egui pointer input that reaches any widget (validate the whole gesture before queuing) |
| `ui.set` | Theme, panels, zoom, view flags |
| `ui.dialog.open/set/confirm/cancel` | Dialogs |
| `ui.screenshot {path}` | PNG of the whole window |
| `ui.render {path, page, scale}` | Headless render of the document |
| `app.open/save/export/quit`, `window.panel.*` | Files and layout |

### CLI

`app-cli run --sample --cmd 'id={json}' --export out.png`, `script`, `commands`, `describe`, `mcp`. It runs the same engine with no window. `--sample` opens a built-in sample document generated in code (a good demo and test fixture).

### MCP server

A stdio JSON-RPC 2.0 server (`app-cli mcp`) wrapping the same methods as tools. Support the current MCP protocol version at build time and the two before it (at writing: 2025-11-25, 2025-06-18, 2025-03-26); answer `initialize` with the newest one the client offers. Headless in-process, or `--connect <port>` to a running app (then it needs the control token, from `--token-file`). Return images (PNG) so an agent can look at results. **Confinement is on by default** (read and write roots = the current directory); `--automation-read-root DIR` and `--automation-write-root DIR` change them; every file a command reads or writes must lie inside, after canonicalising (same path rules as the control channel). Tool descriptions come from the command registry; they never include document content.

### Security checklist (CI and review)

- `SECURITY.md` says how to report a vulnerability.
- Fuzzing: one `cargo-fuzz` target per reader, plus `parse_measure`, the control-line parser, MCP message parsing and script parsing. CI runs each for a short time on every push (the target must not crash on its corpus) and longer nightly. Every crash becomes a regression test and a corpus entry.
- `cargo deny check` in CI: licence allowlist, RustSec advisories (an advisory fails the build until fixed or explicitly waived in `deny.toml` with a reason and date), banned crates, allowed sources.
- Logs never contain document contents, file contents, tokens or credentials; paths only at debug level.
- No telemetry, crash upload or update check without an explicit setting the user turned on.
- Updates (when there is an updater) are signed (ed25519 / minisign) and verified against a key compiled into the app before anything is written; never download and run code otherwise.
- Add-ons/plugins, if the product has them, are scripts or data through the command API; never load native libraries (`dlopen`/`LoadLibrary`) from user-writable locations.

### Tests

- Unit tests next to the code; `proptest` for parsers, geometry and anything that eats input.
- **Golden tests** for kernels: port or write fixtures first, then the code.
- **Command tests** drive `Session::execute` and assert document state and undo/redo round-trips, and that replaying the op records from an empty document gives the same document.
- **Tool tests** feed `PointerEvent`s and assert the emitted `Action`s.
- **UI tests** with `egui_kittest`: render the full window offscreen with wgpu and snapshot-compare. A headless `ui_shot` example reads a jsonl script of control requests and writes PNGs, even with a locked screen. Snapshots have a small tolerance; GPU-dependent differences are documented, not ignored.
- **Format tests:** round-trip, plus black-box comparison against an independent reader when one exists (never ship or link it).
- **Crash-guard tests:** a test command that panics leaves the document equal to its pre-command snapshot; a forced web panic is recovered after reload.
- Platform input differs (copy/paste, context menus, IME, shortcuts that the OS menu bar steals). Regression-test them through the control channel on every OS you ship.

---

## 10. Platform, packaging, resilience

- **GPU start-up fallback:** choose the wgpu backend before the window exists from a list; record each attempt in `gpu.json`; if a backend never presented a frame, skip it next start and say so in the status bar. `WGPU_BACKEND` overrides. (Some hybrid-graphics drivers fault at the first present.)
- **Quit and close:** the OS quit path must not bypass unsaved-document prompts. Route close and Quit through one confirm transaction; cancel it from the native event loop where the platform allows (macOS needs a native termination hook). On forced exit, write recovery state.
- **macOS:** native menu bar (`muda`) mapped onto the same command table; the app bar skips its own menus there. **Windows:** `#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]`. **Linux:** x11 and wayland features.
- **Web:** same UI crate through eframe's web runner. No control socket. Open/Place use the browser picker through `inbox`; Save/Export download; dropped files are read async and size-checked first. URL flags such as `?sample` (URL flags never take paths, URLs or script text). Ship a strict Content-Security-Policy (`script-src 'self' 'wasm-unsafe-eval'`, no inline script). Build with `trunk build --release`.
- **Web performance (decided: speed is the deciding factor).**
  - **WebGPU only, no WebGL2 fallback.** A WebGL2 fallback would force the renderer down to WebGL2's limits: no compute shaders, no storage buffers, no indirect draws. Design for WebGPU's capabilities instead: compute culling and picking, storage buffers, `draw_indexed_indirect`, render bundles for static geometry, instancing per shared definition, and timestamp queries when available.
  - A browser without WebGPU gets one clear screen ("this preview needs WebGPU: use Chrome or Edge, Safari 26+, Firefox 141+ on Windows / 145+ on Apple-silicon Macs, or CadCraft Studio"), never a broken canvas.
  - **Threads and SIMD:** build with wasm threads (`+atomics,+bulk-memory`, `wasm-bindgen-rayon`), `+simd128`, release `opt-level = 3`, `lto = "fat"` for the web crate, then `wasm-opt -O3`. The host sends COOP/COEP (`same-origin` / `require-corp`) so threads work; the build still runs single-threaded if the headers are missing, and says so in the status bar.
  - **Delivery:** brotli-compressed `.wasm`, streaming compilation (`WebAssembly.instantiateStreaming`), long-cache hashed file names, heavy engines loaded on first use.
  - **Measure, do not assume:** `cargo xtask bench` (native) and a `?bench` page (web) time file open, first frame, and frame time while orbiting on the showcase files and a 1M-triangle model. Numbers go into `ROADMAP.md`; a change that slows a benchmark by more than 5 % needs a reason in its commit.
- **Hosting the test version** (owner, 2026-10-15): the web build is published at **`https://rust.getcadcraft.com`**, public but unlisted, by invitation: no password; `noindex` (meta + `X-Robots-Tag`) and `robots.txt` disallow all; not linked from getcadcraft.com, its sitemap or `llms.txt`. Deploys to that subdomain are allowed once it exists; announcing it or linking it anywhere is the owner's call.
- **Mobile/tablet:** eframe's Android and iOS support is limited. If the product ships on tablets, say in `docs/roadmap.md` what the plan is (touch layout, file pickers, lifecycle save on background) instead of letting it fall out silently.
- **Packaging:** `packaging/` with MSI (WiX), deb, rpm, macOS `.app`/dmg (signed + notarised), AppImage, Flatpak as needed; the CLI ships beside the app. Releases embed the optional font set. Never commit signing material, tokens or `.env` files; signing happens in CI or on a release machine from secrets stored outside the repo. Builds are reproducible from the tag (`--locked`, pinned toolchain, `SOURCE_DATE_EPOCH`).
- **Settings and logs:** platform config dir via a small settings module (portable mode, atomic saves, backup of a damaged file, schema migrations). Log with `log`; a log file in the config dir, rotated and size-capped; panics logged with a backtrace before the guard converts them.

---

## 11. Quality gate and workflow

`cargo xtask ci` must pass before every commit:

1. `cargo fmt --check` (rustfmt: `max_width = 140` or whatever `rustfmt.toml` sets, consistently)
2. `cargo clippy --workspace --all-targets --locked -- -D warnings`
3. `cargo test --workspace --locked`
4. `cargo xtask assets` (every asset has an `ASSETS.md` row)
5. `cargo xtask layers` (dependency layering and "no UI toolkit below L6")
6. `cargo xtask wasm` (`cargo check --target wasm32-unknown-unknown` on layers L0–L6)
7. `cargo deny check`
8. `cargo xtask fuzz --smoke` (each fuzz target over its corpus plus 30 s; nightly CI runs longer; needs nightly toolchain, so it may run in CI only)

Workflow:

- **One task id per commit:** `M2.1: caret navigation in the text tool`. Do not commit unrelated edits together. Do not commit build output.
- **Docs are part of the work.** When a feature lands, update the affected doc and its status line, shrink or delete the closed item in `docs/gaps.md`, add one line to `ROADMAP.md`'s progress log. Each doc carries `Last reviewed / Last updated / Change` and a short revision table. Describe what the code does today, with measured numbers.
- **Parallel agents:** separate `CARGO_TARGET_DIR` each, edit only the crates you own, delete your target dir when done.
- **Performance:** measure before optimising (frame time, compose time, render time); budget typing and dragging at 60 fps on the sample document.
- **Dependencies on other repos:** pin by git `rev`, never a branch; publish the dependency's commit before the consumer.
- **Do not stop to ask** about things the plan covers. Ask only for product decisions, licences, names, spending, publishing or releasing, or anything destructive.

---

## 12. If the app is a 3D modeller / CAD tool

Everything above applies. Additionally:

- **Kernel in the lower layers, f64 throughout**, with one documented tolerance constant and deterministic iteration (`BTreeMap` or insertion-ordered maps); ids stable across edits. Port or write kernel tests (golden meshes, boolean results, face/loop topology, undo round-trips) before the UI.
- **Large coordinates:** the GPU works in f32. Never upload world coordinates directly: render relative to the camera (subtract a per-frame f64 origin on the CPU, or split high/low parts), so survey and map coordinates (e.g. x = 500 000 m, y = 4 000 000 m) stay steady to 1 mm. A test fixture at those coordinates must pick, snap and render without jitter.
- **Topology:** keep a boundary-representation style model (vertices, edges, faces with outer loop and hole loops, instances of shared definitions with 4×4 matrices). Groups and components are definitions plus instances. Operations that edit geometry (push/pull, offset, follow-me, move, rotate, scale, boolean) are commands operating on the model, with a preview variant for gestures.
- **Viewport:** wgpu callback, camera in your own code (so picking and drawing share matrices), ray picking with a BVH, render on demand. Provide orbit/pan/zoom on middle / right / wheel with any tool, a view cube, standard views, perspective and parallel projection, section planes, shaded / hidden-line / wireframe / x-ray, edge rendering, sun and shadows, materials and textures. Reversed-Z depth for large scenes.
- **Inference engine** (snap with ranked priority): points (endpoint, midpoint, face centre, origin, guide points) within about 12 px; then directions from the start point (axes, parallel and perpendicular to the last hovered edge); then on-edge, on-guide; then on-face; then the working plane or ground. Colour-coded markers plus a text tip. Arrow keys lock an axis, Shift locks the current inference; locks work in the group's own axes while editing inside it; hidden geometry never snaps.
- **Typed input** ("value box") during every tool: lengths with units, `[x,y,z]` absolute points, `<dx,dy,dz>` offsets, counts (`24s`), factors (`2`, `/3`, `3x`). It is the numeric field from section 7 in the status bar. Counts and factors are capped (an array of `1000000x` is refused with a message, not attempted).
- **I/O:** native JSON/zip, then OBJ, glTF/GLB, STL, 3MF, Collada, DXF/DWG, IFC, SKP, as needed. Readers for proprietary formats are written clean-room from public specs. Every reader enforces vertex/face/size limits and reports dropped features. Printer-readiness checks (watertight, manifold, flipped, self-intersecting) before STL/3MF export.
- **Large models:** target interactive speed at 1M triangles: shared meshes per definition, BVH, scoped edits, lazy/hidden geometry retention.
- **Booleans:** prefer a pure-Rust implementation; if you must use a C++ kernel, isolate it behind a `trait SolidOps` so the rest stays pure Rust and wasm-safe, and fuzz the boundary (degenerate, non-manifold and huge inputs) because a C++ crash there is not catchable.

## 13. If the app is a 2D page/vector/photo tool

- Document-to-pixels with `vello_cpu` on a worker thread; text through `skrifa` + `harfrust`; your own line breaker if typography matters (identical breaks on screen and in PDF).
- Colour management in its own crate (ICC v2/v4, CMYK/RGB/Gray, soft proof). Export through a guarded PDF writer; embed font subsets; keep real text.
- Fonts embedded in documents are hostile input: parse with `skrifa` only, cap glyph counts and table sizes.
- Overlays for guides, grids, handles and text threads are painter vectors above the raster.

---

## 14. Definition of done for any feature

- A command (or several) exists with doc string, `enabled`, tests, and undo works.
- It runs headless (`app-cli`) and through the control channel.
- The UI exposes it (menu, shortcut, panel or tool) using only tokens and shared widgets.
- A screenshot has been taken and checked against the reference.
- `cargo xtask ci` passes; docs and `ROADMAP.md` updated; one commit with a task id.
- No new `unwrap`/`panic`/`unsafe`/unchecked indexing; hostile-input paths are bounded and fuzzed; a regression test exists for every crash found.

---

## 15. Project rules: the CadCraft Rust fork

This section is what makes the generic file above the CadCraft fork's file. It wins over sections 1–14.

- **What it is.** A fork of CadCraft (the eight apps — CadCraft, PartCraft, BimCraft, CivilCraft, ModelCraft, DetailCraft, TakeoffCraft, MarkupCraft — and CadCraft Studio) in Rust. The reference app is the CadCraft 0.5 JavaScript build in `getcadcraft.com` (`public_html/assets/apps/<app>/dev/`, docs in `docs/apps/`, the fork map at `/roadmap-issues/fork-map`). It is ours, so it may be read as source; the clean-room rules apply to everyone else's products. CadCraft's own clean-room engines (cc-dwg, cc-ifc, cc-skp, m3d) port with their spec-gap notes.
- **Parity is the gate.** A ported engine or format counts as done only when it matches the JS build on the corpus (`/var/www/CADCRAFT/testfiles`, fetched by `cargo xtask corpus`, never committed) and the existing oracles. Port the JS tests and golden fixtures first. Report parity numbers, not impressions.
- **Same files, same words, same agents.** Read and write the file formats the 0.5 apps save, byte-compatible where the JS writer is deterministic. Use the 0.5 `lang/*.json` catalogs and keys unchanged (en and es-MX at least; es-MX ships as draft) and the Markdown help. MCP tool names and shapes match 0.5's `host.js` / `docs/AGENTS-SPEC.md` so an assistant set up for 0.5 works unchanged.
- **Control channel = Studio's rules.** Match `desktop-shell/src-tauri/src/control.rs`: 127.0.0.1 only, bearer token (32 random bytes, base64url, constant-time compare, never in a URL), Host check, any `Origin` → refused, JSON only, 4 MB cap. Section 9's JSON-lines channel is acceptable only with its token; if the fork exposes MCP over HTTP it follows control.rs exactly.
- **Studio Live.** Co-editing speaks `docs/LIVE-PROTOCOL.md` version 1 and passes `tools/live-test/vectors.json`. Edits travel as op records (section 6), never snapshots.
- **Booleans:** the 0.5 apps use Manifold (Apache-2.0, C++). Until a pure-Rust boolean passes parity on the corpus, Manifold through a Rust binding behind `trait SolidOps` is the approved exception to "pure Rust". OpenCascade, web-ifc, LibreDWG and any GPL/LGPL/MPL code stay out (oracles only, run as programs).
- **Licence:** Apache-2.0 for our code; every dependency on the section 3 allowlist. No server code, secrets, `.env` values or site-only code in the fork's repo.
- **Identity:** commits and tags are made as `CadCraft <ddgconsultant@getcadcraft.com>` with no assistant co-author trailers or "Generated with" lines anywhere (commits, PRs, READMEs, release notes, app copy). Install the commit-msg hook used by the other CadCraft repos that refuses those names.
- **Repo and test site (owner, 2026-10-15).** The fork lives in its own public repo **`github.com/getcadcraft/cadcraft-rust`** (approved), and its web build goes on **`rust.getcadcraft.com`** (section 10, Hosting). Pushing to that repo and deploying to that subdomain are fine.
- **Everything else waits for the owner:** releases, moving any `current` symlink on getcadcraft.com, links from the main site, Discord posts, store uploads, signing with release keys. Report "ready" and ask.
- **SketchUp files:** port our own cc-skp (`lib/skp/`), not OpenSKP. Measured 2026-10-15: OpenSKP's C++ parser was 0.5–2.7× the speed of cc-skp's JavaScript (slower on the big files), so it brings no speed. OpenSKP stays a black-box oracle and the speed baseline to beat.
- **Copy.** Do not market the fork as "native" or "no browser engine" on the site, in the apps or in posts. Call it a test version of CadCraft; describe it factually in the repo README.
- **Tablets:** 0.5 Studio runs on Android and iPad. The fork's roadmap must say whether and how it covers them (section 10, Mobile).
- **Large coordinates** (section 12) are a requirement, not an option: CivilCraft and the GIS layers work in projected survey coordinates.
