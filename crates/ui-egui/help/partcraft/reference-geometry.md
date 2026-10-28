---
title: Reference planes, axes and points
summary: Make planes offset from a face, at an angle, through three points, midway, tangent to a cylinder, square to an edge or through an edge and a point; axes; points at edge middles, crossings and plane corners; sketch on them and use them as revolve axes, pattern directions and mirror planes.
section: tasks
order: 65
keywords: datum, between objects, midpoint, intersection point, plane through edge and point, reference plane, work plane, construction plane, datum plane, offset plane, plane at angle, midplane, tangent plane, reference axis, construction axis, datum axis, reference point, work point, sketch on plane
---

Reference geometry is the scaffolding you build features on: a **plane** to sketch on where no face is, an **axis** to revolve or pattern about, a **point** to stop an extrude at. It has no material of its own. Planes show in the view as pale squares, axes as dashed lines, points as small crosses; all three join the history and follow what they were made from when you change a size upstream.

They are in the Features group's {{ui:More}} menu: {{ui:Plane}}, {{ui:Axis}}, {{ui:Point}}.

## Make a plane

1. Click a flat face, or a base plane in the history, to start from it (optional).
2. Choose {{ui:More}}, then {{ui:Plane}}. It starts as an offset plane 20 % of the part's size away from what you picked (or from the top plane).
3. In the right panel choose the **Type** and its references:

| Type | References | Numbers |
|---|---|---|
| Offset from a plane or face | **From plane** | **Distance (mm)**, negative for the other side |
| At an angle, through an edge or axis | **From plane**, **Turn about** (a world axis, an axis feature, or a picked straight edge) | **Angle (°)** |
| Through three points | **Point 1**, **Point 2**, **Point 3** | — |
| Midplane between two | **First plane**, **Second plane** (parallel: half way; meeting: the plane that halves the angle) | — |
| Tangent to a cylinder | **Cylinder** (a cylindrical face), **Near point** (which side) | — |
| Square to an edge, at a point | **Edge**, **At point** (the plane goes through the edge's point nearest it, and through the point too) | — |
| Through an edge and a point | **Straight edge** (or an axis), **Through point** | — |
| Square to an edge at its middle | **Edge** (half way along it, square to it there), or with no edge **Point 1** and **Point 2** (the plane half way between them) | — |

Every reference has a **Pick** button: choose it, then click in the view. A plane slot takes a base plane, a reference plane or a flat face; a point slot takes the end of an edge (click near that end), a point feature, or a spot on a face; an edge slot takes an edge. {{key:Esc}} cancels a pick. **Flip the normal** turns the plane over (which way a sketch on it faces).

## Sketch on a plane

Select the plane (in the history or by clicking its square in the view) and choose {{ui:Sketch}}, or **Sketch on it** in its panel. The sketch follows the plane: change the plane's distance and the sketch, and every feature on it, moves too.

## Axes and points

- {{ui:Axis}}: through two points, the axis of a cylinder or cone, where two planes meet, along a straight edge (or the axis of a circular edge), or square to a face through a point.
- {{ui:Point}}: at coordinates, at a vertex or sketch point, or at the centre of a circular edge, plus the points "between objects":

| Point type | References |
|---|---|
| Middle of an edge (or between two points) | **Edge** (half way along its length, also for arcs and curves), or with no edge **Point 1** and **Point 2** |
| Where an edge or axis meets a face or plane | **Edge** (straight or curved) or an axis, and **Meets plane** (a plane or flat face) or **Or a face** (a flat face or a cylinder). A circle crosses a plane twice and a line goes through a cylinder twice: **The other crossing** takes the second |
| Where three planes meet | **First**, **Second** and **Third plane** (planes or flat faces) |

Each one fails with a plain message when its references do not fit: a line parallel to the plane, a line that misses the cylinder, an edge that does not reach the plane, three planes that do not meet in one point, a point on the line it should fix a plane with.

Use them in other features:

| Where | How |
|---|---|
| Revolve | **Axis**: pick the axis feature from the list (it must lie in the sketch's plane) |
| Circular pattern | **Axis**: the axis feature |
| Linear pattern | **Direction**: an axis feature, or a plane (its normal) |
| Mirror | **Mirror plane**: a reference plane |
| Extrude | **End**: Up to face or plane, Up to vertex (a point feature). Not built yet in this version (it needs an exact kernel) |
| Draft | **Neutral plane** |

## When a reference is gone

A plane made from a face that no longer exists fails with a message naming it, such as "The face it sits on is gone from Extrude1." It is never moved to another face. The sketches and features on it are marked "not built" until you pick a new reference; the rest of the part stays as it was.

:::note
This version builds every part with the mesh kernel, which makes planes, axes and points from base planes, sketch points, coordinates and other references only. References on faces and edges of the solid need an exact kernel: not built yet in this version ("It uses a face or edge of the solid, which only an exact kernel can find: not built yet in this version.").
:::
