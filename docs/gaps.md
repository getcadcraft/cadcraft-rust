# Gaps against CadCraft 0.5

Last updated 2026-10-17 · Change: app views for six apps; per-app gaps below. Ranked; shrink as features land.

1. **Drafting vs 0.5** (after M4): the commands past the M4 set (HATCH, DIM*, BLOCK/INSERT, STRETCH, FILLET, …), the
   ribbon / menus / properties panel, constraints, 3D, UCS, block editor, add-on hooks, recovery snapshots; linked
   drawings are not shown in edit mode (the viewer shows them); objects with a dense linetype draw continuous past 400
   dashes (`DISPLAY_MAX_DASHES`); DXF 2000 save is slower than the JS (0.7×); every edit rebuilds the whole display.
2. **Web speed**: the browser build reads only 1.2–1.5× faster than 0.5's JS (native 2.6–4.2×): wasm threads next.
3. **Viewer details** vs CadCraft 0.5: raster images (IMAGE) not drawn, per-viewport frozen layers, print view /
   lineweights, find, recent files, settings, help panel, language files (en / es-MX), touch gestures.
4. **CivilCraft app view** (M11, read-mostly): no drawing tools yet (alignment, pad, pipe, breakline, boundary,
   feature line, parcel, traverse), no grips / point moves, no profile and section panels, no import dialog (format is
   detected), no LandXML / DXF / DWG / PDF / GLB export from the view (the engine has them), no GIS layers or basemaps,
   no copy / paste of points; `Surface::outline` takes 2.9 s on a 1 M-point TIN (once per surface).
5. **ModelCraft** (M6): render, print check, materials editor, the full inference set; SKP import works.
6. **BimCraft** (M7): IFC 3D view, picking, properties, Find / Quantity; no plan drawing, no BIM Draw tools, no IFC save
   from the view.
7. **PDF sheets: MarkupCraft and TakeoffCraft** (M8/M9): markup tools (pen … text, Length, Area, Callout, Calibrate),
   save marked-up PDF, takeoff count / linear / area, exports, Auto count. Missing: the rest of MarkupCraft's 30+ tools
   (stamps, notes, clouds by outline, compare, BCF…), properties panel, sheet compare, TakeoffCraft's assemblies,
   bid areas, alternates, deducts and slopes in the UI (the engine has them), several PDFs per project.
8. **PartCraft, DetailCraft, Studio**: engines at parity (PartCraft's rebuild kernel in progress), no app view yet.
9. Panel buttons in the new views are partly plain English rather than 0.5 catalog words (MarkupCraft's tool bar and
   TakeoffCraft's export row use the catalogs).
10. Accessibility: full screen-reader support is a **known issue, skipped** (owner, 2026-10-15). Icon buttons get
   accesskit labels where it is free. The desktop build leaves out eframe's `accesskit` bridge (2026-10-17): on Linux it
   pulled zbus crates RustSec lists as unmaintained (RUSTSEC-2026-0370/0371/0372, no upgrade); it comes back with the
   screen-reader work.

Closed: DWG reading R13–R2018 at full-model parity with 0.5 (M2, 2026-10-16); DXF reading and the 2D viewer (M3);
the command line, LINE … TRIM, undo, selection, DWG writing, CadCraft's DWG import and DWG / DXF saving at byte parity (M4).
