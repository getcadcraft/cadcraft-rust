# Session log (fork-rust)

Newest first. One entry per working session: what was done, numbers, what is next.

## 2026-10-20 — M9.21 TakeoffCraft typical groups, notes, clouds and highlights (agent, work/takeoff2)

- `crates/ui-egui/src/takeoffnotes.rs`, ports of takeoffcraft.js: typical groups (T16) — the group field of the selected
  marks (one or several: "—" when they differ; Not typical; each group "Name (×n)"; New typical group… opens the window
  with "Typical n" and ×2, readRepeat's "x3" / "×3", the name and repeat checks staying in the window), the groups list
  (rename, repeats, how many marks, select its marks — on this sheet if it has some, else the first sheet that has —
  delete: its marks count once), the dashed box and "Name ×n" tag round a group's marks; quantities, estimate and exports
  count the repeats (cc-takeoff, already at parity). Markups (T22): the Markup tools note / cloud / highlight (a note at
  a click, a cloud or highlight between two corners at least 4 screen px apart), the asked text (a note needs some, a
  cloud or highlight may be blank), select (on top of the marks, not with Shift), the Markup panel (kind, sheet, status,
  text, Delete), Delete key ("Deleted the cloud."), the Markups list with Open only, drawn as 0.5 draws them (pin with
  the first line, scalloped cloud, see-through highlight) and in the marked PDF (cc-io-takeoff). Commands
  `takeoff.note`, `takeoff.group`, `takeoff.mark=group=…`, `takeoff.tool=note|cloud|highlight`.
- Parity (tkopsdiff): new scenarios groups (52 ops: made from a selection, refused names and repeats, moved in and out,
  renamed, repeats, selected, deleted and undone, a group pasted) and notes (46 ops: every kind, empty text, too small,
  cancel, status and text, delete and undo, Esc): project, CSV, XLSX and marked PDF identical, 52/52 status lines; the
  seven earlier scenarios unchanged (205/205 status lines in all).

## 2026-10-20 — M9.20 TakeoffCraft arcs and circles, dragging corners, several marks selected, the clipboard (agent, work/takeoff2)

- `crates/ui-egui/src/takeoffedit.rs` (new) and `takeofftools.rs`, ports of takeoffcraft.js: arc mode (R: after a point,
  a point on the curve, then the arc's end; closing an outline on an arc; Backspace drops the pending curve point, then
  the last point and the arc that led to it; Esc the curve point first), the Circle tool and round cut-outs (two
  half-circle arcs, "too small" under 3 screen px), arcs in runs, outlines and cut-outs (a run's last point starts no
  arc; an outline's closing check keeps an arc's end). Dragging a corner of the selected mark (runs, outlines, cut-outs,
  counts) or the round handle in the middle of an arc to bend it (a straight line drops the arc); an outline that would
  cross itself or a cut-out that would leave it is put back with 0.5's message. Several marks selected: Shift+click adds
  or takes out (the main mark moves on), Select all / none, corner squares on each, the Selected marks panel (how many of
  each item, their bid area, copy / duplicate / cut / delete), Delete. The clipboard on clipboard.js: copy, cut (one
  step), paste on the pointer (a dashed preview, the click puts it down), paste in place (checked against the sheet's
  size), duplicate, the scale check between sheets, the current bid area on pasted marks. setTool's checks and help
  lines, Undo / Redo say "Undid: …". The draft keeps bulges (`P`); the selected mark shows its segment lengths and area.
  Commands `takeoff.arc`, `takeoff.drag`, `takeoff.edit`, `takeoff.click=x,y,shift`, `takeoff.tool=circle|cutcircle`.
- 0.5 TakeoffCraft has no window or lasso selection (a drag on an empty spot pans; lasso-core.js is not imported by
  takeoffcraft.js), so none was added: multi-select is Shift+click, Select all and a typical group's marks, as in 0.5.
- Parity (`forks/tools/tkopsdiff.mjs`, now with status lines: the app's `--trace` line after each command against what
  the 0.5 handler says): new scenarios arcs (81 ops), drags (55), multi (68 ops, two PDFs): project, CSV, XLSX and
  marked PDFs identical, 110/110 status lines; the four M9.13 scenarios unchanged and their 43/43 status lines too.

## 2026-10-20 — M9.19 MarkupCraft BCF export / import, compare picture overlay, line-up, whole-set compare (agent, work/markup2)

- `crates/ui-egui/src/markupbcf.rs`: BCF 2.1 export of the Markups list as filtered (cc-markup `build_bcf`; one topic per
  markup with a snapshot: the markup flattened into its page by the PDF writer, the page drawn by hayro and cropped as
  view.js `snapshot` crops, PNG) and import (`read_bcf` + `plan_import`: replies and statuses back onto exported
  markups, other topics as notes, one undo step), with 0.5's status lines. Commands `markup.export=bcf`, `markup.bcf`.
- `markupcompare.rs` rewritten around 0.5's compare state: picture overlay (both sheets rendered at the view's scale,
  capped at 2400 px, the old one resampled through the alignment, mixed by compare.js `combine`; the balance slider
  mixes again without rendering) or changed lines; nudge by a step (0.25 / 1 / 5 / 20 pt), align by two points
  (four clicks, `similarity`), Reset; clouds use the alignment; Compare two sets (`matchSheets` by number, ranked, Cloud
  all changes in one step). `markup.compare` takes mode / nudge / step / align / reset / balance / set / set-cloud.
- Parity (mkuidiff): compare2 (rev-a / rev-b: nudged and aligned clouds, a failed alignment, the set compare and its
  clouds) and bcf (three exports, the own file re-imported after a status change, an outside BCF twice, a non-BCF):
  saved PDFs and BCF entries identical (snapshot pictures and the size in the status line excepted), 17/17 status lines.
  Full sweep at the end: 17/17 scenarios identical, 194/194 status lines.

## 2026-10-20 — M9.18 MarkupCraft cut-outs, spaces, legends, sheet links, punch list (agent, work/markup2)

- `crates/ui-egui/src/markupplus.rs`: Add a cut-out to the selected area (holes.js checks: inside, not crossing, not
  overlapping; the area's new value in the status line) and remove them; Spaces (New space: an outline, then its name;
  markups are in the space holding the middle of their bounds; the Spaces list under the sheets, the list's space
  filter, the space row in Properties and the CSV / XLSX space column; rename in Properties); the legend (placed at the
  top right of the view, counting each kind of markup on the sheet or the whole set; title and scope in Properties;
  drawn with its rows on screen and in the saved PDF); sheet links (Link in Properties by sheet number or title, Go to /
  Ctrl+click, Alt+← back, an arrow on linked markups; written as a PDF link when the sheet is in the same file); the punch
  list (add the selection in sheet order, assignee / due / item status, take off; the Punch list tab and its CSV).
  Commands `markup.cutout | space | legend | link | punch`; `cadcraft-shot mkactbench`.
- The sheet view now reads each page's title block text when a PDF opens, as 0.5's openPdf does (`read_text`), so sheet
  numbers and names ("A-101 — Floor plan") match 0.5 everywhere (list, CSV, links, status lines, the sheet list). Open of
  the 200-sheet set: 46 → 205 ms (JS 2,512 ms).
- Parity (mkuidiff): holes (71 ops), spaces (57), links (54 ops on the 4-sheet set: links by number, by a loose number,
  missing and same sheet, back past the start, the punch list with every field and its CSV): saved PDFs (three PDF links),
  CSVs and XLSX identical, 41/41 status lines; the twelve earlier scenarios unchanged (12/12).
- Timings (`forks/tools/mkactbench.mjs`, best of 3, 200 markups on page 1, ms, JS = frozen 0.5 modules in Node):

  | action | kitchen sheet JS / Rust | plan-set-200 JS / Rust |
  |---|---|---|
  | copy + paste | 3.0 / 2.6 | 3.8 / 2.4 |
  | duplicate | 3.6 / 2.3 | 3.6 / 2.3 |
  | list CSV / XLSX / PDF summary | 1.0 / 0.3, 8.2 / 3.3, 81.9 / 4.7 | 1.1 / 0.3, 8.1 / 3.4, 44.3 / 4.5 |
  | rotate page (bake, rotate, read back) | 195.8 / 28.8 | 1,327 / 259 |
  | extract page | 97.3 / 15.8 | 190.2 / 22.8 |
  | punch add / punch CSV | 3.9 / 0.8, 4.5 / 1.3 | 2.5 / 0.9, 2.9 / 1.3 |
  | legend + Save | 55.4 / 10.3 | 131.3 / 15.4 |

## 2026-10-20 — M9.17 MarkupCraft picture and signature stamps, tool chest (agent, work/markup2)

- `crates/ui-egui/src/markupchest.rs`: the tool chest (the six stock tools, Save as a tool from a markup, use, remove,
  import / export the chest file — chest.js), a chest tool in use as the drawing tool's preset (its style, subject,
  stamp or count label and starting text on every markup it draws; its style, label and stamp edited in Properties; a
  toolbar tool drops it), the stamp list's Picture… and My signature… (placed at 3 in / 2 in on a click, fitted in a
  dragged box, turned with the page), the signature pad (strokes → a trimmed transparent PNG), a photo on a markup's
  thread. Pictures are drawn on the sheet from their textures. What the JS keeps in localStorage (chest, picture,
  signature, stamp, count group, tool styles) is `UiApp::mc_prefs`, kept by the desktop app in `markupcraft.json`.
  Commands `markup.chest | picture | signature | photo`; the Tools tab.
- `cc-io-markup::picture` (new): a picked file is hostile — PNG / JPEG only by magic bytes, the header read first and
  the decode bounded by `image::Limits` (16 384 px a side, 512 MB); a picture ≤ 1600 px and ≤ 3 MB is kept byte for byte
  (the JS re-encodes through a canvas, which nothing else can reproduce), a bigger one scaled to 1600 px and written
  again (JPEG at 90, else PNG). New dependency `image` (png + jpeg). Fuzz target `markup_picture` with seeds.
- Parity (mkuidiff): chest (71 ops: every stock tool drawn, preset style / label edits, save, remove, import with bad
  tools and two non-chest files, export) and pictures (36 ops: picture and signature by click, box and short drag, a
  JPEG, a non-picture, photos, copy / paste of pictures): saved PDFs and the chest file identical, 28/28 status lines;
  the ten earlier scenarios unchanged.

