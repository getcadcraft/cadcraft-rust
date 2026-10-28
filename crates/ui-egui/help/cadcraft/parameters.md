---
title: Use parameters and equations
summary: Name sizes, drive them with expressions like width / 2, and type a number on a driven size to change what drives it.
section: tasks
order: 126
keywords: parameters, par, parameters manager, equations, expressions, variables, two-way, design intent, user parameter, fx
---

## One table for every size

The {{ui:Parameters}} panel (the {{ui:Parameters}} button in the {{ui:Parametric}} group, or type {{cmd:PARAMETERS}}) lists every dimensional constraint (`d1`, `rad1`, `dia1`, `ang1`, or the name you gave it) and your own parameters together. Any name can be used in any expression.

Each row has:

- **Name**: type a new one to rename it; every expression that uses it follows.
- **Expression**: a number (`40`) or an expression (`width / 2 + gap`). Type in it and press {{key:Enter}}: the drawing follows.
- **Value**: what it works out to. On a row an expression drives, you can type a number here too: CadCraft works out the parameter behind it. With `d1 = width / 2`, typing `30` in d1's value sets `width` to 60.

When two parameters drive a value (`width / 2 + gap`), a dialog asks which one to change. Every change is one undo step ({{key:Mod+Z}}).

## Add your own

1. Open the {{ui:Parameters}} panel.
2. Type a name (`gap`) and a value (`10`), then choose {{ui:Add}}.
3. Use it: in d1's expression, type `width / 2 + gap` and press {{key:Enter}}.

Names start with a letter or `_` and hold letters, digits and `_`. Function names, `pi` and unit names cannot be names.

## Expressions

| Write | Means |
|---|---|
| `+ - * / ^ %` | add, subtract, multiply, divide, power, remainder |
| `( )` | grouping |
| `sin cos tan asin acos atan atan2` | trigonometry in degrees |
| `sqrt pow exp ln log abs sign` | roots, powers, logarithms |
| `ceil floor int round min max` | rounding and limits |
| `if(a > b, x, y)` | a choice |
| `pi` | 3.14159… |

Numbers may carry a unit: `25mm`, `1in`, `2'`, `90deg`. Lengths with a unit are turned into millimetres, so use units only in drawings measured in millimetres; elsewhere type plain numbers.

## At the command line

`-PARAMETERS` does the same without the panel: `N` new, `E` edit (a dimensional constraint too), `R` rename, `D` delete, `?` list.

## What is refused

- An expression that uses itself, directly or through others: "width, d1 use each other."
- A value a size cannot take: a negative length, an angle over 180°.
- A value the drawing cannot take with its other constraints: nothing changes, and the message names the constraints in the way.
- Deleting a parameter something uses: the panel offers to write its value into them first. A dimensional constraint's name is deleted by deleting the constraint.
