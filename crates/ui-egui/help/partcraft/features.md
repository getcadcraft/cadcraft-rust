---
title: Features and their options
summary: Every feature in the toolbar with its settings, defaults and limits, from extrude to fillet, shell and imported STEP solids, the actions every feature shares, and the part settings.
section: reference
order: 30
keywords: features, not built, extrude, cut, revolve, revolved cut, hole, counterbore, linear pattern, circular pattern, mirror, fillet, chamfer, shell, import, options, defaults, settings, resolution, curves, facets, suppress, rollback
---

Features are listed in the history in the order they were made and rebuilt top to bottom. Each one is named after its kind with a number: Sketch1, Extrude1, Revolve1, Hole1, LPattern1, CirPattern1, Mirror1, Fillet1, Chamfer1, Shell1, Imported1.

PartCraft builds every feature with the mesh kernel. Fillet and Chamfer work on straight edges between two flat faces, and Shell opens flat faces. Imported solids need an exact kernel, which this version does not have: they are not built yet, and their section below describes them as a part made in an earlier build keeps them. See [The mesh kernel](topic:solid-kernels) and [What is not built yet](topic:not-built-yet).

## Sketch

| Setting | Notes |
|---|---|
| Plane | Front, Top or Right plane, or a flat face. Shown, not changed: to move a sketch, make a new one |
| State | Fully defined, Under defined (with the degrees of freedom), Over defined, or Empty |
| Geometry, Relations | How many curves, points and relations |
| Driving dimensions | Every dimension of the sketch, editable |

Buttons: {{ui:Edit sketch}}, {{ui:Export as DXF}}.

## Extrude and Cut

{{ui:Extrude}} and {{ui:Cut}} make the same feature, starting with different settings.

| Setting | Choices | Extrude starts at | Cut starts at |
|---|---|---|---|
| Operation | Boss (add material), Cut (remove material), Intersect (keep the overlap), New body (not built yet in this version) | Boss | Cut |
| End | Blind (a depth), Mid-plane (both ways), Through all | Blind | Through all |
| Depth (mm) | Above 0, up to 1,000,000 | 10 | 10 |
| Reverse direction | Off / on | Off | Off |

- Profile: every closed loop of solid lines, circles and arcs in the sketch. Loops inside loops are holes.
- Through all works only for cuts. It reaches right through the part on the chosen side.
- A cut goes into the material, away from the sketch plane's outward side; Reverse flips it.

## Revolve and revolved cut

| Setting | Choices | Default |
|---|---|---|
| Operation | Boss, Cut | Boss ({{ui:Revolve}}), Cut ({{ui:More}} › {{ui:Revolved cut}}) |
| Angle (°) | Above 0, up to 360 | 360 |
| Axis | First centerline, any line or centerline of the sketch, or an axis feature lying in the sketch's plane | First centerline |

- The profile must stay on one side of the axis. It may touch it.
- A revolve's axis can be used by Circular patterns and by assembly joints.

## Hole

| Setting | Choices | Default |
|---|---|---|
| Type | Simple, Counterbore | Simple |
| Diameter (mm) | Above 0 | 6 |
| Through all | On / off | On |
| Depth (mm) | When Through all is off | 10 |
| Counterbore ⌀ | Wider than the hole | 11 |
| Counterbore depth | Shallower than a blind hole | 6 |
| Reverse direction | Off / on | Off |

- One hole at every loose point of the sketch (points made with the Point tool). The panel shows how many.
- Holes drill square to the sketch plane, into the material.
- Each hole's axis can be used by assembly joints ("Hole1, hole 2").

## Linear pattern

| Setting | Choices | Default |
|---|---|---|
| Features to copy | Any solid features above it | The selected feature, else the last solid feature |
| Direction | X axis, Y axis, Z axis; a line of an earlier sketch; the axis of a revolve; a picked edge | X axis (or the edge picked when you added it) |
| Spacing (mm) | Any number | 20 |
| Count | 1 to 1000, including the original | 3 |
| Reverse direction | Off / on | Off |

## Circular pattern

| Setting | Choices | Default |
|---|---|---|
| Features to copy | As for Linear | As for Linear |
| Axis | As for Direction above | The last revolve's axis, else the Y axis (or the edge picked) |
| Angle (°) | Above 0, up to 360. 360 spaces the copies equally all round; less spreads them over the angle | 360 |
| Count | 1 to 1000, including the original | 6 |
| Reverse direction | Off / on | Off |

## Mirror

| Setting | Choices | Default |
|---|---|---|
| Mirror | The whole body (as built so far), or chosen features | The selected feature, else the whole body |
| Mirror plane | Front, Top or Right plane; a flat face | Right plane, or the flat face picked when you added it |

