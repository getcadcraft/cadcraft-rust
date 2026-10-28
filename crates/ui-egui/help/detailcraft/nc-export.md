---
title: Send NC files to the shop
summary: Export DSTV NC files for every part mark (I-shapes, channels, angles, HSS, plates), zipped, and check them before they reach a machine.
section: tasks
order: 120
keywords: nc, nc1, dstv, cnc, machine, beam line, drill line, plate cutting, export, fabrication, zip
---

DetailCraft writes one DSTV NC file (`.nc1`) per part mark, for I-shapes, channels, angles, rectangular and round HSS, and plates, and zips them together. Beam lines and plate machines read these files to saw, drill and cut.

## Export

1. Clear the {{ui:Problems}} tab first: a connection that could not be built adds no holes.
2. Save the model ({{key:Mod+S}}), so the NC files match a saved model.
3. Choose {{ui:NC files}} in the {{ui:Output}} group.
4. The browser downloads `<name> NC.zip`. The status bar says how many files it holds, and lists any marks it did not write (an angle or a round tube with a cope).

Inside the zip each file is named by its part mark: `b1.nc1`, `c1.nc1`, `p1.nc1` … The zip is uncompressed and opens with any zip tool.

If the model has no parts, the status bar says so.

## What each file contains

- **Header (ST):** the model name as the order, the part mark as drawing and piece, phase 1, the material grade, the quantity, the profile and its dimensions, mass per meter and paint surface per meter. Saw cuts are square.
- **Outer contours (AK):** the web outline with any copes notched out, and each flange outline shortened where a cope removes it.
- **Holes (BO):** every hole with its face, position and diameter, including the holes made by end plates and base plates.

Field by field, with the face and coordinate rules: [NC file fields](topic:nc1-fields).

## What is not written

- Angles and round tubes with a cope: no NC file. Their marks are listed in the status bar.
- Holes in round tubes (DetailCraft does not make any).
- Bevels, inner contours, part markings (scribing) and saw angles other than square.
- Slotted holes (DetailCraft has round holes only).

## Check before machining

DetailCraft's NC files are read back by its own test suite, but they have not been run on your shop's machines.

1. Open the files in your shop's NC viewer or the machine software and compare them with the shop drawings.
2. Check one part of each kind on the machine before running a batch.
3. Check how your software reads the flange y-coordinates and the faces of channels: DetailCraft measures flange y from the behind edge, and lays channels on their back as the standard draws them (see [NC file fields](topic:nc1-fields#faces-and-coordinates)).
