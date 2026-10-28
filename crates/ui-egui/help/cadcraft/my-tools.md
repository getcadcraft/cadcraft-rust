---
title: Buttons of your own (My tools)
summary: Add toolbar buttons that run your own macros, give them shortcut keys, share them as a file, and bring buttons over from a CUIx customization file.
section: tasks
order: 49
keywords: my tools, custom buttons, toolbar buttons, macro, macros, cui, cuix, customize, customise, menu, shortcut keys, keyboard shortcuts, diesel, ^c^c, import buttons, export buttons
---

## What you will learn

How to put buttons of your own on the toolbar, each running a macro written the way AutoCAD writes them, how to give a button a key, and how to bring the buttons of a CUIx file over.

## Add a button

1. Open {{ui:Settings}} ({{key:Mod+,}}) and stay on the {{ui:CadCraft}} tab. Scroll to {{ui:My tools}}.
2. Click {{ui:Add a button…}}.
3. Fill in the box:
   - {{ui:Group}}: the toolbar group it goes in, for example `My tools`. A new name makes a new group.
   - {{ui:Label}}: the words on the button.
   - {{ui:Macro}}: what it types, for example `^C^C_rectang;0,0;@100,50;`.
   - {{ui:Shortcut key}}: click the box and press the keys, for example {{key:Ctrl+Alt+B}}.
   - {{ui:Icon}}: a PNG or SVG picture of at most 64 KB, or none (a star shows).
4. Click {{ui:Save}}.

The button appears at the end of the toolbar, in its group. Click it, or press its key, to run it. {{ui:Edit}} and {{ui:Delete}} in the list change or remove it.

## Write a macro

A macro is what you would type at the command line, with a few signs:

| Sign | Means |
|---|---|
| `^C` | Cancel whatever runs (`^C^C` is the usual start) |
| `;` or a space | {{key:Enter}} |
| `\` | Wait for you: a click or a typed answer, then the macro carries on |
| `_` or `.` before a command | Accepted and ignored (international and built-in names) |
| `*` at the very start | Run again each time it ends, until you press {{key:Esc}} |
| `^O`, `^G` | Turn Ortho, the grid on or off |
| `$M=` | The rest is DIESEL. Only `$(if,…)`, `$(getvar,…)`, `$(=,…)` and `$(eq,…)` are read; a button using anything else says so and does nothing |

Examples:

- `^C^C_line;0,0;@100,0;;` draws a 100-long line from the origin.
- `^C^C_circle;\25;` asks you for the centre, then draws a circle of radius 25 there.
- `^C^C_turnright` turns the view 90° right.

## Shortcut keys

A key needs {{key:Mod}} (Ctrl, or Cmd on a Mac) or {{key:Alt}} with a letter or digit, or one of the free function keys {{key:F2}}, {{key:F4}}, {{key:F5}}, {{key:F6}}, {{key:F9}}. Letters on their own go to the command line. Keys CadCraft, the app or the browser already use (such as {{key:Ctrl+S}} or {{key:Ctrl+Z}}) and keys another button has are refused, and the box says why.

## Share your buttons

- {{ui:Export…}} saves every group and button (with icons and keys) as `my-tools.json`.
- {{ui:Import…}} adds the groups of such a file after yours. A group name you already use gets "(2)"; a key that would clash is left off, and the panel says so.

## Bring buttons over from a CUIx file

1. Click {{ui:Import CUIx…}} and choose the `.cuix` file.

Its toolbars and ribbon panels become groups, each button with its macro and its picture (PNG, or BMP turned into PNG). Its shortcut keys come in a group of their own, each button with its key; the command aliases it may carry come in an {{ui:Aliases}} group. The panel lists what came over.

:::note
The file is only read. Commands CadCraft does not have are listed by name, not guessed: their buttons say so when clicked. DIESEL beyond the few functions above, menus, mouse buttons and workspaces are not read.
:::

## Good to know

- Buttons are kept in this browser (or this desktop app), not in drawings.
- A button runs through the command line, so it is undone like anything you type: one {{key:Mod+Z}} per command it ran.
