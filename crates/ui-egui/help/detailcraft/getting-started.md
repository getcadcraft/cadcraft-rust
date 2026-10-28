---
title: Getting started with DetailCraft
summary: What DetailCraft does today, how to open it, and a first model from grids to a shop drawing in ten minutes.
section: start
order: 10
keywords: start, first model, introduction, steel detailing, rebar, quick start, tutorial, overview
---

DetailCraft is the steel and rebar detailing app in the CadCraft family. You build a 3D model of a steel frame on grids and levels, connect it, and get what the shop needs: part and assembly marks, a bill of materials, single-part shop drawings, DSTV NC files, an IFC model, and a bending schedule for the footings.

This is an early build. It does a short list of things carefully rather than everything at once. Read [What is not built yet](topic:not-built-yet) before you plan real work around it.

## What it does today

- Grids and levels you type in, in millimeters.
- Columns, beams, braces and plates from published profile catalogs (AISC W, S, C, L, HSS and Pipe; European IPE, HEA, HEB and UPN), plus your own parametric profiles.
- Two connections: a bolted end plate from a beam to a column flange, and a column base plate with four anchor rods. Each can be copied to every similar joint.
- Holes and copes typed in by position.
- Rectangular pad footings with a bottom mat and starter bars.
- Part and assembly marks that update by themselves after every change.
- Exports: IFC4, DSTV NC files (I-shapes and plates), bill of materials as CSV, bending schedule as CSV, and a shop drawing PDF of one part.

## Open DetailCraft

1. Open DetailCraft in a desktop browser: Chrome, Edge, Firefox or Safari. See [Browsers and computers](common:browsers).
2. Wait for the screen to fill. The status bar at the bottom says "Ready. Open the Sample, or set grids and levels and start placing members."

The model view needs 3D graphics (WebGL). If the middle of the screen says "This browser could not start 3D graphics (WebGL)", see [Troubleshooting](topic:troubleshooting#the-3d-view-is-missing).

:::note
Your models stay on your computer. Opening a model reads it in the browser, and saving downloads a file. Nothing is uploaded. Cloud saving is not available yet. See [Where your files go](common:your-files). To use DetailCraft with no connection later, choose {{ui:Save for offline}} in the app bar once while online; see [Working offline](common:offline).
:::

:::offline
You are offline. DetailCraft works without a connection only if it was saved for offline use first: the {{ui:Save for offline}} button in the app bar then reads {{ui:Offline ready}}. See [Working offline](common:offline).
:::

## Look at the sample first

The fastest way to see what DetailCraft makes is the sample frame.

1. Choose {{ui:Sample}} in the {{ui:File}} group of the toolbar.
2. The view shows a two-bay frame: six columns with base plates, four beams with end plates, one brace and seven footings with rebar.
3. Click a beam. The {{ui:Properties}} panel on the right shows its part mark, cut length, weight and the holes the end plates made.
4. Open the {{ui:Bill of materials}} tab at the bottom to see every part mark with its count and weight.

The walkthrough [Explore the sample frame](topic:sample-model) goes through it piece by piece.

## Your first model

A new model already has grids 1 to 3 by A to B at 6000 mm and two levels, Level 1 at 0 and Level 2 at 4000.

1. Choose {{ui:New}}. If the current model has changes, DetailCraft asks whether to save them first.
2. Choose {{ui:Plan}} above the view. The level list beside it picks Level 1.
3. Choose {{ui:Column}} in the toolbar, or press {{key:C}}.
4. Click the grid intersection A/1. A W12x50 column appears from Level 1 to Level 2. The status bar says where it went.
5. Click B/1, A/2 and B/2 to place three more.
6. Pick Level 2 in the level list, then choose {{ui:Beam}} or press {{key:B}}.
7. Click A/1, then A/2. A W16x26 beam joins the two column tops, its top of steel on the level.
8. Press {{key:Esc}} to go back to {{ui:Select}}, then click the beam.
9. Choose {{ui:End plates}} in the {{ui:Connect}} group. Both ends get a bolted end plate, the beam is cut back to fit, and the first end plate is now selected.
10. Click the beam again.
11. Choose {{ui:Shop drawing}} in the {{ui:Output}} group. A preview of the beam's shop drawing opens. Choose {{ui:Download PDF}}.
12. Press {{key:Mod+S}} to save the model. The browser downloads `Untitled.detailcraft.json`.

Rename the model in the {{ui:Name}} field at the top left before you save, so the file and every export carry its name.

## Where to go next

- [A tour of the screen](topic:workspace)
- [Set up grids and levels](topic:grids-and-levels)
- [Move around the 3D view](topic:navigate-3d), with a mouse, a trackpad or a touch screen
- [Coming from other detailing software](topic:coming-from), if you know a big detailing package
- [Using this help](common:help)
