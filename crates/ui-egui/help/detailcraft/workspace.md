---
title: A tour of the screen
summary: Every bar, panel and tab in DetailCraft by its on-screen name, and what each one is for.
section: start
order: 20
keywords: screen, layout, toolbar, panels, properties, status bar, reports, interface, ui, tabs
---

DetailCraft fills the browser window below the app bar. From top to bottom: the toolbar, then three columns (the {{ui:Model}} panel, the view, the {{ui:Properties}} panel), then the report tabs, then the status bar.

## The app bar

The slim bar at the very top belongs to the CadCraft site, not to the model. It shows the app name and version, a link to {{ui:About DetailCraft}}, {{ui:Help}} (or {{key:F1}}) for this help, {{ui:Save for offline}}, the screen-mode button and the {{ui:Paper}} / {{ui:Model}} switch.

- {{ui:Save for offline}} keeps DetailCraft, its 3D view and this help in your browser so it opens with no connection. It reads {{ui:Offline ready}} once saved; click it then for {{ui:Save again (get the newest version)}} or {{ui:Remove the offline copy}}. See [Working offline](common:offline).
- {{ui:Paper}} gives a light screen and {{ui:Model}} a dark one. The 3D view follows at once; nothing in your model changes.
- The screen-mode button and {{key:Ctrl+Shift+F}} cycle through normal, full screen, and full screen with the bars hidden. {{key:Ctrl+Shift+F}} uses the Control key on every computer, a Mac included. With the bars hidden, point at the top edge of the screen (or tap it) to bring them back. {{key:Esc}} leaves full screen.

## The toolbar

The toolbar has five groups. Hover over a button to see what it does and its key.

| Group | Buttons | What they do |
|---|---|---|
| {{ui:File}} | {{ui:New}}, {{ui:Open}}, {{ui:Import IFC}}, {{ui:Save}}, {{ui:Sample}} | Start a model, open a saved `.detailcraft.json` or an IFC file, save one, or open the sample frame |
| {{ui:Place}} | {{ui:Select}}, {{ui:Beam}}, {{ui:Column}}, {{ui:Brace}}, {{ui:Plate}}, {{ui:Footing}}, {{ui:Stair}} | The tool the next click in the view uses |
| {{ui:Connect}} | {{ui:End plates}}, {{ui:Base plate}}, and in {{ui:More}}: {{ui:Shear tabs}}, {{ui:Splice}}, {{ui:Gusset}}, {{ui:Similar}} | Add a connection to the selected beam, column or brace, or copy the selected connection to every joint like it |
| {{ui:Edit}} | {{ui:Undo}}, {{ui:Redo}}, {{ui:Copy}}, {{ui:Paste}}, {{ui:More}} | Step back and forward through edits; copy the selection and paste it. {{ui:More}} opens a menu with {{ui:Cut}}, {{ui:Copy}}, {{ui:Paste}}, {{ui:Paste in place}}, {{ui:Duplicate}}, {{ui:Delete}}, {{ui:Select all}} and {{ui:Select none}}, each with its key |
| {{ui:Output}} | {{ui:IFC}}, {{ui:NC files}}, {{ui:BOM}}, {{ui:Shop drawing}}, {{ui:Drawings}}, and in {{ui:More}}: {{ui:Bending}}, {{ui:Weight report by phase}}, {{ui:KISS file for fabrication software}} | Download an export, or open the drawing sheets |

Buttons and menu items that cannot act right now are grayed out, and hovering over one says why. {{ui:End plates}} needs a selected beam, {{ui:Base plate}} a selected column, {{ui:Similar}} a selected connection, {{ui:Shop drawing}} a selected I-shape or plate part, {{ui:Copy}} a selected member or footing, and {{ui:Paste}} something copied. See [Copy, paste and duplicate](topic:copy-paste).

## The Model panel (left)

