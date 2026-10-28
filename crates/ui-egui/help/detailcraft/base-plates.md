---
title: Add base plates and anchor rods
summary: Put a welded base plate with four anchor rods under a column, set its sizes, and copy it to every similar column.
section: tasks
order: 70
keywords: base plate, baseplate, anchor rods, anchor bolts, holding down bolts, column base, foundation bolts, similar
---

A base plate connection puts a plate under the column, cuts the column short by the plate thickness, welds the two together, and adds four anchor rods through holes in the plate.

:::warning
The sizes are starting values, not a design. DetailCraft does not check bearing, rod strength or embedment.
:::

## Add a base plate

1. Choose {{ui:Select}} ({{key:V}}) and click a column. It must be vertical.
2. Choose {{ui:Base plate}} in the {{ui:Connect}} group, or {{ui:Add base plate}} in the {{ui:Properties}} panel.
3. The status bar says "Base plate added with four anchor rods." The new connection is selected.

A column can have one base plate. If it already has one, the status bar says "This column already has a base plate."

## Change it

Select the base plate (click the plate or a rod). Under {{ui:Plate, bolts and weld (mm)}}:

| Setting | Default | What it does |
|---|---|---|
| Plate thickness | 25 | The column is shortened by this much |
| Plate length | 450 | Along the column depth |
| Plate width | 400 | Across the column |
| Rod diameter | 20 | Anchor rod size |
| Hole diameter | 33 | Hole in the plate |
| Bolt grade | F1554 Gr 36 | The rod grade, as text |
| Rod spacing (along depth) | 350 | Between rod centers, along the plate length |
| Rod spacing (across) | 300 | Between rod centers, across |
| Rod projection above plate | 100 | From the plate top to the rod top |
| Rod embedment below plate | 450 | From the plate underside down into the concrete |
| Fillet weld size | 8 | All around the column base |
| Plate material | A36 | Text |
| Grout gap | 0 | Space between the top of concrete and the plate underside |
| Stiffeners | None | None, 2 or 4; see below |
| Stiffener thickness, height | 12, 150 | |
| Shear lug depth | 0 | How far the lug reaches below the plate; 0 = no lug |
| Shear lug thickness, width | 25, 150 | |
| Plate washers | None | On every rod, or none |
| Washer size, thickness | 0, 0 | 0 = the minimum from AISC Design Guide 1 for the rod |

Rod length = embedment + grout gap + plate thickness + projection. With the defaults that is 575 mm.

## Base details: grout, stiffeners, shear lug, washers

All four are off on a new base plate, and on base plates from models made before DetailCraft 0.4.

- **Grout gap.** The plate sits this far above the top of concrete, and the rods get longer by it. AISC Design Guide 1 (2.10) suggests 1 in. on a finished floor and 1½ to 2 in. on a footing or pier; more for large plates and plates with a shear lug.
- **Stiffeners.** Vertical plates welded to the column and the plate, from the column face out to the plate edge, as rectangles (no chamfer or taper yet). **2** stand in line with the web, beyond each flange; **4** add two square to the web, between the flanges, out to the plate sides. On a tube column they stand at the middle of each face. Each is welded to the column and the plate on both sides.
- **Shear lug.** A plate welded all round under the base plate, in line with the column depth, centered. DetailCraft warns if it is no deeper than the grout gap. It does not cut the pocket in the footing; the [clash check](topic:clash-check) leaves the lug and the footing out for that reason.
- **Plate washers.** A square plate washer on each rod, sitting on the base plate, listed in the column's assembly as loose (shipped with the column, not welded: Design Guide 1, 2.6). With size and thickness 0, DetailCraft takes the minimum washer from AISC Design Guide 1, 2nd ed., Table 2.3 for the rod; a metric rod takes the next inch size up (an M20 rod gets the 7/8 in. row: 2½ in. × 5/16 in.). The properties panel shows the size used. The washer's hole is the rod diameter + 1/16 in.: that is DetailCraft's choice, the table does not give one. DetailCraft warns when the rods may not stick out far enough for the washer and a nut (projection less than washer thickness + rod diameter).

Design Guide 1, Table 2.3, also recommends the rod hole in the plate: 1-5/16 in. for a ¾ in. rod, 1-9/16 in. for ⅞ in., 1-13/16 in. for 1 in. DetailCraft does not change your hole size.

Warnings also appear when the rod holes are closer than 1.2 × the hole diameter to the plate edge, when the rods fall inside the column outline, or when the plate is smaller than the column.

## Copy it to similar columns

1. Select the base plate.
2. Choose {{ui:Apply to similar (n)}}, or {{ui:Similar}} in the {{ui:Connect}} group's {{ui:More}} menu.
3. Every vertical column with the same profile that has no base plate and does not stand on another column gets a copy.

## Base plates and footings

A base plate does not need a footing, and a footing does not know about the base plate above it: the anchor rods are not tied into the footing's bars. Place footings with {{ui:Footing}}; see [Place footings with rebar](topic:footings-and-rebar).
