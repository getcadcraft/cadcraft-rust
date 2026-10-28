---
title: Combine, move and scale bodies; path patterns
summary: Make separate bodies, then join, cut or intersect them after the fact, move, turn, align or copy them, scale them evenly or by X, Y and Z, and copy features or bodies along a curve.
section: tasks
order: 72
keywords: body, bodies, multi-body, new body, combine, boolean, join, cut, intersect, subtract, union, keep tools, move body, copy body, rotate body, align, translate, scale, non-uniform scale, stretch, affine, path pattern, pattern along curve, array along path, curve driven pattern
---

:::note
**Not built yet in this version.** Working with several bodies needs an exact geometry kernel, which this version of PartCraft does not have: it builds every part with the mesh kernel. [What is not built yet](topic:not-built-yet) says what to do instead. The rest of this page describes it as it works with an exact kernel.
:::

A part can hold more than one solid. Each separate solid is a **body**. Most features add to or cut from every body they touch; the tools on this page work on whole bodies you choose. They are in the Features group's {{ui:More}} menu: {{ui:Combine}}, {{ui:Move body}}, {{ui:Scale body}} and {{ui:Path pattern}}. All of them need the exact kernel (OpenCascade).

## Make a separate body

Set an extrude's or a revolve's **Operation** to **New body** in its right panel (sweeps and lofts have it too). The new solid stays separate even where it overlaps the others. **Intersect** keeps only the overlap of the new shape with what is there.

Bodies are named after the feature that started them: "Extrude2". A copy is named after its original and the feature that copied it: "Extrude2, copy 1 of Move1". The names stay the same when you change sizes upstream, so the features that use a body keep finding it. With two or more bodies, the part panel (click empty space) lists them with their volumes.

## Rename or hide a body

Click empty space in the view to see the part panel. Its body list (shown when the part has two or more bodies) has a name box and a {{ui:Hide}} button on each row.

- **Rename:** type a new name in the box and press {{key:Enter}} or click away. Every panel then uses it: Combine, Move body, the patterns. Clear the box to go back to the name from the feature that made the body.
- **Hide:** click {{ui:Hide}} on its row. The body leaves the view and clicks go through it, so you can pick faces behind it. {{ui:Show}} brings it back; {{ui:Show all}} brings back every hidden body.

Hiding only changes the view. A hidden body is still built, counted in the volume and the measurements, and saved in STL, 3MF and STEP exports. Names and hidden bodies are saved in the part file, and each change is one step of {{ui:Undo}}.

## Combine bodies

1. Shift+click a face of the **target** body, then a face of each **tool** body.
2. Choose {{ui:More}}, then {{ui:Combine}}.
3. In the right panel choose the **Operation**:

| Operation | Result |
|---|---|
| Join | The target and the tools become one body |
| Cut | The tools are taken away from the target |
| Intersect | Only what the target and every tool share is kept |

**Target body** is the one that keeps its name. Tick or untick **Tool bodies**, or use **Pick in the view** and click their faces. **Keep the tool bodies** leaves the tools in place as well (handy for a mold and its core). If a tool body was already used up by an earlier Combine, the panel says so instead of picking another body.

## Move or copy a body

1. Choose {{ui:More}}, then {{ui:Move body}}. The newest body is ticked; tick the ones to move.
2. Choose the **Move** type:

| Move | What you set |
|---|---|
| Translate | **ΔX**, **ΔY**, **ΔZ** in millimeters |
| Point to point | **From** and **To**: click near the end of an edge, a point feature, or a spot on a face |
| Rotate about an axis | **Axis** (X, Y, Z, an axis feature, or **Pick**: a straight edge, a circle or a cylinder) and **Angle (°)** |
| Align a face to a face or plane | **Face to move** (a flat face of the body), **Onto** (a flat face or a plane) |

Align turns the body so its face lies on the other face, face to face, then slides it along that face's normal only. **Same way** turns it the other way round. **Copy** keeps the original and adds the moved copy as a new body.

## Scale a body

1. Choose {{ui:More}}, then {{ui:Scale body}}, and tick the bodies.
2. Type a **Scale**, or untick **Uniform** and type **Scale X**, **Scale Y** and **Scale Z** (along the world axes).
3. **About** is the point that stays put: the centre of the bodies' box, or a point you pick.

A uniform scale is exact. A non-uniform scale turns circles into ellipses: a cylinder becomes an elliptic cylinder, which no analytic surface holds, so curved faces become B-spline surfaces that stay within 0.0000004 mm of the exact shape; flat faces and straight edges stay exact. The panel says which happened. Scale factors must be more than zero (mirror with {{ui:Mirror}} instead).

## Copy along a path

1. Select the feature to copy in the history, or leave nothing selected to copy the newest body.
2. Choose {{ui:More}}, then {{ui:Path pattern}}. The newest unused sketch is taken as the path.
3. In the right panel set:

| Setting | What it does |
|---|---|
| **Copy** | Bodies (each copy a new body) or Features (joined or cut like the original) |
| **Path** | Tick sketches, or **Pick model edges**; the pieces must join end to end |
| **Count** | Total number, including the original |
| **Spacing** | Equal (spread to the end of an open path, or all round a closed one) or a set distance |
| **Orientation** | Follow the path (each copy turns with it) or Fixed (copies stay parallel) |
| **Reverse direction** | Go the other way along the path |

The copies start where the path passes nearest the original and are spaced by length along the curve.

Linear and circular patterns can copy bodies too: tick **Copy bodies** in their panel and choose the bodies; each copy is a new body.

## Limits

- Bodies need the exact kernel; the mesh kernel keeps one solid and says so.
- A non-uniform scale makes curved faces into close B-spline approximations (see above), not exact cylinders or cones; offset surfaces cannot be scaled that way yet.
- Following the path on a twisting 3D curve carries the copy without twisting it; on flat paths the turn is exact.
- Combine works on solids only. Surface bodies are joined with {{ui:Stitch}}.

See also [Reference planes, axes and points](topic:reference-geometry) for points and planes "between objects", and [Pattern and mirror features](topic:patterns-mirror).
