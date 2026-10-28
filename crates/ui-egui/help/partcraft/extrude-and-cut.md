---
title: Extrude a boss and cut through it
summary: Turn a sketch into a solid with Extrude, set blind, mid-plane, up-to or two-direction depths, a taper, or one contour of the sketch, then sketch on a face and Cut a pocket or a through slot.
section: tasks
order: 40
keywords: extrude, boss, extruded boss, cut, extruded cut, pocket, through all, blind, mid-plane, reverse, depth, cut-extrude, boss-extrude, pad, up to face, up to next, up to vertex, offset from surface, direction 2, two directions, draft while extruding, taper, contour, region, selected contours
---

{{ui:Extrude}} pushes a sketch's closed profile straight out of its plane to make material. {{ui:Cut}} does the same but removes material. Both are the same feature underneath; you can switch one to the other later.

## Extrude a boss

1. Draw a closed profile and close the sketch (see [Draw your first sketch](topic:sketch-basics)). It stays selected.
2. Choose {{ui:Extrude}}.

A 10 mm solid appears, "Extrude1" joins the history with its sketch indented under it, and the status bar says "Extrude1 added. Change its numbers on the right."

**Which sketch is used:** the sketch you just closed, or the sketch selected in the history. With nothing selected, PartCraft takes the most recent sketch that no feature uses yet. If there is none: "Make a sketch first: pick a plane, Sketch, draw a closed profile, Exit."

### Set the depth and direction

In the right panel:

| Setting | Choices | Default |
|---|---|---|
| **Operation** | Boss (add material), Cut (remove material), Intersect (keep the overlap), New body (not built yet in this version: it needs an exact kernel) | Boss |
| **End** | Blind (a depth), Mid-plane (both ways), Through all, Up to face or plane, Up to next face, Up to vertex, Offset from face | Blind |
| **Depth (mm)** | Any size above zero (shown for Blind and Mid-plane) | 10 |
| **Reverse direction** | Grows to the other side of the sketch plane (not shown for Mid-plane) | Off |

Type a number and press {{key:Enter}} or {{key:Tab}}: the part rebuilds immediately. **Mid-plane** puts half the depth on each side of the sketch.

### Up to a face, the next face or a vertex

Not built yet in this version: every "up to" end needs an exact kernel, and the feature says "Extruding up to a face, the next face or a vertex: not built yet (it needs an exact kernel)." Use a Blind depth you measure. With an exact kernel the ends work like this:

- **Up to face or plane**: **Up to** takes a flat face (Pick, then click it) or a reference plane. A face parallel to the sketch gives an exact depth; a slanted one cuts the end on the slope. If the face is behind the sketch: "… Try Reverse direction."
- **Up to next face**: stops at the first flat face of the body the extrusion meets (curved faces are not supported here: it says so).
- **Up to vertex**: stops level with a vertex (click near the end of an edge) or a point feature.
- **Offset from face**: like Up to face, stopping **Offset (mm)** short of it (measured square to the face).

### Two directions, taper, one contour

- **Second direction** (Blind): also extrudes **Second depth (mm)** to the other side of the sketch.
- **Taper (°)**: the sides lean in as they go (negative: out), like a draft; one direction only. Not built yet in this version (it needs an exact kernel): leave it at 0.
- **Regions**: **Pick regions**, then click inside the loops to extrude; only those regions are used (a circle inside a square: click inside the circle for a cylinder, between them for a plate with a hole). **All** goes back to every loop.

Below these, **Sketch dimensions** lists the sketch's driving dimensions, so you can change the profile's sizes without opening the sketch.

:::note
**Through all is only for cuts.** A boss set to Through all fails with "Through all is for cuts. Use a depth for a boss." Switching a feature from Cut back to Boss changes Through all to Blind for you.
:::

## Cut a pocket

1. Click the flat face you want to cut into. It highlights, and the right panel says **Face**.
2. Choose {{ui:Sketch on this face}} (or {{ui:Sketch}} in the toolbar).
3. Draw the pocket's outline, for example a rectangle, and dimension it.
4. Choose {{ui:Exit}}.
5. Choose {{ui:Cut}}. It starts as **Through all**: it removes material right through the part.
6. For a pocket, set **End** to **Blind (a depth)** and type the **Depth (mm)**.

A cut goes into the material, away from the face you sketched on. If the status bar says "There is nothing to cut on that side of the sketch. Try the other direction.", tick **Reverse direction**.

A sketch on the pocket's floor follows the floor: change the pocket depth and the sketch moves with it.

## Profiles with islands

A sketch can hold several closed loops. Loops side by side become separate bosses (or cuts); a loop inside another becomes a hole through the boss. For example, a rectangle with two circles inside extrudes as a plate with two holes.

To use only some loops, pick them with **Regions** (above); another feature can use the other loops of the same sketch.

## Edit it later

- Click "Extrude1" in the history to bring back its settings.
- Double-click its sketch (indented under it) to open the sketch; close it to rebuild.
- See [Change a part through its history](topic:edit-history) for suppressing, rolling back, reordering and deleting.

## Common messages

| Message | What to do |
|---|---|
| "The sketch has no closed profile to extrude." | The sketch has no closed loop of solid lines. Close the gaps, or switch construction lines back with {{key:Q}} |
| "The profile is open: an end at (x, y) is not joined to anything." | Open the sketch and join that end to the next line |
| "There is nothing to cut yet." | A cut needs a solid to cut. Make a boss first |
| "Nothing is left of the part after this." | The cut removed everything. Check its size and direction |

More in [Troubleshooting](topic:troubleshooting).
