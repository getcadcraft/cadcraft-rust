---
title: Model in 3D
summary: See the drawing in 3D on the 3D tab and make simple solids from it: extrude (with a taper or along a path), revolve, push and pull faces, boxes, cylinders and wedges, join, subtract and intersect them, then move, copy, rotate and erase them, and place flat 2D views of them on a sheet.
section: tasks
order: 128
keywords: 3d, three d, solid, solids, extrude, ext, revolve, rev, sweep, taper, path, presspull, press pull, push pull, box, cylinder, cyl, wedge, 3dorbit, orbit, view, vscurrent, visual style, shaded, hidden line, wireframe, x-ray, xray, iso, perspective, parallel, glb, gltf, obj, stl, export3d, stlout, objout, glbout, 3dface, view cube, union, uni, subtract, su, intersect, in, boolean, combine, join solids, viewbase, flatshot, flat view, 2d view, drawing view, front view, top view, side view, hidden lines
---

## What you will make

A 1000 × 600 room footprint extruded into a block 400 high, a face pulled up, and a cylinder beside it — then the model saved with the drawing and exported for a 3D viewer or a printer.

The {{ui:3D}} tab sits beside {{ui:Model}} under the drawing. It shows the model-space drawing lying on the ground in its layer colors (layers that are off or frozen stay hidden) and the solids you make. The first time it opens, the 3D view loads; after that it switches at once.

## Extrude a shape

1. On the {{ui:Model}} tab, type `REC` and press {{key:Enter}}. Type `0,0`, {{key:Enter}}, `@1000,600`, {{key:Enter}}.
2. Click the {{ui:3D}} tab. The toolbar shows the {{ui:3D}} group in place of the drawing groups.
3. Click {{ui:Extrude}} (or type {{cmd:EXTRUDE}}, `EXT`). Click inside the rectangle, then press {{key:Enter}}.
4. Type `400` and press {{key:Enter}}. The history says "Extruded 1 solid, 400 high" with its volume.

Closed polylines (arcs included), rectangles, polygons, circles and ellipses extrude; lines and open polylines do not, and the history says why. You can also select the shapes on the {{ui:Model}} tab first and type `EXTRUDE` there. Instead of typing the height you can click it: the outline follows the cursor up and down. A negative height goes down. Type `D` at the height prompt to delete the 2D shapes after extruding (`D` again keeps them).

At the height prompt, `T` (Taper angle) asks for an angle in degrees: the sides lean in by it, so the top is smaller (a negative angle makes it larger); an angle too steep for the shape is refused. `P` (Path) sweeps the shapes along a path instead (see below).

## Revolve and sweep

