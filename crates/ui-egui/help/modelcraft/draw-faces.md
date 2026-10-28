---
title: Draw faces with lines, rectangles, circles and arcs
summary: Make flat faces on the ground or on other faces, split them with lines, and draw circles and polygons with the number of sides you want.
section: tasks
order: 20
keywords: draw, line, rectangle, circle, polygon, offset, inset, frame, border, arc, curve, spline, smooth, cable, path, bulge, radius, curve, face, floor plan, footprint, split, sides, segments, outline, pencil
---

Everything in ModelCraft starts as a flat **face**: a surface closed by edges. You can draw a face directly with {{ui:Rectangle}}, {{ui:Circle}} or {{ui:Polygon}}, or outline one edge at a time with {{ui:Line}}. Faces are what [Push/pull](topic:push-pull) turns into solids.

## Draw a rectangle

1. Press {{key:R}}.
2. Click the first corner: on the ground, or on any face of the model.
3. Move the pointer. The rectangle stays on the ground (or on the face you started on), and the value box ({{ui:Dimensions}}) shows its width and depth. The tip says "Square" when the sides are equal.
4. Click the opposite corner, or type the two sizes separated by a comma and press {{key:Enter}}: `4m,3m`, `4000,3000` in a millimeter model, or `12',10'6"` in feet and inches.

   A face appears with four edges around it.

The typed sizes go in the direction you were pointing: if the pointer is to the left of the first corner when you type, the rectangle grows to the left.

:::warning
The comma separates the two sizes, so it cannot be a decimal mark. Type `2.5m,3m`, never `2,5m`.
:::

## Draw a circle or a polygon

1. Press {{key:C}} for a circle or {{key:N}} for a polygon.
2. To change the number of sides, type it before you click: `12s` (or just `12`) and {{key:Enter}}. A circle starts with 24 sides and a polygon with 6. The tip shows the current number, and ModelCraft keeps it for the next circle until you change it or switch tools.
3. Click the center, on the ground or on a face.
4. Click to set the radius, or type it ({{ui:Radius}} in the value box) and press {{key:Enter}}.

A circle is a polygon with many sides. When you pull a circle up with Push/pull, its sides are softened so the cylinder reads as one smooth surface.

## Draw an arc

1. Press {{key:A}} ({{ui:Arc}}).
2. Click where the arc starts, then where it ends (or type the length between them).
3. Move the pointer to bulge the arc out, and click. The tip says "Half circle" when the bulge is half the distance between the ends. Or type how far the middle stands off: `2` and {{key:Enter}}.

To draw an arc around a point instead, use {{ui:Arc center}}: click the center, then where the arc starts (or type the radius), then where it ends (or type the angle in degrees, such as `90`). It snaps to 15° steps.

An arc started on a face is drawn on that face; otherwise it lies on the ground, or stands upright when its two ends are at different heights. An arc has 12 segments; type `24s` while drawing for more. Arcs close faces just as lines do: an arc across the corner of a rectangle splits it.

## Draw a smooth curve

{{ui:Curve}} draws a smooth line that passes through every point you click: a cable, a handrail's path, a garden edge. It can leave the ground: points snap to corners, edges and faces anywhere, and the arrow keys lock red, green or blue as with {{ui:Line}}.

1. Click {{ui:Curve}} in the {{ui:Draw}} group.
2. Click the points the curve goes through. Type `<0,0,1>` and {{key:Enter}} for a point 1 above the last one, `[x,y,z]` for an exact place, or a length for a point that far toward the pointer.
3. Press {{key:Enter}} or double-click to finish. Click the first point again to close the curve. {{key:Backspace}} takes the last point back.

A curve selects as one piece. To change it, select it and click {{ui:Edit points}} in {{ui:Entity info}}: its points show as squares. Click one, then click where it goes (or type `<dx,dy,dz>`). {{key:Esc}} puts a point back, and ends editing. A closed flat curve makes a face; a curve that outlines a face can no longer be edited.

## Offset: a border inside or outside a face

{{ui:Offset}} copies a face's outline a set distance inside it (or outside it), for a frame, a border, a sill or the glass of a screen.

1. Press {{key:F}} and click the face, or select the face first and then press {{key:F}}.
2. Move the pointer into the face for an inner outline, or out of it for an outer one. The new outline follows, and the tip says how far.
3. Click, or type the distance and press {{key:Enter}}.

The face splits in two: the inner face and a frame around it (or a new ring outside). Push/pull either one straight away: for a screen, draw the box, push it to its thickness, offset the front face 30 mm inward and push the inner face in a little for the glass. Holes in the face are offset too, and an arc or a circle stays one (smaller or larger) curve.

- **Double-click** a face with {{ui:Offset}} to offset it by the last distance again.
- **Edges:** select a chain of connected edges in one plane (for example the two lines of an L), press {{key:F}}, move to the side you want and type the distance: a parallel copy is drawn, with mitred corners.
- When the distance is more than the face allows, ModelCraft says so and draws nothing.

## Arcs, circles and polygons are curves

Click any segment of an arc, a circle or a polygon and the whole curve is selected. {{ui:Entity info}} then shows its radius, its segments, its angle (arcs) and its length. Type a new radius there to resize it about its center; faces drawn in its plane follow. Once a curve's face has been pushed or pulled, its radius stays as it is.

## Draw with lines

1. Press {{key:L}}.
2. Click where the line starts.
3. Click where it ends, or type a length and press {{key:Enter}}. The typed length goes in the direction the pointer shows.
4. Keep clicking. Each click starts the next line from the end of the last one.
5. Close the outline by clicking the first point again. When the lines close a flat loop, it fills with a face and the tool stops chaining. Press {{key:Esc}} to stop at any time.

Lines follow the red, green and blue directions when you get close to them; the line turns that color and the tip says "On red axis". See [Inference and snapping](topic:inference) for how to lock a direction and snap to points.

### Where a line goes when it touches nothing

- A line that starts on a face stays in that face's plane.
- A line that starts on the ground, or in empty space, stays level with its start point. If you look at the model edge-on, it goes in the vertical plane facing you instead.
- To draw a line to an exact place, type a point: `[x,y,z]` for a point in the model, or `<dx,dy,dz>` for an offset from the start. See [Type exact sizes](topic:value-box).

## Split a face

Draw a line across a face, from one edge to another (or from edge to edge through other lines). The face splits into two faces. Each keeps its side and color. The same works with a rectangle or circle drawn on a face: the shape becomes its own face inside the bigger one, which now has a hole.

This is how you lay out rooms on a floor slab, a window on a wall, or a step on a stair: draw the outline on the face, then push or pull the new face.

## Faces facing the wrong way

Every face has a front and a back. The back shows blue-grey. Faces drawn on the ground face up; faces drawn on another face face the same way it does.

If a face shows its back where you expect the front, select it and click {{ui:Reverse face}} in {{ui:Entity info}}.

## What you see while drawing

- A colored marker where the pointer snaps, and its name in the tip at the bottom left.
- A preview of the line or shape.
- The live size in the value box. You can type over it at any time.

## If the shape is not where you expected

- On a touch screen, see [Troubleshooting](topic:troubleshooting#a-shape-starts-in-the-wrong-place-on-a-touch-screen).
- If a line snapped to something far behind, orbit a little: points hidden behind a face never snap, but points on its edge do.
- Press {{key:Mod+Z}} to undo the last shape; one undo removes one line or one shape.
