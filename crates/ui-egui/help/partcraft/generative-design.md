---
title: Grow a shape from loads (Generative)
summary: Pick the faces that are held and loaded, regions to keep and to keep clear, target volumes and materials, and generate outcomes: the stiffest shapes found for each volume, checked again as solids, compared in a table and downloaded as STL, STEP or a new part.
section: tasks
order: 144
keywords: generative design, topology optimization, topology optimisation, SIMP, lightweight, lattice, organic shape, outcomes, compare, mass, volume fraction, preserve, keep-in, obstacle, keep-out, design space, minimum member size, STL, 3D printing
---

The Generative workspace removes material the loads do not need. You say where the part is held and loaded, what must stay and what must stay clear; PartCraft finds the stiffest shape for each target volume (**topology optimisation**), checks each one again as a solid with the real material, and shows the outcomes side by side. It works on the named faces the mesh kernel gives the part (see [The mesh kernel](topic:solid-kernels)).

Choose **Generative** in the **Workspace** box. Your part is not changed; the study is saved inside the part file.

## Set up a study

1. Press **New study** in the right panel.
2. **Space**: *The part as it is* removes material from your part; *A box around the part* grows a shape inside a box larger than the part by the **Box margin**.
3. **Element size**: leave it empty for an automatic size (about 8,000 cubes), or type one. Finer sizes give finer shapes and take longer.
4. **Minimum member**: members thinner than about twice this do not form. Empty means one and a half cubes.
5. Tick one to six **Target volumes** (the share of the design space the shape may fill) and one to four **Materials**. Every volume is generated once; each material is checked on that shape (the stiffest shape does not depend on the material, but its mass, stress and safety factor do).

## Goals

- {{ui:Fixed}}: click the faces that are held. A **Frictionless face** from the panel's **Add** list holds a face only across itself (use it for symmetry planes).
- {{ui:Force}}: click the loaded faces, then type the total force and choose its direction in the panel. **Pressure** is in the **Add** list.
- {{ui:Preserve}}: click faces whose surroundings must stay solid, and set how deep (**Keep solid within**) — for example around bolt holes. The faces you fix or load are always kept.
- {{ui:Obstacle}}: click faces whose surroundings must stay empty, and set how far (**Keep empty within**) — room for a shaft or a tool.

Press **Done picking** or {{key:Esc}} after clicking faces. Green and red tints show the regions.

## Generate and compare

Press {{ui:Generate}}. The panel shows progress and the time left; **Stop** cancels. Each outcome appears in the **Outcomes** table: target volume, material, **mass**, **largest stress**, **largest displacement** and **safety factor** (yield ÷ stress), all from re-solving the finished shape as solid cubes with the real material.

Choose an outcome to see it in the view (the part is hidden; tick **Show the part through it** to see both). Below the table: how many iterations it took and whether it settled, its stiffness as a solid against the optimiser's own figure, and its surface.

## Use an outcome

- **STL**: a closed triangle mesh, ready for 3D printing.
- **STEP (faceted)**: the same surface as a STEP solid made of flat faces.
- **New part file**: a PartCraft part whose one feature is the outcome as an imported solid. Open it to sketch on it, cut holes or check it in Simulation. It is faceted (thousands of flat faces), not an editable shape.
- **Add to this part** joins the outcome to the part; it is on only for a box design space (with the part itself as the space, the outcome lies inside the part).

**New part file** and **Add to this part** are not built yet in this version: the outcome becomes an imported solid, which needs an exact kernel. The STL and STEP (faceted) downloads work.

## How far to trust it

- It optimises **stiffness** for the given volume. There are no stress limits and no manufacturing rules (overhangs, milling directions, minimum hole sizes) yet: check a chosen outcome in [Simulation](topic:simulation) and adjust.
- Thin members one or two cubes wide are approximations: use a smaller element size for slender parts.
- The method is SIMP with a density filter, a threshold projection and optimality-criteria updates; the reported stress and deflection are of the outcome as cubes, which is honest but coarse at sharp corners.

## Related

- [Check stress and deflection (Simulation)](topic:simulation)
- [Export for printing, STEP and DXF](topic:export-files)
- [What is not built yet](topic:not-built-yet)
