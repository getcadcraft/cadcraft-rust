---
title: View a DWG or DXF drawing
summary: Open a DWG or DXF, move around model space and sheets, read coordinates, and see what is not drawn yet.
section: tasks
order: 100
keywords: dynamic blocks, visibility states, dwg, dxf, cad, drawing, 2d, view, viewer, pan, zoom, model space, paper space, layout, coordinates, open dwg
---

## What you'll do

Open a CAD drawing in 2D, move around it, switch between model space and its sheets, and read coordinates. BimCraft shows DWG and DXF drawings; it does not edit or save them. To change one, click {{ui:Edit in CadCraft}} and the drawing opens in the CadCraft drafting app in a new tab.

## Steps

1. Click {{ui:Choose a file}}, or drop the drawing onto the page.
2. Choose a `.dwg` or `.dxf` file.
3. Wait while the status line says "Reading the drawing…". Large drawings take a few seconds.
4. Look at the tabs above the drawing: {{ui:Model}} first, then one tab per sheet, in the drawing's own order.
5. Click a sheet tab to see that sheet: the paper, with the model shown through its viewports.
6. Click {{ui:Model}} to go back to model space.
7. Move the pointer over the drawing and read the X and Y coordinates in the corner.

## What you should see

- The drawing, fitted to the window. Colors follow the drawing; white lines (color 7) draw in the theme's ink, and pale colors are darkened on the light theme so they stay readable.
- The {{ui:Drawing}} panel on the left: file, size, format (for example "AutoCAD 2018 (AC1032)"), units, entity and layer counts, the number of sheets, and linked drawings and equipment when there are any.
- The {{ui:Layers}} list on the left.
- On the right: how to move around, linked drawings and images, and **Not drawn yet**.

## Moving around

:::mouse
| To | Do this |
|---|---|
| Pan | Drag with any mouse button |
| Zoom | Scroll the wheel; the point under the pointer stays put |
| Fit | Double-click, press {{key:F}}, or click {{ui:Fit}} |
:::

:::mac mouse
On a trackpad, scrolling with two fingers **zooms**, it does not pan. Pinch zooms too. To pan, click and drag.
:::

:::touch
Drag with one finger to pan. Pinch zoom is not built yet for drawings: tap {{ui:Fit}} to see the whole sheet, and use a mouse or trackpad to zoom in.
:::

## Dynamic blocks

A dynamic block is drawn as the drawing shows it: each placed block in the visibility state the drawing was saved with ("Plan" or "Front", "Single bowl" or "Double bowl"). On the right, **Dynamic blocks** lists every one placed in model space: how many are placed, the states they show, and every state the block has. In {{ui:3D}}, click a placed block to see **Dynamic block**, **State shown** and **Other states** in Properties. BimCraft does not switch states.

## Not drawn yet

Some kinds of objects are not drawn in this build. They are counted under **Not drawn yet** on the right ("These parts of the drawing are skipped for now, not guessed at."), so you know something is missing. Lines, polylines, arcs, circles, ellipses, splines, hatches and solid fills, text and multiline text, blocks with attributes, dimensions, leaders and multileaders, tables, images and viewports are drawn. 3D faces, polyface meshes and polygon meshes are drawn as their edges seen from above; switch to {{ui:3D}} to see them as surfaces.

A binary DXF does not open: BimCraft says so. Save it again as an ASCII DXF, or as a DWG, from the program that made it.

## Common mistakes

- **The drawing looks empty.** Some layers may be off or frozen in the drawing itself; they start unticked in {{ui:Layers}}. Click {{ui:All on}}. If the status says "*name* is an empty drawing: it holds no lines, text or blocks.", there is nothing in it.
- **A sheet shows an empty frame.** The sheet may rely on linked drawings. See [Find linked drawings and pictures](topic:dwg-linked-files).
- **Text looks a little different.** BimCraft draws text in a narrow sans-serif font, not the drawing's own fonts, so text can be a little wider or narrower than in the program that made it.
- **"*name* could not be read. It may be damaged, or saved in a format the reader does not handle yet."** The bracket at the end names the file's format code. Save it from the program that made it as an older DWG or as DXF, and try again.
- **Expecting to measure, draw or edit.** BimCraft only views drawings. Click {{ui:Edit in CadCraft}} to open the same file in the CadCraft drafting app, which edits it and saves a copy as DWG or DXF. Your original file is not changed, and linked drawings stay in BimCraft. The button shows only where CadCraft is open to you.

## Next

- [See a DWG or DXF drawing in 3D](topic:dwg-in-3d)
- [Work with layers and sheets](topic:dwg-layers-sheets)
- [Check a drawing in print view](topic:print-view)
