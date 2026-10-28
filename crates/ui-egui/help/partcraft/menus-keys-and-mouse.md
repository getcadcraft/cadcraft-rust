---
title: Menus, keys and mouse
summary: Pick a menu style that matches the way you already work, give any command your own key, move commands between ribbon tabs, and set what the wheel and each mouse button do in the 3D view.
section: reference
order: 6
keywords: menu style, menus, ribbon, toolbar, feature tree, keyboard shortcuts, hotkeys, keys, customize, remap, rebind, mouse, wheel, invert, zoom direction, middle button, orbit, pan, export, import, switching, familiar
---

PartCraft's commands are the same whichever way you reach them. A **menu style** only decides where they sit on screen, what they are called there and which keys run them. Choose one in {{ui:Settings}} ({{key:Mod+,}}) › {{ui:PartCraft}} › {{ui:Menus, keys and mouse}}; the change is immediate, and this browser remembers it.

## The two styles

| Style | What you get |
|---|---|
| {{ui:PartCraft}} | PartCraft's own toolbar: the workspace switcher, File, Sketch, Features, Drawing, Assembly, Edit and View. Keys {{key:S}} sketch, {{key:F}} fit, {{key:M}} measure, {{key:X}} section view. The start. |
| {{ui:Feature tree}} | A ribbon laid out the way feature-based mechanical CAD programs lay theirs: a {{ui:File}} menu (with every export), quick buttons, and the tabs {{ui:Features}}, {{ui:Sketch}}, {{ui:Surfaces}}, {{ui:Evaluate}}, {{ui:Drawing}}, {{ui:Assembly}}, {{ui:Studies}} (the workspaces and their tools) and {{ui:View}}. Extra keys: {{key:E}} extrude, {{key:H}} hole, and {{key:V}} then a letter for a view: {{key:V}} {{key:F}} front, {{key:V}} {{key:B}} back, {{key:V}} {{key:T}} top, {{key:V}} {{key:O}} bottom, {{key:V}} {{key:R}} right, {{key:V}} {{key:L}} left, {{key:V}} {{key:I}} isometric, {{key:V}} {{key:N}} normal to. In the 3D view the middle button orbits, {{key:Shift}} and the middle button pan, the right button pans and the left button only picks. |

Every style keeps {{key:Mod+Z}}, {{key:Mod+Y}}, {{key:Mod+S}}, {{key:Mod+O}}, {{key:Mod+C}}, {{key:Mod+X}}, {{key:Mod+V}}, {{key:Mod+D}} and {{key:Mod+A}}.

A tool of another workspace switches to it first; a Design tool picked in another workspace goes back to Design first. Sketch tools wait for a sketch, the drawing tools for the drawing sheet, the assembly tools for an assembly: the status bar says what to do first.

:::tip
Inside a sketch, on the drawing sheet, in an assembly and in the other workspaces, letters keep their own meaning there ({{key:L}} line, {{key:R}} rectangle, {{key:J}} joint …) in every style. {{key:Delete}}, {{key:Esc}}, {{key:Enter}}, {{key:Tab}}, {{key:Space}} and the arrows always keep theirs. A key of PartCraft's own layout that your style gave another command does nothing, and the status bar says where its command went.
:::

## Make it your own

1. Pick the style closest to what you want, then click {{ui:Customize…}}.
2. On {{ui:Keys and menus}}, find a command (the search box helps). {{ui:Add a key}} and press the key, or two keys in a row. Click a key to take it off.
3. With the ribbon, {{ui:Where it sits}} moves the command to another tab and panel, to the {{ui:File}} menu, or to {{ui:Keys only}}.
4. On {{ui:Mouse}}, set the wheel, the buttons that pan a sketch, and what each button does in the 3D view.
5. Give it a name and click {{ui:Save}}.

Saving a built-in style makes your own copy ("My Feature tree"); the built-in one never changes. A key given to two commands, or one that would run before a two-letter key could be typed, is shown in red and has to be fixed before you can save.

## Mouse

- **Mouse wheel**: {{ui:Scroll up zooms in}} (the start) or {{ui:Scroll up zooms out}}, and how fast. A trackpad pinch always zooms the way your fingers move.
- **Pan the sketch with**: the middle button, the right button, or both. A right click in a sketch still ends the tool's step.
- **Drag in the 3D view**: {{ui:Each view's own}} keeps PartCraft's setting ({{ui:Settings}} › {{ui:PartCraft}}: left orbits, or middle orbits). Or set each button to orbit, pan, zoom or nothing; {{key:Shift}} with the orbit button pans. A click without dragging still picks.

## Take it with you

{{ui:Export}} saves the style in use as a small `.ccinput.json` file; {{ui:Import…}} adds one, from another computer or a colleague. An imported file can only point at PartCraft's own commands: anything else in it is left out and counted in the status bar. {{ui:Delete this style}} removes one of your own.

:::note
These settings stay in this browser. {{ui:Use this style in every app}} under the style cards makes every app open in its matching style (its own toolbar, or a ribbon), with the changes you made in each app. Choosing another style in one app changes only that app.
:::

Related: [Keys and mouse](topic:keyboard) · [PartCraft settings](topic:settings) · [Coming from another parametric modeler](topic:coming-from)
