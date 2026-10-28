---
title: Construction lines and rays
summary: Draw lines that run on without end (XLINE) or from a point one way (RAY) to line things up, and trim them into ordinary lines.
section: tasks
order: 47
keywords: xline, ray, construction line, construction lines, infinite line, guide line, reference line, bisect, offset line, layout lines, xl
---

## What you will learn

How to draw construction lines to line things up while you draft: through two points, level or upright, at an angle, halfway between two directions, or a set distance from a line. And how to trim them into ordinary lines when they have done their job.

## Two kinds

- A **construction line** ({{cmd:XLINE}}) runs on without end both ways.
- A **ray** ({{cmd:RAY}}) starts at a point and runs on without end one way.

Both draw across the whole drawing area whatever the zoom, in split screen and in viewports on a sheet. {{ui:Extents}} leaves them out, so zooming to the drawing still fits what you drew.

## Draw construction lines

1. Click {{ui:Draw}} › {{ui:More}} › {{ui:Construction line}}, or type `XL` and press {{key:Enter}}.
2. Click a point. Every click after it adds a construction line through the first point and the one you click.
3. Press {{key:Enter}} to finish.

Instead of the first point you can type an option:

| Option | What it does |
|---|---|
| `H` (Hor) | Level lines, one through each point you click. |
| `V` (Ver) | Upright lines through each point. |
| `A` (Ang) | Lines at an angle you type (degrees, counter-clockwise from the X axis), through each point. `R` (Reference) measures the angle from a line you pick. |
| `B` (Bisect) | Click the corner, then a point along one side: each point you click along the other side adds the line halfway between the two. |
| `O` (Offset) | Type a distance, pick a line, then click the side: a construction line that far from it, parallel. `T` (Through) puts it through a point you click instead. The distance is remembered for the next time. |

## Draw rays

1. Click {{ui:Draw}} › {{ui:More}} › {{ui:Ray}}, or type `RAY` and press {{key:Enter}}.
2. Click where the rays start.
3. Click a point for each ray to pass through. Press {{key:Enter}} to finish.

## Snap to them and trim them

Construction lines and rays take the Nearest, Perpendicular and Intersection snaps. They have no ends and no middle, so Endpoint and Midpoint don't find anything on them.

They cut and get cut like other lines in {{cmd:TRIM}} and {{cmd:BREAK}}:

- Trim one end off a construction line and it becomes a ray starting at the cut.
- Trim the far part off a ray and it becomes an ordinary line.
- Break a construction line at one point and you get two rays.

{{cmd:MOVE}}, {{cmd:COPY}}, {{cmd:ROTATE}}, {{cmd:MIRROR}}, {{cmd:OFFSET}} and {{cmd:STRETCH}} work on them too. A window selection never picks them, because no window can hold all of an endless line; a crossing window or a click does.

:::note
A DWG (every version) and a DXF 2000 keep them as construction lines. DXF R12 has none, so there they become lines across the drawing, and the save says so. A plot or PDF draws them only across what it prints.
:::
