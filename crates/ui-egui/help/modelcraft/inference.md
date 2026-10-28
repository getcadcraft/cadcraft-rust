---
title: Inference and snapping
summary: What the colored markers mean, which snap wins, and how to lock a direction with the arrow keys or Shift.
section: reference
order: 30
keywords: inference, snap, snapping, endpoint, midpoint, center of face, centre, on edge, on face, axis, red axis, green axis, blue axis, parallel, perpendicular, lock, shift, osnap
---

While a drawing or editing tool is working, ModelCraft looks at what is under the pointer and snaps to the most useful thing nearby. A marker shows what it found and the tip at the bottom left names it.

## Markers and tips

| Tip | Marker | Snaps to |
|---|---|---|
| Endpoint | Green circle | The end of an edge |
| Midpoint | Cyan circle | The middle of an edge |
| Center of face | Blue circle | The middle of the face under the pointer |
| Guide point | Grey | A guide point left by the tape measure |
| Origin | Ochre | Where the red, green and blue axes meet |
| On red axis / On green axis / On blue axis | A line in that color | A line from your start point along that direction |
| Parallel to edge / Perpendicular to edge | Magenta line | A line from your start point parallel or perpendicular to the remembered edge (see below) |
| Locked | Magenta line | A direction you locked with an arrow key or {{key:Shift}} |
| On edge | Red square | Anywhere along an edge |
| On guide | Grey | Anywhere along a guide line |
| On face | Blue diamond | Anywhere on a face |
| On plane | none | The plane you started on, when the pointer is over empty space |
| On the ground | none | The ground (the red-green plane), when nothing else is there |

When an axis line from your start point crosses an edge or guide, ModelCraft snaps to the crossing and the tip names both, such as "On red axis, on edge".

## Which snap wins

1. **Points** within about 12 pixels: endpoints first, then guide points, midpoints, the center of the face under the pointer and the origin.
2. **Directions** from the start point: the red, green and blue axes, and parallel or perpendicular to the remembered edge.
3. **On an edge** or a guide line.
4. **On a face.**
5. **The drawing plane**, or the ground.

Things hidden behind a face never snap. Orbit so you can see the point you want.

## Drawing on a face from a point

A {{ui:Rectangle}}, {{ui:Circle}} or {{ui:Polygon}} started on a corner, a midpoint, an edge or the center of a face is drawn **in the plane of the face under the pointer**, and cuts that face. An edge belongs to two faces, so the side of the edge the pointer is on decides: to draw on the end of a long box, start from the middle of one of its edges with the pointer just inside the end face.

## Parallel and perpendicular

ModelCraft remembers the **last edge the pointer passed over before your first click**. Starting on a corner or on an edge counts as passing over that edge. Once the tool is working, other edges you hover do not replace it.

To draw a line parallel to an existing edge:

1. Press {{key:L}} and hover the edge you want to match.
2. Move to the start point, without crossing other edges on the way if you can, and click.
3. Move in the direction parallel to that edge. The line turns magenta and the tip says "Parallel to edge". Perpendicular works the same way, within the plane you are drawing on.

If the wrong edge was remembered, press {{key:Esc}}, hover the right edge, and start again.

## Lock a direction

After the first click of a tool, you can keep the next point on one line:

| Key | Locks to |
|---|---|
| {{key:Right}} | The red axis |
| {{key:Left}} | The green axis |
| {{key:Up}} | The blue axis (straight up) |
| {{key:Down}} | Parallel to the remembered edge (see above) |

Press the same arrow again to unlock. The status bar says "Locked to the red axis. Press the same arrow to unlock."

While locked, the point slides along the line toward wherever the pointer is, and still lines up with points you hover: hover the corner of another box to draw a line along red exactly as far as that corner.

**{{key:Shift}}** locks whatever direction you are snapped to right now (an axis, or parallel), for as long as you hold it. Let go to unlock.

Locks work in {{ui:Line}}, {{ui:Rectangle}}, {{ui:Circle}}, {{ui:Polygon}}, {{ui:Move}}, {{ui:Scale}} and {{ui:Tape}}. Before the first click, the arrow keys orbit the view instead.

## Inside a group

While you edit inside a group or component, the axes are the group's own axes (they move with it, and turn with it if it was rotated). Snaps still find points and edges outside the group, so you can line things up with the rest of the model.
