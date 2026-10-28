---
title: Split the screen
summary: Show the drawing in two, three or four panes with their own zoom, put a sheet beside the model, and open several drawings side by side.
section: tasks
order: 46
keywords: split, split screen, panes, vports, viewports, tiled, tile, side by side, stacked, two drawings, tabs, document tabs, compare, divider, model and sheet, several drawings, open drawings
---

## What you will learn

How to look at one drawing in several panes at once, each with its own zoom, and how to open several drawings and see them side by side.

## Split one drawing into panes

1. Open a drawing, or draw a few lines and circles.
2. Open {{ui:View}} › {{ui:More}} and choose {{ui:Split side by side}} (or type {{cmd:VPORTS}}, then `2`, then {{key:Enter}}).

The drawing area splits into two panes, both showing the same view. The active pane has a blue frame and gets the crosshair.

3. Click in the other pane. It becomes active. (That first click only switches panes; click again to pick.)
4. Zoom with the wheel. Only that pane zooms.
5. Drag the line between the panes to make one wider. {{key:Home}} on a focused divider puts it back in the middle.

{{cmd:VPORTS}} also takes `3` (two panes on one side and a big one on the other: `L` or `R`), `4`, and `SI` for one pane again. The active pane's view then fills the area. On a sheet, {{cmd:VPORTS}} adds a viewport to the sheet instead (as AutoCAD); type {{cmd:SPLIT}} there, or use the View menu.

## Draw from one pane into another

1. Split side by side, and zoom each pane onto a different corner of the drawing.
2. Type `L` and press {{key:Enter}}, and click the first point in the left pane.
3. Click once in the right pane to make it active, then click the second point there.

The line runs between the two places. A command goes on as you change panes, and the rubber band shows in every pane.

## The model beside a sheet

1. Split side by side.
2. Click the right pane, then click a sheet's tab under the drawing.

The right pane shows the sheet; the left one still shows the model. Each pane keeps its own space, so you can draw on the model and watch the sheet's viewport follow.

## Several drawings in one window

1. Open a drawing. Then open another (or press {{ui:New}}).

Each drawing opens in its own tab over the drawing area. Click a tab to switch; the command line, the panels and undo follow the drawing you switch to. {{key:Mod+C}} in one drawing and {{key:Mod+V}} in another copies objects between them.

2. Click {{ui:Tile the open drawings side by side}} (the split icon at the end of the tabs), or the stacked icon beside it.

Each pane now shows one of the open drawings, up to four. Click a pane to work in that drawing.

3. Click the {{ui:×}} on a tab to close that drawing. If it has unsaved changes, CadCraft asks first.

:::tip
Opening a drawing while the window holds only a new, untouched drawing uses that tab instead of adding one.
:::

## Pan and zoom together

To compare two revisions, tile them side by side, then click {{ui:Link the panes' views}} (the link icon at the end of the tabs), or choose it in {{ui:View}} › {{ui:More}}, or type {{cmd:LINKVIEWS}} and `ON`. Now a pan or zoom in one pane moves every pane showing the model (or every pane showing a sheet) to the same place, so the two drawings stay lined up. {{cmd:LINKVIEWS}} `OFF` lets each pane move on its own again.

## Common mistakes

- **My click did nothing.** It made that pane active. Click again to pick or draw.
- **The wheel zoomed the wrong pane.** The wheel zooms the pane under the pointer and makes it active.
- **The panes went back to one.** Opening a drawing shows it as it was saved; a drawing saved without panes opens in one.
- **Leaving the page warns about unsaved changes I can't see.** Another tab holds a drawing with changes. Switch to it and save, or leave without saving.
