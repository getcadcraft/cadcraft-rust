---
title: Copy, paste and duplicate
summary: Select several parts, copy them with their holes, copes and connections, and paste them on a grid intersection, at an offset, in place, or in another tab. Cut, duplicate, delete and select all.
section: tasks
order: 45
keywords: copy, paste, cut, duplicate, clipboard, paste in place, select all, select none, multi-select, shift click, several, repeat a bay, copy a frame, offset, delete several, edit menu, more menu
---

The {{ui:Edit}} group on the toolbar has {{ui:Undo}}, {{ui:Redo}}, {{ui:Copy}} and {{ui:Paste}}. Its {{ui:More}} menu holds the rest: {{ui:Cut}}, {{ui:Copy}}, {{ui:Paste}}, {{ui:Paste in place}}, {{ui:Duplicate}}, {{ui:Delete}}, {{ui:Select all}} and {{ui:Select none}}, each with its key. A button or menu item that cannot act right now is grayed out; hover over it to read why (for example "nothing copied yet").

Every paste, cut, duplicate and delete is one step: one {{key:Mod+Z}} takes it all back.

## Select several parts

- Click a part to select it. Hold {{key:Shift}} (or {{key:Ctrl}}, or {{key:Mod}}) and click more parts to add them; click a selected one the same way to take it out.
- {{key:Mod+A}} or {{ui:Select all}} selects every member, connection and footing.
- {{key:Esc}} or {{ui:Select none}} clears the selection.

With several things selected, the {{ui:Properties}} panel is headed "4 selected" (or however many) and lists the members by type, the connections, the footings and the gross steel weight, with {{ui:Copy}}, {{ui:Duplicate}} and {{ui:Delete}} buttons.

## Copy

1. Select the members and footings you want.
2. Choose {{ui:Copy}} or press {{key:Mod+C}}.

What comes along:

- Each member whole: profile, material, ends, position, rotation, name, holes and copes. A custom profile it uses is copied too.
- Each footing whole: size, concrete, bottom mat and starter bars.
- End plates and base plates, but only when every member a connection joins is copied: a beam's end plate needs the beam and its column. The status bar says how many connections were left out. To leave connections out on purpose, clear **Copy end plates and base plates with their members** in [DetailCraft settings](topic:settings).

A connection on its own cannot be copied; select its members instead. To give other joints the same connection, use {{ui:Similar}} (see [End plates](topic:end-plates#copy-it-to-every-similar-joint)).

The copy stays in this browser until you copy something else. Another DetailCraft tab or window in the same browser can paste it, so you can copy a frame from one model and paste it into another.

## Paste on a grid intersection

1. Choose {{ui:Paste}} or press {{key:Mod+V}}.
2. Move the pointer over the view. A dashed outline of the copy follows the grid intersection nearest the pointer.
3. Click an intersection. The copy's base point (its lowest corner: the smallest x, y and z of everything copied) lands there.

The pasted parts are selected afterwards, so you can paste again somewhere else, or change them. {{key:Esc}} cancels a paste; the copy stays on the clipboard. {{ui:Plan}} and {{ui:Elevation}} make the right intersection easier to hit.

:::tip
To repeat a bay, select the bay's members (or everything with {{key:Mod+A}}), copy, paste, and click the intersection one bay along from where the copy's lowest corner was.
:::

## Paste from the keyboard

While pasting, the {{ui:Properties}} panel shows what is on the clipboard and two forms:

- {{ui:Paste by offset (mm, from the original)}}: type x, y and z and choose {{ui:Paste at this offset}}. It starts with the offset from [DetailCraft settings](topic:settings).
- {{ui:Paste onto a grid intersection}}: pick a lettered grid, a numbered grid and a level, and choose {{ui:Paste here}}. The base point goes there.

## Paste in place

{{ui:Paste in place}} ({{key:Mod+Shift+V}}) puts the copy at exactly the coordinates it was copied from. Use it after {{ui:Cut}}, or to bring parts from another model at the same place. If the originals are still there, the copy sits on top of them and the status bar warns you: move the copies by their {{ui:Start}} and {{ui:End}} in {{ui:Properties}}, or undo.

## Duplicate

{{ui:Duplicate}} ({{key:Mod+D}}) copies the selection and pastes it at once, moved by the offset in [DetailCraft settings](topic:settings) (6000 mm along x to start with). It does not change what is on the clipboard. The copies are selected afterwards, so pressing {{key:Mod+D}} again makes the next one from them.

## Cut and delete

- {{ui:Cut}} ({{key:Mod+X}}) copies the selection, then deletes it. Paste it where it belongs.
- {{ui:Delete}} ({{key:Delete}} or {{key:Backspace}}) deletes the selection. Deleting a member deletes its connections too.

## Marks after a paste

Part and assembly marks are never stored; DetailCraft numbers them again from the model after every change ([How marks are given](topic:mark-rules)). So:

- A copy that is the same piece as its original (same profile, grade, cut length, holes and copes) shares the original's mark, and the count in {{ui:Part marks}} and the bill of materials goes up.
- A copy that ends up different gets a mark of its own. A beam pasted without its end plates, for example, is not cut back, so it is a different piece. Marks are numbered again across the whole model, so other parts' numbers can change too.

Every pasted member, hole, cope, connection and footing gets a new id, so nothing in the model is ever shared between a copy and its original.

## If the clipboard is refused

What is on the clipboard is checked as carefully as a model file you open: anything that is not a valid DetailCraft part is dropped, and the status bar says what. A clipboard filled by a newer DetailCraft, or by something else, is refused with a message and nothing is pasted. If a private window cannot keep the copy in browser storage, the copy still works in that tab.
