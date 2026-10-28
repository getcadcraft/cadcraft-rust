---
title: Revolve a turned part
summary: Sketch half a profile beside a centerline, revolve it into a round part, choose the axis and angle, and use a revolved cut for grooves.
section: tasks
order: 50
keywords: revolve, revolved boss, revolved cut, turned part, lathe, shaft, axis, centerline, groove, angle, rev cut
---

{{ui:Revolve}} spins a profile around an axis, like a part made on a lathe: shafts, pulleys, flanges, knobs. Revolved cut (in the Features group's {{ui:More}} menu) spins a profile to remove material, for grooves and recesses.

## Revolve a stepped shaft

1. Sketch on **Front plane**.
2. Press {{key:N}} for {{ui:Centerline}}. Start on the origin and click a point straight to the right, for example 100 mm along. This is the axis. It is dashed: construction geometry is not part of the profile.
3. Press {{key:L}} for Line. Draw the **half** profile above the centerline as one closed loop: start on the centerline, go up, step along, and come back down to the centerline, then along it to the start. Its ends must meet: the loop must be closed.
4. Dimension the radii (heights above the centerline) and the lengths. A height of 10 above the centerline makes a diameter of 20.
5. Choose {{ui:Exit}}.
6. Choose {{ui:Revolve}}. The shaft appears, a full turn.

The profile must stay on one side of the axis. If it crosses: "The profile crosses the axis. Keep it on one side of the centerline." A profile edge may lie on the axis.

## Settings

| Setting | What it does | Default |
|---|---|---|
| **Operation** | Boss (add material), Cut (remove material), Intersect (keep the overlap) or New body | Boss for {{ui:Revolve}}, Cut for {{ui:More}} › {{ui:Revolved cut}} |
| **Angle (°)** | How far to spin, from just above 0 up to 360 | 360 |
| **Axis** | First centerline, or any line or centerline in the sketch, listed as "Centerline 1 (l1)" or "Line 3 (l3)" | First centerline |

A partial angle makes a wedge, spun from the sketch plane in one direction.

If the sketch has no centerline and you leave **Axis** on First centerline: "Draw a centerline (a construction line) in the sketch to revolve about." You can also pick a solid line of the profile as the axis.

## Cut a groove

1. With the shaft built, open a new sketch on **Front plane** (the same plane as the shaft's sketch).
2. Draw a centerline along the same axis: start it on the origin and keep it level so it gets an H, or type it with **Add by coordinates** (for example from `0, 0` to `100, 0`). A new sketch cannot snap to another sketch's lines.
3. Draw a small closed rectangle that overlaps the shaft's edge where the groove goes.
4. Choose {{ui:Exit}}, then {{ui:More}} › {{ui:Revolved cut}}.

## Revolved faces in assemblies

A revolve has an axis that other features and assemblies can use:

- a **Circular** pattern uses the last revolve's axis by default (see [Pattern and mirror features](topic:patterns-mirror));
- in an assembly, a joint between faces made by revolves starts as a **revolute** joint, and **Centre on** offers "Axis of Revolve1" (see [Build an assembly](topic:build-assembly)).

## True curves and facets

PartCraft builds a revolve with the mesh kernel (the status bar chip says "Mesh kernel"): round shapes are flat facets, 64 per turn by default, and volumes are a little under the true value (about 0.16 % at 64). The view, STL, 3MF and STEP exports are made of those facets: set **Curves** (click empty space, then the right panel) to Fine (128) or Very fine (256) for smoother ones. True cylinders, cones and tori need an exact kernel: not built yet in this version. See [The mesh kernel](topic:solid-kernels).
