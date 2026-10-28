---
title: Menus, keys and mouse
summary: Pick a menu style that matches the way you already work, give any command your own key, move commands between ribbon tabs, and set what the wheel and the mouse buttons do.
section: reference
order: 6
keywords: menu style, menus, ribbon, toolbar, command line, keyboard shortcuts, hotkeys, keys, customize, remap, rebind, mouse, wheel, invert, zoom direction, middle button, right button, pan, orbit, export, import, switching, familiar, drafting classic, steel ribbon
---

DetailCraft's commands are the same whichever way you reach them. A **menu style** only decides where they sit on screen, what they are called there and which keys run them. Choose one in {{ui:Settings}} ({{key:Mod+,}}) › {{ui:DetailCraft}} › {{ui:Menus, keys and mouse}}; the change is immediate, and this browser remembers it.

## The three styles

| Style | What you get |
|---|---|
| {{ui:DetailCraft}} | DetailCraft's own toolbar (File, Place, Connect, Edit, Output) and its single-letter keys: {{key:V}} select, {{key:B}} beam, {{key:C}} column, {{key:R}} brace, {{key:P}} plate, {{key:F}} footing, {{key:S}} stair. The start. |
| {{ui:Drafting classic}} | For people who type commands: a small ribbon ({{ui:Home}}, {{ui:View}}, {{ui:Output}}) and a command line. Type BEAM, COLUMN, BRACE, PLATE, FOOTING, STAIR, ENDPLATE, BASEPLATE, SHEARTAB, SPLICE, GUSSET, U (undo), ZOOM, PLAN, 3D, DRAWINGS, NC or HELP and press {{key:Enter}}; {{key:Enter}} on nothing repeats the last command, {{key:Esc}} clears it. Start typing anywhere and the letters go to the command line. The middle button pans; in 3D, {{key:Shift}} and the middle button orbit. |
| {{ui:Steel ribbon}} | Tabs the way steel detailing programs arrange them: {{ui:Modeling}} (members, concrete, stairs), {{ui:Detailing}} (connections), {{ui:Edit}}, {{ui:Drawings}} (drawings and reports), {{ui:Fabrication}} (NC files, IFC) and {{ui:View}}, with a {{ui:File}} menu and quick buttons on the right. The single-letter keys stay. The middle button pans; in 3D the left button orbits. |

Every style keeps {{key:Mod+Z}}, {{key:Mod+Y}}, {{key:Mod+S}}, {{key:Mod+O}}, {{key:Mod+C}}, {{key:Mod+X}}, {{key:Mod+V}}, {{key:Mod+Shift+V}}, {{key:Mod+D}} and {{key:Mod+A}}. The ribbons also hold the view bar's {{ui:3D}}, {{ui:Plan}}, {{ui:Elevation}} and {{ui:Fit}}.

:::tip
A key that belongs to DetailCraft's own layout but that your style gave another command does nothing, and the status line says where its command went ("In My Steel ribbon, Column is Ctrl+Alt+C"). {{key:Delete}}, {{key:Esc}}, {{key:Enter}}, {{key:Tab}}, {{key:Space}} and the arrows always keep their meaning, and while a member is half placed or a copy follows the pointer, letters belong to DetailCraft.
:::

## Make it your own

1. Pick the style closest to what you want, then click {{ui:Customize…}}.
2. On {{ui:Keys and menus}}, find a command (the search box helps). {{ui:Add a key}} and press the key, or two keys in a row for a two-letter key. Click a key to take it off.
3. With a ribbon, {{ui:Where it sits}} moves the command to another tab and panel, to the {{ui:File}} menu, or to {{ui:Keys only}}.
4. On {{ui:Mouse}}, set the wheel, the buttons that pan the plan and elevations, and what each button does in the 3D view.
5. Give it a name and click {{ui:Save}}.

Saving a built-in style makes your own copy ("My Steel ribbon"); the built-in one never changes. A key given to two commands is shown in red and has to be fixed before you can save.

## Mouse

- **Mouse wheel**: {{ui:Scroll up zooms in}} (the start) or {{ui:Scroll up zooms out}}, and how fast. A trackpad pinch always zooms the way your fingers move.
- **Pan the plan and elevations with**: the middle button, the right button, or both. With the Select tool the left button always draws a selection window.
- **Drag in the 3D view**: {{ui:Each view's own}} keeps DetailCraft's buttons (left orbits, middle zooms, right pans). Or set each button to orbit, pan, zoom or nothing; {{key:Shift}} with a button that orbits or pans swaps the two.

## Take it with you

{{ui:Export}} saves the style in use as a small `.ccinput.json` file; {{ui:Import…}} adds one, from another computer or a colleague. An imported file can only point at DetailCraft's own commands: anything else in it is left out and counted in the status line. {{ui:Delete this style}} removes one of your own.

:::note
These settings stay in this browser. {{ui:Use this style in every app}} under the style cards makes every app open in its matching style (its own toolbar, a ribbon or Drafting classic), with the changes you made in each app. DetailCraft has no menu-bar style: when another app's menu bar is chosen for every app, DetailCraft keeps its own toolbar.
:::

Related: [Keyboard and mouse](topic:keyboard) · [DetailCraft settings](topic:settings) · [Coming from other detailing software](topic:coming-from)
