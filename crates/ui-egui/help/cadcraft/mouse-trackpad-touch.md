---
title: Mouse, trackpad and touch
summary: How to zoom, pan, right-click and pick in CadCraft with a wheel mouse, a Mac or PC trackpad, or a touch screen, and what differs between browsers.
section: reference
order: 50
keywords: trackpad, touchpad, magic mouse, touch, tablet, ipad, pan, zoom, pinch, right click, two-finger, middle button, browser, chrome, firefox, safari, edge
---

CadCraft is designed for a mouse with a wheel. Everything also works with a trackpad, with a few habits to learn. Touch screens work for picking, with limits.

## Wheel mouse

- **Zoom:** roll the wheel. The drawing zooms about the crosshair, so point at what you want to see.
- **Pan:** hold the wheel down and drag, or drag with the right button.
- **Zoom to everything:** double-click the wheel, or click {{ui:Extents}}.
- **Back to the view before:** click {{ui:Previous}} in the View group, or type `Z`, {{key:Enter}}, `P`, {{key:Enter}}. A run of wheel steps counts as one view, so one {{ui:Previous}} undoes it. Save views you come back to with {{cmd:NAMEDVIEW}} (`NV`).
- **Enter:** right-click.

## Trackpad

:::mac
- **Zoom:** scroll with two fingers, up and down. Pinching also zooms in Chrome, Edge and Firefox.
- **Right-click (Enter):** click with two fingers. Turn on "Secondary click" in **System Settings**, then **Trackpad**, if it does nothing. Control-click may count as an ordinary click in CadCraft rather than a right-click, so use the two-finger click.
- **Pan:** there is no middle button. Zoom out with a two-finger scroll, point at the area you want, and zoom back in: zooming follows the crosshair, so this moves you across the drawing. You can also drag with a two-finger click held down (a right-button drag).
- **Zoom to an area:** click {{ui:Window}} in the View group and click two corners.
:::

:::windows linux
- **Zoom:** scroll with two fingers, up and down.
- **Right-click (Enter):** click with two fingers, or click the lower-right corner of the touchpad, depending on your touchpad settings.
- **Pan:** there is usually no middle button. Zoom out with a two-finger scroll, point at the area you want, and zoom back in: zooming follows the crosshair, so this moves you across the drawing. You can also drag with the right button held down.
- **Zoom to an area:** click {{ui:Window}} in the View group and click two corners.
:::

Holding {{key:Space}} and dragging does **not** pan in CadCraft: {{key:Space}} is Enter.

Sideways two-finger scrolling does nothing; only up and down zooms.

## Touch screens and tablets

CadCraft reads a finger as a mouse click:

- A tap picks a point or an object, or selects.
- Dragging a finger on empty space draws a lasso (moving right first a window, left first a crossing; see [Lasso selection](topic:lasso-selection)).
- In {{cmd:TRIM}} and {{cmd:EXTEND}}, dragging a finger through pieces trims or extends them all.
- Dragging a selected object's grip moves the grip (grips and the pick box are bigger for a finger).
- **Pinch** with two fingers to zoom about the point between them; move two fingers together to **pan**. A second finger cancels whatever the first one began, so a pinch never leaves a mark.
- A **long press** (hold still for about half a second) is a right click: {{key:Enter}}. It ends a command, finishes a selection, or repeats the last command.

Under the command line a **key row** appears: {{ui:Esc}}, Undo, Redo, the characters a tablet keyboard hides (`@` `,` `<` `'` `"` `-`), {{ui:Snap}}, {{ui:Ortho}}, a keyboard button, and a big {{ui:Enter}}. Tap the keyboard button (or the command line) to type a command or a number.

There is no hover, so snap markers only show where your finger is.

:::touch
You are using a touch screen. For anything more than a quick look, an attached keyboard and a mouse or trackpad make CadCraft much easier to use.
:::

## Browsers

CadCraft works the same in Chrome, Edge, Firefox and Safari, on Mac, Windows and Linux. The differences are small:

| Topic | What to know |
|---|---|
| Opening a DWG | Large drawings take a few seconds to read in every browser; the status line shows "Reading …" meanwhile |
| Downloads | Each browser decides where downloads go and whether to ask first; see [Browsers and computers](common:browsers) |
| Open to print | Opens the PDF in a new tab. If the browser blocks pop-ups, CadCraft downloads the PDF instead |
| F7 | Firefox may offer caret browsing; choose No |
| Leaving with unsaved changes | Every browser shows its own "Leave site?" question, which CadCraft cannot change |

:::safari
In Safari, a PDF opened with {{ui:Open to print}} may show in the tab without printing: use **File**, then **Print**.
:::

:::firefox
Firefox may ask what to do with each download. Choose to save the file, or change this once in Firefox's settings under Downloads.
:::
