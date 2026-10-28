---
title: Equations both ways
summary: Change a parameter and the part follows; type a new size into a driven number and PartCraft works out the parameter behind it, asking which one when several could change.
section: tasks
order: 104
keywords: equations, solve backward, driven dimension, two way, reverse, goal seek, parameters, linked values, which parameter
---

An equation such as **Extrude1 depth = thick + extra** works in both directions.

## Forward: change the parameter

On the {{ui:Parameters}} tab, change `thick` from 20 to 30. Extrude1's depth becomes 30 + extra, and the part rebuilds.

## Backward: change the driven number

1. Click "Extrude1" in the history. Its **Depth (mm)** box shows **ƒ**: an equation drives it.
2. Type the size you want, for example `40`, and press {{key:Enter}}.

PartCraft solves for the parameter behind the equation, through as many steps as it takes (`depth = hub_h`, `hub_h = disc_t + 50`: typing a height changes `disc_t`). The status bar says what changed, for example that extra went from 5 to 15.

When two or more parameters drive the number, a box asks **Which parameter should change?** and lists them. Click one; the others keep their values. {{ui:Cancel}} changes nothing.

The same works in a sketch: double-click a driven dimension, type a value, press {{key:Enter}}. It also works in the **Value** column of the parameter table, for a parameter that is itself an expression.

## When it cannot

- A number that must be whole (a pattern count) or stay in range (an angle up to 360°) is refused before anything changes.
- An equation of fixed numbers only (`=40`) has no parameter to change: the number you type replaces it.
- If no value of the parameter gives the size you typed, the status bar says so and nothing changes.
- A sketch that cannot be solved with the new value keeps its previous values, and the status bar names the sketch.

Every change, either way, is one step for {{key:Mod+Z}} and {{key:Mod+Y}}.
