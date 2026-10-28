---
title: Sweep, loft and draft
summary: Sweep a closed profile along a path of sketch curves or model edges, loft through sections on different planes, and add draft to faces; with the options, the exact results, and the messages.
section: tasks
order: 80
keywords: sweep, swept boss, swept cut, pipe, path, loft, lofted boss, blend sections, ruled, draft, draft angle, taper faces, neutral plane, pull direction, molding
---

:::note
**In this version** sweep and loft are not built yet: they need an exact geometry kernel, which this version of PartCraft does not have. [What is not built yet](topic:not-built-yet) says what to do instead; the Sweep and Loft sections describe them as they work with an exact kernel. **Draft works** on flat faces: the mesh kernel builds it, with the same result an exact kernel gives.
:::

All three are in the Features group's {{ui:More}} menu (see [The mesh kernel](topic:solid-kernels)). Each can join material, cut it, keep only the overlap (**Intersect**) or make a **New body** (sweep and loft: **Operation** in the panel).

## Sweep

A sweep moves a closed profile along a path: pipes, handles, springs of wire, rails.

1. Draw the **path** in one sketch: lines and arcs joined end to end (or use model edges, below).
2. Draw the **profile** in another sketch whose plane passes through one end of the path, for example a circle on the top plane at the start of a path drawn on the front plane.
3. Select the profile sketch and choose {{ui:More}}, then {{ui:Sweep}}. The most recent other sketch becomes the path.

In the panel:

| Setting | What it does |
|---|---|
| **Path** | Tick the sketches that make the path (several sketches join end to end into one 3D path); **Pick model edges** adds edges of the part |
| **Orientation** | Follow the path (default), Frenet, Fixed (sections stay parallel to the profile), Keep a direction, Along a guide curve |
| **End scale** | The profile grows or shrinks linearly to this size at the end (1 = constant) |

Sharp corners in the path are mitred. A circle of radius 5 swept along a 100 mm line is a cylinder of exactly 2500π mm³; along a quarter arc of radius 50, a quarter torus of 625π² mm³.

## Loft

A loft blends through two or more sections on different planes.

1. Draw each section in its own sketch (one closed loop each), on base planes, reference planes (see [Reference planes, axes and points](topic:reference-geometry)) or faces.
2. Choose {{ui:More}}, then {{ui:Loft}}. Every unused one-loop sketch becomes a section, in history order.

The panel lists the sections in order (**Up** reorders, **Remove** drops one); **Add a sketch** adds another, **Pick a face** adds a flat face of the part. **Ruled** makes flat faces between sections instead of a smooth blend. Between two squares, both are the same frustum (a 40 mm square to a 20 mm square over 30 mm: exactly 28000 mm³).

Sections with different numbers of sides (a square to a triangle) are matched by OpenCascade, which may twist the side faces; the feature warns you. Draw sections with the same number of sides, starting from matching corners, for the cleanest result.

### Guide rails

Guide rails are curves the loft's sides must follow from the first section to the last, for a bulge, a waist or a flare you draw yourself.

1. Draw two or more rails: open curves (lines, arcs, splines) that run from the first section to the last and cross every section once. One sketch can hold several rails: every separate chain in it is its own rail. Draw a rail so it really meets each section: snap its ends to the sections' points.
2. In the Loft panel, under **Guide rails**, tick the sketches that hold them, or click {{ui:Pick model edges as rails}} and click edges of the part (Esc ends).

With rails the loft is a Gordon surface through the sections and the rails (see [Surfaces through a curve network](topic:gordon-surfaces)), capped flat at both ends into a solid. The panel says how closely it follows them. Every section must then be a sketch (a point or a face cannot take a rail), and **Ruled** does not apply. A rail that misses a section by more than 0.01 mm is refused, with the gap.

Checked: two circles (20 mm and 10 mm across 40 mm) with four straight rails give the cone frustum's volume to within 0.000002 %; arcs that bulge out 5 mm give a barrel that follows every rail within 0.00001 mm.

## Draft

Draft tilts faces so a part comes out of a mold.

1. Choose {{ui:More}}, then {{ui:Draft}} (or {{key:Shift}}+click faces first). With nothing picked you are in pick mode: click each face to tilt, then {{ui:Done}}.
2. Set **Angle (°)**, the **Neutral plane** (where the faces keep their size) and the **Pull direction** (the neutral plane's normal by default).

A positive angle leans the face inward along the pull and removes material; a negative one adds it. The neutral plane starts as the Top plane. In this version only flat faces can be drafted: a cylinder, cone or freeform face says "Draft: in this version flat faces only (the mesh kernel builds it)." The four sides of a 100 × 100 × 50 block drafted 5° about its bottom become a frustum of exactly h/3·(a² + b² + ab), with b = a − 2h·tan 5°.

## Messages

| Message | What to do |
|---|---|
| "The profile's plane has to pass through one end of the path …" | Move the profile sketch to the start (or end) of the path |
| "The path has a gap: its curves do not join end to end …" | Join the path's curves; in separate sketches their ends must meet exactly |
| "A loft section needs exactly one closed loop" | Split the sketch, or remove the extra loop |
| "… cannot be drafted about this neutral plane …" | The face is parallel to the neutral plane or does not meet it |
| "Draft: in this version flat faces only (the mesh kernel builds it)." | A picked face is curved. Remove it; draft only flat faces |

:::note
Not built yet: twisting sweeps, thin-wall and surface sweeps, loft guide rails and end tangency, draft on cylinders, cones and freeform faces.
:::
