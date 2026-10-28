---
title: Online and offline
summary: What BimCraft loads from the site, how Save for offline makes IFC models and DWG drawings open with no connection, and why nothing you open is ever uploaded.
section: reference
order: 60
keywords: offline, online, internet, connection, no network, airplane, site visit, save for offline, offline ready, upload, privacy, wasm, webassembly, ifc engine, dwg, cloud
---

BimCraft reads your files inside the browser tab. Opening, inspecting, editing and saving never send your files anywhere, online or offline. [Working offline](common:offline) explains how offline copies work in every app; this page covers what is particular to BimCraft.

## What BimCraft loads from the site

| Part | When it loads without an offline copy | Size |
|---|---|---|
| The app itself, with the 3D viewer and the IFC engine's code | When you open BimCraft | Several megabytes |
| The IFC engine's WebAssembly part | The first IFC file you open in the tab: "Starting the IFC engine…" | About 1.6 MB |

DWG and DXF drawings, project and family files and folders need nothing extra: their readers are part of the app.

## Save BimCraft for offline use

1. Open BimCraft while you are online.
2. Click {{ui:Save for offline}} in the app bar at the top.
3. Watch the button count up: "Saving 40 of 212", for example. BimCraft's copy is about 11 MB, because it includes the IFC engine.
4. Wait for {{ui:Offline ready}}.

Once it says {{ui:Offline ready}}, IFC models, DWG and DXF drawings, project files, folders and this help all open with no connection. You do not need to open an IFC or a DWG first to "warm up" the readers.

To update or remove the copy, click {{ui:Offline ready}} and choose {{ui:Save again (get the newest version)}} or {{ui:Remove the offline copy}}.

## What the button says

| Button | Means |
|---|---|
| {{ui:Save for offline}} | No offline copy in this browser yet. Click to save one |
| "Saving *n* of *m*" | Saving; keep the tab open until it finishes |
| {{ui:Offline ready}} | Saved. Hover to see when; click for the menu |
| {{ui:Could not save}} | Some files did not download. Check your connection and click to try again. The last good copy, if any, is kept |
| {{ui:Offline not available}} | This window cannot keep an offline copy (a private window or a managed computer). See [Working offline](common:offline) |

:::offline
You are offline.

- If the button says {{ui:Offline ready}}, every kind of file opens as usual.
- If it does not, only what this tab has already loaded works: a reader that never loaded in this tab cannot start, and opening that kind of file shows an error from the browser in the status line. Do not reload the tab until you are online again.
:::

## What never needs a connection

Once BimCraft is open in the tab, these work offline even without a saved copy:

- Reading project and family files and checking folders.
- Picking elements, reading properties and editing IFC values in a model that is already open.
- {{ui:Save as IFC}} and {{ui:Download CSV}}: both are downloads made in the tab.

## What is not available yet

- **Cloud saving.** Files are saved to your computer only.

See [Where your files go](common:your-files).
