---
title: Move and turn the drawing axes
summary: Turn the red, green and blue axes to match a building that is not square to them, or align them to a face, so drawing and typing follow it.
section: tasks
order: 54
keywords: axes, drawing axes, origin, align axes, rotate axes, turn axes, reset axes, angled building, skewed, ucs, coordinate system
---

The red, green and blue axes guide everything you draw: rectangles line up with them, lines lock to them, and the ground is the red–green plane. When a building sits at an angle (a site plan that is not square to north, a wing turned 30°), move the axes onto it, so drawing follows its walls. The model itself does not move.

All three commands are in {{ui:Edit}} › {{ui:More}}.

## Place axes

1. {{ui:Edit}} › {{ui:More}} › {{ui:Place axes}}.
2. Click where the origin goes, such as a corner of the building.
3. Click a point along the new red axis, such as further along a wall.
4. Click a point toward the new green axis. Green is made square to red, and blue stands square to both.

The axes are drawn in their new place. A rectangle you now draw on the ground, or type as `4m,3m`, runs along the turned red and green; the arrow keys lock to them; and the origin snap is at the new origin.

## Align axes to a face

Select one face, then {{ui:Edit}} › {{ui:More}} › {{ui:Align axes to face}}. Blue then points out of the face, red runs along its longest edge, and the origin is at that edge's start. Use it to draw on a sloping roof as if it were the ground.

## Reset axes

{{ui:Edit}} › {{ui:More}} › {{ui:Reset axes}} puts them back where the model began.

## Things to know

- Inside an open group or component, its own axes are used (they turn with it); the model's axes apply at the top level.
- The axes are saved in the ModelCraft file, and each change is one undo step.
- Exports and the standard views ({{ui:Top}}, {{ui:Front}}…) still use the model's own directions.
