---
title: Find your way with the model tree
summary: Browse an IFC model by project, site, building, storey and space, with its elements grouped by kind; tick boxes to show and hide, click a row to pick, double-click to zoom.
section: tasks
order: 23
keywords: model tree, tree, structure, spatial structure, browser, outline, navigator, storey, level, floor, space, room, group, class, hide, show, pick, select, zoom
---

## What you'll do

See how a model is put together and get to any element without hunting for it in 3D: the building, its storeys, the spaces on each storey, and the elements in them, grouped by kind (walls, doors, slabs…).

## Steps

1. Open an IFC model. The {{ui:Model tree}} on the left opens down to the storeys.
2. Click the arrow beside a row to open or close it. A storey holds its spaces first, then its elements by kind with how many there are. An element made of parts, such as a stair, holds its parts.
3. Untick a row's box to hide everything in it; tick it to show it again. A box with a dash means only some of what is in it is shown.
4. Click a row's name to pick what it stands for: one element, or every element in a group, storey or space. {{key:Mod}}+click adds them to the pick, or takes them out.
5. Click the {{ui:i}} button beside a storey, space or building to see its own properties.
6. Double-click a row to zoom to it.

## What you should see

- When you pick an element in the 3D view, the tree opens down to it and marks it.
- Hidden elements are struck through. The tree's boxes and {{ui:Show}} ({{key:H}}, {{key:I}}, {{key:Shift+H}}) work together: see [Hide, isolate and see through elements](topic:hide-and-isolate).
- Elements the model does not place in any storey are in a group at the end, **Not in the spatial structure**.
- A long group shows 200 rows at a time; click **Show more** under it for the rest.

## Keys

With a row's name focused: {{key:Up}} and {{key:Down}} move, {{key:Right}} opens, {{key:Left}} closes or goes up, {{key:Space}} ticks the box, {{key:Enter}} picks.

## Common mistakes

- **A storey's box is ticked but nothing shows.** The {{ui:Storeys}} list below the tree turns whole storeys off on its own: tick it there too.
- **A row picks nothing.** Some rows, such as an empty space, have no elements that can be drawn; the status line says so.

## Next

- [Delete, cut, copy and paste building elements](topic:edit-elements)
- [Show and hide storeys, spaces and edges](topic:storeys-and-spaces)
