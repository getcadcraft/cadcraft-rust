---
title: Set up grids and levels
summary: Type the grid lines, their spacings and the levels that every member is placed on.
section: tasks
order: 30
keywords: grid, grids, axes, gridlines, levels, elevations, storeys, floors, spacing, setup
---

Every member in DetailCraft is placed on a grid intersection at a level. Set the grids and levels first; you can change them later.

## How grids work here

- **Numbered lines** run one way, **lettered lines** the other. The names are only habit: a label can be any text up to 8 characters (1, 2, 3 or A1, A2, or 10, 20).
- You type the labels and the spacings between them, not coordinates. The first line of each set is at 0.
- Numbered lines are lines of constant x; lettered lines are lines of constant y. So a beam from grid 1 to grid 2 runs along x, and a beam from A to B runs along y.
- All values are millimeters.

A new model starts with numbered lines 1 2 3 at 6000 6000, lettered lines A B at 6000, and two levels.

## Set the grids

1. In the {{ui:Model}} panel on the left, open {{ui:Grids}} if it is folded.
2. In {{ui:Numbered lines}}, type the labels separated by spaces, for example `1 2 3 4 5`.
3. In the {{ui:Spacings}} field below it, type one spacing for each gap, for example `6000 6000 7500 6000`. Four gaps for five lines.
4. Do the same in {{ui:Lettered lines}} and its {{ui:Spacings}}, for example `A B C` and `8000 8000`.
5. Choose {{ui:Apply grids}}.

The status bar says "Grids updated." and the view zooms to the new grid. Commas work as separators too.

If something does not add up, nothing changes and the status bar says why, for example "Numbered grids: give one spacing between each pair of lines (4)." Other checks: spacings must be positive millimeters, labels must differ, and there can be at most 60 lines each way.

:::note
Members keep their coordinates when grids move. If you change a spacing after placing steel, the steel stays where it was and is no longer on the grid. Change the grid first, or move members by editing their end coordinates.
:::

## Set the levels

1. Open {{ui:Levels}} in the {{ui:Model}} panel.
2. Each row has a level name and its elevation in millimeters. Edit them as you like.
3. Choose {{ui:Add level}} for another row. It is named "Level 3" (and so on) and placed 4000 mm above the highest level; change both if you need to.
4. To remove a level, choose the × button at the end of its row.
5. Choose {{ui:Apply levels}}.

Nothing changes until you choose {{ui:Apply levels}}. Levels are sorted by elevation when applied. Every level needs a name, two levels cannot share a name, and you must keep at least one level.

Levels can be below zero (a basement at `-3500`) and do not need to be evenly spaced.

## How levels are used

- A column placed at a level runs up to the next level, or to the level you pick in {{ui:Up to}}.
- Beams and plates are placed between two intersections at the level you click.
- Footings are placed with their top at the level you click.
- {{ui:Plan}} looks down at one level at a time.
- The IFC export makes one building storey per level.

## Undo

Grid and level changes are edits like any other: {{key:Mod+Z}} puts the previous grid back.

## Next

[Place columns, beams and braces](topic:place-members)
