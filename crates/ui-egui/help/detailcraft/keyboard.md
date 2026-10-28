---
title: Keys and mouse actions
summary: Every keyboard shortcut and mouse, trackpad and touch action in DetailCraft, from the code.
section: reference
order: 10
keywords: keyboard, shortcuts, hotkeys, keys, mouse, trackpad, touch, ctrl, command, cheat sheet
---

How keys are written in this help: [Keys, mice and trackpads](common:keys). On a Mac, {{key:Mod}} is the Command key; elsewhere it is Ctrl.

## Tools

These work when the cursor is not in a text or number field.

| Key | Does |
|---|---|
| {{key:V}} | {{ui:Select}} |
| {{key:B}} | {{ui:Beam}} |
| {{key:C}} | {{ui:Column}} |
| {{key:R}} | {{ui:Brace}} |
| {{key:P}} | {{ui:Plate}} |
| {{key:F}} | {{ui:Footing}} |
| {{key:S}} | {{ui:Stair}} |
| {{key:M}} | Move the selection by picking two grid intersections ([Select by window, move, rotate, mirror, change many](topic:select-and-change)) |
| {{key:Esc}} | Drop a selection window being dragged, or put dragged parts back; cancel a move, the first point of a beam, brace or plate, or a paste; press again to go back to {{ui:Select}}; in {{ui:Select}}, clear the selection. With the {{ui:More}} menu open, close it |
| {{key:Delete}} or {{key:Backspace}} | Delete the selection (everything selected, in one undo step) |

## File and edit

| Key | Does |
|---|---|
| {{key:Mod+S}} | Save the model (download). Works even while typing in a field |
| {{key:Mod+O}} | Open a model. Works even while typing in a field |
| {{key:Mod+Z}} | Undo (outside text fields; inside one it undoes your typing) |
| {{key:Mod+Y}} or {{key:Mod+Shift+Z}} | Redo |

## Copy and paste

These work when the cursor is not in a text or number field (there they copy, paste and select your text as usual). Each paste, cut, duplicate or delete is one undo step. More: [Copy, paste and duplicate](topic:copy-paste).

| Key | Does |
|---|---|
| {{key:Mod+C}} | {{ui:Copy}} the selected members and footings (with their holes, copes and connections) |
| {{key:Mod+X}} | {{ui:Cut}}: copy the selection, then delete it |
| {{key:Mod+V}} | {{ui:Paste}}: the copy follows the pointer; click a grid intersection for it |
| {{key:Mod+Shift+V}} | {{ui:Paste in place}}: at the coordinates it was copied from |
| {{key:Mod+D}} | {{ui:Duplicate}}: a copy at once, moved by the offset in [DetailCraft settings](topic:settings) |
| {{key:Mod+A}} | {{ui:Select all}}: every member, connection and footing |
| {{key:Esc}} | {{ui:Select none}} |

If you have selected text on the page (in a report table, say) and nothing in the model, {{key:Mod+C}} copies the text, as usual.

{{ui:New}}, {{ui:Sample}} and the exports have no keys; use the toolbar.

None of these keys do anything while a dialog (the shop drawing preview or a question) is open.

## Fields and panels

| Key | Does |
|---|---|
| {{key:Enter}} or {{key:Tab}} | Apply a typed value (each change is one undo step) |
| {{key:Enter}} in a {{ui:Grids}}, {{ui:Levels}}, {{ui:Custom profile}}, hole or cope field | The same as choosing that form's button |
| {{key:Left}} {{key:Right}} | Move between the {{ui:Problems}}, {{ui:Bill of materials}} and {{ui:Bending schedule}} tabs when a tab has focus |
| {{key:Esc}} | Close the shop drawing preview or a question dialog (the same as {{ui:Cancel}}) |

## Screen and help

| Key | Does |
|---|---|
| {{key:Ctrl+Shift+F}} | Cycle normal, full screen, full screen with the bars hidden. Control on every computer, a Mac included |
| {{key:Mod+,}} | Open {{ui:Settings}} ([DetailCraft settings](topic:settings)) |
| {{key:Esc}} | Leave full screen |
| {{key:F1}} | Open this help |

## Mouse

| Action | In 3D | In Plan and Elevation |
|---|---|---|
| Click | Select a part, or place at the nearest intersection when a placing tool is on (or paste there while pasting) | The same |
| {{key:Shift}}, {{key:Ctrl}} or {{key:Mod}} and click | Add the part to the selection, or take it out | The same |
| Press on a selected part and drag | Move the selection across the level; with {{key:Mod}} held, up and down | Move the selection (in an elevation: along it and up and down) |
| Drag with the left button | Turn the model | With {{ui:Select}}: a selection window (to the right: wholly inside; to the left: touching). While placing: pan |
| {{key:Shift}} and drag with the left button | With {{ui:Select}}: a selection window, added to the selection. While placing: pan | With {{ui:Select}}: a selection window, added to the selection |
| {{key:Ctrl}} or {{key:Mod}} and drag with the left button | Pan | Nothing |
| Drag with the right button | Pan | Pan |
| Wheel | Zoom toward the middle of the view | Zoom |
| Drag with the middle button | Zoom | With {{ui:Select}}: pan. Otherwise zoom |
| Drop a `.detailcraft.json` file on the app | Open it | Open it |

A click only counts if the pointer moves less than about 5 pixels between press and release; a longer movement is a drag.

## Trackpad

| Action | Does |
|---|---|
| Two-finger swipe up or down, or pinch | Zoom |
| Press and drag | Turn (3D) or pan (Plan and Elevation) |
| {{key:Shift}} and press and drag | Pan in 3D |
| Two-finger click | Right-click (there is no menu in the view) |

## Touch

On a tablet or a touch screen:

| Action | Does |
|---|---|
| Tap | Select a part, or place at the nearest grid intersection when a placing tool is on (a tap within about a finger's width of it counts) |
| Long press on a part | Add it to the selection, or take it out (what {{key:Shift}}+click does with a mouse) |
| One finger on a selected part, then drag | Move the selection, snapping onto grid intersections |
| One finger elsewhere | Turn the model (3D) or pan (Plan and Elevation) |
| Two fingers | Pinch to zoom, move together to pan |

Buttons, lists and fields are larger when the screen is touch only. Selection windows need a mouse; with fingers, tap and long-press.

## Keys the browser keeps

Some shortcuts belong to the browser and no page can use them, for example {{key:Mod+W}} (close tab), {{key:Mod+T}} (new tab) and {{key:Mod+N}} (new window). If {{key:Mod+Y}} opens your browser's history instead of redoing, use {{key:Mod+Shift+Z}}.

Shortcuts reach DetailCraft only when the page has the keyboard. If a key does nothing, or the browser acts on it instead, click an empty part of the DetailCraft window first (for example after typing in the address bar).
