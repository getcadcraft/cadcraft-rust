---
title: Turn the view and use your own coordinates (UCS)
summary: Turn the view 90° or to any angle without moving anything in the drawing, and set a user coordinate system (UCS) so points, ortho, polar tracking, the grid and new text follow a building's own axes.
section: tasks
order: 48
keywords: ucs, user coordinate system, plan, turn view, rotate view, view twist, ucs z, ucs 3point, ucs object, ucs world, named ucs, ucsfollow, turnleft, turnright, turnworld, angled building, rotated grid
---

## What you will learn

How to look at a drawing turned, so a building drawn at an angle reads square on screen, and how to type points along its own axes. Nothing in the drawing moves while you do this: objects keep their coordinates, a turn is not an edit, and the drawing is not marked as changed.

## Turn the view a quarter

1. Click {{ui:View}} › {{ui:More}} › {{ui:Turn view 90° left}} or {{ui:Turn view 90° right}}.

The drawing turns on screen about the middle of the drawing area. Do it again to keep turning.

2. Click {{ui:View}} › {{ui:More}} › {{ui:Back to world}} to level it again.

The commands behind them are {{cmd:TURNLEFT}}, {{cmd:TURNRIGHT}} and {{cmd:TURNWORLD}}. To give one a key, add a line such as `TL=TURNLEFT` to your aliases with {{cmd:ALIASEDIT}}.

## Line up with an angled building

1. Type `UCS` and press {{key:Enter}}, then `OB` and {{key:Enter}}.
2. Click a wall line near the end that should be the origin.

The UCS now starts at that end with its X axis along the wall, and the view turns so the wall is level on screen. The axis icon in the lower-left corner shows where the UCS axes point.

3. Draw as usual. With the UCS set:
   - `x,y` and `@dx,dy` are measured along the UCS axes; `d<a` measures the angle from the UCS X axis.
   - {{ui:Ortho}}, polar tracking and the grid follow the UCS axes, and so does the crosshair.
   - New text and new paragraphs run along the UCS X axis; {{cmd:DIMLINEAR}}'s horizontal and vertical follow it too.
   - The coordinates in the status bar and from {{cmd:ID}} are UCS coordinates.
4. To type a world point while a UCS is set, put `*` in front: `*0,0` or `@*100,0`.
5. Type `UCS`, {{key:Enter}}, then {{key:Enter}} again to go back to the world.

## Other ways to set a UCS

At the {{cmd:UCS}} prompt:

| You type | What it does |
|---|---|
| a point | A new origin; then click a point on the new X axis, or press {{key:Enter}} to keep the angle |
| `Z` | Turn the UCS about its origin by an angle you type (degrees, counter-clockwise) or pick |
| `3` (3point) | An origin, a point along the positive X axis, and a point on the positive-Y side |
| `OB` (OBject) | From a line, arc, circle, polyline segment, text, block reference, point or construction line |
| `P` (Previous) | The UCS before this one (the last ten are kept while the drawing is open) |
| `W` (World) | Back to the world |
| `NA` (NAmed) | `S` saves the current UCS under a name, `R` restores one, `D` deletes one, `?` lists them |

Every change turns the view so the new UCS X axis is level. {{cmd:PLAN}} turns the view without changing the UCS: {{key:Enter}} for the current UCS, `U` for a saved one, `W` for the world.

## Good to know

- Each pane of a split screen keeps its own turn. The UCS belongs to the drawing.
- Sheets work in paper coordinates; set a UCS on the {{ui:Model}} tab, or inside a viewport.
- Selection windows dragged in a turned view select what lies inside them on screen.
- LISP routines get points in the UCS from `getpoint`, can read `UCSORG`, `UCSXDIR`, `UCSYDIR` and `VIEWTWIST`, and `trans` converts between the world, the UCS and the display.

:::note
A saved file keeps the UCS and the view's turn in a later build; until then they last while the drawing is open.
:::
