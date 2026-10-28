---
title: Pattern and mirror features
summary: Repeat features in a row with Linear, round an axis with Circular, and copy features or the whole body across a plane with Mirror.
section: tasks
order: 70
keywords: pattern, linear pattern, circular pattern, polar array, array, mirror, symmetric, copy features, bolt circle, seed, mirror body
---

Patterns and mirrors copy features you have already built. The copies are part of the history: change the original and the copies follow.

What gets copied is what the feature does: a boss pattern adds material, a cut or hole pattern removes it. Patterns can be patterned and mirrored again. An imported STEP solid can be copied too; fillets, chamfers and shells cannot.

## Linear pattern

1. Select the feature to copy in the history (for example a Hole or a Cut). With nothing selected, the last solid feature is used.
2. Choose {{ui:Linear}}. Three copies appear 20 mm apart along X.
3. In the right panel, set:

| Setting | What it does | Default |
|---|---|---|
| **Features to copy** | Tick every feature to copy. Only features above the pattern are listed | The selected feature |
| **Direction** | X axis, Y axis, Z axis, a line or centerline from any earlier sketch, the axis of a revolve, or a picked edge | X axis |
| **Spacing (mm)** | Distance between neighbors | 20 |
| **Count** | Total number, **including the original** (1 to 1000) | 3 |
| **Reverse direction** | Copies go the other way | Off |

### Use an edge as the direction

1. Click a straight edge of the part in the view. The right panel says **Edge** and shows its length.
2. Choose {{ui:Linear}}: the pattern runs along that edge.

Or select the pattern, pick an edge, and choose "Picked edge (use the one picked now)" in **Direction**. A picked edge is fixed in space: it does not follow later changes. A sketch line follows its sketch.

## Circular pattern

1. Select the feature to copy (for example one hole near the rim of a disc).
2. Choose {{ui:Circular}}. Six copies appear equally spaced all the way round.
3. Set:

| Setting | What it does | Default |
|---|---|---|
| **Features to copy** | As for Linear | The selected feature |
| **Axis** | X, Y or Z axis, a sketch line, the axis of a revolve, or a picked edge | The last revolve's axis if there is one, else the Y axis |
| **Angle (°)** | 360 spaces the copies equally all round. Less spreads them over that angle, first and last at the ends | 360 |
| **Count** | Total number, including the original | 6 |
| **Reverse direction** | Turns the other way | Off |

The world axes pass through the origin. For a disc not centered on the origin, use a sketch centerline or a revolve's axis.

### Example: the eight bolt holes of a flange

1. Revolve the flange (see [Revolve a turned part](topic:revolve)) about the Y axis.
2. Sketch on the flange's flat top face, put one point on the bolt circle, and make a Hole.
3. With the Hole selected, choose {{ui:Circular}}. The axis is already "Axis of Revolve1".
4. Set **Count** to `8`.

## Mirror

1. Select the feature to mirror in the history, or select nothing to mirror **the whole body as built so far**.
2. Choose {{ui:Mirror}}. It mirrors across the **Right plane**.

To mirror the whole body across a face of the part instead, click that flat face first, then choose {{ui:Mirror}}. (Clicking a face clears the history selection, so this always mirrors the whole body.)

Settings in the right panel:

- **Mirror**: tick **The whole body (as built so far)**, or clear it and tick the features to copy.
- **Mirror plane**: Front plane, Top plane, Right plane, the face it uses now ("A face of Extrude1" or "A fixed plane"), or "The face picked now". To mirror chosen features across a face: select the Mirror in the history, click the face in the view, and choose "The face picked now".

Mirroring the whole body across a face doubles the part, for example a half bracket into a full one.

## Messages

- "Build a feature to copy first." or "Build a solid first." There is nothing solid to copy yet.
- "Choose the features to copy." Every **Features to copy** box is cleared.
- "Extrude2 has no solid to copy." That feature failed or is suppressed.
- "Fillet1 changes the body's edges or walls; it cannot be copied. Copy the features that make solids (extrudes, revolves, holes, imports)." Fillets, chamfers and shells cannot be patterned or mirrored; pattern first, then fillet.
- "There is no body to mirror yet."

:::note
Pattern copies cannot be skipped one by one, and a pattern cannot follow a sketch curve. Sketch patterns and mirrors are not built yet either: draw repeated sketch geometry by hand or with **Add by coordinates**.
:::
