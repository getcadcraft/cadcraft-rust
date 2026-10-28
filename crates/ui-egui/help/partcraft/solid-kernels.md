---
title: The mesh kernel
summary: This version of PartCraft builds every part with a mesh kernel that runs in your browser. What that means for round faces, volumes and files, how to tell it is working, and which features wait for an exact kernel.
section: start
order: 40
keywords: kernel, mesh, manifold, exact, b-rep, faceted, curves, opencascade, occt, fillet needs exact, not built, offline, webassembly
---

A geometry kernel is the engine that turns your history into a solid. This version of PartCraft has one: the **mesh kernel** (manifold-3d). It runs on your computer as a small WebAssembly program and is ready a moment after the app opens. Nothing about your part is sent anywhere.

| | Mesh kernel |
|---|---|
| Round shapes | Flat facets. **Curves** in the part panel sets how fine |
| Volumes and areas | Of the faceted solid, a little low on round parts |
| Sketches, extrude, cut, revolve, holes, patterns, mirror, reference planes | Yes |
| Fillet and Chamfer | On straight edges between flat faces; rounds that meet are mitred |
| Shell and Draft | Shell opens flat faces (exact wall thickness); Draft tilts flat faces |
| Exports | STL, 3MF, STEP (faceted), drawing PDF and DXF |
| Drawing views | Drawn from the facets, section views included |
| Faces and edges | Named and known by kind (flat, cylinder, cone, sphere, torus; straight, circle), taken from the sketches: the Simulation, Generative and Manufacture workspaces, Measure, picking and the hole table use them |

The chip near the right end of the status bar says **Mesh kernel** while it builds your part.

## What waits for an exact kernel

An exact (B-rep) kernel keeps true circles, cylinders and cones, and smooth fillets on any edge, sweeps, lofts, surfaces, sheet metal, ribs, several bodies, STEP import and exact STEP export are built on one. Earlier lab builds of PartCraft used OpenCascade for that. Its license (the LGPL) does not fit this release, where every app is Apache-2.0 and uses only permissively licensed libraries, so it is not included. Nothing here has a date.

Until then those features say **Not built yet** when you choose them: the status bar gives the reason, and their buttons' tooltips end with "not built yet (needs an exact kernel)". [What is not built yet](topic:not-built-yet) lists each one with what to do instead.

A part made with them in an earlier build still opens. Those features stay in the history marked as not built (hover one for the reason), the rest of the part builds, and saving keeps them unchanged.

## Round faces and Curves

The mesh kernel draws a circle as a polygon. **Curves** in the part panel sets how many sides a full circle gets; more sides look smoother and measure closer to the true value, and take longer to build. Volumes, areas and distances from {{ui:Measure}} are of this faceted solid: for a hole or a boss they are a little smaller than the round shape's.

## Online and offline

The mesh kernel is part of the app's own download. To work with no connection, save PartCraft for offline use while you are online: choose {{ui:Save for offline}} in the app bar and wait until it says **Offline ready**. Details: [Working offline](common:offline).
