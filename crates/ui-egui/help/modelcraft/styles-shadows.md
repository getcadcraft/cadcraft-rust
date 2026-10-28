---
title: Styles and sun shadows
summary: Show the model shaded, as hidden lines, as a wireframe or see-through, and study the shadows it casts at any date, time and place.
section: tasks
order: 110
keywords: presentation view, clean view, hide guides, hide axes, style, shaded, hidden line, wireframe, x-ray, edges, shadows, sun, sun study, solar, date, time, latitude, longitude, time zone, north
---

The {{ui:Style}} panel on the right changes how the model is drawn. Nothing in it changes the model itself, and it is not part of undo. Each [scene](topic:scenes) remembers its own style, and the file remembers the style you had when you saved.

## Faces

Choose one in the {{ui:Faces}} list:

| Style | Looks like |
|---|---|
| {{ui:Shaded}} | Solid faces, lit softly. Faces seen from behind are blue-grey. Imported colors show. |
| {{ui:Hidden line}} | Faces in the background color with edges in ink: a clean line drawing with hidden lines removed. White on {{ui:Paper}}, dark on {{ui:Model}}. |
| {{ui:Wireframe}} | Edges only. Faces are not drawn, but they still count when you click and snap: you cannot pick through them. |
| {{ui:X-ray}} | See-through faces, to see what is behind or inside. As in wireframe, clicks and snaps still stop at the nearest face. |

{{ui:Show edges}} turns the edges on or off in every style except {{ui:Wireframe}}. With it off, {{ui:Hidden line}} shows almost nothing, since its faces match the background. Soft edges, such as the sides of a pulled circle, are never drawn.

## Turn on sun shadows

1. Tick {{ui:Sun shadows}}. The sun settings below it become active, and the model casts shadows on itself and on the ground.
2. Set the place:
   - {{ui:Latitude}}: north is positive, south negative (New York is about 40.7, Sydney about -33.9).
   - {{ui:Longitude}}: east is positive, west negative (New York is about -74.0).
   - {{ui:UTC offset}}: the site's offset from UTC in hours, such as `-5` for New York in winter. ModelCraft does not apply daylight saving time; use `-4` for New York in summer.
3. Set the {{ui:Date}} (month and day) and drag the {{ui:Time}} slider. The time moves in 15-minute steps and shows next to the slider, such as "3:00 pm".

A line under the settings says where the sun is, such as "Sun 52° up, 250° from north. North is the green axis." At night it says "The sun is down at this time."

A new model starts with the sun on June 21 at 3:00 pm, at latitude 40, longitude -105, UTC offset -7.

## Point the model north

ModelCraft's north is always the **green axis**. To study a real site, turn the model so its north side faces along green: select everything ({{key:Mod+A}}), press {{key:Q}}, click a center on the ground, and turn it by the site's angle from north (see [Rotate and scale](topic:rotate-scale)).

## A sun study

1. Set up the place and the date.
2. Add a scene ({{ui:Add scene}} in {{ui:Scenes}}).
3. Move the time slider, then add another scene. Repeat for each hour you want.
4. Click {{ui:Play}} to step through them. See [Save views as scenes](topic:scenes).

## Things to know

- Shadows can look jagged on a very large model, since one shadow map covers the whole model.
- Shadows are not drawn in {{ui:Wireframe}}.
- The sun position follows the NOAA solar calculation and is accurate to a fraction of a degree, which is plenty for massing studies. Atmospheric refraction is ignored, so very low sun angles near sunrise and sunset can be off slightly.

## Presentation view

{{ui:Views}} › {{ui:Presentation view}} (or {{key:Shift+P}}) shows only the model: the axes, guide lines and guide points, section planes' outlines (the cut stays), the box of an open group, the snapping marks and tips, and the selection highlight all go. Use it to look at the model clearly, or before a screenshot. Tools still work, and still snap; you just do not see the marks. Press {{key:Shift+P}} again to bring them back. Dimensions stay, as they are part of the drawing.
