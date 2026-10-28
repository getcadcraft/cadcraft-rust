---
title: Plot to PDF
summary: Make a PDF of a sheet at full size, or of the model at a scale or fitted to the paper, and print it from the browser.
section: tasks
order: 130
keywords: plot, print, pdf, publish, exportpdf, paper, scale, monochrome, line weight, pen, page setup, plot window
---

## What you will make

A PDF of a sheet at full size, then a quick PDF of the model at 1:100, and a printout.

The PDF is made inside your browser; nothing is uploaded. It is a vector PDF: lines stay sharp at any zoom, and text can be searched.

## Plot a sheet

1. Click the sheet's tab under the drawing.
2. Press {{key:Mod+P}}, click {{ui:Plot}}, or type `PLOT` and press {{key:Enter}}.

The {{ui:Plot to PDF}} dialog opens. It says which sheet it plots and on what paper, "at full size … as you see it". The preview on the right shows the sheet.

3. In {{ui:Ink}}, choose {{ui:Black (monochrome)}} for black lines, or {{ui:Drawing colours}} to keep the colors.
4. In {{ui:Line weight}}, choose a pen width from 0.13 mm to 0.70 mm. Lines with the default lineweight print at this width; lines with their own lineweight (layer or object) print at that.
5. Click {{ui:Download PDF}}.

The browser downloads a PDF named after the drawing and the sheet, for example `Drawing1-A-101 Plan.pdf`. The history reports its size in bytes and its paper size in millimeters.

A sheet always plots at 1:1 on its own paper. To change the paper, change it on the sheet itself (see [Set up a sheet](topic:sheets-and-viewports)). Layers set not to plot are left out, and so are viewport frames.

## Every sheet in one PDF

With two or more sheets, the {{ui:Plot to PDF}} dialog has {{ui:All sheets in one PDF}}; or type {{cmd:PUBLISH}}. {{key:Enter}} takes every sheet; `S` (Some) asks for their names, separated by commas. One PDF downloads with a page per sheet, in tab order, each at its own paper size, and a bookmark per sheet with its name. It uses the ink and pen width you last chose in the dialog.

## Plot the model

1. Click the {{ui:Model}} tab.
2. Press {{key:Mod+P}}.

The dialog says it plots the model onto a sheet of paper, and shows more settings:

3. In {{ui:Area}}, choose what to plot:
   - {{ui:Everything (extents)}}: the whole drawing.
   - {{ui:What is on screen}}: the area you see in the drawing now.
   - {{ui:A window…}}: then click {{ui:Pick}}, click two corners on the drawing, and the dialog comes back.
4. In {{ui:Paper}}, choose a size: ISO A4 to A0, Letter, Legal, Tabloid and ANSI C and D, or ARCH C, D and E.
5. In {{ui:Orientation}}, choose {{ui:Landscape}} or {{ui:Portrait}}.
6. In {{ui:Scale}}, choose {{ui:Fit to paper}} or a scale such as `1:100`.

Under the settings, a line reports the scale and how big the area comes out on the paper. If it is bigger than the paper, it says so: the edges are cut off. Choose a smaller scale, a bigger paper, or {{ui:Fit to paper}}.

7. Click {{ui:Download PDF}}.

A model plot leaves a 5 mm margin round the edge, because most printers cannot print to the edge. {{ui:Fit to paper}} gives an odd scale (the history and the info line say "(fitted)"); choose a standard scale when the drawing must measure true.

:::note
Scales read in real units. For 1:100 to be right, set {{ui:Units}} in {{ui:Current settings}} to what one drawing unit is. A unitless drawing is taken to be in millimeters.
:::

## Print

1. In the dialog, click {{ui:Open to print}}.
2. The PDF opens in a new browser tab.
3. Print from that tab with the browser's print command ({{key:Mod+P}} there).

In the printer settings, choose "Actual size" or 100% (not "Fit") to keep the scale.

:::safari
Safari may show the PDF and wait for you to use **File**, then **Print**.
:::

If your browser blocks the new tab, CadCraft downloads the PDF instead and says so in the history. Allow pop-ups for the site to open it in a tab next time.

## Pictures

Pictures print at their own resolution, under the lines. {{ui:Pictures}} in the dialog caps them at 600, 300 or 150 dpi for a smaller file. Lines, text and hatches always stay sharp (vector), and transparency stays transparency. See [Pictures in a drawing](topic:pictures).

## Settings that are remembered

The dialog remembers your last paper, orientation, scale, ink and line weight in this browser. A picked window is kept until you reload the page; after that the dialog starts at {{ui:Everything (extents)}}.

## Common mistakes

- **The PDF is blank or the buttons are grey.** There is nothing to plot (an empty model), or {{ui:A window…}} is chosen and no window is picked yet ("Pick the window to plot.").
- **Text shows "?" in the PDF.** The PDF uses the built-in Helvetica font, which only has Western European characters. The history counts how many characters were replaced.
- **All lines are the same thickness.** Their lineweight is the default: give layers or objects a {{ui:Lineweight}} in the right panel (see [Line weights and linetypes](topic:units-papers-scales#line-weights-and-linetypes)).
- **Construction lines printed.** Set their layer not to plot: click its printer button in the {{ui:Layers}} panel.
- **The printout is the wrong size.** The printer scaled it. Print at actual size.
- **{{key:Mod+P}} opened the browser's print dialog.** The cursor was in a box in the right panel, or another dialog was open. Press {{key:Enter}} or {{key:Esc}} to leave the box, then try again, or click {{ui:Plot}}.
- **{{key:Mod+P}} did nothing.** A command is running. Press {{key:Esc}} first.
