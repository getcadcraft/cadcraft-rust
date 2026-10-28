---
title: Dimension and resize what you drew
summary: Place dimensions between corners, on arcs and circles, or between two edges; type a new length to resize the model. Retype an edge's length or a group's width, depth and height in Entity info.
section: tasks
order: 85
keywords: dimension, dimensions, resize, change size, edit size, length, width, depth, height, stretch, driving dimension, annotate, measure, after the fact, radius, diameter, angle, angular, arc, circle, R, Ø
---

Sizes in ModelCraft are not fixed once something is drawn. A dimension shows a length on the model, and typing a new length into it resizes the model to match. You can also retype an edge's length, or a group's width, depth and height, in {{ui:Entity info}}.

## Place a dimension

1. Press {{key:D}}, or click {{ui:Dimension}} in the toolbar.
2. Click a corner to measure from. It snaps to corners, midpoints and edges, as drawing does.
3. Click the second corner.

   Or click once on an **edge** (the tip says "On edge") to dimension that whole edge in one click.
4. Move the pointer to where the dimension line should go, and click. You can also type the offset, such as `0.5m`, and press {{key:Enter}}.

   The dimension line stays level for a level edge, unless you are looking at it from the side. For an upright edge it stays in the upright plane that faces you best.

The status bar says "Dimension 4 m." Dimensions show their length in the model's unit and update when you change the unit in the picker.

## Radius, diameter and angle

The same {{ui:Dimension}} tool dimensions round things and angles:

- **An arc:** click the arc. The tip says {{ui:Radius}}. Move the pointer to where the text should go and click: the dimension reads **R 1.5 m**.
- **A circle:** click the circle. You get a **diameter** (**Ø 3 m**) with a line across the circle through its centre. Polygons work the same way, with their corners on the circle.
- **The angle between two edges:** click one edge, then click a second edge that meets it at a corner (while you are over it, the tip says {{ui:Angle between the edges}}). Then move into the angle you want, and click. Inside the corner you get the angle between the edges; move to one side and you get the angle beside it (for example 150° instead of 30°). How far from the corner you click sets where the arc goes; or type that distance.

To switch a radius to a diameter or back, select the dimension and choose in {{ui:Show as}} in {{ui:Entity info}}.

**Resizing with them:** type a new radius or diameter right after placing one, or select it later and type. The arc or circle keeps its centre and changes size; a face it bounds (a disc, the end of a slot) changes with it. Two things only measure:

- **An angle dimension** never changes the model. To change an angle, rotate the edges with {{ui:Rotate}}.
- **The circle at the end of a cylinder** (a circle you pulled with {{ui:Push/pull}}) cannot be resized from its dimension, because its sides would have to change too. Scale the cylinder instead, about its axis.

## Resize the model with a dimension

Right after placing a dimension, type a new length and press {{key:Enter}}: `6m`, `12'`, `3500` in a millimeter model. The model changes to that size.

Later, at any time:

1. With {{ui:Select}}, click the dimension's text or line. {{ui:Entity info}} says **Dimension** and shows its {{ui:Length}}.
2. Type the new length and press {{key:Enter}}. You can type straight into the value box, or into the {{ui:Length}} box in {{ui:Entity info}}.

### What moves

The second corner you clicked moves, and so does everything beyond it:

- **Both corners on one piece** (a box's width, a wall's height): the side at the moving corner slides out or in, and faces joined to it stretch. A box stays a closed solid, and anything sitting wholly on that side, such as a window component, moves with it.
- **The corners on two separate pieces** (the gap between two boxes): the moving corner's piece moves whole, and the gap changes.
- **The moving corner inside a group or component** that the other corner is not in: the whole group moves.
- **Both corners inside the same group or component**: it is stretched inside. A component changes everywhere it is placed, as editing it always does.

To change which end moves, select the dimension and choose in {{ui:Moves}} in {{ui:Entity info}}:

| Choice | What happens |
|---|---|
| {{ui:Second end}} (the default) | The end you clicked second moves |
| {{ui:First end}} | The end you clicked first moves |
| {{ui:Both ends (centered)}} | Each end moves half the change, so the middle stays put |

### When a resize is refused

ModelCraft changes nothing and says why in the status bar:

- "a face would bend out of its plane": the dimension runs at a slant across the shape. Dimension along an edge, or along red, green or blue.
- "a face would shrink to nothing or turn inside out": the new length is too short for the shape.
- "the end that moves is not on a corner of the model": that end was placed in empty space or on the middle of an edge. Choose the other end in {{ui:Moves}}, or place the dimension on two corners.
- "a length must be above zero".

Every resize is one step: {{key:Mod+Z}} undoes it.

## Retype a size in Entity info

You don't always need a dimension:

- **An edge:** select it. Its {{ui:Length}} in {{ui:Entity info}} is a box you can type into. Typing a length into the value box works too. The end further along red, then green, then blue moves, and what is joined to it stretches. For example, retyping the front bottom edge of a box makes the box wider to the right.
- **A group or component:** select it. {{ui:Width (red)}}, {{ui:Depth (green)}} and {{ui:Height (blue)}} are boxes. Its far side (right, back, top) moves. These are the group's own axes, before any rotation you gave it.

Press {{key:Enter}} to apply, or {{key:Esc}} to put the old value back.

## Dimensions follow the model

A dimension is attached to the two corners it was placed on (an angle: to its corner and the far ends of its two edges; a radius or diameter: to its arc or circle). Move, push/pull or resize the geometry, and the dimension follows and shows the new length. It stays attached when you make the geometry into a group or component.

If an end was placed somewhere that isn't a corner (on a face, or in empty space), or its corner is erased, that end stays where it was. The dimension still measures, but it can only resize from its other end.

## Remove dimensions

- One dimension: select it and press {{key:Delete}}, or click it with the {{ui:Eraser}}.
- All dimensions: with nothing selected, click {{ui:Delete all dimensions}} in {{ui:Entity info}}.

Dimensions are saved in the `.modelcraft` file. They are not exported to other formats, which carry only the 3D model. A file with dimensions still opens in an older ModelCraft, without them.
