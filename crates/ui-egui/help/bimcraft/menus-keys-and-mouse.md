---
title: Menus, keys and mouse
summary: Pick a menu style that matches the way you already work, give any command your own key, move commands between menus, and set what the wheel and each mouse button do.
section: reference
order: 6
keywords: menu style, menus, menu bar, ribbon, command line, keyboard shortcuts, hotkeys, keys, customize, remap, rebind, mouse, wheel, invert, zoom direction, middle button, pan, orbit, two-letter keys, export, import, switching, familiar
---

BimCraft's commands are the same whichever way you reach them. A **menu style** only decides where they sit on screen, what they are called there and which keys run them, so you can keep the habits you have. Choose one in {{ui:Settings}} ({{key:Mod+,}}) › {{ui:BimCraft}} › {{ui:Menus, keys and mouse}}; the change is immediate, and this browser remembers it.

## The five styles

| Style | What you get |
|---|---|
| {{ui:CadCraft}} | BimCraft's own toolbar and keys ({{key:W}} wall, {{key:D}} door, {{key:N}} window, {{key:X}} section, {{key:F}} fit). The start. |
| {{ui:Home design}} | A menu bar (File, Edit, Build, Library, 3D, CAD, Tools, View) over a row of building tools, the way residential home-design programs lay them out. The same single keys, plus {{key:Mod+L}} for the library, {{key:F9}} for the 3D view, {{key:F10}} for the plan and {{key:F11}} for both. |
| {{ui:BIM ribbon}} | Tabs of large buttons (Architecture, Insert, Annotate, Modify, View, Output) with a File menu and quick buttons on the right. Two-letter keys: {{key:W}} then {{key:A}} for a wall, {{key:D}} {{key:R}} door, {{key:W}} {{key:N}} window, {{key:S}} {{key:C}} section, {{key:Z}} {{key:F}} fit. The middle button pans; {{key:Shift}} and the middle button orbit. |
| {{ui:Drafting classic}} | A small ribbon and a command line, for people who type commands: WALL, DOOR, WINDOW, ROOF, U (undo), ZOOM, SECTION, PLOT, INSERT, HELP. Start typing anywhere and the letters go to the command line; {{key:Enter}} runs, {{key:Enter}} on nothing repeats the last command, {{key:Esc}} clears it. |
| {{ui:Viewer}} | For opening and checking models rather than drawing them: {{ui:File}}, {{ui:View}}, {{ui:Show}}, {{ui:Check}} and {{ui:Edit}} menus over a row of viewing tools (fit, top, front, side, section, measure, hide, isolate, see-through, show all, walk). The building tools wait in a {{ui:Model}} menu at the end. The same keys as BimCraft's own style. |

The menus and ribbons show once a model is open or you start a new building; before that, the panel on the left opens files and starts buildings.

:::tip
A key that belongs to BimCraft's own layout but not to the style you picked does nothing, and the status line says where its command went ("In BIM ribbon, Section is SC"). {{key:Delete}}, {{key:Esc}}, {{key:Enter}}, {{key:Tab}}, {{key:Space}} and the arrows always keep their meaning, and a running tool keeps its own keys: {{key:R}} and {{key:Tab}} while placing, {{key:W}} {{key:A}} {{key:S}} {{key:D}} while walking, {{key:H}} with the door and window tools, numbers while you type a length.
:::

## Make it your own

1. Pick the style closest to what you want, then click {{ui:Customize…}}.
2. On {{ui:Keys and menus}}, find a command (the search box helps). {{ui:Add a key}} and press the key, or two keys in a row for a two-letter key. Click a key to take it off.
3. With a menu bar or a ribbon, {{ui:Where it sits}} moves the command to another menu or panel, or to {{ui:Keys only}}.
4. On {{ui:Mouse}}, set the wheel, the buttons that pan the plan, and what each button does in the 3D views.
5. Give it a name and click {{ui:Save}}.

Saving a built-in style makes your own copy ("My Home design"); the built-in one never changes. A key given to two commands, or one that would run before a two-letter key could be typed, is shown in red and has to be fixed before you can save.

## Mouse

- **Mouse wheel**: {{ui:Scroll up zooms in}} (the start) or {{ui:Scroll up zooms out}}, and how fast. A trackpad pinch always zooms the way your fingers move.
- **Pan the plan and drawings with**: any of the left, middle and right buttons. {{key:Space}} and drag pans too; a left drag pans only where a click would not pick or draw.
- **Drag in the 3D views**: {{ui:Each view's own}} keeps BimCraft's buttons (left orbits, right pans). Or set each button to orbit, pan, zoom or nothing; {{key:Shift}} with the button swaps orbit and pan. A click without dragging still picks.

## Take it with you

{{ui:Export}} saves the style in use as a small `.ccinput.json` file; {{ui:Import…}} adds one, from another computer or from a colleague. An imported file can only point at BimCraft's own commands: anything else in it (a command this version doesn't have, a key that doesn't work here) is left out and counted in the status line. {{ui:Delete this style}} removes one of your own.

:::note
These settings stay in this browser. {{ui:Use this style in every app}} under the style cards makes every app open in its matching style (its own toolbar, a menu bar, a ribbon or Drafting classic; {{ui:Viewer}} is BimCraft's alone and has no match elsewhere), with the changes you made in each app; an app without a matching style keeps its own. Choosing another style in one app changes only that app.
:::

Related: [Keyboard and mouse](topic:keyboard) · [BimCraft settings](topic:settings) · [Coming from other BIM and drawing programs](topic:coming-from)
