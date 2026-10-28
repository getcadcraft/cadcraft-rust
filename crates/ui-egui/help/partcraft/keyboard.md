---
title: Keys and mouse
summary: Every keyboard shortcut and mouse action in PartCraft, grouped by where it works: everywhere, copy and paste, on the part, in the 3D view, in a sketch, in the history and in an assembly.
section: reference
order: 10
keywords: keyboard, shortcuts, hotkeys, keys, mouse, orbit, rotate, pan, zoom, wheel, right click, undo, redo, save, open, escape, delete, alt arrow, copy, paste, cut, duplicate, select all, middle button
---

Keys do nothing while you are typing in a box in a panel, except {{key:Mod+S}} and {{key:Mod+O}}, which always work. Letter keys work without a modifier.

General notes about keys in the browser, and on keyboards without function keys: [Keys, mice and trackpads](common:keys).

## Everywhere

| Key | Action |
|---|---|
| {{key:Mod+S}} | Save the part (in an assembly: the assembly). Closes an open sketch first |
| {{key:Mod+O}} | Open a part or assembly file (asks first if there are unsaved changes) |
| {{key:Mod+Z}} | Undo |
| {{key:Mod+Y}} or {{key:Mod+Shift+Z}} | Redo |
| {{key:F1}} | Open or close this help (also {{ui:Help}} in the app bar). {{key:Esc}} closes it too. On many laptops and Mac keyboards, hold Fn |
| {{key:Ctrl+Shift+F}} | Screen mode: normal, full screen, full screen with the bars hidden. Control on a Mac too, not ⌘ |
| {{key:Esc}} | Leave full screen (when in full screen) |
| {{key:Mod+,}} | Settings, including the PartCraft tab (see [PartCraft settings](topic:settings)) |

In a panel's number box, {{key:Mod+Z}} is the browser's own text undo. In the number box over a sketch dimension, it undoes in PartCraft.

## Copy, paste and duplicate

They act on what you are working in: the selected geometry in an open sketch, the selected feature in the history, or the selected part in an assembly. Details in [Copy, paste and duplicate](topic:copy-paste).

| Key | Action |
|---|---|
| {{key:Mod+C}} | Copy the selection |
| {{key:Mod+X}} | Cut: copy, then delete |
| {{key:Mod+V}} | Paste. In a sketch the copy follows the pointer: click where it goes |
| {{key:Mod+Shift+V}} | Paste in place: same coordinates (in an assembly, the same place) |
| {{key:Mod+D}} | Duplicate: copy and paste in one step, leaving the clipboard alone |
| {{key:Mod+A}} | Select all (in a sketch) |
| {{key:Esc}} | Select none (and cancel a paste that follows the pointer) |

In a number box, these keys copy and paste text as usual. Selected text in a panel is copied by the browser.

## On the part (no sketch open)

| Key | Action |
|---|---|
| {{key:S}} | Sketch on the selected plane or flat face; with nothing picked, choose where by clicking |
| {{key:F}} | Fit the part in the view |
| {{key:M}} | Measure, in every workspace and in an assembly; {{key:Esc}} clears the picks, {{key:Esc}} again closes it (see [Measure distances, angles and mass](topic:measure)) |
| {{key:X}} | Section view on or off, in every workspace and in an assembly; {{key:Esc}} closes it (see [Look inside with a section view](topic:section-view)) |
| {{key:Delete}} | Delete the selected feature (asks if others use it) |
| {{key:0}} … {{key:9}}, {{key:.}} | With a feature selected: type its main number (an Extrude's depth, a Hole's diameter, a pattern's count, a fillet's radius …), then {{key:Enter}}. {{key:Esc}} keeps the old one |
| {{key:Backspace}} | Delete the selected feature, when one is selected |
| {{key:Esc}} | Stop choosing a sketch plane, or clear the selection (including edges or faces picked with {{key:Shift}}) |

## In the 3D view

Click in the view first so it has the keyboard focus.

