---
title: Freeform shapes with Form, and surface features
summary: Push and pull a smooth control cage in the Form workspace, crease edges, use symmetry, check and finish it into an exact solid or surface, then patch, loft, stitch, thicken, offset, extend and trim surfaces in Design. What is exact and what is approximated.
section: tasks
order: 92
keywords: form, freeform, t-spline, subdivision, catmull-clark, cage, quadball, crease, symmetry, zebra, curvature, surface, patch, fill, loft surface, stitch, sew, thicken, offset surface, extend, trim
---

:::note
**Not built yet in this version.** Finishing a form into the part, and the surface features, needs an exact geometry kernel, which this version of PartCraft does not have: it builds every part with the mesh kernel. [What is not built yet](topic:not-built-yet) says what to do instead. The rest of this page describes them as they work with an exact kernel.
:::

The **Form** workspace makes organic shapes. You edit a **cage**, a coarse mesh of corners, edges and faces. The shape is the smooth surface the cage controls (its Catmull–Clark limit surface), so moving one corner bends the surface gently around it. When it looks right, **Finish** adds it to the part history as a *Form* feature, and the exact kernel turns it into true B-spline surfaces. You can then fillet it, cut it, join it to other features and export it as STEP.

Choose **Form** in the **Workspace** list at the left of the toolbar.

## Start a cage

1. Set **Size** and **Divisions** in the right panel. Divisions is how many faces each side gets.
2. Click {{ui:Box}}, {{ui:Quadball}} or {{ui:Plane}} in the toolbar. The panel also has **Cylinder**, **Torus** and **Faces by clicks**.

- A **quadball** is a box cage fitted so its corner points lie on a sphere. Between those points the surface sits a little inside the sphere: about 0.4 % less volume with 2 divisions.
- A **plane** is open, so it becomes a surface body (see "Surface features in Design" below).
- **Faces by clicks**: click 4 points on the top (ground) plane or on corners of the cage to make one face. Faces that share corners join up. Press {{key:Enter}} after 3 points for a triangle, or {{key:Esc}} to stop.

Your unfinished cage is saved with the part file, so you can close the file and carry on later.

## Pick and move

- Click a corner, an edge or a face of the cage. {{key:Shift}}+click adds or drops items. {{key:Esc}} clears the selection.
- A gizmo appears at the middle of the selection. Drag an arrow to move along it, or a square to move in that plane. Press {{key:W}} to move, {{key:E}} to rotate and {{key:R}} to scale, or use the buttons in the panel. {{ui:Gizmo}} in the toolbar steps through them.
- **Soft falloff** (mm) also pulls the corners within that distance, fading smoothly to nothing at the edge of that distance.
- Every drag is one undo step: {{key:Ctrl+Z}} and {{key:Ctrl+Y}} work as usual.

## Change the cage

The **Modify** buttons in the panel work on the selection:

| Button | What it does |
|---|---|
| {{ui:Extrude}} | Picked faces move out by **Extrude distance**, along their normals, with new side faces. Picked open edges grow new faces outward. |
| {{ui:Edge loop}} / Insert edge loop | Splits the ring of four-sided faces that crosses the picked edge, halfway along. More loops give tighter curves. |
| Insert point | Adds a corner in the middle of the picked edge. |
| Subdivide | Splits each picked face into four-sided faces around its middle. |
| Bridge | Joins two picked faces, or two openings (pick one open edge on each), with a tube of faces. Both need the same number of edges. |
| Fill hole | Closes the opening through the picked open edge with one face. |
| Weld | Merges the picked corners into one, at their middle. |
| Flatten | Moves the picked corners onto the plane that fits them best. |
| Delete | Removes the picked faces, edges (two faces merge) or corners. |

## Sharp edges, symmetry and display

- {{ui:Crease}} makes the picked edges sharp. **Crease weight** 10 is fully sharp. A smaller weight stays sharp for that many refinement steps and then softens, which gives a tight round instead of a hard fold. **Uncrease** makes them smooth again. **Sharp vertex** pins a corner so the surface passes through it with a point.
- **Symmetry** mirrors the cage across X, Y or Z = 0. It keeps the positive side, mirrors it, and joins the two halves. After that, an edit on either side happens on both. Corners on the mirror plane stay on it.
- {{ui:Display}} switches between **Smooth** (the surface the feature will be) and **Box** (the cage faces themselves).
- **Analysis**: **Zebra stripes** reflect a striped light from the current view. They bend smoothly across a smooth surface and break at a crease. **Mean curvature** shades convex areas red, concave areas blue and flat areas white. It is measured on the displayed mesh with the cotangent formula, so it is a guide, not a measurement.

## Check and finish

- **Check exact** converts the cage in the exact kernel without changing your part. It reports how many B-spline patches the cage makes, how many are exact, and whether the result is a closed, valid solid and its volume.
- {{ui:Finish}} adds a *Form* feature to the history, or updates the form you were editing, and takes you back to Design. A closed cage becomes a solid. Its **Operation** in the Design panel can be New body, Join, Cut or Intersect. An open cage becomes a surface body.
- To change a form later, select it in the history and click **Edit in Form**. The built body is hidden while you edit its cage.
- **Discard** throws the cage away without touching the history.

## What is exact and what is approximated

Each four-sided face of the cage, refined when needed, becomes one bicubic B-spline patch.

- Where every corner of a face is *regular* (four edges meet inside the surface, or three on an open edge or along a crease), the patch is **exactly** the subdivision surface. The tests check this to 1e-9 mm.
- Around an **extraordinary point** (three, five or more edges meeting, a corner of a three- or five-sided face, or the end of a crease) there is no exact polynomial patch. There the patch takes the exact point and tangent plane at that corner and the neighbors' edge curves, so the patches still meet with no gap. Tangent continuity across the edges that meet at the point is approximate. The Form panel and the feature's panel give the largest angle, typically under 2°. A higher **Level** in the feature's panel makes these patches smaller and closer.
- Sharp creases are real edges of the solid, so you can fillet them.
- Each patch is its own face, so a form has many faces and lines are drawn between them.

## Surface features in Design

The surface commands are in Design's {{ui:More}} menu, under the features. They need the exact kernel. Surface bodies are drawn see-through in blue. They are not part of the solid until you thicken or stitch them.

| Command | What it does |
|---|---|
| Patch | Fills a closed loop of edges. {{key:Shift}}+click the edges of the solid first, or select a surface body to fill its opening. **Tangent (G1)** makes the patch meet that surface smoothly. |
| Loft surface | A surface through one chain of curves per sketch (open or closed), in history order. |
| Stitch | Sews surface bodies along their shared edges. If they close up, the result is a solid, with an Operation like other features. |
| Thicken | Gives a surface body a thickness: along its normal, against it, or half each way. The result is a solid. |
| Offset surface | A copy of a surface body moved along its normals. The original stays. |
| Extend surface | Grows a one-face surface past the edges you choose. |
| Trim surface | Cuts a surface body where another surface body, or the solid, crosses it. Click **Pick a point on the piece** to choose which piece to keep, or keep everything else. |

Stitch, Thicken, Extend and Trim use up the surface body they start from. Later features use the new one.

Limits today: forms with several separate closed pieces do not convert into one solid; regular areas are not merged into larger surfaces; and see [What is not built yet](topic:not-built-yet).
