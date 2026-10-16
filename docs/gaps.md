# Gaps against CadCraft 0.5

Last updated 2026-10-16 · Change: DWG reading closed (M2). Ranked; shrink as features land.

1. **The CadCraft app itself:** no viewer yet (M3: DWG/DXF model + sheets, pan/zoom, layers), no drafting (M4).
2. **DXF reading** (`dxf-model.js` / CadCraft `dxf.js`): not ported yet (M3).
3. **DWG writing** (`dwg-save.js` / cc-dwg `writeDwg`): not ported yet (M4).
4. The other seven apps, cc-ifc, cc-skp, m3d (M5+).
5. Accessibility: full screen-reader support is a **known issue, skipped** (owner, 2026-10-15). Icon buttons get
   accesskit labels where it is free.

Closed: DWG reading R13–R2018 at full-model parity with 0.5 (M2, 2026-10-16).