| Key | Action |
|---|---|
| {{key:Left}} {{key:Right}} {{key:Up}} {{key:Down}} | Turn the view 15 degrees |
| {{key:Shift+Left}} and the other arrows | Pan |
| {{key:Plus}} or {{key:=}} | Zoom in |
| {{key:Minus}} | Zoom out |

| Mouse | Action |
|---|---|
| Drag with the left button | Turn (orbit) the view |
| Drag with the right or middle button | Pan |
| {{key:Shift}}, {{key:Ctrl}} or ⌘ held while dragging with the left button | Pan |
| Roll the wheel | Zoom at the pointer (away from you zooms in) |
| Two-finger swipe on a trackpad | Turn the view; with {{key:Shift}}, pan. Pinch zooms. Set by **Scrolling** in [PartCraft settings](topic:settings) |

With **Turning the view: Middle button** in [PartCraft settings](topic:settings), the middle button turns the view, {{key:Shift}} with the middle button or the right button pans, and the left button only picks.
| Click | Pick a face (it highlights whole), an edge or a plane |
| {{key:Shift}}+click an edge or face | Collect several edges or faces |
| Click empty space | Clear the pick |

## While picking edges or faces for a fillet, chamfer or shell


| Key or mouse | Action |
|---|---|
| Click an edge or face | Add it, or drop it if it is already in |
| {{key:Esc}} or {{key:Enter}} | Finish picking (same as {{ui:Done}}) |
| {{key:F}} | Fit |
| Arrow keys, with the view focused | Turn; with {{key:Shift}}, pan |

## On the drawing sheet

| Key | Action |
|---|---|
| {{key:F}} | Fit the sheet |
| {{key:Plus}} / {{key:Minus}} | Zoom (with the sheet focused) |
| {{key:Shift+Left}} and the other arrows | Pan (with the sheet focused) |
| {{key:Left}} {{key:Right}} {{key:Up}} {{key:Down}} | Nudge the selected GD&T symbol 1 mm |
| {{key:Delete}} or {{key:Backspace}} | Delete the selected symbol |
| {{key:Esc}} | Stop placing a symbol; else deselect; else back to the model |

| Mouse | Action |
|---|---|
| Click on a view, with Datum, Frame, Finish or Detail chosen | Place it there (snaps to the nearest line) |
| Click a symbol | Select it |
| Drag a symbol | Move it (the leader stays on its point) |
| Roll the wheel | Zoom at the pointer |
| Drag with the right or middle button | Pan |

## In a sketch

| Key | Action |
|---|---|
| {{key:L}} | Line |
| {{key:R}} | Rectangle |
| {{key:C}} | Circle |
| {{key:A}} | Arc (center, start, end) |
| {{key:P}} | Point |
| {{key:N}} | Centerline |
| {{key:F}} | Fillet a corner |
| {{key:D}} | Dimension |
| {{key:B}} | Spline (fit points) |
| {{key:T}} | Trim |
| {{key:E}} | Extend |
| {{key:O}} | Offset ({{key:B}} while offsetting: both sides) |
| {{key:M}} | Mirror |
| {{key:S}} | Select |
| {{key:Q}} | Switch the selected curves to construction and back |
| {{key:Enter}} | End a line chain or a spline |
| {{key:Delete}} or {{key:Backspace}} | Delete the selection |
| {{key:Mod+A}} | Select everything in the sketch |
| {{key:Mod+C}} / {{key:Mod+X}} / {{key:Mod+V}} | Copy, cut, paste geometry (paste follows the pointer) |
| {{key:Mod+Shift+V}} | Paste at the same coordinates |
| {{key:Mod+D}} | Duplicate the selection and place it with a click |
| {{key:Esc}} | Step back: cancel the number box or a paste, end the line chain, back to Select, clear the selection, then close the sketch |

