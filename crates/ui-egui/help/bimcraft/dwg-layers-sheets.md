---
title: Work with layers and sheets
summary: Turn drawing layers on and off, switch between model space and sheets, and understand how sheets show the model through their viewports.
section: tasks
order: 110
keywords: layers, layer on off, freeze, thaw, sheets, layouts, paper space, tabs, viewports, layer manager, la
---

## What you'll do

Clean up the view of a busy drawing by turning layers off, then look at the drawing's sheets one by one.

## Steps: layers

1. Open a DWG or DXF drawing.
2. Find the {{ui:Layers}} list on the left. Each layer has a checkbox, a color swatch and its name.
3. Untick a layer to hide everything on it.
4. Click {{ui:All off}} to hide every layer.
5. Tick only the layers you want to see.
6. Click {{ui:All on}} to show everything again.

## What you should see

Layers disappear and reappear at once, on {{ui:Model}} and on every sheet. Layers that are off or frozen in the drawing itself start unticked.

## Steps: sheets

1. Look at the row of tabs above the drawing: {{ui:Model}}, then each sheet by name.
2. Click a sheet tab.
3. Look at the sheet: the paper sits on a darker desk, and each viewport shows its part of the model at its own scale and angle.
4. Zoom into a viewport to read detail. You are zooming the sheet, not the viewport, so the whole sheet moves together.
5. Click {{ui:Fit}} or double-click to see the whole sheet again.

## How sheets work here

A sheet (a layout, or paper space) holds a title block and viewports that look into model space. BimCraft draws each viewport clipped to its edge, at its own scale and twist, and hides the layers frozen in that viewport. Each sheet's first viewport is the sheet itself and is not drawn twice.

## Common mistakes

- **Expecting layer changes to be saved.** Layer settings only affect the view, until you open another file.
- **Expecting per-viewport layer control.** The {{ui:Layers}} list switches a layer everywhere. Layers frozen in one viewport by the drawing stay frozen there.
- **Looking for a layer search or layer states.** Not built yet. The list is in the drawing's order.
- **Expecting to work inside a viewport.** There is nothing to activate: BimCraft only shows the sheet. Use {{ui:Model}} to look at model space directly.