## Fillet and Chamfer

In this version the mesh kernel builds them on straight edges between two flat faces (outside or inside corners), with one radius. A round edge says "Fillet: in this version only straight edges between two flat faces (the mesh kernel builds them). Round or bevel curved edges in the sketch."; where rounded edges meet at a corner they are mitred, and the feature warns "Fillet by the mesh kernel: where rounded edges meet they are mitred, not blended." Chamfer types (equal distance, two distances, distance and angle, with **Flip sides**) all work. See [Round, bevel and hollow](topic:fillet-chamfer-shell).

| Setting | Choices | Default |
|---|---|---|
| Radius (mm) (Fillet) | Above 0 | 2 |
| Distance (mm) (Chamfer) | Above 0, the same on both faces | 1 |
| Edges | The picked edges, each listed by the two faces it joins ("Extrude1 end face × Extrude1 side"), with **Remove** | The edges picked when you added it |

- Pick edges before choosing the button ({{key:Shift}}+click for several), or choose it with nothing picked and click edges afterwards.
- {{ui:Pick edges}} (or double-click the feature) changes the edges.
- Edges are kept by the two faces they join, so they survive size changes upstream.

## Shell

The mesh kernel builds it. Only flat faces can be opened ("Shell: in this version it opens flat faces only (the mesh kernel builds it)."); walls of different thicknesses are not built yet. See [Round, bevel and hollow](topic:fillet-chamfer-shell).

| Setting | Choices | Default |
|---|---|---|
| Wall (mm) | Above 0 | 2 |
| Walls grow outward (the faces stay inside) | Off: walls go inward. On: walls go outward | Off |
| Open faces | The faces removed, at least one, with **Remove** | The faces picked when you added it |

{{ui:Pick faces}} changes the open faces.

## Imported solid

Made by opening a STEP file in an earlier build: opening STEP files is not built yet in this version, and an imported solid in an older part stays in the history marked as not built. It shows the file name and the size of the STEP text kept in the part file.

| Setting | Choices | Default |
|---|---|---|
| Up in the file | Z up (most CAD), Y up | Z up |

Patterns and mirrors can copy an imported solid, but not a fillet, chamfer or shell.

## What every feature has

At the bottom of each feature's panel:

| Button | Does |
|---|---|
| {{ui:Suppress}} / {{ui:Unsuppress}} | Switch it off or on. Features that need it are not built while it is off |
| {{ui:Roll back to here}} | Build only the features above this one. New features go in at the rollback bar |
| {{ui:Move up}} / {{ui:Move down}} | Reorder, when nothing it uses would end up below it. Grayed with the reason otherwise |
| {{ui:Delete}} | Delete it, and (after asking) every feature that uses it |
| Name | Rename it (up to 60 characters) |

Features with a sketch also list **Sketch dimensions**, editable in place.

## States in the history

| State | Shown as | Meaning |
|---|---|---|
| Built | Normal | All well |
| Warning | ⚠ | Built, with something to check: "Over defined: 1 relation is not needed." or "Nothing is left of the part after this." |
| Error | ⚠ error, red | Could not be built; the message says why |
| Not built | ⚠ not built | Needs a feature that failed, is suppressed or rolled back, or needs an exact kernel, which this version does not have ("Sweep: not built yet. …") |
| Suppressed | off | Switched off |
| Rolled back | grayed, below the rollback bar | Not built until you roll forward |

## Part settings

Click empty space so nothing is selected. The right panel shows **Part**:

- **Name**: the part's name, used for file names and the drawing's title block (up to 120 characters).
- **Curves**: see below.

### Curves resolution

The mesh kernel builds the solid itself from flat facets, so the view, STL, 3MF, the faceted STEP, the volume and the drawing views all follow **Curves**: how many facets per full turn.

| Setting | Facets per turn | Volume of a round shape |
|---|---|---|
| Coarse (32 per turn) | 32 | About 0.64 % under the true value |
| Normal (64 per turn) | 64 | About 0.16 % under (the default) |
| Fine (128 per turn) | 128 | About 0.04 % under |
| Very fine (256 per turn) | 256 | About 0.01 % under |

Finer curves give smoother exports and views, and slower rebuilds. The setting is saved in the part file.

## Not built yet

Variable-radius fillets, fillets and chamfers on round edges, shells opening curved faces, draft on curved faces, sweep, loft, rib, imported STEP solids, new bodies, tapers, "up to" a face or vertex, and references on the solid's faces and edges need an exact kernel, which this version does not have. Also not built: tapped and countersunk holes. See [What is not built yet](topic:not-built-yet).
