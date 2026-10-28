---
title: Connect beams to columns with end plates
summary: Add bolted end plates where a beam meets a column flange or web, set the plate, bolts and weld, and copy the connection to every similar joint.
section: tasks
order: 60
keywords: end plate, endplate, moment connection, bolted connection, joint, bolts, weld, extended end plate, flush, similar, copy connection, component
---

An end plate connection welds a plate to the beam end and bolts it to the column flange or web. DetailCraft places the plate, cuts the beam back, drills matching holes in the plate and the column flange, adds the bolts and the weld, and checks edge distances and spacing.

:::warning
DetailCraft does not design connections. The settings are starting values for you to change; nothing checks strength. The edge and spacing warnings use the minimums of EN 1993-1-8 Table 3.3, which may differ from the rules you work to.
:::

## What it can connect

- A **level** beam (both ends at the same height), upright (rotation 0), with an I-shape profile.
- Into the **flange** or the **web** of a **vertical** I-shape column, square to it.
- The beam end must sit on the column's line, between the column's bottom and top.

Skewed or sloped beams and non-I sections are refused with a reason: use [shear tabs](topic:shear-tabs) for those. On a web the plate and the beam flanges must fit between the column flanges; DetailCraft warns when they do not.

:::tip
At rotation 0 a column's flanges face along the numbered grid direction (x). A beam running from A to B meets the web, and gets an end plate on the web.
:::

## Add end plates to a beam

1. Choose {{ui:Select}} ({{key:V}}) and click the beam.
2. Choose {{ui:End plates}} in the {{ui:Connect}} group, or {{ui:Add end plates}} in the {{ui:Properties}} panel.
3. Every free end of the beam that meets a column flange or web square gets a plate. The status bar says "2 end plates added. Select one to change its plate, bolts and weld; Similar copies it to joints like it."
4. The first new end plate is selected and its settings are in the {{ui:Properties}} panel.

If nothing happens, read the status bar:

- "This beam does not end on a column line. End plates go where a beam end meets a vertical column."
- "This beam meets a column at an angle or slopes: end plates are built for square, level joints. Use Shear tabs."
- "Both ends already have connections."

## Change the plate, bolts and weld

Click the end plate (or one of its bolts) to select the connection. Under {{ui:Plate, bolts and weld (mm)}}:

| Setting | Default | What it does |
|---|---|---|
| Plate thickness | 20 | The plate's thickness; also how far the beam is cut back past the flange |
| Plate width | 180 | Across the joint |
| Extension above beam | 0 | Above the top flange; above 0 makes an extended end plate |
| Extension below beam | 0 | Below the bottom flange |
| Bolt diameter | 20 | Nominal bolt size |
| Hole diameter | 22 | Your clearance hole in the plate and the column flange |
| Bolt grade | A325 | Text; printed in the bill of materials |
| Bolt rows | 4 | Rows of two bolts, centered on the plate height |
| Row pitch | 75 | Vertical spacing between rows |
| Gauge | 100 | Horizontal spacing between the two bolts of a row |
| Bolt length allowance | 30 | Added to the grip for the nut, washer and thread; see below |
| Fillet weld size | 8 | All around the beam end |
| Plate material | A36 | Text |

Type a new value and press {{key:Enter}} or {{key:Tab}}. The plate, holes, bolts and the beam's cut length update at once. Each change is one undo step.

Below the settings you see what the connection makes:

- {{ui:Bolts}}: for example "8 × 20 × 70 A325 (grip 36.2)".
- {{ui:Weld}}: size, where, and its length.
- {{ui:Plate}}: its size and its part mark.

### How the numbers are worked out

- The beam is cut back by half the column depth (on a web: half the web thickness) plus the plate thickness, so the plate sits on the face.
- The plate height is the beam depth plus the two extensions.
- Bolt length = grip (plate thickness + column flange or web thickness) + allowance, rounded up to the next 5 mm.
- The weld length shown is the perimeter of the beam section.

### Warnings

The connection still builds, but these appear under it and in the {{ui:Problems}} tab:

- Bolt rows or bolts closer to the plate edge than 1.2 × the hole diameter.
- Row pitch under 2.2 × the hole diameter, or gauge under 2.4 × the hole diameter.
- Holes that run into the column web or its fillet, or off the column flange.
- A plate wider than the column flange, or (on a web) wider than the space between the flanges; beam flanges that do not fit between them.
- A bolt row too close to a beam flange, or a gauge too tight for the beam web and its weld.

## Copy it to every similar joint

1. Select the connection you have set up.
2. Choose {{ui:Apply to similar (n)}} in the {{ui:Properties}} panel, or {{ui:Similar}} in the {{ui:Connect}} group's {{ui:More}} menu. The number in brackets is how many joints will get it.
3. Every joint with the same beam profile and column profile, beam into the same kind of face, that has no connection yet gets a copy with the same settings. The status bar says how many.

Copies are independent: changing one later does not change the others. Joints that already have a connection are left alone; delete those first if you want them replaced.

## Moving and deleting

Connections are remade from their settings every time the model changes, so moving or resizing a beam or column carries its end plates along. If a change means the joint no longer fits, the connection stays in the model and the {{ui:Problems}} tab says why, for example "The beam end is not on the column line any more."

To delete a connection, select it and press {{key:Delete}}. Deleting the beam or the column deletes its connections too.

Next: [Add base plates and anchor rods](topic:base-plates).
