---
title: Change sizes without opening the sketch
summary: Drive a part from the numbers in the right panel: sketch dimensions, depths, counts and angles, how to plan a part so the right numbers drive it, and where parameters and equations take over.
section: tasks
order: 100
keywords: parameters, driving dimensions, design intent, change size, equations, global variables, design table, configurations, resize, modify dimension
---

Every size in a PartCraft part is a number you can change later: the dimensions in its sketches and the settings of its features. You do not need to open a sketch to change its dimensions.

## Change a sketch dimension from the panel

1. Click a feature in the history, for example "Extrude1".
2. Under the extrude's own settings, **Sketch dimensions (Sketch1)** lists every dimension of its sketch: "Distance", "Diameter", "Angle (°)" and so on.
3. Type a new value in one box and press {{key:Enter}} or {{key:Tab}}.

The sketch solves with the new value, and the whole part rebuilds. Selecting the sketch itself shows the same list as **Driving dimensions**.

A radius or diameter must be more than zero, and an angle is between 0 and 180 degrees; the status bar says so if not.

:::tip
Which box is which? The boxes are listed in the order the dimensions were made. If you are not sure, open the sketch (double-click it), double-click the dimension on the canvas, and change it there.
:::

## Type the main number straight away

Select a feature in the history (or just add it) and type a number: it goes into the feature's main box, the one you change most (an Extrude's **Depth**, a Revolve's **Angle**, a Hole's **Diameter**, a pattern's **Count**, a Fillet's **Radius**, a Shell's wall). Press {{key:Enter}} to set it, or {{key:Esc}} to keep the old one. Adding an Extrude says so in the status bar: "Extrude1 added. Change its numbers on the right, or type a number now for its depth."

## The numbers that drive a part

| Where | What you can change |
|---|---|
| Sketch | Every dimension: lengths, distances, radii, diameters, angles |
| Extrude and Cut | Depth, end condition, direction, boss or cut |
| Revolve and revolved cut | Angle, axis, boss or cut |
| Hole | Diameter, depth, through all, counterbore diameter and depth |
| Fillet, Chamfer, Shell | Radius, distance, wall thickness and direction, and which edges or faces |
| Linear and Circular | Count, spacing or angle, direction or axis |
| Part | Curves (how finely round shapes are faceted) |
| Drawing | Where the section cuts, the detail's size and scale, GD&T values |

## Plan the part so the right numbers drive it

The numbers you give are the ones you can change. A few habits make later changes behave:

- **Dimension what matters for the job.** If a hole must stay 15 mm from an edge, dimension it from that edge, not from the origin.
- **Fully define sketches.** Then a change moves only what it should (see [Fully define a sketch](topic:relations-dimensions)).
- **Use relations for intent.** Equal on two holes means one diameter drives both. Symmetric shapes: put the origin on the center line, or use Midpoint.
- **Sketch on faces made by features.** A sketch on an extrude's end face moves when the depth changes.
- **Pattern instead of copying.** One hole patterned eight times has one diameter to change.

## Beyond single numbers

- **One number driving several:** use parameters and equations: [Drive a part with parameters](topic:parameters).
- **Several sizes of one part:** save each as a [configuration](topic:design-tables); they are kept in the part file.
- **Arithmetic in a number box:** type `=40/2` or `=width / 2`. That makes an equation; see [Equations both ways](topic:equations-two-way).
- **Inch units:** the part shows millimeters. In a number box with `=`, or in a parameter, `2in` is turned into 50.8.

See [What is not built yet](topic:not-built-yet).
