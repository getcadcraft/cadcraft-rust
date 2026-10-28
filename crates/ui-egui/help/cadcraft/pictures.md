---
title: Pictures in a drawing
summary: Place a PNG or JPEG picture, clip it to a rectangle or a polygon, show the pictures a DWG uses, and plot them sharp.
section: tasks
order: 87
keywords: picture, image, imageattach, iat, image, imageclip, clip, png, jpeg, jpg, logo, photo, raster, imagedef, xref, plot, pdf, resolution
---

## What you will make

A company logo placed in a title block, clipped to the part you want, and a PDF in which it prints at its own resolution.

## Place a picture

1. Type {{cmd:IMAGEATTACH}} (or `IAT`) and press {{key:Enter}}. Pick a PNG or JPEG file.
2. `Specify the lower-left corner`: click, or type a point.
3. `Specify its width`: the width in drawing units. {{key:Enter}} keeps one unit per pixel.
4. `Specify its rotation angle`: {{key:Enter}} for none.

The picture is one object: select it by its frame, and move, copy, rotate, scale or erase it like any block.

:::note
The picture file stays with the drawing while it is open. A saved DWG names the file, as other programs do; it does not hold the pixels. Keep the picture next to the drawing.
:::

## Show the pictures a DWG uses

A DWG stores only where its pictures were, so CadCraft shows each one as a frame with its file name.

1. Type {{cmd:IMAGE}} (or `IM`). The command line lists each picture file and whether it shows.
2. Pick the files (several at once). A file whose name contains the one the drawing names also counts, because exports often add a project prefix.

## Clip a picture

1. Type {{cmd:IMAGECLIP}} (or `ICL`) and select the picture.
2. Choose `R` and click two corners, or `P` and click the points of a polygon, then {{key:Enter}}. `D` removes the clip.

## Plot pictures

Pictures go into the PDF as pictures, at their own resolution and under the lines. In the {{ui:Plot}} dialog, {{ui:Pictures}} can cap them at 600, 300 or 150 dpi to make the file smaller. A JPEG goes in as it is.

## What is saved where

| Format | Pictures |
|---|---|
| DWG (every version) | As pictures (IMAGE), with their clip |
| DXF | As their frames with the file name; the save says so |

## Common mistakes

| You see | Why | Fix |
|---|---|---|
| A frame with a file name | CadCraft does not have the file | {{cmd:IMAGE}}, then pick the file |
| "That is not a picture" | The object picked is not a picture | Pick the picture's frame |
