---
title: Trackpad, touch and small screens
summary: How to turn, pan and zoom with a Mac or laptop trackpad, what works on a touch screen and what still needs a keyboard, and how the screen rearranges on a phone or narrow window.
section: reference
order: 50
keywords: trackpad, touchpad, magic mouse, mac, touch, tablet, ipad, phone, pinch, two finger, right click, secondary click, gestures, small screen, mobile
---

PartCraft is made for a mouse with a wheel and a middle button, but a trackpad works well once you know the gestures, and a touch screen works for looking at parts and simple sketching.

## Trackpad

:::mac
| To | Do this |
|---|---|
| Turn the part (3D) | Click and drag with one finger (or drag with three fingers, if you turned on three-finger drag) |
| Pan (3D) | Hold {{key:Shift}} (or ⌘) and click-drag, or click-drag with two fingers pressed (a secondary-click drag) |
| Turn the part, swiping | Swipe with two fingers; with {{key:Shift}}, the swipe pans. Needs **Scrolling** set to "Work it out" (the start) or "Trackpad" |
| Zoom (3D and sketch) | Pinch. Zoom goes toward the pointer. With **Scrolling** set to "Mouse wheel", a two-finger scroll zooms too |
| Right-click | Click with two fingers, or {{key:Ctrl}}+click |
| Pan in a sketch | Swipe with two fingers ("Work it out" or "Trackpad"), or click-drag with two fingers pressed. {{key:Shift}}+drag does not pan in a sketch (it draws a selection window) |
| Add to a selection in a sketch | {{key:Shift}}+click or ⌘+click |

Turn on secondary click in **System Settings, Trackpad, Point & Click** if two-finger click does nothing.
:::

:::windows linux
| To | Do this |
|---|---|
| Turn the part (3D) | Press the touchpad (or tap and hold) and drag |
| Pan (3D) | Hold {{key:Shift}} or {{key:Ctrl}} and drag, or drag with the right button area pressed |
| Turn the part, swiping | Swipe with two fingers; with {{key:Shift}}, the swipe pans. Needs **Scrolling** set to "Work it out" (the start) or "Trackpad" |
| Zoom (3D and sketch) | Pinch where the touchpad supports it. Zoom goes toward the pointer. With **Scrolling** set to "Mouse wheel", a two-finger scroll zooms |
| Right-click | Two-finger tap, or the lower-right corner of the touchpad |
| Pan in a sketch | Swipe with two fingers ("Work it out" or "Trackpad"), or drag with the right button area pressed. {{key:Shift}}+drag does not pan in a sketch |
| Add to a selection in a sketch | {{key:Shift}}+click or {{key:Ctrl}}+click |
:::

**Scrolling** is in [PartCraft settings](topic:settings): "Work it out" (the start) treats sideways or fine-grained scrolling as a trackpad, "Trackpad" always does, "Mouse wheel" never does. A pinch zooms in all three.

The arrow keys turn the 3D view and {{key:Shift}} with the arrows pans, once you have clicked in the view: handy on a trackpad. {{key:Plus}} and {{key:Minus}} zoom.

## Touch screens

PartCraft reads touch input, so tablets, touch laptops and phones work, with limits.

:::touch
**Touch keys**

Once you touch the screen, a small bar appears at the lower right of the view with the keys a finger lacks. It goes away when you use a mouse.

- {{ui:Esc}}: the Esc key. It cancels what you started or steps back, exactly as the key does.
- {{ui:Add}} (in a sketch, with Select): while it is on, each tap adds an item to the selection or takes it out, like {{key:Shift}}+click. Tap it again to go back to one item at a time.
- {{ui:Done}} (while you draw a line chain, a spline or a polygon): finishes it, like a right-click.

Holding a finger still for about half a second does the same for one tap: with a drawing tool it finishes what you draw, and with Select it adds the item under your finger to the selection or takes it out. Points and lines are easier to hit with a finger: PartCraft looks about twice as far round a tap as round a click.

**In the 3D view**

- One finger: turn the part.
- Two fingers: pinch to zoom, move together to pan.
- Tap: pick a face, an edge or a plane. There is no hover highlight before you tap.

**In a sketch**

- Tap with a drawing tool to place each point: tap the start, then tap the end. Snapping happens where you lift your finger.
- Two fingers pinch and pan. Putting a second finger down cancels a drag the first finger began.
- To end a line chain, tap {{ui:Done}}, hold your finger still, or tap {{ui:Line}} again (or another tool) in the toolbar.
- With Select, tap to select; drag a point or line to move it; drag on empty space for a window. A second tap replaces the selection unless {{ui:Add}} is on (or you hold the finger still on the second item).
- To change a dimension, use **Edit** beside it in the right panel.

**The drawing sheet**

Tap to place and select symbols, and drag a symbol to move it. Pinch to zoom, move two fingers to pan, or drag one finger across empty paper. A second finger stops a symbol you were dragging, and it goes back.

**What still needs a keyboard**

- Typing numbers: the screen keyboard comes up in number boxes and dimension fields.
- Keyboard shortcuts for tools: use the toolbar.
:::

## Small screens and phones

- At 1280 pixels or wider, the toolbar fits on one row.
- Between about 760 and 1100 pixels, the side panels get narrower.
- Below about 760 pixels (a phone, or a narrow window), the screen stacks: toolbar (swipe it sideways for more buttons), the 3D view, the status bar, then the properties panel and the history. Scroll the page to reach the panels.
- Menus ({{ui:Export}}, {{ui:Views}}) open across the width of the screen.

:::note
On iPhone there is no full screen mode in the browser; {{key:Ctrl+Shift+F}} and the screen-mode button switch only between normal and the bars hidden.
:::

A phone is fine for opening a part, turning it and checking its facts. Real modeling needs a bigger screen and a mouse or trackpad.

## Browsers

Current Chrome, Edge, Firefox and Safari run PartCraft on Windows, macOS and Linux, and on tablets and phones, as long as WebGL and WebAssembly are available. See [Browsers and computers](common:browsers).
