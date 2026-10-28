---
title: Erase and clean up
summary: Erase edges, groups and guides with the eraser or the Delete key, heal split faces, and fix faces that face the wrong way.
section: tasks
order: 90
keywords: erase, eraser, delete, remove, heal, merge faces, clean up, stray lines, reverse face, back face, undo
---

## Erase with the eraser

1. Press {{key:E}} for the {{ui:Eraser}}.
2. Click an edge, a group or component, or a guide. It is erased.
3. To erase several, press and drag across them. Everything you pass over highlights, and all of it goes when you release.

The eraser does not pick faces: a face goes when you erase the edges it needs. Faces are what the eraser looks through to find edges.

## What happens to the faces

- **An edge between two faces in the same plane** (a line you drew across a floor, for example) disappears and the two faces **heal** into one.
- **An edge that a face needs to stay closed** (the edge of a box) takes that face with it. Erase one top edge of a box and the top and one side go, leaving an open box.
- **Loose ends** (points no edge uses any more) are cleaned up.

Erasing the last copy of a component also removes the component from the {{ui:Components}} panel.

## Erase with the Delete key

1. Select what you want to remove with {{ui:Select}}.
2. Press {{key:Delete}} or {{key:Backspace}}.

This removes faces too. A face deleted this way leaves its edges behind, so you can delete the top of a box and keep its outline, then draw something else there.

## Clean up stray lines on a flat face

If a wall or floor shows lines you don't want (left over from drawing), erase them with the {{ui:Eraser}}. Lines lying in the face heal away; the face stays.

## A face facing the wrong way

A face seen from behind is blue-grey. If a face shows blue-grey on the outside of your model:

1. Select the face.
2. Click {{ui:Reverse face}} in {{ui:Entity info}}.

Face direction matters most when you export: renderers, game engines and 3D printers may hide or misread faces that point inward.

## Undo a mistake

{{key:Mod+Z}} undoes one step at a time, {{key:Mod+Y}} (or {{key:Mod+Shift+Z}}) redoes. A drag with the eraser is one step, however many things it took. The status bar names what was undone: "Undid erase."
