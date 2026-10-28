---
title: General-arrangement and assembly drawings
summary: Make plans, elevations and one assembly drawing per assembly mark from the model, on A4 to A1 or ANSI B to D sheets, issue them with revision letters, and download them as PDF, DWG or DXF.
section: tasks
order: 62
keywords: hidden lines, hidden line removal, dashed, revision cloud, clouds, revision triangle, anchor bolt plan, anchor rods, rod schedule, base plate schedule, setting out, issue, revision, rev, revision table, re-issue, issue log, transmittal, dwg, dxf, cad file, export drawings, drawings, general arrangement, ga, plan, elevation, assembly drawing, sheet size, paper size, a4, a3, a2, a1, ansi b, ansi c, ansi d, tabloid, 11 x 17, 22 x 34, pdf, print, scale, title block, model ref
---

Choose {{ui:Drawings}} in the {{ui:Output}} group. DetailCraft makes the whole set from the model as it is now. Nothing is stored: change the model, open {{ui:Drawings}} again, and the set is made again.

## What is in the set

- **Plans**, one per level that has something on it, looking down.
- **Elevations**, one per grid line with members within 1 m of it.
- **The anchor bolt plan**, when the model has base plates: every base plate seen from above with its column and anchor rods, the grids with their dimensions, a typical detail of each rod pattern (BP1, BP2 …) with the rod spacing and edge distances, and two schedules: the anchor rods by mark (R1, R2 …: quantity, diameter, length, projection above the plate, embedment, grade) and the base plates by grid (column, pattern, plate, rods, top of plate). A column off the grid is located by its coordinates.
- **Assembly drawings**, one per assembly mark: front, top and end views, chained dimensions to holes and plate edges, the part list, loose bolts and rods, and weld and hole notes. Each welded plate has a fillet weld symbol in the front view (AWS A2.4): the arrow points at the joint, the triangle below the reference line means the arrow side, the figure to its left is the weld size in mm, and a circle where the arrow meets the line means all around.

**Hidden lines.** Plans and elevations leave out the lines of a part where a part wholly in front of it covers it (a beam seen end-on behind a column). Assembly drawings draw a plate dashed where the main part covers it: behind the web in the front view (seen from the part's −u side), under the top flange in the top view. Parts that sit at the same depth, or overlap in depth, are both drawn in full.

Each sheet has a title block with the drawing number, the scale, the model name, the date, the sheet number with the sheet size, and the model ref: a code made from the saved model, so a print can be matched to the model it came from.

## Choose the sheet size

Pick the size in the list at the top of the {{ui:Drawings}} window: {{ui:A4}}, {{ui:A3}}, {{ui:A2}}, {{ui:A1}}, {{ui:ANSI B}} (11 × 17 in), {{ui:ANSI C}} (17 × 22 in) or {{ui:ANSI D}} (22 × 34 in). Every sheet is landscape. The set is made again on the new size at once, and the same size is used for shop drawings. It is also in [DetailCraft settings](topic:settings#drawings), and this browser remembers it.

- A bigger sheet draws at a larger scale. Scales are always standard ones (1:10, 1:20, 1:25, 1:50, 1:100 and so on).
- Text stays the same size on every sheet, so it stays readable when printed.
- On a small sheet a long, thin assembly can get a larger scale across its depth than along its length. The view title then says both scales, and the dimensions govern.

## Issue drawings and revisions

The line above the preview says where the sheet shown stands: **Not issued yet**, **Rev B, issued … ; unchanged since**, or, in red, **Changed since rev B: needs re-issue**. The sheet list marks issued sheets with their revision and the ones that need re-issue.

- Type what the issue is for (it starts as "Issued for fabrication") and choose {{ui:Issue this sheet}}, or {{ui:Issue changed sheets}} for every sheet that is new or has changed since its last issue.
- The first issue of a sheet is revision A, then B, C and so on (I and O are skipped).
- The title block shows the drawing number with its revision ("A-B1  rev B"), and a revision table beside it lists the latest issues: revision, date and description. On a small sheet it shows as many as fit.
- A sheet "changes" when what is drawn on it changes: a moved member, a new hole, a renumbered mark. The date, the sheet count and the revision table itself do not count. Changing the sheet size changes every sheet.
- The issue log is saved in the model file, and issuing is one undo step. The [KISS file](topic:kiss-export) carries each drawing's latest revision.

### Revision clouds

From the second issue of a sheet on, every part or footing that changed is drawn inside a revision cloud with a revision triangle carrying the letter.

- While a sheet has changed since its last issue, the clouds show what changed since then, with the coming revision in the triangle (rev B issued and changed: the triangles say C).
- Once you issue it, the clouds stay on that revision's sheet, as on a printed set, until the sheet changes again.
- Each sheet clouds only changes it shows. Plans and elevations draw no holes or copes, so they cloud a member that moved, was turned or resized, or whose mark changed. Assembly drawings and the anchor bolt plan also cloud new or moved holes and copes.
- A renumbered mark counts: when a change makes a beam a new assembly, the beams whose mark moved up (B1 to B2) are clouded on plans and elevations.
- A part that was deleted leaves nothing to cloud.
- Sheets issued before revision clouds came in (DetailCraft 0.4) start clouding from their next issue.
- Clouds are in the PDF, DXF and DWG downloads, and never count as a change themselves.

## Download

Choose the format in the list beside the buttons, then {{ui:This sheet}} for the sheet shown or {{ui:All sheets}} for the whole set.

- **PDF**: to print or send. {{ui:All sheets}} makes one PDF, one page per sheet, each page the chosen size.
- **DWG**, in the 2018, 2013, 2010, 2007, 2004 or 2000 format: to open the sheets in a CAD program. Pick an older format for an older program.
- **DXF (R12)**: the version almost every CAD program opens.

A DWG or DXF sheet is drawn at full size: one drawing unit is one millimeter on the paper, so it prints at 1:1 and every view keeps the scale its title says. Each pen is its own layer: DC-OUTLINE, DC-THICK, DC-THIN, DC-HIDDEN, DC-CENTER, DC-GRID and so on, with its line weight (DWG) and its dashes. Words are text in the DetailCraft text style (Arial). Dimensions are the lines and figures you see, not dimension objects. {{ui:All sheets}} puts the DWG or DXF files in one zip, one file per sheet.

:::note
DXF R12 has no line weights and no fills: the layers keep their names, so set the pens by layer when you plot, and filled shapes are left out (the status bar says how many).
:::

Print at 100% (no "fit to page") on paper of the same size if you want the stated scales to hold.
