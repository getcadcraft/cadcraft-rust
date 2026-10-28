---
title: Move around the 3D view
summary: Turn, pan and zoom with a mouse, a trackpad or a touch screen, and when to use Plan and Elevation instead.
section: tasks
order: 10
keywords: orbit, rotate, pan, zoom, navigate, view, camera, trackpad, touch, plan, elevation, fit, mouse
---

The view has three modes, chosen in the view bar above it: {{ui:3D}}, {{ui:Plan}} and {{ui:Elevation}}. The mouse buttons do different things in 3D than in Plan and Elevation, so read both tables.

## In 3D

:::mouse
| To | Do this |
|---|---|
| Turn around the model | Drag with the left button |
| Pan | Drag with the right button, or hold {{key:Shift}} and drag with the left button |
| Zoom | Turn the wheel, or drag up and down with the middle button (wheel pressed) |
| Select a part | Click it (a click that moves less than a few pixels) |
| See the whole model | The fit button at the right end of the view bar |
:::

:::touch
| To | Do this |
|---|---|
| Turn around the model | Drag with one finger |
| Pan | Drag with two fingers |
| Zoom | Pinch with two fingers |
| Select a part | Tap it |
| See the whole model | The fit button at the right end of the view bar |
:::

The view cube at the top right (under the view bar) turns with the model: click a face, edge or corner to look from there, drag it to turn, or click its house button to go back to the start. See [The view cube](topic:view-cube).

The model turns around the middle of the view, and the wheel zooms toward the middle of the view, not toward the pointer. To look closely at one corner, pan it to the middle first, then zoom.

## In Plan and Elevation

These views look straight at the model and cannot be turned.

:::mouse
| To | Do this |
|---|---|
| Pan | Drag with the left or the right button |
| Zoom | Turn the wheel, or drag up and down with the middle button |
| Select a part | Click it |
:::

:::touch
| To | Do this |
|---|---|
| Pan | Drag with one finger |
| Zoom | Pinch with two fingers (dragging two fingers also pans) |
| Select a part | Tap it |
:::

## On a Mac trackpad

:::mac
- **Zoom:** swipe up or down with two fingers, or pinch. Both reach the view as zooming.
- **Turn (3D):** press the trackpad and drag with one finger. If you use tap to click, double-tap and drag works too when dragging is turned on in **System Settings, Accessibility, Pointer Control, Trackpad Options**.
- **Pan (3D):** hold {{key:Shift}} and drag. Holding {{key:Ctrl}} or {{key:Mod}} while dragging also pans. A two-finger click is a right-click, but dragging with it is awkward on a trackpad.
- **Pan (Plan and Elevation):** just drag.
- A Magic Mouse has no middle button: use the {{key:Shift}} drag to pan and a swipe on its surface to zoom.
:::

More about right-clicking and panning without a mouse: [Keys, mice and trackpads](common:keys).

## Plan and Elevation make placing easy

In {{ui:3D}}, intersections at different levels can sit on top of each other on screen, and a click goes to the nearest dot. In {{ui:Plan}} and {{ui:Elevation}} only the dots of the level or grid line you are looking at can be picked.

1. Choose {{ui:Plan}}, then pick a level in the list beside the buttons. The view looks down at that level; anything more than 1.5 m above it is cut away.
2. Or choose {{ui:Elevation}}, then pick a lettered grid line ("Grid A", "Grid B" …). The view looks straight at that line; only steel within 1 m of it is shown.

Elevations are only on lettered grid lines. To work on a line of numbered grids (a frame running from A to B), use {{ui:3D}}, or place by typing in the {{ui:Add by grid}} form. See [Place columns, beams and braces](topic:place-members#place-by-grid-without-clicking).

## Selecting

- Click a part to select it. It turns the highlight color and the {{ui:Properties}} panel shows it.
- Click empty space, or press {{key:Esc}}, to clear the selection.
- Steel wins over concrete: a click on a column inside a see-through footing selects the column. Click the footing away from the column, or one of its bars, to select the footing.
- Clicking a bolt or an end plate selects its connection.
- In Plan and Elevation, parts that are cut away cannot be clicked.
- Hold {{key:Shift}} (or {{key:Ctrl}}, or {{key:Mod}}) and click to add a part to the selection or take it out; {{key:Mod+A}} selects everything. There is no window (drag a box) selection yet. What you can do with several: [Copy, paste and duplicate](topic:copy-paste).

You can also select from lists: click a mark in {{ui:Part marks}} on the left, a part mark in the {{ui:Bill of materials}} tab, or {{ui:Select}} beside a line in {{ui:Problems}}.

## If the view looks wrong

- Lost the model? Choose the fit button.
- The view is blank or gray with a message about WebGL: see [Troubleshooting](topic:troubleshooting#the-3d-view-is-missing).
- Everything still works without the 3D view except clicking in it: the panels, reports and exports all run, and {{ui:Add by grid}} places members by typing.
