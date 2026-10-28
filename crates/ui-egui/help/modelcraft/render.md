---
title: Render a picture of the model
summary: Make a picture that looks like daylight: sky, sun from your model's place and date, soft shadows, glass and metal looks, then save it as a PNG.
section: tasks
order: 112
keywords: render, rendering, path tracing, path traced, ray tracing, global illumination, reflections, refraction, high quality, photo, photoreal, realistic, picture, image, png, still, sky, sun, daylight, shadows, soft shadows, ambient occlusion, glass, metal, glossy, materials, exposure, brightness, overcast, dusk, studio, presentation
---

{{ui:Render a picture…}} makes a picture of the view that looks like daylight rather than a drawing: a sky, the sun where your model's place and date put it, soft shadows, light from the whole sky that darkens corners and gaps, and colors that can look like glass, metal or gloss paint. The model is not changed.

## Render the view

1. Set up the view the way you want the picture: orbit, zoom, pick a [scene](topic:scenes), hide what should not show. The picture uses the view's own camera.
2. Click {{ui:Export}} in the {{ui:File}} group, then {{ui:Render a picture…}}.
3. Choose the {{ui:Size}} and the {{ui:Quality}}, then click {{ui:Render}}.

   The picture fills in as it renders and gets smoother with each pass. {{ui:Stop}} (or {{key:Esc}}) ends it early and keeps the picture so far.

4. Click {{ui:Save PNG}} to download it.

:::tip
Start with {{ui:Draft}} at {{ui:The view's size}} to check the camera and the look, then render {{ui:Good}} or {{ui:Best}} at a bigger size for the final picture.
:::

## Settings

| Setting | What it does |
|---|---|
| {{ui:Size}} | The view's size, twice the view's size, Full HD, 4K or a square. Fixed sizes keep the view's camera and show more or less at the sides |
| {{ui:Mode}} | {{ui:Fast}}, or {{ui:High quality (path traced)}} (below) with its {{ui:Time limit}} |
| {{ui:Quality}} | How many passes (or, in high quality, samples) are averaged: {{ui:Draft}} 16, {{ui:Good}} 64, {{ui:Best}} 256. More passes give smoother shadows and edges and take longer |
| {{ui:Sky}} | {{ui:Clear sky}}, {{ui:Overcast}} (soft light, faint shadows), {{ui:Dusk}} (low, warm light) or {{ui:White studio}} (a plain white background) |
| {{ui:Sun}} | The sun comes from the date, time and place in {{ui:Style}} › sun. Tick {{ui:Use another date and time for this picture}} to try another time without changing the model's style |
| {{ui:Brightness}} | Lighter or darker, in steps (stops) |
| {{ui:Ground under the model}} | A ground that takes the shadows and fades into the horizon. Turn it off for a model that floats |
| {{ui:Draw edges}} | Draws the model's edges thinly over the picture, for a drawn look |

When the sun is below the horizon at the chosen time it is night: the sky goes dark and the model's [lamps](topic:lights) light it. The dialog says so.

The settings are saved in the `.modelcraft` file, so the next picture of the same model starts where you left off.

## Make colors look like glass or metal

Under {{ui:Materials}}, every color on screen has a look: {{ui:Matte}}, {{ui:Satin}}, {{ui:Glossy}}, {{ui:Metal}}, {{ui:Polished metal}}, {{ui:Glass}} or {{ui:Frosted glass}}. {{ui:Unpainted faces}} are the faces with no color of their own.

To make windows glass, [paint](topic:paint) the window panes one color, then set that color to {{ui:Glass}}. Glass lets the sky and what is behind it show through, tinted by its color, and reflects the sky.

:::note
In the {{ui:Fast}} mode, glass and metal reflect the sky, not the rest of the model, and light does not bend through glass. The {{ui:High quality (path traced)}} mode below does both.
:::

## High quality

Choose {{ui:Mode}} › {{ui:High quality (path traced)}} for a picture that follows light the way a camera sees it:

- light bounces between surfaces, so a room lit through a window, or by a lamp, is lit all round and colors bleed onto their neighbors;
- glass is real glass: light bends through it and it reflects the model;
- metal and polished surfaces reflect the rest of the model;
- light panels light as panels, with soft shadows.

It is much slower than {{ui:Fast}}: each pass traces every pixel's light again. The picture starts grainy and clears as samples add up, so start small ({{ui:The view's size}}, {{ui:Draft}}) and choose a {{ui:Time limit}}: when it runs out, the picture so far is kept. {{ui:Stop}} keeps it too. The first pass waits while the browser prepares the path tracer, a few seconds on a graphics card, longer without one.

In this mode edges are not drawn, textures show their color and roughness but not their bumps, and a section cut is made on the model before the picture starts.

## How long it takes

The picture is made on your own computer, by its graphics card; nothing is uploaded. How long it takes depends on that card. Time grows with the passes and the pixels: {{ui:Best}} has 16 times the passes of {{ui:Draft}}, and a 4K picture has about four times the pixels of Full HD. A computer without a graphics card still renders, more slowly.

:::note
If the dialog says the browser can't render pictures, the browser has no WebGL 2. Update it, or turn on hardware acceleration in its settings.
:::
