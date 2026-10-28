---
title: Render a picture of the model
summary: Make a daylight picture of the 3D view: sky, the sun at your site's latitude, soft shadows, glass windows, then save it as a PNG.
section: tasks
order: 32
keywords: render, rendering, path tracing, path traced, ray tracing, global illumination, reflections, refraction, high quality, photo, photoreal, realistic, picture, image, png, still, sky, sun, daylight, shadows, soft shadows, ambient occlusion, glass, windows, metal, materials, exposure, brightness, overcast, dusk, studio, presentation, visualization
---

{{ui:Render}} on the toolbar makes a picture of the 3D view that looks like daylight: a sky, the sun where the site's latitude puts it, soft shadows, light from the whole sky that darkens corners, and colors that can look like glass or metal. The model and the file are not changed.

## Render the view

1. Set up the 3D view the way you want the picture: orbit and zoom, hide storeys or elements you don't want, and set a [section](topic:section-plane) if the picture should show a cut.
2. Click {{ui:Render}} on the toolbar.
3. Choose the {{ui:Size}} and the {{ui:Quality}}, then click {{ui:Render}} in the dialog.

   The picture fills in as it renders and gets smoother with each pass. {{ui:Stop}} (or {{key:Esc}}) ends it early and keeps the picture so far.

4. Click {{ui:Save PNG}} to download it. It is named after the model's file.

:::tip
Start with {{ui:Draft}} at {{ui:The view's size}} to check the camera, then render {{ui:Good}} or {{ui:Best}} at a bigger size for the final picture.
:::

## What shows in the picture

What the 3D view shows solid: hidden and see-through elements, hidden storeys and hidden models are left out, and rooms show only when {{ui:Spaces}} is on. A section plane or section box cuts the picture as it cuts the view. Elements show in their own colors: coloring by property (the {{ui:Color}} tab) is not used.

## Settings

| Setting | What it does |
|---|---|
| {{ui:Size}} | The view's size, twice the view's size, Full HD, 4K or a square |
| {{ui:Mode}} | {{ui:Fast}}, or {{ui:High quality (path traced)}} (below) with its {{ui:Time limit}} |
| {{ui:Quality}} | How many passes (or, in high quality, samples) are averaged: {{ui:Draft}} 16, {{ui:Good}} 64, {{ui:Best}} 256. More passes give smoother shadows and edges and take longer |
| {{ui:Sky}} | {{ui:Clear sky}}, {{ui:Overcast}}, {{ui:Dusk}} or {{ui:White studio}} (a plain white background) |
| {{ui:Sun}} | June 21 at 15:00 at the latitude the file gives for its site (IfcSite), turned to the model's true north. Tick {{ui:Use another date and time for this picture}} for another time |
| {{ui:Brightness}} | Lighter or darker, in steps (stops) |
| {{ui:Ground under the model}} | A ground at the model's lowest point that takes the shadows and fades into the horizon |
| {{ui:Draw edges}} | Draws the elements' edges thinly over the picture |

When the sun is below the horizon at the chosen time it is night: the sky goes dark and the model's lamps light it.

## Lamps

Every light fixture in the model is a lamp in the picture: the fixtures in an IFC file (IfcLightFixture, or an element typed as a light fixture), including ones you place from the library. Each shines down from just under the fixture. When the file gives a fixture's wattage, its brightness follows from it; otherwise it is 800 lumens, a warm white.

The dialog lists the lamps under {{ui:Lamps}}: untick one to switch it off, or type its brightness in lumens. These changes are saved in the IFC (the fixture's `CadCraft_Light` property set, where you can also change them in Properties), and {{key:Ctrl+Z}} undoes them.

When a file says nothing about where its site is, the sun is placed at 40° north, and the dialog says so. The time is the site's clock time, worked out from its longitude, so it can be an hour off where clocks are set differently.

These settings are kept in this browser for the next picture. They are not saved in the IFC file.

## Glass and other materials

Under {{ui:Materials}}, every color on screen has a look: {{ui:Matte}}, {{ui:Satin}}, {{ui:Glossy}}, {{ui:Metal}}, {{ui:Polished metal}}, {{ui:Glass}} or {{ui:Frosted glass}}. Colors the IFC file makes see-through (window glazing, for example) start as {{ui:Glass}}.

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

The picture is made on your own computer, by its graphics card; nothing is uploaded. Time grows with the passes and the pixels: {{ui:Best}} has 16 times the passes of {{ui:Draft}}, and a 4K picture has about four times the pixels of Full HD. Large models take longer per pass.
