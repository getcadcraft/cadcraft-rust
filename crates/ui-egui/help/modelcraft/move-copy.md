---
title: Move, copy and make arrays
summary: Move things by clicking or by an exact distance, copy them with a modifier key, and make a row of evenly spaced copies.
section: tasks
order: 50
keywords: move, copy, duplicate, array, row, spacing, divide, offset, stretch, columns, repeat, multiple copy
---

{{ui:Move}} ({{key:M}}) moves whatever is selected from one point to another. Holding a modifier key makes it a copy, and after a copy you can type how many more you want.

## Move something

1. Select what you want to move (see [Select things](topic:select)). If nothing is selected, Move takes the thing you click on.
2. Press {{key:M}}.
3. Click a point to move from. Pick something meaningful, such as a corner: that point will land exactly where you click next.
4. Move the pointer. The selection follows. It snaps to points, edges and the red, green and blue directions like a line does.
5. Click where the point goes, or type a distance and press {{key:Enter}}.

   The status bar's tip returns to "Click a point to move from…" for the next move.

A typed distance goes in the direction you are pointing. You can also type an exact offset, `<2m,0,0>` (2 m along red), or an exact destination for the point you picked, `[0,0,0]` (the origin). See [Type exact sizes](topic:value-box).

To move along one axis only, press an arrow key after the first click: {{key:Right}} locks to red, {{key:Left}} to green, {{key:Up}} to blue. Press the same arrow again to unlock.

## Moving part of a shape stretches it

Geometry that is not grouped is connected. If you select one face or a few edges and move them, the faces attached to them stretch to follow. Move the top face of a box to the side and the box leans; move one edge of the roof and the roof changes pitch. Faces that can no longer stay flat fold into triangles.

To move a whole object without stretching it, select all of it first (triple-click it), or better, make it a group or component (see [Groups and components](topic:groups-components)).

## Copy something

1. Select what you want to copy.
2. Press {{key:M}}.
3. Hold {{key:Ctrl}} (or {{key:Alt}}) and click the point to move from. On a Mac, {{key:Mod}} or {{key:Alt}} does the same. The tip says "Ctrl moves instead", which means you are copying.

   You can also press and release {{key:Ctrl}} or {{key:Alt}} after the first click to switch between moving and copying.

4. Click where the copy goes, or type a distance and press {{key:Enter}}.

   The original stays where it was, and the copy is selected.

## Copy, cut, paste and duplicate

- {{key:Mod+C}} copies the selection, {{key:Mod+X}} cuts it (copies, then erases), {{ui:Copy}} and {{ui:Paste}} are also buttons in the toolbar's {{ui:Edit}} group, and {{ui:More}} beside them has cut, paste in place and duplicate.
- {{key:Mod+V}} pastes: an outline of the copy follows the pointer, snapping like any tool, and a click puts it down with its lowest corner there. {{key:Esc}} cancels.
- {{key:Mod+Shift+V}} pastes in place, exactly where the copy came from — useful for moving something into a group you have opened.
- {{key:Mod+D}} ({{key:Shift+D}} in the Modeler style) duplicates: a copy is made and moves with the pointer at once; click to put it down, type a distance, or press {{key:Esc}} to drop it. Right after, type `3x` for more copies in a row.

A pasted group becomes a new group. A pasted component is another placement of the same component, so editing one still changes them all. Copies also paste into another ModelCraft tab in this browser (they come in as new definitions there).

## A row of copies

Right after a copy, while {{ui:Move}} is still the tool:

- Type `3x` (or `*3`) and press {{key:Enter}} for three copies in a row, each at the same spacing as the first copy. You get three new copies besides the original.
- Type `/3` and press {{key:Enter}} to divide instead: three copies spread evenly from the original to where you put the first copy, the last one at that spot. Use this to fill a span: place one copy at the far end, then divide.

You can type a different number right after: `5x` replaces the `3x` row. The tip reminds you: "Copied. Type 3x for more copies in a row, or /3 to divide the distance."

### Example: columns every 6 meters

1. Make a column and make it a component ({{key:G}}), so all copies change together later.
2. Press {{key:M}}, hold {{key:Ctrl}} and click the column's base corner.
3. Move along the red axis and type `6m`, then {{key:Enter}}.
4. Type `4x` and {{key:Enter}}. You have five columns, 6 m apart.

## Things to know

- Press {{key:Esc}} before the second click to cancel; everything goes back where it was.
- Move works inside the group you are editing. To move a whole group, close it first and select it from outside.
- With **Collide** on (bottom right, and in [ModelCraft settings](topic:settings#solids-stop-where-they-touch)), a move stops where the selection touches another solid instead of passing through it. Click {{ui:Collide}} to turn it off for a move that should go through.
- To put a copy inside another group: copy it, open the other group, and paste ({{key:Mod+Shift+V}} keeps it in the same place).
