---
title: Sheet metal: flanges and the flat pattern
summary: Start a sheet metal part from a sketch (a flat plate or a bent profile), set its thickness, bend radius and K-factor once, bend flanges up from its edges with bend reliefs, and lay it flat as a DXF for cutting.
section: tasks
order: 94
keywords: flat pattern, unfold, development, developed length, bend allowance, bend line, dxf for laser, laser cutting, sheet metal, base flange, edge flange, flange, bend, bend radius, k-factor, k factor, thickness, gauge, relief, bend relief, channel, bracket, enclosure, bent sheet
---

:::note
**Not built yet in this version.** Sheet metal needs an exact geometry kernel, which this version of PartCraft does not have: it builds every part with the mesh kernel. [What is not built yet](topic:not-built-yet) says what to do instead. The rest of this page describes it as it works with an exact kernel.
:::

Sheet metal parts start with a **Base flange**. It sets the sheet for the whole part: its **thickness**, the **inside bend radius** and the **K-factor**. Every **Edge flange** after it bends with those same numbers, so the part stays one sheet that can be unfolded. Both need the **exact kernel** (see [The mesh kernel](topic:solid-kernels)).

## Start with a base flange

1. Draw a sketch. Use a **closed profile** for a flat plate (a rectangle, or any closed outline with holes). Use **one open chain of lines** for a bent sheet, such as an L, a U or a Z drawn as the sheet's edge view.
2. Select the sketch in the feature history.
3. Open {{ui:More}} in the Features group and choose {{ui:Base flange}}.

In the panel on the right:

- **Thickness (mm)** is the sheet's gauge.
- **Bend radius (mm)** is the inside radius of every bend. 0 gives a sharp inside corner; the outside still turns with a radius of one thickness.
- **K-factor** says where the neutral layer sits inside the sheet, from 0 (the inside face) to 0.5 (the middle). It decides how long the sheet is when laid flat. 0.44 is the default; your shop or material supplier can give you theirs.
- **Depth (mm)** is how far a bent profile runs out of the sketch plane. A plate ignores it.
- **Thickness outside the bends**: an open profile is one face of the sheet. By default the thickness goes on the inside of the first bend, so the lines you drew are the outside of the channel. Tick it to put the lines on the inside instead. The answer is the same whichever end you drew from.

Every corner of an open chain becomes a bend. A line too short to hold its bends says so, with the length it needs.

## Bend an edge flange

1. Click a straight edge of the sheet's top or bottom face (the long edges, not the thin ones).
2. Choose {{ui:More}} › {{ui:Edge flange}}. With no edge picked, PartCraft waits for you to click one; Esc cancels.

The flange bends up from that edge's face around the sheet's inside radius, then runs straight. In the panel:

- **Angle (°)** is how far it bends, from 1 to 180. 90 is a square flange.
- **Length (mm)** is the straight leg after the bend.
- **Bend the other way** bends it from the other face (down instead of up).
- **Offset at the start** and **Offset at the end** shorten the flange from each end of the edge.
- **Bend relief**: when an end is offset, a slot is cut into the sheet beside it so the bend does not tear the metal there. **Relief width** sets the slot's width; its depth is the bend radius plus the thickness. Choose **None** to leave the corner uncut.

To change the edge later, press {{ui:Pick}} in the panel and click another one.

:::tip
Use parameters for the thickness and radius (see [Parameters and equations](topic:parameters)) so a change of gauge updates every flange at once.
:::

## Lay it flat

Both panels end with **Flat pattern**. Press {{ui:Lay flat}}:

- A preview shows the sheet unfolded, with each bend's centre line dashed.
- **Size** and **Area** are those of the flat blank.
- Each bend is listed with its direction (**up** or **down**, seen from the base flange's sketch side), angle and inside radius.

Every bend takes the length of the sheet's neutral layer through it, the **bend allowance**: angle (in radians) × (inside radius + K-factor × thickness). So a 90° bend of a 2 mm sheet with a 2 mm radius and K-factor 0.44 adds 4.52 mm of flat sheet.

On a flat plate, holes, cuts and bend reliefs show in the flat where they really are, including holes through a flange. A bent base flange lays out as one rectangle: its developed length × its depth, with a bend line per corner.

{{ui:Save DXF}} saves the flat pattern for a laser, punch or waterjet, in millimetres:

- the outline on layer **OUTLINE**, as lines, arcs and circles;
- the bend lines on layer **BEND**, dashed;
- a note on each bend on layer **BEND_NOTES**, such as "UP 90° R2".

Change anything and the flat goes away; press {{ui:Lay flat}} again for the new one.

:::note
Check the K-factor with your shop before cutting: the flat's length depends on it, and it varies with material, thickness and tooling.
:::

## Messages

- **Sheet metal needs a base flange before it** — an edge flange was added before any base flange. Move it below one, or add a base flange first.
- **An edge flange needs a straight edge** — flanges bend along lines only; curved edges cannot take one yet.
- **That edge is not on a face of the sheet's thickness** — the edge belongs to a face that is not the sheet's top or bottom (for example a hole's wall). Pick an edge of the top or bottom face.
- **The offsets leave nothing of the edge** — the two offsets add up to the whole edge or more.
- **Edge flanges on a bent base flange cannot be laid flat yet** — start from a flat plate and bend everything up with edge flanges, which unfold.
- **Cuts and holes made after a bent base flange are not shown in its flat yet** — the flat of a bent profile is its developed rectangle only.
