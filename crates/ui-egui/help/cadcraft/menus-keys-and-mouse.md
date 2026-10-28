---
title: Menus, keys and mouse
summary: Pick a menu style that matches the way you already work, give any command your own key or a key that types a command, move commands between ribbon tabs, and set what the wheel and the mouse buttons do.
section: reference
order: 6
keywords: menu style, menus, ribbon, toolbar, command line, keyboard shortcuts, hotkeys, keys, customize, remap, rebind, shortcuts, text keys, mouse, wheel, invert, zoom direction, middle button, right button, pan, export, import, switching, familiar, drafting classic
---

CadCraft's commands are the same whichever way you reach them, and the command line is always there with its aliases (`L`, `C`, `TR` …). A **menu style** only decides where the buttons sit on screen, what they are called there and which keys run them. Choose one in {{ui:Settings}} ({{key:Mod+,}}) › {{ui:CadCraft}} › {{ui:Menus, keys and mouse}}; the change is immediate, and this browser remembers it.

## The three styles

| Style | What you get |
|---|---|
| {{ui:CadCraft}} | CadCraft's own toolbar: File, Draw, Modify, Annotate, Parametric, Blocks, Sheets, Edit and View, each with a {{ui:More}} menu. The start. |
| {{ui:Drafting classic}} | A ribbon laid out the way classic 2D drafting programs lay theirs: a {{ui:File}} menu and quick buttons, then the tabs {{ui:Home}} (Draw, Modify, Annotation, Layers, Block, Properties, Utilities, Clipboard), {{ui:Insert}}, {{ui:Annotate}}, {{ui:Parametric}}, {{ui:View}}, {{ui:Manage}}, {{ui:Output}} and {{ui:3D}}. Type commands as always. The middle button pans; a right click is {{key:Enter}}, and dragging with the right button does nothing. |
| {{ui:Ribbon}} | A ribbon of plain task tabs: {{ui:Draw}}, {{ui:Modify}}, {{ui:Annotate}}, {{ui:Parametric}}, {{ui:Blocks}}, {{ui:Sheets}}, {{ui:View}}, {{ui:3D}} and {{ui:Tools}}, with a {{ui:File}} menu and quick buttons on the right. The middle and right buttons pan, as in CadCraft's own style. |

Every style keeps CadCraft's keys: {{key:Mod+Z}}, {{key:Mod+Y}}, {{key:Mod+S}}, {{key:Mod+Shift+S}}, {{key:Mod+O}}, {{key:Mod+P}}, {{key:Mod+C}}, {{key:Mod+X}}, {{key:Mod+V}}, {{key:Mod+D}}, {{key:Mod+A}}, and {{key:F3}}, {{key:F7}}, {{key:F8}}, {{key:F10}}, {{key:F11}}, {{key:F12}} for the drafting aids. Letters are never keys: CadCraft types them into the command line.

The ribbons also hold the Layers panel's tools and the drafting aids of the status bar. The {{ui:3D}} tab fills once the 3D tab under the drawing has been opened; a 3D tool picked on the Model tab opens the 3D tab first, and a 2D tool picked on the 3D tab goes back to the Model tab first.

:::tip
A key that belongs to CadCraft's own layout but that your style gave another command does nothing, and the status bar says where its command went ("In My keys, Ortho is Ctrl+Alt+O"). {{key:Delete}}, {{key:Esc}}, {{key:Enter}}, {{key:Tab}}, {{key:Space}} and the arrows always keep their meaning. While a command runs, or while letters are typed at the command line, keys belong to the command line.
:::

## Make it your own

1. Pick the style closest to what you want, then click {{ui:Customize…}}.
2. On {{ui:Keys and menus}}, find a command (the search box helps). {{ui:Add a key}} and press the key. Click a key to take it off.
3. With a ribbon, {{ui:Where it sits}} moves the command to another tab and panel, to the {{ui:File}} menu, or to {{ui:Keys only}}.
4. On {{ui:Mouse}}, set the wheel and the buttons that pan the drawing.
5. Give it a name and click {{ui:Save}}.

Saving a built-in style makes your own copy ("My Drafting classic"); the built-in one never changes. A key given to two commands is shown in red and has to be fixed before you can save.

## Keys that type a command

{{cmd:SHORTCUTS}} (View › More › {{ui:Keyboard shortcuts…}}) gives a key a whole command line, such as {{key:Ctrl+Alt+L}} for `LINE` or {{key:F9}} for `ZOOM E`. These keys belong to the style in use: the first time this version opens, the keys you made in an older version are carried into your own style "My CadCraft keys", once, and work as before. Exporting the style takes them along.

## Mouse

- **Mouse wheel**: {{ui:Scroll up zooms in}} (the start) or {{ui:Scroll up zooms out}}, and how fast. A trackpad pinch always zooms the way your fingers move.
- **Pan the drawing with**: the middle button, the right button, or both. A right click without dragging is always {{key:Enter}}. The left button always picks and draws in CadCraft, so ticking it changes nothing here.
- **Drag in the 3D views**: the 3D tab keeps its own buttons in every style (left orbits, middle and right pan).

## Take it with you

{{ui:Export}} saves the style in use as a small `.ccinput.json` file; {{ui:Import…}} adds one, from another computer or a colleague. An imported file can only point at CadCraft's own commands: anything else in it is left out and counted in the status bar. {{ui:Delete this style}} removes one of your own.

:::note
These settings stay in this browser. {{ui:Use this style in every app}} under the style cards makes every app open in its matching style (its own toolbar, a ribbon or Drafting classic), with the changes you made in each app. CadCraft has no menu-bar style: when another app's menu bar is chosen for every app, CadCraft keeps its own toolbar.
:::

Related: [Keyboard and mouse](topic:keyboard) · [CadCraft settings](topic:settings) · [Coming from other drafting software](topic:coming-from) · [Command aliases](topic:command-aliases)
