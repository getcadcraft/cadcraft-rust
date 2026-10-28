---
title: Make a drawing with views, a section and GD&T
summary: Open the drawing sheet, add a section and a detail view, place datums, feature control frames and surface finish symbols, choose the sheet size, fill in the title block and the revisions, and export the sheet as a PDF.
section: tasks
order: 140
keywords: hole table, hole chart, hole positions, tags, drawing, 2d drawing, sheet, sheet size, a4, a3, a2, a1, a0, ansi, title block, revision, revision block, drawing number, material, pdf, views, orthographic, third angle, isometric, section view, detail view, gd&t, datum, feature control frame, tolerance, position, flatness, surface finish, roughness, title block, print
---

PartCraft makes a drawing sheet from the part: front, top and right views in third-angle projection, an isometric view, overall sizes and a title block on a sheet from A4 to A0 or ANSI A to E. On top of that you can add a section, a detail view and GD&T symbols. The sheet updates when the part changes, and its settings are saved in the part file.

## Open the drawing sheet

1. Build the part. (With nothing built: "Build a solid first, then make its drawing.")
2. Choose {{ui:Drawing}} in the toolbar.

The 3D view is replaced by the sheet. The toolbar shows {{ui:Datum}}, {{ui:Frame}}, {{ui:Finish}}, {{ui:Detail}}, {{ui:PDF}} and {{ui:Back}}, and the right panel is headed **Drawing**. The status bar says how the views were made: "Drawing (curves are drawn as facets). Datum, Frame and Finish place GD&T on a view; the panel sets a section and a detail view." The views are drawn from the mesh kernel's facets, so round edges are short straight pieces; true arcs need an exact kernel, which this version does not have.

Move round the sheet: roll the wheel to zoom, drag with the right or middle button to pan, press {{key:F}} to fit the sheet. With the sheet focused, {{key:Plus}} and {{key:Minus}} zoom and {{key:Shift}} with the arrow keys pans.

Choose {{ui:Back}}, {{ui:Back to the model}} or press {{key:Esc}} (with nothing selected and no tool active) to return to the 3D model.

## What is on the sheet

- **The sheet you choose**, landscape (A3 unless you pick another), with a border and a **title block**: title, company, drawing number, revision, material, drawn / checked / approved by, date, scale, sheet size, "Units: millimetres · Third-angle projection", and whether the views are exact or faceted.
- A **revision block** in the top-right corner once you add a revision.
- **Front, top and right views**: the top view above the front view, the right view to its right. Hidden edges dashed.
- An **isometric view** where there is room.
- **Overall sizes**: width and height on the front view, depth on the top view.
- One **scale** for the main views from the ISO series (1:1, 1:2, 1:5 … 2:1, 5:1, 10:1), picked so they fit.

## Choose the sheet size

On the drawing sheet, with nothing selected, pick {{ui:Sheet}} in the right panel: **A4, A3, A2, A1, A0** (ISO 216) or **ANSI A, B, C, D, E** (11 × 8.5 to 44 × 34 in). The views are laid out again at the largest ISO scale that fits, so a bigger sheet can show the part bigger.

## Fill in the title block

The right panel's **Title block** fields go straight onto the sheet: {{ui:Title}}, {{ui:Drawing no.}}, {{ui:Revision}}, {{ui:Company}}, {{ui:Drawn by}}, {{ui:Checked by}}, {{ui:Approved by}}, {{ui:Date}} (year-month-day), {{ui:General tolerance}} and {{ui:Finish}} (the last two print just above the title block). Type in a box and press {{key:Enter}} or click away. An empty box leaves its cell blank, except:

- **Title** shows the part's name until you type one.
- **Date** shows today until you type one.
- **Revision** shows the newest revision in the list until you type one.

**Material** comes from the part: click empty space in the model, then pick {{ui:Material}} in the Part panel. The same material gives Measure's mass properties their density.

## Add revisions

Under **Revisions**, click {{ui:Add revision}}. Each new row takes the next letter (A, B, C …, after Z comes AA) and today's date. Type what changed and who made the change. {{ui:Delete}} removes a row. The revision block in the top-right corner of the sheet lists them, oldest first, up to 20.

Sheet size, title block, revisions and material are saved in the part file, and each change is one step of {{ui:Undo}}.

## Add a section view

The part is cut by a plane and the cut is hatched. Like the other views, the section is drawn from facets, so round edges are short straight pieces.

1. In the right panel, set **Section A-A** to **Of the front view**, **Of the top view** or **Of the right view**.
2. The section appears on the sheet, hatched where the cut passes through material, with the cutting line and its "A" arrows on the view it is taken from.
3. Set **Cut at (mm)** to move the cutting plane. It starts through the middle of the part.