| Mouse | Action |
|---|---|
| Click | Place points with a drawing tool; select with Select |
| {{key:Shift}}+click | Add to or take from the selection ({{key:Ctrl}}+click on Windows and Linux, ⌘+click on a Mac, do the same) |
| Drag on empty space (Select) | Window: selects everything fully inside |
| Drag a point, line or circle edge (Select) | Move it; relations hold |
| Drag a dimension's number (Select) | Move the label |
| Double-click a dimension | Change its value |
| Double-click or right-click while drawing lines | End the chain |
| Drag with the right or middle button | Pan |
| Roll the wheel | Zoom at the pointer |

### In the number box

| Key | Action |
|---|---|
| {{key:Enter}} | Apply the value |
| {{key:Esc}} | Close without changing |

Clicking away also applies the value.

## In the history list

| Key | Action |
|---|---|
| {{key:Tab}} | Reach the list |
| {{key:Up}} / {{key:Down}} | Move between rows |
| {{key:Enter}} or {{key:Space}} | Select the row |
| {{key:Alt+Up}} / {{key:Alt+Down}} | Move the selected feature up or down, where the history allows |

| Mouse | Action |
|---|---|
| Click | Select a plane or feature |
| Double-click a sketch | Open it |

## In an assembly

| Key | Action |
|---|---|
| {{key:J}} | Start a joint: then click a flat face on each of two parts |
| {{key:F}} | Fit |
| {{key:Esc}} | Cancel the joint you started, or clear the selection |
| {{key:Delete}} | Remove the selected part or joint |
| {{key:Backspace}} | Remove the selected part or joint |
| {{key:Mod+Z}} / {{key:Mod+Y}} | Undo and redo in the assembly's own list |
| {{key:Mod+D}} | Duplicate the selected part (same part, beside the others) |
| {{key:Mod+C}}, then {{key:Mod+V}} or {{key:Mod+Shift+V}} | Copy a part, paste it beside the others or in the same place |

Mouse in the view: as in the 3D view above. Click a part to select it. Double-click a part in the list to edit it.

## In the Script workspace

| Key | Action |
|---|---|
| {{key:F5}} | Run the script (in the editor, the panel or the view) |
| {{key:Mod+Enter}} | Run the selected lines, or the line the cursor is on |
| {{key:Tab}} / {{key:Shift+Tab}} | Indent / outdent the selected lines. To leave the editor with the keyboard, press {{key:Esc}}, then {{key:Tab}} |
| {{key:Mod+/}} | Comment the selected lines out, or back in |
| {{key:Mod+F}} / {{key:Mod+H}} | Find / find and replace in the script; {{key:Enter}} or {{key:F3}} next match, {{key:Shift+Enter}} or {{key:Shift+F3}} previous, {{key:Esc}} closes |
| {{key:Mod+Shift+[}} / {{key:Mod+Shift+]}} | Fold / unfold the block the cursor is on (a function, if, for, while … end) |
| {{key:Mod+Space}} | List completions for the name you are typing (it also opens after two letters); {{key:Up}} / {{key:Down}} choose, {{key:Enter}} or {{key:Tab}} takes one, {{key:Esc}} closes |
| {{key:Enter}} in the line under the console | Run that one statement; {{key:Up}} and {{key:Down}} bring back earlier ones |
| {{key:F}} | Fit the view to what scripts show |

See [Write and run scripts](topic:craftscript).

## Shortcuts the browser may take

- {{key:Mod+Y}} in Safari may open the history: use {{key:Mod+Shift+Z}} to redo.
- {{key:Mod+D}} is the browser's bookmark key: PartCraft takes it while the app has the focus, and leaves it alone while you type in a box.
- {{key:F11}} and {{key:Mod+Plus}} / {{key:Mod+Minus}} are the browser's full screen and page zoom, not PartCraft's. Use {{key:Ctrl+Shift+F}} for full screen and the wheel for the model.
- Function keys on many laptops and Macs need the Fn key: press Fn+{{key:F1}} for help, or use the {{ui:Help}} button.

Touch and trackpad gestures: [Trackpad, touch and small screens](topic:trackpad-and-touch).
