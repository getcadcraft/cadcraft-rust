---
title: Rotate and scale
summary: Turn things about a point by clicking or by typing degrees, and resize them evenly by a factor.
section: tasks
order: 60
keywords: scale grips, stretch, non-uniform, per axis, flip, mirror, mirrored copy, flip along, rotate, turn, angle, degrees, protractor, scale, resize, enlarge, shrink, factor, uniform scale
---

## Rotate

{{ui:Rotate}} ({{key:Q}}) turns the selection about a center point. It takes three clicks: the center, a starting direction, and the end direction.

1. Select what you want to turn. If nothing is selected, Rotate takes the thing you click first.
2. Press {{key:Q}}. A ring follows the pointer, showing the plane you will turn in.
3. Click the center of rotation.
   - On the ground or in empty space, the ring lies flat (blue): the turn is about a vertical axis, like turning a building on its site.
   - On a face, the ring lies in that face's plane: the turn is about the face's own direction. Click on a wall to turn something about a horizontal axis.
4. Click a point that sets where the angle starts, for example along an edge of the thing you are turning.
5. Move the pointer around the center. The angle shows in the tip and in the value box ({{ui:Angle}}). It clicks into steps of 15° when you get close to one.
6. Click to finish, or type the angle in degrees and press {{key:Enter}}: `90`, `45`, `-30`.

   Positive angles turn counterclockwise, seen from the side the ring faces.

Like Move, rotating part of connected geometry twists the faces attached to it. Group things first to turn them whole.

:::note
The tip says arrow keys pick the rotation axis, but in this build they orbit the view instead before the first click. To choose the axis, click the center on a face that faces the way you want. See [Not built yet](topic:not-built-yet).
:::

## Scale

{{ui:Scale}} ({{key:S}}) resizes the selection. Select what you want to resize first, then press {{key:S}}: small squares (grips) appear on the corners, the edges and the middles of the sides of its box.

### Stretch or scale with the grips

- A **corner** grip scales evenly in every direction, about the opposite corner.
- The grip in the middle of a **side** stretches along that direction only, about the opposite side: make a box longer without making it taller.
- The grip in the middle of a **box edge** stretches in the two directions across it.
- Hold {{key:Ctrl}} when you click a grip to scale about the middle of the box instead.

Click a grip, move the pointer, and click again; or click the grip and type a factor (`2` doubles, `0.5` halves) and press {{key:Enter}}. To give each direction its own factor, type three, for red, green and blue: `2,1,0.5`. Inside an open group, the directions are the group's own.

### Scale about any point

Click anywhere other than a grip to scale evenly about a point of your own. It takes three clicks:

1. Select what you want to resize. If nothing is selected, Scale takes the thing you click first.
2. Press {{key:S}}.
3. Click the point that stays put (not on a grip), for example a bottom corner.
4. Click a reference point. Its distance from the first point counts as 1×.
5. Move the pointer toward or away from the first point. The factor shows in the tip and in the value box ({{ui:Scale}}).
6. Click to finish, or type a factor and press {{key:Enter}}: `2` doubles the size, `0.5` halves it.

### Scale to an exact size

To make something an exact length, work out the factor: new length divided by current length. The value box does not do arithmetic, so work it out first: to make a 3.6 m wall 4.2 m long, 4.2 divided by 3.6 is about 1.1667, so type `1.1667`. Select the edge first to read its current length in {{ui:Entity info}}.

### Fix an import that came in the wrong size

OBJ files have no unit, so a model drawn in millimeters comes in 1000 times too big. Select the imported group, press {{key:S}}, click a corner, click any other point, and type `0.001`. For a model drawn in inches, type `0.0254`.

## Things to know

- Scale is uniform only: there is no stretching in one direction and no grips on a box around the selection. To make a box longer in one direction, push/pull its end, or move its end face.
- Scaling a group or component changes the copy you scaled; other copies of a component keep their size.
- Press {{key:Esc}} before the last click to cancel.

## Flip

{{ui:Edit}} › {{ui:More}} › {{ui:Flip along red}}, {{ui:Flip along green}} or {{ui:Flip along blue}} mirrors the selection in place, through the middle of its box: a left-hand stair becomes a right-hand one. Faces keep facing out, and a group or component is mirrored as a whole (its insides are not rewritten).

{{ui:Flipped copy along red}} (and green, blue) leaves the original where it is and puts a mirrored copy beside it, across the far side of the selection's box: the other door of a pair, the other wing of a building. Loose geometry touching there joins the original, as touching geometry always does; make the original a group first to keep them apart.

Inside an open group, red, green and blue are the group's own axes.
