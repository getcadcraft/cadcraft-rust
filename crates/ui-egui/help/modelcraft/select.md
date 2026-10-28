---
title: Select things
summary: Click, double-click and triple-click, drag a window or a crossing box, add and remove with modifier keys, and delete.
section: tasks
order: 45
keywords: select, selection, window, crossing, add to selection, deselect, select all, triple click, double click, delete, erase
---

{{ui:Select}} ({{key:Space}}) is the tool ModelCraft starts with, and the one {{key:Esc}} brings you back to. Selected things turn blue, and {{ui:Entity info}} on the right says what they are.

## Click

| Do this | Selects |
|---|---|
| Click a face or an edge | That face or edge |
| Click a group or component | The whole group or component |
| Double-click a face | The face and its edges |
| Double-click an edge | The edge and the faces on either side |
| Double-click a group or component | Opens it for editing (see [Groups and components](topic:groups-components)) |
| Triple-click a face or edge | Everything connected to it: the whole object |
| Click empty space | Nothing (clears the selection) |
| Double-click empty space while editing a group | Closes the group |

Edges are easier to hit than faces: when the pointer is within a few pixels of an edge, the click takes the edge.

## Drag a box

- **Left to right** draws a solid box: a **window** selection. Only things wholly inside it are selected.
- **Right to left** draws a dashed box: a **crossing** selection. Anything the box touches is selected, and a face is selected when the box sits entirely inside it.

A group or component counts as one thing: a window must hold all of it, a crossing only has to touch it.

## Add and remove

Hold a key while you click or drag:

| Key | Effect |
|---|---|
| {{key:Shift}} | Toggles: adds what is not selected, removes what is |
| {{key:Mod}} | Adds |
| {{key:Shift}} and {{key:Mod}} together | Removes |

To select everything in the model (or everything in the group you are editing), press {{key:Mod+A}}.

## Clear the selection

Press {{key:Esc}} (with {{ui:Select}} as the tool), or click empty space.

## Delete what is selected

Press {{key:Delete}} or {{key:Backspace}}. Faces, edges, groups and guides in the selection are removed in one step; {{key:Mod+Z}} brings them back.

Deleting an edge removes the faces that need it, unless the edge only separated two faces in the same plane: then the two faces join into one. Deleting a face leaves its edges. See [Erase and clean up](topic:erase).

## Select inside a group

Clicking a group selects the whole group. To select a face inside it, double-click the group to open it, then click. While a group is open, clicks only reach what is inside it; everything else is faded. Press {{key:Esc}} with nothing selected, or double-click empty space, to close it.

The {{ui:Outliner}} on the left selects groups and components by name, which helps when they are hidden behind other things.
