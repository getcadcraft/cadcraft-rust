# Gaps against CadCraft 0.5

Last updated 2026-10-18 · Change: PartCraft (M13.1) and DetailCraft (M14.1) app views; per-app gaps below. Ranked; shrink as features land.

1. **Drafting vs 0.5** (after M4): the commands past the M4 set (HATCH, DIM*, BLOCK/INSERT, STRETCH, FILLET, …), the
   ribbon / menus / properties panel, constraints, 3D, UCS, block editor, add-on hooks, recovery snapshots; linked
   drawings are not shown in edit mode (the viewer shows them); objects with a dense linetype draw continuous past 400
   dashes (`DISPLAY_MAX_DASHES`); DXF 2000 save is slower than the JS (0.7×); every edit rebuilds the whole display.
2. **Web speed**: the browser build reads only 1.2–1.5× faster than 0.5's JS (native 2.6–4.2×). Wasm threads were built
   and measured slower (one global allocator lock; roadmap § Wasm threads); next: fewer allocations in the readers.
3. **Viewer details** vs CadCraft 0.5: raster images (IMAGE) not drawn, per-viewport frozen layers, print view /
   lineweights, find, recent files, settings, help panel, language files (en / es-MX), touch gestures.
4. **CivilCraft app view** (M11, read-mostly): no drawing tools yet (alignment, pad, pipe, breakline, boundary,
   feature line, parcel, traverse), no grips / point moves, no profile and section panels, no import dialog (format is
   detected), no GIS layers or basemaps (so exports carry no map pictures),
   no copy / paste of points; `Surface::outline` takes 2.9 s on a 1 M-point TIN (once per surface).
5. **ModelCraft** (M6): render, print check, materials editor, the full inference set; SKP import works.
6. **BimCraft** (M7): IFC 3D view, picking, properties, Find / Quantity; no plan drawing, no BIM Draw tools, no IFC save
   from the view.
7. **PDF sheets: MarkupCraft and TakeoffCraft** (M8/M9): markup tools (pen … text, Length, Area, Callout, Calibrate),
   save marked-up PDF, takeoff count / linear / area, exports, Auto count. Missing: the rest of MarkupCraft's 30+ tools
   (stamps, notes, clouds by outline, compare, BCF…), properties panel, sheet compare, several PDFs per project.
   TakeoffCraft has its item / mark panels, assemblies, bid areas, alternates, deducts, slopes and cut-outs (M9.13);
   still missing there: arcs and circles in runs and outlines, dragging mark corners, multi-select, groups (typicals),
   notes / clouds, the 3D view, revisions.
8. **PartCraft** (M13): part view on the mesh kernel — open part / script, sample, 3D with face picking, history,
   parameters, undo, rollback, suppress, Save, STL / 3MF / STEP / IFC at byte parity. Missing: the sketcher and every
   tool that makes or edits features (new sketch / extrude / hole … , picking edges and faces for them, delete, move,
   rename), assemblies, drawings (PDF), DXF of a sketch or face, measure / mass / section, configurations (a parameter
   edit does not follow `configs` yet), the exact kernel's features (they say "not built", as 0.5's mesh build does),
   rebuilds on a thread (a big part blocks the window), Spanish words. **Studio**: engine at parity, no app view yet.
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