- {{ui:Model}}: the model {{ui:Name}}. It names every file you save or export.
- {{ui:Grids}}: the grid lines and their spacings, and {{ui:Apply grids}}. See [Set up grids and levels](topic:grids-and-levels).
- {{ui:Levels}}: one row per level (name and elevation in mm), {{ui:Add level}} and {{ui:Apply levels}}.
- {{ui:Custom profile}}: make your own parametric section. Closed until you open it.
- {{ui:Part marks}}: every part mark in the model with its profile, length and count. Click a mark to select the first part with it.

{{ui:Grids}}, {{ui:Levels}} and {{ui:Custom profile}} fold away: click their headings.

## The view (middle)

The view shows the model in 3D. Above it, the view bar has three buttons and a list:

- {{ui:3D}}: turn around the model freely.
- {{ui:Plan}}: look straight down at one level. A list of levels appears beside the buttons. Anything more than 1.5 m above the level is cut away so you can see and click the level clearly.
- {{ui:Elevation}}: look straight at one lettered grid line (Grid A, Grid B …). A list of lettered grids appears. Only the steel within 1 m of that line is shown.
- The fit button at the right end of the view bar zooms to show the whole model.

Small dots mark every grid intersection at every level. While a placing tool is on, the dot nearest the pointer lights up and the status bar names it, for example "A/2 at Level 1". Colors tell the member types apart: columns, beams, braces and plates each have their own color, bolts and rods another, footings are see-through and rebar is drawn as tubes. Holes show as dark plugs; copes are cut.

How to turn, pan and zoom: [Move around the 3D view](topic:navigate-3d).

When the model is empty, the view shows a short "Start a model" list. You can also drop a saved model file anywhere on the view to open it.

## The Properties panel (right)

What this panel shows depends on what you are doing, and its heading says which: "Model", "Place a beam", "Beam b2", "End plate connection", "Base plate", "Footing F1", "3 selected" or "Paste".

- **Nothing selected:** a summary of the model (members, connections, footings, parts, marks, bolts, steel and rebar weight, grids, levels).
- **A placing tool on:** the options for the next piece (profile, material, position, rotation, and for columns {{ui:Up to}}), a summary of the chosen profile, and an {{ui:Add by grid}} form for placing without clicking in the view.
- **A member selected:** its name, profile, material, position, rotation, the coordinates of its two ends, its marks, cut length and weight, its holes and copes, and buttons for connections, its shop drawing and {{ui:Delete}}.
- **A connection selected:** every plate, bolt and weld setting, the bolts and weld it makes, its warnings, and {{ui:Apply to similar}}.
- **A footing selected:** its position and size, the bottom mat, the starter bars, and the bar marks it uses.
- **Several things selected:** how many members of each type, connections and footings, their steel weight, and {{ui:Copy}}, {{ui:Duplicate}} and {{ui:Delete}}.
- **Pasting:** what is on the clipboard, and forms to paste by a typed offset or onto a grid intersection you pick from lists.

Number fields take effect when you press {{key:Enter}} or leave the field. Each change is one step you can undo.

## The report tabs (bottom)

- {{ui:Problems}}: everything DetailCraft could not build or wants you to check, with a {{ui:Select}} button on each line. The tab shows the count in brackets.
- {{ui:Bill of materials}}: parts, assemblies, and bolts and anchor rods, with weights and a {{ui:Download CSV}} link. Click a part mark to select that part.
- {{ui:Bending schedule}}: every bar mark in every footing, with a {{ui:Download CSV}} link.

With a tab focused, {{key:Left}} and {{key:Right}} move between tabs.

## The status bar

The bar along the bottom has three parts: the grid intersection under the pointer while placing, the latest message (what just happened, or why something was refused), and running totals of parts, bolts, steel weight and rebar weight. When an action seems to do nothing, read the message here first.

The browser tab title starts with a dot (•) while the model has changes you have not saved.

## On a narrow screen

Below about 760 pixels wide, the columns stack: toolbar, view, status bar, {{ui:Properties}}, report tabs, then the {{ui:Model}} panel last. Scroll the page to reach them. DetailCraft is laid out for a desktop or laptop screen; a phone works for looking, not for detailing.
