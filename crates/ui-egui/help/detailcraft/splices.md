---
title: Splices
summary: Join two pieces of a beam or column end to end with bolted flange and web cover plates.
section: tasks
order: 33
keywords: splice, beam splice, column splice, cover plate, splice plate, flange plate, web plate, bolted splice, joint, lift, connect
---

A bolted splice joins two pieces of one I-shape end to end: a cover plate outside each flange and a plate on each side of the web, bolted through both pieces. DetailCraft cuts the pieces back to the gap, places the four plates, drills the holes in the plates and in both pieces, adds the bolts and warns about edge distances, spacing and fit.

## Where it works

- Two beams, or two column lifts, of the **same I-shape profile**, meeting **end to end in line**: the ends no more than 50 mm apart, the pieces parallel and on one line (within 1 mm). Either piece may be drawn in either direction.
- Level, sloped or vertical pieces all work.
- Two pieces of different depth or flange width are refused, as are channels, angles and tubes.

## Add splices

1. Draw the two pieces so they meet end to end. To split a member that is already drawn, draw it again as two pieces.
2. Select one (or both) and choose {{ui:Splice}} in the {{ui:Connect}} group's {{ui:More}} menu.
3. Every free joint of the selected pieces with another piece of the same profile gets a splice. The status bar says how many.

Select the splice (click one of its plates) to change its settings in the right panel. {{ui:Apply to similar (n)}} copies it to every other free joint between pieces of the same profile.

## Settings (mm)

| Setting | Default | What it does |
|---|---|---|
| Gap between the ends | 10 | Each piece is cut back by half of it |
| Bolt diameter, hole, grade, length allowance | 20, 22, A325, 30 | As for end plates |
| Flange plate thickness | 12 | The plate is as wide as the flange |
| Flange bolt rows (each side) | 3 | Rows across the flange on each side of the joint |
| Flange bolt pitch | 75 | Along the member |
| Flange bolt gauge | 0 | Between the two bolt lines; 0 = flange width less 80 |
| Flange end distance | 40 | Member end to first bolt, and last bolt to plate end |
| Web plate thickness | 8 | One on each side of the web |
| Web bolt rows | 3 | Up the web, centered |
| Web bolt columns (each side) | 2 | Along the member on each side of the joint |
| Web bolt pitch, up the web / along the member | 75, 75 | |
| Web end distance | 40 | As for the flanges |
| Web plate height | 0 | 0 = the clear web between the fillets less 10 at top and bottom |
| Plate material | A36 | Text |

Plate lengths follow from the bolts: flange plate = gap + 2 × (2 × end distance + (rows − 1) × pitch); the web plates likewise with their columns. With the defaults the flange plates are 470 long.

## Good to know

- The plates belong to the **first piece drawn** of the two (its properties call it the first piece): they show on its assembly drawing and ship with it, bolted, not welded. The second piece's assembly drawing shows only its holes.
- Flange bolts grip the flange and its plate; web bolts grip the web and both web plates. The bolt lengths are listed separately in the properties.
- Warnings use the same EN 1993-1-8 minimums as the other connections (end and edge distance 1.2 × hole, pitch 2.2 × hole along and 2.4 × hole across), plus a web plate deeper than the clear web and flange bolts too close to the web for the fillet.
- DetailCraft does not design the splice (no strength check), and does not make fill plates for pieces of different thickness.
- Copying both pieces copies their splice; deleting either piece deletes it.
