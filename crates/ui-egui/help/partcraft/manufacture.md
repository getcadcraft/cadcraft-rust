---
title: Make toolpaths and G-code (Manufacture)
summary: Set up stock and a work origin, choose tools, add face, pocket, contour, drill and chamfer operations by picking faces, simulate the stock for gouges and cycle time, and download G-code for Fanuc-style, GRBL or LinuxCNC controls.
section: tasks
order: 140
keywords: adaptive, adaptive clearing, constant engagement, trochoidal, hsm, high speed machining, manufacture, CAM, toolpath, G-code, NC, CNC, mill, milling, pocket, contour, profile, facing, drill, peck, G83, G81, G73, chamfer, stock, work offset, G54, post processor, GRBL, LinuxCNC, Fanuc, feeds and speeds, simulation, gouge, cycle time
---

The Manufacture workspace turns the part into toolpaths for a 3-axis milling machine and writes them as G-code. It is **2.5-axis**: the tool stays vertical, walls are cut straight down and floors flat. It works on the named faces the mesh kernel gives the part (see [The mesh kernel](topic:solid-kernels)).

Lab build: check every program in your control's own simulator, and run it in the air above the stock first.

## Set up

1. Choose **Manufacture** in the workspace switcher, then {{ui:New setup}}.
2. On the **Setup** tab choose:
   - **Tool axis (+Z)**: which way the tool points into the part. World +Y (PartCraft's up) is the default; machine X is then world X and machine Y is world −Z.
   - **Work origin (G54)**: a stock corner or centre, top or bottom, the model's top centre, or the model origin.
   - **Stock**: a box or a cylinder, either the model plus offsets (sides, top, bottom) or a fixed size.
   - **Material** (sets the starting feeds and speeds), the **post** (control), the machine's maximum spindle speed, rapid rate and tool change time, coolant and the program number.

## Tools

The **Tools** tab holds the library: flat, ball and bull nose end mills, drills, chamfer mills and face mills, each with a T number, diameter, flutes and flute length (plus corner radius, point or chamfer angle where they apply).

Spindle speed and feed come from the material: n = 1000 × cutting speed / (π × diameter), feed = n × flutes × chip load (drills: n × feed per rev). The numbers are conservative starting values from published ranges, not tuned for your machine; type your tool maker's cutting speed, chip load, rpm or feed to override them. When the speed passes the machine's maximum it is held there and the feed follows, keeping the chip load.

## Add operations

Each toolbar button adds an operation and, where it needs geometry, starts picking: click faces or edges in the view (click again to drop one), then {{ui:Done picking}} or {{key:Esc}}.

| Operation | Pick | What it does |
|---|---|---|
| {{ui:Face}} | nothing | Zig-zag passes over the whole stock, past its edges, down to the model's top |
| {{ui:Pocket}} | a flat floor face, a round hole, or a closed chain of edges | Clears the region in offset passes inside-out, at each step-down; islands (faces standing up inside the floor) are left; a floor open all round (nothing higher outside it) is cleared out to the stock edge |
| {{ui:Adaptive}} | the same as Pocket | Clears the same region in passes that each cut a set width (**Engagement**, a fraction of the diameter), so the cutter never takes a full-width slot, even in corners; it goes in by a helix and finishes the walls at each level. Narrow cuts allow a deeper step-down and a higher feed |
| {{ui:Contour}} | a flat face (its outline), a round face, or edges | Follows the outline offset by the tool radius, outside or inside, with lead-in and lead-out arcs and optional tabs |
| {{ui:Drill}} | holes (click a hole's wall or its rim) | Drills each hole with a canned cycle: peck (G83), drill (G81) or chip break (G73); or give a diameter to drill every hole of that size |
| {{ui:Chamfer}} | a face's outline or edges | Runs a chamfer mill along the edge so it cuts the width you set |

Select an operation in the list to change its tool, heights (clearance, retract, feed height, top, bottom), step-down, stepover, stock to leave, finishing pass, entry (helix, ramp or plunge), climb milling, tabs or cycle. {{ui:Up}} and {{ui:Down}} reorder, {{ui:Suppress}} leaves it out, {{ui:Delete}} removes it. Everything is in the undo history and saved in the part file.

A pocket's stepover is at most half the tool diameter, so every point of its floor lies within one tool radius of a pass. Inside corners keep the tool's radius.

**Adaptive clearing** keeps the cutter's engagement near one value. An engagement of 0.15 of the diameter is an engagement angle of about 46°: the arc of the cutter's edge in the material. An offset pocket, by contrast, cuts full width on its first pass and wraps further round in every inside corner. Between passes the tool stays down where it moves through cut air, and lifts and comes down in cut air elsewhere. The passes are worked out on a fine grid over the region (cells about a twelfth of the tool radius), so the engagement can run up to about a cell wider than set; the walls are finished exactly. A big open region makes a long program with many short moves.

## Simulate

Toolpaths are made again after every change (the part's too). {{ui:Simulate}} also shows the stock left after the whole program. The panel shows:

- **Cycle time**: feed moves at their feed, rapids as straight lines at the setup's rapid rate, plus the tool change time. Real machines accelerate, so they take longer.
- **Gouges**: places where the tool went more than 0.02 mm below the part's surface. Red marks show them in the view.
- **Stock left on the part**.

The stock is a height map of the top (a Z-map), so it cannot show undercuts. Drag **Playback** to move the tool along the program. In the view, feed moves are solid and rapids dashed, one colour per operation.

## Post the G-code

Choose the control under **Post** and press {{ui:Download G-code}} (or {{ui:Post}} on the toolbar):

| Control | File | Notes |
|---|---|---|
| Fanuc-style ISO | .nc | %, O number, line numbers, T M6 with G43 H, canned cycles |
| GRBL | .nc | No tool changer or canned cycles: a tool change pauses with M0, drilling is written as plain moves |
| LinuxCNC | .ngc | T M6 with G43 H, canned cycles, M2 |

Every file starts with comments for the program, stock, work offset and tools. Coordinates are in millimetres with 3 decimals. {{ui:Toolpath CSV}} and {{ui:Toolpath JSON}} export the moves themselves.

## Limits

- 2.5-axis only: no 3D roughing or finishing of curved faces (the flange's cone is left as steps), no rest machining. Adaptive clearing is 2D (flat floors).
- Holders and shanks are not checked for collisions; a warning appears when a cut is deeper than the flute length.
- Feeds and speeds are starting values; there is no machine-specific post editor.
