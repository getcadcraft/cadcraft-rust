---
title: Terrain: ground from contours, and sculpting it
summary: Make ground from contour lines or as a grid, then raise and lower it with a soft brush.
section: tasks
order: 55
keywords: terrain, ground, landscape, topography, contours, contour lines, site, slope, hill, mound, sandbox, smoove, sculpt, grade, TIN
---

Terrain is ground made of triangles, so it can follow any shape. It is a group: double-click it to edit it.

## From contour lines

1. Get the contour lines at their heights: draw them, or [import a site plan](topic:site-plan) and move each contour up to its level.
2. Select them (all the edges of all the contours).
3. {{ui:Edit}} › {{ui:More}} › {{ui:Terrain from selected contours}}.

ModelCraft joins the contour points with triangles (a TIN), and softens every edge inside so the ground shades smooth. The lines themselves stay; erase or hide them when you no longer need them. Very dense contours are thinned to 8 000 points.

## A grid to sculpt

{{ui:Edit}} › {{ui:More}} › {{ui:Terrain grid…}} asks for a width, a depth and a cell size, then places a flat grid of triangles where you click its corner. Small cells sculpt smoother but make more faces (at most 200 × 200 cells).

## Sculpt: raise and lower the ground

1. Double-click the terrain to open it.
2. {{ui:Edit}} › {{ui:More}} › {{ui:Sculpt terrain}}. A circle shows the brush on the ground under the pointer.
3. Drag up to raise the ground under the brush, or down to lower it. The middle moves most and the ground eases to nothing at the circle's edge, so mounds and hollows come out smooth.

To make a mound an exact height, click once, then type the height, such as `1.5m` (a minus lowers it), and press {{key:Enter}}. To change the brush, type a length before you click: `5m` gives a brush 5 m across its radius.

Each sculpt is one undo step.

## Things to know

- Sculpt moves only triangle faces, so every face stays flat. Faces drawn by hand (rectangles, polygons) are not terrain: the tool says so.
- Sculpting moves points straight up and down; it does not add points. For finer detail, make a grid with smaller cells.
- Terrain exports like any other group. In IFC it is a building element proxy, like everything else.
