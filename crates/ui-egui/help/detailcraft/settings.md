---
title: DetailCraft settings
summary: Start new members as US or European shapes, set how far Duplicate moves a copy, choose whether copies bring their connections, and show or hide bolts in the 3D view.
section: reference
order: 5
keywords: settings, preferences, sheet size, paper size, drawings, options, steel, aisc, en, european, us shapes, metric shapes, default profile, grade, s355, a992, duplicate offset, paste offset, copy connections, bolts, anchor rods, hide bolts, reset
---

Open {{ui:Settings}} with the gear in the app bar or {{key:Mod+,}}. The first tab, {{ui:DetailCraft}}, holds the settings that belong to this app; {{ui:General}} holds the ones every CadCraft app shares (theme, toolbar, panels). DetailCraft's settings exist only in the app: you change them here, they take effect at once, and this browser remembers them. They are not saved in your model file.

## New members

Which profiles and grades the placing tools start with:

| Tool | US shapes (AISC), A992 | European shapes (EN), S355 |
|---|---|---|
| {{ui:Beam}} | W16x26, A992 | IPE300, S355 |
| {{ui:Column}} | W12x50, A992 | HEB200, S355 |
| {{ui:Brace}} | HSS6x6x1/4, A500 Gr C | HEA160, S355 |
| {{ui:Plate}} | A36 | S355 |

US shapes is the start. The European choice uses an HEA160 for braces because DetailCraft's EN catalog has no hollow sections yet ([Profile catalog](topic:profile-catalog)). You can still pick any profile and grade for the next piece in the {{ui:Properties}} panel while a placing tool is on. Members already in the model keep their own profile and grade.

## Duplicate and paste

- **Offset (mm)**: x, y and z. {{ui:Duplicate}} ({{key:Mod+D}}) puts the copy this far from the original, and the {{ui:Paste by offset (mm, from the original)}} form starts with it. It starts at 6000, 0, 0: one 6 m bay along the numbered grids. Set it to your bay spacing, or to a level height in z to copy a floor up.
- **Copy end plates and base plates with their members** (on at the start): a connection comes along with a copy only when every member it joins is copied. Clear it to copy members and footings without their connections, for example to try a different connection on the copy.

See [Copy, paste and duplicate](topic:copy-paste).

## 3D view

**Show bolts and anchor rods** (on at the start). Clear it to hide them in the view and see the holes and plates of a busy joint. Hidden bolts cannot be clicked; click the plate to select the connection. The bill of materials, drawings and every export still include every bolt and rod.

## Corner indicator

What shows in the corner of the 3D view: **View cube** (the start; top right — click a face, edge or corner to look from there, drag it to orbit, drag its compass ring to turn the model), **Axis icon** (a small X, Y, Z marker at the bottom left that turns with the view: X red, Y green, Z blue) or **None**. Only in {{ui:3D}}; {{ui:Plan}} and {{ui:Elevation}} show neither. See [The view cube](topic:view-cube).

## Reset

{{ui:Reset DetailCraft settings}} at the bottom of the dialog puts these back to the start (US shapes, offset 6000, 0, 0, connections copied, bolts shown), along with the {{ui:General}} ones.

## Drawings

**Sheet size**: the sheet drawings and shop drawings are made on, landscape: A4, A3 (the start), A2, A1, ANSI B, ANSI C or ANSI D. The {{ui:Drawings}} and shop drawing windows have the same list. See [General-arrangement and assembly drawings](topic:drawings).

## Rebar

- **ASTM bar names**: #3 to #11, or the soft-metric #10 to #36 for the same bars.
- **Bending schedule shape codes**: DetailCraft's S, L, U; ACI 315 types; or BS 8666 shape codes. See [Rebar reference](topic:rebar-reference).

## Dragging to select

**Rectangle** (as before) or **Lasso**: a freehand line, with {{key:Space}} switching between inside, touching and fence while you drag. See [Select with a window](topic:select-and-change#select-with-a-window).
