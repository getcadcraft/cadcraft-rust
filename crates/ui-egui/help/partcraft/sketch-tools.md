---
title: Splines, trim, offset and other sketch tools
summary: Draw fit-point and control-point splines, trim and extend curves, offset a chain, mirror about a centerline, add polygons and slots, pattern sketch geometry and project model edges.
section: tasks
order: 15
keywords: spline, fit point, control point, b-spline, nurbs, curve, trim, extend, offset, mirror, symmetric, polygon, hexagon, slot, pattern, linear pattern, circular pattern, project, project edge, include, convert entities
---

These tools work in an open sketch. {{ui:Spline}} and {{ui:Trim}} are in the toolbar. The others are under {{ui:More}} at the end of the sketch tools. Each tool is one undo step, and {{key:Esc}} stops it. For the basic drawing tools see [Draw your first sketch](topic:sketch-basics).

## Splines

A spline is a smooth curve. There are two kinds.

| Tool | Key | How to draw |
|---|---|---|
| {{ui:Spline}} | {{key:B}} | Click the points the curve passes through. End with {{key:Enter}}, {{key:Esc}}, a right-click or a double-click. Click the first point again to close the spline |
| {{ui:Control-point spline}} (under {{ui:More}}) | | Click the control points. The curve is pulled toward them but only touches the first and last. The dashed control polygon shows while you draw and afterwards |

- **The points are sketch points.** Drag them, or give them relations and dimensions like any other point. A spline's first point can snap onto a line's end, which joins them with a coincident relation.
- **Tangent at an end.** Select a line, arc or spline that meets one end of a spline, and the spline, then choose {{ui:T Tangent}}. The spline's end direction then follows the line. A tangent away from the spline's ends is refused: "A tangent to a spline needs the line, arc or spline to meet one end of it."
- **Ends.** Without a tangent relation, a fit-point spline's ends are natural (no curvature at the very end). The curve is cubic and passes through every point, with its spacing set by the distances between the points.
- **Closed splines** make a profile on their own. Open splines join lines and arcs into a closed loop like any other curve.
- Deleting a point inside a spline removes that point and keeps the spline, while it still has at least two points (three when closed).

Extrude and revolve build the spline with the mesh kernel from short straight pieces (true curves need an exact kernel: not built yet in this version). DXF export writes a spline as a polyline that stays within 0.01 mm of the curve, because DXF R12 has no spline entity.

## Trim and extend

| Tool | Key | How |
|---|---|---|
| {{ui:Trim}} | {{key:T}} | Point at a curve: the piece that would go shows red, up to the nearest crossings. Click to remove it. A curve that crosses nothing is removed whole |
| {{ui:Extend}} (under {{ui:More}}) | {{key:E}} | Click a line or arc near the end to extend. It grows to the next curve it meets. "There is nothing for that end to reach." when nothing is in the way |

Trim and Extend work on lines, arcs, circles and splines (Extend: lines and arcs).

- A trimmed **circle** becomes an arc. Its radius or diameter dimension moves to the arc.
- Trimming the middle of a line leaves two lines that stay on the same straight line. Trimming the middle of an arc leaves two arcs with the same center and an equal relation.
- A trimmed **spline** becomes a control-point spline that follows the old curve exactly. A tangent relation at an end that is kept stays.
- The new end is put on the curve that cut it (on the line, arc or circle, or coincident with its end). Splines cannot hold a point, so a spline that cuts adds no relation.

**Relations that no longer fit are removed**, and the status bar says how many. For example, a length dimension on a trimmed line is removed, but horizontal or parallel relations stay.

## Offset

1. Choose {{ui:Offset}} under {{ui:More}} or press {{key:O}}.
2. Click a line, arc, circle or spline. Its whole chain highlights: all the curves joined end to end, up to a point where three or more meet. To offset only part of a chain, select those curves first, then choose Offset and click one of them.
3. Move the pointer to the side where the copy goes. A dashed preview follows. Press {{key:B}} for both sides.
4. Click, type the distance, and press {{key:Enter}}.

Lines and arcs are offset exactly. Corners are extended or trimmed until they meet. The copy has one **Offset** dimension, and the other pieces follow it: double-click the dimension to change the whole copy. A distance too big for the chain is refused, for example "That distance is too big for this chain: a side would turn inside out."

A **spline** is offset by a new fit-point spline within 0.001 mm of the true offset. This copy is fixed: it does not follow later changes, and it has no Offset dimension. A spline in a chain needs smooth (tangent) joins at its ends.

## Mirror

1. With Select, pick what to mirror.
2. Choose {{ui:Mirror}} under {{ui:More}} or press {{key:M}}.
3. Click the line or centerline to mirror about.

Each mirrored point gets a **Symmetric** relation with its original, so the copy follows when the original changes. Points that lie on the mirror line are shared, and mirrored circles get an Equal relation. If nothing was selected, click the items to mirror, press {{key:Enter}}, then click the mirror line.

## Polygons and slots

| Tool | How |
|---|---|
| {{ui:Polygon}} | Click the center, then a corner, then type the number of sides (3 to 64). The sides are equal and the corners sit on a construction circle. Dimension the circle to size the polygon |
| {{ui:Slot}} | Click one end's center, then the other end's center, then click to set the width. Two lines, tangent to two equal half circles |

## Sketch patterns

Select the geometry first, then:

- {{ui:Linear pattern}}: click a start point, then where the first copy goes, then type the number of copies (including the original).
- {{ui:Circular pattern}}: click the center, then type the number of copies, spread evenly round a full turn.

The copies bring the relations among the selected geometry with them (a circular pattern leaves out horizontal and vertical ones). The copies are not linked to the original: change one, and the others stay as they are.

## Project edges of the part

1. Choose {{ui:Project edge}} under {{ui:More}}.
2. Click a straight or circular edge of the part behind the sketch.

The edge is copied onto the sketch plane exactly and fixed (Fix relation). It does not follow later changes to the part. Press {{key:Q}} to make it construction geometry. A circle tilted to the sketch plane would project to an ellipse, so it is refused. So are free-form edges and edges that point straight out of the sketch. Projecting needs an exact kernel: it is not built yet in this version.
