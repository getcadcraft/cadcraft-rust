---
title: Lasso selection
summary: Press and drag a freehand outline round objects to select them, as a window, a crossing or a fence; switch with Space while you drag.
section: tasks
order: 31
keywords: lasso, lassomode, drag, freehand, select, selection, window, crossing, fence, space, polygon, touch, pen, rectangle
---

## What you will learn

How to select objects by drawing round them, and how to get the rectangle back when you want it.

## Draw a lasso

1. Draw a few lines and circles close together.
2. With no command running, press the mouse button on an empty spot, and drag round some of them, moving **right** first.
3. Let go.

Only the objects wholly inside the outline are selected. The outline is drawn solid while you drag: this is a window lasso.

## Window, crossing and fence

Which objects you get depends on the first move of your drag, and on {{key:Space}}:

| You drag | Lasso | Selects |
|---|---|---|
| Right first | Window (solid outline) | Only objects **wholly inside** |
| Left first | Crossing (dashed, green) | Objects inside **and** objects the outline touches |
| Press {{key:Space}} while dragging | Window, then crossing, then fence | Fence (a dashed path, not closed): only objects the path crosses |

1. Press on an empty spot and drag round some objects, moving **left** first. Everything the outline touches is selected too.
2. Press {{key:Esc}}.
3. Press and drag again; while still holding the button, press {{key:Space}} once. The outline turns into a crossing; press {{key:Space}} again for a fence.

{{key:Shift}} while you let go takes the objects out of the selection instead (with "Clicking objects: Adds" in Settings).

## Inside a command

A lasso works wherever CadCraft asks `Select objects`: draw it, then press {{key:Enter}} to finish. {{cmd:STRETCH}} takes a crossing lasso as its window, so you can stretch an L-shaped part of a drawing in one go.

{{cmd:TRIM}} and {{cmd:EXTEND}} use a drag differently: there it trims (or extends) every piece the path crosses. See [Trim and extend](topic:trim-and-extend).

## The rectangle

A click on empty space, a move, and a second click always draws the rectangle window (left to right) or crossing (right to left).

To make a drag draw the rectangle again, open {{ui:Settings}} › {{ui:CadCraft}} › {{ui:Dragging to select}} and choose {{ui:Rectangle}}, or type {{cmd:LASSOMODE}}, then `0`. Type {{cmd:LASSOMODE}}, then `1` for the lasso. The choice is kept in this browser.

:::touch
Drag a finger or a pen the same way. A tap still picks one object.
:::

## Common mistakes

- **Nothing was selected.** The lasso was a window and nothing lay wholly inside. Start the drag to the left, or press {{key:Space}} while dragging.
- **The drag moved a grip.** Pressing on a selected object's grip moves the grip. Start the lasso on an empty spot.
- **Space ended the command.** {{key:Space}} switches the lasso only while the button is held down; otherwise it is Enter.
