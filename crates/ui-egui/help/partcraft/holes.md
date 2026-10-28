---
title: Drill holes and counterbores at sketch points
summary: Put points in a sketch on a face, make simple, counterbored, countersunk or tapped (ISO metric) holes at every point, set diameter and depth, and fix holes that drill the wrong way.
section: tasks
order: 60
keywords: hole, hole wizard, drill, modelled thread, 3D printing, counterbore, cbore, countersink, csink, tapped hole, thread, tap drill, metric thread, M6, through all, blind hole, bolt hole, points, hole centers
---

{{ui:Hole}} drills one hole at each loose point of a sketch. A loose point is one made with the Point tool, not the end of a line or the center of a circle.

## Drill holes in a face

1. Click the flat face to drill into, then choose {{ui:Sketch on this face}}.
2. Press {{key:P}} and click where each hole goes. Dimension the points from the edges or the origin if their position matters (see [Fully define a sketch](topic:relations-dimensions)).
3. Choose {{ui:Exit}}.
4. Choose {{ui:Hole}}.

Holes go through the part, 6 mm across. The right panel shows **Holes**: how many were made, "one per sketch point".

**Which sketch is used:** the sketch you just closed or the selected one; otherwise the most recent unused sketch that has loose points. If there is none: "Make a sketch with points where the holes go first (Sketch on a face, then Point)."

## Settings

| Setting | Choices | Default |
|---|---|---|
| **Type** | Simple, Counterbore, Countersink, Tapped (ISO metric) | Simple |
| **Diameter (mm)** | The hole's diameter | 6 |
| **Through all** | Ticked: right through the part. Clear it for a blind hole | Ticked |
| **Depth (mm)** | Shown when Through all is clear | 10 |
| **Counterbore ⌀** | Shown for Counterbore: the wider recess's diameter | 11 |
| **Counterbore depth** | Shown for Counterbore | 6 |
| **Reverse direction** | Drills to the other side of the sketch plane | Off |

Holes drill into the material, square to the sketch plane. A blind hole's depth is measured from the sketch plane; its bottom is flat.

## A counterbored bolt hole

1. Make the holes as above.
2. Set **Type** to **Counterbore**.
3. Set **Diameter (mm)** to `6.6` (clearance for an M6 bolt), **Counterbore ⌀** to `11` and **Counterbore depth** to `6.5`.

The counterbore must be wider than the hole ("The counterbore must be wider than the hole.") and, for a blind hole, shallower than it ("The counterbore must be shallower than the hole.").

## A bolt circle

For holes evenly spaced round a center, make **one** hole and pattern it: select the Hole feature, then choose {{ui:Circular}} (see [Pattern and mirror features](topic:patterns-mirror)). The pattern follows when you change the hole's size.

## When holes go the wrong way

- "There is nothing to drill on that side of the sketch. Try reversing the direction." The sketch sits on the far side of the material. Tick **Reverse direction**.
- "Put sketch points where the holes go (Point tool), then make the hole." The sketch has no loose points. Line ends and circle centers do not count.
- "There is nothing to drill yet." A hole needs a solid. Make a boss first.

## Holes in assemblies

Each hole has an axis. In an assembly, **Centre on** can put a joint on a hole's axis, listed as "Hole1, hole 2", so a pin or bolt part lines up with it. See [Build an assembly](topic:build-assembly).

## Countersunk holes

Set **Type** to **Countersink**, then **Countersink ⌀** (at the surface) and **Countersink angle**: 90° (ISO metric flat-head screws) or 82° (inch screws). Take the countersink diameter from the screw maker's data (a little over the screw head's diameter). The cone's face is named "countersink" in the panels.

## Tapped holes

Set **Type** to **Tapped (ISO metric)** and choose the **Thread**, M2 to M24 coarse. The hole is drilled at the tap drill size from the table below (nominal diameter minus pitch, to the drill series, as ISO 2306 lists for a 6H thread); the **Diameter (mm)** box is not used. For a blind hole, **Thread depth (mm)** sets how deep the thread goes.

| Thread | M2 | M2.5 | M3 | M4 | M5 | M6 | M8 | M10 | M12 | M14 | M16 | M18 | M20 | M22 | M24 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Pitch | 0.4 | 0.45 | 0.5 | 0.7 | 0.8 | 1 | 1.25 | 1.5 | 1.75 | 2 | 2 | 2.5 | 2.5 | 2.5 | 3 |
| Tap drill ⌀ | 1.6 | 2.05 | 2.5 | 3.3 | 4.2 | 5 | 6.8 | 8.5 | 10.2 | 12 | 14 | 15.5 | 17.5 | 19.5 | 21 |

The thread is **cosmetic** at first: the model has the drilled hole, the view draws the thread's major diameter as dashed circles, and the drawing sheet carries the callout as a note, such as "4× M8×1.25 - 6H, thread through". STEP exports the drilled hole only.

Tick **Modelled** to cut the real helical thread into the hole, for 3D printing. **Clearance (mm)** makes it that much larger all round, so a printed bolt fits: 0.1 to 0.2 mm suits most printers. The callout then ends "(modelled)". See [Threads on shafts and bores, drawn or modelled](topic:threads), which also puts threads on shafts.

:::note
Counterdrilled holes, inch threads and pipe threads are not built yet.
:::
