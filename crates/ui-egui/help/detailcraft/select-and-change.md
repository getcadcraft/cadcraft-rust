---
title: Select by window, move, rotate, mirror, change many
summary: Drag a window to select, drag selected parts to move them, move, rotate or mirror the selection (or a copy of it), or change the profile, material, position or rotation of many members at once.
section: tasks
order: 44
keywords: lasso, fence, freehand selection, drag, drag and drop, move by dragging, snap, window, crossing, box select, drag select, select many, move, rotate, mirror, flip, copy, change many, several members, profile, material, position, rotation, varies, grid line
---

Each of these is one undo step: one {{key:Mod+Z}} takes it all back.

## Select with a window

- In {{ui:Plan}} and {{ui:Elevation}}, drag with the left button from an empty spot. In {{ui:3D}}, hold {{key:Shift}} and drag (a plain drag turns the model).
- Drag **to the right** for a window: it takes what is wholly inside, drawn with a solid edge.
- Drag **to the left** for a crossing window: it takes everything it touches, drawn dashed.
- Hold {{key:Shift}} while you start the drag to add to the selection instead of replacing it.
- A plan or elevation only offers what it shows: parts cut away above the plan, or more than 1 m from the elevation's grid line, are not picked.
- {{key:Esc}} while dragging drops the window.

### Lasso

Choose **Lasso** under {{ui:Dragging to select}} in [DetailCraft settings](topic:settings) and the same drag draws a freehand line instead of a rectangle.

- Its first move sideways decides how it selects: **to the right**, what is wholly inside (solid line); **to the left**, what it touches (dashed).
- Press {{key:Space}} while dragging to switch between inside, touching and **fence**: an open line that takes whatever it crosses (dotted, orange).
- {{key:Shift}} to add and {{key:Esc}} to drop it work as for the window.

:::touch
On a touch screen, one finger pans the plan and elevation as before; select parts by tapping them.
:::

In {{ui:Plan}} and {{ui:Elevation}}, pan with the right or middle button while the {{ui:Select}} tool is on.

## Drag parts to move them

1. Select the parts first (click, {{key:Shift}}+click or a window).
2. Press on one of the selected parts and drag. The whole selection moves; a dashed outline shows where it goes.
3. Let go to place it. {{key:Esc}} before you let go puts everything back.

- In {{ui:3D}} and {{ui:Plan}} the parts move across the level. Hold {{key:Mod}} while you start the drag in {{ui:3D}} to move them up and down instead.
- In {{ui:Elevation}} they move within the elevation: along the grid line and up and down.
- The part you hold snaps onto a grid intersection when its end comes within a few pixels of one (the intersection lights up and the status bar names it). Elsewhere the move goes in 10 mm steps.
- Pressing on a part that is not selected still turns or pans the view, so a drag never moves something by accident.
- The drag is one undo step, like {{ui:Move}}. Connections follow their members.

## Change a property on many members

Select several members. Under **Change on all of them** in the {{ui:Properties}} panel:

- {{ui:Profile}}, {{ui:Position}}: for the beams, columns and braces among them.
- {{ui:Plate thickness (mm)}} and {{ui:Plate width (mm)}}: for the plates.
- {{ui:Material}} and {{ui:Rotation (°)}}: for all of them.

A field shows **Varies** when the members do not agree. Pick or type a value and every member it applies to takes it; the status bar says how many changed and how many kept their own.

## Move, rotate or mirror

Select members and footings (a single one works too), then use **Move, rotate, mirror** in the {{ui:Properties}} panel. Connections are not moved on their own: they follow their members. If you move a beam but not its column, the status bar says how many connections now join a moved member to one that stayed, and [Problems](topic:troubleshooting) says if they no longer fit.

- **Move:** type x, y and z in millimeters and choose {{ui:Move}}. Or choose {{ui:Pick two points}} (or press {{key:M}}): click the grid intersection to move from, then the one to move to. A dashed outline follows the pointer.
- **Rotate in plan:** type the angle (counter-clockwise, seen from above) and choose what to turn about: the selection's middle, or a grid intersection. Columns turn with the frame.
- **Mirror:** choose a grid line. The mirror image is the same profile turned the other way: an I-shape keeps its flanges, a channel faces the other way (its top and bottom swap), and holes and copes go with the part.
- Tick **Keep the original (make a copy)** to leave the selection where it is and move, rotate or mirror a copy. Its connections come along when {{ui:Copy end plates and base plates with their members}} is on in [DetailCraft settings](topic:settings).

Rotating turns about a vertical line and mirroring is in a vertical plane, so levels stay level and footings stay upright.
