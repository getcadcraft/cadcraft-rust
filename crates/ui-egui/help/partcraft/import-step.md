---
title: Open a STEP file and build on it
summary: Bring a supplier's or another CAD program's STEP solid into PartCraft, as a new part or added to the current one, set which way is up, and add sketches and features to it.
section: tasks
order: 87
keywords: step, stp, import, open step, ap203, ap214, ap242, supplier model, dumb solid, imported solid, up axis, z up, y up
---

:::note
**Not built yet in this version.** Opening STEP files needs an exact geometry kernel, which this version of PartCraft does not have: it builds every part with the mesh kernel. [What is not built yet](topic:not-built-yet) says what to do instead. The rest of this page describes it as it works with an exact kernel.
:::

PartCraft opens STEP files (`.step` or `.stp`) as **imported solids**: an exact solid you can sketch on, cut, drill, fillet and combine with your own features. It has no history of its own: you cannot change the dimensions it was made with, only add to it or cut from it.

STEP import needs the exact kernel. If it is not ready yet, PartCraft waits for it ("Loading the exact kernel to read STEP…").

## Open a STEP file

1. Choose {{ui:Open}} or press {{key:Mod+O}}, and pick the `.step` or `.stp` file. Or drop it on the 3D view.
2. If the part on screen is empty, the STEP solid becomes a new part named after the file.
3. If the part already has features, PartCraft asks: "Import bracket.step: 1 solid, 64 faces. Add it to Part1, or start a new part with it?" Choose {{ui:Add to this part}}, {{ui:New part}} or {{ui:Cancel}}.

An **Imported1** feature appears in the history and the status bar says, for example, "Imported bracket.step: 1 solid, 64 faces, 15200.512 mm³. Sketch on its flat faces and add features; it is kept inside the part file."

## Set which way is up

Most CAD programs use Z up; PartCraft uses Y up. By default PartCraft turns the file so its Z points up here.

If the part arrives lying on its side, select **Imported1** and set **Up in the file** to **Y up**. The panel also shows the file name and how much STEP text is stored.

## Build on it

- Click a flat face of the imported solid and choose {{ui:Sketch on this face}}.
- Add cuts, holes, bosses, fillets, chamfers or a shell as on any part.
- Use {{ui:Linear}}, {{ui:Circular}} and {{ui:Mirror}} with the imported solid as a feature to copy.
- Check its size in the **Part** box: volume, surface area, size.

## Where it is kept

The STEP text is stored **inside** the `.partcraft.json` file, so the part file is self-contained: you do not need the STEP file again. The part file gets bigger by the size of the STEP file.

## Messages

| Message | What it means |
|---|---|
| '"part.stp" is not a STEP file (it does not start with ISO-10303-21).' | The file is not plain STEP text. Ask the sender for an AP203, AP214 or AP242 STEP file |
| "That STEP file is too big (over 60 MB) to import here." | Simplify the model at the source, or send parts one by one |
| "This STEP file could not be read (fail)." | The file is damaged or uses parts of STEP the kernel cannot read |
| "This STEP file has no closed solid (only surfaces or wires), so PartCraft cannot use it as a body." | The model is surfaces only. Ask for a solid |
| "STEP needs the exact kernel (OpenCascade), which did not load: …" | See [The mesh kernel](topic:solid-kernels) |

## Limits

- The imported solid keeps its shape exactly, but has no editable features. Changing a hole size in it means cutting or filling it with your own features.
- Colors, names of bodies, assemblies and PMI (tolerances in the file) are not read. An assembly STEP file comes in as all its solids in one imported feature (the **Bodies** line of the Part box counts them), not as separate parts. A closed surface model is sewn into a solid where it can be.
- STEP is the only other format PartCraft opens. STL, IGES and other programs' native files are not supported.
