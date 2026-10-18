# Session log (fork-rust)

Newest first. One entry per working session: what was done, numbers, what is next.

## 2026-10-18 — M9.14 MarkupCraft's tools and Properties panel as 0.5 has them (agent, work/markuptools)

- M9.14: `crates/ui-egui/src/markuptools.rs` rewritten as a port of markupcraft.js's pointer handlers (onDown /
  pointermove / pointerup / dblclick and `KIND`): all 0.5 drawing tools in toolbar order — pen, highlight, line, arrow,
  arc (three clicks), rectangle, ellipse, polyline, polygon, revision cloud (box) and free cloud (by outline), callout,
  text, note, stamp, calibrate, length, angle (three clicks), area, count (click adds to the selected group, Shift+click
  takes one away). Two-point tools take a drag or two clicks; Shift snaps to 45° and squares boxes; outlines end with a
  double-click or Enter. A finished gesture is `newMarkup` → `commitNew`: the tool's style (default plus the panel's
  choices), author, the page's rotation, the current layer, the type's subject — one undo step with the 0.5 label.
  Text, callout and note open an editor over the sheet (`startEdit` / `commitEdit`; the box grows as the text wraps;
  nothing typed = no text markup). Stamps sized as `stampBox`. Measuring tools on an uncalibrated sheet go to
  Calibrate first, then back. Select: pick, Shift-pick, move (drag), grips (resize / move a vertex), window and crossing
  selection, Delete (`deleteSelection`).
- `markupprops.rs`: the Properties panel (right of the sheet): one markup — author, dates, measured values (length,
  area / perimeter / cut-outs, count, angle), status (logs a status reply), subject, layer (or a new one), stamp, count
  group name, text, comment, style (colour, line width, fill + fill colour + fill opacity, opacity, text size); several
  — status and style for all; a drawing tool and no selection — that tool's style and count group / stamp; nothing —
  the sheet's facts. Each change is the JS handler (same checks, one `Store::change`).
