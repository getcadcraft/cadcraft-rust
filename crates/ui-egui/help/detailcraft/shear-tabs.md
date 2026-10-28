---
title: Shear tabs
summary: Bolt a beam's web to a plate shop-welded to a column flange or web, also where the beam meets the column at an angle in plan or slopes.
section: tasks
order: 31
keywords: shear tab, single plate, fin plate, simple connection, web connection, skewed, skew, sloped, raked, angled beam, bolted, connect
---

A shear tab (single plate, fin plate) is a plate welded to the column in the shop; on site the beam's web is bolted to it with one vertical row of bolts. DetailCraft places the tab, cuts the beam back clear of the column, drills the holes in the tab and the beam web, adds the bolts and the weld, and warns about edge distances and fit.

## Where it works

- A beam (I-shape or channel) whose end lies on the line of a **vertical** I-shape column, meeting its **flange** or its **web**.
- **Square, skewed in plan, or sloped**: the tab follows the beam's web and the beam is cut back so its nearest corner keeps the gap to the column. A beam rising or falling at more than 60° is refused.

## Add shear tabs

1. Select the beam.
2. In the {{ui:Connect}} group, open {{ui:More}} and choose {{ui:Shear tabs}} (or choose {{ui:Add shear tabs}} in the {{ui:Properties}} panel).
3. Every free end of the beam that meets a column gets a tab. Each beam end takes one connection: a shear tab or an end plate.

Select a tab (click it, or a bolt) to see and change its settings. {{ui:Apply to similar}} copies it, as a shear tab, to every joint like it that has no connection yet.

## Settings (mm)

| Setting | Starts at | Meaning |
|---|---|---|
| Plate thickness | 10 | The tab's thickness |
| Plate width | 115 | How far the tab reaches out from the column face, along the beam |
| Bolt diameter, Hole diameter, Bolt grade | 20, 22, A325 | As for end plates |
| Bolt rows, Row pitch | 3, 75 | The bolts in the vertical row |
| Edge distance (top and bottom bolts) | 38 | From the top and bottom bolts to the tab's ends; the tab is rows × pitch + 2 × this deep |
| Bolt group raised by | 0 | 0 centers the bolts on the beam depth |
| Gap to the column | 13 | Clear gap between the column and the beam's nearest corner |
| Bolt line from the column face | 65 | Along the beam, from the column face to the bolts |
| Bolt length allowance | 30 | Added to the grip (tab + beam web), rounded up to 5 mm |
| Fillet weld size | 6 | Both sides of the tab to the column |

## Good to know

- The tab is part of the column's assembly (welded in the shop); the beam's assembly has its holes only.
- A skewed or sloped beam is cut further back than a square one so its nearest corner keeps the gap. DetailCraft cuts beams square: the fabricator cuts the end to suit. The tab's edge at the column is drawn square too.
- DetailCraft does not check strength. Warnings use EN 1993-1-8 Table 3.3 minimums for edge distances and pitch, and say when the tab is deeper than the beam web between its fillets, or the bolts sit too close to the beam's cut end.
- The [clash check](topic:clash-check) leaves a tab and the members it joins out: they are meant to meet.
