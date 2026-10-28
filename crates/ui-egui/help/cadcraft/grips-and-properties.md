---
title: Edit with grips and Properties
summary: Stretch, move and resize objects by their grips, and type exact lengths, radii, angles and coordinates in the Properties panel.
section: tasks
order: 60
keywords: grips, properties, stretch, length, radius, coordinates, edit, quick properties, properties palette, change, layer, colour, color
---

## What you will learn

Two quick ways to change an object without a command: drag its **grips**, or type new values in the **Properties** panel on the right.

## Grips

When you select objects (up to 200 at a time) with no command running, small squares appear on them. These are grips:

| Object | Grips |
|---|---|
| Line | Both ends, and the middle (moves the whole line) |
| Circle | The center (moves it) and four points on the circle (change the radius) |
| Arc | The center, and both ends (change where it starts and stops) |
| Polyline | Every vertex |
| Text, point, block reference | The insertion point |
| Ellipse | The center |
| Dimension | Its defining points and the dimension line position |
| Viewport | The center (moves it) and the four corners (resize it) |

### Move a grip

1. Draw a line and click it. Three grips appear.
2. Click the grip at one end. The status bar says "Click where this grip goes (Esc to cancel). Snaps and Ortho apply."
3. Move the crosshair. A preview follows.
4. Click the new position, or snap to a point on another object and click.

The end moves and the rest of the line stays put. You can also press on a grip and drag it, then let go where it should go.

:::note
You cannot type a coordinate for a grip. For an exact position, snap to a point, or type the value in Properties.
:::

Press {{key:Esc}} or right-click to cancel a grip move.

## Properties

The right panel shows the properties of what is selected.

1. Click a line. The panel heading reads "Line".
2. Click in {{ui:Length}}, type `2500` and press {{key:Enter}}.

The line is now exactly 2500 long, keeping its start point and angle. {{key:Enter}} or clicking elsewhere applies a value; {{key:Esc}} puts the old value back. Every change is one undo step, also when many objects change at once.

Fields take expressions (`=2*45`) and lengths with units (`3'6"`, `900mm`), every point has X, Y and Z, and several objects can be changed together. All of it is in [The Properties panel](topic:properties-panel).

## Common mistakes

- **No grips appear.** A command is running (press {{key:Esc}}), or more than 200 objects are selected, or the object is on a locked layer.
- **The grip went to the wrong place.** A snap caught it. Hold {{key:Shift}} while you click to ignore snaps.
- **Typing in Properties did nothing.** Press {{key:Enter}} or click elsewhere to apply. A refused entry (zero or negative lengths and radii, text that is not a value) turns the field red and says why.
- **Typed letters went into a Properties field.** While a field has the cursor, keys type there. Press {{key:Enter}} or {{key:Esc}}, then type the command.
