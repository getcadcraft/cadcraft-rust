---
title: Stairs
summary: Place a straight steel stair from its rise, going and width: two stringers and tread plates, one welded assembly.
section: tasks
order: 36
keywords: stair, stairs, staircase, stringer, tread, riser, going, nosing, flight, steel stair, ibc 1011, blondel
---

A straight steel stair: two stringers (channels with their toes in, or flat bars on edge) and a plate tread at every step, welded between them. DetailCraft works out the number of risers from the rise, checks them against the building code's limits, and makes the stair one shop assembly.

## Place a stair

1. Choose {{ui:Stair}} in the {{ui:Place}} group, or press {{key:S}}.
2. Set the stair on the right: the **direction of climb** (0° climbs along +x, 90° along +y), the **rise** (0 = up to the next level), the **going**, the **clear width** between the stringers, the **highest riser allowed**, the **stringer**, the **tread plate thickness**, the **nosing** and the material.
3. Click a grid intersection: that is the foot of the first riser, on the stair's center line.

The status bar says how many risers it made. Select the stair (click a stringer or a tread) to change any of its settings, its position or its phase; the properties also show the risers, treads, run, pitch and 2 × riser + going.

## What DetailCraft works out

- **Risers.** As few as keep each riser at or under the highest allowed: a 3000 mm rise at 178 mm or less gives 17 risers of 176.5 mm. There is one tread fewer than risers (the top floor is the last step).
- **Treads.** A plate going + nosing deep (280 + 25 = PL6x305 by default) and as long as the clear width, its top at each riser height, its front edge (the nosing) in front of the riser line.
- **Stringers.** At the stair's pitch, from where they meet the lower floor to where they meet the upper floor, just outside the clear width, their top edge 50 mm above the line of the nosings. A channel's toes face into the stair; a flat bar such as PL12x250 stands on edge.
- **Welds.** Each tread to both stringers. The treads and the right stringer are welded to the left stringer, so the stair gets one assembly mark (ST1) and its own assembly drawing; the stringers get part marks st…, the treads p….
- **Checks.** Warnings when a riser is outside 4 to 7 in. (102 to 178 mm) or the going is under 11 in. (279 mm), the limits of IBC 2021 section 1011.5.2, when 2 × riser + going is outside 600 to 650 mm (a common comfort rule), and when the stringer is too shallow to carry the treads. An unknown stringer profile stops the stair from being built.

## Good to know

- Move, rotate, mirror, copy, paste, delete and phases work on a stair as on a member. A rotation turns the climb; a mirror sends it the mirrored way.
- Stringers export to IFC as IfcMember and treads as IfcPlate; both go to NC files and the bill of materials. The clash check leaves a stair's own parts alone (they are welded together).
- The stringers are cut square: their end cuts against the floors, landings, handrails, a toe plate and grating treads are not made yet.
- DetailCraft does not design the stair: the stringers and treads are not checked for strength or deflection.
