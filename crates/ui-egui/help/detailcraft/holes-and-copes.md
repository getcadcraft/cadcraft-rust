---
title: Add holes and copes
summary: Type holes into webs, flanges and plates, and cope the flanges at a beam end, measured from the part's cut end.
section: tasks
order: 80
keywords: hole, holes, bolt hole, cope, notch, cut, coping, drill, punch, web hole, flange hole
---

Holes and copes are typed in by position. They belong to the member and move with it.

Holes made by connections (end plates, base plates) are added for you; you do not type those. They are listed as "… holes from connections (edit the connection)".

## Where positions are measured from

- **x** runs along the part from its **cut start**: the start end after any connection has cut it back. So x = 0 is the real end of the steel, not the grid point.
- **y** is measured across the face from the section's center:
   - on the **web**: up (positive) or down from mid-depth;
   - on a **flange** or a **plate**: across the width, from the center line.
- The member's start is the first point you clicked when you placed it. The {{ui:Ends (x, y, z mm)}} rows show which end is which.

## Add a hole

1. Select the member.
2. In the {{ui:Holes}} box of the {{ui:Properties}} panel, pick the {{ui:Face}}: {{ui:Web}}, {{ui:Top flange}} or {{ui:Bottom flange}} (for an I-shape, channel or rectangular tube), or {{ui:Plate face}} (for a plate).
3. Type {{ui:x along (mm)}}, {{ui:y across (mm)}} and {{ui:Diameter (mm)}}. The form starts at x 100, y 0, diameter 22.
4. Choose {{ui:Add hole}}.

The status bar says "Hole added. x is from the cut start; y from the section center (web: up, flange/plate: across)." The hole appears in the list, and in the view as a dark plug through the face.

To remove a hole, choose the × beside it in the list.

Angles and round tubes do not take holes yet: their {{ui:Holes}} box says so.

:::note
In the 3D view holes are drawn as dark plugs, not cut through the steel. They are real holes in the NC files, the shop drawings and the IFC export.
:::

A hole whose x is beyond either end of the part shows in the {{ui:Problems}} tab as "a hole is off the end of the part".

## Add a cope

Copes are for I-shapes: they cut away the top or bottom flange (and that depth of web) at one end, so the beam clears another member.

1. Select an I-shape member.
2. In the {{ui:Copes}} box, pick {{ui:End}} ({{ui:Start}} or {{ui:End}}) and {{ui:Side}} ({{ui:Top}} or {{ui:Bottom}}).
3. Type {{ui:Length (mm)}} (along the beam from that end) and {{ui:Depth (mm)}} (down from the top, or up from the bottom). The form starts at 100 x 40.
4. Choose {{ui:Add cope}}.

The view shows the cope cut out of the steel. The list reads, for example, "Top at the start: 100 x 40". Remove it with its ×.

Copes are square: there is no radius at the inside corner yet.

## How holes and copes affect marks

Two parts get the same part mark only if their holes and copes match within 1 mm, allowing for the part being turned over. Adding a hole to one beam of a group gives it a new mark. See [Mark rules](topic:mark-rules).
