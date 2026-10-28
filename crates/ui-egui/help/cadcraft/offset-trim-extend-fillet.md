---
title: Offset, trim, extend, fillet and chamfer walls
summary: Turn single lines into double walls with Offset, clean the corners with Fillet, and cut and lengthen lines with Trim and Extend.
section: tasks
order: 50
keywords: offset, trim, extend, fillet, chamfer, bevel, break, breakatpoint, lengthen, walls, corner, cut, lengthen, parallel, radius, cleanup
---

## What you will make

A small room with 200-thick walls: the inside face is drawn, the outside face is offset, the corners are cleaned up, and a doorway is cut through one wall.

## Draw the inside face

1. Type `L` and press {{key:Enter}}.
2. Type `0,0` and press {{key:Enter}}.
3. Type `@5000,0` and press {{key:Enter}}.
4. Type `@0,4000` and press {{key:Enter}}.
5. Type `@-5000,0` and press {{key:Enter}}.
6. Type `C` and press {{key:Enter}}.
7. Click {{ui:Extents}}.

You have four separate lines. (A closed polyline would offset in one go; separate lines show how Fillet cleans corners.)

## Offset the outside face

1. Type `O` and press {{key:Enter}}. The prompt reads `Specify offset distance [Through] <Through>:`.
2. Type `200` and press {{key:Enter}}.
3. At `Select object to offset`, click the bottom line.
4. At `Specify point on side to offset`, click anywhere below it. A parallel line appears 200 below.
5. Click the right line, then click to its right.
6. Click the top line, then click above it.
7. Click the left line, then click to its left.
8. Press {{key:Enter}} to end the command.

You should see four outer lines, each the same length as its inner line, so the outer corners do not meet.

:::tip
Press {{key:Enter}} at the distance prompt to use the last distance again. Type `T` for Through: then each copy goes through the point you click, at whatever distance that is.
:::

{{cmd:OFFSET}} works on lines, arcs, circles and polylines. Offsetting a closed polyline offsets the whole outline, corners included.

## Clean up the outer corners with Fillet

{{cmd:FILLET}} joins two lines at a corner: it trims or extends both so they meet. With a radius of 0 the corner is square.

1. Type `F` and press {{key:Enter}}. The prompt reads `Select first line [Radius]:`.
2. If the last fillet you did used a radius, type `R`, press {{key:Enter}}, type `0` and press {{key:Enter}}.
3. Click the outer bottom line near its right end.
4. At `Select second line`, click the outer right line near its bottom end.

The two lines now meet in a sharp corner, and the command ends.

5. Press {{key:Enter}} to repeat {{cmd:FILLET}}, and do the next corner.
6. Repeat until all four outer corners meet.

Click each line on the part you want to **keep**. With a radius above 0, Fillet adds an arc between the lines instead.

:::tip
Type `M` at the first prompt to do all four corners in one go, or draw the outside face as one closed polyline and type `P`: every corner is done at once. Hold {{key:Shift}} as you click the second line for a sharp corner without changing the radius.
:::

## Bevel a corner with Chamfer

{{cmd:CHAMFER}} cuts a corner off with a straight line.

1. Type `CHA` and press {{key:Enter}}.
2. Type `D` and press {{key:Enter}}. Type `300` and press {{key:Enter}} for the first distance, then press {{key:Enter}} to use 300 for the second too.
3. Click the inner top line near its left end, then the inner left line near its top end.

The corner is cut back 300 along each line and a bevel line joins them. Type `A` instead of `D` to give a length on the first line and an angle.

## Cut a doorway with Trim

First draw the two door jambs across the bottom wall, then trim the wall lines between them.

1. Type `L` and press {{key:Enter}}.
2. Type `1000,200` and press {{key:Enter}}.
3. Type `@0,-600` and press {{key:Enter}}, then press {{key:Enter}} again.
4. Press {{key:Enter}} to repeat {{cmd:LINE}}.
5. Type `1900,200` and press {{key:Enter}}.
6. Type `@0,-600` and press {{key:Enter}}, then press {{key:Enter}} again.

You now have two short lines crossing the bottom wall 900 apart.

7. Type `TR` and press {{key:Enter}}. The history says `Current settings: Quick mode, edges: No extend`.
8. Click the inner bottom line between the two jambs. That piece disappears. (Rest the pointer on it first: the piece that will go turns grey and dashed.)
9. Click the outer bottom line between the two jambs. That piece disappears too.
10. Click the part of each jamb that sticks out above the wall, and the part below it.
11. Press {{key:Enter}} to end the command.

You should see a 900 opening in the bottom wall, with the jambs closing the wall ends.

In Quick mode (the default) Trim does not ask you to choose cutting edges first: every visible object is a cutting edge. Pick the piece you want gone, or drag a path through several pieces to trim them all at once. Type `U` and press {{key:Enter}} to put back the last pick or drag without leaving the command. Fence, crossing, chosen edges and {{key:Shift}} to extend are in [Trim and extend](topic:trim-and-extend).

## Extend a line to meet another

{{cmd:EXTEND}} lengthens a line, arc or polyline end until it meets the next object in that direction.

1. Type `L` and press {{key:Enter}}.
2. Type `1000,2000` and press {{key:Enter}}.
3. Type `@2000,0` and press {{key:Enter}}, then press {{key:Enter}} again. The line stops 2000 short of the right wall.
4. Type `EX` and press {{key:Enter}}.
5. Click the new line near its right end.
6. Press {{key:Enter}} to end the command.

The line now runs to the inner face of the right wall. Every visible object is a boundary; pick near the end that should grow. Inside {{cmd:TRIM}}, {{key:Shift}}+click does the same.

## Break and lengthen

{{cmd:BREAK}} (`BR`, or {{ui:Modify}} › {{ui:More}} › {{ui:Break}}) cuts a gap without a cutting edge: click the object where the gap starts (that click is the first point), then click where it ends. On a circle the part counter-clockwise from the first point to the second goes. `F` at the second question lets you give the first point again, and {{cmd:BREAKATPOINT}} splits an object in two at one point with no gap.

{{cmd:LENGTHEN}} (`LEN`) changes a length at one end:

1. Type `LEN` and press {{key:Enter}}. Click a line to see its current length; the prompt asks again.
2. Choose how: `DE` (Delta: an amount to add, negative to take off; `A` for an arc's angle), `P` (Percent: 50 halves it), `T` (Total: the length it should be) or `DY` (Dynamic: drag the end).
3. Click each object near the end that should move. `U` takes back the last one; {{key:Enter}} ends.

Lines, arcs and open polylines (their end segment) can be lengthened.

## Common mistakes

- **A whole line disappeared.** Nothing crossed it, so Quick mode erased it. Type `U` to put it back; check that the cutting line really crosses it (zoom in) and is not on a layer that is off.
- **"No boundary in that direction."** Extend found nothing to run into. Pick nearer the end that should grow.
- **"Radius is too large."** The arc would run past the end of one of the lines. Use a smaller radius.
- **"Only the first or last segment of an open polyline can be cut back."** Pick two segments of the polyline next to each other, or type `P` to round every corner of it.
- **The offset went to the wrong side.** The side is where you click after picking the object. Undo with {{key:Mod+Z}} and click on the other side.
- **"Cannot offset that far."** The offset would turn the arc, circle or polyline inside out. Use a smaller distance.
