---
title: Drive a part with parameters
summary: Name the sizes that matter (width, thickness, bolt size), link any feature number or sketch dimension to them with =, and change the whole part from one table.
section: tasks
order: 102
keywords: parameters, where used, used by, global variables, variables, equations, design intent, driving dimensions, link, expression, formula, name a dimension, linked values
---

A parameter is a named number, such as `width = 120` or `margin = bolt * 1.5`. Link a depth, a hole diameter, a pattern count or a sketch dimension to it, and changing the parameter changes all of them in one step. This is how you keep design intent in the part instead of in your head.

## Add parameters

1. In the right panel, click the {{ui:Parameters}} tab.
2. Under **Variables**, type a name in the "name" box, for example `width`.
3. Type its value in the next box, for example `120`, and press {{key:Enter}} (or click {{ui:Add}}).

A value can be a number or an expression of other parameters: `bolt * 1.5`, `width / 2 - 10`, `if(width > 100, 6, 4)`. The **Value** column shows what it works out to. A name starts with a letter or `_` and has only letters, digits and `_`; names of functions and units (`sin`, `mm`, `pi`) are refused, and so is a loop such as `a = b` with `b = a`.

## Link a number to a parameter

1. Click a feature in the history, for example "Extrude1". Its settings show on the {{ui:Properties}} tab.
2. In a number box, such as **Depth (mm)**, type `=` and an expression: `=thick` or `=width / 4`.
3. Press {{key:Enter}}.

The box now shows the result, and an **ƒ** button appears next to its label. Point at it to see the equation. Sketch dimensions work the same way, both in the **Sketch dimensions** list of a feature and in the dimension box of an open sketch (double-click a dimension, type `=width`, press {{key:Enter}}).

Numbers you can link: extrude depth, revolve angle, hole diameter, depth and counterbore sizes, pattern count, spacing and angle, fillet radius, chamfer distance, shell wall thickness, every sketch dimension, and the sizes of the newer features (second depth, taper, countersink, reference plane distance, draft angle and others).

## Change a parameter

On the {{ui:Parameters}} tab, type a new value or expression in the **Expression** column and press {{key:Enter}}. Everything linked to it changes and the part rebuilds, as one step: {{key:Mod+Z}} undoes it.

To change a number from the other end, see [Equations both ways](topic:equations-two-way).

## See where a parameter is used

Under each parameter on the {{ui:Parameters}} tab, a line says what uses it:

- the numbers it drives, such as **Extrude1 depth** or **Sketch1 D2 (distance)**. Click one to select its feature and show its properties;
- the numbers it drives **through** other parameters, such as **Hole1 diameter (through h, d)** when `d = h + 2` and `h = w / 2`;
- the **parameters** whose expressions use it, the **configurations** that set it, and the **scripts** that read it (see [Write and run scripts](topic:craftscript)).

"Not used yet." means nothing uses it: you can delete it without asking.

## Unlink, rename, delete

- **Unlink a number:** click its **ƒ** button, or {{ui:Unlink}} in the **Equations** list. It keeps its current value and no parameter changes it any more.
- **Rename a parameter:** type a new name in its **Name** box. Every expression that uses it is rewritten.
- **Delete a parameter:** click × on its row. If something uses it, PartCraft asks whether to write its current value into those expressions first.
- **Deleting a feature** drops the equations of its numbers; the status bar says which.

:::note
Parameters are saved in the `.partcraft.json` file (as file version 3). A part without parameters is saved exactly as before.
:::

## Expressions

| Write | Means |
|---|---|
| `120`, `12.5`, `1e3` | a number (millimetres, or degrees for an angle) |
| `2in`, `1ft`, `3"`, `1.5cm`, `0.2m` | a length in other units, turned into millimetres |
| `30deg`, `0.5rad` | an angle, turned into degrees |
| `+ - * / %` and `^` | arithmetic; `^` is a power, `-2^2` is −4 |
| `sin cos tan asin acos atan atan2` | trigonometry in degrees |
| `sqrt pow exp ln log abs sign` | other functions |
| `ceil floor int round(x, digits) min max` | rounding and limits |
| `if(test, a, b)` with `< <= > >= == !=` | a choice: `if(bolt >= 16, 6, 8)` |
| `pi` | 3.14159… |

Names are case sensitive: `Width` and `width` are two parameters.

See also [Configurations and design tables](topic:design-tables) and [Scripts and the command line](topic:scripts-and-cli).
