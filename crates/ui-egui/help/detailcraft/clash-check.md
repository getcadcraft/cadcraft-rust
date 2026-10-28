---
title: Check for clashes
summary: Find members, plates and footings that reach into each other, leaving out the parts a connection means to meet, and go to each clash.
section: tasks
order: 48
keywords: clash, clash check, collision, interference, overlap, intersect, hard clash, coordination, check model
---

Open the {{ui:Clashes}} tab under the view and choose {{ui:Check now}}. DetailCraft looks at every pair of parts in the phases shown: steel into steel, and steel into a footing. The tab lists each clash, the deepest first, with:

- the two parts, by part mark (or footing mark) and profile;
- **Overlap (mm)**: how far a point of one part is inside the other, up to the nearest face;
- **Near**: the grid intersection and level it is at, or its coordinates;
- {{ui:Show}}: selects both parts and brings the place to the middle of the view.

The tab's title shows how many clashes the last check found. The check does not run by itself: after you change the model the list says so, and {{ui:Check now}} runs it again.

## What is not a clash

- Parts that only touch: a plate against a flange, a base plate on its footing, two members end to end. **Tolerance (mm)** sets how far two parts may run into each other before it counts (1 mm to start; up to 50).
- Parts a connection joins: an end plate with its beam and with the column flange it is bolted to, the beam and the column of that joint, a base plate with its column, a splice's plates with both pieces, a brace with its gusset (a tube's slot is not cut in the model). The connection means them to meet.
- A base plate's shear lug in the footing under it (it sits in a grout pocket).
- Bolts and anchor rods (they pass through holes) and rebar.
- Parts in hidden phases. See [Phases](topic:phases).

A brace or beam that runs into a column with no connection between them **is** a clash: that joint still needs a connection. The sample frame's brace shows this: it has no gusset. Select it and choose {{ui:Gusset}} ([Brace gussets](topic:brace-gussets)) and its clashes go.

## Good to know

- Overlap measures depth into material, so a member run into a column's thin web or flange shows only a few millimeters even when it runs far in.
- Parts are checked as they are cut: copes and the cut-back of a beam at its end plates are taken into account. Holes are not.
