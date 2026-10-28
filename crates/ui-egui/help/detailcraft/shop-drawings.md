---
title: Make a shop drawing
summary: Preview and download a shop drawing PDF of one beam, column or plate on A4 to A1 or ANSI B to D, with its holes, copes, dimensions and title block.
section: tasks
order: 110
keywords: shop drawing, single part drawing, fabrication drawing, pdf, detail drawing, piece drawing, print
---

DetailCraft draws one sheet per part mark, for I-shapes and plates. The sheet is made on the spot from the model; it is not stored or edited.

## Make one

1. Select a beam, a column or a plate. For an end plate or a base plate, select its connection.
2. Choose {{ui:Shop drawing}} in the {{ui:Output}} group. In the {{ui:Properties}} panel the same button is called {{ui:Shop drawing}} for members and {{ui:Plate drawing}} for connections.
3. A preview opens, titled with the part mark, for example "Shop drawing b1". Under it: the profile, length and quantity, the sheet size, for example "A3 landscape, millimeters". The list next to the title changes the sheet size; the drawing is made again on it (see [General-arrangement and assembly drawings](topic:drawings#choose-the-sheet-size)).
4. Choose {{ui:Download PDF}}. The browser downloads `<name> <mark>.pdf`.
5. Close the preview with the × or {{key:Esc}}.

If {{ui:Shop drawing}} is grayed out, nothing is selected, or the selected part is a channel, angle or tube. Those are not drawn yet.

## What is on the sheet

For an I-shape:

- **Elevation**, looking at the web: the part with its copes notched, web holes as circles and flange holes as hidden (dashed) lines.
- **Top flange plan**, seen from above, when any flange has holes. Bottom-flange holes show dashed.
- **Section** through the profile.
- **Dimensions:** chained hole positions along the part plus the overall length, web hole heights, the gauge, and cope sizes.
- **Hole list:** each group of holes, for example "8 x Ø22 in the top flange".
- **Weld**, when the part has plates welded to it.
- **Title block:** part mark, assembly mark, profile, material, length, quantity, weight each and total, model name and date.

For a plate: the plate face with its holes, and its edge as the section.

## Scales

The scale is picked from a list (1:1 up to 1:200) so the part fits the sheet. A long, shallow part would be too thin to read at one scale, so when the depth would come out under 30 mm on paper the depth is drawn at a larger scale than the length. The view title then says so, for example "length 1:20, depth 1:5 (not to one scale; dimensions govern)". Always read sizes from the dimensions, not by measuring the sheet.

## Things to know

- The quantity is the number of parts with that mark in the model. Marks can renumber after a change; see [Check marks and the bill of materials](topic:marks-and-bom).
- This is one part on its own, without weld symbols; assembly drawings (with weld symbols) and general arrangement drawings are in [Drawings](topic:drawings). The sheet cannot be edited or annotated in DetailCraft. To mark it up, open the PDF in a PDF markup app such as MarkupCraft.
- The title block says "DetailCraft lab build — millimeters".
- Text uses the standard PDF Helvetica font. Characters outside the Western European set print as "?", and the status bar counts them.

## Print it

Open the downloaded PDF in your PDF viewer and print it at 100% (no "fit to page") on paper of the size you chose if you want the stated scales to hold. On a smaller sheet, scale it and rely on the dimensions.
