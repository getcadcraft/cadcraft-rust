---
title: Files: open, save and export
summary: The part and assembly files PartCraft opens and saves, every export format and what it keeps, where files go, and what changes offline or between browsers.
section: start
order: 30
keywords: open, save, export, download, file, partcraft.json, assembly file, stl, 3mf, step, dxf, pdf, where are my files, offline, save as
---

PartCraft works on files on your computer. It never uploads a part, and there is no account or cloud storage involved. Cloud saving is not available yet.

## What PartCraft opens

| File | Extension | How |
|---|---|---|
| A PartCraft part | `.partcraft.json` | {{ui:Open}}, {{key:Mod+O}}, or drop it on the 3D view |
| A PartCraft assembly | `.partcraft-asm.json` | The same: PartCraft sees which kind it is |
| A STEP solid | `.step`, `.stp` | Not built yet in this version: it needs an exact kernel, and the status bar says "Opening STEP files: not built yet…" |

STL, DXF, IGES and other programs' own part files cannot be opened (see [What is not built yet](topic:not-built-yet)). How STEP import worked in builds with an exact kernel: [Open a STEP file and build on it](topic:import-step).

Rules when opening:

- A part or assembly file name must end in `.json`, and the file must be under 50 MB.
- Files are checked before anything is shown. A damaged or edited file is refused with a plain reason, for example "This is not a PartCraft file."
- A file saved by a newer PartCraft is refused: "This file was saved by a newer PartCraft (file version 3); this one reads version 2." Older files open.
- If the part on screen has unsaved changes, PartCraft asks first: {{ui:Cancel}}, {{ui:Discard changes}} or {{ui:Save first}}.

## Saving

{{ui:Save}} or {{key:Mod+S}} saves what is on screen:

- On a part: `<part name>.partcraft.json`, the complete history (sketches, relations, dimensions, features, the rollback position), the drawing sheet's section, detail and GD&T notes, and the full text of any STEP solid imported in an earlier build. Features this version cannot build are saved back unchanged. The solid itself is not stored; PartCraft rebuilds it from the history when the file opens.
- In an assembly: `<assembly name>.partcraft-asm.json`, with **every part inside it**. One file carries the whole assembly, so you can send it on its own.

Saving is a **download**. Your browser puts the file in its downloads folder, or asks where to put it if you set it to ask. Saving again makes a new download; the browser may name it `Part1 (1).partcraft.json` rather than replace the old one. Rename or move files in {{pick:mac=Finder|windows=File Explorer|linux=your file manager}} as you like.

Characters other than letters, digits, spaces, `_`, `.` and `-` in the part name become `_` in the file name.

:::tip
Most browsers have a setting to ask where to save each download. Turn it on if you want a "Save as" choice every time: in Chrome and Edge it is under Downloads in the settings; in Firefox under General, Files and Applications; in Safari under General, File download location.
:::

More about where downloads go and how to keep files safe: [Where your files go](common:your-files).

## Exporting

The {{ui:Export}} menu writes other formats. Each is a download, named after the part.

| Menu item | File | What is in it |
|---|---|---|
| {{ui:STL, for 3D printing}} | `<name>.stl` | Binary STL of the solid's triangles, millimeters, **Z up** |
| {{ui:3MF, for 3D printing}} | `<name>.3mf` | 3MF with the solid as one object, millimeters, **Z up** |
| {{ui:STEP AP242 (exact solid)}} | `<name>.step` | Not built yet in this version (grayed): it needs an exact kernel |
| {{ui:STEP AP214 (exact solid)}} | `<name>.step` | Not built yet in this version (grayed) |
| {{ui:STEP (faceted, mesh kernel)}} | `<name>.step` | A solid made of flat faces from the mesh: **round surfaces are many small flat facets**. Millimeters, in PartCraft's own axes (Y up). The STEP to use for other CAD programs in this version |
| {{ui:DXF of the sketch or flat face}} | `<name> - <sketch>.dxf` or `<name> - face.dxf` | DXF R12. A sketch: lines, arcs, circles and points in the sketch's own coordinates, on layers PROFILE, CONSTRUCTION and POINTS. A picked flat face: its outlines as closed polylines (round edges as their facets) |
| {{ui:Drawing (PDF): views, section, detail, GD&T}} | `<name> - drawing.pdf` | The A3 drawing sheet: three views, isometric, section and detail if set, overall sizes, GD&T symbols, title block |

:::note
{{ui:STEP AP242 (exact solid)}} and {{ui:STEP AP214 (exact solid)}} are not built yet in this version: use {{ui:STEP (faceted, mesh kernel)}}. It has the right shape and volume, but every round face is many flat facets, so other programs cannot select a hole as a cylinder. Set **Curves** finer first if that matters.
:::

- Exports are of the part on screen. Export is turned off in an assembly; there is no assembly export yet.
- STL, 3MF, the faceted STEP and the face DXF are always made of facets. The **Curves** setting (in the right panel with nothing selected) sets how fine they are, from 32 to 256 facets per turn. Choose Fine or Very fine before exporting STL or 3MF for a smooth print.
- The PDF drawing uses the reader's built-in Helvetica font. Characters it cannot show print as "?".

Step by step: [Export for printing, STEP and DXF](topic:export-files) and [Make a drawing sheet](topic:drawing-pdf).

## Unsaved work

- PartCraft keeps a recovery copy of the open part or assembly in this browser a moment after each change, so a crash or a closed tab does not lose it: the next time you open PartCraft it asks "Recover your work?". It is not a save: save to a file to keep your work. See [Recover unsaved work](common:recover).
- If you try to close the tab or reload with unsaved changes, the browser asks "Leave site?". That is the browser's own box; PartCraft cannot change its wording.
- {{ui:New}}, {{ui:Open}} and closing an assembly ask before discarding unsaved changes.

## Online and offline

Everything happens on your computer: the mesh kernel (a WebAssembly program) is downloaded with the app and builds geometry locally, and files are read from and written to your disk. No part data goes over the network. See [The mesh kernel](topic:solid-kernels).

To work with no connection at all, save PartCraft for offline use while you are online: choose {{ui:Save for offline}} in the app bar and wait while it counts ("Saving 40 of 212") until it says **Offline ready**. The saved copy includes the 3D library and the mesh kernel, so afterwards modeling, saving, every export and drawings all work offline. If it says **Could not save**, click it to try again while online; **Offline not available** means this browser window cannot keep a copy (private windows and some managed computers). With the copy saved, clicking **Offline ready** offers {{ui:Save again (get the newest version)}} and {{ui:Remove the offline copy}}. Details: [Working offline](common:offline).

:::online
You are online. Even without saving for offline use, once PartCraft has loaded and the status bar chip says "Mesh kernel", you can keep working if the connection drops. Just do not reload the page until you are back online.
:::

:::offline
You are offline. If PartCraft is already open, keep working; saving and exporting still work. If the app bar says **Offline ready**, PartCraft also opens from scratch with no connection; otherwise do not reload this tab.
:::

## Differences between browsers

- **Downloads.** Each browser has its own downloads folder and its own way of asking. If a browser asks whether this site may download files, allow it, or saving and exporting cannot work.
- **Drag and drop** of a file onto the view works in Chrome, Edge, Firefox and Safari.
- **Files from a phone or tablet.** On iPhone and iPad, Open shows the Files app; saved files go to Files, Downloads. On Android they go to the Downloads folder.

There is no difference in what the files contain between browsers or operating systems.
