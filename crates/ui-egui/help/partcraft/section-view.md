---
title: Look inside with a section view
summary: Cut the part or an assembly with a plane to see inside it, with the cut faces filled. Move the plane with a slider, cut on a flat face you click, flip which side is cut away.
section: tasks
order: 126
keywords: section, section view, cut, cutaway, cross section, clipping, inside, slice, plane, caps, hatch, assembly section
---

The **section view** cuts the model with a plane so you can see inside: holes, pockets, wall thicknesses, how parts in an assembly sit together. It works in every workspace and in an assembly. Open it from the **View** group's {{ui:Views}} menu, {{ui:Section view}}, or press {{key:X}}. A card opens at the bottom left of the view.

Only the view is cut. The part, its volume, measurements and exports stay whole, and nothing is saved in the file.

## Cut through the middle

1. Press {{key:X}}. The part is cut by the front plane through its middle, and the cut faces are filled with a tinted colour.
2. Drag the {{ui:Position}} slider, or type a distance in mm, to move the cut.
3. Choose another {{ui:Plane}}: {{ui:Front (XY)}}, {{ui:Top (XZ)}} or {{ui:Right (YZ)}}.
4. Tick {{ui:Cut away the other side}} to keep the half that was cut away instead.

## Cut on a face

1. Click {{ui:Pick a face}} on the card.
2. Click a flat face in the view. The cut starts flush with it, with the plane on that face.
3. Move the {{ui:Position}} to go deeper. The number is the distance along the face's outward direction.

## Clicks and the cut

Clicks in the view go to what is left after the cut, so you can pick a face or an edge inside the part, start a sketch on it, or measure across it. Untick {{ui:Fill the cut faces}} to see through the cut into the hollow parts.

## Close it

Press {{key:Esc}}, press {{key:X}} again, or click {{ui:Close}}. Opening a sketch or the drawing sheet closes it too.

:::tip
For a section on the drawing sheet (hatched, with its cutting-plane line), use {{ui:Drawing}}: see [Make a drawing (PDF)](topic:drawing-pdf).
:::
