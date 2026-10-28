---
title: Sketch relations and dimensions
summary: Every relation and dimension a PartCraft sketch has, what to select for each, its symbol, and how it behaves when solving.
section: reference
order: 20
keywords: relations, constraints, coincident, horizontal, vertical, parallel, perpendicular, tangent, equal, concentric, midpoint, fix, distance, horizontal distance, vertical distance, radius, diameter, angle, symbols, glyphs
---

Relations fix how geometry sits relative to other geometry. Dimensions fix sizes. To add a relation, select items with the Select tool ({{key:S}}) and choose its button in the right panel; to add a dimension, use the Dimension tool ({{key:D}}) or the dimension buttons in the right panel. How to work with them: [Fully define a sketch](topic:relations-dimensions).

"Curve" below means a circle or an arc. The origin counts as a point.

## Relations

| Button | Symbol | Select | Effect |
|---|---|---|---|
| {{ui:• Coincident}} | • (shown only when selected or in trouble) | Two points | The points meet |
| {{ui:• Coincident}} | • | A point and a line | The point lies on the line (or its extension) |
| {{ui:• Coincident}} | • | A point and a curve | The point lies on the circle or arc |
| {{ui:H Horizontal}} | H | A line, or two points | The line is level, or the two points are at the same height |
| {{ui:V Vertical}} | V | A line, or two points | The line is upright, or the points are one above the other |
| {{ui:∥ Parallel}} | ∥ | Two lines | The lines run the same way |
| {{ui:⊥ Perpendicular}} | ⊥ | Two lines | The lines meet at a right angle |
| {{ui:T Tangent}} | T | A line and a curve | The line touches the curve without crossing |
| {{ui:T Tangent}} | T | Two curves | They touch, outside or inside, as drawn |
| {{ui:T Tangent}} | T | A spline and a line, arc or spline that meets one of its ends | The spline leaves that end in the other curve's direction |
| {{ui:= Equal}} | = | Two lines | Same length |
| {{ui:= Equal}} | = | Two curves | Same radius |
| {{ui:◎ Concentric}} | ◎ | Two curves | Same center |
| {{ui:◎ Concentric}} | ◎ | A point and a curve | The point sits at the curve's center |
| {{ui:M Midpoint}} | M | A point and a line | The point sits at the line's middle |
| {{ui:↔ Symmetric}} | ↔ | Two points and a line | The points are mirror images about the line (Mirror adds these) |
| {{ui:⚓ Fix}} | ⚓ | A point, a line, a curve or a spline | Holds it exactly where it is now (a spline: its points) |

Lines here include centerlines. Drawing adds some relations for you: snapping to a point makes it coincident, snapping to a curve puts the point on it, nearly level or upright lines get H or V, and a rectangle comes with its four corners joined, two H and two V.

**Fix** is a quick way to stop something moving, but the part then cannot follow a dimension change elsewhere. Prefer dimensions to the origin.

## Dimensions

| Button | Select | Measures | Shown as |
|---|---|---|---|
| {{ui:Distance}} | A line | Its length | Distance 40 |
| {{ui:Distance}} | Two points | The straight distance between them | Distance 40 |
| {{ui:Distance}} | A point and a line | The distance from the point to the line, square to it | Distance 40 |
| {{ui:Distance}} | Two parallel lines | The gap between them | Distance 40 |
| {{ui:Horizontal distance}} | Two points, or a line | The left-right distance only | Horizontal distance 40 |
| {{ui:Vertical distance}} | Two points, or a line | The up-down distance only | Vertical distance 40 |
| {{ui:Radius}} | A circle or arc | Its radius | Radius R10 |
| {{ui:Diameter}} | A circle or arc | Its diameter | Diameter ⌀20 |
| {{ui:Angle}} | Two lines that are not parallel | The angle between them, 0 to 180 degrees | Angle 30° |

With the Dimension tool, PartCraft picks the kind from what you click and where you place the number:

| You click | You get |
|---|---|
| One line | Its length; for a slanted line, horizontal if the number goes above or below it, vertical if to the side |
| One circle | Diameter |
| One arc | Radius |
| Two points | Distance; horizontal or vertical as for a slanted line |
| A point and a line | Distance from the point to the line |
| Two lines | Angle, or the distance if they are parallel |
| A point and a circle | Nothing: "Those items cannot be dimensioned together." Dimension to the circle's center point instead |

**Offset** (shown as the distance) is made by the Offset tool: it holds a copied line parallel at that distance, or a copied arc or circle concentric with a radius that much bigger or smaller. The other pieces of the same offset carry an **O** relation and follow the one Offset dimension. See [Splines, trim, offset and other sketch tools](topic:sketch-tools).

Dimension values are millimeters (or degrees), never negative. Radius and diameter must be more than zero. A value of 0 is allowed for distances: two points 0 apart are coincident.

## How solving behaves

- **Least movement.** When a value changes, the solver moves the geometry as little as possible. A line whose length changes grows equally at both ends if nothing holds either end.
- **No flipping.** Distances from a point to a line, horizontal and vertical distances, angles and tangency remember which side they were made on. Changing a value never flips the geometry to the other side.
- **Conflicts leave things alone.** If relations contradict each other, the sketch stays exactly as it was and the conflicting ones turn red.
- **Redundant relations** (implied by others) are named in the status bar ("is already implied. Delete it.") and the feature still builds, with a warning.
- **Tangent joints.** Where a fillet arc meets its lines at a shared end point, tangency is solved in a form that stays well defined there, so sketch fillets do not read as over defined.

## Not available yet

Collinear, coradial and pierce relations; a point on a spline; driven (reference) dimensions; ordinate and arc-length dimensions; equations between dimensions. Use Parallel plus a 0 distance for collinear lines.
