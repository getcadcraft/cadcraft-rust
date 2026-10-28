---
title: Model on a site plan from a DWG or DXF
summary: Bring a CAD site or floor plan in as lines on the ground, answer the units question, and trace it into faces and masses.
section: tasks
order: 100
keywords: site plan, dwg, dxf, cad, import, dynamic blocks, visibility states, text, hatch, spline, ellipse, 3d face, trace, floor plan, lot, property line, units, underlay, context
---

ModelCraft can bring in the lines of a 2D CAD drawing, a site plan or a floor plan, and lay them on the ground so you can model on top of them.

## Import the drawing

1. Click {{ui:Import}} in the {{ui:File}} group (or drop the file on the 3D view).
2. Pick a `.dwg` or `.dxf` file.

   A large DWG takes a moment: the status bar says "Reading plan.dwg…" until it is in.

3. If the drawing does not say what its units are, a dialog titled "Drawing units" asks: "What is one drawing unit?" Choose {{ui:Inches}}, {{ui:Feet}}, {{ui:Millimeters}} or {{ui:Meters}}. Most US site plans are drawn in feet, architectural plans in inches, and metric plans in millimeters.

   The drawing arrives as a group of edges on the ground, selected, and the view zooms to it. The status bar says how many edges came in, for example "Imported site.dwg: 1697 edges on the ground, in a group. Explode it to draw on it directly."

## What comes in

- Lines, polylines (including their arcs), circles and arcs, and blocks with everything in them, from **model space** only. Curves become short straight pieces.
- Lines, polylines, arcs, circles, ellipses and splines land flat on the ground, as edges.
- **Hatches** become faces on the ground, with their islands as holes, so you can push/pull a hatched area straight away.
- **3D faces** and solid fills become faces; 3D faces keep their heights.
- **Text** (single-line text, multiline text and attribute values) is drawn as edges in a simple single-stroke lettering, at the text's own height, angle and alignment. Its font is not the drawing's, so the words may run a little longer or shorter. Characters the lettering has no shape for are drawn as small boxes and counted.
- **Dimensions** are drawn as the drawing shows them (their lines, arrowheads and text, as edges and faces).
- **Dynamic blocks** come in as the drawing shows them: each placed block in the visibility state it was saved with, and only that state's lines. The status bar lists them with the states shown and the other states each one has, for example "Receptacle shown as Duplex ×38, GFI ×7 (states: Duplex, GFI)". ModelCraft does not switch states.
- Not imported: points, tables, multileaders, regions, 3D solids, images and other object types. The status bar lists them by type, such as "Not imported: 12 POINT." Anything on a layout (paper space) is left out too, without being listed.
- Layers are not kept: all lines come in alike.

If the drawing is far from its origin (state plane coordinates, for instance), it still imports, but it may land far from ModelCraft's origin. Drawings reaching more than 10,000 km from their origin are refused: move the drawing nearer to 0,0 in the CAD program first.

## Trace it into faces

The imported lines rarely close neatly into faces by themselves. Two ways to work:

**Keep the plan as an underlay (recommended)**

1. Leave the plan as its own group.
2. Draw your building on top with {{ui:Rectangle}} and {{ui:Line}}, snapping to the plan's corners ("Endpoint") and edges ("On edge").
3. Push/pull your faces up. The plan stays untouched underneath, and you can erase the whole group later with one click.

Snapping works across groups: you can snap to the plan's corners without opening it.

**Draw on the plan's own lines**

1. Select the plan group and click {{ui:Explode}}. Its lines become loose edges.
2. Where a closed outline should be a face, draw one line over any of its edges with {{ui:Line}}, from corner to corner. When the outline is closed and flat, the face usually fills in. If it doesn't, look for a gap or an overlap in the outline and draw over it.
3. Push/pull it.

This works best on a clean plan. On a busy survey, tracing on top is faster.

## Troubles

- "This is a binary DXF. Save it as ASCII DXF and try again." Save the DXF again from the CAD program as ASCII (text).
- "…has no lines, arcs or circles in model space to bring in." The drawing is probably all on layouts, or all text and hatches.
- "This drawing could not be read (…). It may be damaged, or saved in a DWG version the reader does not handle…" The brackets say why, for example "DWG version AC1009 is not supported". Save it as a DXF, or as a DWG from AutoCAD 2000 to 2018, from the CAD program.
- The plan is the wrong size: you picked the wrong unit. Undo ({{key:Mod+Z}}) and import again, or [scale](topic:rotate-scale) the group by the right factor (12 between feet and inches, 25.4 between inches and millimeters).

To turn contour lines into ground you can sculpt, see [Terrain](topic:terrain).
