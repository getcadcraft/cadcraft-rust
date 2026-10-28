---
title: Export a model for other programs
summary: Choose the right export for rendering, the web, 3D printing, BIM and BimCraft, and check what each one keeps.
section: tasks
order: 130
keywords: print, 3mf, pdf, paper, image, png, picture, screenshot, render, save view, export, share, render, rendering, gltf, glb, collada, dae, obj, stl, 3d print, ifc, bim, bimcraft, send, other software
---

Export makes a copy of the model in another format. Keep your `.modelcraft` file too: it is the only file that keeps everything, and the only one ModelCraft opens directly.

## Export

1. Save first ({{key:Mod+S}}), so your working file is safe.
2. Click {{ui:Export}} in the {{ui:File}} group.
3. Choose the format. The browser downloads it, named like the model.

The whole model is exported. To export one part, save, erase the rest, export, then undo or reopen.

## Which format

| You want to | Choose | Notes |
|---|---|---|
| Open it in another 3D modeler | {{ui:Collada (.dae)}} | Keeps groups, nested groups, components (shared, not copied), names and colors. Z up, meters. |
| Render it, or use it in a visualization program | {{ui:OBJ (.obj)}} or {{ui:glTF binary (.glb)}} | OBJ is the most widely read; it writes each group placement as a separate object, but has no colors and no shared components. glTF keeps components and colors. |
| Put it on the web, in a game engine or in AR | {{ui:glTF binary (.glb)}} | One file with everything in it. |
| Print it in 3D | {{ui:Check for 3D printing (STL or 3MF)…}} | Checks the model is closed and faces the right way, then writes STL or 3MF in millimeters. See [Get a model ready for 3D printing](topic:print-3d). {{ui:STL for printing (.stl, millimeters)}} writes the whole model without the check. |
| Open it in BimCraft or another BIM program | {{ui:IFC for BimCraft (.ifc)}} | Each top-level group or component becomes one building element. |

The status bar confirms each export, for example "Exported House.stl (millimeters)." or "Exported House.dae (Collada 1.4.1, Z up, meters; components stay shared)."

## Before you export

- **Group your objects.** Collada, glTF and IFC keep groups and components as separate objects. Loose geometry goes out as one lump; in IFC it becomes one element called "Loose geometry".
- **Name them.** Group and component names go into Collada and IFC, so a wall called "North wall" arrives as "North wall".
- **Check face direction.** Faces showing blue-grey on the outside will point inward in the export. Fix them with {{ui:Reverse face}} (see [Erase and clean up](topic:erase#a-face-facing-the-wrong-way)).
- **For printing,** make the parts closed solids: select each group and check that {{ui:Entity info}} says "Solid: Yes". Then scale the model to the printed size, or scale it in the printer software.

## Send a model to BimCraft

1. Make each building part a group or component and name it.
2. Choose {{ui:Export}}, then {{ui:IFC for BimCraft (.ifc)}}.
3. Open the downloaded `.ifc` file in BimCraft.

There is no direct link between the two apps yet. The IFC has one storey, and every element is a generic building element proxy with no type or properties: it carries shape and name only. Closed groups become solid shapes; open ones become surfaces.

## Lines without faces

Edges that are not part of any face (guide-like lines, an imported site plan) go into OBJ and Collada as lines. glTF and STL have no place for them and leave them out. Guides and scenes are not exported to any format.

## Save the view as an image

{{ui:Export}} › {{ui:Image of the view (.png)}} saves exactly what the 3D view shows, at its size on screen. {{ui:Image at twice the size (.png)}} saves it twice as wide and twice as tall (for print or a sharp slide), with dimensions and marks redrawn at that size rather than stretched.

For a picture that looks like daylight (sky, sun, soft shadows, glass), use {{ui:Export}} › {{ui:Render a picture…}} instead: see [Render a picture](topic:render).

For a clean picture, turn on [presentation view](topic:styles-shadows#presentation-view) ({{key:Shift+P}}) first: the axes, guides, section outlines and the selection are left out. Set up the camera, style and shadows the way you want them, or go to a [scene](topic:scenes); to save a moving film of your scenes, use {{ui:Export animation}} in the {{ui:Scenes}} panel.

## Print the view, or save it as a PDF

- {{ui:Export}} › {{ui:PDF of the view (.pdf)}} saves a one-page PDF: the view at twice its screen size, with its dimensions, on an A4 page (Letter when the model is in feet and inches), landscape, with a line under it giving the file name, the date and the unit.
- {{ui:Export}} › {{ui:Print the view…}} puts the same page in front of your browser's print dialog. Choose a printer, or "Save as PDF".

The page shows the view as it is: there is no scale on a perspective view. For a drawing to scale, switch {{ui:Perspective}} off and look from {{ui:Top}} or {{ui:Front}}, and add [dimensions](topic:dimensions) for the sizes that matter. [Presentation view](topic:styles-shadows#presentation-view) leaves out the axes, guides and the selection; dimensions stay.
