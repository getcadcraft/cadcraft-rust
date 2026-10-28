---
title: Get started with PartCraft
summary: What PartCraft does today, what your browser needs, and a first part in ten minutes, from sketch to saved file.
section: start
order: 10
keywords: start, first run, quick start, introduction, tutorial, first part, plate, parametric, part modeler
---

PartCraft is the parametric part and assembly app of the CadCraft family. You draw a 2D sketch, tie it down with relations and dimensions, turn it into a solid with features such as extrude, revolve and hole, and change any number later: the part rebuilds from its history. Parts then go into assemblies, where joints place them and let you check the motion.

What this build does:

| Job | What you get |
|---|---|
| Sketches | Lines, rectangles, circles, arcs, points and centerlines, with relations and dimensions, and a live state: under, fully or over defined |
| Solid features | Extrude, cut, revolve, revolved cut, simple and counterbored holes, linear and circular patterns, mirror, fillets and chamfers on straight edges between flat faces, and shells and drafts on flat faces. Sweeps, lofts and the other features that need an exact kernel are not built yet in this version: see [What is not built yet](topic:not-built-yet) |
| History | Edit any feature's numbers, suppress, roll back, reorder and delete, with undo |
| Assemblies | Parts joined by rigid, revolute and slider joints, a clash check and a motion check |
| Drawings | An A3 sheet with three views, an isometric, a section, a detail view, overall sizes, datums, feature control frames and surface finish symbols, exported as PDF |
| Files | Its own part and assembly files; STL, 3MF, faceted STEP, DXF and PDF out |

Everything runs in your browser, on your computer. **Nothing is uploaded.** See [Files: open, save and export](topic:files) for where files go.

## What you need

- A current Chrome, Edge, Firefox or Safari with **WebGL** (3D graphics) and **WebAssembly** turned on. Both are on by default. The solid geometry is built by a WebAssembly mesh kernel that loads with the app: see [The mesh kernel](topic:solid-kernels).
- A mouse is easiest. A trackpad works, and touch screens work for viewing and simple sketching: see [Trackpad, touch and small screens](topic:trackpad-and-touch).
- A screen at least 1280 pixels wide shows the whole toolbar in one row. Narrower windows scroll the toolbar sideways; on a phone the panels open as sheets from a bar at the bottom.

More about browsers: [Browsers and computers](common:browsers).

:::note
PartCraft runs in the browser, and on the desktop inside CadCraft Studio. Cloud saving is not available yet: you open parts from your computer and save them back to it as files.
:::

## Open PartCraft

1. Open PartCraft.
2. Wait while the screen says "Loading the solid kernel…". This takes a moment the first time.
3. You see an empty 3D view with three base planes, the history on the left ("Part1", then Front plane, Top plane, Right plane) and a box in the view headed "Start a part".
4. The chip near the right end of the status bar says "Mesh kernel": PartCraft builds every part with the mesh kernel in this version, so round faces are flat facets. You can start modeling straight away.

If the view says the browser cannot show 3D, or the kernel did not load, see [Troubleshooting](topic:troubleshooting#the-app-does-not-start).

## Your first part: a plate with a hole

You will make a 60 × 40 mm plate, 10 mm thick, with a 6 mm hole through it. Units are millimeters.

### Sketch the outline

1. Click **Front plane** in the history on the left. The plane highlights in the view.
2. Choose {{ui:Sketch}} in the toolbar, or press {{key:S}}. The view turns to look straight at the plane, and the Line tool is active.
3. Press {{key:R}} for the Rectangle tool.
4. Move the pointer onto the origin (the small red circle with a cross). A small square shows that it snaps. Click.
5. Move up and to the right and click again. You have a rectangle. Its lines are blue: blue means they can still move.

### Give it sizes

1. Press {{key:D}} for the Dimension tool.
2. Click the top line, then click a little above it. A number box opens with the current length.
3. Type `60` and press {{key:Enter}}. The rectangle changes to 60 mm wide.
4. Click the left line, then click a little to the left of it. Type `40` and press {{key:Enter}}.

The chip at the top left of the view now says **Fully defined** and the lines turn to ink (black on the Paper theme, light on the Model theme). Every point and size is fixed.

:::tip
If you started the rectangle away from the origin, the chip still says "Under defined · 2 degrees of freedom". Press {{key:S}} for Select, click one corner, {{key:Shift}}+click the origin, then choose {{ui:• Coincident}} in the right panel.
:::

### Extrude it

1. Choose {{ui:Exit}} in the toolbar, or press {{key:Esc}} until the sketch closes (usually twice).
2. Choose {{ui:Extrude}}. A 10 mm solid appears. "Extrude1" is added to the history, with "Sketch1" indented under it.
3. The right panel shows the extrude's settings. To change the thickness, type a new number in **Depth (mm)** and press {{key:Enter}} or {{key:Tab}}. The part rebuilds at once.

### Add the hole

1. Click the large flat face at the front of the plate (the end of the extrusion). The whole face highlights and the right panel says **Face**.
2. Choose {{ui:Sketch on this face}} in the right panel.
3. Press {{key:P}} for the Point tool and click near the middle of the face.
4. Choose {{ui:Exit}}.
5. Choose {{ui:Hole}}. A 6 mm hole goes through the plate. Change **Diameter (mm)** in the right panel if you like.

The **Part** box at the bottom left now reads a volume, a surface area, the size (60 × 40 × 10 mm) and "Through holes: 1".

### Save it

1. Press {{key:Mod+S}} or choose {{ui:Save}}.
2. Your browser downloads `Part1.partcraft.json` to its downloads folder. The file holds the whole history: open it again with {{ui:Open}} to keep editing.

To rename the part first, click empty space in the view so nothing is selected and type a new **Name** in the right panel.

## Where to go next

- [Draw your first sketch](topic:sketch-basics) and [Fully define a sketch](topic:relations-dimensions) explain the sketcher properly.
- [Change a part through its history](topic:edit-history) shows how to edit what you built.
- [Round, bevel and hollow](topic:fillet-chamfer-shell) and [Make a drawing](topic:drawing-pdf) finish the part. [What is not built yet](topic:not-built-yet) lists what this version does not do, such as sweeps and lofts.
- [Coming from another parametric modeler](topic:coming-from) lists the habits that work differently here.
- [Keys and mouse](topic:keyboard) is the full list of shortcuts.
- [Using this help](common:help).
