---
title: Follow me: sweep a face along a path
summary: Make pipes, cables, handrails, moldings and rings by sweeping a face along edges, arcs or a curve.
section: tasks
order: 35
keywords: follow me, sweep, path, pipe, cable, tube, handrail, molding, moulding, gutter, ring, torus, lathe, extrude along
---

{{ui:Follow me}} moves a face along a path and builds the solid it passes through. The face keeps its shape and stays square to the path, and at each corner of the path the pieces meet on a clean mitre.

## Make a cable

1. Draw the path: a {{ui:Curve}} through the points the cable passes (it can rise and fall), or {{ui:Line}}s and {{ui:Arc}}s joined end to end.
2. At the start of the path, draw the cable's section: a small {{ui:Circle}} across the path. Start the circle on the path's end point and turn it to face along the path; or draw it on a face that crosses the path there.
3. Select the path (one click selects a whole curve or arc; {{key:Ctrl}}+click adds more edges).
4. Click {{ui:Follow me}} in the {{ui:Modify}} group, then click the circle.

The tube is a closed solid, with smooth (soft) sides.

## Without selecting the path first

Click {{ui:Follow me}}, click the face, then click any edge of the path. The path is the chain of edges through that edge, up to where three or more edges meet.

## What to know

- **Closed paths** (a circle, a closed curve, a rectangle's outline) make a ring: the face is used up and the ring closes on itself. Draw a circle on the ground for the path and a small circle standing across it for a donut.
- **Off the path:** the face does not have to touch the path. A profile drawn beside a path sweeps along beside it, like a molding along the top of a wall.
- **The path must not branch**, and must be one connected chain. ModelCraft says so when it is not.
- **A face lying along the path** (in the same plane as the path's first edge) cannot be swept: turn it to cross the path.
- It is one undo step.
