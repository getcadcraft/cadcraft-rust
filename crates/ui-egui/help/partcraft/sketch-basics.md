---
title: Draw your first sketch
summary: Open a sketch on a plane or a flat face, draw lines, rectangles, circles, arcs, points and centerlines, round a corner, select, drag and delete.
section: tasks
order: 10
keywords: sketch, draw, line, rectangle, circle, arc, point, centerline, construction, fillet, sketch fillet, round corner, snap, select, window, drag, profile
---

Every solid starts as a sketch: a flat drawing on a plane. This walkthrough covers the drawing tools. Tying the sketch down with relations and dimensions is in [Fully define a sketch](topic:relations-dimensions).

## Start a sketch

You can sketch on a base plane or on any flat face of the part.

1. Pick where to sketch:
   - a base plane: click **Front plane**, **Top plane** or **Right plane** in the history, or click the plane in the view;
   - a flat face: click the face in the view. The whole face highlights.
2. Choose {{ui:Sketch}} in the toolbar or press {{key:S}}. (With a face picked you can also choose {{ui:Sketch on this face}} in the right panel; with a plane, {{ui:Sketch on it}}.)

The view turns square to the plane, the sketch tools appear in the toolbar, and the Line tool is ready. If you press {{ui:Sketch}} with nothing picked, the base planes show and the status bar says "Click a plane or a flat face to sketch on. Esc stops."

:::note
A curved face cannot hold a sketch: "That face is curved. Pick a flat face or a plane to sketch on."
:::

**A sketch on a face follows that face.** If the face was made by a sketch edge or an extrude's end, the new sketch moves when the feature changes (a deeper pocket takes the sketch on its floor with it). If the panel says the face is "Fixed where picked", the sketch stays where the face is now.

## The drawing tools

Each tool has a toolbar button and a key. Choose the same button again, press {{key:S}}, or press {{key:Esc}} to go back to Select.

| Tool | Key | How to draw |
|---|---|---|
| {{ui:Line}} | {{key:L}} | Click the start, then click each next point. Lines chain end to end. End the chain with {{key:Esc}}, {{key:Enter}}, a right-click or a double-click. Clicking an existing point also ends it |
| {{ui:Rectangle}} | {{key:R}} | Click one corner, then the opposite corner. You get four lines joined at the corners, two horizontal and two vertical |
| {{ui:Circle}} | {{key:C}} | Click the center, then a point on the circle |
| {{ui:Arc}} | {{key:A}} | Click the center, then the start, then the end. The arc runs the way you move the pointer round the center |
| {{ui:Point}} | {{key:P}} | Click. A point on its own marks a hole center for {{ui:Hole}} |
| {{ui:Centerline}} | {{key:N}} | Like a line, one segment at a time. Drawn dashed; it is construction geometry, used as a revolve axis or a reference |
| {{ui:Fillet}} | {{key:F}} | Click a corner where two lines meet, type the radius, press {{key:Enter}} |
| {{ui:Spline}} | {{key:B}} | Click the points the curve passes through; {{key:Enter}} ends, a click on the first point closes it. See [Splines, trim, offset and other sketch tools](topic:sketch-tools) |
| {{ui:Trim}} | {{key:T}} | Click the piece of a curve to remove, up to the nearest crossings |
| {{ui:Dimension}} | {{key:D}} | See [Fully define a sketch](topic:relations-dimensions) |

While you draw, a small label next to the pointer shows the length, the size or the diameter.

{{ui:More}}, at the end of the sketch tools, holds the control-point spline, Extend, Offset, Mirror, Polygon, Slot, the sketch patterns and Project edge: see [Splines, trim, offset and other sketch tools](topic:sketch-tools).

### Snapping

Snapping adds relations for you, so the sketch stays joined when it moves:

- On an **end point or the origin**, a small square shows. The new point is made coincident with it.
- On a **line, circle or arc**, a small cross shows. The new point is put on that curve.
- A line within 3 degrees of horizontal or vertical snaps to it and gets a horizontal or vertical relation. The label shows "H" or "V".

To avoid a snap, zoom in so you click farther from the point or curve.

## Round a corner

1. Press {{key:F}} or choose {{ui:Fillet}}.
2. Click the corner point where two lines meet.
3. A box opens with a suggested radius. Type the radius and press {{key:Enter}}.

Both lines are trimmed and joined by a tangent arc with a radius dimension. Messages you may see: "A fillet needs a corner where exactly two lines meet." and "That radius is too big for these lines."

:::tip
A sketch fillet rounds a corner of the profile, so the extrude gets rounded vertical edges. To round edges of the solid itself (for example the top edges of a plate), use the {{ui:Fillet}} feature on the part instead: see [Round, bevel and hollow](topic:fillet-chamfer-shell). In a sketch, the toolbar's Fillet is the sketch tool; on the part, it is the feature.
:::

## Construction geometry

Construction lines, circles and arcs are dashed and gray. They guide the sketch but are not part of the profile that gets extruded or revolved.

1. Select the lines, circles or arcs.
2. Press {{key:Q}} or choose {{ui:Construction}}. Press it again to switch them back.

## Select, drag and delete

Press {{key:S}} (or {{key:Esc}} until no tool is active) for Select.

- Click a point, line, circle, arc, relation symbol or dimension to select it. {{key:Shift}}+click adds to the selection or takes away. On Windows and Linux {{key:Ctrl}}+click does the same; on a Mac, ⌘+click.
- Drag on empty space to draw a window: everything fully inside it is selected.
- Drag a point or a line to move it. The solver keeps every relation and dimension as it moves, so only free geometry moves. Drag a circle's edge to change its radius.
- Drag a dimension's number to move the label.
- Press {{key:Delete}} or {{key:Backspace}}, or choose {{ui:Delete}} in the right panel, to delete the selection. Deleting geometry deletes the relations on it.

## Zoom and pan in the sketch

- Roll the wheel to zoom at the pointer.
- Drag with the middle or right button to pan.
- {{ui:Fit}} in the toolbar or the right panel fits the sketch in the view. In a sketch the {{key:F}} key is the Fillet tool, not Fit.

## Close the sketch

Choose {{ui:Exit}} in the toolbar or {{ui:Exit sketch}} in the right panel, or press {{key:Esc}} until the sketch closes. Each {{key:Esc}} first cancels what is in progress, then returns to Select, then clears the selection, and the next one closes the sketch.

The sketch stays selected, so the next {{ui:Extrude}} or {{ui:Revolve}} uses it.

## What makes a good profile

{{ui:Extrude}} and {{ui:Revolve}} use **every closed loop** of the sketch's solid (non-construction) lines, circles, arcs and splines. Loops inside loops become holes.

- Every end must meet exactly one other end. "The profile is open: an end at (12.5, 40) is not joined to anything." names the place.
- No three ends may meet at one point: "The profile branches at (0, 20): more than two ends meet there."
- Points and construction geometry are ignored by the profile.

Next: [Fully define a sketch with relations and dimensions](topic:relations-dimensions).
