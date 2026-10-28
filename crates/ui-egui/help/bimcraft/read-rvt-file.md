---
title: Read a project or family file
summary: Open an .rvt, .rfa, .rte or .rft file and read its release, worksharing, project information, family parameters, links and preview picture, without the program that made it.
section: tasks
order: 60
keywords: rvt, rfa, rte, rft, project file, family, release, version, which version, build, worksharing, central, project information, preview, thumbnail, links
---

## What you'll do

Find out what is in a project or family file without opening it in Revit: which release saved it, whether it is workshared, the project information, and what it links to. Useful when a file arrives from someone else and you need to know what release to open it in.

## Steps

1. Click {{ui:Choose a file}}.
2. Choose an `.rvt`, `.rfa`, `.rte` or `.rft` file.
3. Wait a moment. The status line says "Reading the file…" and then "Opened *file name*".
4. Look at the top of the card: the preview picture saved in the file, the kind of file and the release it was saved in, and the project name.
5. Scroll down to read the sections of the card.

## What you should see

The card in the middle, with these sections when the file holds them:

| Section | What it tells you |
|---|---|
| File | File name, size, kind (project, project template, family or family template), the release and build it was saved in, worksharing, the central model's path, where it was last saved, who saved it, locale and document GUID |
| Project information | The project's own fields, such as name, number, client, address and status. Fields still at their new-project defaults are left out. |
| Family, Family: … | For a family: its name, category, release, last update and its parameters, group by group |
| Linked and referenced files | Every linked model, CAD link, image and keynote table the file points to, with its saved path |

On the left, the {{ui:File}} panel repeats the key facts and adds **Read**: how much of the file BimCraft actually read, usually about 40 KB of a file many megabytes in size. On the right, the {{ui:Properties}} panel says element properties need the 3D model.

## Why there is no 3D

The 3D model and the elements inside a project file are stored in an undocumented format. BimCraft reads only the parts of the file that are documented or plainly readable: the file information, project information, family data, the link list and the preview picture. It does not guess at the rest. To see the model in 3D and read element properties, export IFC from the program that made the file. See [Get a project file into 3D through IFC](topic:rvt-to-ifc).

## Common mistakes

- **Expecting 3D.** See above.
- **Links show only their saved path.** A file opened on its own cannot check whether its links exist. Open the folder it lives in instead, and BimCraft marks each link as found or not: [Open a whole project folder](topic:open-project-folder).
- **"This file is incomplete…"** The file was cut off while being copied, uploaded or synced. A cloud-drive file that is still "online only" does this too. BimCraft still opens it and shows only what was recovered: an amber notice at the top of the card, a **Recovered from this file** section listing the parts whose every byte is in the file, and **Not in the file** for the rest. Nothing is filled in for the missing part, so the release, preview picture or project information can be missing when they were stored past the cut. If even the file's list of contents was cut off, nothing can be opened. Copy the original again and reopen it for the full file.
- **"No preview picture in this file".** Some files are saved without one. That is normal.
- **Opening a backup by mistake.** Files named like `Tower.0004.rvt` are numbered backups. Opening the folder groups them with their model.
