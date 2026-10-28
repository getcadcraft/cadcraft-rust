---
title: What is not built yet
summary: What this early DetailCraft build cannot do, and what to do instead today.
section: trouble
order: 20
keywords: limits, limitations, missing, roadmap, not supported, coming, planned, workaround, todo
---

DetailCraft is an early build. This page lists what it does not do yet, so you can plan around it. Nothing here is a promise of a date.

## Modeling

| Not yet | Instead, today |
|---|---|
| Dragging one end of a member (stretching it) | Edit {{ui:Start}} or {{ui:End}} under {{ui:Ends (x, y, z mm)}} in the {{ui:Properties}} panel |
| A select filter (only beams, only one profile) | Drag a window, then {{key:Shift}}+click to take parts out |
| Array a selection; rotating about a sloped axis; mirroring in a sloped plane | Copy and paste it at an offset, or {{ui:Duplicate}} it; rotate and mirror work in plan ([Select by window, move, rotate, mirror, change many](topic:select-and-change)) |
| Points off the grid while clicking | Place on the grid, then type the end coordinates |
| Elevations along numbered grid lines; saved views; work planes | Use {{ui:3D}}, or {{ui:Add by grid}} |
| Holes drawn as real cuts in the 3D view | Holes show as dark plugs; they are real in every export |
| Rounded or angled copes, other cuts and fittings | Square copes on I-shapes only |
| Holes you add to angles and round tubes | Angle holes come from [brace gussets](topic:brace-gussets) only |
| Imperial input | Type millimeters (25.4 per inch) |
| Editing or removing a custom profile | Add a corrected profile under a new name |

## Stairs

| Not yet | Instead, today |
|---|---|
| Handrails, landings, toe plates, grating treads, spiral and dog-leg stairs | Straight flights only; see [Stairs](topic:stairs) |
| Stringer end cuts against the floors | Square-cut stringers; cut them in your shop software |

## Connections

DetailCraft builds five connections: a brace gusset (see [Brace gussets](topic:brace-gussets)); a bolted splice between two pieces of one I-shape (see [Splices](topic:splices)); a flush or extended bolted end plate from a level, upright I-shape beam to an I-shape column flange or web; a shear tab from a beam (square, skewed or sloped) to a column flange or web; and a column base plate with four anchor rods.

| Not yet | Instead, today |
|---|---|
| Skewed or sloped end plates | Use [shear tabs](topic:shear-tabs), which take skewed and sloped beams |
| Clip angles; splice fill plates and splices between different sizes; corner gussets welded to beam and column, chamfered gussets | Detail them in other software, or add plates and holes by hand as loose parts; see [Splices](topic:splices), [Brace gussets](topic:brace-gussets) |
| Chamfered or tapered base stiffeners; grout holes; the footing pocket for a shear lug | Rectangular stiffeners; see [Base plates](topic:base-plates) |
| Connection design checks (strength) | Check connections with your engineer's calculations; DetailCraft only warns about edge distance and spacing |
| Slotted holes | Round holes only |

## Rebar

| Not yet | Instead, today |
|---|---|
| Rebar in anything but rectangular pad footings | None today |
| Laps, development lengths | Check them yourself |
| Shape codes beyond straight, one bend and a U; ACI 315 types for straight and one-hook bars | The three shapes print as BS 8666 00, 11, 21 or ACI Type 2; see [Rebar reference](topic:rebar-reference) |
| Metric bars other than BS 4449 H8–H40 (e.g. CSA 10M–35M) | ASTM soft-metric names #10–#36, or BS bars |

## Drawings and exports

| Not yet | Instead, today |
|---|---|
| Portrait sheets, A0, and sheets of your own size; general-arrangement views on skewed grids | Landscape A4 to A1 and ANSI B to D ([General-arrangement and assembly drawings](topic:drawings)); scale the PDF to print on another size |
| Shop drawings of channels, angles and tubes | I-shapes and plates only |
| Weld symbols on single-part shop drawings; symbols other than fillets | Assembly drawings carry fillet weld symbols; the shop drawing gives the weld as text |
| Editing a drawing in the app | Change the model and make the drawing again; mark up the PDF in a PDF app such as MarkupCraft |
| Drawings as DWG with dimension objects, or the model at full scale in model space with the sheets as layouts | Each sheet is a full-size DWG or DXF of the paper, dimensions as lines and figures ([General-arrangement and assembly drawings](topic:drawings#download)) |
| NC holes in round tubes, copes on angles and round tubes in NC files | Not written; see [NC file fields](topic:nc1-fields) |
| Bevels, markings and inner contours in NC files | None today |
| Bringing connections, footings, rebar, slotted or rectangular holes and holes in round tubes in from an IFC file | {{ui:Import IFC}} brings in beams, columns, members, plates, grids, levels, round holes, copes and bolts, and its report lists what it left out |
| Custom report layouts | The three CSV files |

## Panels

| Not yet | Instead, today |
|---|---|
| Moving the reports at the bottom ({{ui:Problems}}, {{ui:Bill of materials}}, {{ui:Bending schedule}}) | Only the {{ui:Model}} and {{ui:Properties}} panels move. See [Arrange the panels](common:arrange-panels) |

## Working together and saving

| Not yet | Instead, today |
|---|---|
| Cloud saving and sync between computers | Save `.detailcraft.json` files and share them like any file. Cloud saving is not available yet |
| Other people's cursors and "is editing" labels in a live session; grids, levels and phases merged item by item | Edits still merge (grids, levels and phases go whole, the later change wins); see [Detail one structure together, live](topic:work-live) |
| Invite links to a live session over the internet | Join with a short code on the same network |

## Platforms

- DetailCraft is made for a desktop, laptop or tablet screen. It works on a phone too: the model keeps the screen and the panels and reports open as sheets from a bar at the bottom, but there is little room for the panels.
- Offline use needs DetailCraft saved for offline use first, with {{ui:Save for offline}} in the app bar; once it reads {{ui:Offline ready}}, everything in this build works with no connection. See [Working offline](common:offline).

If one of these gaps blocks your work, tell us through the [support page](https://getcadcraft.com/support); it helps us decide what to build next.
