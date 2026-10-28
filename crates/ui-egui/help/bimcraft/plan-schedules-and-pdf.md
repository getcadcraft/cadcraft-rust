---
title: Schedules, dimensions and a PDF of the plan
summary: Door, window, wall and room schedules that change as you draw, dimensions on the plan, and the plan of a level saved as a PDF at a standard scale.
section: tasks
order: 20
keywords: schedule, door schedule, window schedule, wall schedule, room schedule, quantities, takeoff, area, csv, spreadsheet, dimensions, dimension, print, pdf, plot, scale, 1/4", 1:50, sheet
---

## What you'll do

See what the building you drew adds up to, while you draw it: how many doors and windows of each size, how much wall of each type, how big each room is. Put dimensions on the plan and save it as a PDF to print or send.

## Schedules

With {{ui:Draw}} on, **Schedules** in the left panel shows one table at a time:

- **Doors** and **Windows**: each one by its mark (D1, D2 … and W1 …, given as you place them), its type, width, height (and sill), level and the wall it is in.
- **Walls by type**: for each wall type, how many walls, their thickness, their total length and their net area (doors and windows left out), with a total.
- **Rooms**: number, name, level, area and perimeter, with the total area.

The tables change with every edit. Click a row to pick what it lists. **Download CSV** saves the table, to open in a spreadsheet.

Marks are the elements' IFC tag: change one in **Properties** (Tag) and the schedule follows. For every IFC model, drawn or not, [Add up quantities, list elements and schedule spaces](topic:quantity-totals) adds up any property.

## Dimensions

The plan shows each drawn wall's length along its outside face (an inside wall: along one face) and, for a wall with doors or windows, a chain from the corners to each opening. {{ui:Dimensions}} in the model bar hides them and shows them again.

## Save the plan as a PDF

1. Show the level you want and set the units in the **Levels** panel.
2. Click {{ui:Print}} in the model bar.
3. BimCraft saves "<file> — <level>.pdf": the plan in black and white at the largest standard scale that fits the page (1/4" = 1'-0", 1/8" = 1'-0" … on Letter in feet and inches; 1:50, 1:100 … on A4 in millimeters), with the walls filled, doors, windows, room names and areas, dimensions, and a title strip with the level, the file, the cut height, the date and the scale.

The PDF is drawn as lines and text, so it stays sharp at any zoom and its text can be searched.

## Good to know

- The schedules list what the drawing tools made. Doors and windows from another program show in Quantity totals instead.
- To price the job, hand the quantities to TakeoffCraft: see [Quantities to TakeoffCraft](topic:takeoff). The CSV download stays for a spreadsheet.
