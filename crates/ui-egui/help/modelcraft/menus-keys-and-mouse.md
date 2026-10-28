---
title: Menus, keys and mouse
summary: Pick a menu style that matches the modeler you already know, give any command your own key, move commands between menus, and set what the wheel and the middle and right mouse buttons do.
section: reference
order: 6
keywords: menu style, menus, menu bar, toolbar, keyboard shortcuts, hotkeys, keys, key style, keymap, customize, remap, rebind, mouse, wheel, invert, zoom direction, middle button, right button, orbit, pan, export, import, switching, familiar, push-pull, modeler
---

ModelCraft's tools are the same whichever way you reach them. A **menu style** decides where they sit on screen, what they are called there and which keys run them. Choose one in {{ui:Settings}} ({{key:Mod+,}}) › {{ui:ModelCraft}} › {{ui:Menus, keys and mouse}}; the change is immediate, and this browser remembers it.

## The three styles

| Style | What you get |
|---|---|
| {{ui:ModelCraft}} | ModelCraft's own toolbar (File, Draw, Modify, Organize, Edit, Camera) and its single-letter keys: {{key:L}} line, {{key:R}} rectangle, {{key:P}} push/pull, {{key:M}} move, {{key:Q}} rotate, {{key:S}} scale, {{key:E}} eraser, {{key:O}} orbit. The start. |
| {{ui:Push-pull}} | A menu bar (File, Edit, View, Camera, Draw, Tools, Window) over a row of the main tools, with the same single-letter keys. |
| {{ui:Modeler}} | A header menu bar (File, Edit, View, Select, Add, Object, Mesh) and the keys of mesh modelers: {{key:W}} select, {{key:G}} grab, {{key:R}} rotate and {{key:S}} scale the selection at once ({{key:X}}, {{key:Y}} or {{key:Z}} locks an axis while it moves), {{key:E}} push/pull, {{key:B}} rectangle, {{key:X}} delete, {{key:A}} select all, {{key:Alt+A}} none, {{key:Shift+D}} duplicate, {{key:Mod+G}} group, {{key:H}} hide. The middle button orbits; {{key:Shift}} and the middle button pan, {{key:Mod}} and the middle button zoom; the right button cancels what a tool is doing. |

The key style you may know from {{ui:Keys}} at the bottom right of the view and from {{ui:Edit}} › {{ui:More}} is the same choice: picking the modeler keys there picks the {{ui:Modeler}} style, and picking the push-pull keys leaves it for the {{ui:ModelCraft}} style. If you had chosen the modeler keys in an older version, ModelCraft opens in the {{ui:Modeler}} style once, by itself.

:::tip
A key that belongs to the push-pull keys but that your style gives another command does nothing, and the status line says where its command went ("In Modeler, Push/pull is E"). {{key:Delete}}, {{key:Esc}}, {{key:Enter}}, {{key:Tab}}, {{key:Space}} and the arrows always keep their meaning, numbers always go to the value box, and while a tool is moving something its own keys win.
:::

## Make it your own

1. Pick the style closest to what you want, then click {{ui:Customize…}}.
2. On {{ui:Keys and menus}}, find a command (the search box helps). {{ui:Add a key}} and press the key. Click a key to take it off.
3. With a menu bar, {{ui:Where it sits}} moves the command to another menu, or to {{ui:Keys only}}.
4. On {{ui:Mouse}}, set the wheel and what the buttons do in the 3D view.
5. Give it a name and click {{ui:Save}}.

Saving a built-in style makes your own copy ("My Modeler"); the built-in one never changes. Your copy reads keys the way the style it started from does: one made from {{ui:Modeler}} keeps the grab, rotate and scale keys working at once.

## Mouse

- **Mouse wheel**: {{ui:Scroll up zooms in}} (the start) or {{ui:Scroll up zooms out}}, and how fast. A trackpad pinch always zooms the way your fingers move.
- **Drag in the 3D view**: {{ui:Each view's own}} keeps ModelCraft's buttons (the middle button orbits, the right button or {{key:Shift}} and the middle button pan). Or set the middle and right buttons to orbit, pan, zoom or nothing; {{key:Shift}} with the button swaps orbit and pan. The left button always belongs to the tool in use.
- **Trackpads**: {{ui:Settings}} › {{ui:ModelCraft}} › {{ui:Scrolling}} still decides whether two fingers orbit (trackpad) or zoom (mouse). See [Orbit, pan and zoom](topic:orbit-pan-zoom).

## Take it with you

{{ui:Export}} saves the style in use as a small `.ccinput.json` file; {{ui:Import…}} adds one, from another computer or from a colleague. An imported file can only point at ModelCraft's own commands: anything else in it is left out and counted in the status line. {{ui:Delete this style}} removes one of your own.

:::note
These settings stay in this browser. {{ui:Use this style in every app}} under the style cards makes every app open in its matching style: {{ui:Push-pull}} goes with the other apps' menu-bar styles. {{ui:Modeler}} has no counterpart in the other apps, so it stays a ModelCraft choice.
:::

Related: [Keys and mouse actions](topic:keyboard) · [ModelCraft settings](topic:settings) · [Coming from other modelers](topic:coming-from)
