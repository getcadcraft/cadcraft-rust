---
title: Light a model with lamps
summary: Put lamps, spotlights and light panels in a model so rendered pictures can show rooms at night or lit interiors, with soft shadows.
section: tasks
order: 113
keywords: light, lights, lamp, lamps, lighting, bulb, ceiling light, spotlight, downlight, light panel, area light, lumens, brightness, color temperature, warm, night, interior, render
---

Lamps light [rendered pictures](topic:render). They don't change the 3D view, where each lamp shows as a small mark in its own color.

## Add a lamp

1. In the {{ui:Lights}} section on the right, click {{ui:Lamp}}, {{ui:Spotlight}} or {{ui:Light panel}}.
2. Click where it goes. Snapping works as when drawing.
   - On a ceiling or wall a **lamp** hangs 25 cm off the face, on the side you are looking from; a **light panel** lies on the face and shines away from it.
   - A **spotlight** takes two clicks: where it is, then what it points at.

The new lamp is selected in {{ui:Lights}}, ready to change.

## Change a lamp

Click a lamp's name in {{ui:Lights}}, or its mark in the view with {{ui:Select}}. Then:

| Setting | What it does |
|---|---|
| {{ui:Brightness (lumens)}} | How much light it gives. A household bulb is about 800 lumens; a bright ceiling panel 3000 |
| {{ui:Color}} | {{ui:Warm}} (a home bulb), {{ui:Neutral}} or {{ui:Daylight}}, or any color |
| {{ui:Beam (degrees)}} | A spotlight's cone, edge to edge |
| {{ui:Bulb size}}, {{ui:Width}}, {{ui:Depth}} | Bigger bulbs and panels give softer shadows |
| {{ui:Red (x)}}, {{ui:Green (y)}}, {{ui:Height}} | Where it is |

The tick beside a lamp's name switches it on or off. {{ui:Delete the light}} removes it. Each change is one step: {{key:Ctrl+Z}} undoes it. Lamps are saved in the `.modelcraft` file.

## Pictures at night

In {{ui:Render a picture…}}, tick {{ui:Use another date and time for this picture}} and pick an evening hour. With the sun down the sky goes dark and only the lamps light the model. Raise {{ui:Brightness}} in the dialog if the picture is too dark, or add lamps.

:::tip
For a room seen from inside, put the camera inside the room (orbit and zoom in, or a [scene](topic:scenes)), add a lamp on the ceiling and render at night. In the {{ui:Fast}} mode corners away from a lamp stay darker than in a photo; the [high-quality mode](topic:render#high-quality) lets the light bounce off the walls.
:::
