---
title: Copy, paste and duplicate
summary: Copy objects to CadCraft's clipboard, paste them where you click or in place, duplicate with the pointer, and bring work across from another drawing or tab.
section: tasks
order: 45
keywords: copy, cut, paste, paste in place, duplicate, clipboard, copyclip, cutclip, pasteclip, pasteorig, copybase, ctrl c, ctrl v, ctrl x, ctrl d, between drawings, another tab, base point
---

## What you will learn

How to copy objects to the clipboard and paste them where you click, paste them at their own coordinates, duplicate them with the pointer, cut them, and carry them into another drawing. Everything here is also in the toolbar's {{ui:Edit}} group: {{ui:Copy}}, {{ui:Paste}} and the {{ui:More}} menu.

## Copy and paste where you click

1. Select the objects: click them, or drag a window.
2. Press {{key:Mod+C}}, or click {{ui:Copy}} in the {{ui:Edit}} group. The command line says how many objects were copied. They stay selected.
3. Press {{key:Mod+V}}, or click {{ui:Paste}}. The copy appears on the cursor, hanging from the lower-left corner of the copied objects.
4. Click where that corner goes, or type a point such as `2000,500` and press {{key:Enter}}. Object snaps and Ortho work as usual.

The pasted objects are selected, so you can move or change them straight away. One {{key:Mod+Z}} takes the whole paste away.

:::tip
Want the copy to hang from a point you choose, such as the corner of a wall? Type `COPYBASE`, click the base point, select the objects, and press {{key:Enter}}. Pasting then places that point where you click. To hang every copy from its center instead of its lower-left corner, change {{ui:Paste point}} in [CadCraft settings](topic:settings).
:::

## Paste in place

Press {{key:Mod+Shift+V}}, or choose {{ui:Paste in place}} in the {{ui:More}} menu. The copy goes in at exactly the coordinates it was copied from, with no click. Use it to carry objects into another drawing at the same place, or from a sheet into another sheet.

## Duplicate and move

1. Select the objects.
2. Press {{key:Mod+D}}, or choose {{ui:Duplicate and move}} in the {{ui:More}} menu.
3. A copy sits on top of the original and moves with the pointer. Click where it goes. You can also type a displacement such as `@0,-600`, or a distance toward the cursor.

{{key:Esc}} before the click leaves nothing behind. The duplicate is one undo step and does not touch the clipboard.

## Cut

Press {{key:Mod+X}}, or choose {{ui:Cut}} in the {{ui:More}} menu. The objects go on the clipboard and are erased from the drawing, in one undo step. Paste them back with {{key:Mod+V}} or {{key:Mod+Shift+V}}.

## Between drawings and tabs

The clipboard keeps its copy when you open another drawing, and it is shared by every CadCraft tab in this browser. Copy in one tab, then press {{key:Mod+V}} in another.

- Blocks the objects use come with them. If the drawing you paste into already has a block of the same name, its own definition is used, as AutoCAD does.
- Layers the objects use are added, with the color and settings they had.
- The copy is not rescaled when the two drawings use different units: CadCraft says so, and you can use {{cmd:SCALE}} afterwards.
- Viewports stay on their sheet; they are left out of a copy.

:::note
The clipboard is CadCraft's own. You cannot paste CadCraft objects into another program, or paste from another program into CadCraft: save as DWG or DXF instead. A very large copy may be too big to share between tabs; the command line then says only this tab can paste it.
:::

## Common mistakes

- **{{key:Mod+V}} pasted text into the command line.** You had typed something there, so the keys worked on the text. Press {{key:Esc}} to clear it, then press {{key:Mod+V}} again.
- **Nothing happened, and the status bar says to finish the current command.** Another command is running. Press {{key:Esc}} first.
- **The paste landed far away.** It hangs from the lower-left corner of what you copied, which may be a distant object you selected by accident. Use {{ui:Paste in place}}, or copy with `COPYBASE`.
- **"Nothing pasted" with a reason.** What another tab left on the clipboard could not be read (it was damaged, or changed by hand). Copy the objects again.

See the full list of keys in [Keyboard and mouse](topic:keyboard), and every prompt in the [Command reference](topic:commands#clipboard).
