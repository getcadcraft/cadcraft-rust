---
title: Issues and BCF files
summary: Save a view of the model as an issue — camera, picked elements, hidden ones, section — share it as a BCF file with other coordination tools, and open their BCF files to see their issues.
section: tasks
order: 29
keywords: bcf, bcfzip, issue, issues, topic, viewpoint, clash report, coordination, comment, snapshot, open bcf, save bcf, export, import
---

## What you'll do

Point at a problem in the model and keep the view that shows it, then hand it on in a BCF file, the format other coordination tools read and write.

## Steps

1. Open an IFC model and set up the view: orbit to it, pick the elements involved, hide what is in the way, cut it with a section plane or box.
2. Click the {{ui:Issues}} tab on the right, then {{ui:New issue from this view}}.
3. Type a title, and a comment if you like; choose a status; give your name or e-mail (signed in, your account's name is filled in; see [BimCraft settings](topic:settings#your-name-on-issues)). Click {{ui:Create issue}}.
4. Click {{ui:Show}} on an issue to put its view back.
5. Click {{ui:Save BCF}} to download every issue as one `.bcf` file.
6. To see issues someone else made, click {{ui:Open BCF…}} and choose their `.bcf` or `.bcfzip` file.

## What you should see

- Each issue in the list with a picture of its view, its title, status, author and date.
- {{ui:Show}} puts back the camera, the picked elements, what was hidden (or the only things shown) and the section. Elements are found by their GlobalId in every open model; the status line says if some are not there.
- A section plane or box from another tool is shown when it lines up with the model's axes; tilted or extra planes come back as a section volume (up to 6 planes). An issue keeps the first box of a volume of several boxes.

## Good to know

- The file is BCF 2.1 and keeps the view in the model's own coordinates, so other tools put the camera in the same place.
- Issues stay in this tab until you save them. Opening another model keeps the list; open the model the issues are about before you show them.
- A parallel (orthogonal) camera from another tool is shown with this viewer's perspective camera, backed off so the same part of the model fills the view.

## Common mistakes

- **Show moved the camera somewhere odd.** The issue was made on another model or with other coordinates. Open the right model first.
- **The file will not open.** It is not a BCF zip, or it is damaged; the status line says why.

## Next

- [Hide, isolate and see through elements](topic:hide-and-isolate)
- [Cut an IFC model with a section plane](topic:section-plane)
