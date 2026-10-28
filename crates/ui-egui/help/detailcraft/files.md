---
title: Open, save and export files
summary: The DetailCraft model file, every export it writes, where the files go, and what differs by browser and connection.
section: start
order: 30
keywords: open, save, export, download, file, json, ifc, nc1, dstv, csv, pdf, zip, formats, import
---

DetailCraft has one file of its own, the model, and five exports. Every save and every export is a download. Nothing is uploaded, and nothing is kept on a server.

## The model file

A DetailCraft model is saved as `<name>.detailcraft.json`, where `<name>` is the model name in the {{ui:Model}} panel. It holds the grids, levels, members (with their holes and copes), connections and their settings, footings and your custom profiles.

Parts, marks, bolts, welds and bars are not stored: DetailCraft works them out again from the model every time it opens. That is why a model file is small.

### Save

1. Type a name in the {{ui:Name}} field at the top of the {{ui:Model}} panel.
2. Choose {{ui:Save}} in the {{ui:File}} group, or press {{key:Mod+S}}.
3. The browser downloads `<name>.detailcraft.json`. The status bar says "Saved … (downloaded)", and the dot at the start of the browser tab title goes away.

Each save is a new download. The browser does not overwrite the file you opened; depending on its settings it adds a number to the name, such as `Frame (1).detailcraft.json`, or asks where to save. Keep your own versions by renaming the downloads.

:::chromium
Chrome and Edge save to your Downloads folder unless **Ask where to save each file before downloading** is on in the browser's download settings. Turn it on if you want to pick a folder and file name each time.
:::

:::firefox
Firefox saves to your Downloads folder, or asks each time if **Always ask you where to save files** is on in its settings.
:::

:::safari
Safari saves to your Downloads folder. To change the folder, open **Safari, Settings, General, File download location**.
:::

### Open

1. Choose {{ui:Open}} in the {{ui:File}} group, or press {{key:Mod+O}}.
2. Pick a `.detailcraft.json` file.

Or drag the file from {{pick:mac=Finder|windows=File Explorer|linux=your file manager}} and drop it on the view. The view says "Drop a DetailCraft model to open it" while you drag.

If the open model has unsaved changes, DetailCraft asks first: {{ui:Cancel}}, {{ui:Discard changes}} or {{ui:Save first}}.

DetailCraft checks every field of a file before it uses it. Anything it cannot use is left out and the status bar says what, for example "2 members were not valid and left out." A model saved by a newer DetailCraft is refused with a message rather than opened half right.

### What it opens

Its own model files (`.json`, up to 50 MB) and IFC files (`.ifc`, IFC2X3 or IFC4, up to 200 MB). An IFC file, opened or brought in with {{ui:Import IFC}}, becomes a new model of its beams, columns, members, plates, grids and levels, with the holes and copes cut in them and its bolts as loose bolts; a report lists what was left out. See [IFC export](topic:ifc-export#import). It does not open DWG, DXF or any other detailing program's model yet; trying shows "DetailCraft opens its own models (.detailcraft.json) and IFC files (.ifc)."

## Lost work, recent files, the desktop app

- While the model has changes you have not saved, DetailCraft keeps a copy in this browser. If the tab or the browser stops before you save, the next time you open DetailCraft it asks **Recover your work?**: recover the copy, or let it go. See [Recover your work](common:recover).
- Models you opened lately are listed on the start screen when the browser lets DetailCraft open them again; click one to reopen it.
- In the desktop app, {{ui:Save}} writes the model back to the file you opened (in the browser it downloads a copy), and double-clicking a DetailCraft model or an IFC file opens it in DetailCraft.

## Exports

All exports use the model name. Choose them in the {{ui:Output}} group.

| Button | File | What is in it |
|---|---|---|
| {{ui:IFC}} | `<name>.ifc` | IFC4: storeys, grids, every steel part as a profile extrusion with its holes and copes, assemblies, bolts and anchor rods, footings and every bar. See [Export IFC](topic:ifc-export). |
| {{ui:NC files}} | `<name> NC.zip` | One DSTV `.nc1` file per part mark. See [Send NC files to the shop](topic:nc-export). |
| {{ui:BOM}} | `<name> BOM.csv` | Three tables in one file: parts, assemblies, and bolts and anchor rods. See [Marks and the bill of materials](topic:marks-and-bom). |
| {{ui:Bending}} | `<name> bending schedule.csv` | Every bar mark in the footings. See [Footings and rebar](topic:footings-and-rebar). |
| {{ui:Shop drawing}} | `<name> <mark>.pdf` | One sheet (A3 unless you chose another size) for the selected part (I-shapes and plates). See [Make a shop drawing](topic:shop-drawings). |
| {{ui:Drawings}} | `<name> drawings.pdf`, `<name> <sheet>.pdf`; `<name> <sheet>.dwg` or `.dxf`, several in `<name> drawings DWG.zip` (or `DXF.zip`) | The plans, elevations and assembly drawings on the sheet size you chose; DWG and DXF at full size, 1 unit = 1 mm on the paper. See [General-arrangement and assembly drawings](topic:drawings). |

### Notes on the CSV files

- They start with a byte-order mark so spreadsheet programs read accented characters correctly.
- A cell that starts with `=`, `+`, `-` or `@` (and is not a plain number) gets a leading apostrophe, so a spreadsheet never runs it as a formula.
- Lengths are millimeters and weights kilograms.

### Notes on the PDF

Text in the PDF uses the standard Helvetica font. Characters it does not have (outside the Western European set) print as "?", and the status bar tells you how many.

## Units

DetailCraft works in millimeters throughout: grids, levels, coordinates, profiles, holes and every export. Profile names keep their published form (W12x50 is a US designation even though its dimensions are shown in mm). There is no imperial input yet.

## Online and offline

Opening, saving and exporting all happen in your browser, so they work the same with or without a connection once DetailCraft has loaded.

To use DetailCraft with no connection later, choose {{ui:Save for offline}} in the app bar while you are online. The button counts up ("Saving 3 of 20") and then reads {{ui:Offline ready}}. The saved copy includes the 3D graphics library (about 2.6 MB) and this help, so once it is saved the 3D view, every export and the help all work offline. Details, and what still needs a connection: [Working offline](common:offline).

:::offline
You are offline. If the app bar shows {{ui:Offline ready}}, opening, saving and exporting work as usual. If it does not, keep this tab open, do not reload it until you are back online, and save your model now.
:::

Cloud saving is not available yet, and there are no desktop builds yet. Back up your `.detailcraft.json` files the way you back up any other work file. More in [Where your files go](common:your-files).

## Unsaved work

DetailCraft keeps the model only in the open tab. If you close or reload the tab with unsaved changes, the browser asks whether to leave the page; its wording is the browser's own. Save with {{key:Mod+S}} often.
