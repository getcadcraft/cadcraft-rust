# Gaps against CadCraft 0.5

Last updated 2026-10-16 · Change: drafting and DWG/DXF saving closed (M4). Ranked; shrink as features land.

1. **Drafting vs 0.5** (after M4): the commands past the M4 set (HATCH, DIM*, BLOCK/INSERT, STRETCH, FILLET, …), the
   ribbon / menus / properties panel, constraints, 3D, UCS, block editor, add-on hooks, recovery snapshots; linked
   drawings are not shown in edit mode (the viewer shows them); objects with a dense linetype draw continuous past 400
   dashes (`DISPLAY_MAX_DASHES`); DXF 2000 save is slower than the JS (0.7×); every edit rebuilds the whole display.
2. **Web speed**: the browser build reads only 1.2–1.5× faster than 0.5's JS (native 2.6–4.2×): wasm threads next.
3. **Viewer details** vs CadCraft 0.5: raster images (IMAGE) not drawn, per-viewport frozen layers, print view /
   lineweights, find, recent files, settings, help panel, language files (en / es-MX), touch gestures.
4. The other seven apps, cc-ifc, cc-skp, m3d (M5+).
5. Accessibility: full screen-reader support is a **known issue, skipped** (owner, 2026-10-15). Icon buttons get
   accesskit labels where it is free.

Closed: DWG reading R13–R2018 at full-model parity with 0.5 (M2, 2026-10-16); DXF reading and the 2D viewer (M3);
the command line, LINE … TRIM, undo, selection, DWG writing, CadCraft's DWG import and DWG / DXF saving at byte parity (M4).
