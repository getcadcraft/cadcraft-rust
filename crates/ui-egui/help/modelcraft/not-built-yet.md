---
title: Not built yet
summary: What this build of ModelCraft does not do yet, and what to do instead today.
section: trouble
order: 20
keywords: missing, not yet, limitations, roadmap, materials, textures, text, layers, trim, split, cloud, desktop
---

ModelCraft is an early build. This page lists what it does not do yet, so you don't go looking for it, and the nearest way to get the job done today.

## Drawing tools

| Not yet | Instead, today |
|---|---|
| Freehand drawing | Click the points with {{ui:Curve}} |
| Changing an arc's or circle's number of segments after drawing it | Erase it and draw it again after typing `24s` |
| Choosing a typeface for [3D text](topic:text3d), or editing its words after placing it | Delete it and add the text again |
| Changing an angle by typing into an angle dimension, or a cylinder's size from its circle's dimension | Rotate the edges with {{ui:Rotate}}; scale the cylinder with {{ui:Scale}} |
| Dimensions in exported model files | Dimensions show in images, prints and PDFs of the view; model files carry geometry only |
| Trim, Split and Outer shell | [Union, Subtract and Intersect](topic:solids) closed groups, or [intersect faces](topic:intersect) and erase what you do not need |
| Choosing the rotation axis with the arrow keys | Click the {{ui:Rotate}} center on a face that faces the way the axis should point |
| Locks on more than one inference at a time | Lock one direction with an arrow key, then snap to a point |
| Resizing along a slant, or from a dimension end that is not on a corner | Place the dimension on two corners, along an edge or along red, green or blue |

## Organizing and viewing

| Not yet | Instead, today |
|---|---|
| Layers of DWG and DXF site plans as tags | The site plan comes in as one group: explode it and group the parts you want to tag |
| Several section cuts at once, a section box, or a section's outline exported as a drawing | One [section plane](topic:sections) cuts at a time; export the view as an image |
| Showing soft edges dashed, or softening while you drag a slider | [Soften by angle](topic:soften) in Entity info, and undo to try another angle |
| An online library of models to search, or components with options to pick (colour, style) | The [starter library](topic:starter); import Collada, OBJ or glTF models; [stretch](topic:stretch) a copy to size |
| Independent copies of a group | Make it a component, then {{ui:Make unique}} (see [Troubleshooting](topic:troubleshooting#editing-one-group-changed-another)) |

## Materials and colors

| Not yet | Instead, today |
|---|---|
| Your own image textures, or changing a library material's size or color | [Paint with the material library](topic:paint#paint-with-a-material) or a solid color |
| Textures in exported files | Exports get each material's color and name |
| Textures in Collada files | They are counted and left out on import; textures in `.skp` and `.glb` files come in |

## Files

| Not yet | Instead, today |
|---|---|
| Saving SketchUp files, and text and dimensions from them | [Import](topic:import-sketchup-files) keeps faces, colors and textures; export Collada (`.dae`) to go back to SketchUp |
| Dimensions and guides in DWG and DXF exports | Add them in the CAD program after exporting |
| Points, tables, multileaders, regions and 3D solids from DWG and DXF | They are counted and left out; text comes in as single-stroke lettering, not the drawing's font |
| 3D solids, surfaces, regions, meshes, tables and multileaders from DWG and DXF | They are counted and left out too. Export the 3D parts from the CAD program as OBJ or Collada and import that |
| IFC types, properties or materials | The IFC export has [storeys](topic:formats#ifc-ifc) and generic elements; add the rest in a BIM program |
| Sending a model straight to BimCraft | Export {{ui:IFC for BimCraft (.ifc)}} and open it in BimCraft |
| Loose edges in glTF and STL | Use OBJ or Collada for line work |
| glTF extensions, sparse data, separate `.bin` files | Export a single `.glb` from the other program |
| Collada skins, morphs, cameras, lights, look-at and skew transforms, external references | Left out on import and counted in the message |
| Save As with a name, backup files | Rename the downloaded file. After a crash, ModelCraft offers your unsaved work back ("Recover your work?") |
| Cloud saving and sync between computers | Not available yet. Save files to your computer, and copy them where you need them |

## Performance

Undo keeps a full copy of the model for each step, so very large imports make every edit slower. Keep imports lean, and save and reopen to clear the history.

## Apps and platforms

- **Desktop:** ModelCraft runs in the browser on Mac, Windows and Linux, and in CadCraft Studio, the desktop app with all eight apps in one install. iPad, Android and iPhone come with 0.6.
- **Touch:** see [Orbit, pan and zoom](topic:orbit-pan-zoom) for the finger gestures. Pressure and pen tilt are not used.
- **Sheets:** ModelCraft [prints one view per page](topic:export-model#print-the-view-or-save-it-as-a-pdf); there are no sheet layouts with several views or a title block. Export to CadCraft-friendly DWG/DXF for drawings to scale.
