---
title: Foundations under the walls
summary: Put a slab on grade, stem walls with a slab, a crawlspace or a basement under a building with one click; footings are sized from the walls and the number of stories, and the foundation follows when the walls move.
section: tasks
order: 21
keywords: foundation, footing, slab on grade, thickened edge, monolithic slab, stem wall, crawlspace, crawl space, basement, basement wall, frost, concrete, footing width
---

## What you'll do

Give the building a foundation that matches its walls. BimCraft follows the outside of the walls, sizes the footings from the wall type and how many stories stand on them, and makes the foundation again whenever the walls change.

## Put a foundation in

1. On the level whose walls stand on it (usually the first), open {{ui:Build}} and choose {{ui:Foundation}}.
2. Choose the kind:
   - **Slab on grade**: a 4" concrete slab whose top is the floor, with a thickened edge under the walls.
   - **Stem walls and slab**: foundation walls up to the floor on footings, with a slab inside them.
   - **Crawlspace**: foundation walls that stop under the floor framing, on footings, with no slab.
   - **Basement**: full-height foundation walls on footings with a basement slab, under the floor framing.
3. Point inside the building: the footings show dashed with their size. Click.

Clicking a building that already has a foundation picks it, so you can change it instead of adding a second one.

## Change it

Pick the foundation (click inside the building with {{ui:Foundation}}, or click any of its parts in the 3D view or the model tree) and change it in **Properties**: the kind, the footing width and depth, how far down the footing's underside is, the wall thickness, how far below the floor the walls stop, the basement height and the slab thickness. Each change is one undo step.

Sizes marked **auto** come from the walls: footings 12", 15" or 23" wide for one, two or three stories on a slab or stem walls, 18", 22" or 27" over a crawlspace or basement, and 4" wider under masonry or concrete walls. Type a size to set your own; type `0` to go back to auto.

:::warning
The automatic sizes are labelled defaults for typical light-frame houses, not engineering. A designer or engineer checks footings, depths and walls for the soil, the frost depth and the loads.
:::

## What's saved

A foundation is saved as an IFC element assembly ("Foundation") made of a footing (or a slab's thickened edge), a foundation wall and a slab as its kind needs, all concrete, each with its quantities: lengths, widths, heights, areas and volumes, ready for takeoffs.

## Good to know

- Move, add or delete walls and the foundation under them is made again; draw walls on the level above and the footings below widen for the extra story.
- {{key:Delete}} removes a picked foundation with all its parts.
- With the site's ground in the model (see [The site's ground, the building pad and paving](topic:terrain)), stem walls and crawlspace footings step down with it in 8" steps.
- Footings under interior walls are not made yet.
