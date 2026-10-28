---
title: Check draft for moulding and casting
summary: Colour the part by its draft angle against the pull direction of a mould, see which faces release, which need more draft and which release from the other half, with each one's share of the area.
section: tasks
order: 96
keywords: draft analysis, draft, taper, mould, mold, die casting, injection moulding, pull direction, parting line, undercut, release, check draft
---

Parts made in a mould, a die or a sand casting need **draft**: walls tilted a little so the part slides out. **Draft analysis** colours every face by how much it tilts away from the direction the mould opens (the **pull**).

## Turn it on

1. In the Design workspace, open {{ui:Views}} and choose {{ui:Draft analysis}}.
2. Set **Pull**: the direction the mould opens, along X, Y or Z either way, or **Square to a picked face**: press {{ui:Pick a face}} and click a flat face of the part.
3. Set **Minimum draft**, in degrees: the least tilt your process needs (often 1° to 3°).

The part is coloured:

| Colour | Means |
|---|---|
| Green: **Positive draft** | Tilted toward the pull by at least the minimum: releases from this half of the mould |
| Ochre: **Needs draft** | Within the minimum either way, such as a vertical wall: add draft here |
| Red: **Negative draft** | Tilted away from the pull by at least the minimum: releases from the other half, or is an undercut if the part is to come out of one half |

Each class's share of the part's surface area is shown beside it. **Reverse the pull** looks from the other half of the mould: green and red swap.

Curved faces are judged point by point, so a cylinder lying across the pull shows a band that needs draft along its sides.

The colours follow the part as you change it. {{key:Esc}} or {{ui:Close}} turns the analysis off; nothing is saved in the part.

:::tip
To add draft, use the {{ui:Draft}} feature on the ochre faces (see [Sweep, loft and draft](topic:sweep-loft-draft)), then check again.
:::
