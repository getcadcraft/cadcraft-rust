---
title: Type exact sizes in the value box
summary: Every way to type lengths, sizes, points, offsets, angles, sides and copies, and how units and bare numbers are read.
section: reference
order: 40
keywords: value box, measurements box, vcb, type, exact, dimension, length, units, feet, inches, metric, millimeters, coordinates, offset, array, sides, angle, decimal comma
---

The value box sits in the status bar at the bottom, with a label that changes with the tool. While you work it shows the live size; at any moment you can type an exact one.

## How to type

1. Start the action with a click (the first corner, the face, the point to move from).
2. Type. You don't need to click the value box: a digit or one of `. , - ' " [ < * /` sends your typing there.
3. Press {{key:Enter}}.

Press {{key:Esc}} in the value box to clear it and go back to the view. On a touch screen, tap the value box to bring up the keyboard.

If ModelCraft cannot read what you typed, the status bar says "Type a length such as 3m, 10'6" or 4m,3m." If the tool cannot use it, it says "That did not work: …" with the reason.

## Lengths

| Type | Means |
|---|---|
| `3m` `320cm` `3200mm` | Meters, centimeters, millimeters |
| `10'` `10ft` `10 feet` | Feet |
| `6"` `6in` `6 inches` | Inches |
| `10'6"` `10' 6"` `10'-6 1/2"` | Feet and inches, with a fraction |
| `6 1/2"` `6-1/2"` `3/4"` | Inches with a fraction |
| `1.5ft` `.5m` | Decimals with a unit |
| `120` | A bare number, in the model's unit |

**A bare number uses the model's unit**, as shown in the picker at the bottom right: inches in a feet-and-inches model (so `120` is 10 feet), millimeters in a millimeter model, and so on. Typing a unit always wins, whatever the model's unit. Curly quotes and prime marks (’ ” ′ ″) are read as feet and inches too.

:::warning
Always use a **period** for decimals: `2.5m`. A comma separates two values, so `2,5` means 2 and 5 (two values), whatever your language settings.
:::

## Per tool

| Tool | Label | Type |
|---|---|---|
| {{ui:Line}} | {{ui:Length}} | A length (toward the pointer), or a point or offset (below) |
| {{ui:Rectangle}} | {{ui:Dimensions}} | Two lengths: `4m,3m`, `12',10'`. A semicolon works too: `4m;3m` |
| {{ui:Circle}}, {{ui:Polygon}} | {{ui:Sides}}, then {{ui:Radius}} | Before the first click, the number of sides: `12s` or `12` (3 to 999). After it, the radius |
| {{ui:Push/pull}} | {{ui:Distance}} | A length. A negative one goes the other way |
| {{ui:Move}} | {{ui:Distance}} | A length (toward the pointer), or a point or offset. After a copy: `3x`, `*3`, `/3` |
| {{ui:Rotate}} | {{ui:Angle}} | Degrees: `90`, `-45`, `22.5` |
| {{ui:Scale}} | {{ui:Scale}} | A factor above 0: `2`, `0.5`, `1.25` |
| {{ui:Tape}} | {{ui:Offset}} or {{ui:Length}} | A length |
| {{ui:Dimension}} | {{ui:Offset}}, then {{ui:Length}} | Where the dimension line goes; right after placing, a new length resizes the model |
| {{ui:Select}} | {{ui:Value}} | With one dimension or edge selected, a new length resizes the model. See [Dimension and resize](topic:dimensions) |

## Points and offsets

For {{ui:Line}} and {{ui:Move}}, after the first click:

- `[x,y,z]` goes to that exact point in the model (in the group's own axes when you are editing inside a group). Example: `[0,0,3m]` is 3 m straight above the origin.
- `<dx,dy,dz>` goes that far from the first point along red, green and blue. Example: `<0,0,10'>` is 10 feet straight up.

Each of the three values can have its own unit: `<12',0,6">`.

## Copies after a move

Right after a copy with {{ui:Move}}:

| Type | Makes |
|---|---|
| `3x` or `*3` | 3 copies in a row at the same spacing |
| `/3` | 3 copies spread evenly from the original to the first copy |

Up to 500 copies. See [Move, copy and make arrays](topic:move-copy#a-row-of-copies).

## Typed lengths need a direction

A single typed length goes the way the pointer is pointing from the first click. If you type right after clicking, before moving at all, there is no direction yet and nothing happens. Move the pointer a little toward where you want to go, then type.

## The model's unit

Change the unit in the picker at the bottom right: {{ui:ft and in}}, {{ui:in}}, {{ui:m}}, {{ui:cm}} or {{ui:mm}}. It changes how lengths are shown and how bare numbers are read; the model stays the same size. The unit is saved in the file.

How sizes are shown:

| Unit | Lengths | Areas | Volumes |
|---|---|---|---|
| ft and in | `10' 6 1/2"` (to 1/16") | sq ft | cu ft |
| in | `126 1/2"` | sq in | cu in |
| m | `3.2 m` | m² | m³ |
| cm | `320 cm` | cm² | cm³ |
| mm | `3200 mm` | m² from 0.01 m², else mm² | m³ from 0.01 m³, else mm³ |

A new model starts in feet and inches when the browser's language is set to a US region (and Liberia and Myanmar), and in millimeters everywhere else. To start in meters, change the picker after {{ui:New}}.
