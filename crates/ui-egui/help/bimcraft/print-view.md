---
title: Check a drawing in print view
summary: Switch a DWG or DXF to print view to see it black on white with its line weights and without the layers that never print.
section: tasks
order: 130
keywords: print, plot, preview, plot preview, line weights, lineweight, monochrome, black and white, defpoints, no plot
---

## What you'll do

See how a sheet will look on paper before you print it from the program that made it: black on white, line weights at their true width, and without layers that are set not to print.

## Steps

1. Open a DWG or DXF drawing.
2. Click a sheet tab above the drawing.
3. Click {{ui:Print view}} at the end of the tabs, or press {{key:P}}.
4. Zoom in on a detail to compare heavy and light lines.
5. Click {{ui:Print view}} again, or press {{key:P}}, to go back to the normal colors.

## What you should see

- White paper on a gray desk, every line and text in black, whatever the theme.
- Line weights: on a sheet each weight is drawn at its real width on paper, so a 0.5 mm line is twice as thick as a 0.25 mm line at any zoom. A line is never drawn thinner than its screen weight.
- Layers set not to print, and the DEFPOINTS layer, are hidden.
- The {{ui:Print view}} button stays pressed. Print view stays on when you switch sheets or open another drawing, until you turn it off.

In {{ui:Model}}, print view uses screen weights only (thin lines one pixel, heavier ones a little thicker), because model space has no paper size.

## Common mistakes

- **Expecting it to print.** Print view is a preview. Printing and PDF export are not built in BimCraft yet; print from the program that made the drawing.
- **Expecting plot styles.** Pen tables and plot style colors are not read. Everything is plain black.
- **Pressing P while typing.** {{key:F}} and {{key:P}} do nothing while you are typing in a field, or while the equipment schedule is showing.
