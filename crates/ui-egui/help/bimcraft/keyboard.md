---
title: Keys and mouse
summary: Every key, mouse, trackpad and touch action in BimCraft, grouped by what you are looking at.
section: reference
order: 10
keywords: keyboard, shortcuts, hotkeys, keys, mouse, trackpad, touch, orbit, pan, zoom, hide, isolate, see-through, undo, redo, copy, paste, duplicate, save, fit, print view, escape, settings, draw, wall, door, window, plan
---

BimCraft has few shortcuts. Most work is done with the mouse and the panels. Shortcuts written with {{key:Mod}} use Command on a Mac and Ctrl elsewhere. See [Keys, mice and trackpads](common:keys) for how keys are written in this help.

## Everywhere

| Key | What it does |
|---|---|
| {{key:Ctrl+Shift+F}} | Cycle the screen modes: normal, full screen, full screen with the bars hidden. Ctrl on every platform, including a Mac |
| {{key:Esc}} | Leave full screen |
| {{key:Mod+,}} | Open {{ui:Settings}}. See [BimCraft settings](topic:settings) |
| {{key:Tab}} | Move between buttons, checkboxes and values. Every control can be reached from the keyboard |

## 3D view (IFC models)

| Key or action | What it does |
|---|---|
| {{key:F}} | Fit the whole model, three-quarter view (same as {{ui:Fit}}) |
| {{key:Esc}} | Clear the selection, or stop placing or pasting |
| Click | Pick the element under the pointer; click empty background to clear |
| {{key:Mod}}+click or {{key:Shift}}+click | Add an element to the pick, or take it out |
| {{key:H}} | Hide the picked elements. See [Hide, isolate and see through elements](topic:hide-and-isolate) |
| {{key:I}} | Show only the picked elements (isolate) |
| {{key:T}} | Show the picked elements, everything else see-through |
| {{key:Shift+H}} | Show every element again |
| {{ui:Walk}} | Then {{key:W}} {{key:A}} {{key:S}} {{key:D}} or arrows walk, {{key:Shift}} runs, drag looks round, {{key:Esc}} stops. See [Walk through a model](topic:walk) |
| {{key:M}} | Measure distances and face areas. See [Measure distances, face areas and locations in 3D](topic:measure) |
| Drag with the left button | Orbit |
| Drag with the right button | Pan |
| {{key:Shift}} or {{key:Mod}} and drag with the left button | Pan (handy on a trackpad) |
| Scroll the wheel, or drag with the middle button | Zoom |
| Two-finger scroll or pinch on a trackpad | Zoom |
| One finger (touch) | Orbit; a tap picks |
| Two fingers (touch) | Pinch to zoom, move together to pan |

| {{key:X}} | Section plane on and off. See [Cut an IFC model with a section plane](topic:section-plane) |
| {{key:Shift+X}} | Section box round the picked elements, with the room set in Settings |

There are no keys for {{ui:Top}}, {{ui:Front}} and {{ui:Side}}; use the buttons.

## Edit

The same commands are in the toolbar: {{ui:Undo}}, {{ui:Redo}}, {{ui:Copy}}, {{ui:Paste}} and the {{ui:More}} menu, which shows each key. They work in the 3D view of an IFC model, not while you type in a field. See [Copy, paste and duplicate library items](topic:copy-paste).

| Key | What it does |
|---|---|
| {{key:Mod+Z}} | Undo the last change |
| {{key:Mod+Y}} or {{key:Mod+Shift+Z}} | Redo what Undo took back |
| {{key:Mod+C}} | Copy the picked elements (their names and properties also go to the clipboard as text) |
| {{key:Mod+X}} | Cut the picked elements: copy them, then delete them |
| {{key:Mod+V}} | Paste what you copied: it follows the pointer, click where it goes |
| {{key:Mod+Shift+V}} | Paste in place: where a cut came from, or beside the originals after a copy |
| {{key:Mod+D}} | Duplicate: copy the picked elements and start pasting them |
| {{key:Delete}} | Delete the picked elements, with their parts and openings. See [Delete, cut, copy and paste building elements](topic:edit-elements) |
| {{key:Mod+A}} | Pick every element shown |
| {{key:Esc}} | Select none |

