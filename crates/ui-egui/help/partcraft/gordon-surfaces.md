---
title: Surfaces through a curve network (Gordon surface)
summary: Make one smooth surface through a grid of profile and guide curves drawn in sketches or taken from model edges, close it to a solid when the profiles are closed, and what is exact, what is fitted, and the messages when a network does not fit.
section: tasks
order: 93
keywords: tangent, g1, tangency, tangent continuity, mirror plane, gordon surface, curve network, boundary surface, net surface, network surface, profiles, guides, guide curves, loft with guides, guide rails, skin, transfinite, coons, b-spline surface, freeform surface
---

:::note
**Not built yet in this version.** The Gordon surface needs an exact geometry kernel, which this version of PartCraft does not have: it builds every part with the mesh kernel. [What is not built yet](topic:not-built-yet) says what to do instead. The rest of this page describes it as it works with an exact kernel.
:::

A **Gordon surface** is one surface that passes through every curve of a network: a set of **profiles** running one way and a set of **guides** running the other, crossing like the threads of a net. It is how you shape a hull, a fender or a handle from a few section curves you control directly. It needs the **exact kernel** (see [The mesh kernel](topic:solid-kernels)).

## Draw the network

1. Draw the **profiles**: two or more curves running side by side, for example sections on parallel planes. Lines, arcs, circles and splines all work, and a chain of curves joined end to end counts as one curve.
2. Draw the **guides**: two or more curves running across the profiles. Each guide has to cross every profile once, and each profile every guide once.
3. Make the curves really meet. Draw a guide's sketch so its curve passes through the profiles' points (snap to projected points, or type the coordinates). The network accepts gaps up to the **Tolerance** (0.01 mm to start with).

One sketch can hold several curves: every separate chain in it is its own curve. So you can draw all the guides in one sketch on the top plane, for instance.


## Make the surface

1. Choose {{ui:More}} in the Features group, then {{ui:Gordon surface}}.
2. PartCraft takes the sketches nobody uses yet: those in planes parallel to the first become **profiles**, the others **guides**. Check the list on the right and set each sketch to **Profiles**, **Guides** or **Not used**.
3. To use edges of the part, click **Pick edges as profiles** (or **as guides**), click the edges in the view, then press Esc.

The result is a **surface body**. {{ui:Thicken}} turns it into a solid, {{ui:Stitch}} sews it to other surfaces, and {{ui:Trim surface}} cuts it.

### Close it to a solid

When the profiles are closed curves (circles, closed splines, closed chains) and the first and last profile each lie flat in a plane, tick **Close to a solid**. The ends are capped with flat faces and the closed shape joins, cuts, intersects or becomes a new body like any other feature. This is how you make a loft that follows guide curves. The same works with closed guides.

## Tangent to a plane along an edge

A Gordon surface passes through its curves, but on its own it does not match the slope of what it meets. To make it leave an edge **tangent** to a plane (G1), such as a mirror plane, so the mirrored copy joins without a crease, or a flat face it grows from:

1. Select the Gordon feature. Under **Tangent to**, choose a plane in {{ui:Add a plane…}} (Front, Top, Right or a reference plane), or pick a flat face in the view and press {{ui:Add the picked flat face}}.
2. PartCraft finds the boundary curve of the network that lies in that plane, and the surface leaves it tangent to the plane.

If a plane holds two boundary curves (a floor under both ends), add it twice: each copy takes the next one. Up to four planes, one per edge.

The report says along which curve each plane made it tangent and the steepest angle the surface met that plane at before. Guides that already leave that curve tangent to the plane are followed exactly. Guides that climb away from it are bent in a narrow band near the edge, and the report's "follows every curve within" figure says by how much: draw the guides leaving the edge flat for an exact fit.

## What is exact and what is fitted

The panel says which of these applies, and how far the surface strays from your curves.

- **Exact**: when every curve is polynomial (lines, splines and chains of them) and the guides cross all the profiles at the same parameters, the surface is exactly the Gordon surface of those curves. A network of iso curves cut from a bicubic surface gives that surface back, and four straight lines give a flat or twisted (bilinear) patch.
- **Fitted**: arcs, circles and edges of the part are not polynomial, and curves that cross at different parameters have to be re-timed so the crossings line up. Those curves are fitted by cubic B-splines within one ten-millionth of the network's size (about 0.00001 mm for a 100 mm part). A cylinder patch made from arcs and lines stays within 0.00001 mm of the true cylinder.

Between the curves the surface is whatever the method gives: the profiles blended across, plus the guides blended across, minus the blend of their crossing points. It is smooth everywhere except where a curve has a sharp corner. Where two Gordon surfaces share a boundary curve they meet edge to edge (G0), not tangent.

## When a network does not fit

| Message | What to do |
|---|---|
| "… do not meet: they pass 0.4 mm apart" | Make that profile and guide cross, or raise the Tolerance |
| "… meets … 2 times" | A guide may cross each profile only once: split a looping curve, or use fewer profiles |
| "… in a different order" or "folds over itself" | Two guides cross each other (or two profiles do). The families must run side by side |
| "at least two profiles and two guides" | Add curves, or set more sketches to Profiles or Guides |
| "mix open and closed curves" | Make all the profiles open or all closed (the same for the guides) |
| "An end … is not flat, so it cannot be capped" | Draw the first and last closed profile in one plane each, or leave Close to a solid off |

## Limits

- One B-spline face per network: no G1 or G2 tangency to neighbouring surfaces.
- A closed network's seam, at the first guide, is joined edge to edge but not tangent.
- The Loft feature itself has no guide rails; use a Gordon surface closed to a solid instead.
- Very wiggly curves, or crossings spaced very unevenly along one curve compared to another, can need too many points to fit; the feature then says so.
