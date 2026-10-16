# Gaps against CadCraft 0.5

Last updated 2026-10-16 · Change: viewing closed (M3). Ranked; shrink as features land.

1. **Drafting** (M4): the command line, LINE … TRIM, undo, selection, save.
2. **Web speed**: the browser build reads only 1.2–1.5× faster than 0.5's JS (native 2.6–4.2×): wasm threads next.
3. **Viewer details** vs CadCraft 0.5: raster images (IMAGE) not drawn, per-viewport frozen layers, print view /
   lineweights, find, recent files, settings, help panel, language files (en / es-MX), touch gestures.
4. **DWG writing** (`dwg-save.js` / cc-dwg `writeDwg`): not ported yet (M4).
4. The other seven apps, cc-ifc, cc-skp, m3d (M5+).
5. Accessibility: full screen-reader support is a **known issue, skipped** (owner, 2026-10-15). Icon buttons get
   accesskit labels where it is free.

Closed: DWG reading R13–R2018 at full-model parity with 0.5 (M2, 2026-10-16); DXF reading and the 2D viewer (M3).
