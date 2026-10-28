---
title: Copy, paste and duplicate
summary: Copy sketch geometry, features and assembly parts; paste where you click or in place; duplicate in one step; and what comes along with a copy and what does not.
section: tasks
order: 95
keywords: copy, paste, cut, duplicate, clipboard, paste in place, select all, select none, delete, ctrl c, ctrl v, ctrl d, instance, component, copy sketch, copy feature, copy part
---

The {{ui:Edit}} group in the toolbar holds {{ui:Undo}}, {{ui:Redo}}, {{ui:Copy}}, {{ui:Paste}} and a {{ui:More}} menu with {{ui:Cut}}, {{ui:Copy}}, {{ui:Paste}}, {{ui:Paste in place}}, {{ui:Duplicate}}, {{ui:Delete}}, {{ui:Select all}} and {{ui:Select none}}, each with its key. What they copy depends on where you are: geometry in an open sketch, a feature in the history, or a part in an assembly.

A grayed-out item cannot act right now. Hover over it: its tooltip says why, for example "Select sketch geometry first" or "The clipboard holds Sketch2 and Extrude2: features paste into the history, so exit the sketch first".

| Action | Key | In a sketch | In the history | In an assembly |
|---|---|---|---|---|
| Copy | {{key:Mod+C}} | The selected points and curves | The selected sketch, extrude, revolve or hole (with its sketch) | The selected part |
| Cut | {{key:Mod+X}} | Copy, then delete them | Copy, then delete the feature | Copy, then remove the part |
| Paste | {{key:Mod+V}} | The copy follows the pointer; click where it goes | Adds the copy at the end of the history | A new copy beside the other parts |
| Paste in place | {{key:Mod+Shift+V}} | At the same coordinates | Same as Paste | In the same place as the copied part |
| Duplicate | {{key:Mod+D}} | Copy and move in one go; the clipboard is left alone | A second copy right after you | A second copy beside the other parts |
| Delete | {{key:Delete}} | The selection | The feature (asks if others use it) | The part or joint |
| Select all | {{key:Mod+A}} | Everything in the sketch | Not available | Not available |
| Select none | {{key:Esc}} | Clears the selection | Clears the selection and picks | Clears the selection |

Every paste, duplicate and cut is one step in the undo list: {{key:Mod+Z}} takes it back, {{key:Mod+Y}} puts it back.

## In a sketch

1. Select what to copy: click it, {{key:Shift}}+click for more, drag a window, or press {{key:Mod+A}} for everything.
2. Press {{key:Mod+C}} or choose {{ui:Copy}}. The status bar says what was copied.
3. Press {{key:Mod+V}} or choose {{ui:Paste}}. A dashed copy follows the pointer, held by its middle. It snaps to points: put a copied circle's center on a point by clicking that point.
4. Click where it goes. The pasted geometry stays selected, so you can drag it or add relations to it straight away. {{key:Esc}} cancels the paste.

What comes along:

- A copied line, circle or arc brings its own end points and center.
- Relations and dimensions **between copied items** come along: a copied rectangle is still a rectangle, with its sizes.
- Relations **to geometry you did not copy**, and to the origin, are dropped. The status bar counts them: "2 relations to geometry outside the copy will not come along". So a pasted copy is free to move until you tie it in with relations or dimensions.
- A fixed point stays fixed, at its new position.

{{ui:Paste in place}} puts the copy exactly on the original's coordinates, in this sketch or in another one: handy to bring a profile from one sketch into another. In the same sketch the copy lies on top of the original; drag it away.

{{ui:Duplicate}} ({{key:Mod+D}}) copies the selection and starts placing it at once, without touching what is on the clipboard.

:::tip
To repeat a hole center or a slot several times, copy it once and paste it several times: every {{key:Mod+V}} starts a new copy that follows the pointer.
:::

## In the history

1. Click a sketch, an extrude, a revolve or a hole in the history.
2. {{key:Mod+C}} copies it. A sketch-based feature is copied together with its sketch.
3. {{key:Mod+V}} adds the copy at the end of the history (at the rollback bar when you have rolled back), with new names such as Sketch3 and Extrude2.

The copy sits on the same plane or face as the original, so it builds on top of it and the part looks the same. Select the new sketch and change its plane in the right panel, or open it and drag or re-dimension the profile, to move the copy.

- Paste into **another part**: open that part and press {{key:Mod+V}}. When the copy's sketch was on a face that part does not have, it goes on the front plane, and the status bar says so.
- {{ui:Duplicate}} does copy and paste in one step, inside the same part.
- {{ui:Cut}} copies the feature, then deletes it (asking first if other features use it).

Patterns, mirrors, fillets, chamfers, shells and imported solids cannot be copied: they hang on edges, faces or features picked in this part. Copy the sketch or the feature they are built on instead, or pattern it with {{ui:Linear}} or {{ui:Circular}}.

## In an assembly

1. Click a part in the view or in the list on the left.
2. Choose {{ui:Duplicate}} ({{key:Mod+D}}), or {{key:Mod+C}} then {{key:Mod+V}}.

The new copy is the **same part**: edit one and every copy changes. It appears 20 mm to the right of everything already in the assembly, turned the same way as the copy, and stays there until a joint places it. {{ui:Paste in place}} ({{key:Mod+Shift+V}}) puts it exactly where the copied part is; join it to move it (a slider joint's offset pushes it along).

Pasting a part copied from another assembly brings the part along, the same as {{ui:Insert}} from its file. {{ui:Add copy}} in the Assembly group does the same as {{ui:Duplicate}}, and works with nothing selected (it copies the last part).

## The clipboard

- It is PartCraft's own clipboard, not the computer's: you cannot paste PartCraft geometry into another program, or text into PartCraft's views.
- It is kept in this browser, so another PartCraft tab can paste what you copied. It is read back and checked like an opened file; anything damaged or from another version is refused.
- A part with a large imported STEP file may be too big for the browser to keep: it then pastes only in this tab.
- In a number box or the help search, {{key:Mod+C}} and {{key:Mod+V}} copy and paste text as usual.

On the drawing sheet, copy and paste are not available yet; {{ui:Delete}} and {{ui:Select none}} work on the selected symbol. See [Not built yet](topic:not-built-yet).
