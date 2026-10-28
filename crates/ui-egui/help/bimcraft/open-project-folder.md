---
title: Open a whole project folder
summary: Point BimCraft at a project folder to see every model, its backups and links at once, and let the Start here box suggest which file to work on.
section: tasks
order: 80
keywords: folder, directory, project folder, network drive, server, start here, which file, backups, links, xrefs, recent folders, drag folder
---

## What you'll do

Open a whole project folder, such as the one you were handed for a job, and let BimCraft sort it out: which models are there, which copies are backups, what links to what, which files are damaged, and which file you most likely want to open.

Only the few kilobytes each model card needs are read, so a folder on a network drive or a synced cloud folder is fine.

## Steps

:::chromium
1. Click {{ui:Open a folder}} on the left (or in the middle of the empty screen).
2. Choose the project folder in the window that opens and click **Select** (or **Upload** or **Open**, depending on your system).
3. Click **View files** when the browser asks "Let site view files?". This only lets the tab read the folder; nothing is uploaded.
4. Wait while the status line counts: "Listing the folder: … files", then "Checking Revit files: … of …".
:::

:::firefox
1. Click {{ui:Open a folder}} on the left (or in the middle of the empty screen).
2. Choose the project folder in the window that opens and confirm. Firefox may ask you to confirm that the page can read the folder's files; they still stay on your computer.
3. Wait while the status line says "Reading the folder: … files", then "Checking Revit files: … of …".
:::

:::safari
1. Click {{ui:Open a folder}} on the left (or in the middle of the empty screen).
2. Choose the project folder in the window that opens and click **Upload**. Despite the word, nothing leaves your Mac: the files are only handed to the tab.
3. Wait while the status line says "Reading the folder: … files", then "Checking Revit files: … of …".
:::

You can also **drag the folder** from {{pick:mac=Finder|windows=File Explorer|linux=your file manager}} onto the page. A dragged folder opens without any permission prompt in every browser.

## What you should see

The folder report fills the middle:

- The folder name, counts of models, CAD drawings, IFC models and files, and the total size.
- Red or amber notes if any files are incomplete or any linked files are missing from the folder.
- The {{ui:Start here}} box: the file BimCraft suggests you open, with the reasons.
- **Models**: one row per model, with its backups grouped under it.
- **Linked and referenced files**, **IFC models**, **CAD drawings** and **Set aside**, when the folder has any.

On the left, the {{ui:Folder}} panel sums it up; **Problems** says "None" when every copy is sound.

## Use the Start here box

The box names one file and says why, for example:

- "Newest saved work in this folder (Mar 3, 2025)."
- "*Consultant.rvt* is linked into other models, so it is background, not the file to edit."
- "*Tower.rvt* looks like the obvious choice, but it is damaged; its last good copy is from …"
- "This is a central model: in Revit, open it with "Create New Local" and work in the local copy."

1. Read the reasons.
2. Click {{ui:Open its card}} to read that file's card, or {{ui:Show it in the list}} to find its row in **Models**.
3. Check with whoever last worked on the project before you rely on it. The box says so too: it is a best guess from names, dates and links.

## Open files from the report

1. Click a model's name to open its card. The card checks each link against the folder.
2. Click {{ui:Back to}} and the folder name at the top of the card (or {{ui:Back to the folder}} on the left) to return.
3. Click any IFC file under **IFC models** to open it in 3D.
4. Click any DWG or DXF under **CAD drawings** to open it in 2D. Its linked drawings and pictures are found in the folder automatically.

## Reopen a folder later

:::chromium
Folders you opened with {{ui:Open a folder}} or by dragging are listed under {{ui:Recent folders}} on the left. Click {{ui:Reopen}} and the folder name to open it again without choosing it. After a browser restart, Chrome or Edge asks once more; choose "Allow on every visit" to stop it asking. The list is kept in this browser only, never sent anywhere. Click × to forget a folder.
:::

:::firefox safari
This browser cannot remember folders between visits. Choose the folder again with {{ui:Open a folder}}, or drag it in.
:::

## Common mistakes

- **Picking a folder that is too big.** Over 20,000 files: "This folder has more than 20,000 files. Pick a smaller folder, such as one project." Pick the project folder, not the whole drive.
- **Picking the parent of several projects.** The {{ui:Start here}} box then chooses across projects. Pick one project.
- **Revit's own folders look ignored.** `*_backup`, `*_log` and `Revit_temp` folders are housekeeping and are listed under **Set aside**, not read.
- **Expecting a model to open in 3D from its card.** A project file is a card only. Open its IFC export.
- **Phones and tablets.** Many mobile browsers cannot choose folders at all. Use a computer.

See [Check a folder for damaged copies, missing links and leftovers](topic:check-folder-health) and [Folder report: roles, statuses and links](topic:folder-roles).
