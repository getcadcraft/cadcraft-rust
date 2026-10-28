---
title: Check for clashes
summary: Find where elements of one set run into elements of another — ducts through beams, walls into slabs — in one model or across several, with a tolerance, and turn a clash into an issue.
section: tasks
order: 30
keywords: clash, clash detection, clash check, hard clash, interference, collision, coordination, overlap, tolerance, mep, structure
---

## What you'll do

Find the places where two sets of elements run into each other, for example everything in the services model against the structure, then look at each one and keep the ones that matter as issues.

## Steps

1. Open an IFC model, and add the others with {{ui:Add a model…}} if you are checking across models. See [See several IFC models together](topic:several-models).
2. Open {{ui:Clash check}} in the left panel.
3. In {{ui:Check}} and {{ui:Against}}, choose two sets: the elements you picked, a class (over every open model), a saved search set (see [Find elements by class, name or property](topic:find)), or everything.
4. Set how much overlap to ignore, in metres. 0.01 ignores elements that only just meet.
5. Click {{ui:Run}}. The check runs in the background; {{ui:Cancel}} stops it.
6. Click a result to see it: the two elements are picked and shown, everything else see-through, and the view zooms to them.
7. Click {{ui:Make an issue of it}} under a result to write it up in the {{ui:Issues}} tab. See [Issues and BCF files](topic:issues-bcf).

## What you should see

- The clashes listed with both elements' names (and their files, when several models are open) and how deep they overlap, the deepest first.
- Faces that only touch, such as a wall standing on a slab or a door in its opening, are not clashes.

## Good to know

- This is a basic hard-clash check: elements whose surfaces cross. An element hidden wholly inside another (a pipe buried in a wall) is not found yet.
- The depth is how far the two elements' bounds overlap at the least: a guide, not an exact measurement.

## Next

- [Measure distances, face areas and locations in 3D](topic:measure)
- [Hide, isolate and see through elements](topic:hide-and-isolate)
