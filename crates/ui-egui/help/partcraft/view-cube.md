---
title: The view cube
summary: Click a face, edge or corner of the cube in the corner of the view to look from there, drag it to orbit, turn the compass ring, step around a face-on view, and go home.
section: tasks
order: 11
keywords: view cube, viewcube, cube, compass, north, home view, standard views, top, front, right, iso, corner, edge, orbit, rotate view, axis icon, corner indicator, navigation
---

The view cube sits at the top right of the 3D view (not while sketching or on the drawing sheet). It turns with the view, so it always shows which way you are looking: {{ui:Top}} is the part's +Y side (the Top plane), {{ui:Front}} the +Z side (the Front plane faces you when you look at it), {{ui:Right}} the +X side. It stays see-through until you point at it.

## Look from a side, an edge or a corner

1. Point at the cube. The part under the pointer lights up: the middle of a face, an edge (it lights on both faces that share it) or a corner (on all three faces).
2. Click it. The view turns to look from there, keeping the same middle and zoom.

There are 26 places to click: 6 faces ({{ui:Top}}, {{ui:Bottom}}, {{ui:Front}}, {{ui:Back}}, {{ui:Left}}, {{ui:Right}}), 12 edges and 8 corners. A corner gives the classic three-quarter view; an edge looks halfway between two faces.

Clicking {{ui:Front}}, {{ui:Top}} or {{ui:Right}} gives the same views as {{ui:Views}} in the toolbar, without the zoom to fit.

## Orbit by dragging the cube

Drag the cube to turn the view, the same as orbiting in the view itself. A drag is never taken as a click, so you can grab the cube anywhere.

## The compass ring

The ring under the cube carries N, E, S and W.

- **Drag the ring** to turn the model about the vertical axis, as if it stood on a turntable. The ring follows the pointer.
- **Click a letter** to look from that side, keeping how steeply you look down. In a view from straight above, the letter you click goes to the bottom of the screen.

## Face-on views: arrows

When you look straight at a face, small arrows appear round the cube:

- The four **triangles** bring the face on that side to the front: the triangle above the cube shows the face that was at the top, and so on.
- The two **curved arrows** at the top right turn the picture a quarter turn clockwise or counter-clockwise, without changing which face you look at. Use them to turn a plan so that a long wall runs across the screen.

## Home

The house button at the top left of the cube goes back to the three-quarter view and fits the part in. {{key:Home}} does the same while the cube has the keyboard focus.

## With the keyboard

{{key:Tab}} to the cube, then:

| Key | Does |
|---|---|
| {{key:left}} / {{key:right}} | choose a side, edge or corner round the cube, in eighth-turn steps (screen readers say which) |
| {{key:up}} / {{key:down}} | choose a higher or lower one, up to {{ui:Top}} or down to {{ui:Bottom}} |
| {{key:Enter}} or {{key:Space}} | look from the one chosen |
| {{key:Shift+left}} / {{key:Shift+right}} | turn the view a quarter turn about the vertical axis |
| {{key:Home}} | the home view |
| {{key:Esc}} | leave the cube |

Keys pressed on the cube stay with the cube: they do not start PartCraft tools.

## Axis icon or nothing instead

Open {{ui:Settings}} ({{key:Mod+,}}), the {{ui:PartCraft}} tab, and set **Corner indicator** to {{ui:View cube}} (the start), {{ui:Axis icon}} (a small X, Y, Z marker at the bottom left: X red, Y green, Z blue) or {{ui:None}}. This browser remembers the choice. See [PartCraft settings](topic:settings#corner-indicator).

:::note
The cube shows the part as PartCraft builds it: Y up, so the cube's {{ui:Top}} is the Top plane. The compass letters are only for turning; a part has no north.
:::
