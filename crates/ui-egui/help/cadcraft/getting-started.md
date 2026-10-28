---
title: Getting started with CadCraft
summary: What the CadCraft drafting app does, where it runs, and your first drawing from a blank screen to a saved file.
section: start
order: 10
keywords: start, first drawing, new, open, save, save as, begin, tutorial, introduction, quick start
---

CadCraft is a 2D drafting app. You draw and edit lines, arcs, polylines, text, dimensions and blocks, set up sheets with title blocks and viewports, and plot them to PDF. It opens DWG and DXF drawings and saves them as DWG or DXF.

It runs in your web browser. There is nothing to install: the site gives you the address, and the whole app loads into the page. Your drawings open from your own computer and are saved back to it. **Nothing is uploaded.**

If you have drafted before, read [Coming from other drafting software](topic:coming-from) after this page. Most habits carry over, and the few that do not are listed there.

## What you need

- A recent Chrome, Edge, Firefox or Safari on a Mac, Windows or Linux computer. See [Browsers and platforms](common:browsers).
- A mouse with a wheel is the most comfortable. A trackpad works; see [Mouse, trackpad and touch](topic:mouse-trackpad-touch).
- A keyboard. CadCraft is driven by typed commands, like most drafting programs.

## The idea in one paragraph

You type a command (or click its button), and the command line at the bottom asks you questions one at a time: "Specify first point", "Specify next point". You answer each question by clicking in the drawing or by typing a value, then pressing {{key:Enter}}. Pressing {{key:Esc}} cancels. That is the whole interaction model; everything else is detail.

## Your first drawing

You will draw a 4000 by 3000 rectangle out of lines, put a circle in the middle, and save the file.

1. Open CadCraft. The command line at the bottom shows `Command:` and the drawing area shows a short "Start drawing" card.
2. Type `L` and press {{key:Enter}}. You do not need to click the command line first: typing anywhere goes there.
3. The prompt changes to `Specify first point:`. Type `0,0` and press {{key:Enter}}.
4. Type `@4000,0` and press {{key:Enter}}. A line appears, running right. The `@` means "from the last point".
5. Type `@0,3000` and press {{key:Enter}}.
6. Type `@-4000,0` and press {{key:Enter}}.
7. Type `C` and press {{key:Enter}}. `C` is the Close option: the last line goes back to the first point and the command ends.
8. Click {{ui:Extents}} in the View group of the toolbar. The rectangle fills the screen.

You should now see four lines forming a rectangle, and the history above the command line lists what you typed.

9. Type `C` and press {{key:Enter}} to start the circle command.
10. Type `2000,1500` and press {{key:Enter}} for the center.
11. Type `500` and press {{key:Enter}} for the radius.

A circle appears in the middle of the rectangle.

12. Press {{key:Mod+S}}, or click {{ui:Save}} in the File group.

The browser downloads `Drawing1.dwg` to your usual downloads folder, and the history says it saved a copy as DWG. That file opens in CadCraft again, and in most other CAD programs. (If you chose DXF in {{ui:Save as}} before, in this browser, a new drawing saves as `Drawing1.dxf` instead.)

:::tip
Press {{key:Enter}} on an empty command line to repeat the last command. Press {{key:Space}} instead of {{key:Enter}} if you prefer: outside of text, it does the same.
:::

## Open an existing drawing

1. Press {{key:Mod+O}}, or click {{ui:Open}}.
2. Choose a `.dwg` or `.dxf` file in the file picker.

You can also drag a DWG or DXF file from {{pick:mac=Finder|windows=File Explorer|linux=your file manager}} and drop it on the drawing area. The file is read in the page; it never leaves your computer.

See [Files: what opens, what saves](topic:files) for what is kept and what is converted.

## Saving: what to expect

Saving in a browser works differently from a desktop program:

- {{ui:Save}} always **downloads a new copy**. It does not overwrite the file you opened. If a file with the same name is already in your downloads folder, the browser usually adds a number, such as `Drawing1 (1).dwg`.
- {{ui:Save}} uses the drawing's format: DWG if you opened a DWG, DXF if you opened a DXF. To choose, press {{key:Mod+Shift+S}} or click {{ui:Save as}}, then pick {{ui:DWG}} (and its version), {{ui:DXF R12}} or {{ui:DXF 2000}}. See [Saving](topic:files#saving).
- The file you opened is never changed.
- Opening another file, or {{ui:New}}, puts it in its own tab over the drawing, so nothing you have open is closed. If you close a drawing's tab or leave the page with unsaved changes, CadCraft asks first: {{ui:Save a copy first}}, discard or {{ui:Cancel}}. Closing the browser tab shows the browser's own "Leave site?" question instead.

Cloud saving is not available yet. Where your files go and why they stay private is explained in [Your files and privacy](common:your-files).

## Where to go next

Press {{key:F1}} or click {{ui:Help}} in the bar at the top to open this help beside the drawing at any time. It works with no connection.

- [The CadCraft screen](topic:workspace) names every bar and panel.
- [Draw with coordinates](topic:draw-with-coordinates) is the best first walkthrough.
- [Keyboard and mouse](topic:keyboard) lists every key.
- Type `HELP` at the command line for a list of every command, or read the [Command reference](topic:commands).
