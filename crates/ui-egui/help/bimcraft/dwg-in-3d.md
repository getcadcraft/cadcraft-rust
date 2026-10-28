---
title: See a DWG or DXF drawing in 3D
summary: Switch a drawing's model space to 3D to see elevations, thickness, 3D faces, meshes and blocks, orbit round it, click an object to see its properties, and see what is not drawn yet.
section: tasks
order: 105
keywords: 3d, dwg, dxf, drawing, model space, orbit, section, cut, clipping, pick, select, properties, attributes, handle, elevation, thickness, extrusion, 3dface, polyface, mesh, 3d polyline, blocks, views, perspective, parallel, extents, acis, solids
---

## What you'll do

Look at a drawing's model space in 3D. Lines and polylines sit at their real heights, objects with a thickness stand up as walls, and 3D faces, meshes and 3D blocks show as surfaces you can orbit round. The sheets stay 2D.

## Steps

1. Open a DWG or DXF. See [View a DWG or DXF drawing](topic:view-dwg).
2. Click {{ui:3D}} in the bar above the drawing, or press {{key:3}}.
3. Wait a moment the first time: the status line says "Building the drawing in 3D…", then how many lines and faces it shows.
4. Orbit, pan and zoom (see the table below).
5. Click a view in the second bar: {{ui:Iso}}, {{ui:Top}}, {{ui:Bottom}}, {{ui:Front}}, {{ui:Back}}, {{ui:Left}} or {{ui:Right}}.
6. Click {{ui:Perspective}} to switch between a perspective and a parallel view.
7. Click {{ui:Extents}} or press {{key:F}} to see the whole drawing again.
8. Click {{ui:3D}} again, press {{key:3}}, or click {{ui:Model}} or a sheet tab to go back to 2D.

## What you should see

- Model space in 3D, Z up, in the drawing's own colors. Faces are a shade lighter than their edges.
- Layers you turn off in {{ui:Layers}} disappear from the 3D view too.
- On the right: the number of lines and faces, the size of the drawing in X, Y and Z, **In the 2D view only** and **Not drawn yet**.

## Cut it with a section plane

Click {{ui:Section}} on the 3D bar, or press {{key:X}}. The bar under it works as it does for IFC models (see [Cut an IFC model with a section plane](topic:section-plane)): cut from the {{ui:Top}}, {{ui:Front}} or {{ui:Side}}, drag or type the offset — in the drawing's own units here — and {{ui:Flip}} to keep the other side. What the plane cuts away cannot be picked. Going back to 2D takes the cut away.

## See an object's properties

Click an object in the 3D view. It turns the pick color and {{ui:Properties}} shows what the file says about it: its type, layer, color, linetype and handle, then its own data, such as a line's start, end and length, a circle's center and radius, a 3D face's corners, or a block reference's block, insertion point, scale, rotation and attribute values.

- A block reference picks as a whole, the blocks nested in it too.
- A click a few pixels from a line picks the line.
- Hold {{key:Mod}} and click to pick more objects, or to take one out; {{ui:Properties}} then counts them by type.
- {{key:Esc}}, or a click on empty space, clears the pick.
- Objects on layers you turned off cannot be picked.

## Moving around

:::mouse
| To | Do this |
|---|---|
| Orbit | Drag with the middle button, or with the left button |
| Pan | Drag with the right button, or hold {{key:Shift}} and drag |
| Zoom | Scroll the wheel; it zooms toward what is under the pointer |
| See it all | {{ui:Extents}} or {{key:F}} |
| Look from a side | Click a face, edge or corner of the view cube in the bottom-right corner |
:::

The view cube can be swapped for an axis icon, or hidden, in {{ui:Settings}}. See [BimCraft settings](topic:settings).

:::mac mouse
On a trackpad, swipe with two fingers to orbit, hold {{key:Shift}} and swipe to pan, and pinch to zoom.
:::

:::touch
Drag with one finger to orbit. Pinch with two fingers to zoom, and move them together to pan.
:::

## What is drawn in 3D

| In the drawing | In 3D |
|---|---|
| Lines, 3D polylines | At their real heights |
| Polylines, circles, arcs, ellipses | In their own plane, at their elevation, tilted when the drawing tilts them |
| Thickness on lines, polylines, circles and arcs | Walls of that height; circles are closed top and bottom |
| 3D faces, polyface meshes, polygon meshes | Shaded surfaces with their visible edges |
| Blocks | Placed, turned and scaled in 3D, nested blocks too |
| Hatches | Their outlines |
| Plain 2D linework | Flat, where it was drawn |

## In the 2D view only

Text, multiline text and the visible attributes of blocks are drawn in 3D, in their own plane at their own height, and a click on them picks them. Wide polylines show their width as a flat band. Multileaders, pictures and hatch patterns are counted under **In the 2D view only**: see them on the {{ui:Model}} tab or the sheets.

## Not drawn yet

Solids and surfaces stored as ACIS bodies (3D solids, bodies, regions and surfaces) and subdivision meshes are listed under **Not drawn yet** with how many there are. They are skipped, not guessed at.

## Common mistakes

- **Everything looks flat.** The drawing is 2D: its lines have no height. Click {{ui:Front}} to check; a flat drawing shows as a line.
- **A solid model shows nothing.** Solids made with a solid modeler are ACIS bodies, listed under **Not drawn yet**. Export the model as IFC or as 3D faces from the program that made it.
- **The drawing is a tiny cluster in the middle.** Something far away in model space, such as an old copy of a plan, makes the extents large. Zoom in with the wheel where you want to look.

## Next

- [Work with layers and sheets](topic:dwg-layers-sheets)
- [Cut an IFC model with a section plane](topic:section-plane)
