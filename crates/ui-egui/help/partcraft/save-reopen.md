---
title: Save, reopen and keep working
summary: Save parts and assemblies to your computer, name them, reopen them by button, key or drag, keep versions, and avoid losing work in a browser tab.
section: tasks
order: 150
keywords: save, open, reopen, load, download, versions, backup, rename, file name, lost work, leave site, partcraft.json, partcraft-asm.json, new part
---

PartCraft keeps nothing between visits. Your part lives in the browser tab until you save it as a file, and comes back when you open that file.

## Save a part

1. Name the part: click empty space in the view so nothing is selected, then type the **Name** in the right panel and press {{key:Enter}}. The history heading changes to match.
2. Press {{key:Mod+S}} or choose {{ui:Save}}.
3. The browser downloads `<name>.partcraft.json`. The status bar says "Saved Bracket.partcraft.json (the full history; reopen it here to keep editing)."

If a sketch is open, saving closes it first.

The file holds every sketch, relation, dimension and feature, plus suppressed features, the rollback position, the drawing sheet's settings and GD&T notes, and the full text of any imported STEP solid. Your undo list is not saved.

## Save an assembly

In an assembly, the same {{ui:Save}} or {{key:Mod+S}} downloads `<assembly name>.partcraft-asm.json`. Every part is stored inside it: you do not need the separate part files to reopen it.

While you edit a part from inside an assembly, {{key:Mod+S}} saves that part on its own as a `.partcraft.json`. Choose {{ui:Back}} first, then save, to save the whole assembly.

## Reopen

Any of these:

- choose {{ui:Open}} or press {{key:Mod+O}}, then pick the file;
- drag the file from {{pick:mac=Finder|windows=File Explorer|linux=your file manager}} onto the 3D view ("Drop a PartCraft part to open it" appears).

PartCraft tells parts from assemblies by what is inside, so both open the same way. A `.step` or `.stp` file opens as an imported solid: see [Open a STEP file and build on it](topic:import-step). The status bar confirms: "Opened Bracket.partcraft.json: 7 features." or "Opened Hinge.partcraft-asm.json: 3 parts, 2 joints."

If the part on screen has unsaved changes, PartCraft first asks: {{ui:Cancel}}, {{ui:Discard changes}} or {{ui:Save first}}.

:::warning
Close an assembly ({{ui:Close}}) before opening a part file. If you open a part while an assembly is on screen, the part loads behind the assembly and you only see it after closing the assembly. Opening an assembly file also replaces an unsaved assembly without asking, so save it first.
:::

## Start over

{{ui:New}} starts an empty "Part1". It asks first if there are unsaved changes, and closes an open assembly (asking if it is unsaved).

## Keep versions

Each save is a new download, so you can keep versions simply by keeping the files:

- Put a version or date in the name before saving: "Bracket rev B".
- Your browser may add " (1)", " (2)" when a file of the same name is already in the downloads folder. The newest is the one with the highest number, or check the date in {{pick:mac=Finder|windows=File Explorer|linux=your file manager}}.
- Move finished files out of the downloads folder into your project folder.

See [Where your files go](common:your-files) for where downloads go on each system.

## Do not lose work

- Save before closing the tab, reloading, or letting the computer sleep for long. The browser asks "Leave site?" if you try to close with unsaved changes, but a crash or a closed laptop gives no warning.
- Private or incognito windows work, but nothing else is different: the file is still the only copy.
- Cloud saving and version history are not available yet.
