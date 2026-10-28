---
title: Change a part through its history
summary: Edit features and sketches after the fact, suppress, roll back, insert features earlier, reorder, rename and delete, and undo any of it.
section: tasks
order: 90
keywords: history, feature tree, design tree, edit feature, edit sketch, suppress, unsuppress, rollback, roll back, roll forward, reorder, move feature, delete, rename, undo, redo, rebuild, regenerate, parent child
---

A PartCraft part is its history: the list of planes, sketches and features on the left. PartCraft rebuilds the solid from the top of that list every time something changes. Changing a number early in the list changes everything built on it.

## Edit a feature

1. Click the feature in the history (or the sketch indented under it).
2. Change its settings in the right panel. Each change rebuilds at once.

There is no OK or Cancel: every change is applied. If you do not like it, press {{key:Mod+Z}}.

## Change which edges or faces a fillet, chamfer or shell uses

Double-click the feature in the history, or select it and choose {{ui:Pick edges}} (or {{ui:Pick faces}}). Click edges or faces to add or drop them, then {{ui:Done}} or {{key:Esc}}. See [Round, bevel and hollow](topic:fillet-chamfer-shell).

## Edit a sketch

1. Double-click the sketch in the history, or select it and choose {{ui:Edit sketch}} in the right panel (or {{ui:Sketch}} in the toolbar).
2. The part goes back to how it was when the sketch was made: features after it are hidden while you edit.
3. Change the geometry, relations or dimensions.
4. Close the sketch. The whole part rebuilds with the change.

To change only a size, you do not need to open the sketch. Select the feature or the sketch and change the number under **Sketch dimensions** (or **Driving dimensions**) in the right panel. See [Change sizes without opening the sketch](topic:change-sizes).

## When a feature fails

If a change makes a feature impossible, PartCraft builds everything else and marks the problem:

- the feature shows **⚠ error** in the history, and its message is at the top of its right panel (hover the row to read it too);
- features that need it show **⚠ not built**, with "Needs Extrude2, which is failing.";
- the status bar chip says "1 feature failing".

Fix the cause (or undo), and the part builds again. Common messages and what to do are in [Troubleshooting](topic:troubleshooting).

## Suppress a feature

Suppressing switches a feature off without deleting it: useful to try a part without a hole or a pattern.

1. Select the feature.
2. Choose {{ui:Suppress}}. Its row shows **off**. Features that need it are not built either.
3. Choose {{ui:Unsuppress}} to switch it back on.

## Roll back

Rolling back builds the part only up to a point, so you can see it as it was, or add a feature earlier in the history.

1. Select a feature and choose {{ui:Roll back to here}}. That feature and every one below it are grayed and not built. A **Rollback bar · roll forward** row marks the place.
2. Any feature you add now goes in **at the bar**, before the grayed ones.
3. Click **Rollback bar · roll forward** to build everything again.

The rollback position is saved in the file.

## Reorder features

1. Select the feature.
2. Choose {{ui:Move up}} or {{ui:Move down}}, or, with its history row focused, press {{key:Alt+Up}} or {{key:Alt+Down}}.

A feature can never move above something it uses. The button is grayed, and hovering it (or trying the key) gives the reason, for example "Extrude2 uses Sketch2, so Sketch2 has to come first."

What a feature uses: its sketch; the face its sketch sits on; the features a pattern or mirror copies; a sketch line or revolve used as a direction or axis.

## Delete a feature

1. Select the feature and choose {{ui:Delete}} in the right panel or the toolbar, or press {{key:Delete}}.
2. If other features use it, PartCraft asks: "Extrude3, Hole1 use Extrude1 and will be deleted too." Choose {{ui:Delete all}} or {{ui:Cancel}}.

Deleting a feature does not delete its sketch: the sketch reappears in the view as an unused sketch, ready for another feature. Delete the sketch separately if you do not need it.

## Rename

Select the feature and type a new **Name** at the bottom of the right panel (up to 60 characters). The part's own name is in the right panel when nothing is selected.

## Undo and redo

- {{key:Mod+Z}} or {{ui:Undo}} undoes the last change: a feature, a setting, a sketch edit, a drag. The status bar says what, for example "Undid change dimension."
- {{key:Mod+Y}}, {{key:Mod+Shift+Z}} or {{ui:Redo}} redoes it.
- PartCraft keeps the last 200 steps. Changes on the drawing sheet (a section, a moved GD&T symbol) are in the same list. Opening a file or starting a new part clears it.

:::mac
On a Mac, use ⇧⌘Z to redo. In Safari, ⌘Y may open the browser's history instead.
:::