- The sheet view now opens a file as markupcraft.js does (`store.load` with the file's calibrations, sheet names, layers,
  columns; one id sequence with the reader) and saves as `bake` does (a sheet without a scale clears the file's, the
  count badge size per page, typed sheet numbers as labels, the file's own annotations rewritten, layers, columns).
  Commands `markup.down | move | up | dbl | enter | esc | type | commit | style | prop | cal | delete | redo | save |
  clock`, `pdf.zoom`; `cadcraft-shot pdfbench`.
- Parity (`forks/tools/mkuidiff.mjs`: the app driven headless vs markupcraft.js's handlers replayed on the frozen 0.5
  modules, then Save; the marked-up PDFs read back with pdf.js + pdf-lib, annotation dictionaries, appearance streams
  and the /MarkupCraft JSON compared): 4 scenarios (235 gestures and edits: every tool, the editor, stamps, counts,
  calibrate and presets, styles, status / subject / layer / comment / text, select, move, window, delete, undo / redo)
  on 4 PDFs (kitchen sheet, Carey, blank grid, cairo): 16 / 16 identical. tkopsdiff still 12 / 12.
- Timings (`forks/tools/mkbench.mjs`, best of 3, ms, JS = frozen 0.5 in Node with pdf.js 6 + @napi-rs/canvas):

  | file | path | JS | Rust |
  |---|---|---|---|
  | kitchen sheet (1 p) | open / render 1× / 2× / save / takeoff PDF | 16.6 / 83.0 / 150.7 / 69.7 / 17.4 | 0.1 / 28.8 / 80.9 / 2.9 / 8.5 |
  | Carey (1 p) | same | 41.8 / 272.0 / 269.6 / 21.0 / 11.2 | 0.3 / 68.1 / 83.8 / 2.2 / 7.1 |
  | plan-set-200 (200 p, 9.9 MB) | same | 2,512 / 213.7 / 217.2 / 137.7 / 84.6 | 46.5 / 108.0 / 228.3 / 7.2 / 15.8 |
  | dense-sheet-300k (1 p) | same | 837 / 8,844 / 15,002 / 53.1 / 21.4 | 21.5 / 2,197 / 6,881 / 3.5 / 11.9 |

  Rendering at 2× on the 200-sheet set is the one path not faster (0.95×). Big files in testfiles/generated/markup.

## 2026-10-18 — M9.13 TakeoffCraft's panels: assemblies, bid areas, alternates, deducts, slopes (agent, work/markuptools)

- M9.13: `crates/ui-egui/src/takeoffprops.rs` + `pdfcmd.rs`: the current item's settings as 0.5's buildItemProps has
  them (name, type, colour, unit, Also measure (second conditions + their unit), height / depth, back-out and what it
  takes area out of, slope + pitch, wall thickness / opening head / base height, waste, material, labour, labour by crew
  with its sum line, CSI number), its measured values and its assembly (components: name, unit, quantity formula with
  the live result, waste, material, labour, remove), Duplicate / Move up / Move down / Delete. The selected mark (Select
  tool: takeoffcraft.js markAt): item, bid area, length / deducts / drawn / plan length, each run segment with its slope
  (item's, level, rake, hip) and a Deduct box, the mark's pitch, area / back-outs / plan area / slope factor / cut-outs,
  Add a cut-out (the Cut-out tool) and Remove last cut-out, a count's spot elevation, Delete. Bid areas: the picker new
  marks go in (marks in other areas dim), New bid area and Manage bid areas windows (rename, base / add / deduct
  alternate, order, delete, add), the By-area table with the base bid and each alternate's amount. Deduct mode (E) on
  runs. Clicking follows takeoffcraft.js `click` / `finishDraft` (double-click repeat, close by the first corner,
  dedupe keeping deducts on the right segments, the scale / outline / back-out checks). Each edit is the JS handler
  (same parsing, same `Takeoff` call, patch and undo label: one undo step). Commands `takeoff.select | set | comp |
  itemact | tool | deduct | click | mark | area | undo | redo`. The panel's estimate is the base bid (baseEstimate), as
  the JS shows it. Catalog words throughout; plain English only for "Draw" (no item yet) and command errors.
- Parity (`forks/tools/tkopsdiff.mjs`: the app driven headless by cadcraft-shot vs the frozen 0.5 handlers replayed on
  the 0.5 model; three scenarios, 166 edits incl. undo / redo): project, CSV, XLSX and marked PDF identical, 12 / 12.
## 2026-10-18 — M15.1 wasm threads, measured (lead)

- Built the threaded web build in a scratch copy (atomics, build-std on a pinned nightly, wasm-bindgen-rayon, pool jobs for
  DWG opens). It runs, but the readers scale negatively on threads: 1 read 53 ms, 8 at once 1,147 ms (std's global
  allocator lock); the Carey drawing opened slower than single-threaded. Not shipped; recipe and numbers in
  docs/roadmap.md § Wasm threads. Next for web speed: fewer allocations in the readers.

## 2026-10-18 — M13.2 merges and a fuzz fix (lead)

- Merged M14.1/M14.2 (DetailCraft view) and M13.1 (PartCraft view) into main. Opening a part, a DetailCraft model or a
  CivilCraft project now closes the other two views.
- M13.2: the `m3d_import` fuzz target ran out of memory in `cc_io_m3d::site` (the DXF site plan): nested inserts of
  3DFACEs built millions of filled outlines before site.js's 200 000 check, which runs after drawing. The drawing now
  stops as soon as the count is passed: same refusal, bounded memory. Regression test
  `dxf_site_plan_nested_faces_refused_early` (a million faces through six levels of blocks) and the find as a seed.

## 2026-10-18 — M13.1 PartCraft in the app (agent, work/partapp)

- M13.1: `crates/ui-egui/src/partview.rs` + `partpanel.rs`: a PartCraft view. Opens what 0.5 PartCraft opens — its
  `.partcraft.json` (part.js `readPart`) and `.pcscript` (script.js `scriptToPart`); assemblies say "not yet" — and a
  built-in sample (`assets/samples/partcraft-bracket.pcscript`, our own bracket: 6 parameters, fillet, chamfers, a hole
  pattern, a slot; `part.sample`, app bar "Part sample", `cadcraft-shot --part`). Rebuilt through `kernel::regenerate`
  (the solved sketches go back into the part, as the JS solves them in place) and shown in the 3D view, Z up through the
  exports' `(x, −z, y)`: one GPU mesh per named face plus the meshfaces.js edges, orbit / pan / zoom, Fit, Isometric /
  Front / Top / Right, Edges on/off. Hover names the face beside the pointer and in the status line, a click selects it
  (CPU ray against the solid's triangles → `triFace`; partcraft.js `faceWords` ported: "Bolt hole wall (copy 2 of
  Bolts)"); Properties shows partcraft.js `faceProps` (kind, radius, area, made by, face, normal / axis, reference).
  History panel: features in order with partcraft.js's flags (⚠ error, off, ⚠ not built, − / + sketches), the
  rollback bar (click = roll forward), ◀ / ▶ / ▶▶ roll buttons, the selected feature's numbers (params-part.js targets in
  use; ƒ = driven by an equation), Suppress / Unsuppress, Roll back to here; the part's facts (`renderFacts`).
  Parameters tab: variables (expression editable, value evaluated), Add, equations (editable). Every edit is
  partcraft.js `commit` (one undo step, rebuild); parameters follow params-panel.js `edit` / `typed` (a number on a
  driven field solves the parameter behind it, `=expr` links). Save part and the exports partcraft.js has on the mesh
  kernel — STL, 3MF, STEP AP242 / AP214 (faceted), IFC 4 — named and worded as partcraft.js does, through
  `services.save`. Commands `part.sample / save / export (stl|3mf|step|step214|ifc[@ms]) / param / undo / redo / rollto /
  suppress / select / face`. Words from PartCraft's catalog; plain English only for "Part sample", "Edges", the
  assembly refusal and command errors.
- Parity (`forks/tools/pcuidiff.mjs`: the app driven headless by cadcraft-shot vs the frozen 0.5 modules replaying the
  same edits in one kernel instance): save, STL, 3MF, both STEPs and IFC byte-identical and every status line identical
  on the sample, on the sample after 10 edits (parameters, a backward solve, a link, a plain number, suppress, roll back
  and forward), and on the generated 10 × 10 and 15 × 15 perforated plates (up to 7.3 MB STEP).
- Rebuild ms, release, median (JS = kernel.js on manifold-3d 3.5.4 wasm in Node, fresh instance per run as kbench.mjs):
  sample (1,698 triangles) JS 57.6 / Rust 25.0 (2.3×); plate 10 × 10 (26,012) 4,633 / 2,452 (1.9×); 15 × 15 (58,512)
  16,721 / 11,435 (1.5×); 20 × 20 (400 holes) the JS dies (wasm RuntimeError after ~85 s), Rust 24.2 s. Big parts block
  the window while they rebuild (the JS does too).
## 2026-10-18 — M14.1 DetailCraft in the app (agent, work/detailapp)

- M14.1: `crates/ui-egui/src/detail{view,3d,panel,export}.rs`: DetailCraft's view. Opens 0.5's own model file
  (`<name>.detailcraft.json`, `Model.load`: readModel checks it), the sample (sample.js) and, while a DetailCraft model is
  open, an IFC (ifc-import.js; the import report in a window), as detailcraft.js does; after every change `recompute()`
  (build → markParts → markFootings → bendingSchedule) and the 3D scene as view3d.js builds it: each part its profile
  extruded along its frame, copes cut by splitting the length, holes as dark cylinders, bolts and rods, footings as
  see-through boxes with their bars as tubes, grids on every level with bubbles and level names over the picture; parts of
  the same shape share one mesh. Orbit / pan / zoom / fit / Iso-Top-Front-Right, click picks a part (its member,
  connection or stair) or a footing. Panels with the catalog's words: the model, phases (show / hide), part marks, members
  by type, footings; Properties of a member (name, profile from the catalog groups, material, position, rotation, phase,
  ends, plate thickness / width; part mark, assembly, cut length, weight, cut-back, its problems, Shop drawing) or the
  model summary; reports: problems, bill of materials, clashes (clash.js on the shown phases), weight report by phase,
  bending schedule, cut list. Every edit is one `Model::change` (the engine's own undo / redo stacks and labels).
  Commands `detail.sample | new | select | set | undo | redo | save | export | setting | clash | phase | tab | camera |
  timings`; the shot tool gained `--open FILE` (as a drop).
- Exports (detailcraft.js `saveModel`, `exportIfc`, `exportNc`, `exportBom`, `exportBending`, `exportKiss`, `exportNest`,
  `exportReport`, `exportReportPdf`, the drawings dialog's all / one sheet as PDF, DWG 2000–2018 or DXF R12 (several in a
  zip), the shop drawing PDF), named by `safeName()` and the catalog, with 0.5's status lines, through `services.save`;
  the clock from the caller (`@<ms>` fixes it; DWG GUIDs then cc-io-dwg's fixed sequence).
- Parity (forks/tools/dcuidiff.mjs: the app driven headless by cadcraft-shot vs the frozen JS composition from the same
  source and the same edit): the sample (58 checks), the sample with a brace rotated, frames13 (the sample pasted 13 times)
  with a beam's profile changed, grid10x6x4, grid24x12x5 (2,820 members, 5,868 parts) and the two IFC fixtures imported
  (52 checks each): project file, IFC, NC zip, BOM / bending / cut list / weight CSVs, KISS, DXF, DWG and their zips
  byte-identical, every PDF identical in pdf.js read-back (operators + text) and with its streams inflated, every status
  line identical; 0 differences.
- Timings (forks/tools/dcuibench.mjs, release, best of 3; JS = frozen 0.5 in Node): open + build + marks/rebar, ms:
  sample 3.1 JS / 2.8 Rust, frames13 17.0 / 14.1, grid10x6x4 46.0 / 29.2, grid24x12x5 222.1 / 248.0 (0.9×: cc-detail's
  `readModel` takes 94 ms against the JS's 15 ms there; build 64.5 / 51.8, marks 89.2 / 154.9). The 3D scene: 6 ms
  (sample) to 116 ms (grid24x12x5). Exports on grid24x12x5, the app's export alone vs the JS composition: IFC 1,362 /
  2,344, all drawings PDF 1,029 / 1,812, DXF zip 1,394 / 3,734, DWG 2000 zip 2,216 / 1,767, DWG 2018 zip 1,733 / 1,903,
  cut list 192 / 196. Big models in testfiles/generated/detail (LARGE-FILES.md).
- M14.2: the report tabs' tables are made once per model version and settings (the cut list nested every bar each
  frame: a frame of the 2,820-member model took 84 ms of UI time, now 17 ms); a clash's Show also brings it to the middle
  of the view, 2.5 m across (view3d.js `focusPoint`); Properties headings for connections, stairs and footings from the
  catalog (app.props.*); empty weights read 0.0 kg, not -0.0.

## 2026-10-17 — M11.6 plot dates from the caller

- M11.6: io-civil's plot and sheet builders no longer read the clock: `plan_sheet`, `plan_profile_sheets`,
  `section_sheets`, `staking_sheets` take the "made" date and `to_pdf` / `to_pdf_pages` the CreationDate from the
  caller (the web build has no clock there, so its plot PDFs said 1970). The app passes its own time; the harness the
  time it runs. Replaces M11.5's after-the-fact patch of the PDF. cvuidiff --demo all identical; cvdiff seeds 1–3:
  20,240 calls, the 1 accepted byte-flipped LAZ difference.

## 2026-10-17 — M11.5 CivilCraft exports in the app (agent, work/civilexp)

- M11.5: `crates/ui-egui/src/civilexport.rs`: civilcraft.js `saveProject`, `exportLandXml`, `exportDxf`, `exportDwg`
  and `plotDialog`, site-view3d.js `exportGLB`, composed from the engine's writers as 0.5 composes them. DXF / DWG / PDF
  draw civilcraft.js `buildScene(mm)` in full (io-civil `plan_scene` over every point and the shown surface's contours,
  plus sample lines, feature lines with z, parcels, line-code figures, daylighted feature lines), not the plan view's
  trimmed draw lists. Names by `fileStem()` (GLB: site-view3d's `fileName`), status lines from the catalog (cv.lx.exported,
  cv.dxf.done, cv.dwg.done, cv.plot.done, v3d.exported, cv.export.none …). The CivilCraft panel's Export group (catalog
  labels): Save, LandXML, DXF, GLB, DWG with the format (2000–2018), PDF with ink, paper, area (everything / on screen)
  and the scale list with the fitting one marked. `civil.export` command: project | landxml | dxf | dwg[:AC10xx|year] |
  pdf[:paper=…,scale=…,ink=mono,area=view] | glb, plus `@<ms>` for a fixed clock (then the DWG's GUIDs are cc-io-dwg's
  fixed sequence). The clock comes from the caller: the project's `saved`, LandXML's date, the DWG's created date, the
  PDF's title strip and CreationDate (the engine reads the wall clock there; the view swaps in the caller's date).
  `civil.save` writes the same project file (it had `"saved":0` at the end; now 0.5's ISO date after `version`).
  Not done: map pictures in the DWG / PDF (the view has no GIS map layers yet).
- Parity (forks/tools/cvuidiff.mjs: the app's files via cadcraft-shot `--save-dir` vs the frozen JS composition on the
  project file the app saved): sample site, demo site (alignment + corridor, pad, network, parcel, feature line; all six
  DWG years), site-topo.csv and the 500 k-vertex LandXML TIN: project, LandXML, DXF, DWG and GLB byte-identical, the
  PDF identical in pdf.js read-back (33,919 operators, 263 texts on the big one) and with its Flate streams inflated;
  every status line identical. Export ms on the 500 k TIN, app (release) vs JS in Node: project 1,118 / 426, LandXML
  3,241 / 2,229, DXF 9,784 / 17,259, DWG 2000 6,524 / 9,551, DWG 2018 7,116 / 11,605, PDF 9,798 / 8,464, GLB 2,409 / 5,988.

## 2026-10-17 — M8.7 TakeoffCraft Auto count in the app

- M8.7: `crates/ui-egui/src/autocount.rs`: takeoffcraft.js `runAutoCount` on the open sheet: box one symbol (drag with
  Auto count on), render at the work scale, cut and trim the template, search with cc-takeoff's matcher (right angles,
  or every 15° with "Any angle"), review (`suppress` at the slider's threshold, 75 % to start; a click rejects or restores
  a match; matches over the item's own marks start rejected), then "Add n marks" as one undo step ("Auto count: n item").
  `ensureItem('count')` as 0.5. `takeoff.autocount` command (box, threshold, accept, discard). Kitchen fixture, the
  harness's shelving box: 22 matches at 75 % in 0.8 s, all 22 known shelves marked within 12 pt.
- M8.8: on the desktop a boxed search runs on its own thread (as the JS uses a worker; the thread opens its own copy of
  the PDF): "Searching this sheet…", the view stays live (pan), the review appears when it lands. The web build and the
  `takeoff.autocount` command search in place. Shot tool: `--wait MS`.

## 2026-10-17 — M8.6 TakeoffCraft exports in the app

- M8.6: the takeoff panel's Export row (catalog labels): XLSX, CSV, Marked PDF, Save takeoff, and the
  `takeoff.export` command (xlsx, csv, pdf, project). Named and written as takeoffcraft.js does: `safeName(name)`
  + .xlsx / .csv (BOM, baseEstimate) / "<pdf> (takeoff).pdf" (with the legend) / .takeoff.json (`JSON.stringify(…, null,
  1)`: new `jv::stringify_indent`). The open PDF joins the project's files, so sheets are labelled "<file>, page n".
  Checked against the frozen JS on a UI-made takeoff (3 counts, a 3-point run, 1/4" = 1'-0"; tools/tkuidiff.mjs):
  project, CSV and every workbook entry but its dates byte-identical; the marked PDF reads back identical in pdf.js
  (95 operators, legend text) and is the same file as the parity-checked CLI path. Shot tool: `--save-dir`.

## 2026-10-17 — M9.12 MarkupCraft measuring tools

- M9.12: Length, Area, Callout and Calibrate on the sheet view, labelled from MarkupCraft's catalog (`tool.*`, `cal.*`,
  `hint.*`; the tool bar no longer has plain-English tool names). Calibrate: drag across a known distance, type it
  (`parseLength`: 24'-0", 7.2 m, …) in a unit, Set scale → `calibrationFrom` in one `cal.set` undo step, then Length is
  next (as 0.5). Scale presets (SCALE_PRESETS) and Remove scale in the bar; `markup.scale` and `markup.finish` commands.
  Dimensions and areas read in the sheet's units (ShapeEnv gets the sheet's calibration). Area: a corner per click,
  double-click / Enter ends, Esc drops. Callout: tip at the press, box past the release (finishCallout sizes).
  Fixed: drag tools began where egui saw the drag (a few px late), now at the press origin. On the 612 × 792 pt grid at
  1/4" = 1'-0": a 404 pt dimension reads 22'-5 13/16", a 278 × 202 pt trapezoid 173.77 sq ft. Shot tool: `--mouse-move`.

