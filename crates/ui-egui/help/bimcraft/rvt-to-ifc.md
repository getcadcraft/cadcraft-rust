---
title: From a project file to BimCraft through IFC
summary: The IFC export settings that open best in BimCraft (version, property sets, base quantities, rooms, linked models, coordinates), how to open the result, and what the export keeps.
section: tasks
order: 70
keywords: export, ifc export, rvt to ifc, convert, 3d, geometry, ifc2x3, ifc4, coordination view, reference view, design transfer view, property sets, base quantities, rooms, spaces, linked files, shared coordinates, survey point, project base point, settings
---

## What you'll do

Turn a project file into an IFC file you can orbit, inspect, check and edit in BimCraft. BimCraft does not read the 3D model inside an `.rvt` file yet, so the export happens in the program that made the file (Revit); BimCraft opens the result.

## Steps

1. Open the project in the program that made it. BimCraft's card names the release it was saved in.
2. Open a 3D view that shows what you want to export.
3. Choose **File > Export > IFC**.
4. In the export dialog choose a setup (or **Modify setup…**) with the settings below.
5. Choose a folder and file name, and export.
6. In BimCraft, click {{ui:Choose a file}} and choose the new `.ifc` file. Linked models exported as their own files open into the same scene with {{ui:Add a model}}.

## The settings that open best here

| Setting in the exporter | Choose | Why |
|---|---|---|
| IFC version | **IFC4 Reference View** for looking, checking and taking off; **IFC2X3 Coordination View 2.0** when others need IFC2X3 | Both open fully. **IFC4 Design Transfer View** opens too, with more exact shapes, but the file is larger and slower. |
| File type | **IFC** (plain) | BimCraft does not open `.ifczip` or `.ifcxml`. |
| Phase / visible elements | The phase you mean, and **Export only elements visible in view** when you set up the 3D view for it | Leaves out what you hid on purpose. |
| Property sets | **Export Revit property sets** and **Export IFC common property sets** | They become the property sets BimCraft shows in {{ui:Properties}}, and that {{ui:Find}}, {{ui:Color}} and {{ui:Model checks}} use. |
| Base quantities | **Export base quantities** | {{ui:Quantity totals}} and schedules use them (lengths, areas, volumes) instead of guessing. |
| Rooms and areas | **Export rooms in 3D views** (or the Rooms / Spaces option) | Rooms arrive as spaces: click {{ui:Spaces}} to see them. |
| Linked files | **Export linked files as separate IFCs** | Each link becomes its own file; open them together with {{ui:Add a model}} and they line up. |
| Coordinate base | **Shared coordinates** (or the survey point) | The file then carries its place on the map (IfcMapConversion), so Measure › Location, map layers on the ground and point clouds placed by map coordinates work. The project base point keeps the model near the origin but without its map place. |
| Split walls and columns by level | On, when you take off quantities per storey | Each storey's part is its own element. |

## What you should see

The model in 3D, with storeys taken from the project's levels and the project information in the {{ui:File}} panel. **Exported from** names the exporter.

## What the export keeps, and what it does not

- **Kept**: geometry, storeys, element kinds, names, types, most parameters (as property sets), base quantities when asked for, rooms as spaces, and the map place with shared coordinates.
- **Not kept**: views, sheets, annotation, families as editable families, worksets and design options. An IFC file is a snapshot of the model's elements, not the project. For the sheets, export DWG and open it in the 2D view.

## Common mistakes

- **Parameters missing in BimCraft.** Turn on the property set options above and export again.
- **Rooms missing.** Turn on the rooms option, then click {{ui:Spaces}}.
- **Saving as `.ifczip`.** Choose plain IFC, or unzip the `.ifczip`.
- **No map place.** Export with shared coordinates; check that the project's survey point is set.
- **Expecting edits to flow back.** Edits you save in BimCraft go into a new IFC file, not into the project file.
