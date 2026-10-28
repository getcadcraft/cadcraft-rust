---
title: Export a KISS file for fabrication software
summary: Send the bill of materials to a fabrication management (MIS) program as a KISS 1.1 file: assemblies, parts, phases as sequences, holes, copes and welds.
section: tasks
order: 58
keywords: kiss, kss, mis, fabrication management, production control, estimating, bill of materials, bom, export, fabtrol, sequence
---

In the {{ui:Output}} group, open {{ui:More}} and choose {{ui:KISS file for fabrication software}}. DetailCraft downloads `<name>.kss`, a KISS file ("Keep It Simple, Steel", version 1.1), the plain text format most fabrication management programs import a bill of materials from.

## What is in it

- The header: a job number (the first eight letters and digits of the model name), the model name, the date and time, and whether sizes are metric.
- For each assembly mark: its main part (quantity = how many of the assembly), a drawing line (A-B1 and so on, the assembly drawing's number), an assembly line (name, type, weight, overall length, depth and width), one sequence line per [phase](topic:phases) the assembly is in, and labor lines: holes (how many, diameter, thickness through), copes and welds.
- Then each part attached to it, with the **total** quantity (per assembly × assemblies), as the standard asks.
- Loose bolts and anchor rods on drawing BOLTS, assembly FIELD.
- Every line gives the type (W, C, L, HSS, PL, or MI for a custom profile), size, grade, cut length in millimeters, part name and weight.

## Imperial or metric

In [DetailCraft settings](topic:settings), **KISS file sizes** chooses:

- **Automatic** (the start): imperial when every member is an AISC shape, metric otherwise.
- **Imperial**: shapes by their AISC designation (W 16X26, HSS 4X4X.250); plates and bolts, which DetailCraft keeps in millimeters, to the nearest 1/16 in; weights in pounds.
- **Metric**: European shapes by their name (IPE300), plates as thickness X width in millimeters; weights in kilograms.

Lengths are always in millimeters with two decimals, whatever the setting: the KISS standard asks for that and the receiving program converts them.

:::note
Grades are cut to the standard's eight characters: A572 Gr 50 becomes A572-50. Commas in names become spaces (the format has no quoting). Check one import in your program before relying on it: DetailCraft's KISS files are checked against the standard here, not in a fabrication program yet.
:::