## 2026-10-17 — M11 CivilCraft in the app (agent, work/civilapp)

- M11.1–M11.2: `crates/ui-egui/src/civil{view,plan,panel,3d}.rs`. Opens what 0.5 opens: `.civilcraft.json` (through
  `Project::from_json`), LandXML (civilcraft.js `openLandXml`: first surface as TIN, alignments + first profile, first
  pipe network, the rest in notes) and point files (CSV / text, format detected). Plan = io-civil `plan_scene` + the
  screen's own items (sample lines, feature lines, parcels, line-code figures, daylighted feature lines) at the zoom
  bucket (power of two, view.js), linework in one cc-render space relative to the plan centre, f64 view, picking
  (civilcraft.js `pickAt`) and hover (E/N, ground, graded, station/offset) in f64. Panels: site, points and group use,
  surface facts + report, display switches, objects; properties of alignment / pad / structure / pipe / point / feature
  line / parcel; earthwork (grid method) and network check. 3D: cc-civil `build_site3d` in the shared viewport
  (relative to the site origin; coloured lines as crossed ribbons; decluttered labels). `civil.*` UI commands.
- M11.3: large surveys. Zoom-independent parts (TIN outline, line-code figures) cached between buckets; contours drawn
  from the engine's lists (no JSON); point crosses written straight into the GPU bytes. Synthetic survey at x ≈ 500 000,
  y ≈ 4 000 000 (`civil.synthetic`, debug build of the shot tool, busy machine): 100 k points: parse 210 ms, TIN 163 ms,
  contours 50 ms, first plan 0.3 s; 1 M points: parse 2.8 s, TIN 2.4 s, contours 0.8 s, first plan 3.1 s (2.9 s of it
  the engine's `Surface::outline`), plan rebuild on a zoom step 0.10 s (was 4.7 s), panel facts 0.2 s. 60 px/ft at
  501 000 / 4 001 000: TIN edges meet the survey point exactly.

## 2026-10-17 — M9 engines: TakeoffCraft, MarkupCraft; BimCraft's engine

- M9.1 (agent): `cc-takeoff` + `cc-io-takeoff`: TakeoffCraft's DOM-free modules at exact parity (`tools/todiff.mjs
  --seeds 1-10 --exact`: 18,442 records, 0 differences; PDF half 29/29 read back identical by the frozen pdf.js; XLSX
  stored entries byte-identical). Formulas 4.4×, auto count 2× the JS; estimate and XLSX on par (JSON-value state kept
  for parity). Rasters from hayro (match scores within 0.041 of pdf.js's).
- M9.2 (agent): `cc-markup` + `cc-io-markup`: MarkupCraft at exact parity (`tools/mkdiff.mjs` seeds 1–60: 30,994
  records; `tools/mkpdf.mjs` seeds 1–10: 2,262 comparisons; 0 differences). Open a 60-sheet set 6.2×, write 8.4×,
  display lists 33×, BCF 10× the JS. Fuzzing found a panic inside hayro-syntax (`Dict::entries` on a bad hex string):
  dictionaries are now read value by value (regression test).
- M7.9–M7.10 (agent): `cc-bim`: BimCraft's engine (model load, property index, Color by, find, quantities, schedules,
  checks, compare, clash, BCF, tree, visibility, element reading) at exact parity (`tools/bimdiff.mjs`: 38,788 cases
  on fixtures + 119 public/corpus IFC files; BCF zips byte-identical). Load 2.8–5.4×, modules 1.2–4.9× the JS.
- M9.4 (agent): `cc-civil` + `cc-io-civil` + `cc-proj` (proj4 port): CivilCraft at parity (`tools/cvdiff.mjs` seeds
  1–10: 67,101 calls, 4 accepted differences on byte-flipped LAZ / damaged rasters; seeds 2–10 move the fixtures to
  survey coordinates). TIN 2.45×, contours 2.78×, grid volumes 1.24×, LandXML 1.16× the JS. Fuzzing found two `laz`
  crate crashes (guarded, regression seeds); LAZ is not decoded in the web build, where a panic cannot be caught.
  `num_to_string` now picks the even digit on 17-digit ties as V8 does, behind a cheap binary test (proved on 300,000
  values) so the exact expansion runs almost never; wdiff 468/468, idiff 220/220, iodiff 801/801 unchanged.
- M10.1–M10.5.1 (agent): `cc-part`: PartCraft's engine without the rebuild kernel — document model, params,
  configurations, sketch + solver, threads, GD&T, BOM, .pcscript, mesh faces / named faces, reference geometry,
  assemblies + joints, standard parts, drawings and exports (STL, 3MF, DXF, STEP, PDF, IFC4) — at exact parity
  (`tools/pcdiff.mjs` seeds 1–20: 239,669 records, 0 differences). measure 31×, refgeom 26×, analysis 5× the JS.
  Open: `kernel.js` rebuild / meshSection / interference need cc-manifold extrude, revolve, trimByPlane, CrossSection …
- M11.1–M11.4 (agent + lead): CivilCraft in the app (project / LandXML / points; plan at survey coordinates; panels;
  3D site; `civil.*` commands); 1 M points: zoom-step rebuild 0.10 s.
- M9.12: TakeoffCraft mode on the sheet view: sheet scale (the 0.5 presets), count / linear / area items, marks clicked
  on the sheet (double-click or Enter ends a length or area), live quantities and the estimate total from cc-takeoff
  (`quantities`, `estimate`), undo / redo through `Takeoff`; `takeoff.mode`, `takeoff.scale`, `takeoff.item`,
  `takeoff.finish` commands. Kitchen sheet at 1/4" = 1'-0": the serving line measures 40.47 ft.
- M10.20–M10.22 (agent): `cc-detail` + `cc-io-detail`: DetailCraft at exact parity (`tools/dcdiff.mjs` seeds 1–10:
  55,714 calls, 0 differences; writers byte-for-byte, PDFs inflated-equal; IFC import on 117 corpus files). Mark parts
  3–8×, clash 2–3×, PDF 2–8×, IFC out 2–2.6×, IFC import 2–4× the JS; readModel / build / KISS slower (JSON-value state
  kept for parity). Fuzz targets detail_model, detail_clip, detail_nc1_read, detail_ifc_import.
- M9.11: desktop eframe without `accesskit` (screen readers are the skipped known issue): its Linux bridge pulled zbus
  crates RustSec marked unmaintained today (RUSTSEC-2026-0370/0371/0372, no upgrade), failing `cargo deny`.
- M9.10: MarkupCraft tools on the sheet view: Select (listDistance pick), Pan, Pen, Highlighter, Line, Arrow,
  Rectangle, Ellipse, Cloud, Text; colours; each gesture one `Store::change` step through makeMarkup (the 0.5 ids, styles,
  undo labels), drawn while dragging from the same display list; Undo / Redo / Delete (Ctrl+Z, Delete); Save marked-up
  PDF through cc-io-markup writeMarkedUp; `markup.tool`, `markup.undo` commands; shot tool `--mouse-drag`.
- M9.8: MarkupCraft markups on the sheet view: read with cc-io-markup openPdf when the PDF opens, drawn from cc-markup's
  display lists (shapes.js: the list the saved PDF's appearances come from) through the page's viewport transform;
  pages MarkupCraft draws itself (annotMode "disable") render without their annotations' own appearances (hayro
  `render_annotations`, which auto count keeps on); Markups switch with counts.
- M9.7: crash fix (gate fuzz smoke, `civil_las` OOM): a layered LAZ chunk listing a 4 GB layer made laz-rs size a buffer
  to it before reading; every chunk's layer sizes are now checked against the bytes left before laz-rs reads it (chunks
  found by chunk_size or the chunk table); regression fixture + unit tests + seed; crs-las parity unchanged (358 calls,
  the same 2 accepted differences on byte-flipped files).
- M9.6: PDF plan sets open in the app (the sheet view MarkupCraft and TakeoffCraft build on): sheets by page label,
  hayro render into one texture re-rendered per half-octave of zoom (capped 8192 px a side), pan / wheel zoom / Page
  Up-Down; the kitchen fixture sheet renders in ~22 ms. `pdf.page` command.
- M7.11: BimCraft's Find (rules on any property, every / any, shows only or hides what it finds) and Quantity totals
  (group by, add up a quantity, CSV as totalsCsv) on the IFC view, on cc-bim; the property index is built on first use;
  BimCraft's 0.5 catalogs (en, es-MX) shipped unchanged; `bim.find`, `bim.totals`, `view.show_all` commands.
- M6.15–M6.16: ModelCraft Select all / group / component / explode / delete / paint colours, Scenes, Components,
  view styles (see below).

## 2026-10-16 — M6 ModelCraft and M7 BimCraft in the app

- M6.8 (agent): `cc-m3d-tools` (L4): ModelCraft's camera.js, infer.js, collide.js, tools.js (all 29 tools), sun,
  value box, terrain, components that stretch, starter library; exact parity (`forks/tools/tdiff.mjs --exact`: 19 test
  sections 1,281/1,281 records, seeds 1–10 4,708/4,708); infer / pick 1.9–2.9× the JS.
- M6.10 (agent): `cc-io-m3d` (L3): every format of io.js + solids, SketchUp import, print3d; byte parity 801/801 after
  M6.11 (a signed-zero `Math.max` in the DWG writer); 1.2–15× the JS. M6.12: the open/save glue in `cc-m3d-files` (L4).
- In the app (desktop + web): `.modelcraft`, SKP, OBJ, GLB/glTF, DAE open as models; the tool palette drives
  cc-m3d-tools (left = tool, middle/right orbit, wheel zoom, tool letters, value box, arrows lock, Esc, undo/redo) with
  the tools' overlay (markers, tips, rubber bands, previews, grips, selection window, hover/selection); Export (ModelCraft,
  GLB, OBJ, STL, DAE, IFC4, DXF, DWG); standard views, parallel/perspective, Fit (model.bbox()).
- M7.4–M7.8: IFC in the 3D view: cc-ifc view export (stored meshes once, pieces placing them), feature edges,
  picking (box tree + exact ray–triangle), Properties panel (attributes, property sets, quantities, type), Model panel
  (storeys, classes, hide/show), section cut (clip plane). Clinic_Architectural (2,586 products, 168,703 triangles)
  read and drawn in ~320 ms.
- M6.14–M6.15: side panel (Entity Info, Outliner, Tags, Paint colour from `paint::PALETTE`), es-MX/en catalogs;
  Select all (Ctrl+A), Make group (Ctrl+G), Make component (Ctrl+Shift+G), Explode and Delete as modelcraft.js does them
  (one undo step each, purge after erase); scriptable as `model.edit`. Scenes panel (add, go to, update, delete, Previous / Next, Page Up / Down;
  camera, style and visibility as goScene, moving there over 650 ms as goScene does) and Components panel (count, Place). M6.16: view styles
  (Shaded, Hidden line, Wireframe, X-ray, Edges) for ModelCraft and the IFC view; hidden line fills faces with the
  background colour.
- Checked in Chromium over WebGPU on rust.getcadcraft.com: `?model` (orbit), `?ifc` (click → properties), Open → SKP.
- Measured for M9: hayro renders the Carey PDF sheet 9× faster than pdf.js at 1× (roadmap § M9 inputs).

## 2026-10-16 — M5 done; cc-ifc, cc-manifold, cc-skp done; M6 started

- M5.1 speed debt: DXF 2000 save of the 3.4 MB drawing 702 → 357 ms (JS 450: now 1.26×; DWG 2018 1.45×, DWG 2000
  1.5×): one output buffer, each number formatted once straight into it (toPrecision digits from the shortest digits,
  proven equal on 200,000 random doubles), entities built straight into the saver's values (`cc_cad::entity::ObjSink`
  instead of entity → JSON → value), no deep copy of the blocks. Save parity unchanged (byte-identical 78/78, 32/32,
  110/110 per version). Edit mode: a per-object display cache (`DisplayCache`): an edit walks only the objects that
  changed (all of them when a layer, linetype, style, block or the drawing settings changed): 127 → ~38 ms per edit on
  the 3.4 MB drawing; equal to a full walk (tested through edits, undo, layer and block changes).
- Measured: the DWG reader as single-threaded wasm (WASI under Node) reads the 3.4 MB drawing in 257 ms vs the JS's 527
  (2.05×): the browser's 1.2–1.5× (M3) came from the 174 KB sample, where fixed costs dominate.
- M5.2: golden references of the public fixtures in CI (`tests/golden`, from the kit's `ref.mjs --public-out`).
- M5.3: nightly fuzz OOM in `cad_dxf_read`: nested block inserts multiplied without end in an object's outline and the
  walk; budgets (2 M primitives per outline, 10 M visits per walk) + regression tests.
- M6.1/M6.2 groundwork: `cc-render::mesh3d` (3D viewport GPU side: meshes per definition, camera-relative f64
  instances, reversed-Z, MSAA offscreen target, styles, read-back; headless cube at survey coordinates) and
  `cc-geom::bvh` (bvh.js, same splits and visiting order).
- M5.4 (agent): `cc-m3d` = ModelCraft's model.js in full (+ vec, earcut, units formatters) at exact parity
  (`forks/tools/mdiff.mjs --exact`: 640/640 op records + 1,247/1,247 function cases per seed, seeds 1–10; V8's TimSort
  ported because NaN keys make comparators inconsistent), 3.4–8.4× the JS; fuzz `m3d_load` 5.4 M runs.
- M7.1 (agent): `cc-io-ifc`: ifc-step 118/118 exact (4.4–7.9×), ifc-geom with booleans behind `trait SolidOps`.
- M7.2 (agent): `cc-manifold`: Manifold 3.5.4's mesh boolean ported to pure Rust (Apache-2.0 port, notices kept),
  identical to the manifold-3d wasm on 11,093 cases (incl. 1,102 recorded from the IFC corpus), 1.8× the wasm; works
  on the web (no C++). Chosen over the approved C++ binding because a binding cannot build for wasm32-unknown-unknown.
- M7.3: IFC openings through cc-manifold: ifc-geom 115/115 (was 105/115), 3.5–7.9× the JS with booleans on both
  sides.
- M8.1 (agent): `cc-io-skp`: readSkp 27/27 byte-identical canonical JSON, 5.4–14.2× the JS, faster than OpenSKP's
  recorded times on every corpus file; fuzz `skp_read`.
- M6.3/M6.4: the 3D viewport in the UI (offscreen render on demand + blit, orbit / pan / zoom from camera.js) on
  desktop and web (`?demo3d`, checked in Chromium over WebGPU), deployed.
- Next (M6): camera.js / infer.js / tools.js and io.js formats on cc-m3d (agents), then ModelCraft's app shell.

## 2026-10-16 — M4 done

- M4.7: `cc-io-cad` (L4, agent-ported): CadCraft's DWG import (dwg-import.js + helpers), DXF read / write R12 (dxf.js),
  DWG save 2000–2018 (dwg-save.js over the M4.4 writer) and DXF 2000 (dxf2000.js). `forks/tools/idiff.mjs` (frozen JS
  vs `cadcraft-cli import / dxf-out / save`): 0 differences on 78 kit DWGs, 32 other DWGs, 110 DXFs (import, Doc after
  load, R12 text, re-read); saves byte-identical with equal reports, 78/78 per version (2000, 2004, 2007, 2010, 2013,
  2018, DXF 2000), also on the other DWGs, the DXFs, a synthetic fixture and 15 dynamic-block variants. Fuzz target
  `cad_dxf_read` (read → load → R12 → DXF 2000 → DWG save): 3 × 10 min, no crash.
- M4.8: Edit and Save in the app. **Edit** imports the open DWG / DXF into an editable drawing (the viewer stays the
  fast default for viewing); **Save as** DWG 2018 … 2000, DXF 2000, DXF R12 (desktop: dialog + write; web: download),
  Ctrl+S = the last format. Checked: a headless test (sample → Edit → LINE → all 8 formats → each re-opened with the new
  line), cadcraft-shot `--edit --save`, Chromium on the local build and on rust.getcadcraft.com (Edit, CIRCLE, RECTANG,
  Save as DWG 2004 → an AC1018 download holding both).
- Edit-mode display fixes found by measuring the big drawings: a linetype's dash on a circle kept the circle's `full`
  flag and was drawn as a whole circle (Carey: 328 dashed circles = 906,000 segments → 22,000); a dash budget per
  object (`DISPLAY_MAX_DASHES` = 400, past it the object draws continuous; parity paths keep the JS's 20,000); batches
  per layer (the layers panel works in edit mode); layer colours.
- Command line: the field keeps the focus on Enter, and the input hook passes one Enter per frame (what follows waits a
  frame): typed answers were lost or merged on the web when keys came faster than frames.
- `round_precision` (toPrecision) from the shortest round-trip digits instead of 40 exact digits: proven equal, checked
  on 200,000 random doubles; DXF 2000 save of the 3.4 MB drawing 834 → 702 ms.

## 2026-10-16 — M4 in progress

- M4.1: `cc-engine::prompt`: commands are async blocks that ask questions (`Prompter::ask`) and a runtime-free
  `Runner` answers them (Esc = `Cancelled`), single-threaded, wasm-safe.
- M4.2: `cc-cad` (L1, doc.js: typed entities, change/undo/redo across spaces, blocks, layouts, load, snapshot) and
  `cc-cad-geom` (L2, geom.js, dims, dimstyle, units, walk, font, leader, linetypes, hatch). Agent-ported. Parity with
  `forks/tools/gdiff.mjs --exact` (no tolerance): 0 differences on ~18,000 geometry cases per seed (seeds 1–10) and
  1,210/1,210 doc-op steps. V8's sin/cos are fdlibm's: ported (`jsmath.rs`, Sun notice kept, NOTICE updated);
  tan/atan/atan2/acos from the `libm` crate match V8 exactly.
- Fuzz target `cad_load` (any JSON → `Doc::load` → snapshot → grips, explode, walk): 648,764 runs in 10 min, no crash.
- `cc-cad-geom::display`: the editable document walked into the viewer's display model, so the drafting canvas
  renders through the M3 GPU renderer.
- M4.4: the cc-dwg writer (agent): `write_dwg` byte-identical to the 0.5 JS `writeDwg` on 78 files × 6 versions
  (468/468, reports equal), 72/72 synthetic models; 1.2–3.1× faster single-threaded; fuzz target `dwg_roundtrip`.
- M4.5: the command line and drafting commands (agent): LINE PLINE CIRCLE ARC RECTANG POLYGON ELLIPSE POINT TEXT ERASE
  MOVE COPY ROTATE SCALE MIRROR OFFSET TRIM EXTEND UNDO REDO ZOOM; `forks/tools/kdiff.mjs` (the frozen JS Engine with a
  stub host vs `cadcraft-cli cmds`): 0 differences, 26 seeds, ~70,000 records (6 seeds bit-exact).
- M4.6: drafting in the app: New drawing (Ctrl+N, `?new` on the web), the command line (history, prompt, Enter /
  Space / right click = Enter, Esc, Delete, Ctrl+Z / Ctrl+Y, F3 object snap, F8 ortho), cadcraft.js's click rules (point
  questions: object snap end/mid/cen/quad/int/perp/node/ins with markers, then ortho; object questions: pick or
  window; idle: pick, Shift removes, a click on nothing starts a window, drag = window, right-to-left = crossing),
  the rubber band, selection highlight with grips, ZOOM requests. The 0.5 language catalogs (en, es-MX) ship unchanged
  in `assets/lang/cadcraft/`. Checked in Chromium (WebGPU) with typed commands and real clicks.
- Still running: DWG import + CadCraft's DXF read/write, then DWG save (dwg-save.js) and DXF 2000 (new crate `io-cad`,
  harness idiff.mjs), by an agent in its own work copy.

## 2026-10-16 — M3 done

- `cc-drawing` (L1): the display model (spaces → batches per layer × colour × weight: polylines, fills, texts),
  serialising to the JS `compileDrawing` shape. `cc-io-dwg::compile` + `dxf_model`: BimCraft's compile step and DXF
  reader ported at parity (0 differences: 78 corpus DWGs, 19 client DWGs, 106 DXFs). Agent-ported, merged in M3.2.
- `cc-render` (L3): wgpu, instanced anti-aliased line quads + lyon-tessellated fills, positions f32 relative to each
  space's origin and the view centre subtracted in f64 (survey coordinates stay steady), one uniform slot per view
  (model, or sheet + each viewport), one draw call per visible batch.
- `cc-ui-egui` (L6): canvas through an `egui_wgpu` paint callback, sheets (paper, viewports clipped/scaled/twisted
  like dwg-view.js, viewport frames), texts on top (egui text, culled, budgeted), layers panel, sheet tabs, status bar,
  `UI_COMMANDS` + `UiApp::run`, injected `Services` (pickers, inbox, xref lookup), Dark/Light tokens with a contrast
  test, bundled OFL fonts (egui's default fonts carry the Ubuntu Font Licence: not on our list).
- Apps: desktop `cadcraft` (eframe + rfd), web `cadcraft-web` (WebGPU only; a clear message without it; panic →
  message on the page), `cadcraft-shot` (headless screenshots + frame bench through egui_kittest on wgpu/lavapipe).
  `cargo xtask web` builds with trunk and moves the inline init script into a hashed file (CSP: no inline script).
- Deployed through `forks/deploy.sh rust dist/web` → https://rust.getcadcraft.com (`?sample`, `?bench`).
- Numbers in ROADMAP.md. Honest gap: the browser build is only 1.2–1.5× the JS reader (native 2.6–4.2×).
- Dev tools outside the repo: `forks/tools/cdiff.mjs` (compile parity), `forks/tools/webshot.mjs` (Chromium +
  SwiftShader WebGPU screenshots: needs `--use-vulkan=swiftshader --use-angle=swiftshader --enable-unsafe-swiftshader`).

## 2026-10-16 — M2 done

- **cc-dwg in Rust** (`crates/io-dwg`): a line-by-line port of CadCraft's own `lib/dwg` reader (≈6,500 lines of JS →
  ≈12,900 lines of Rust incl. tests): R13–R2018 containers (R2004 LZ, R2007 Reed-Solomon + R21 LZ), bit reader, header variables,
  classes, object map, every object decoder (entities, tables, objects, ACAD_TABLE family, ACIS solids + data storage,
  surfaces, multileaders, dynamic blocks), and the model assembly of `read.js`.
- **Design:** the reader builds the JS model itself as a dynamic `value::Obj` (insertion-ordered, `&'static` keys), so
  `parity dwg` emits the same JSON as `readDwg` and the kit diffs the *whole* model, not a summary. Objects decode in
  parallel (rayon) and entities convert in parallel; R2004 pages decompress in parallel. Every decoder runs under the
  crate's deny list (no unwrap/panic/indexing/unchecked arithmetic/lossy casts); a panic in one object would cost that
  object only (`catch_unwind` guard).
- **Parity:** `tools/fork-parity run.mjs --fork rust --engine dwg` → 78/78 match. Every other DWG in testfiles: 0 diffs.
- **Speed:** 2.6–4.2× the JS on the corpus; 3.0–3.4× on the large client drawings; single-threaded 1.8–2×
  (ROADMAP.md has the table). Allocation is the remaining cost (a dynamic model allocates per field); a faster allocator
  measured ~35% more, but the good ones are C (mimalloc) — not taken (AGENTS.md: no C unless unavoidable).
- **Safety:** fuzz target `dwg_read` (libFuzzer): 30 s in every `cargo xtask ci`, a 10-minute run at M2's end (see the
  M2 report); 500 mutated corpus files (truncations, bit flips, random bursts) through the CLI: no panic.
- **How it was built:** the lead ported the core and the table decoders; five agents ported entities, objects, tables,
  solids/surfaces and multileaders/dynamic blocks in parallel work copies, each to zero differences on its types, merged
  in M2.1–M2.4; M2.5 = speed.
- Next: M3, the 2D viewer (DWG/DXF, model + sheets, pan/zoom, layers) on desktop and web (WebGPU), deployed to
  rust.getcadcraft.com.

## 2026-10-15

- M1: workspace skeleton (crates by layer, apps, xtask, fuzz), lints, configs, CI workflow. `cargo xtask ci` green.
