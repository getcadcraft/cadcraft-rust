---
title: Find linked drawings and pictures
summary: Show a drawing's linked drawings (xrefs) and title-block pictures, by opening its folder or adding the files by hand.
section: tasks
order: 120
keywords: xref, external reference, linked drawing, attach, overlay, image, picture, logo, title block, missing xref, etransmit, reference manager
---

## What you'll do

Make a drawing complete: show the drawings it links to (external references, or xrefs) and the pictures it uses, such as a logo in the title block.

## The easy way: open the folder

1. Click {{ui:Open a folder}} and choose the folder that holds the drawing and its links, for example the whole `Cad` folder of a project.
2. In the folder report, scroll to **CAD drawings**.
3. Click the drawing.
4. Look at the {{ui:Properties}} panel on the right, under **Linked drawings** and **Images**.

BimCraft looks for each linked drawing by its name ("*name*.dwg"), in the same folder as the drawing first, then anywhere in the folder you opened. Pictures are found the same way, and also by a similar name: an eTransmit or Revit export often adds the project's name in front, so `logo.png` is found as `Tower-logo-5425051.png`.

## The other way: add files by hand

1. Open the drawing on its own.
2. On the right, under **Linked drawings**, click {{ui:Add linked drawings…}}. Under **Images**, the button is {{ui:Add pictures…}}; both open the same chooser.
3. Choose the linked `.dwg` files and the picture files. You can choose several at once.
4. Wait while BimCraft opens the drawing again with them.

## What you should see

Each linked drawing and picture is listed with a label:

| Label | Means |
|---|---|
| **Shown** | Found and drawn. The path under it says where it came from, or "added by you". |
| **Shown, similar name** | A picture found by a similar name. Check it is the right one. |
| **Not found** | Not in the folder or the files you added. A missing picture shows as its empty frame. |
| **TIFF cannot be shown in a browser** | The picture is a TIFF file, which browsers do not draw. Save it as PNG with the same name and add it. |
| **Could not be read** | The file was found but could not be opened as a picture. |

The {{ui:Drawing}} panel on the left shows "*n* of *m* loaded" for linked drawings. Linked drawings' layers appear in {{ui:Layers}} as "*link name*|*layer*".

## Common mistakes

- **Opening only the drawing.** "This drawing shows its own content only until its linked drawings are added. Opening the whole project folder finds them automatically."
- **Opening a folder too small.** If the links live in a sibling folder, open the parent folder instead.
- **Renamed links.** BimCraft matches linked drawings by name. A link renamed on disk is not found; add it by hand.
- **Links of links.** BimCraft loads the drawings the open drawing links to. Drawings linked inside those are not looked for, so they usually do not show in this build.