- {{cmd:REVOLVE}} (`REV`, or {{ui:More}} › {{ui:Revolve about an axis}}): select closed shapes, press {{key:Enter}}, then click the two ends of the axis (or type `X` or `Y` for the drawing's axis through 0,0), then type an angle or press {{key:Enter}} for all the way round. The shape turns about the axis and stands up out of the ground: draw half a vase's outline beside a line, revolve it about the line. The shape may touch the axis but not cross it. A full turn has 48 steps.
- {{cmd:SWEEP}} (or {{ui:More}} › {{ui:Sweep along a path}}, or `P` at EXTRUDE's height prompt): select closed shapes (the profile), press {{key:Enter}}, then click the path: a line, polyline, arc, circle or ellipse. The profile stands up across the path, its middle on it, and runs along it; corners are mitered, and a closed path makes a ring. A small rectangle swept along a room's outline makes a skirting; a circle along a polyline makes a pipe.

## Push and pull a face

1. Click {{ui:Press/pull}} (or type {{cmd:PRESSPULL}}).
2. Click the top face of the block.
3. Type `100` and press {{key:Enter}}, or move the cursor and click. The block is 500 high.

## Boxes, cylinders and wedges

- {{cmd:BOX}}: click or type two corners, then a height.
- {{cmd:CYLINDER}} (`CYL`): the center, the radius, then a height. Cylinders and extruded circles have 48 sides, shaded smooth.
- {{cmd:WEDGE}} (`WE`): two corners and a height. It is full height at the first corner and slopes to nothing at the other.

On the 3D tab a click lands on the face or edge under the cursor, else on the ground, so a box can stand on another solid.

## Move, copy, rotate and erase solids

On the 3D tab, a click selects a solid; drag left to right for a window, right to left for a crossing. Then:

- {{cmd:MOVE}}: click a base point and where it goes, or type `<100,0,0>` (or `@100,0`) for a distance along X, Y and Z.
- {{cmd:COPY}}: the same, leaving the original.
- {{cmd:ROTATE}}: type an angle (it turns about the middle of the selection, around Z), or click the center, a start point and the angle.
- {{cmd:ERASE}} or {{key:Delete}}.

## Join, subtract and intersect solids

Open {{ui:More}} in the 3D toolbar, or type the command:

- {{cmd:UNION}} (`UNI`): select the solids, press {{key:Enter}}: they become one solid.
- {{cmd:SUBTRACT}} (`SU`): select the solids to keep and press {{key:Enter}}, then the ones to take away and press {{key:Enter}}. A hole through a block: draw a cylinder through it, then subtract the cylinder.
- {{cmd:INTERSECT}} (`IN`): select the solids, press {{key:Enter}}: only what they all have in common is left.

The history says the volume of the result. Each is one undo step. The first one you run loads the 3D boolean engine (about 0.5 MB; included in {{ui:Save for offline}}). Only closed solids can be combined: a solid with open edges (from another program's loose 3D faces) is refused with that reason. The result is one solid; curved faces stay made of flat facets, as the cylinder was.

Double-click a solid to edit its faces and edges on their own; {{key:Esc}} closes it again. Every 3D change is one undo step in the same history as the drawing: {{key:Mod+Z}} undoes the last thing you did on either tab.

## Flat views on a sheet

1. Open a sheet tab (or stay on {{ui:Model}}), then type {{cmd:VIEWBASE}} (or `FLATSHOT`).
2. Choose the view: {{key:Enter}} for the front, or `T` (top), `R` (right), `L` (left), `BA` (back), `BO` (bottom), `I` (isometric).
3. {{key:Enter}} shows hidden lines, dashed; `H` leaves them out.
4. {{key:Enter}} takes the scale CadCraft suggests: on a sheet, the largest standard scale (such as 1:20) at which the view fits in under half the paper. Or type one: `1:50`, `2:1`.
5. Click, or type, where the view's center goes.

The view is one block reference, `VIEW-FRONT-1` and so on: visible edges on layer VIEW-VISIBLE (0.5 mm), hidden ones on VIEW-HIDDEN (dashed, 0.25 mm). Move it, dimension it and plot it like any other drawing. It is a picture of the solids when you made it: after changing them, erase the view and run {{cmd:VIEWBASE}} again. For more views, run it once per view.

## Look around

Drag with the wheel to orbit, {{key:Shift}} + wheel drag or the right button to pan, and turn the wheel to zoom at the cursor. On a trackpad, two fingers orbit and a pinch zooms. A right click without dragging is {{key:Enter}}, as on the other tabs. `3DORBIT` lets the left button orbit until you press {{key:Enter}} or {{key:Esc}}.

- {{ui:Views}} (or {{cmd:VIEW}}): Iso, Top, Bottom, Front, Back, Left, Right, Perspective or Parallel, and Zoom to everything.
- {{ui:Style}} (or {{cmd:VSCURRENT}}, `VS`): Shaded, Shaded with edges, Hidden line, Wireframe or X-ray.

Points you click snap to the solids (corners, edges, faces) and to the 2D drawing on the ground: the ends and middles of its lines and arcs, and points along them. The 2D lines are never selected on the 3D tab. When a command asks for 2D objects (EXTRUDE, REVOLVE, SWEEP), a click picks one, a drag left to right takes what is wholly inside the window, and a drag right to left takes what it crosses, as on the {{ui:Model}} tab.

A view cube sits at the top right of the 3D tab: click a face, edge or corner to look from there, drag it to orbit, and the house button goes back to the iso view. That is {{ui:Automatic}}, the start of {{ui:Corner indicator}} in [CadCraft settings](topic:settings). Set to {{ui:View cube}}, the {{ui:Model}} tab shows a cube too, looking from the top; clicking any other side opens the 3D tab looking from there. Set to {{ui:Axis icon}}, both tabs show the axis marker instead.

## Save and export

Saving keeps the 3D model, in DWG and in DXF. Every face goes in as a 3D face on the layer `CC-3D`, so other programs show the solids, and CadCraft keeps its own exact copy in the file, which it reads back when you open the drawing again, with the 3D view as it was (camera, projection and visual style). 3D faces made by other programs come in on the 3D tab as loose faces, kept together as one group. A 3D face inside a block comes in once for each reference to that block in the model, placed and turned as the reference is; in the block itself, and on a sheet, a 3D face is drawn flat, as its outline.

{{ui:Export}} in the 3D group downloads the solids alone: GLB for viewers and the web ({{cmd:GLBOUT}}), OBJ ({{cmd:OBJOUT}}) or STL for 3D printing, in millimeters ({{cmd:STLOUT}}). {{cmd:EXPORT3D}} asks which.

:::note
The 3D model is real size. A drawing without units counts as millimeters, so `1000` is one meter. If you change the drawing's units later (Properties, with nothing selected), the solids are scaled with them so they stay on the drawing, and the history says so. One {{key:Mod+Z}} undoes the change of units and the scaling together.
:::
