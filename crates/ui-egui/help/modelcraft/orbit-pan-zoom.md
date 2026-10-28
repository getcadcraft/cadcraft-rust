---
title: Orbit, pan and zoom
summary: Move around the model with a mouse, a trackpad, a touch screen or the keyboard, and jump to standard views.
section: tasks
order: 10
keywords: orbit, pan, zoom, long press, three fingers, two fingers, tap, navigate, camera, view, rotate view, trackpad, touch, pinch, middle button, zoom extents, perspective, parallel, top view, iso
---

You can move the camera at any time, even in the middle of drawing a line or pushing a face: the tool waits where it was.

## With a three-button mouse

| To | Do this |
|---|---|
| Orbit | Drag with the middle button (press the wheel) |
| Pan | Drag with the right button, or hold {{key:Shift}} and drag with the middle button |
| Zoom | Turn the wheel. It zooms toward the face under the pointer, or toward the point under it at the depth of the view's center |
| See the whole model | {{key:Shift+Z}} or {{ui:Extents}} |

Orbit turns the view around the point at the center of the view, not around the pointer. To orbit around a particular part, zoom toward it with the wheel first: zooming moves the center with it.

A right-click without dragging does nothing; ModelCraft has no right-click menu.

## Without a middle button

Use the camera tools in the toolbar, or the keyboard:

1. Press {{key:O}} (or click {{ui:Orbit}}) and drag with the left button to orbit.
2. Press {{key:H}} (or click {{ui:Pan}}) and drag to pan.
3. Press {{key:Z}} (or click {{ui:Zoom}}) and drag up to zoom in, down to zoom out.
4. Press {{key:Space}} to go back to {{ui:Select}}, or press the key of the tool you want next.

The camera tools replace the tool you were using. To look around in the middle of an operation without losing it, use the keyboard instead:

- The arrow keys orbit.
- {{key:Shift}} with the arrow keys pans.

While a drawing tool is in the middle of an operation (after its first click), the arrow keys lock directions instead of orbiting. See [Inference and snapping](topic:inference#lock-a-direction).

## On a trackpad

The easiest way: open {{ui:Settings}} ({{key:Mod+,}}), {{ui:ModelCraft}} tab, and set Scrolling to **Trackpad** (or leave it on "Work it out"). Then swipe with two fingers to orbit, hold {{key:Shift}} and swipe to pan, and pinch to zoom — no camera buttons. See [ModelCraft settings](topic:settings#scrolling-mouse-wheel-or-trackpad).

With Scrolling set to **Mouse wheel**:

:::mac
- **Zoom:** scroll with two fingers, or pinch. Both zoom toward the pointer.
- **Pan:** press with two fingers (a secondary click) and keep the fingers down while you drag. Turn on "Secondary click" in **System Settings**, then **Trackpad**, if two-finger click does nothing.
- **Orbit:** press {{key:O}} and drag with one finger, or use the arrow keys.
- Holding {{key:Ctrl}} while you click may not count as a right-click in ModelCraft, depending on the browser: it can act as a click with {{key:Ctrl}} held instead. Use the two-finger press to pan.
:::

:::windows linux
- **Zoom:** scroll with two fingers. Pinch zooms too where the touchpad driver sends it as a scroll with {{key:Ctrl}}.
- **Pan:** drag with the right button (on many touchpads, press the lower-right corner and drag), or press {{key:H}} and drag.
- **Orbit:** press {{key:O}} and drag, or use the arrow keys.
:::

With Mouse wheel, two-finger scrolling zooms; it does not pan. That is the opposite of many web pages, and it is how the mouse wheel behaves in most 3D modelers.

:::tip
{{key:Space}} picks the {{ui:Select}} tool in ModelCraft. Holding {{key:Space}} and dragging does not pan, as it does in some other apps.
:::

## On a touch screen

:::touch
- **One finger** uses the current tool: **tap** to click, **press and drag** to draw or drag.
- While a drawing tool's finger is down, a **loupe** above it shows what is under the finger, so you can see the endpoint, midpoint or edge it snaps to before you lift.
- **Hold still** for half a second to finish what the tool is doing, as {{key:Enter}} does: a curve, a line chain. With nothing going on, holding still and then dragging the same finger **orbits** until you lift it.
- **Two fingers** dragged together **pan**; **pinch** to zoom. Both work in one gesture.
- **Three fingers** dragged together **orbit**.
- To swap them (two fingers orbit, three pan), open {{ui:Settings}}, {{ui:ModelCraft}} tab, Touch screen.
- Putting a second finger down **cancels** what the first finger started (a rectangle half drawn, a push/pull in progress). Lift your fingers and start again.
- A small bar at the bottom of the view has **Done** ({{key:Enter}}), **Cancel** ({{key:Esc}}) and **Undo**, for when there is no keyboard.
- Snapping and picking reach a little further than with a mouse, and the buttons are bigger, so a finger can hit them.
:::

:::mouse
On a touch screen, one finger uses the current tool (tap to click, hold still to finish, or hold still and drag to orbit), two fingers pan and pinch to zoom, three fingers orbit, and a small bar offers Done, Cancel and Undo.
:::

## Standard views

1. Click {{ui:Views}} at the right end of the toolbar.
2. Choose {{ui:Iso}}, {{ui:Top}}, {{ui:Front}}, {{ui:Right}}, {{ui:Back}}, {{ui:Left}} or {{ui:Bottom}}.

The view turns to that direction and fits the whole model. {{ui:Front}} looks north, along the green axis; {{ui:Top}} looks straight down with north up.

The view cube at the top right of the view does the same in one click, plus edges and corners, a compass ring to turn the model and a home button. See [The view cube](topic:view-cube).

## Perspective and parallel projection

Choose {{ui:Views}}, then {{ui:Perspective}}. A check mark means perspective is on. With it off, the view is a parallel (orthographic) projection: lines that are parallel in the model stay parallel on screen, which is what you want for elevations from {{ui:Front}} or {{ui:Right}}. The status bar says "Perspective." or "Parallel projection."

Each scene remembers whether it was in perspective. See [Save views as scenes](topic:scenes).
