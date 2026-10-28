---
title: Show and hide storeys, spaces and edges
summary: Turn storeys off to see inside a building, show rooms and spaces, and switch the edge lines on and off.
section: tasks
order: 20
keywords: storeys, levels, floors, hide, isolate, rooms, spaces, ifcspace, edges, visibility, section
---

## What you'll do

Look inside an IFC model by turning off the storeys above the one you care about, then show the rooms and spaces, and switch the edge lines off for a cleaner look.

## Steps: look at one floor

1. Open an IFC model. See [Open an IFC model and look around in 3D](topic:open-ifc-model).
2. Find the {{ui:Storeys}} list on the left. The top storey is first, like reading a section from the roof down.
3. Untick the roof and every storey above the floor you want.
4. Click {{ui:Top}} to look down onto that floor.
5. Tick the storeys again to put them back.

## What you should see

Each storey you untick disappears at once, with everything on it. The list only shows storeys that hold something visible. Elements that are not assigned to any storey are grouped as **Not on a storey** at the bottom; site elements and some furniture often land there.

## Steps: show rooms and spaces

1. Click {{ui:Spaces}} in the toolbar above the model. The button stays pressed.
2. Look at the model: rooms and spaces appear as pale, see-through volumes.
3. Click a space to read its properties, such as its name, number and area, on the right.
4. Click {{ui:Spaces}} again to hide them.

Spaces are hidden when a model opens because they would otherwise cover the walls and floors.

## Steps: edges

1. Click {{ui:Edges}} to turn the thin edge lines off.
2. Click it again to turn them back on.

On very large models (over 1.5 million triangles) the button is greyed out with the tip "Edges are off for very large models". This saves memory.

## Common mistakes

- **Unticking a storey and still seeing parts of it.** An element belongs to the storey the exporting program assigned it to. A tall wall or a stair often belongs only to its base storey, so it stays when the storey above is hidden, or vanishes with the storey below.
- **Hiding storeys to see inside.** A section box is often quicker: see [Cut an IFC model with a section plane](topic:section-plane#a-section-box). You can also untick rows in the {{ui:Model tree}} (see [Find your way with the model tree](topic:model-tree)).
- **Clicking a space through a wall.** Clicking picks the nearest visible surface. Hide the storey above, or switch to {{ui:Top}}, then click.
- **Storey names look odd.** BimCraft shows the storey names saved in the file. If they say "Level 1" or a number, that is what the exporter wrote.
