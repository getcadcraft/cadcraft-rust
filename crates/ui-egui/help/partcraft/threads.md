---
title: Threads on shafts and bores, drawn or modelled
summary: Put an ISO metric or trapezoidal thread on any round face, a shaft or a bore. Keep it cosmetic for drawings, or model the real helical thread for 3D printing, with a clearance so printed parts fit.
section: tasks
order: 62
keywords: thread, threads, modelled thread, modeled thread, helical, helix, screw, bolt, nut, shaft, bore, external thread, internal thread, cosmetic thread, metric thread, trapezoidal, acme, lead screw, Tr16x4, M10, 3D printing, print, clearance, fit, left hand, tapped
---

{{ui:Thread}} puts a thread on round faces: the outside of a shaft (an external thread) or the inside of a bore (an internal thread). PartCraft tells which from the face.

A thread is **cosmetic** at first: the face stays plain, the view draws the thread as dashed circles and the drawing sheet notes it, such as "M10×1.5 - 6g, thread 20 long". Tick **Modelled** to cut the real helical thread into the part, for 3D printing or a model that must look right.

## Thread a shaft or a bore

1. Build the shaft or the bore at the thread's size: a Ø10 shaft for an M10 bolt, or a Ø8.5 bore (the tap drill) for an M10 nut.
2. Click the round face, then choose {{ui:More}} › {{ui:Thread}}. With no round face selected, PartCraft asks you to click the faces, then {{ui:Done}}.
3. In the right panel, check the **Size**. **Fit to the face** picks the size by itself: for a shaft, the size whose major diameter is the shaft's; for a bore, the size whose minor diameter it fits.

The thread starts at the face's open end, where the part does not carry on along the axis: the free end of a shaft, the mouth of a bore. **Start at the other end** turns that round.

## Settings

| Setting | Choices | Default |
|---|---|---|
| **Form** | ISO metric (60°), M1.6 to M64 coarse; trapezoidal (Tr, 30°), Tr8×1.5 to Tr60×9 | ISO metric |
| **Size** | Fit to the face, or one size from the list | Fit to the face |
| **The whole face** | Clear it to give a **Length (mm)**, measured from the start | Ticked |
| **Start at the other end** | The thread runs from the other end of the face | Off |
| **Hand** | Right hand or left hand | Right hand |
| **Modelled** | Cut the real helical thread | Off |
| **Clearance (mm)** | Shown when Modelled is ticked: how much smaller a shaft's thread is, and how much larger a bore's, all round | 0.1 |

A size that does not suit the face is built anyway, cut to the face, and the panel says so, such as "A Ø12 mm shaft is not M10's Ø10 mm". Several faces can share one Thread feature; with **Fit to the face**, each gets its own size.

## Modelled threads for 3D printing

A modelled thread follows the standard basic profile: ISO 68-1 for metric threads (flat crests and roots, 60° flanks), ISO 2901 for trapezoidal ones (30° flanks). The **Clearance** moves the whole profile in (a shaft) or out (a bore) so printed parts screw together:

- **0.1 to 0.2 mm** suits most printers. Print a test bolt and nut first.
- **0** is the standard profile, for parts that are machined or for checking a fit.

A bolt and a nut with the same clearance never touch along their flanks: the gap between them is twice the clearance, all round.

:::tip
Fine pitches print poorly: M3 and smaller, or a pitch under 0.5 mm, is close to what a 0.4 mm nozzle can draw. Model larger threads, and use a heat-set insert or a tapped hole for small ones.
:::

A modelled thread is part of the solid: STL, 3MF and STEP exports carry it, and Measure measures it. It makes the part heavier to rebuild: an M10 thread 20 long is about 150,000 triangles.

## Tapped holes

A **Tapped (ISO metric)** hole has a **Modelled** switch too, with the same **Clearance**. See [Drill holes and counterbores at sketch points](topic:holes).

## When a thread does not build

- "A thread goes on a round face (a cylinder): a shaft or a bore." The face picked is flat or curved some other way.
- "No standard size fits a round face of Ø0.6 mm: choose a size." Fit to the face found nothing within 12 % of the face. Choose a **Size**, or change the face.
- "A face this thread was on is gone (…). Pick it again." An earlier change removed the face. Click {{ui:Pick faces}} and click the face again.

:::note
Inch threads (UNC, UNF), pipe threads, tapered threads, thread run-outs and chamfered thread starts are not built yet. The exact kernel shows a modelled thread as cosmetic.
:::
