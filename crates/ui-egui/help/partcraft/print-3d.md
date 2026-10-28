---
title: Get a part ready for 3D printing
summary: Check that a part or assembly is closed, fits your printer and stands on a base, then download it as STL or 3MF for your slicer.
section: tasks
order: 131
keywords: 3d print, 3d printing, printer, print, slicer, stl, 3mf, watertight, closed, manifold, bed, bed size, build plate, overhang, supports, base, millimeters, export, threads, clearance
---

A slicer needs a closed shape: no holes, every face pointing out, nothing passing through itself. Parts PartCraft builds are closed by construction, so the check mostly tells you the size, whether the part fits your printer's bed, whether it stands on a flat face, and how much of it overhangs.

## Check and download

1. Optional, once: set your printer's bed in {{ui:Settings}} › PartCraft › **3D printer**: width, depth and height in millimeters. Leave them empty to skip that check.
2. Choose {{ui:Export}}, then {{ui:Check for 3D printing (STL or 3MF)…}}.

   The dialog says "Ready to print" or "Not ready to print", lists anything wrong, and gives the size on the bed (X × Y × Z, in millimeters), the volume and the number of triangles.

3. Click {{ui:Download 3MF}} or {{ui:Download STL}}. The file is the same as {{ui:Export}} › {{ui:3MF, for 3D printing}} or {{ui:STL, for 3D printing}}.

In an assembly, the check and the files take the whole assembly, every copy where it is placed.

The part is checked standing as the export writes it: what points up in PartCraft (square to the Top plane) points up on the bed. A part sketched on the Top plane lies flat.

## What the dialog can say

**Not ready to print** (a slicer may refuse it or print it wrong): holes (edges with a face on one side only), edges shared by three or more faces, faces facing opposite ways, a body inside out, or a body passing through itself. A part built in PartCraft should never show these; if one does, see [Troubleshooting](topic:troubleshooting).

**Warnings** (it prints, but have a look first):

- **Larger than your printer's bed**, even turned 90° on it.
- **Nothing lies flat at the bottom**: turn the part in your slicer so a face rests on the bed, or it will need supports or a raft.
- **Leans out more than 45° from upright**: those faces need support, standing as it is. Another way up may need less.
- **Separate bodies overlap** or **meet face to face**: slicers usually merge them. {{ui:Combine}} in Features › More joins them into one body. See [Bodies](topic:bodies).
- **Under 1 mm across**: check the part's sizes.

## Threads that print

A modelled thread prints as a real thread. Give it some **Clearance** so a printed bolt goes into a printed nut: see [Threads](topic:threads).

## Smooth round faces

STL and 3MF are made of triangles. For smoother round faces, click empty space so nothing is selected and set **Curves** to **Fine (128 per turn)** or **Very fine (256 per turn)** in the right panel before you export. More triangles make bigger files.

## Not checked

Wall thickness, details smaller than your nozzle can draw, and how the slicer will place supports. Your slicer shows those.
