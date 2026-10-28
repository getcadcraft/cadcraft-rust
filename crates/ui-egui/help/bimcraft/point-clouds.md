---
title: Show a point cloud beside the model
summary: Open a laser scan (LAS, LAZ, XYZ or PTS) in the 3D view on its own or with an IFC model, color it, cut a thin slice through it, measure to its points, and hand it to the drafting app to trace as-built lines.
section: tasks
order: 27
keywords: point cloud, laser scan, scan, lidar, las, laz, xyz, pts, as-built, as built, survey, registration, slice, plan cut, snap, measure, trace, scan to bim
---

## What you'll do

Look at a laser scan of a building next to its model: check that what was built matches the design, measure between scanned points, and cut a thin slice to read walls, doors and columns from.

## Steps

1. Open the cloud: {{ui:Open}} a `.las`, `.laz`, `.xyz` or `.pts` file, or drop it on the page. With a model open, the cloud joins the model in the 3D view; opening a cloud never closes the model or its unsaved changes. Without a model, the cloud opens on its own.
2. The cloud bar appears under the toolbar. It says how many points show and how many the file has.
3. Choose {{ui:Color by}}: the file's own colors (when the scanner stored them), {{ui:Height}}, {{ui:Intensity}}, {{ui:Class}} (LiDAR classes such as ground, vegetation and building) or {{ui:One color}}. {{ui:Size}} makes the points bigger or smaller.
4. Click {{ui:Plan}} under {{ui:Slice}} to keep only a thin band 1.2 m above the lowest point, seen from the top: walls, door openings and columns read like a floor plan. {{ui:Front}} and {{ui:Side}} cut upright bands for elevations and sections. Move the band with {{ui:At}}, and change {{ui:Thickness}} (meters). {{ui:Off}} shows the whole cloud again.
5. Click {{ui:Measure}} in the toolbar. The pointer snaps to the cloud's points (the label says "point cloud"), so you can measure from wall to wall in the slice, or from a model element to the scan. A model corner or edge within reach still wins.
6. With {{ui:Measure}} › {{ui:Location}}, a cloud point gives its latitude and longitude from the cloud's own coordinate system, when the file names one.
7. {{ui:Hide}} hides the cloud for a moment; {{ui:Remove}} takes it out. {{ui:Show}} › {{ui:Point cloud}} hides or shows the bar.

## Where the cloud goes

{{ui:Place}} in the bar says how the cloud lines up with the model:

- **Model coordinates**: the cloud's numbers are the model's own (a scan registered to the design's coordinates).
- **Map coordinates**: the model has a map conversion (IfcMapConversion) and the cloud is in the same projected system: the cloud is placed through it. If the two name different systems, the status line says so.
- **Centered on the model**: when neither puts the cloud near the model, its middle goes on the model's middle and its lowest point on the model's floor. Pick another choice to override it.

When the file does not say its units, BimCraft reads meters and offers {{ui:Units}} in the bar.

## Trace the as-built lines

Click {{ui:Trace in CadCraft}} to hand the cloud to the drafting app in a new tab. It opens there under the drawing, where a plan slice and object snap let you draw the walls over the scan. See the CadCraft topic on point clouds.

## How many points

A scan can hold hundreds of millions of points. BimCraft reads every point once and keeps an even share of them to draw: {{ui:Show}} in the bar sets how many (1, 2, 5 or 10 million). More points need more memory and a faster graphics card. Counts per LiDAR class are of every point in the file.

:::note
Files are read on this computer; nothing is uploaded. LAS 1.0–1.4 and LAZ are read (LAZ files up to 1 GB, held in memory while they are read); text clouds are `x y z`, with color (`r g b`) and intensity columns when they are there. E57 files are not read yet.
:::
