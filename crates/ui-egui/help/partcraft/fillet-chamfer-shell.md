---
title: Round, bevel and hollow with Fillet, Chamfer and Shell
summary: Pick edges and add constant or variable-radius fillets and equal, two-distance or distance-angle chamfers, hollow a part with Shell by picking the faces to open, change which edges or faces a feature uses, and fix the usual failures.
section: tasks
order: 85
keywords: fillet, round, blend, chamfer, bevel, shell, hollow, wall thickness, edges, faces, pick edges, shift click, open face, exact kernel, mesh kernel, not built
---

:::note
**In this version** PartCraft builds fillets and chamfers with the mesh kernel, on **straight edges between two flat faces** only: box edges and the straight edges of extrudes and cuts, outside or inside corners (an inside corner gets material added). Round edges such as hole rims, and a radius that changes along the edge, are not built yet. Where two rounded edges meet at a corner the rounds are mitred (they meet in a sharp crease), not blended. **Shell** works too, built by the mesh kernel: it opens flat faces only, shells with different wall thicknesses are not built yet, and it is slow on parts with many round faces. See [What is not built yet](topic:not-built-yet).
:::

{{ui:Fillet}} rounds edges, {{ui:Chamfer}} bevels them and {{ui:Shell}} hollows the part. All three work on the solid's edges and faces and are built by the mesh kernel. Their rounds are faceted like every round face (**Curves** sets how finely); the round faces are cylinders you can pick and measure (see [The mesh kernel](topic:solid-kernels)).

## Fillet or chamfer edges

### Pick the edges first

1. Click an edge of the part. The whole edge highlights, and the right panel says **Edge**: its kind (Straight, Circle or arc, Curve), length and the faces it lies between.
2. To add more edges, {{key:Shift}}+click them. Each one is marked; {{key:Shift}}+click a marked edge to drop it. The panel heading counts them ("4 edges") and the status bar says "4 edges picked. Fillet or Chamfer rounds them; Shift+click picks more, Esc clears."
3. Choose {{ui:Fillet}} or {{ui:Chamfer}} in the toolbar, or the same buttons in the right panel.

The feature is added with all the picked edges: a 2 mm round, or a 1 mm chamfer. The status bar says, for example, "Fillet1 added (4 edges). Change its size on the right; Pick edges changes which."

### Or add the feature first

1. With nothing picked, choose {{ui:Fillet}} or {{ui:Chamfer}}.
2. PartCraft goes into picking: the right panel heading says "Fillet1: picking", the part shows as it was before the fillet (the panel says so), and the toolbar shows {{ui:Done}}.
3. Click each edge to add it; click it again to drop it. The status bar counts them.
4. Choose {{ui:Done}} (or {{ui:Done picking}} in the panel), or press {{key:Esc}} or {{key:Enter}}.

### Settings

| Setting | Fillet | Chamfer |
|---|---|---|
| Size | **Radius (mm)**, default 2 | **Distance (mm)** back from the edge on both faces, default 1 |
| **Edges** | How many | How many |

The list below names each edge by the two faces it joins, such as "Edge 1: Extrude1 end face × Extrude1 side". **Remove** drops one. {{ui:Pick edges}} goes back to picking. Double-clicking the feature in the history does the same.

Edges are remembered by the two faces they join, so they stay on the right edge when you change a dimension earlier in the history.

### Variable fillets and chamfer types

- **Variable radius** (fillet): not built yet in this version ("A fillet whose radius changes along the edge: not built yet (it needs an exact kernel). Give it one radius."). With an exact kernel: tick it and set **End radius (mm)**. The radius changes linearly from **Radius** at one end of each edge to **End radius** at the other; **Swap the ends** turns it round.
- **Type** (chamfer): **Equal distance**, **Two distances** (a **Second distance** on the other face) or **Distance and angle** (the distance on one face, the bevel at **Angle (°)** to that face). **Flip sides** puts the first distance on the other face.

## Shell a part

1. Click the flat face to leave open, for example the top of a box. Or {{key:Shift}}+click several faces.
2. Choose {{ui:Shell}} in the toolbar, or {{ui:Shell, opening this face}} in the face's panel.

The part becomes a 2 mm wall with the picked faces open. Settings:

| Setting | What it does | Default |
|---|---|---|
| **Wall (mm)** | The wall thickness | 2 |
| **Walls grow outward (the faces stay inside)** | Ticked: the walls are added outside, so the inside keeps the part's size | Off: the walls go inward and the outside keeps its size |
| **Open faces** | How many faces are removed | |

Inward, the walls are exactly the wall thickness: outside corners stay sharp, inside corners come out rounded (radius = the wall), and a hole through the part gets a tube of wall round it. Outward, the walls are added outside the original surface, which becomes the hollow, and the outer edges come out rounded. The inner faces are named after the faces they offset, so you can pick them.

Only flat faces can be opened: picking a curved one says "Shell: in this version it opens flat faces only (the mesh kernel builds it)." A shell is slow on parts with many round faces (several seconds per thousand triangles): use a coarser **Curves** setting while you design.

{{ui:Pick faces}} changes which faces are open, the same way as picking edges. The list names each one, such as "Face 1: Extrude1 end face". A shell with no open faces is not allowed: "Pick the faces to remove (open), then shell."

## Order matters

Features build top to bottom. A fillet before a shell is shelled with the part, so the inside gets rounded corners too. A fillet after a shell rounds only the edges you pick. Move features with {{ui:Move up}} and {{ui:Move down}} (see [Change a part through its history](topic:edit-history)).

Patterns and mirrors cannot copy a fillet, chamfer or shell: "Fillet1 changes the body's edges or walls; it cannot be copied. Copy the features that make solids (extrudes, revolves, holes, imports)." Pattern the holes or cuts first, then fillet all the copies' edges in one fillet.

## When it fails

| Message | What to do |
|---|---|
| "A 5 mm round does not fit here (it is too big for a face next to one of the edges, or the edges meet in a corner the kernel cannot blend)." | Use a smaller radius, or fillet fewer edges at a time (several fillet features, smallest last) |
| "An edge it rounds is gone: the edge between Extrude1 and Cut2." | An earlier change removed that edge. Choose {{ui:Pick edges}}, remove the red edge and pick the new one |
| "The shell could not be made with this thickness (…)." | The wall is thicker than a narrow part of the model, or than a fillet's radius. Make the wall thinner, or the fillets larger than the wall |
| "That face has no name the history can follow; pick another." | Pick a face made by a feature (most faces are) |
| "Fillet: in this version only straight edges between two flat faces (the mesh kernel builds them). Round or bevel curved edges in the sketch." (also Chamfer) | A picked edge is round, such as a hole rim, or lies on a curved face. Remove it with {{ui:Pick edges}}, and round or bevel that corner in the sketch instead |
| "A fillet whose radius changes along the edge: not built yet (it needs an exact kernel). Give it one radius." | Untick **Variable radius** |
| "Fillet by the mesh kernel: where rounded edges meet they are mitred, not blended." (⚠ in the history) | A warning, not an error: the part is built. Picked edges meet at a corner, and the rounds meet there in a sharp crease. Fillet fewer edges, or accept the crease |
| "Shell: in this version it opens flat faces only (the mesh kernel builds it)." | An open face is curved. {{ui:Pick faces}} and remove it; open only flat faces |

## Limits

In this version fillets and chamfers work only on straight edges between two flat faces, with one radius, and rounds that meet at a corner are mitred. Shells open flat faces only. With an exact kernel, a variable fillet changes linearly along each edge (one law per feature; no radius set at points along it). Face fillets, full-round fillets and shells with different wall thicknesses are not built yet.
