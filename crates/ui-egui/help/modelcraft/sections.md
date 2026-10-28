---
title: Section planes: cut the model open
summary: Place a section plane to see inside the model, slide it through, reverse it or turn the cut off.
section: tasks
order: 49
keywords: section, section plane, section cut, cut, cutaway, clip, inside, floor plan, slice, cross section
---

A section plane cuts the model open, so you can see and work inside it: a floor plan of a house, the inside of a cabinet. The model is not changed; the cut is only a view.

## Place one

1. Click {{ui:Section}} in the {{ui:Camera}} group.
2. Click a face. The plane is placed on that face, with four arrows pointing at you.

   Click away from any face for a plane square to the direction you look in most. Press an arrow key first to stand it square to red ({{key:Right}}), green ({{key:Left}}) or blue ({{key:Up}}) instead.

The side the arrows point at is **cut away**. A plane placed on a face cuts nothing yet: slide it into the model.

## Slide it, reverse it, turn it off

Click the plane's outline to select it. In {{ui:Entity info}}:

- {{ui:Along its arrows}} says where it sits. Type a new distance and press {{key:Enter}} to slide it through the model; typing `-1m` on a plane you placed on a front face moves it 1 m into the model.
- {{ui:Reverse}} turns the arrows round, so the other side is cut away.
- {{ui:Active (cuts the model)}}: one plane cuts at a time. Tick another plane's box, or place a new one, and that one cuts instead.
- Rename it in {{ui:Name}}; delete it with {{key:Delete}} or the {{ui:Eraser}}.

Where the plane passes through a closed solid, the cut is filled in a dark color. Geometry that is not closed (a single face, an open box) is cut but not filled.

## Show and hide

In {{ui:Views}}:

- {{ui:Show section planes}} hides the planes' outlines and arrows (the cut stays).
- {{ui:Show section cuts}} turns the cut off and shows the whole model, without deleting the planes.

[Presentation view](topic:styles-shadows#presentation-view) hides the outlines too.

## Things to know

- What is cut away cannot be clicked or snapped to, so you can draw on what the cut shows.
- Section planes, which one is active and the two switches are saved in the ModelCraft file. Each change is one undo step.
- Exports always contain the whole model: a section is a view, not an edit.
