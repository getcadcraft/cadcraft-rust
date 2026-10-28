---
title: Brace gussets
summary: Connect a brace end to a column or beam with a gusset plate: angles and flat bars bolted, tubes slotted and welded.
section: tasks
order: 34
keywords: gusset, gusset plate, brace, bracing, vertical bracing, angle brace, flat bar, strap, hss brace, knife plate, slotted tube, connect
---

A brace gusset is a plate shop-welded to the column (or beam) the brace lands on, in the plane of the brace. DetailCraft cuts the brace back clear of everything at the joint, places and sizes the gusset, and joins the brace to it: an **angle** or a **flat bar** with one line of bolts, a **tube** (square, rectangular or round HSS) slotted over the gusset and welded.

## Where it works

- The brace's end lies on the line of a column or a beam (within 1 mm), and the brace does not run along that line. A column is the support where there is one; otherwise the beam.
- The gusset lies in the plane of the brace and the support's line. An **angle** must have one leg in that plane, and a **flat bar** its face: turn the brace (its {{ui:Rotation}}) until it does. With the angle's legs at 45° to the plane, DetailCraft says so and builds nothing.
- I-shapes and channels as braces are not connected.

## Add gussets

1. Select one or more braces.
2. Choose {{ui:Gusset}} in the {{ui:Connect}} group's {{ui:More}} menu.
3. Every end of the selected braces that lands on a column or beam and has no gusset gets one.

Select a gusset (click the plate) to change its settings. {{ui:Apply to similar (n)}} copies it to every free brace end with the same brace and support profiles.

## What DetailCraft works out

- **Cut back.** The brace is shortened at that end until its corners clear, by {{ui:Brace clearance}} (25), every member at the joint (beside it, or past its end when the member ends there) and a base plate under the column.
- **Bolts (angles, flats).** {{ui:Bolts}} (3) in one line along the brace, the first {{ui:Bolt end distance}} (40) from the cut end, at {{ui:Bolt pitch}} (75). On an angle they go through the leg in the gusset's plane, {{ui:Bolt gauge}} from the heel (0 = the middle of the leg); on a flat bar on its center line (or off it by the gauge). Bolt grip = the leg or flat + the gusset.
- **The gusset.** A rectangle: from the support's face out past the last bolt by the end distance, and along the support as far as the bolts need. It sits against the back of the angle's leg (or the flat's face). For a tube it runs through the tube's middle, under {{ui:Tube lap on the gusset}} (200) of the tube, with room for the welds.
- **Joints.** At a column base the gusset is carried down onto the base plate. Where a beam crosses the joint (a brace up into a beam-column corner), the gusset stays under or over the beam. If that leaves the bolts or the lap without their end distance, DetailCraft warns.
- **Welds.** The gusset to its support, both sides. A tube to the gusset, four lines along the lap.

## Good to know

- The gusset belongs to the support's assembly (shop-welded). The brace ships with its holes drilled, and goes to its own NC file.
- A tube's slot is not cut in the model; the [clash check](topic:clash-check) leaves the brace and its gusset alone.
- The gusset is not welded to a beam it touches, and corner gussets welded to both beam and column, chamfered corners and the work-point layout of a Uniform Force Method connection are not made yet.
- DetailCraft does not design the gusset or the bolts (no strength checks); the warnings are spacing and fit.
