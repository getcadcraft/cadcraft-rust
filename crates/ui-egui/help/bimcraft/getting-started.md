---
title: Getting started with BimCraft
summary: What BimCraft does today, how to open your first model, look at it in 3D, change one value and save a copy.
section: start
order: 10
keywords: start, first run, open, ifc, rvt, dwg, quick start, introduction, viewer, bim
---

BimCraft is the building-model app of the CadCraft family. In this build it **opens, reads and inspects** building files, and it **edits the data in IFC models**. It does not draw or model new elements yet. Think of it as a model viewer and checker that also lets you correct names, tags and property values in an IFC file.

What it opens:

| File | What you get |
|---|---|
| IFC (`.ifc`) | The full model in 3D: orbit, storeys, element properties, and editing of names and property values |
| Project and family files (`.rvt`, `.rfa`, `.rte`, `.rft`) | A file card: the release it was saved in, project information, the preview picture and linked files. No 3D. |
| DWG and DXF | The drawing in 2D: model space and every sheet, layers, linked drawings, print view, and an equipment schedule for tagged foodservice drawings |
| A project folder | A report of every model in the folder: backups, links, damaged copies, and a suggestion of which file to start with |

Everything happens in your browser, on your computer. **Nothing is uploaded.** See [Files BimCraft opens and saves](topic:files) for the details.

## Open BimCraft

1. Open BimCraft in a current browser. Chrome, Edge, Firefox and Safari all work; [Browsers, devices and memory](topic:platforms) lists the differences.
2. Wait for the empty screen that says "Open a building model."

:::note
BimCraft runs in the browser, and on the desktop inside CadCraft Studio, with the other apps. Cloud saving is not available yet: you open files from your computer and save copies back to it.
:::

## Open your first model

The quickest way to see what BimCraft does is an IFC file.

1. Click {{ui:Choose a file}} in the middle of the screen.
2. Pick an `.ifc` file in {{pick:mac=Finder|windows=File Explorer|linux=your file manager}} and confirm.
3. Wait while the status line at the bottom of the screen says "Starting the IFC engine…", then "Building geometry".

You should see the model in 3D in the middle, a {{ui:File}} panel and a {{ui:Storeys}} list on the left, and an empty {{ui:Properties}} panel on the right. The status line ends with "Opened" and the number of elements.

:::tip
You can also drag a file from {{pick:mac=Finder|windows=File Explorer|linux=your file manager}} straight onto the page. The screen shows "Drop to open" while you drag.
:::

:::offline
The IFC engine loads from the site the first time you open an IFC model, unless BimCraft was saved for offline use. DWG and DXF drawings, project files and folders need nothing extra: their readers are part of the app. If the app bar's button says {{ui:Offline ready}}, everything opens offline. If not, IFC models will not open until you reconnect unless one already opened in this tab. See [Online and offline](topic:online-offline).
:::

:::online
Going somewhere without a connection? Click {{ui:Save for offline}} in the app bar and wait for {{ui:Offline ready}}. The saved copy (about 11 MB) includes the IFC engine. See [Working offline](common:offline).
:::

## Look around

:::mouse
1. Drag with the left mouse button to orbit around the model.
2. Scroll the wheel to zoom.
3. Drag with the right mouse button to pan. On a Mac trackpad, hold {{key:Shift}} and drag instead.
4. Press {{key:F}} or click {{ui:Fit}} to see the whole model again.
:::

:::touch
1. Drag with one finger to orbit.
2. Pinch with two fingers to zoom, and move two fingers together to pan.
3. Tap {{ui:Fit}} to see the whole model again.
:::

## Do one real thing: fix a tag

1. Click a wall, door or any element in the model. It turns blue, and its properties appear on the right.
2. In the {{ui:Properties}} panel, click the value next to **Tag**.
3. Type the new tag and press {{key:Enter}}.
4. Look at the {{ui:Changes}} section that opened on the left: your change is listed with the old and new value.

## Save a copy

1. Click {{ui:Save as IFC}} in the {{ui:Changes}} section, or press {{key:Mod+S}}.
2. Find the new file in your downloads folder. It is named after the original with " (edited)" added, for example `Clinic (edited).ifc`.

Your original file is never changed. [Change IFC properties and save an edited copy](topic:edit-ifc-properties) explains editing in full.

## Where to go next

- [The BimCraft screen](topic:workspace): every panel by name.
- [Coming from other BIM and drawing programs](topic:coming-from): what feels different if you are used to Revit.
- [Open a whole project folder](topic:open-project-folder): point BimCraft at a project folder and let it sort the files out.
- [View a DWG or DXF drawing](topic:view-dwg): sheets, layers and print view.
