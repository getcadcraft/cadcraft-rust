# Gaps against CadCraft 0.5

Last updated 2026-10-18 · Change: CivilCraft tools and panels (M11.7–M11.10), BimCraft Draw (M7.13), MarkupCraft / TakeoffCraft panels (M9.13–15); per-app gaps below. Ranked; shrink as features land.

1. **Drafting vs 0.5** (after M4): the commands past the M4 set (HATCH, DIM*, BLOCK/INSERT, STRETCH, FILLET, …), the
   ribbon / menus / properties panel, constraints, 3D, UCS, block editor, add-on hooks, recovery snapshots; linked
   drawings are not shown in edit mode (the viewer shows them); objects with a dense linetype draw continuous past 400
   dashes (`DISPLAY_MAX_DASHES`); DXF 2000 save is slower than the JS (0.7×); every edit rebuilds the whole display.
2. **Web speed**: the browser build reads only 1.2–1.5× faster than 0.5's JS (native 2.6–4.2×). Wasm threads were built
   and measured slower (one global allocator lock; roadmap § Wasm threads); allocation measured not to be the cost; next: profile the decoder.
3. **Viewer details** vs CadCraft 0.5: raster images (IMAGE) not drawn, per-viewport frozen layers, print view /
   lineweights, find, recent files, settings, help panel, language files (en / es-MX), touch gestures.
4. **CivilCraft app view** (M11): drawing tools, typed traverse / deed calls, grips, point moves and Delete (M11.7),
   the profile panel with PVI add / drag / delete and the PI / PVI tables (M11.8), the section panel and sample lines
   (M11.9), point selection with copy / cut / paste / duplicate (M11.10) are in at byte parity (cvtooldiff 52/52).
   Missing: cross-section sheets (PDF) and mass haul from the section panel, feature lines in the profile (WL.20), the
   curve "why" fixes (WL.23), the other Properties edits (names, pad elevation and slopes, corridor, pipes, feature
   line grades …), the lasso (a Shift+drag that bends) and its window / crossing / fence modes, renumber settings,
   the import dialog (format is detected), GIS layers and basemaps (so exports carry no map pictures), the catchment
   tool, touch's draft bar; a 500k-point survey zoomed out draws every point cross (fast, but a carpet).
5. **ModelCraft** (M6): render, print check, materials editor, the full inference set; SKP import works.
6. **BimCraft** (M7.13: IFC edit + save, storey plan + PDF, Draw tools, schedules at parity): no grips (dragging wall ends, sliding doors / windows), no residential sync (res-follow.js), no
   Library items placed from the plan.
7. **PDF sheets: MarkupCraft and TakeoffCraft** (M8/M9): every 0.5 MarkupCraft drawing tool (M9.14) with select /
   move / grips / window, the text editor and the Properties panel, save marked-up PDF at parity; takeoff count / linear
   / area, exports, Auto count. Missing in MarkupCraft: picture and signature stamps, tool chest presets, cut-outs in
   areas, spaces, legends, links, replies, punch list, markups list, copy / paste, sheet rename, pages menu, BCF,
   compare's picture overlay / line-up / whole-set compare, closing one PDF of a plan set (several PDFs and compare by
   changed lines are in, M9.15).
   TakeoffCraft has its item / mark panels, assemblies, bid areas, alternates, deducts, slopes and cut-outs (M9.13);
   still missing there: arcs and circles in runs and outlines, dragging mark corners, multi-select, groups (typicals),
   notes / clouds, the 3D view, revisions.
8. **PartCraft** (M13): part view on the mesh kernel — open part / script, sample, 3D with face picking, history,
   parameters, undo, rollback, suppress, Save, STL / 3MF / STEP / IFC at byte parity. Missing: the sketcher and every
   tool that makes or edits features (new sketch / extrude / hole … , picking edges and faces for them, delete, move,
   rename), assemblies, drawings (PDF), DXF of a sketch or face, measure / mass / section, configurations (a parameter
   edit does not follow `configs` yet), the exact kernel's features (they say "not built", as 0.5's mesh build does),
   rebuilds on a thread (a big part blocks the window), Spanish words. **Studio** (M16): the local control channel (M16.1) and the headless MCP server with 0.5's 22 tools (M16.2) are in; missing there: `drawing_plot` (plot.js) and LISP scripts. The window's live tools are in (M16.3) except drawing_plot, view_screenshot and DetailCraft's (app_info only). Live and the start screen are being built.
   rebuilds on a thread (a big part blocks the window), Spanish words. **Studio** (M16): the local control channel (M16.1) and the headless MCP server with 0.5's 22 tools (M16.2) are in; missing there: `drawing_plot` (plot.js) and LISP scripts. Live tools in the window, Live and the start screen are being built.
   rebuilds on a thread (a big part blocks the window), Spanish words. **Studio** (M16): the local control channel for assistants is in (M16.1, `cadcraft --control`); the MCP tools, Live and the start screen are being built.
   rebuilds on a thread (a big part blocks the window), Spanish words. **Studio**: in progress (M16).
   **DetailCraft** (M14.1): view, member properties with undo, reports, clash check and every 0.5 export at parity. Missing: placing
   members / plates / footings / stairs, connection tools and parameters (end plate … gusset, apply to similar), holes and
   copes editors, grids / levels / custom profiles, phases add / rename / remove, copy / paste, move / rotate / mirror,
   window and lasso selection, plan and elevation views, the drawings dialog's preview and issue log, the Settings dialog
   (`detail.setting` only), es-MX labels; several of the bar's labels are plain English (view buttons, Steel sample).
9. Panel buttons in the new views are partly plain English rather than 0.5 catalog words (MarkupCraft's tool bar and
   TakeoffCraft's export row use the catalogs).
10. Accessibility: full screen-reader support is a **known issue, skipped** (owner, 2026-10-15). Icon buttons get
   accesskit labels where it is free. The desktop build leaves out eframe's `accesskit` bridge (2026-10-17): on Linux it
   pulled zbus crates RustSec lists as unmaintained (RUSTSEC-2026-0370/0371/0372, no upgrade); it comes back with the
   screen-reader work.

Closed: DWG reading R13–R2018 at full-model parity with 0.5 (M2, 2026-10-16); DXF reading and the 2D viewer (M3);
the command line, LINE … TRIM, undo, selection, DWG writing, CadCraft's DWG import and DWG / DXF saving at byte parity (M4).