When text is selected on the page, {{key:Mod+C}} copies that text as usual.

## Plan view (Draw)

These work while {{ui:Draw}} is on and you are not typing in a field. See [Draw walls](topic:draw-walls) and [Doors and windows](topic:doors-and-windows).

| Key or action | What it does |
|---|---|
| {{key:V}} | {{ui:Select}} |
| {{key:W}} | {{ui:Wall}} |
| {{key:D}} | {{ui:Door}} |
| {{key:N}} | {{ui:Window}} |
| {{key:H}} | With {{ui:Door}} or {{ui:Window}}: hinges at the other end |
| {{key:Shift}} while drawing a wall | Keep it at 0°, 90°, 180° or 270° |
| Digits and the foot and inch marks (`'` and `"`) | Start typing a length into the {{ui:Length}} box; {{key:Enter}} uses it |
| {{key:Enter}}, {{key:Esc}}, right-click or double-click | End the wall you are drawing; {{key:Esc}} again goes back to {{ui:Select}} |
| {{key:Delete}} | Delete the picked walls, doors and windows; the walls left join up again |
| {{key:F}} with the pointer over the plan | Fit the plan |
| Scroll the wheel | Zoom about the pointer |
| Drag with the middle or right button, or {{key:Space}} and drag | Pan; with {{ui:Select}} on, a plain drag pans too |
| Drag a door or window with {{ui:Select}} | Move it along its wall, or onto another wall |
| Two fingers (touch) | Pinch to zoom, move together to pan |

## Placing and pasting library items

| Key | What it does |
|---|---|
| {{key:R}} | Turn the item 90° |
| {{key:Tab}} | Wall snap on and off: the item's back goes against the nearest wall |
| {{key:Enter}} in **X, Y (mm)** | Place at the typed position |
| {{key:Esc}} | Stop placing or pasting |

## Editing IFC values

| Key | What it does |
|---|---|
| {{key:Enter}} | Keep the value you typed |
| {{key:Esc}} | Cancel and put the old value back |
| Click elsewhere | Keep the value you typed |
| {{key:Mod+Z}} | Undo the last change. Only while you have unsaved changes and are not typing in a field |
| {{key:Mod+Y}} | Redo what Undo took back |
| {{key:Mod+S}} | Save as IFC. Only while you have unsaved changes; otherwise the browser's own save command opens |

To edit with the keyboard alone: press {{key:Tab}} until a value is highlighted, then press {{key:Enter}} to start typing.

## 2D drawings (DWG and DXF)

| Key or action | What it does |
|---|---|
| {{key:F}} | Fit the drawing or the sheet |
| {{key:P}} | Print view on and off |
| Double-click | Fit |
| Drag with any button | Pan |
| Scroll the wheel | Zoom at the pointer |
| Two-finger scroll or pinch on a trackpad | Zoom at the pointer. Two-finger scroll does not pan |
| One finger (touch) | Pan. Pinch zoom is not built yet for drawings |

## Drawings in 3D

| Key or action | What it does |
|---|---|
| {{key:3}} | 3D on and off (also on the 2D view) |
| {{key:F}} | Extents: the whole drawing |
| Drag with the middle or left button | Orbit |
| Drag with the right button, or {{key:Shift}} and drag | Pan |
| Scroll the wheel | Zoom toward what is under the pointer |
| Two-finger swipe on a trackpad | Orbit; with {{key:Shift}}, pan |
| Pinch on a trackpad | Zoom |
| One finger (touch) | Orbit |
| Two fingers (touch) | Pinch to zoom, move together to pan |

{{key:F}}, {{key:P}} and {{key:3}} do nothing while you type in a field, while the equipment schedule is showing, or when you hold {{key:Mod}} or {{key:Alt}}.

## Files

| Action | What it does |
|---|---|
| Drag a file onto the page | Open it |
| Drag a folder onto the page | Open it as a project folder, with no permission prompt |