## 2026-10-20 — M9.16 MarkupCraft clipboard, Markups list, replies, sheet names, Pages menu (agent, work/markup2)

- `crates/ui-egui/src/markupedit.rs`, ports of markupcraft.js: Copy / Cut / Paste (beside the copy, a step further each
  time; in place; at the pointer with Ctrl+V; the middle of the view when "beside" is off screen) / Duplicate / Select
  all on clip.js, each one undo step with the 0.5 labels and status lines; Undo / Redo say "Undid …" (afterHistory).
  Replies on the selected markup's thread (Properties: the thread, Reply; a reply does not touch the markup's date).
  Rename sheet (number and title typed by hand, undoable; page titles follow `sheetInfo` everywhere; the number becomes
  the page label on save). The Pages menu: rotate, move earlier / later, insert a blank page, insert pages from a PDF,
  delete pages, extract pages (a new PDF), add a blank page to a blank set — the markups baked in, the pages changed with
  cc-io-markup `pages`, the file read back (its sheets unsaved, undo history starts again, as 0.5 says). The Markups tab:
  filters (sheet, author, status, type, space, layer, column), order (sheet, author, status, type, created, modified,
  columns), the count line, a row per markup (a click jumps to it), CSV / XLSX / PDF summary of the listed markups.
  Commands `markup.edit | reply | rename | pages | filter | jump | export | tab`; the tool bar has Copy, Paste, More
  (cut, paste in place, duplicate, delete, select all / none) and Pages. Plain English: none new.
- Parity (`forks/tools/mkuidiff.mjs`, now with status lines: every command whose 0.5 handler says something is compared
  with the app's `--trace` line; zips compared entry by entry): new scenarios clip (91 ops, two PDFs), replies (22),
  sheets (30 ops on a 4-sheet set: rename, every page action, extract, insert a PDF), list (77 ops: every export three
  times over filters and orders): saved PDFs, extracted PDF, CSV, XLSX and PDF summary identical, 104/104 status lines;
  the six M9.14–M9.15 scenarios unchanged (10/10 checks).
## 2026-10-20 — M4.20 Visibility states in the block editor: BVSTATE, BVHIDE, BVSHOW; final sweep (agent, work/xrefs)

- `cmd/visedit.rs`: visedit.js — BEDIT opens a block that has visibility states (`dyn.vis`, from a DWG) on the state
  its objects show, the other states' objects kept aside; BVSTATE (list, Set current, New with Hide all / Show all /
  Keep current, Rename, Delete, with 0.5's refusals), BVHIDE (selected objects out of the current state), BVSHOW (the
  hidden objects shown faded, picked within the host's pick radius). BCLOSE rebuilds the definition with its states
  (`visEdited` for the DWG writer's dynblock rewrite) and references follow (dyn-blocks.js `followStates` /
  `setVisibility`: renamed states, a deleted state → the first, a reference moves to a "*B<n>" copy showing its state);
  objects added or erased in the editor save it plain, with 0.5's note. `Host::pick_radius` (the window: its pick
  tolerance). Making new visibility parameters stays out, as in 0.5.
- Parity: kdiff (a new scenario: states switched, made, renamed, deleted, objects hidden and shown, UNDO inside the
  editor, BCLOSE with references following, the plain fallback) 78/78 records identical.
- Final sweep on this commit: kdiff seed 3, all 39 scenarios + 20 random scripts, 5073/5073 records identical; idiff
  `--set kit --save --xrefs` (every kit drawing gets two linked drawings, two clips, two binds and two pictures, then
  saves) byte-identical 78/78 in DWG 2000, 2004, 2007, 2010, 2013, 2018, DXF 2000 and DXF R12 (624/624), reports equal.
- Not timed against the JS (commands on a block's handful of objects; the kdiff scenario runs in under a millisecond a
  step). No screenshot: no drawing in the corpus has a dynamic block with visibility states (the editor shows the
  state's objects as ordinary objects).
- Gate note: `fuzz m3d_tools` hit a 30-minute timeout in `cc_m3d::edges::add_edge` (ModelCraft's kernel, not touched
  here) on the M4.19 tree; artifact kept outside the repo; every other fuzz target passed.

## 2026-10-20 — M4.19 Pictures: IMAGEATTACH, IMAGE, IMAGECLIP, raster images drawn (agent, work/xrefs)

- `cmd/pictures.rs`: pictures-cmd.js — IMAGEATTACH (corner, width, angle; a "*I<n>" block through cc-io-cad's
  `picture_block`), IMAGE (the files the drawing's pictures use, shown / not found, then the picked files matched by name
  or by a name containing it, as eTransmit prefixes them), IMAGECLIP (Rectangular / Polygonal / Delete; a picture block
  other references share gets a copy of its own first). `cc-cad` `pictures`: the files kept with the drawing while it is
  open (`Doc::pictures`, `picture_version`; not undoable, not saved), `sniffPicture` / `jpegInfo` / `addPicture` /
  `findPicture` with pictures.js's caps (64 MB, 100 million pixels).
- Drawing: walk reports each picture frame (`WalkOut::picture`, pictures.js `pictureOut`) and leaves out a picture's name
  text once its file is loaded; the editor's display collects them per space. `cc-io-cad` `raster::decode`: PNG / JPEG
  through the `image` crate (png + jpeg only) with `image::Limits` (30 000 px a side, 512 MB of allocations, the 100 MP
  cap), sampled down to the GPU's texture side; a new fuzz target `cad_raster` with three generated seeds. The window
  (`picmesh.rs`) draws each picture under the linework as a textured mesh over its frame cut to the picture's rectangle
  (ear clipping), also inside sheet viewports; the viewer finds the files a DWG names in its folder through
  `Services::read_path` (desktop) and hands them to the editor; on the web IMAGE takes them, as 0.5 in the browser.
- Parity: kdiff (two new scenarios: IMAGEATTACH / IMAGE / IMAGECLIP with a shared picture block, a not-a-picture file,
  prefixed names; IMAGE with no pictures) 58/58 records identical; idiff `--xrefs` now also places two pictures (a PNG and
  a JPEG named by a Windows path) before saving: byte-identical in DWG 2000 and 2018, DXF 2000 and DXF R12 (the full kit sweep is in the M4.20 entry).
- Timings (drafttime, JS / Rust ms): IMAGEATTACH (a 4 × 4 PNG) + IMAGE 6.4 / 0.4 on Carey, 6.0 / 0.4 on ar1all,
  5.2 / 0.3 on the kitchen sample. Decoding for the screen has no JS counterpart (the browser's createImageBitmap).
- Screenshot checked: two pictures placed in a new drawing, one turned 20° and clipped to a polygon, a line drawn over them.

## 2026-10-20 — M4.18 Linked drawings and clipped references: XATTACH, XREF, XCLIP (agent, work/xrefs)

- `cmd/xref.rs`: xref.js (`attachXref` with VISRETAIN-style kept layer settings and the "Name (2)" free names, `detachXref`
  keepRefs / keepRoot, `bindXref` → "Name$0$Layer", `xrefs`), layertools.js's XATTACH (one undo step, Esc at the
  insertion point keeps the link as 0.5 does) and XREF (? / Attach / Reload / Bind with WL.17's "clipped part only" /
  Detach; a link by name or by picking a reference), xclip.js's XCLIP (Rectangular / Polygonal / Delete, the clip in
  block coordinates). `cc-cad-geom` `xclip`: `insidePoly`, `clipLine`, `clipArc`, `clipRing`, `clipEntities`, and
  `ClipOut`, the walk output walk.js wraps a clipped reference's contents in — so the window, plots and extents clip.
- Files: the other drawing comes through `Host::pick_drawing` (`can_pick_files`, `Pick::{Now, Later}`, a new
  `Kind::File` while a picker that answers later is open, `Engine::deliver_drawing` / `deliver_files`). The host reads
  it into typed objects (`XSource`, as the JS reader hands over objects), so attaching moves them instead of parsing.
  Window: `Services::pick_files` (desktop: a file dialog filtered to DWG / DXF or pictures; web: the browser's picker,
  the files arriving in `Services::picked` — the web has no folders, as 0.5 in the browser); cadcraft-shot `--pick`.
  CLI: the `cmds` script's `files` (drawings as doc.load JSON or a path read with cc-io-cad; pictures as base64 or a path).
- Parity: kdiff (two new scenarios: XATTACH / XREF ? Reload Bind Detach / XCLIP on a link; XCLIP on turned and scaled
  block references) 135/135 records identical; idiff `--xrefs` (new: link the kit's base-plan and mleaders, XCLIP the
  first with a rectangle, bind the second, link base-plan again, clip it with a polygon, bind the clipped part; both
  sides save) byte-identical in DWG 2000 and 2018, DXF 2000 and DXF R12 on base-plan and the kitchen sample (the full kit sweep is in the M4.20 entry).
- Timings (drafttime, JS / Rust ms, best of 3, the drawing linked into itself): Carey (21,491 objects) XATTACH 16 / 21
  (0.8×: each object is copied once more for its undo step; JS keeps references), + XCLIP 23 / 31, + XREF Bind 35 / 21,
  + XCLIP + Bind clipped part 43 / 21, + Detach 53 / 21, XCLIP ALL references 17 / 19; ar1all (9,021) 83 / 23,
  20 / 22, 184 / 103, 277 / 196, 94 / 32, 14 / 13. The first cut parsed the picked drawing inside XATTACH (800 ms on
  Carey, 0.1×); the host now reads it into typed objects and attach moves them.
- Screenshot checked: base-plan linked into a new drawing, clipped to a polygon (columns cut at the boundary).

## 2026-10-20 — M7.15 BimCraft library items placed from the plan (agent, work/bimgrips)

- `cc-bim draw::place` (ifc-place.js `IfcPlacer.place`, `nextItemNumber`, `snapToWall`, `wallFaces`' minimum
  rectangle, `planPolylines`, `boxMesh`, `isClosed`; libitem.js `planBounds`, `boxFromPlan`, `canonicalJson`): a
  library item (format 1 JSON) placed into the open IFC model — one type per uuid@version or sha256 holding the Body
  (IFC4 IfcTriangulatedFaceSet; IFC2X3 IfcFacetedBrep / IfcShellBasedSurfaceModel; CC-RENDER-PARTS colours as styled
  parts), FootPrint and Clearance maps, each placement an occurrence relative to its storey with mapped items, the
  type / storey relations and the CadCraft_Placement / LibraryItem / Equipment / SourceAttributes psets; a failed
  placement leaves nothing behind. `Editor::place_item` is one change (`Change::Place`, "Desk (item 4)"), undo and
  redo (placed again, a new id) as ifc-edit.js. Job steps `placeItem`, `nextItemNumber` for `cadcraft-cli bimdraw`.
