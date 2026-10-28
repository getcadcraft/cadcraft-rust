---
title: Start a new building
summary: Make a blank building model with its levels, draw its walls, place items from the library, and save it as IFC.
section: start
order: 15
keywords: new, new building, blank, empty, start from scratch, template, levels, storeys, floors, create, model
---

## What you'll do

Start a building model from nothing instead of opening one. BimCraft makes a blank IFC model with a project, a site, the building and its levels. You can place equipment and furniture on the levels from the library and save it as an IFC file.

:::note
Walls can be drawn now: see [Draw walls](topic:draw-walls). Doors, windows, floors and roofs are being built next.
:::

## Steps

1. Click {{ui:New building}} under **Open a file or folder**, or on the start page.
2. Type the **Building name**.
3. Choose how many **Levels** it has (1 to 200).
4. Type the **Level height**, floor to floor: for example `10'-0"`, `10'6"` or `3000 mm`. A plain number is feet in the US, millimeters elsewhere.
5. Click {{ui:Create}}. The levels are stacked from 0, each one level height above the last, and named Level 1, Level 2 and so on. They show in the model tree, and the plan of Level 1 opens beside the 3D view with {{ui:Draw}} on.
6. Draw the walls: see [Draw walls](topic:draw-walls). Rename levels or change their heights in the **Levels** panel.
7. Place items: see [Place equipment and furniture from the library](topic:place-from-library). The placing bar lists every level.
8. Click {{ui:Save as IFC}}. The file is named after the building, for example `Smith's house.ifc`.

## Good to know

- The model is saved in millimeters, as IFC4. Other BIM programs open it.
- **Storeys** in the left panel lists only levels with something on them; the model tree lists them all.
- Until you save, the new building counts as unsaved work. BimCraft asks before you open another file or leave, and keeps a copy in this browser in case the tab closes.
