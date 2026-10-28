---
title: Keys and mouse actions
summary: Every keyboard shortcut, mouse action, trackpad gesture and touch gesture in ModelCraft, grouped by what they do.
section: reference
order: 10
keywords: keyboard, shortcuts, keys, hotkeys, mouse, trackpad, touch, gestures, cheat sheet, ctrl, command, arrow keys
---

These are the push-pull keys, the ones ModelCraft starts with. For the other style, see [Modeler keys](topic:keyboard#modeler-keys) below and [ModelCraft settings](topic:settings).

Shortcuts work when the 3D view or a toolbar button has focus. If you just typed in a panel field or changed a list in {{ui:Style}}, click in the 3D view first, or your keys go to that field. See [Keys, mice and trackpads](common:keys) for how keys are written on each computer.

## Tools

| Key | Tool |
|---|---|
| {{key:Space}} | {{ui:Select}} |
| {{key:L}} | {{ui:Line}} |
| {{key:R}} | {{ui:Rectangle}} |
| {{key:C}} | {{ui:Circle}} |
| {{key:N}} | {{ui:Polygon}} |
| {{key:A}} | {{ui:Arc}} |
| {{key:F}} | {{ui:Offset}} |
| {{key:B}} | {{ui:Paint}} |
| {{key:P}} | {{ui:Push/pull}} |
| {{key:M}} | {{ui:Move}} |
| {{key:Q}} | {{ui:Rotate}} |
| {{key:S}} | {{ui:Scale}} |
| {{key:E}} | {{ui:Eraser}} |
| {{key:T}} | {{ui:Tape}} measure |
| {{key:D}} | {{ui:Dimension}} |
| {{key:O}} | {{ui:Orbit}} |
| {{key:H}} | {{ui:Pan}} |
| {{key:Z}} | {{ui:Zoom}} |

Tool letters work without modifier keys only: {{key:Shift}} with a letter does nothing, except the two below.

## Model and file

| Key | Does |
|---|---|
| {{key:G}} | Make component from the selection |
| {{key:Shift+G}} | Make group from the selection |
| {{key:Shift+Z}} | Zoom extents (the whole model) |
| {{key:Mod+Z}} | Undo |
| {{key:Mod+Y}} or {{key:Mod+Shift+Z}} | Redo |
| {{key:Mod+S}} | Save (downloads the `.modelcraft` file) |
| {{key:Mod+O}} | Open a model |
| {{key:Mod+A}} | Select everything (in the open group, if one is open) |
| {{key:Mod+C}} | Copy the selection |
| {{key:Mod+X}} | Cut the selection |
| {{key:Mod+V}} | Paste: the copy follows the pointer, click where it goes |
| {{key:Mod+Shift+V}} | Paste in place (where it was copied from) |
| {{key:Mod+D}} | Duplicate: a copy moves with the pointer, click to put it down |
| {{key:Delete}} or {{key:Backspace}} | Erase the selection |
| {{key:PageDown}} / {{key:PageUp}} | Next / previous scene |
| {{key:F1}} | Open or close this help (also {{ui:Help}} in the app bar; {{key:Esc}} inside the help closes it) |
| {{key:Ctrl+Shift+F}} | Screen mode: full screen, then bars hidden, then normal (Control on a Mac too) |

## Esc

{{key:Esc}} backs out one level each time you press it:

1. Cancels what the tool is in the middle of (a line being drawn, a push/pull, a move). Everything goes back.
2. Switches to {{ui:Select}}.
3. Clears the selection.
4. Closes the group you are editing.

It also stops scenes from playing. In the value box, {{key:Esc}} clears what you typed and returns to the view.

## While a tool is working

| Key | Does |
|---|---|
| A digit, or one of `. , - ' " [ < * /` | Starts typing in the value box |
| {{key:Enter}} | Applies what you typed in the value box |
| {{key:Right}} | Locks to the red axis (press again to unlock) |
| {{key:Left}} | Locks to the green axis |
| {{key:Up}} | Locks to the blue axis |
| {{key:Down}} | Locks parallel to the last edge you hovered before the first click |
| Hold {{key:Shift}} | Holds the direction you are snapped to, while held |
| {{key:Ctrl}}, {{key:Mod}} or {{key:Alt}} (Move) | Switches between moving and copying |

Arrow-key locks work in {{ui:Line}}, {{ui:Rectangle}}, {{ui:Circle}}, {{ui:Polygon}}, {{ui:Move}}, {{ui:Scale}} and {{ui:Tape}} (when measuring from a point), after the first click. {{ui:Push/pull}} always moves along the face's direction and has no locks. In {{ui:Rotate}} the arrow keys show a "Locked" message after the first click but do not change the turn.

## Arrow keys at other times

When no tool is in the middle of something, the arrow keys move the camera:

| Key | Does |
|---|---|
| {{key:Left}} {{key:Right}} {{key:Up}} {{key:Down}} | Orbit |
| {{key:Shift}} with an arrow | Pan |

## Mouse

| Action | Does |
|---|---|
| Click | Uses the current tool |
| Press, drag, release | Same as two clicks (for drawing tools); a box selection (for {{ui:Select}}); erases along the way (for {{ui:Eraser}}) |
| Double-click | {{ui:Select}}: face with its edges, edge with its faces, opens a group; empty space closes the group. {{ui:Push/pull}}: repeats the last distance |
| Triple-click | {{ui:Select}}: everything connected |
| {{key:Shift}} + click | {{ui:Select}}: toggle in and out of the selection |
| {{key:Mod}} + click | {{ui:Select}}: add to the selection |
| {{key:Shift}} + {{key:Mod}} + click | {{ui:Select}}: remove from the selection |
| Middle-button drag | Orbit, with any tool |
| {{key:Shift}} + middle-button drag | Pan, with any tool |
| Right-button drag | Pan, with any tool |
| Wheel | Zoom toward the pointer |

There is no right-click menu.

## Trackpad

What a two-finger swipe does depends on {{ui:Settings}} › {{ui:ModelCraft}} › Scrolling:

| Gesture | Trackpad (or worked out) | Mouse wheel |
|---|---|---|
| Two-finger swipe | Orbit | Zoom toward the pointer |
| {{key:Shift}} + two-finger swipe | Pan | Zoom |
| Pinch | Zoom toward the pointer | Zoom toward the pointer |
| Two-finger press and drag (secondary click held) | Pan | Pan |

See [Orbit, pan and zoom](topic:orbit-pan-zoom#on-a-trackpad) and [ModelCraft settings](topic:settings#scrolling-mouse-wheel-or-trackpad).

## Modeler keys

With {{ui:Settings}} › {{ui:ModelCraft}} › Controls set to Modeler style:

| Key | Does |
|---|---|
| {{key:W}} or {{key:Space}} | {{ui:Select}} |
| {{key:L}} / {{key:B}} / {{key:C}} / {{key:N}} | {{ui:Line}} / {{ui:Rectangle}} / {{ui:Circle}} / {{ui:Polygon}} |
| {{key:E}} | {{ui:Push/pull}} (extrude) |
| {{key:T}} | {{ui:Tape}} measure |
| {{key:D}} | {{ui:Dimension}} |
| {{key:G}} | Move the selection at once, from the pointer |
| {{key:R}} | Rotate the selection about its middle (blue axis) |
| {{key:S}} | Scale the selection about its middle |
| {{key:X}} / {{key:Y}} / {{key:Z}} while moving or rotating | Lock the red / green / blue axis (again to unlock) |
| Click or {{key:Enter}} | Put it down |
| {{key:Esc}} or right-click | Put it back |
| {{key:Shift+D}} | Duplicate and move the copy |
| {{key:X}} or {{key:Delete}} | Delete the selection |
| {{key:A}} / {{key:Alt+A}} | Select all / select nothing |
| {{key:Tab}} | Open the selected group or component; again to close it |
| {{key:Mod+G}} / {{key:Mod+Shift+G}} | Make group / make component |
| Numpad {{key:1}} / {{key:3}} / {{key:7}} | Front / right / top view ({{key:Mod}} for back / left / bottom) |
| Numpad {{key:5}} | Perspective on or off |
| Numpad {{key:.}} | Zoom to the selection |
| {{key:Home}} or {{key:Shift+C}} | Zoom extents |
| {{key:Shift+Tab}} | Snapping on or off |
| Middle-button drag / with {{key:Shift}} / with {{key:Mod}} | Orbit / pan / zoom |
| {{key:Alt}} + drag / {{key:Alt+Shift}} + drag | Orbit / pan (no middle button needed) |

Copy, paste, undo, save and open keep the keys listed above. Typed values, the value box and {{ui:Push/pull}} work the same in both styles.

## Touch screen

| Gesture | Does |
|---|---|
| Tap | A click with the current tool |
| Double-tap | A double-click |
| Press, drag, lift | Same as press, drag, release with a mouse |
| Two fingers: pinch | Zoom |
| Two fingers: move together | Pan (Settings › ModelCraft › Touch screen can make it Orbit) |
| Three fingers: move together | Orbit (Pan when two fingers orbit) |
| Hold one finger still | Finishes the tool, as Enter; with nothing going on, then drag to orbit |
| A second finger during a one-finger action | Cancels that action |
| {{ui:Orbit}} tool, then drag one finger | Orbit |

To type a value on a touch screen, tap the value box at the bottom, type, and press the keyboard's Enter (or Go) key.
