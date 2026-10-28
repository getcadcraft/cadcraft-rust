---
title: 3D text
summary: Add lettering to a model as faces, flat or solid, standing on any face: a sign, a house number, a name on a door.
section: tasks
order: 36
keywords: 3d text, text, letters, lettering, sign, signage, house number, label, engrave, font, words
---

{{ui:3D Text}} in the {{ui:Draw}} group makes lettering out of faces, which you can paint, push/pull or cut into a solid.

## Add text

1. Click {{ui:3D Text}}.
2. Type the text. Press {{key:Enter}} in the box for a second line.
3. Set the {{ui:Letter height}} (the height of a capital letter, such as `30cm` or `1'`) and the {{ui:Depth}}:
   - `0` gives **flat letters**: faces lying on whatever you place them on, ready to paint or to push/pull.
   - A depth such as `5cm` gives **solid letters**, closed solids you can use with the [solid tools](topic:solids).
4. Click {{ui:Place}}, then click where the text goes.

On a face, the text lies on that face, reading left to right: on a wall it stands upright; on the ground or a table it lies flat along red. Away from any face it lies on the ground.

The text is a **component** named after it, so every copy changes if you edit one, and it shows in the {{ui:Components}} list to place again.

## Things to know

- The letters are ModelCraft's own single-stroke font, thickened: clean and readable, with no choice of typeface yet.
- Letters, digits and common signs (including accented letters) are drawn; a character the font does not have shows as a small box.
- To change the words, delete the text and add it again.
- To cut lettering into a surface, make solid letters, place them half-way into a solid group, and [subtract](topic:solids) them.
