---
title: Intersect faces
summary: Draw the lines where faces cut through each other, so you can push, erase or paint the pieces.
section: tasks
order: 53
keywords: intersect, intersect faces, intersect with model, intersect with selection, cut, opening, hole, join, lines where faces cross, boolean
---

When one shape passes through another, ModelCraft does not draw the line where their faces meet: they just overlap. **Intersect** draws those lines, as edges, so the faces split there. Then you can push/pull, erase or paint the pieces.

Both commands are in {{ui:Edit}} › {{ui:More}}.

## Intersect with model

Cuts what you select against **every face on screen**.

For example, a beam (a group) passes through a wall (another group), and you want an opening in the wall:

1. Double-click the wall to open it, and select all of its faces ({{key:Ctrl+A}}).
2. {{ui:Edit}} › {{ui:More}} › {{ui:Intersect with model}}.

The beam's outline is drawn on both faces of the wall, as two new faces. Push one of them through the wall to make the opening, then close the wall.

## Intersect with selection

Cuts the selected faces, groups and components **only against each other**. Select at least two.

## Where the lines go

The lines are always drawn in the **open context**: in the model when no group is open, or in the group you are editing. Groups and components that are not open are not changed. So to cut a group's faces, open it first, as in the example above.

## Things to know

- Faces that lie in the same plane are not intersected.
- Lines that close a loop on a face make a new face, as drawn lines do.
- Each intersect is one undo step. If nothing crosses, nothing changes.
- For closed shapes, the [solid tools](topic:solids) (Union, Subtract) do the whole job in one step.
