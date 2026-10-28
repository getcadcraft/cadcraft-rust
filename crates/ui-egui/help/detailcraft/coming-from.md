---
title: Coming from other detailing software
summary: How the habits of a full detailing package map to DetailCraft, and the differences that catch experienced detailers out.
section: switching
order: 10
keywords: switching, migrate, familiar, workflow, comparison, differences, numbering, components, catalog, drawings, habits
---

If you detail in Tekla Structures, most of DetailCraft will look familiar: grids, levels, profiles from a catalog, parts and assemblies, connections, numbering, NC files, IFC. The scale is very different: Tekla Structures is a full production system, and DetailCraft is a small, early browser app that does a few jobs end to end. This page tells you where your habits carry over and where they do not.

:::tip
If your hands know the ribbon of Tekla Structures, pick the {{ui:Steel ribbon}} menu style in {{ui:Settings}} › {{ui:DetailCraft}} › {{ui:Menus, keys and mouse}}: Modeling, Detailing, Edit, Drawings, Fabrication and View tabs, and the middle button pans. If you come from AutoCAD and like to type commands, {{ui:Drafting classic}} adds a command line (BEAM, COLUMN, ENDPLATE, ZOOM …). See [Menus, keys and mouse](topic:menus-keys-and-mouse).
:::

## Quick map

| You are used to | In DetailCraft |
|---|---|
| Creating grids with coordinates in a grid dialog | {{ui:Grids}} in the left panel: labels and the spacings between lines, not absolute coordinates. Numbered lines run one way, lettered lines the other |
| Levels as grid Z coordinates | A separate {{ui:Levels}} list, name and elevation per row, applied with {{ui:Apply levels}} |
| Picking points anywhere with snaps | Clicks snap to grid intersections only. Off-grid points are typed into a member's {{ui:Ends (x, y, z mm)}} |
| The column, beam and plate commands | {{ui:Column}} ({{key:C}}), {{ui:Beam}} ({{key:B}}), {{ui:Brace}} ({{key:R}}), {{ui:Plate}} ({{key:P}}), {{ui:Footing}} ({{key:F}}) |
| Part properties dialog (double-click a part) | Click once: the {{ui:Properties}} panel on the right is always live; there is no dialog and no Modify or Apply button |
| Position on plane, rotation and depth settings | {{ui:Position}} (top of steel, centered or bottom on the line) and {{ui:Rotation (°)}}. No separate horizontal offsets |
| The profile catalog | A fixed list of 1,259 published sizes (AISC W, S, C, L, HSS, Pipe; European IPE, HEA, HEB, UPN), plus parametric plates and your own {{ui:Custom profile}} sections |
| Material catalog | Seven grades in a list: A992, A36, A572 Gr 50, A500 Gr C, S235, S275, S355 |
| Applying a connection component and its dialog | Select the beam, choose {{ui:End plates}}. The settings appear in the {{ui:Properties}} panel. Only two connections exist (end plate to column flange, base plate) |
| Auto-connection, or copying a joint to similar joints | Select a connection, choose {{ui:Apply to similar (n)}} |
| Numbering with series and start numbers | No command: marks update by themselves after every change, with fixed prefixes c, b, br, p and C, B, BR, P |
| Bolt and hole dialogs | Holes are typed in by face and position. Bolts come only from connections |
| Cut, fit and polygon cut tools | Copes (top or bottom, at either end) on I-shapes only. Connection cut-backs are automatic |
| Reports and templates | Three fixed reports in tabs at the bottom, and CSV downloads |
| Drawing creation and the drawing list | One shop drawing at a time, for the selected I-shape or plate, previewed then downloaded as PDF. Nothing is kept or edited |
| NC file export | {{ui:NC files}}: one DSTV file per part mark, I-shapes and plates only, zipped |
| IFC export | {{ui:IFC}}: IFC4 of the whole model, with rebar |
| Rebar in any concrete part | Rebar only in rectangular pad footings, US bar sizes, with a bottom mat and starter bars |
| Model saved in a project folder, multi-user | One `.detailcraft.json` file you download. Single user, on your computer |
| Undo history | {{key:Mod+Z}} and {{key:Mod+Y}}, up to 200 steps, kept until you open or start another model |

## Differences that trip people up

### Marks change on their own

There is no numbering step. Marks are recalculated after every change, so they are always current, but they can also renumber: add a column and yesterday's c3 may now be c4. There is no "numbering history" to keep old marks. Finish the model, then issue the BOM, NC files and shop drawings together from one saved file.

### Middle mouse button zooms; it does not pan

In the 3D view: left-drag turns, right-drag pans, the wheel zooms, and dragging with the middle button also zooms. Holding {{key:Shift}} while left-dragging pans too. The wheel zooms toward the middle of the view, not toward the pointer. There is no rotation-center pick: the model turns around the middle of the view. See [Move around the 3D view](topic:navigate-3d).

### Columns face one way by default

At rotation 0 a column's flanges face along the numbered-grid direction. Beams that run between lettered grid lines frame into the web, and end plates are refused there ("Beams framing into a column web are not built yet"). Rotate those columns 90° before you add connections.

### Elevations only on lettered lines

{{ui:Elevation}} looks along lettered grid lines only. There are no views along numbered lines, no saved views and no work planes. Use {{ui:3D}} or {{ui:Add by grid}} for frames on numbered lines.

### Selection, copy and move

Select several parts with {{key:Shift}}+click or {{key:Mod+A}}, then copy, cut, paste (on a grid intersection, at a typed offset, or in place) and duplicate them, connections included ([Copy, paste and duplicate](topic:copy-paste)). There is no window selection, no select filter, no move, mirror or array, and properties are changed one member at a time in the {{ui:Properties}} panel.

### Grids do not drag members along

Members keep their coordinates when you change the grid spacing; nothing moves with the grid lines. Change the grid before you place steel, or edit each member's end coordinates afterwards.

### Connections are not designed

The two connections place plates, holes, bolts and welds from the numbers you give. There is no design check of any kind, only edge and spacing warnings (from EN 1993-1-8 Table 3.3).

### Holes are not cut in the 3D view

Holes show as dark plugs. They are true holes in the NC files, the shop drawings and the IFC file. Copes are cut in the view.

### Weights are gross

The steel weight does not deduct holes or copes, and the bolt weight is not included.

### Millimeters only

Everything is typed and shown in millimeters, even for AISC shapes. Profile names keep their published form (W12x50, HSS6x6x1/4).

### No drawings to manage

There is no drawing list, no drawing editor, no general arrangement or assembly drawings. A shop drawing is made when you ask for it and downloaded; to change it, change the model and make it again.

### It runs in a browser

Saving downloads a file. Keyboard shortcuts that the browser keeps for itself cannot be used. Your models never leave your computer: see [Where your files go](common:your-files).

## What to keep doing in your current software

Today DetailCraft suits small frames, checking an idea, teaching, or producing NC files and shop drawings for simple beam and column work. Keep complex connections, general arrangement drawings, erection drawings, multi-user projects and anything listed in [What is not built yet](topic:not-built-yet) in the software you use now.