Set **Section A-A** back to **None** to remove it.

## Add a detail view

1. Set **Detail B** to the view to enlarge (or choose {{ui:Detail}} in the toolbar).
2. Click the center of the area to enlarge, on that view. A circle labeled "B" marks it, and the enlarged "Detail B" appears on the sheet with its own scale, for example "Detail B (2:1)".
3. Set **Detail radius (mm)** for the size of the circle and **Detail scale** for the enlargement: 2, 4, 5 or 10 × the sheet.

A detail can be taken from the front, top, right or section view, not from the isometric. If there is no room for every view, the panel says "No room on the sheet for: detail. Make the detail smaller or drop a view."

## Add a hole table

A hole table lists the part's holes with their positions, for drilling or CNC: choose **Hole table** › **Of the top view** (or front, or right) in the panel.

- Every hole seen end-on in that view gets a tag beside it: **A1**, **A2** … for the first size, **B1** … for the next.
- The table lists each tag, its **X** and **Y** in millimetres, and its size: "Ø14 THRU", "Ø6.6 × 10 DEEP", a counterbore or countersink, or a tapped hole's thread callout.
- X and Y are measured from the lower-left corner of the view's outline, marked **X0 Y0** on the sheet.
- Holes made by patterns are included. Holes that are not square to the view are counted in the panel and left out of the table.

The table goes in a free area of the sheet and into the PDF.

## Place GD&T and surface finish symbols

1. Choose {{ui:Datum}}, {{ui:Frame}} (a feature control frame) or {{ui:Finish}} (a surface finish symbol).
2. Click the point on a view where it attaches. It snaps to the nearest drawn line within 2.5 mm.
3. The symbol appears with a leader, selected, and the right panel shows its settings.

Drag a symbol to move it; its leader stays on the point. With a symbol selected, the arrow keys nudge it 1 mm. {{key:Delete}} or {{key:Backspace}} (or {{ui:Delete}} in the panel) removes it; {{ui:Done}} or {{key:Esc}} deselects it. Every change is in the undo list.

### Datum

**Letter**: A to Z, without I, O and Q (they read as 1, 0 and O). Each new datum takes the next free letter.

### Feature control frame

| Setting | Choices |
|---|---|
| **Tolerance** | Straightness, Flatness, Circularity, Cylindricity, Profile of a line, Profile of a surface, Perpendicularity, Angularity, Parallelism, Position, Concentricity and Symmetry (both removed in Y14.5-2018, and marked so), Circular runout, Total runout |
| **Zone (mm)** | The tolerance value, default 0.1 |
| **Diameter zone (⌀)** | For characteristics with a cylindrical zone; on by default for Position |
| **Material** | RFS (none), MMC (Ⓜ), LMC (Ⓛ), where the characteristic allows it |
| **Primary / Secondary / Tertiary datum** | Up to three datum letters, each with a **… modifier** (MMB, LMB) where allowed |

PartCraft checks the frame against the rules of ASME Y14.5 and says what is wrong at the top of the panel, for example "Flatness is a form tolerance: it takes no datum references." or "Position needs at least one datum reference." The line **Reads** spells out the frame.

### Surface finish

| Setting | Choices | Default |
|---|---|---|
| **Ra (µm)** | Roughness average | 1.6 |
| **Process** | any process, machined, no material removed | machined |
| **Note** | Up to 40 characters, such as a lay or a process | |

With nothing selected, the panel lists **Notes on the sheet**, each with **Select**.

## Export the PDF

Choose {{ui:PDF}} in the toolbar, {{ui:Export PDF}} in the panel, or {{ui:Export}}, then {{ui:Drawing (PDF): views, section, detail, GD&T}} from the model. The PDF downloads as `<part name> - drawing.pdf`, and the status bar lists what is on it.

Print it at "Actual size" on paper of the sheet's size to keep the scale true.

## Limits

| Not built yet | Do this today |
|---|---|
| Dimensions other than the three overall sizes | Add them on the PDF with a markup app, or export a sketch or face as DXF and dimension it in a 2D CAD program |
| More than one section or detail; broken-out, auxiliary and projected views | Not available |
| First-angle projection, moving views, portrait sheets | Third angle, automatic layout, landscape |
| Free text notes and tables on the sheet (other than the title and revision blocks) | Use the finish note, or add text on the PDF |
| Drawings of assemblies | Not available |

Characters outside the Western European set print as "?" (the PDF uses the reader's built-in Helvetica). The GD&T symbols are drawn as lines, so they print in any reader.
