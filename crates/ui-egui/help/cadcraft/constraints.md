---
title: Constrain a drawing
summary: Keep lines horizontal, ends together and sizes fixed with geometric and dimensional constraints, and let the drawing follow when you change a size.
section: tasks
order: 125
keywords: constraint, constraints, parametric, geomconstraint, gcon, dimconstraint, dcon, gchorizontal, gcvertical, gcparallel, gcperpendicular, gccoincident, gctangent, gcequal, gcfix, gcsymmetric, gccollinear, gcconcentric, dclinear, dchorizontal, dcvertical, dcaligned, dcradius, dcdiameter, dcangular, dcconvert, autoconstrain, delconstraint, constraintbar, cbar, dcdisplay, degrees of freedom, fully constrained
---

## What you will make

A plate whose width you change in one place: its sides stay square, one corner stays put, its height stays half its width.

Constraints work on lines, polylines, circles, arcs and points on the {{ui:Model}} tab. Each one you add is one undo step.

## Draw and constrain the outline

1. Type `REC` and press {{key:Enter}}. Type `0,0`, {{key:Enter}}, `100,50`, {{key:Enter}}.
2. Type `AUTOCONSTRAIN` and press {{key:Enter}}. Type `ALL`, {{key:Enter}}, then {{key:Enter}} again.

   The history says "4 constraints applied". Small boxes with **H** and **V** appear beside the sides: these are the constraint bars.
3. Type `GCFIX` and press {{key:Enter}}. Click near the lower-left corner. A box with ⊕ appears: that corner stays where it is.

`AUTOCONSTRAIN` adds coincident constraints where ends nearly meet and horizontal or vertical ones where lines are nearly level or plumb. Type `S` at its prompt to change how near (0.05 drawing units and 1° to start with).

## Add the sizes

1. Type `DCHORIZONTAL` and press {{key:Enter}}.
2. Click near the lower-left corner, then near the lower-right corner.
3. Click below the rectangle for the dimension line.
4. The prompt shows `Dimension text <d1=100>`. Type `width=120` and press {{key:Enter}}.

   The rectangle is now 120 wide, and the label reads `width=120`.
5. Type `DCVERTICAL` and press {{key:Enter}}, type `O` and press {{key:Enter}}, then click the right side.
6. Click to the right of it, type `width/2` and press {{key:Enter}}.

   It is 60 high. The label reads `fx: d1=width/2`: an expression drives it.

The history now says the outline is fully constrained. To change its size, change `width` (see [Use parameters and equations](topic:parameters)), or double-click the `width=120` label, type a new value and choose {{ui:Apply}}.

## Moving constrained objects

Move, rotate, scale, grips and the Properties panel all keep the constraints: what you moved is held, and the rest follows. When the constraints cannot follow, the edit is not made and the history says why, for example "Not done: the constraints hold that object where it is". Erasing an object removes its constraints with it.

## Mirror and trim constrained objects

- {{cmd:MIRROR}} copies the constraints among the objects you mirror onto the copies. Sides (which way a tangent touches, which side a distance is measured to) are worked out again for the mirrored copy, and copied dimensions get names of their own, as a paste does. With `Erase source objects` `Y`, the constraints stay on the mirrored objects. The history says how many came along.
- {{cmd:TRIM}} keeps a line's or an arc's constraints on what is left: a constraint on an end goes to the piece that still has that end, a rule about direction ({{ui:Horizontal}}, {{ui:Vertical}}, {{ui:Parallel}}, {{ui:Perpendicular}}) holds on every piece, and a dimension between the two outer ends keeps measuring them. Constraints that held the part that was cut away, or a length the cut changed ({{ui:Equal}}, an arc's sweep), go. A trimmed circle keeps its center and radius constraints. Trimmed polylines lose theirs.

## The constraints

| Command | Keeps |
|---|---|
| {{cmd:GCHORIZONTAL}}, {{cmd:GCVERTICAL}} | a line or segment level or plumb (`2P`: two points level or plumb) |
| {{cmd:GCPARALLEL}}, {{cmd:GCPERPENDICULAR}}, {{cmd:GCCOLLINEAR}} | two lines parallel, square or on one line |
| {{cmd:GCCOINCIDENT}} | two points together; `O`: a point on a line, circle or arc |
| {{cmd:GCTANGENT}} | a line and a circle or arc, or two of them, touching |
| {{cmd:GCCONCENTRIC}}, {{cmd:GCEQUAL}} | two circles or arcs on one centre; two lengths or two radii equal |
| {{cmd:GCSYMMETRIC}} | two points, lines or circles mirrored across a line |
| {{cmd:GCFIX}} | a point (click near it) or a whole object (`O`) where it is |
| {{cmd:DCLINEAR}}, {{cmd:DCHORIZONTAL}}, {{cmd:DCVERTICAL}}, {{cmd:DCALIGNED}} | a distance |
| {{cmd:DCRADIUS}}, {{cmd:DCDIAMETER}}, {{cmd:DCANGULAR}} | a radius, a diameter, an angle between two lines or an arc's angle |

{{cmd:GEOMCONSTRAINT}} and {{cmd:DIMCONSTRAINT}} ask which one; {{ui:Constrain}} and {{ui:Dimension}} in the {{ui:Parametric}} group run them, and {{ui:More}} there lists each. Where you click picks the part: a click near an end, the middle or a center takes that point; a click on a polyline takes the segment under it. The first object stays and the second moves to meet it.

A constraint that conflicts with others, or adds nothing because the others already imply it, is refused with a message and nothing moves.

## See, select and remove

- Click a box in a constraint bar (or a coincident square, or a dimension label) to select that constraint; the history names it. Press {{key:Delete}} to remove it.
- {{cmd:DELCONSTRAINT}} removes every constraint from the objects you select.
- {{cmd:CONSTRAINTBAR}}: `S` shows every bar, `H` hides them; select objects first to show or hide theirs. {{ui:More}} has {{ui:Show constraint bars}} and {{ui:Hide constraint bars}}.
- {{cmd:DCDISPLAY}} shows or hides the dimensional constraints. They never plot.
- {{cmd:DCCONVERT}} turns dimensions drawn on lines, circles and arcs into dimensional constraints.

:::note
If you have used AutoCAD's parametric drawing, the command names and prompts are the same. Constraints and parameters are saved in DWG and DXF files in a form other programs ignore; see [Files](topic:files#constraints-in-files).
:::
