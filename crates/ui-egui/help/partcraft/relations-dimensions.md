---
title: Fully define a sketch with relations and dimensions
summary: Add relations from the right panel and dimensions with the Dimension tool, read the under, fully and over defined colors, and fix a sketch that is over defined.
section: tasks
order: 20
keywords: relations, constraints, dimensions, smart dimension, fully defined, under defined, over defined, degrees of freedom, coincident, horizontal, vertical, parallel, perpendicular, tangent, equal, concentric, midpoint, fix, conflict, redundant
---

A sketch is **fully defined** when every point and every size is fixed by relations and dimensions. A fully defined sketch changes only when you change a number, so the part behaves predictably when you edit it later. PartCraft does not force you to fully define a sketch, but it always shows how close you are.

## Read the sketch state

The state shows in three places: the chip at the top left of the view, the message at the top of the right panel, and the flag in the history.

| State | Chip | Geometry color | What it means |
|---|---|---|---|
| Under defined | "Under defined · 3 degrees of freedom" | Blue | Some geometry can still move. Drag blue geometry to see which way |
| Fully defined | "Fully defined" | Ink (black on Paper, light on Model) | Every point and size is fixed |
| Over defined | "Over defined" | Red on the relations involved | Some relations contradict each other, or one is already implied by others |

Construction geometry is gray and dashed whatever its state. Selected items are orange. A "degree of freedom" is one way something can still move: a free point has two (left-right and up-down), a free circle has three (its center and its radius).

The legend under the message in the right panel repeats the colors.

## Add a dimension

1. Press {{key:D}} or choose {{ui:Dimension}}.
2. Click what to measure:
   - **one line**: its length;
   - **one circle**: its diameter; **one arc**: its radius;
   - **two points** (an end point, a center, a loose point or the origin): the distance between them;
   - **a point and a line**: the distance from the point to the line;
   - **two parallel lines**: the distance between them; **two lines at an angle**: the angle between them.
3. After one item, the status bar says "Click a second item, or click where the dimension goes." Click a second item, or click in empty space where you want the number.
4. A number box opens with the current value. Type the size and press {{key:Enter}}. {{key:Esc}} keeps the measured value.

**Where you place the number decides the kind** for a slanted line or two points: place it above or below to get a horizontal distance, to the left or right to get a vertical distance, and elsewhere for the true (aligned) distance.

The box accepts plain numbers in millimeters or degrees, such as `25`, `12.5` or `1e3`. A leading `⌀`, `Ø` or `R`, or a trailing `mm` or `°`, is ignored. Angles are 0 to 180 degrees. Arithmetic such as `20+5` is not accepted.

To change a dimension later, double-click its number in the sketch, or choose **Edit** beside it in the list in the right panel.

:::tip
You can also select geometry first (Select tool) and choose a dimension button in the right panel: {{ui:Distance}}, {{ui:Horizontal distance}}, {{ui:Vertical distance}}, {{ui:Radius}}, {{ui:Diameter}} or {{ui:Angle}}. Only the ones that fit the selection are shown. This is the way to give a circle a radius instead of a diameter.
:::

## Add a relation

Relations are added from the right panel, not with a tool.

1. Press {{key:S}} for Select.
2. Click one item, then {{key:Shift}}+click a second one if the relation needs two. The origin can be selected too.
3. The right panel says "Selected: line l3, point p1" and shows a button for each relation that fits, such as {{ui:∥ Parallel}} or {{ui:• Coincident}}.
4. Choose the button. The sketch solves at once.

If no relation fits, the panel says "No relation fits this selection. Select one or two items (Shift+click adds)."

The full list of relations, what each one needs and what it does is in [Sketch relations and dimensions](topic:sketch-relations).

Relations show in the sketch as small symbols next to the geometry (H, V, ∥, ⊥, T, =, ◎, M, ⚓). Coincident dots are hidden unless the geometry is selected or the relation is in trouble. Click a symbol to select the relation.

## A worked example: an L-shaped bracket profile

1. Sketch on **Front plane**. With the Line tool, start on the origin and click six corners of an L, finishing on the origin again. The chain ends by itself when you click the first point.
2. Lines drawn close to horizontal or vertical already have H and V relations, except the last one: a line that ends on an existing point gets no automatic relation. Press {{key:S}}, click that last line and choose {{ui:V Vertical}} in the right panel. The chip says "Under defined · 4 degrees of freedom".
3. Press {{key:D}}. Click the bottom line, click below it, type `80`, {{key:Enter}}.
4. Click the left line, click left of it, type `60`, {{key:Enter}}.
5. Dimension the top short line `20` and the inner vertical line `45` the same way.
6. The chip now says **Fully defined** and the whole L turns to ink.

## Fix an over defined sketch

When a new relation or dimension contradicts the others, PartCraft does not move anything. The geometry stays exactly as it was, the relations involved turn red, and the status bar names them:

- "Over defined: Horizontal, Distance 40 cannot all be met. Undo, or delete one of them." The relations conflict.
- "Over defined: Parallel is already implied. Delete it." The relation adds nothing: the others already force it. (A rectangle's sides are already parallel, for example.)

To fix it:

1. Press {{key:Mod+Z}} to undo the last change, or
2. Find the red item in the **Relations and dimensions** list in the right panel and choose **Delete** beside it. You can also click its symbol or number in the sketch and press {{key:Delete}}.

:::note
There are no "driven" (reference) dimensions in this build. A dimension that would only report a size the other relations already fix is treated as over defined. Read sizes in the panel instead: select a line and look at the Dimension buttons, or check the part facts after you extrude.
:::

An over defined sketch with a real conflict makes the feature that uses it fail ("Over defined: 1 relation cannot all be met."), and the history marks it **⚠ error**. A sketch that is only redundant still builds, with a ⚠ warning.

## Drag to find what is free

In Select, drag any blue point or line. The solver moves it as little as possible while keeping every relation. If a drag would break a relation, the geometry snaps back. This is the quickest way to see which dimension is still missing.

## The relations list

With nothing selected, the right panel lists **Relations and dimensions**. Each row has:

- **Edit** (dimensions only): opens the number box;
- **Select**: selects it in the sketch;
- **Delete**: removes it.

With geometry selected, the list shows only the **Relations on the selection**.
