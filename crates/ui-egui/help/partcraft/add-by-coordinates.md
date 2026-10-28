---
title: Add sketch geometry by typing coordinates
summary: Use Add by coordinates in the sketch panel to place lines, centerlines, rectangles, circles and points at exact positions, with the keyboard only.
section: tasks
order: 30
keywords: coordinates, typed input, keyboard entry, numeric entry, exact position, x y, absolute coordinates, accessibility, no mouse
---

When you know the numbers, typing them is faster than clicking and dimensioning. **Add by coordinates** sits in the right panel whenever a sketch is open. It also lets you build a sketch without a pointer at all.

Coordinates are in millimeters, in the sketch's own axes: x to the right and y up as you look at the sketch, with (0, 0) at the origin (the red circle with a cross).

## Add a shape

1. Open a sketch (see [Draw your first sketch](topic:sketch-basics)).
2. In the right panel, scroll to **Add by coordinates**.
3. Choose the shape in **Add**: Line, Centerline, Rectangle, Circle or Point.
4. Fill in the boxes:

| Shape | Boxes |
|---|---|
| Line | **From x, y** and **To x, y** |
| Centerline | **From x, y** and **To x, y** |
| Rectangle | **Corner x, y** and **Opposite corner x, y** |
| Circle | **Centre x, y** and **Radius** |
| Point | **At x, y** |

5. Choose {{ui:Add}}, or press {{key:Enter}} in any box.

The shape appears and the status bar says, for example, "Added a rectangle." The boxes empty, ready for the next one. Each addition is one undo step.

Type a pair as two numbers separated by a comma or a space: `0, 0`, `25 -10` or `12.5,40`. Negative numbers and decimals are fine. A radius must be more than zero. If something does not read as a number, the status bar says: 'Type coordinates as "x, y" and a radius above zero.'

## Example: a slotted plate outline, typed

1. Open a sketch on **Front plane**.
2. **Add**: Rectangle. **Corner x, y**: `-50, -30`. **Opposite corner x, y**: `50, 30`. {{key:Enter}}.
3. **Add**: Circle. **Centre x, y**: `-30, 0`. **Radius**: `5`. {{key:Enter}}.
4. **Centre x, y**: `30, 0`. **Radius**: `5`. {{key:Enter}}.
5. Choose {{ui:Exit sketch}}, then {{ui:Extrude}}: a 100 × 60 plate with two 10 mm holes.

## Typed geometry is placed, not tied down

Typed coordinates put geometry in the right place, but they are **not relations**. The shapes stay blue (under defined) and could move if you drag them or change another dimension:

- a typed rectangle comes with its own corner, horizontal and vertical relations, like a drawn one, but nothing ties it to the origin;
- a typed line has no horizontal or vertical relation, even if you typed a level line;
- a typed circle's center is free.

To make the sketch fully defined, add dimensions and relations afterwards (see [Fully define a sketch](topic:relations-dimensions)). For example, select a rectangle corner and the origin and choose {{ui:• Coincident}}, or dimension the circle centers from the origin.

:::note
Without a pointer you can get this far: move to a plane row in the history with {{key:Tab}} and the arrow keys, press {{key:Enter}} to select it, press {{key:S}} to sketch, add shapes here, and reach **Edit** beside each dimension and {{ui:Exit sketch}} with {{key:Tab}}. Selecting geometry, and so adding relations or new dimensions, still needs a click or a tap in this build.
:::

## Limits

- Arcs cannot be typed yet. Draw them with the Arc tool, or type a circle and use relations.
- There are no relative coordinates (from the last point) or polar coordinates (length and angle) in this box.
- Typed geometry does not snap to existing points. Type the same coordinates as an existing point, then add a {{ui:• Coincident}} relation between them if they must stay joined.
