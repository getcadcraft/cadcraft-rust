---
title: Troubleshooting
summary: Messages BimCraft shows, what causes them, and what to do, for opening files, folders, 3D models, drawings and edits.
section: trouble
order: 10
keywords: problem, error, not working, cannot open, failed, incomplete, corrupt, slow, crash, blank, missing, message, help
---

Messages appear in the status line at the bottom of the stage, in red when something failed. Find the message below.

## Opening files

**"*name* is not an IFC, RVT, RFA, DWG or DXF file."**
BimCraft did not recognize the file. Check the extension and that the file is not empty. PDFs, images and office files do not open.

**".ifczip is not supported yet. Save or export the model as a plain .ifc file."** (also for `.ifcxml`)
Unzip the `.ifczip` (it is an ordinary zip holding an `.ifc`) or export plain IFC.

**"… is a project file BimCraft does not read yet."**
A `.pln`, `.pla`, `.tpl` or `.bpn` file. Export IFC or DWG from it and open that: see [Open .pln and .pla project files](topic:pln-project-files).

**"This IFC file could not be read."**
The IFC file is damaged or not really IFC. Open it in a text editor: a real IFC file starts with `ISO-10303-21;`. Export it again.

**"This file is incomplete: it is … MB, but its own index points to data at least … MB in. …"**
The project or family file was cut off while being copied, uploaded or synced. A cloud-drive file that is "online only" looks the same. A file opened on its own still opens, with this message at the top of the card and "Showing what was recovered; copy the original again for the full model.": the card shows only the parts whose every byte is in the file and lists the rest under **Not in the file**. Make the file available offline in your cloud drive, or copy the original again, and reopen it. In a folder, BimCraft shows the newest complete backup instead.

**"… The file's own list of contents is in the missing part too, so nothing in it can be opened."**
The file was cut off so early that not even its list of contents is there. Nothing can be recovered from this copy. Copy the original again and reopen it.

**"This compound file has no BasicFileInfo stream, so it is not a Revit file."**
The file is a compound file of another kind (old office documents use the same container). Check you picked the right file.

**"Still working on the last one…"**
BimCraft is still busy with the previous file or folder. Wait for the status line to finish, then try again.

**The file chooser greys out my file.**
The chooser only offers `.ifc`, `.rvt`, `.rfa`, `.rte`, `.rft`, `.dwg` and `.dxf`. Drag other files onto the page instead, and BimCraft will tell you whether it can read them.

## Opening folders

**"This folder has more than 20,000 files. Pick a smaller folder, such as one project."**
You picked a folder too high up, such as a whole drive. Pick one project folder.

**"The browser did not give access to *name*. Choose it again with Open a folder."**
You clicked {{ui:Reopen}} on a recent folder and the browser's permission was refused or has lapsed. Choose the folder again, or drag it in.

**The browser says it will "upload" files.**
That is the browser's own wording for handing files to a page. BimCraft does not send them anywhere. Drag the folder in to avoid the question.

**Nothing happens on a phone or tablet.**
Many mobile browsers cannot choose folders. Open single files, or use a computer.

## The IFC engine loads the first time

**The status line stays on "Starting the IFC engine…", or shows an error such as "Failed to fetch".**
The IFC engine could not be downloaded. You may be offline, or a firewall or content blocker is stopping it.

:::offline
You are offline. If the app bar's button does not say {{ui:Offline ready}}, the IFC engine was never saved in this browser. Reconnect, open the file again, then click {{ui:Save for offline}} so it works offline next time. See [Online and offline](topic:online-offline).
:::

:::online
Reload the page and try again. On a work network, ask your IT team to allow the site's `.wasm` files.
:::

## Saving for offline use

**The button says {{ui:Could not save}}.**
Some files did not download, usually because the connection dropped. Check your connection and click the button again. Any earlier offline copy is kept until a new one is complete.

**The button says {{ui:Offline not available}}.**
This window cannot keep an offline copy, for example a private window or a managed work computer. See [Working offline](common:offline).

**The button stays on "Saving *n* of *m*" for a while.**
BimCraft's copy is about 11 MB, most of it the IFC engine. Keep the tab open until it says {{ui:Offline ready}}.

**BimCraft opened offline but looks out of date.**
Your saved copy is from an earlier visit. When you are online, click {{ui:Offline ready}} and choose {{ui:Save again (get the newest version)}}.

## 3D view

**The model is not where I expected, or it is tiny.**
Press {{key:F}} or click {{ui:Fit}}. Fit always frames everything in the model, including storeys you have unticked and stray elements far from the building, so the building can look small. Zoom in from there.

**Clicking does not pick anything.**
Hold the mouse still while you click: a small drag orbits. Elements on unticked storeys cannot be picked. With {{ui:Spaces}} on, the see-through spaces are picked before what is inside them.

**The {{ui:Edges}} button is greyed out: "Edges are off for very large models".**
The model has more than 1.5 million triangles. Edges are left off to save memory.

**The tab turns slow, white, or shows the browser's crash page.**
The model is too large for the memory the browser gives the tab. Close other tabs and reload. Export a smaller IFC: one building, one discipline or fewer storeys. See [Browsers, devices and memory](topic:platforms).

**The 3D view stays blank.**
WebGL may be turned off or blocked. In the browser's settings, turn hardware acceleration (or graphics acceleration) on and restart the browser.

## Editing IFC

**"Use a number."** The value is numeric. Type a plain number without units, such as `2.4`.

**"Use a whole number."** The value is a count or integer. Remove the decimals.

**"Only single-value properties can be edited."** Lists, enumerations and quantities are read-only.

**{{key:Mod+S}} opens the browser's "Save page" window.**
There are no unsaved changes, so BimCraft left the key to the browser. Close the window.

**{{key:Mod+Z}} does nothing.**
It works only while you have unsaved changes and are not typing in a field. After a save, there is nothing left to undo.

**The {{ui:Changes}} list still shows edits after I discarded them and opened a drawing or a folder.**
When you answer the "Discard them?" question with OK and then open something other than an IFC model, the old edits are kept in memory and {{ui:Save as IFC}} would still save them. To really start fresh, open the original IFC again, or reload the page.

**My edit changed more elements than I picked.**
You changed a property in a set marked "(type)". Type properties apply to every element of the type. Click {{ui:Undo}}. See [Change a type property for every element of a type](topic:type-vs-instance).

## Drawings

**"*name* could not be read. It may be damaged, or saved in a format the reader does not handle yet (…)."**
Save the drawing from the program that made it as an older DWG (2018 or earlier) or as DXF, and try again.

**"This drawing has nothing to show."** or **"*name* is an empty drawing: it holds no lines, text or blocks."**
The drawing is empty, or holds only things BimCraft does not draw yet (see **Not drawn yet** on the right).

**"This drawing is very large; only part of it is shown."**
BimCraft stopped drawing to stay responsive. Turn off layers you do not need, or open the sheets you need from a smaller drawing.

**Parts of a sheet are missing.**
Linked drawings may be missing: look under **Linked drawings** on the right. See [Find linked drawings and pictures](topic:dwg-linked-files). Layers may also be off: click {{ui:All on}}.

**A picture shows as an empty frame.**
Its file was not found, or it is a TIFF ("TIFF cannot be shown in a browser"). Add a PNG or JPEG copy with {{ui:Add pictures…}}.

**Scrolling on my trackpad zooms instead of moving the drawing.**
That is how drawings work in BimCraft. Click and drag to pan.

## Still stuck

Read [What is not built yet](topic:not-built-yet): the thing you are looking for may not exist in this build.
