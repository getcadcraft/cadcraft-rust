---
title: Copy, paste and duplicate library items
summary: Copy a placed library item and paste it where you click, in place beside the original, or into another model; copy any other element's properties as text; undo and redo.
section: tasks
order: 155
keywords: copy, paste, duplicate, clipboard, paste in place, cut, delete, select all, undo, redo, edit menu, more, library item, placement, repeat, array
---

## What you'll do

Put more copies of an item you already placed: a second work table, a row of sinks. This topic is about **placed library items**, the elements BimCraft wrote into the model: one of them can be pasted into any model, even in another tab. Walls, doors and every other element are copied, cut, pasted and deleted the same way within their own model: see [Delete, cut, copy and paste building elements](topic:edit-elements).

The commands are in the toolbar above the model: {{ui:Undo}}, {{ui:Redo}}, {{ui:Copy}}, {{ui:Paste}} and {{ui:More}}, a menu with {{ui:Cut}}, {{ui:Copy}}, {{ui:Paste}}, {{ui:Paste in place}}, {{ui:Duplicate}}, {{ui:Delete}}, {{ui:Select all}} and {{ui:Select none}}. A command that cannot act right now is greyed out; point at it to read why, or click it and the status line says why.

## Steps

1. Open an IFC model with a library item in it, or place one first. See [Place equipment and furniture from the library](topic:place-from-library).
2. Click the item in the 3D view. Its properties show a **Library item** box.
3. Click {{ui:Copy}} or press {{key:Mod+C}}.
4. Click {{ui:Paste}} or press {{key:Mod+V}}. The placing bar opens and a see-through box follows the pointer, turned like the original.
5. Press {{key:R}} to turn it 90°, {{key:Tab}} to snap its back to a wall, or type X and Y in the placing bar.
6. Click where the copy goes. Each click places another copy; press {{key:Esc}} to stop.
7. Click {{ui:Save as IFC}} to download the edited copy of the model.

## Paste in place

{{ui:More}}, {{ui:Paste in place}} or {{key:Mod+Shift+V}} pastes with no clicking, at the same X and Y and turned the same way:

- **Into the same model, on the same storey**: two items in exactly one spot would be no use, so the copy goes right beside the original, one item width plus 100 mm along the item's own width. Paste in place again from the new copy to make a row.
- **Onto another storey or into another model**: exactly the same X, Y and turn. BimCraft uses the storey with the same name; if there is none, the one at the nearest height. The status line says which.

## Duplicate

{{ui:More}}, {{ui:Duplicate}} or {{key:Mod+D}} copies the picked item and starts pasting it straight away: one step instead of two.

## Into another model or another tab

What you copy is kept in this browser, so you can open another IFC model, or BimCraft in another tab, and paste there. The other model needs the library item itself: BimCraft has it when the item was placed, swapped in or updated in this tab. An item placed in an earlier session can be pasted again in its own model only; in another model, place it from the {{ui:Library}} tab.

## What you should see

- The copy in the model, picked, with the same library item, license, equipment data, quantity and note as the original, and the **next free item number**. Change the number in Properties, under `CadCraft_Placement`.
- "Placed: *item* (item *n*)" in the change list. {{key:Mod+Z}} takes it back; {{key:Mod+Y}} puts it back again.

## Good to know

- Copies share the original's type, so all of them use one set of geometry, as placing the same item twice does.
- {{ui:Undo}} and {{ui:Redo}} cover every change: values you edited, placements, pastes, swaps and updates. Redo puts a placement back at the same place; a new change clears what could be redone.
- In the 2D view of a drawing there is nothing to select, so there is nothing to copy. Use {{ui:Edit in CadCraft}} to change the drawing, when it shows.

## Common mistakes

- **{{ui:Paste}} says the elements came from another model.** The last thing you copied was a building element, or several elements: those paste into their own model only. Pick one placed library item and copy it.
- **{{ui:Paste}} says the copy came from another model without its library item.** Place the item from the {{ui:Library}} tab once in this tab, then copy and paste it.
- **{{key:Mod+C}} copies page text instead.** Text was selected on the page. Click the model first.

## Related

- [Keys and mouse](topic:keyboard#edit)
- [BimCraft settings](topic:settings)
