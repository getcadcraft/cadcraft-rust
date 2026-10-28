---
title: Measure and place guides
summary: Read lengths and areas, measure between two points with the tape, and lay out guide lines and points to draw against.
section: tasks
order: 80
keywords: measure, tape measure, protractor, angle, angled guide, guide, guideline, construction line, distance, dimension, length, area, offset, layout, guide point
---

## Read sizes without measuring

Select something and look at {{ui:Entity info}}:

- An edge shows its **length**.
- A face shows its **area** and how many holes it has.
- Several faces show their **total area**.
- A group or component shows its **size** (width × depth × height of the box around it) and, if it is a closed solid, its **volume**.
- With nothing selected, {{ui:Entity info}} shows the size of the whole model.

Lengths and areas use the unit in the picker at the bottom right. A millimeter model shows areas of 0.01 m² and up in square meters (a 4 × 3 m room reads "12 m²").

While you draw, the value box shows the live length, size, radius or distance, so you rarely need to measure afterward.

To change a size after drawing, select an edge or a group and retype its length or size in {{ui:Entity info}}. Or place a [dimension](topic:dimensions) and type a new length into it.

## Measure between two points

1. Press {{key:T}} for the {{ui:Tape}} measure.
2. Click a point: a corner, a midpoint, a point on a face, or the ground. (Clicking an edge does something else; see below.)
3. Move to the second point. The tip and the value box ({{ui:Length}}) show the distance. It snaps to points and to the red, green and blue directions.
4. Click. The status bar says "Measured 4' 6"." and a **guide point** is left at the second point.

To leave the guide point at an exact distance instead, type it after the first click and press {{key:Enter}}: it goes in the direction you are pointing.

## Place a guide line

A guide line is an endless dashed line, parallel to an edge, at a distance you choose. Use guides to lay out a setback, a floor height, or where a window starts.

1. Press {{key:T}}.
2. Click on an **edge** (not its end). The tip says "On edge".
3. Move off the edge, to the side the guide should go. A dashed line follows, parallel to the edge, and the value box ({{ui:Offset}}) shows the distance.
4. Click, or type the offset and press {{key:Enter}}: `1.5m`, `5'`.

   The status bar says "Guide line 1.5 m from the edge."

Starting on an existing guide line makes another guide parallel to it, so you can step a grid out from one guide.

If you type an offset without moving off the edge first, ModelCraft cannot tell which side you mean: "Move off the edge first to show which side."

## A guide at an angle: Protractor

1. Click {{ui:Protractor}} in the {{ui:Modify}} group.
2. Click the center: the point the guide passes through. On a face, the protractor lies on that face; elsewhere it lies flat. After this click, {{key:Right}}, {{key:Left}} or {{key:Up}} stands it square to red, green or blue instead (press again to undo).
3. Click a point that sets 0°, for example along an edge.
4. Move to the angle you want and click: it snaps to 15° steps. Or type the angle in degrees, such as `22.5`, and press {{key:Enter}}; the guide goes to the side the pointer is on.

The guide line passes through the center at that angle. Draw along it with {{ui:Line}} for a roof pitch, a chamfer or a splayed wall.

## Draw against guides

Drawing tools snap to guide lines ("On guide") and guide points ("Guide point"), and to where a guide line crosses an axis direction. Start a rectangle where two guides cross to place it exactly.

## Remove guides

- One guide: click it with {{ui:Select}} and press {{key:Delete}}, or click it with the {{ui:Eraser}}.
- All guides: with nothing selected, click {{ui:Delete all guides}} in {{ui:Entity info}}.

Guides are saved in the `.modelcraft` file. They are not exported to other formats.