- `bimplace.rs` (bimcraft.js `startPlacing` / `moveGhost` / `commitPlace` / `askPlacement` / `typedPlace` /
  `toggleSnap` / `stopPlacing`): an item file opened or dropped on the edited model (or "Place in the model" in the
  edit bar) starts placing on the plan's level; the pointer carries the footprint and box, R turns 90°, Tab snaps the
  back to the nearest wall face (from the model's wall meshes, float32 like web-ifc), a click or typed X, Y asks for
  item number / quantity / notes (/ stand-in height) and places; Esc stops. The place bar under the model bar.
  Commands `bim.place.pick`, `.turn`, `.snap`, `.at`, `.ok`, `.cancel`, `.stop`.
- Parity (`forks/tools/bmlibdiff.mjs`): engine — the six LibraryCraft seed items and eight synthetic ones (meshes
  closed / open, parts, no body, every plan entity, every class mapping, shared types) on new buildings and the
  certification Building-Architecture model in IFC2X3, IFC4 and IFC4X3: 188/188 steps, 16/16 saved IFC
  byte-identical; app — four sessions (new building with a room; the IFC4 and IFC2X3 models), 20 placements (6
  snapped to walls), turns, typed points, a refused point, cancel, undo / redo: status lines 73/73, saved IFC and plan
  PDF 8/8 byte-identical to the frozen 0.5.
- Timings (`bmlibdiff.mjs --bench`, 20 ranges on the IFC4 model, after the last one; JS / Rust ms): the placement
  0.97 / 1.28 (JS median 2.64), 3D again 13.8 / 3.8 (3.6×), plan again 14.0 / 4.3 (3.2×); letting go 28.7 / 9.4 (3.1×).
- Next: the Library panel and its sources, swap / update, copy / paste of placements; M7.16 residential sync.

## 2026-10-20 — M7.14 BimCraft plan grips (agent, work/bimgrips)

- `bimgrips.rs` (model-ui.js `gripWall` / `onDown` / `gripMove` / `onMove` / `onUp`): with Select and one drawn wall
  picked, its ends (squares) and middle (a circle) are grips. An end drags with the wall tool's snapping (other walls'
  ends, middles and face corners; 15° steps from the fixed end; Shift square; free lengths in 10 mm or 1/2"), the
  middle moves the whole wall (90° steps from where it was taken); the preview is the wall joined to the others, its
  length and the snap marker. A press on a door or window slides it along its wall or onto another (the middle snaps,
  the rest in the round step; a door swings to the pointer's side on a new wall), its distances to the face's ends
  shown, the symbol out of the plan while it moves. 4 px or less is a click; too short a wall and openings that do not
  fit are refused with 0.5's words. Each drag is one change (one undo step), the dragged element stays picked; after
  any other plan change the pick stays what it was (0.5's `keepSel`, it used to jump to the new element).
- Gestures are commands: `bim.down`, `bim.move`, `bim.up`, `bim.drag=x0,y0,x1,y1[,shift]`, `bim.view=cx,cy,scale`
  (the plan's centre and zoom: the snap and pick tolerances are screen px), `bim.timings`. The mouse goes through the
  same commands (a press the plan takes no longer pans).
- Parity (`forks/tools/bmgripdiff.mjs`: model-ui.js's grip / drag / hover / hit code copied, the engine modules
  imported; 6 scripted scenarios — end grips with Shift and undo / redo, middle grips, feet-inches, too short, openings
  slid along / past the end / onto other walls / clicks, no-fit — and 8 seeded ones whose gestures are picked on the JS
  side from the plan of the moment): status lines 248/248, saved IFC and plan PDF byte-identical 28/28.
- Timings (`bmgripdiff.mjs --bench`, one inner wall end of a grid of rooms dragged in 30 moves and let go; JS / Rust
  ms): 71 walls — preview 0.40 / 0.11 (3.7×), change 29.1 / 12.3, 3D 48.8 / 4.8, plan 21.8 / 12.8, let go 99.7 / 29.9
  (3.3×); 262 walls — preview 2.11 / 0.73 (2.9×), let go 495 / 226 (2.2×).
## 2026-10-20 — M4.17 UCS and view turns, LWDISPLAY / POLARANG, COUNT (agent, work/cadcmds)

- `cmd/ucscmd.rs`: UCS (origin + X axis, NAmed Restore / Save / Delete / ?, OBject from a line / circle / arc /
  polyline / text / block reference / point / construction line / dimension, Previous (10, this session), World, Z,
  3point), TURNLEFT / TURNRIGHT / TURNWORLD, PLAN (Current ucs / a saved Ucs / World) — ucs.js `fromObject`,
  `threePoint`, `setUcs`; the Doc keeps `{ current, named }` as the savers write them, not an edit. LWDISPLAY and
  POLARANG ask the host's settings; COUNT (pick → select and count, list, export counts / attributes as CSV through
  `Host::save_blob`; count.js's CSV with formula-injection quoting).
- Window: UCS / PLAN / TURN* turn the view (`View2D.twist`) so the UCS X axis is level; POLARANG sets the step of
  polar tracking, now on F10 (`edit.polar`); COUNT's CSV goes through the Save service. LWDISPLAY says lineweights on
  screen are the browser's: the window draws one width (gaps item 3).
- Parity: kdiff scenarios 2002/2002 records identical with 3 new scenarios (UCS options / PLAN / turns, LWDISPLAY /
  POLARANG, COUNT with both exports compared byte for byte); the stubs on both sides now carry the page's settings,
  turnView and saveBlob. Final sweep on this commit: 20 random scripts each at seeds 3, 4 and 7, 2800/2800 each (seed 1
  with 40 scripts ran the JS harness out of Node heap, as noted for M4.14).
- Timings (JS / Rust ms, best of 3–5): Carey UCS Z 30 + TURNWORLD 3.6 / 0.2, PLAN + POLARANG + LWDISPLAY 5.9 / 0.1,
  COUNT list 4.8 / 1.1, export counts 6.1 / 1.8; ar1all 3.8 / 0.2, 6.3 / 0.1, 17 / 2.5, 66 / 4.2. COUNT export
  attributes was 0.8× on ar1all (11.6 ms against 9.3: `toFixed(6)` through the exact decimal expansion for 6,000
  numbers); a rounding path that is exact away from ties brings it to 3.5 ms (Carey 17 ms, JS 40–170 under load).
  The JS column is noisy on this server (the second run's JS times were 2–3× the first's).
- Screenshot checked: the kitchen sample turned 30° by UCS Z with COUNT's list.

## 2026-10-19 — M4.16 Sheets and viewports, named views (agent, work/cadcmds)

- `cmd/layout.rs`: LAYOUT (sheets.js `sheetContents`: border, title block on layer "Title block", one viewport onto
  the model at the largest standard scale that fits; `defaultPaper`, `aimViewport`, `bestScale`, `modelExtents` over
  cc-io-cad's papers and scales), MVIEW, VPORTS (on a sheet MVIEW; in the model the host's split panes), MSPACE /
  PSPACE (the host's viewport work), VPLAYER (? / Freeze / Thaw / Reset / Vpvisdflt; layer patterns with * and ?;
  All / Select / Current), VIEWLABEL (a text with `field: {k: 'vpview', vp}`), NAMEDVIEW (? / Restore / Save /
  Window / Delete, per tab). `cc-cad` `Doc::change` runs commands.js's view-label edit hook inside the change: a label
  follows its viewport's view name in the same undo step (`view_labels_follow_their_viewport_name`). The engine reads
  no clock: a new sheet's date comes from `Host::now_ms`.
- Window: the sheet tabs show while editing, with cadcraft.js's "+" (LAYOUT); a tab click opens that space in the
  drawing and LAYOUT / MSPACE / PSPACE move the tab; MSPACE shows the model zoomed to the viewport's window and PSPACE
  goes back to the sheet. VPORTS in the model says split panes are the browser's (the window has one pane).
- CLI: `cadcraft-cli save … --script cmds.json` runs a command script before saving (idiff.mjs `--sheets`); the cmds
  stub logs the page's hooks (setPanes, enterViewport / leaveViewport, viewportAt) as kdiff.mjs's stub does.
- Parity: kdiff scenarios (2 new: sheets and viewports, named views) all identical; idiff `--set kit --save --sheets`
  (each of the 78 kit drawings gets LAYOUT, VIEWLABEL, VPLAYER Vpvisdflt + Freeze, a second MVIEW, a sheet named view
  and a second LAYOUT + MVIEW on both sides, then saves): DWG 2000 / 2004 / 2007 / 2010 / 2013 / 2018, DXF 2000 and
  DXF R12 byte-identical 78/78 each (624/624), reports equal.
- Timings (JS / Rust ms, best of 3): Carey LAYOUT 40 / 19 (2.1×), LAYOUT + MVIEW 82 / 38 (2.2×), LAYOUT + VPLAYER
  Freeze * 41 / 19 (2.2×), NAMEDVIEW Save + Restore 6.4 / 0.1; ar1all 31 / 8.5, 46 / 18, 29 / 9.2, 6.6 / 0.1.
- Screenshot checked: a new sheet with its title block, a second viewport and its view label.

## 2026-10-19 — M4.15 Clipboard, QSELECT / SELECTSIMILAR / FIND, UNITS (agent, work/cadcmds)

- `cc-engine` `clip` (clipboard.js with blockcopy.js's `planCopy(…, {refs})` / `applyCopy(…, 'keep')`): `make_clip`
  (objects without ids, `field` or readers' `_` keys; every block they reach, nested first; their layers' settings;
  viewports stay on the sheet), `read_clip` (the untrusted-store validator: format, version, limits, entity types and
  fields, finite numbers, colours, layer and block names, references and block loops, attribute tags; depth and node
  caps), `Clipboard` (this engine's copy plus the host's shared store: `Shared::Text` is read through `read_clip`,
  `Shared::Clip` is a copy made in this window), `base_point`, `paste_preview`, `paste_clip`. Commands COPYCLIP CUTCLIP
  COPYBASE PASTECLIP PASTEORIG (`cmd/clipcmd.rs`). Not carried: constraints (gcons.js `clipConstraints`; the fork
  solves none).
- `cmd/qselect.rs`: qselect.js (`QS_PROPS`, `propsFor`, `quickSelect`, `valuesOf`, `similarTo`, wildcards, ICU-like
  numeric `localeCompare`) and findtext.js (`findMatches` with match case / whole words / wildcards, `applyMatches`);
  QSELECT SELECTSIMILAR (Settings toggles) FIND on the command line; `cmd/units.rs` UNITS (`setLengthFormat`, the
  format phrase and example). Host hooks: `clip_put` / `clip_get`, `paste_base`, `quick_select`, `find_dialog`.
- Window (`ui-egui`): one clipboard for every drawing the window opens (copy in one, paste in another); Ctrl+C / X / V
  and Ctrl+Shift+V run the clipboard commands while the command line is empty; QSELECT and FIND open the Quick select
  and Find and replace dialogs (`qsdlg.rs`, cadcraft.js's: type with counts, property, operator, value list or text,
  include / exclude, append, a live count; find / replace with the options, the look-in kinds and a live list),
  every control a `qs.*` / `find.*` command. 0.5 has no Units dialog: UNITS is the command line.
- Parity: kdiff (seed 1) scenarios 1928/1928 records identical on this commit with 7 new scenarios (clipboard ×2,
  QSELECT, SELECTSIMILAR, FIND, UNITS ×2); 30 random scripts at seed 7 4200/4200.
- Timings (drafttime, best of 3; JS / Rust ms): Carey Foodservice (21,491 objects) COPYCLIP ALL + PASTEORIG 261 / 67
  (3.9×), + PASTECLIP 363 / 107 (3.4×), CUTCLIP ALL + UNDO 153 / 31 (5.0×), QSELECT layer 14.5 / 8.1 (1.8×),
  SELECTSIMILAR 71 / 29 (2.5×), FIND select 10.8 / 6.6 (1.6×), FIND replace + UNDO 23 / 15 (1.6×), UNITS 3.1 / 0.6;
  ar1all floor plan (9,021) 244 / 38, 290 / 65, 223 / 23, 8.8 / 4.3, 27 / 9.4, 3.2 / 2.0, 4.0 / 2.2, 2.9 / 0.6. The
  first cut was slower than 0.5 on CUTCLIP (0.9×) and FIND replace (1.0×): every copy serialised the clip as JSON for
  a store the stub never reads, and FIND copied every string and grouped hits quadratically; now the store takes the
  clip itself, plain searches run in place over the text and hits group through an index.
- Screenshots checked (cadcraft-shot on the kitchen sample): the Quick select dialog, Find and replace with a live list,
  UNITS on the command line.

## 2026-10-20 — M4.14 and M11.11–M11.14 merged and deployed

- On the Rust repo's build: M4.14 kdiff seed 3 all same, propsdiff 720/720, plotdiff 32/32, mcpdiff 0 different
  + 2 known (LISP); M11.11–M11.14 cvtooldiff 76/76 projects, 2,084/2,084 status lines, 20/20 files; gate green both
  times; live at builds/20261019T235801Z. Fork jobs share one 2.5-core cgroup slice (`forkrust.slice`, owner
  2026-10-19), at most two agents building. Work copies, their target dirs and old deploys deleted after each merge.

## 2026-10-19 — M11.11–M11.14 CivilCraft sheets, mass haul, WL.20 / WL.23, Properties edits, lasso (agent, work/civilsheets)

- M11.11 (`civilsheets.rs`): the alignment's Sample lines block opens Section sheets (PDF) (paper, scale,
  exaggeration, live page count and notes, Download PDF through io-civil `section_sheets` + `to_pdf_pages`) and Mass
  haul (shrink or swell, the diagram, balanced stretches, what is left over, CSV). Commands `civil.xs*`, `civil.mh*`.
  The frame now runs every command a button asks for (it ran only the last one).
- M11.12: feature lines in the profile (WL.20: crossings as diamonds with "name elevation", runs within a distance
  dashed and named, the Feature lines button and "Alongside within" field saved in the project's display, a crossing
  mark selects its line) and the "why" blocks (WL.23: curves and vertical curves that do not fit, under-minimum radii,
  each fix a button, one undo step, its label said), the design minimum radius. Commands `civil.profile.fl`,
  `.within`, `.pickfl`, `.fl.info`, `civil.why`.
- M11.13 (`civilpropedit.rs`): `civil.prop=<field>=<value>` and typed fields in Properties for the alignment (name,
  start, corridor and its assembly, superelevation Δ and rate table), pad, structure, pipe (and Auto inverts), feature
  line (name, use, closed, daylight and slopes, vertex elevations and grades, Raise, From surface, Even grade, the
  stepped offset), parcel (name, point of beginning, course radius and turn), one survey point (number, E, N, Z, D), and
  with nothing selected the project's name, units, longest edge and the network's type and rules; the Site block's
  name and units edit the project.
- M11.14 (`civillasso.rs`): Shift+drag stays a box until it bends 12 px off its line, then a lasso whose mode follows the
  first sideways move; Space cycles window / crossing / fence, Esc cancels (`civil.lasso` headless); Settings'
  point fields (row order, numbers for pasted points, start) in the left panel and `civil.settings`.
- Parity (`forks/tools/cvtooldiff.mjs`, 19 scenarios × 4 sites, 6 new): 76/76 projects, 2084/2084 status lines, 20/20
  exported files (section PDFs compared inflated, mass haul and section CSVs byte for byte) identical to the frozen 0.5.
- Timings (`forks/tools/cvsheetbench.mjs`, 500k-point survey, 107 sections, best of 3): lasso 8.5 ms vs 44 ms (5.2×);
  mass haul, feature-line profile and why under 0.1 ms on both; section sheets PDF 128 ms vs 58 ms (0.5×: layout 3.7
  ms, the rest is io-civil's PDF page writer, an engine follow-up).
- Next: superelevation's per-curve table and diagram, the faster PDF page writer, the import dialog, GIS layers.
## 2026-10-19 — M4.14 Whole-drawing edits share each object's extra data (agent, work/cadshare)

- `cc-cad`: `Entity::extra` is now `Extra`, an `Arc<Obj>` shared copy-on-write (reads through `Deref`; `insert`,
  `remove`, `get_mut`, `make_mut` copy only when shared; removing a key that is not there copies nothing). A clone of
  an object for a preview, an edit's before / after or an undo op no longer deep-copies its xdata, extension
  dictionaries and unknown codes (8,995 of the floor plan's 9,021 objects carry `xd`), as the JS shares them.
  `Doc::update` / new `replace_in` move the old object into the undo op instead of copying it and store the new one
  once (update returns a bool; `update_in` still gives the object back); `add` and `load` make missing layers without
  copying a `Layer`; entity spaces hash their u64 ids with a one-multiply hasher (`omap::IdBuild`; maps keyed by names
  keep SipHash); `OMap::get` / `contains_key` take borrowed keys (`&str` for layer names, no `to_string` per lookup).
- `cc-engine`: MOVE / COPY and MATCHPROP read the object in place instead of cloning it first; `Host::preview` takes
  the preview `Vec` (the window kept a copy of every rubber-band object). `ui-egui`: the selection highlight and the
  rubber band draw borrowed objects (SELECT ALL drew a copy of the whole drawing every frame); a selection window when
  idle is linear (it scanned the selection per picked id). `cc-cad-geom`: `walk` / `walk_batches` take `&[E: Borrow<Entity>]`,
  so the display build groups references instead of copying every object.
- Undo stays op-based with O(1) shares per object. Tests: `crates/cad/tests/doc_tests.rs`
  `extra_keys_shared_through_edits_and_undo`, `omap` `replace_keeps_place_and_ids_hash_fast`.
- Parity on this build: kdiff (seed 1, 22 scenarios + 8 scripts) 2478/2506, report identical line for line to the
  build before the change (the 28 are one random script where the JS harness itself throws `RangeError: Invalid string
  length` stringifying a REVCLOUD preview at 4·10⁸ coordinates); seeds 3 and 4 1120/1120 each (seed 2: the JS side ran
  out of Node heap); propsdiff 720/720; plotdiff 32/32; mcpdiff 297 messages, 0 different, 2 known (LISP); idiff --set kit
  --save: 78 files 0 differences, DWG 2000–2018 and DXF 2000 saves byte-identical 78/78 each; the two Carey drawings saved
  as 2000 / 2018 / DXF 2000 byte-identical to the build before.
- Timings (`forks/tools/drafttime.mjs`, best of 3, 1-core cap; before = 391d059 on the same run conditions):

  | Drawing | Command | 0.5 JS ms | Rust before | Rust after | × JS |
  |---|---|---:|---:|---:|---:|
  | ar1all floor plan (9,021) | SELECT ALL + MOVE | 42 | 219 | 15 | 2.7× |
  | | ARRAY 2 × 1 of ALL | 132 | 211 | 26 | 5.0× |
  | | EXPLODE ALL | 132 | 110 | 38 | 3.5× |
  | | MATCHPROP onto ALL | 68 | 110 | 22 | 3.1× |
  | | ERASE ALL + UNDO | 12 | 29 | 7.7 | 1.6× |
  | | STRETCH / LIST / LAYISO / HATCH | 19–92 | 7–74 | 7–34 | 1.4–7.4× |
  | Carey Foodservice (21,491) | SELECT ALL + MOVE | 60 | 116 | 36 | 1.7× |
  | | ARRAY 2 × 1 of ALL | 191 | 122 | 69 | 2.8× |
  | | EXPLODE ALL | 103 | 129 | 89 | 1.2× |
  | | MATCHPROP onto ALL | 142 | 96 | 46 | 3.1× |
  | | ERASE ALL + UNDO | 18 | 28 | 15 | 1.2× |
  | | STRETCH / LIST / LAYISO / HATCH | 23–181 | 9–97 | 10–85 | 1.6–5.1× |

  The JS column is noisy (V8 warm-up; 21–70 ms for the floor plan's MOVE across runs); M4.10's table was taken at
  load 30–50 and is not comparable.

## 2026-10-19 — wave 3 merged; CPU cap on the VPS

- Merged into the Rust repo and deployed: M6.17 ModelCraft panels (mcuidiff on this build 1,200/1,200 status lines,
  1,243/1,243 files), M14.3 DetailCraft tools (558/558 + sample 58/58), M13.3–M13.4 PartCraft sketcher, features,
  threaded rebuilds, configurations (257/257 steps, 27/27 files), M4.10–M4.12 drafting, Properties, plot (on this build:
  propsdiff 720/720, plotdiff 32/32, mcpdiff 0 different + 2 known LISP, gate green; kdiff was stopped by the
  server's low-memory guard before finishing; the agent's run was 1,386/1,386 + 8,600 random records; rerun owed).
- The VPS host caps the whole server at 30 % CPU after sustained load (2026-10-18). Fork builds, gates and harnesses
  now run one at a time under a 1-core cgroup cap (`systemd-run --scope -p CPUQuota=100% … nice -n 19`); the move to
  a 16-core WSL2 machine is planned. Disk: stale target output, harness temp dirs and old deploys pruned (95 % → 63 %).
- Next: share each object's extra data in cc-cad (MOVE / ARRAY / MATCHPROP / ERASE + UNDO on the 9k floor plan run
  0.1–0.6× the JS), then the gaps list.

## 2026-10-18 — M4.12 Plot to PDF and `drawing_plot` (agent, work/drafting)

- `crates/io-cad/src/plot.rs`: plot.js — papers (ISO / ANSI / ARCH), layout sheets and the model (extents or a window),
  fitted or typed scales (`1:50`, `2:1`, `1/4" = 1'-0"`), monochrome or colour, pen width, linetypes, hatches, texts
  (WinAnsi, missing characters counted), viewports clipped; PDF 1.4 with deflated streams (miniz) and agents.js
  `plotPdf`'s report. `cadcraft-cli plot`, the Plot dialog in the window (PLOT / PRINT / EXPORTPDF, Ctrl+P: area,
  paper, orientation, scale, ink, line weight, a live preview, Pick a window through `_PLOTWIN`, Download PDF), and the
  MCP tool `drawing_plot` headless and in the window (it said "not ported").
- Parity (`forks/tools/plotdiff.mjs`, plain streams byte for byte + the deflated stream inflates to them, report
  compared): 32/32 plots identical on the default set; the kit corpus (78 drawings, every layout and the model, mono and
  colour): 1128/1128 identical. The sheet report rounds as agents.js does (`num`, 1e-9). Rust tests: `crates/io-cad/tests/plot.rs`. `mcpdiff.mjs` (now calling `drawing_plot`, its known plot difference removed): 297 messages, 295 the same, 0 different, 2 known (LISP).

## 2026-10-18 — M4.11 Properties panel (agent, work/drafting)

- `crates/engine/src/props.rs`: propdefs.js / propexpr.js — groups (All + one per type), General / Geometry / Text /
  Pattern / dimension / Misc sections per type, *Varies*, each change one `PROPERTIES` undo step (objects on locked
  layers left as they were), the number reader (units, feet and inches, `45d30'`, `0.5r`, `=2*45` through cc-part's
  expr.js port). PROPERTIES (PR, PROPS, CH, MO, DDMODIFY), PROPERTIESCLOSE, QPMODE; the panel on the right (Ctrl+1),
  commands `props.open/close/group/vertex/set`; `cadcraft-cli props` for the harness.
- Parity (`forks/tools/propsdiff.mjs`, groups, defs, values, applied result, document, undo): 720/720 records
  identical (12 scripts × 60 steps). Rust tests: `crates/engine/tests/props.rs`. Not ported: the dynamic-block Custom section, Attributes, Library, sheet / view
  sections, Quick Properties' pop-up, the point pick buttons.

## 2026-10-18 — M4.10 Drafting commands past the M4 set (agent, work/drafting)

- Ranked list of 0.5's 190 commands in `docs/gaps.md` item 1. Ported at parity: HATCH with pick-inside boundaries
  (hatch.js `boundaryAt` → `cc_cad_geom::hatch::boundary_at`: islands, open / none / too large), the DIM family
  (linear, aligned, radius, diameter, angular, continue, baseline, DIMSCALE … DIMLUNIT), FILLET, CHAMFER, EXPLODE,
  STRETCH (the windows of "Select objects"), ARRAY (rectangular, polar, path-less), BREAK, BREAKATPOINT, LENGTHEN, JOIN,
  MATCHPROP, XLINE, RAY, MTEXT, REVCLOUD, MLEADER, TEXTEDIT, BLOCK, ATTDEF, INSERT, BEDIT / BCLOSE (the block editor in
  its own space), PURGE, DUPLICATE, SELECT / SELECTALL / DESELECT, DIST, AREA, MEASUREGEOM, ID, LIST, the layer tools
  (LAYISO … LAYDEL), RENAME, LINETYPE, LTSCALE, LWEIGHT. Edits stay display-incremental (the block editor's space swaps
  in the model slot).
- `IdSet` (the selection set) had a linear `has`: ALL on 21,000 objects took 200 ms+ per call; now hashed.
- Parity (`forks/tools/kdiff.mjs`, every record: drawing, printed lines, prompts, previews, selection, undo):
  22 scenarios 1386/1386 identical; random scripts over every ported command, seeds 1–7: 5600 + 3000 / 8600
  identical. Rust tests: `crates/engine/tests/drafting.rs` (14 cases from bin/test-cadcraft.mjs).
- Timings (`forks/tools/drafttime.mjs`, frozen JS engine vs `cadcraft-cli cmds`, best of 3, machine at load 30–50):

  | Drawing | Command | 0.5 JS ms | Rust ms | × |
  |---|---|---:|---:|---:|
  | Carey Foodservice (21,491 objects) | HATCH pick inside | 585 | 12 | 51× |
  | | SELECT ALL + MOVE | 1008 | 702 | 1.4× |
  | | STRETCH (crossing ¼) | 896 | 387 | 2.3× |
  | | ARRAY 2 × 1 of ALL | 2306 | 994 | 2.3× |
  | | EXPLODE ALL | 3215 | 1891 | 1.7× |
  | | MATCHPROP onto ALL | 4697 | 1722 | 2.7× |
  | | LAYISO + LAYUNISO | 3090 | 212 | 15× |
  | | LIST ALL | 3103 | 3092 | 1.0× |
  | | ERASE ALL + UNDO | 299 | 202 | 1.5× |
  | ar1all floor plan (9,021 objects) | HATCH pick inside | 905 | 13 | 71× |
  | | SELECT ALL + MOVE | 324 | 1623–5511 | 0.1–0.2× |
  | | ARRAY 2 × 1 of ALL | 1000 | 1602 | 0.6× |
  | | MATCHPROP onto ALL | 498 | 813 | 0.6× |
  | | ERASE ALL + UNDO | 86 | 221 | 0.4× |
  | kitchen-equipment (254 objects) | HATCH (whole drawing) | 16608 | 7698 | 2.2× |
  | | the other eight | 5–23 | 0.5–19 | 1.0–10× |

  The slow rows: every object is deep-copied (its extra JSON fields) for each preview and each change, where the JS
  shares nested data; perf shows serde_json Value / indexmap clones. Fix belongs in cc-cad (shared extras), listed in
  gaps.
## 2026-10-18 — M13.4 PartCraft rebuilds on a thread, configurations (agent, work/partsketch)

- Rebuilds of parts whose last rebuild took over 60 ms go to a thread in the desktop window (`BACKGROUND_REBUILDS`, set
  by apps/cadcraft): the build chip says "Rebuilding…", the window keeps drawing, and as in partcraft.js's exact-kernel
  path the newest part wins (an edit during a rebuild sends the part again when the thread returns; a stale result is
  dropped). Headless runs, the web build and sketch / pick modes rebuild in place. Test: two edits during one thread
  rebuild end with the newest part built.
- Parameter edits follow the configurations (configs.js `followParams` in params-panel.js `edit`), and the Parameters tab
  has 0.5's Configurations: rows with Apply / Update / Delete and rename, save the current values under a name, Export
  as a design table (CSV). Command `part.config` (save / update / apply / rename / delete / export).
- Parity: the `configs` scenario joins pcuidiff: 7 scenarios, 257 steps and 27 files byte-identical to 0.5.

## 2026-10-18 — M13.3 PartCraft sketcher and feature tools (agent, work/partsketch)

- `crates/ui-egui/src/partsketch.rs`: sketchview.js in Rust — the sketch editor over the part view with 0.5's tools and
  gestures: select (click, Shift+click, window, drag a point / curve / circle edge / dimension label with the solver live),
  line (chains, snaps to points and curves, 3° horizontal / vertical), centerline, rectangle, circle, arc (follows the turn),
  point, polygon, slot, spline / control spline (click the first point to close), trim, extend, offset (B: both sides),
  mirror, linear / circular pattern, fillet, dimension (aligned / horizontal / vertical from where the label goes; type the
  value), paste; value boxes (`Ask`), keys (L N R C A P D F S B T E O M Q, Esc, Enter, Delete). Every edit is one undo step
  through partcraft.js's `sketchCommit` (solve, conflicts said, rebuild up to the sketch); `partsketchui.rs`: the sketch tool
  bar, the sketch panel (state, legend, relations / dimensions for the selection, the relations list, numeric entry, Exit /
  Fit) and the 2D stage (the model's edges projected onto the sketch plane, the value box).
- `partview.rs` / `partfeat.rs`: new part, base planes in the history, Sketch on a plane / picked flat face / selected sketch,
  exit; feature tools Extrude, Cut, Revolve, Revolve cut, Hole, Linear / Circular pattern, Mirror, Fillet, Chamfer, Shell
  (edges and faces picked in the view or with Pick edges / faces, Shift+click collects; picking mode shows the part before
  the feature), the feature panel's fields (operation, end, depth, reverse, angle, axis, hole type / diameter / through /
  counterbore, seeds, direction, spacing, count, mirror plane, radius, distance, wall, outward, drop a reference), sketch
  dimensions, delete (asks when features depend on it), move up / down, rename — each one undo step with 0.5's labels.
  Commands: `part.new`, `part.plane`, `part.sketch`, `part.sketch.exit`, `part.sk.*` (tool, view, click, move, drag, dbl,
  right, key, value, enter, rel, select, all, none, construction, unrel, edit, num, fit), `part.feature`, `part.set`,
  `part.dim`, `part.delete`, `part.move`, `part.pick`, `part.pick.done`, `part.edge`, `part.times`.
- Parity: `forks/tools/pcuidiff.mjs --scenario all` drives the app and the frozen page logic (pchost.mjs: partcraft.js's
  host code with the real sketchview.js on a fake canvas) with the same commands: 6 scenarios, 241 steps, every status line
  and 22 saved parts / STL / 3MF / STEP / IFC files byte-identical (0 differences).
- Plain English where 0.5 has no key: the sketch view report of `part.sk.view`, `part.times`, "select sketch geometry first"
  style command errors.
## 2026-10-18 — M14.3 DetailCraft's modelling tools (agent, work/detailtools)

- M14.3: `crates/ui-egui/src/detailtools.rs` (tool state and detailcraft.js's actions on the engine) and
  `detailprops.rs` (the panels), hooks in `detailview.rs` / `detailpanel.rs` / `detail3d.rs`, `View3::lock_orbit`.
  Placing beams, columns (up to the next level or a chosen one), braces, plates, footings and stairs by clicking grid
  intersections (snap within 18 points, marker, rubber band, the intersection's name) or with the tool options' "Add by
  grid"; tool options as 0.5 types them (profile, material, position, rotation, plate size, footing mat and starters,
  stair sizes; US / EU steel from Settings). Connections: end plates, shear tabs, base plate, splices, gussets, Apply to
  similar, every parameter in Properties (choices, numbers, grades). Several selected (Shift+click, Ctrl+A, selection
  window and crossing in plan / elevation or Shift+drag in 3D, lasso with Space cycling the mode), copy / cut / paste on
  a grid intersection (ghost follows the pointer), by offset or in place, duplicate, delete; "Move, rotate, mirror" by
  form or by picking two intersections (M), "Change on all of them". Left panel: grids, levels, phases (show, rename,
  add, remove, current, marks per phase), custom profiles; holes and copes editors; footing and stair fields; the
  Settings window (steel, offset, copy connections, bolts, drag lasso, sheet size, KISS sizes, bar names, shape codes);
  plan (looking down on a level, 1.5 m above cut) and elevation (north at a lettered line, ±1 m) views. Keys as 0.5
  (V B C R P F S M, Esc, Delete, Ctrl+Z/Y/C/X/V/D/A, Ctrl+Shift+V). Every edit is one step on the engine's undo stack.
  Fixed on the way: the stair's status line lacked its rise and width; a click on nothing no longer says "Nothing
  selected" (0.5 says nothing).
- Commands (all headless): `detail.tool`, `detail.node`, `detail.opt`, `detail.bygrid`, `detail.act`, `detail.paste`,
  `detail.xf`, `detail.many`, `detail.grids`, `detail.levels`, `detail.phases`, `detail.hole`, `detail.cope`,
  `detail.profile`, `detail.view`, `detail.window`, `detail.lasso`, `detail.settings`; `detail.set` also takes footing,
  stair and connection fields; `detail.select` takes lists and `+ref`.
- Parity (forks/tools/dcuidiff.mjs `--steps-file`, dcdiff/page.mjs = detailcraft.js's functions without the DOM; steps in
  forks/tools/dcdiff/steps/): the status line after every step, then the saved project and every export. place 45 steps
  100/100, +connections 76 steps 135/135, +editing 120 steps 179/179 (window and crossing selections included),
  +setup 89 steps 144/144: 558 comparisons, 0 differences.
- Timings on testfiles/generated/detail/grid24x12x5 (2820 members, 3048 connections; forks/tools/dcuitoolbench.mjs,
  best of 3, machine at load 25–30 so both sides are noisy): place a beam JS 2101 ms vs Rust 1103 ms (+498 ms 3D scene);
  a column 976 vs 648; change a profile 1287 vs 786; window select a quarter 714 vs 387; move a column 6311 vs 2191; copy +
  paste in place 4000 vs 2131. The cost on both sides is the full rebuild after an edit (engine build + marks); the JS
  times leave out three.js's scene rebuild, the Rust 3D scene is listed apart. "Duplicate everything" ran past an hour in
  the JS harness and was left out.
- Not done: dragging selected parts (D2), the view cube / axis icon, exact clipping of parts crossing a plan's or an
  elevation's band, settings kept between runs, the clipboard between windows other than through the system clipboard,
  the drawings dialog's preview and issue log, es-MX.
## 2026-10-18 — M6.17 ModelCraft's panels (agent, work/modelpanels)

- `crates/ui-egui/src/mcact.rs` (actions), `mcpanels.rs` (the side panel), `mcprint.rs`, `mcmats.rs`: 0.5's panels in the
  app, each edit one undo step through the cc-m3d functions modelcraft.js calls, with its undo labels and status words:
  Entity info (renderInfo: model counts and size, face / edge / curve / group or component / dimension / section /
  guide / several, typed lengths for edges, radii, dimensions and sizes, name, tag picker, solid yes / no and volume,
  the solid checker's Show problems and Fix, Soften / Harden all, Edit, Explode, Make unique, Make component), the
  Outliner (nested rows, search, eye, F2 / right-click rename, drag a row onto a group or the top to move it, crumbs and
  Close group), Components, Colors (palette, another colour) and Materials (the 34-material library in its 11
  categories; paint as `mat:<id>`, shown in the library colour), Tags (untagged row, eye, rename in place, delete,
  add), Style (faces, edges, sun shadows, date, time, latitude, longitude, UTC offset), Scenes (add, go, update,
  rename, delete, previous / next), Hide / Isolate / Unhide all, and Check for 3D printing (print3d's report in the
  panel with Download STL / 3MF of the checked mesh).
- Everything is `model.edit=<action>` (select:, tree-eye:, tree-move:, tag-rename:, paint:, size:, soften:, scene-rename:,
  sun:, cam:, units:, print-check, …; mcact.rs's doc lists the forms). File › Save writes 0.5's file (model, `view`
  camera and style, images); opening a `.modelcraft` file restores its view and images.
- Keys as modelcraft.js: Shift locks the current inference while held, Alt / Ctrl reach the tools, arrows lock an axis
  only while a lockable tool is busy (else orbit, Shift pans) with sayLock's words, Esc steps back (tool → Select →
  selection → open group). A double-click that opens or closes a group runs the page's hooks (message, selection).
- Library materials export, print and show as their colour once the app registers them (`cc_io_m3d::util::set_material_colors`,
  as the page's setMaterialColors; the CLI and engine tests leave it unset, as io.js under Node).
- Fixes: theme toggle no longer reopens the model as it was first opened (edits were lost); a model opens with one
  scene build instead of three and no copy; undo / redo say 0.5's "Undone: …" and keep the selection that survives.
- `forks/tools/mcuidiff.mjs`: a seeded walk over 10 inputs (5 ModelCraft files, 5 SketchUp imports) × 5 seeds × 80
  panel actions, replayed in the frozen JS page functions and in the app: 4,000 / 4,000 status messages and 4,076 /
  4,076 saved files (each step's `.modelcraft` plus the STL / 3MF downloads) byte-identical. `cadcraft-shot mcbench` and
  `mcuidiff.mjs --bench` time open / panel edits / save / 3D-print check (ROADMAP).
- Plain English where 0.5 has no key: the Outliner's search glyph, eye / delete / rename glyphs (●, ◌, 🗑, ✎, ⟳).

## 2026-10-18 — M16.6 Studio merged (lead)

- Merged M16.1–M16.5 (control channel, `cadcraft-cli mcp`, live tools, cc-live, start screen). M16.6: the control
  channel's MarkupCraft / TakeoffCraft tools follow the plan sets of M9.15 (every open PDF's sheets as `<file id>:<page>`,
  in file order). On main's build: mcpdiff 292 messages, 289 the same, 0 different, 3 known (plot.js, LISP); live tool
  definitions 43, 0 different.

## 2026-10-18 — M16.5 CadCraft Studio's start screen (agent, work/studio)

- `crates/ui-egui/src/hub.rs`: with nothing open the window shows Studio's hub: the eight apps as tiles in their layer
  colours with role and short text from the 0.5 site catalog (`assets/lang/catalog`), an Open button that opens each
  app's sample (CadCraft also "New drawing"), SuperCadCraft as "coming later", recent files with app filter chips and
  relative times, the Open-a-file card and the Help topics from Studio 0.5's hub catalog (`assets/lang/studio`, en and
  es-MX, from studio@57640c4). Commands `hub.sample`, `hub.recent`, `hub.filter`, `hub.lang`, `hub.home` (the CadCraft
  button in the app bar). Recent files persist in `recent.json` in the config folder (desktop). Narrow windows stack.
- Plain English where 0.5 has no key: "New drawing", relative times ("just now", "3 minutes ago"), tooltips.
  Screenshots: hub dark / light / with recent files / 800 px wide.

## 2026-10-18 — M16.4 Studio Live protocol core (agent, work/studio)

- `crates/live` (cc-live, L3): LIVE-PROTOCOL.md version 1 — our own bounded MessagePack (live-client.js's encoding rules),
  the `{t, s, c, n, q, b}` envelope, the session core deciding edits from `touched` (roles, locks, holds, property /
  structural / create / delete / read rules, delete wins, guarded reads, refusal `by`) as Studio's session server does, and a
  headless client (numbered pending edits as op records `{command, params, base_revision, result_revision}`, commits in
  order, key versions, refusals, `hello {resume}`).
- The shared vectors pass: 54 envelopes encode to the same bytes and decode back, 4 other encodings read, 6 bad inputs
  refused, all 11 scenarios decide as expected; a copy in `crates/live/tests/vectors.json` is checked against the site's.
  Fuzz target `live_frame` (60 s clean). Not built: the WebSocket transport, the PAKE handshake and sealed frames, snapshots.

## 2026-10-18 — M16.3 the window's live tools (agent, work/studio)

- The control channel's window answers with 0.5's live tools for the app it shows (data/live-tools-0.5.json from the frozen
  agents.js; 43 definitions checked by mcpdiff, 0 different): CadCraft (describe, list, get, check, selection, view,
  command_run as one `Assistant:` step with the `[Assistant]` history line, undo, save in place), TakeoffCraft, MarkupCraft
  (markup_add), ModelCraft (box / cylinder / move / erase / undo on cc-m3d); DetailCraft lists its tools with app_info
  only. Not yet: drawing_plot, view_screenshot, DetailCraft's other 11. `cc_mcp::{cad,takeoff,markups,modelcraft}::live_run`
  hold the logic; apps/cadcraft/src/control.rs only maps the window's state to them.

## 2026-10-18 — M16.2 the headless MCP server (agent, work/studio)

- `cadcraft-cli mcp`: 0.5's stdio MCP server (tools/mcp/server.mjs) with the 22 headless tools of tools.mjs, definitions byte
  for byte; `--root` / `--automation-read-root` / `--automation-write-root`, confinement on by default (the current folder),
  `--allow`, `--log`, `--verbose`, `CADCRAFT_MCP_ROOTS` / `_ALLOW`; `--connect PORT --token-file F` relays to a running app.
  `cc_mcp::cad` ports CadCraft's agents.js (describeDrawing, listObjects, objectInFull, checkDrawing, runInput with one
  `Assistant: …` undo step, undoSteps) plus plot.js extentsOf, sheets.js scaleLabel and commands.js describe; `ifc`, `takeoff`
  and `markups` port ifc.mjs, TakeoffCraft's and MarkupCraft's agents.js.
- Parity (`forks/tools/mcpdiff.mjs`, frozen 0.5 server vs ours, one script): 249 messages, 246 the same, 0 different, 3
  known (drawing_plot: plot.js not ported; LISP not ported, and the drawing that LISP line was in); 223 / 243 answers byte
  for byte, the rest equal as JSON (key order inside drawing objects). Written DXF / CSV byte for byte, XLSX entry by entry.
- Time over the script (release, same machine): JS 5,592 ms, Rust 2,298 ms (2.4×); drawing_open 3.2×, objects_list 4.0×,
  drawing_save 1.4×, ifc_properties 2.4×, markups_list 59× (the JS figures include loading pdf.js on first use).

## 2026-10-18 — M16.1 the control channel (agent, work/studio)

- `crates/mcp`: the dispatcher (0.5 tools/mcp/core.mjs, answer for answer: MCP + control-channel methods, the JSON
  Schema subset with 0.5's messages, permission groups, the call log), JS-exact JSON text, the local HTTP control
  channel with desktop-shell control.rs's rules on our own HTTP reader (127.0.0.1, token 0600 in the config folder,
  constant-time compare, Host check, Origin refused, JSON only, 4 MB, Mcp-Session-Id), file confinement (paths.mjs +
  AGENTS § 9), and the window's side (Studio's `windows_list` + routing, -32001 / -32002, busy refusals).
- `cadcraft --control [PORT] [--allow …]`: requests answered on the UI thread through a queue. 18 tests (auth, bad
  token, Origin, Host, non-JSON closes, 4 MB cap, sessions, confinement, the whole path into a UiApp); fuzz targets
  `control_request`, `mcp_message` (60 s each clean).

## 2026-10-18 — merges and a hayro-syntax fix (lead)

- Merged M9.13–M9.15 (MarkupCraft / TakeoffCraft panels, tools, several PDFs, compare) M7.12–M7.13 (BimCraft IFC
  edit + save, storey plan, Draw tools, schedules) and M11.7–M11.10 (CivilCraft tools, profile and section panels, points). On main's build: tkopsdiff 17/17, mkuidiff 6/6 identical.
- M9.16: the `markup_pdf_read` fuzz target hit a panic in hayro-syntax 0.8.0 (`ArrayIter::next` unwraps an item it had
  skimmed as valid; reached from page loading, so the crate-side workarounds in jsdoc.rs could not cover it). The crate is
  vendored in `vendor/hayro-syntax` with that unwrap returning the `Option`, via `[patch.crates-io]`; regression test
  `array_that_does_not_read_back` (panics without the patch) and the input as a fuzz seed.

## 2026-10-18 — M11.10 CivilCraft survey points: selection, copy / cut / paste / duplicate, Delete (agent, work/civiltools)

- M11.10: `crates/ui-egui/src/civilpoints.rs`: several survey points selected (Shift+click, Shift+drag a box, Ctrl+A,
  Esc clears; one picked point is the selected point with its Properties and grip), Copy (Ctrl+C: the copy in the app
  and P,N,E,Z,D rows on the system clipboard), Cut (Ctrl+X), Paste (Ctrl+V: point rows copied elsewhere paste too) and
  Duplicate (Ctrl+D) carry the points under the pointer — crosses where they would land and a line from where they are —
  until a click places them (Enter = at their own coordinates, Esc cancels), Paste in place (Ctrl+Shift+V); clashing
  numbers renumbered (clip.js `renumber`), one undo step each, the new points selected; Delete removes the picked
  points. Toolbar Copy / Paste. civilcraft.js copyPoints / paste / startPlacing / commitPaste / boxSelect words.
- Commands: `civil.pts` (all | none | toggle:x,y | box:x0,y0,x1,y1), `civil.copy`, `civil.cut`, `civil.paste`
  (inplace), `civil.duplicate`. `cadcraft-shot --mods shift|ctrl` (held for the pointer and keys that follow) and
  `--event copy|cut|paste:TEXT`: the real Shift+drag box, copy and paste events checked in a shot.
- Parity: `cvtooldiff.mjs` gained a points scenario (nothing to copy / duplicate / paste, toggles in and out, copy +
  paste with renumbering, duplicate cancelled then placed in place, undo, boxes with and without points, cut + paste in
  place, a selected point dragged and deleted, two picked points deleted, all copied and pasted far away):
  **52/52 projects byte-identical, 908/908 status lines, 4/4 files** to the frozen 0.5 JS.
- Speed on the 500k-point survey: select all and frames with every point picked 6–11 ms; pasting 200 000 copied points
  (clip.js's cap) 1.3 s for the edit, then the surface of 700 000 points rebuilds (7.7 s, the TIN).

## 2026-10-18 — M11.9 CivilCraft section panel and sample lines (agent, work/civiltools)

- M11.9: `crates/ui-egui/src/civilsections.rs`: the section view under the plan (X, an alignment's Section view
  button), as sections-ui.js and section-view.js show it: alignment, ‹ station › (← → Home End), exaggeration (fit, 1×,
  2×, 5×, 10×), the end-area volumes, CSV; one section drawn with its grid, centreline, ground, design, cut and fill
  shaded, corridor points (labels stepped up when they would touch), station, areas and exaggeration; the current
  section as a line on the plan. One bottom panel at a time (the profile closes it and it closes the profile). The
  alignment's Properties gained Sample lines: add, spacing and widths typed, an extra station typed (and removed),
  the facts with the grid volumes, Section view, Remove sample lines — each one undo step with 0.5's labels.
- Commands: `civil.sections`, `civil.sections.al`, `civil.sections.go` (index | prev | next | first | last),
  `civil.sections.ve`, `civil.sections.csv`, `civil.sampling` (add | remove | step= | left= | right= | extra+= |
  extra-=).
- Parity: `cvtooldiff.mjs` gained a sections scenario (refused edit before adding, add, spacing / widths incl. a bad
  one, extra stations incl. a repeat and a bad one, go last / prev / index, CSV twice, remove extra, undo, remove all,
  undo / redo) and compares every file the app saved: **48/48 projects byte-identical, 772/772 status lines, 4/4 CSV
  files byte-identical** to the frozen 0.5 JS.
- Speed: stepping sections on the 500k-point survey, 4–5 ms a frame.

## 2026-10-18 — M11.8 CivilCraft profile panel (agent, work/civiltools)

- M11.8: `crates/ui-egui/src/civilprofile.rs`: the profile under the plan (R, an alignment's Profile view button), as
  civilcraft.js `renderProfile` and profile-view.js draw it: the existing ground, the tangents through the PVIs, the
  design profile, the station and elevation grid with its exaggeration, grades on the tangents, each vertical curve's
  L / K with BVC / EVC ticks and its high or low point, the PVIs and their labels; the reading under the pointer
  (station, ground, design, grade, cut / fill) and its point on the plan. Editing: drag a PVI (kept 0.01 inside its
  neighbours, to 0.01, one undo step), Add PVIs (I) and a click adds one, the selected PVI goes with Delete, Start from
  the ground, the alignment's PVI table (station, elevation, curve length typed, × removes, the number selects) and PI
  table (easting, northing, radius typed, × removes), each one undo step with 0.5's labels.
- Commands: `civil.profile`, `civil.profile.al`, `civil.profile.start`, `civil.pvi.addmode`, `civil.pvi.add`
  (sta,z), `civil.pvi.drag` (i:sta,z), `civil.pvi.select`, `civil.pvi.set` (i:sta|z|L=text), `civil.pi.set`
  (i:x|y|r=number). `cadcraft-shot --key NAME` presses a key (the gesture path: A, clicks, Backspace, Enter checked).
- Parity: `cvtooldiff.mjs` gained a profile scenario (start, adds incl. a repeat and one past the end, drags clamped by
  neighbours, typed station / elevation / length, a bad station, select + Delete, undo / redo, then Delete of the
  alignment) on the four sites: **44/44 projects byte-identical, 668/668 status lines identical**.
- Speed: on the 500k-point survey the profile panel's frame is 4 ms with the pointer reading (the ground profile is the
  engine's memo).
- Not yet: feature lines in the profile (0.5 WL.20), the section panel (sample lines / section view), the PI table's
  curve-fit "why" buttons (WL.23).

## 2026-10-18 — M11.7 CivilCraft drawing tools, typed dialogs and grips (agent, work/civiltools)

- M11.7: `crates/ui-egui/src/civiltools.rs`: the plan's tools as 0.5 CivilCraft has them — Select, Alignment, Pad,
  Pipes, Breakline, Boundary, Feature line, Parcel — toolbar buttons and keys (V A G P B O L), a click per point, Enter or
  a double click finishes, Backspace drops the last point, Esc cancels; the hint over the plan, the rubber band, the
  Breakline snap square, the alignment's length and bearing at the pointer. Each finish is civilcraft.js `finishDraft`
  (features-ui.js / parcels-ui.js `finish`): one `Project::change`, same ids, names, JSON, undo labels and status words
  (fitted PI radii, pad elevation from the ground, structures joined and pipe inverts from `auto_inverts` against the
  graded-else-EG surface, feature-line z from points or the ground, parcels closing on the first corner). Grips: the
  selected alignment's PIs, pad corners, feature line vertices, a structure and the one selected survey point drag
  (4 px slack, preview, one undo step on release; Esc puts a point back); Delete removes the selection; a PI row's
  Remove. `civildialogs.rs`: the Traverse dialog (traverse-ui.js) and Parcel from deed calls (parcels-ui.js), read as
  typed with the engine's parsers, added as survey points / a parcel in one step, the traverse report as CSV.
- Everything is a command: `civil.tool`, `civil.click` / `civil.dblclick` (world x,y; same code as the canvas click),
  `civil.finish`, `civil.back`, `civil.cancel`, `civil.delete`, `civil.grip` (x1,y1:x2,y2), `civil.pi.remove`,
  `civil.traverse` / `civil.calls` (JSON form, `add`). `cadcraft-shot --trace` prints the status line after every
  `--run` (and goes on past a refused command).
- Parity (`forks/tools/cvtooldiff.mjs`, new): 10 scenarios (every tool, too-few-points, repeats, close-on-first,
  joins, grips of each kind, a survey point drag, undo/redo, PI remove, Delete of each kind, traverse by bearings and
  by angles to a point, deed calls with a curve and with a gap, refused forms) on four sites — the sample (state-plane
  feet), the synthetic survey (x ≈ 500 000, y ≈ 4 000 000), the demo (network, pads), a metric site at survey
  coordinates: **40/40 saved projects byte-identical, 572/572 status lines identical** to the frozen 0.5 JS.
- Speed on the 500k-point survey (`testfiles/generated/civil/survey-500k.civilcraft.json`, new, in LARGE-FILES.md;
  `forks/tools/cvtoolbench.mjs`): snap search per pointer move 16.5 ms JS vs a whole 6 ms frame; breakline finish +
  rebuilt surface + next plan 2550 vs 1211 ms; pad finish + graded surface + volumes 3655 vs 2000 ms; open 2776 vs
  1477 ms. The plan's TIN outline now uses a flat edge table (`civilplan::fast_outline`, same rings as
  `Surface::outline`, tested): 2.6 s → 0.15–0.3 s on a million triangles; a pad edit no longer rebuilds it.
- Plain English where 0.5 has no key: the command palette labels/params of the new `civil.*` commands.
- Next: profile and section panels (PVI editing), point add (0.5 has paste/duplicate), multi-point selection.
## 2026-10-18 — M7.13 BimCraft's IFC editing, plan view and Draw tools in the app (agent, work/bimdraw)

- M7.13 (crates/ui-egui `bimedit.rs`, `bimplan.rs`; cc-bim `draw::schedule`): the IFC view edits and saves. Properties
  edits names, attributes and property values (one undo step each), Undo / Redo (Ctrl+Z / Ctrl+Y), **Save as IFC**
  through the host ("<name> (edited).ifc", the 0.5 status line), **New building** (dialog + `bim.new`). **Draw**
  opens the storey plan beside / instead of the 3D view: cut walls with their pieces, door and window swings, rooms
  with names and areas, roofs, dimensions (ft-in or mm), levels panel (add, rename, elevation,
  height), Fit, Print (the plan PDF of model-pdf.js) and Schedules (doors, windows, walls, rooms; CSV). Tools: Wall
  (chains with snapping, typed lengths, closing a room flips the outside), Door, Window (17 types, flip, typed
  distance), Floor, Ceiling, Roof (hip, gables on the short or long sides, shed, pitch), Select (walls, openings,
  rooms; Properties shows each one's own editor), Delete. Every action is a command (`bim.*`, 30 commands) and a
  gesture; each change is one undo step. Rooms have no piece in the 3D view, so a room picked on the plan is kept
  beside the 3D pick.
- Parity: `forks/tools/bmuidiff.mjs --ui` drives cadcraft-shot through the commands and compares with the frozen JS:
  the saved IFC, the plan PDF, both schedule CSVs and the status lines are byte-identical; the sample's Properties
  edit + undo + redo + Save too. Whole harness: **2,513 / 2,513 steps equal, 420 / 420 saved files byte-identical**.
- Timings (forks/tools/bmbench.mjs, best of 2, JS 0.5 in Node vs Rust release; ms):

  | File | MB | open JS / Rust | load JS / Rust | plan JS / Rust | edit + save JS / Rust |
  |---|---:|---:|---:|---:|---:|
  | cc-ifc-big-210.ifc | 200.7 | 4728 / 2118 | 27539 / 5562 | 396 / 261 | 8035 / 529 |
  | cc-ifc-big-120.ifc | 115.3 | 1367 / 753 | 8554 / 2765 | 208 / 148 | 3002 / 256 |
  | AC25-Molio-IFC2X3.ifc | 70.5 | 571 / 306 | 2302 / 1315 | 42 / 24 | 1775 / 115 |
  | Molio_with_URIs.ifc | 70.5 | 426 / 272 | 3328 / 1286 | 28 / 13 | 1591 / 161 |
  | IFC_Schependomlaan_geo.ifc | 47.0 | 722 / 418 | 2864 / 1215 | 85 / 36 | 1263 / 201 |

- Not in 0.5, so not here: columns (BimCraft 0.5 has no column tool). Not ported yet: grips (dragging wall ends,
  sliding openings), res-follow.js residential sync, the Library's placed items.

## 2026-10-18 — M7.12 BimCraft's building tools and IFC editor in Rust (agent, work/bimdraw)

- M7.12: cc-ifc writes: `IfcFile` keeps edits beside the original bytes (model.js: `set` / `add` / `remove`, every read
  sees them, `ids`, `save` with untouched lines copied byte for byte and written ones formatted by step.js's
  `formatValue` / `formatString` / `formatReal`; FILE_NAME's time stamp from the caller). `IfcFile::open_owned` keeps
  its bytes (an editable model the app holds). cc-bim `wapi`: cc-ifc's IfcAPI writing side (line objects with the JS
  value shapes, `toApi` / `fromApi`, `CreateIfcEntity`, `CreateIfcType`, `WriteLine`, `DeleteLine`, GlobalIds from a
  seeded mulberry32 byte stream, `GetWorldTransformMatrix` on the edited file, `lengthUnitMm`, `retypeLine`).
- cc-bim `draw` (CadCraft 0.5 part 3 track BM, one module per JS file): ifc-edit.js (attributes, single property values
  with copy-on-write of shared sets and values, building-tool changes, undo / redo), model-write.js + the IfcPlacer
  parts it uses (journal, placements, extrusions, shapes, psets / quantities, materials, colours, deleting),
  model-levels.js, model-walls.js (types, L / T / X joins, IFC), model-openings.js (17 door / window types, bodies,
  plan symbols), model-rooms.js, model-slabs.js (rooms, floors, ceilings, roofs that follow the walls; ShapeKey),
  res-roof.js + res-skeleton.js (the Roof tool's weighted straight skeleton, presets, fascia, soffit, gable walls),
  model-ops.js, model-tools.js, model-units.js (+ libitem.js `parseLength`), new-model.js, model-plan.js (plan cut of
  the rebuilt model, wall pieces, dimensions, the plan scene of model-ui.js) and model-pdf.js. res-follow.js's
  residential sync is not ported (it only acts on residential elements; the draw tools leave such models alone).
- `cadcraft-cli bimdraw <job.json>`: open or New building, then the page's steps (model-ui.js wallPoint chains,
  placeHere, placeSlab, Properties changes, levels, deletes, undo / redo, attribute / property edits, plan + plan
  PDF, Save as IFC). Parity: `forks/tools/bmuidiff.mjs` runs the same steps on the frozen JS: **2,473 / 2,473 steps
  equal, 415 / 415 saved IFC files byte-identical, every plan scene and plan PDF equal** (13 scripted jobs, 12 seeded
  random houses, edits + Save + plans on every public fixture and corpus IFC under 40 MB).
- Gate: fmt, clippy, tests, assets, layers, wasm, deny pass; fuzz smoke passes for every target except
  `markup_pdf_read`, which found a panic inside hayro-syntax 0.8.0 (`object/array.rs:153` unwrap, reached from
  `cc_io_markup::jsdoc::JsDoc::open`), not in this work: the find is kept outside the repo for the markup owner.
## 2026-10-18 — M9.15 Several PDFs per plan set; Compare sheets (agent, work/markuptools)

- M9.15: the sheet view holds a project of PDFs (`PdfFile` per file: MarkupCraft id "f1", "f2"…, pages, bytes, the
  reader's page keys / views / titles / removeRefs). Opening a PDF while a plan set is open adds it, as 0.5's Open does:
  its markups, scales, sheet names, layers and columns join the one store with the same id sequence, and TakeoffCraft's
  file list gets it (marks are "<hash>:<page>" of the open file). The sheet list shows every file's sheets under its
  name; `pdf.file` switches. Save follows markupcraft.js saveFiles (one file → it; several → the files with changes,
  else the open one; saved sheets stop being dirty); TakeoffCraft's marked PDF is written once per file that has marks
  (exportPdf). Compare sheets by changed lines (`markupcompare.rs`, markupcraft.js vectorCompare / cloudChanges): any
  sheet of any open file against the open one through cc-io-markup `page_ops` → `page_segments` (cached) →
  `diffSegments` at 1.5 pt; lines only on the old sheet in red, only on the new one in blue, the status line with
  their lengths; Cloud the changes adds a revision cloud round each cluster (gap 18, ≥ 6 pt) in one undo step with
  "Changed since <old sheet>". Command `markup.compare` (old key | cloud | stop). Not yet: the picture overlay, line-up /
  nudge / align by two points, compare a whole set, closing one file.
- Parity: mkuidiff.mjs gained `twofiles` (draw on both files, Save twice: the dirty-file rule) and `compare` (a second
  PDF, compare, cloud, status, Save): 6 scenarios × kitchen sheet 6 / 6, plus compare on rev-a / rev-b (a generated
  revision pair: lines gone, a moved group, a new room → 16 clouds, each with its status reply) and on blank-grid /
  pdflib edits: identical.
  tkopsdiff.mjs gained `twofiles` (marks on two files, scales per sheet, one item across both, four exports incl. two
  marked PDFs): 17 / 17 identical over 4 scenarios.

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
