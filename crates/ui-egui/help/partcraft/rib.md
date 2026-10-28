---
title: Ribs and gussets
summary: Stiffen a part with a rib drawn as one line: it fills from the line to the walls, a set thickness across the sketch plane, on the side you choose.
section: tasks
order: 92
keywords: rib, gusset, web, stiffener, brace, bracket, corner support, thin wall, strengthen
---

:::note
**Not built yet in this version.** The rib needs an exact geometry kernel, which this version of PartCraft does not have: it builds every part with the mesh kernel. [What is not built yet](topic:not-built-yet) says what to do instead. The rest of this page describes it as it works with an exact kernel.
:::

A **rib** (or gusset) is a thin wall that stiffens a part, such as the triangle inside an L-bracket's corner. You draw only its free edge: PartCraft fills the space between that line and the walls. It needs the **exact kernel** (see [The mesh kernel](topic:solid-kernels)).

## Add a rib

1. Make a plane through the middle of where the rib goes (for an L-bracket 40 deep, a plane 20 in front of its sketch: see [Reference planes, axes and points](topic:reference-geometry)).
2. Sketch on it **one line** from wall to wall — or a few lines end to end for a bent rib edge. The line may stop short of the walls: its ends are carried on until they meet them.
3. Select the sketch and choose {{ui:More}} › {{ui:Rib}}.

The rib fills from the line to the walls on the side toward the middle of the part, so a gusset lands in the inside corner. If that side is open and the other is closed, PartCraft flips it for you and says so.

## Settings

- **Thickness (mm)** is the rib's thickness across the sketch plane. It can be driven by a parameter.
- **Thickness** (the choice below it): **Centred on the sketch plane**, **In front of the plane** (along its normal) or **Behind the plane**.
- **Flip** fills the other side of the line.

## Messages

- **On this side the rib's edge never meets the body** — nothing closes in the space on that side: tick or untick **Flip**, or move the line so walls bound it.
- **A rib needs one open chain of lines in its sketch** — the sketch has a closed shape, an arc or a spline, or lines that do not join end to end.
- **The rib's edge runs inside the body there** — the line lies inside the solid; draw it in the open space between the walls.
