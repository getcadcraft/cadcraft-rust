---
title: The CadCraft screen
summary: A tour of every bar, panel and tab in the drafting app, by the name it has on screen.
section: start
order: 20
keywords: screen, interface, toolbar, ribbon, panels, command line, status bar, tabs, layout tabs, properties, layers panel, tour
---

The screen has seven parts. From top to bottom: the app bar, the toolbar, the drawing area with a panel on each side, the tabs under the drawing, the command line and the status bar.

## The app bar

The thin bar at the very top belongs to the site, not to the drawing. It shows the CadCraft mark, the app name, which version you are running, a link to the app's page, a {{ui:Help}} button (or {{key:F1}}), a {{ui:Save for offline}} button, a {{ui:Full screen}} button and two theme buttons, {{ui:Paper}} and {{ui:Model}}.

The {{ui:Settings}} button (gear) holds CadCraft's own settings and the ones every app shares: see [CadCraft settings](topic:settings).

{{ui:Save for offline}} keeps a copy of CadCraft, including its DWG reader, in this browser so it opens with no connection. Its label shows the state: "Saving N of M", "Offline ready", "Could not save" or "Offline not available". See [Working offline](common:offline).

:::note
The {{ui:Paper}} and {{ui:Model}} buttons in the app bar only change the colors of the screen: {{ui:Paper}} is a light theme, {{ui:Model}} a dark one. They have nothing to do with the {{ui:Model}} tab under the drawing, which switches between the drawing and its sheets.
:::

The {{ui:Full screen}} button, or {{key:Ctrl+Shift+F}} (the Control key on every computer, Mac included), cycles through three screen modes: normal, full screen, and full screen with the bars hidden. With the bars hidden, move the pointer to the top edge of the screen to bring them back.

## The toolbar

Under the app bar is the toolbar. Every button runs a command, exactly as if you had typed it, so the command line asks the same questions either way. Hover a button to see its command alias. The groups are:

| Group | Buttons |
|---|---|
| File | {{ui:New}}, {{ui:Open}}, {{ui:Save}}, {{ui:Save as}}, {{ui:Plot}} |
| Draw | {{ui:Line}}, {{ui:Polyline}}, {{ui:Circle}}, {{ui:Arc}}, {{ui:Rectangle}}, {{ui:Polygon}}, {{ui:Ellipse}}, {{ui:Text}}, {{ui:Paragraph}}, {{ui:Hatch}}, and {{ui:More}}: {{ui:Point}}, {{ui:Construction line}} and {{ui:Ray}} (see [Construction lines and rays](topic:construction-lines)) |
| Modify | {{ui:Move}}, {{ui:Copy}}, {{ui:Rotate}}, {{ui:Scale}}, {{ui:Mirror}}, {{ui:Stretch}}, {{ui:Offset}}, {{ui:Trim}}, {{ui:Extend}}, {{ui:Fillet}}, {{ui:Chamfer}}, {{ui:Explode}}, {{ui:Erase}}, and {{ui:More}}: {{ui:Array}}, {{ui:Join}}, {{ui:Match properties}}, {{ui:Break}} and {{ui:Lengthen}} |
| Annotate | {{ui:Linear}}, {{ui:Aligned}}, {{ui:Angle}}, {{ui:Radius}}, {{ui:Diameter}} (dimensions), {{ui:Leader}} (multileader), and {{ui:More}}: {{ui:Continue dimension}}, {{ui:Baseline dimension}} and {{ui:Revision cloud}} |
| Parametric | {{ui:Auto}}, {{ui:Constrain}}, {{ui:Dimension}}, {{ui:Parameters}}, and {{ui:More}} with each constraint (see [Constrain a drawing](topic:constraints)) |
| Blocks | {{ui:Make}}, {{ui:Insert}}, {{ui:Library}}, and {{ui:More}}: {{ui:Edit block}} and {{ui:Define attribute}} |
| Sheets | {{ui:New sheet}}, {{ui:Viewport}} |
| Edit | {{ui:Undo}}, {{ui:Redo}}, {{ui:Copy}} (to the clipboard), {{ui:Paste}}, and {{ui:More}}: a menu with {{ui:Cut}}, {{ui:Copy}}, {{ui:Paste}}, {{ui:Paste in place}}, {{ui:Duplicate and move}}, {{ui:Delete}}, {{ui:Select all}}, {{ui:Select none}}, {{ui:Quick select}}, {{ui:Select similar}} and {{ui:Find and replace}}, each with its key |
| View | {{ui:Extents}}, {{ui:Window}}, {{ui:Previous}}, and {{ui:More}}: {{ui:Measure distance}}, {{ui:Area}}, {{ui:Measure (radius, angle …)}}, and the split screen: {{ui:Split side by side}}, {{ui:Split stacked}}, {{ui:Four panes}}, {{ui:One pane}} (see [Split the screen](topic:split-screen)), and {{ui:Turn view 90° left}}, {{ui:Turn view 90° right}}, {{ui:Back to world}} (see [Turn the view](topic:ucs-and-turned-view)) |

{{ui:Copy}} in the {{ui:Modify}} group is the {{cmd:COPY}} command (base point, then where each copy goes); {{ui:Copy}} in the {{ui:Edit}} group puts objects on the clipboard for {{ui:Paste}}. See [Copy, paste and duplicate](topic:copy-paste). The button of the command that is running stays highlighted until the command ends. Tooltips name the Windows shortcuts ("Ctrl+O"); on a Mac, {{key:Mod+O}} works too.

On a short window (a laptop with its display scaled to 125 % or more, for example) the toolbar never takes the drawing's place: when the drawing would get too little height, the buttons drop the words under their icons and the group names (hover a button for its name), and if that is still not enough the toolbar becomes one row that scrolls sideways. A taller window brings the words back.

## The left panel

The panel on the left has three sections.

- **{{ui:Layers}}.** Every layer in the drawing, with a filled dot on the current one. Click a name to make that layer current. Each row has a color swatch (click it to choose a color) and four buttons: on/off (eye), freeze (snowflake), lock (padlock) and plot (printer). {{ui:New layer}} opens a box to add one. See [Organize a drawing with layers](topic:layers).
- **{{ui:Blocks}}.** The blocks defined in the drawing, how many times each is used, and an {{ui:Insert}} button for each. See [Make and insert blocks](topic:blocks).
- **{{ui:Drawing}}.** Facts about the open file: name (with "(changed)" when there are unsaved edits), format, size, units, number of objects, layers, blocks and sheets. After you open a file, an "About this file" box lists anything CadCraft could not show or had to convert.

## The drawing area

The middle of the screen is the drawing. The pointer is a crosshair. When CadCraft wants a point, a small marker and a label ("Endpoint", "Midpoint") show where an object snap will land. When it wants objects, the crosshair has a pick box and objects under it light up.

When the drawing is empty, a "Start drawing" card shows a few first commands. It goes away as soon as you run a command or open a file. You can drop a DWG or DXF file anywhere on this area to open it.

## The tabs under the drawing

Below the drawing are the tabs: {{ui:Model}} first, then {{ui:3D}}, then one tab per sheet (layout), then a {{ui:+}} button.

- {{ui:Model}} is the drawing itself, at full size, in drawing units.
- {{ui:3D}} shows the drawing lying on the ground and the solids made from it. While it shows, the toolbar has a {{ui:3D}} group in place of the drawing groups. See [Model in 3D](topic:3d-solids).
- Each sheet tab is a sheet of paper with a title block and viewports onto the model. What you draw there is in paper units (millimeters or inches). Double-click inside a viewport to work on the model through it (see [Work on the model through a viewport](topic:sheets-and-viewports#work-on-the-model-through-a-viewport)). A long sheet name is cut short on its tab; hover the tab for the whole name. With many sheets, {{ui:Settings}} › {{ui:CadCraft}} › {{ui:Sheet tabs}} › {{ui:Sheet numbers only}} shows just each sheet's number (A-101) on its tab.
- {{ui:+}} makes a new sheet with a border, a title block and one viewport. It is the same as the {{cmd:LAYOUT}} command.
- Double-click a sheet tab to rename it. With a tab focused, {{key:Left}} and {{key:Right}} move between tabs.

Each tab keeps its own zoom and pan. See [Set up a sheet](topic:sheets-and-viewports).

## The right panel: Properties

The panel on the right changes with what is selected.

- **Nothing selected, on the {{ui:Model}} tab:** the heading reads {{ui:Current settings}}. Here you set the {{ui:Layer}}, {{ui:Colour}} and {{ui:Text height}} for new objects, the drawing {{ui:Units}}, and the sizes for new dimensions ({{ui:Text height}}, {{ui:Arrow size}}, {{ui:Decimals}}, {{ui:Overall scale}}).
- **Nothing selected, on a sheet:** the heading reads "Sheet:" and the sheet name. You can rename the sheet, change its {{ui:Paper}} and {{ui:Orientation}}, {{ui:Plot this sheet}} or {{ui:Delete this sheet}}. The settings for new objects follow below.
- **One object selected:** its type as the heading (Line, Circle, Text, Dimension, Viewport …), then its layer, color and geometry. Type a new value and press {{key:Enter}} to change it.
- **Several objects selected:** how many, and their common {{ui:Layer}} and {{ui:Colour}}. Changing either changes them all. "Varies" means they differ.

See [Edit with grips and Properties](topic:grips-and-properties).

Below it, the {{ui:Parameters}} panel opens when you choose {{ui:Parameters}} or type {{cmd:PARAMETERS}}: every parameter and dimensional constraint in one table. See [Use parameters and equations](topic:parameters).

## The command line

At the bottom is the command line: the history of what you did, and under it the prompt.

- The prompt label reads `Command:` when nothing is running. While a command runs, it shows the question, its options in square brackets and the default in angle brackets, for example `Specify radius of circle [Diameter] <500>:`.
- As you type a command name, a hint to the right suggests matching commands. Press {{key:Tab}} to accept the first one. "No such command" means nothing matches.
- {{key:Up}} and {{key:Down}} bring back lines you typed before.
- The history keeps the last 400 lines. Messages from commands, measurements from {{cmd:DIST}} and {{cmd:LIST}}, and save and plot reports appear here.

## The status bar

The very bottom row is the status bar.

- **Coordinates** on the left: the X and Y of the crosshair. While a command has a base point, it also shows the distance and angle from it, for example `Δ 1200 < 90°`.
- **Messages** in the middle, such as "Ortho on" or "Current layer: Walls". They fade after a few seconds.
- **{{ui:Grid}}**, **{{ui:Ortho}}**, **{{ui:Polar}}**, **{{ui:Snap}}**, **{{ui:Track}}**, **{{ui:Dyn}}** and **{{ui:LWT}}** on the right. Each is a toggle; a pressed button is on. {{ui:LWT}} shows lineweights on screen (see [Line weights and linetypes](topic:units-papers-scales#line-weights-and-linetypes)). The small arrow next to {{ui:Snap}} opens {{ui:Object snaps}}, where you tick which snaps are active. See [Snap to points on objects](topic:object-snaps).

## The Plot to PDF dialog

{{ui:Plot}} (or {{key:Mod+P}}) opens the {{ui:Plot to PDF}} dialog, with the settings on the left and a live preview on the right. See [Plot to PDF](topic:plot-pdf).

## The help

{{key:F1}} or the {{ui:Help}} button opens this help as a side panel; the drawing stays usable beside it. It works with no connection. {{key:F1}} again, or {{key:Esc}} inside the panel, closes it. See [Using this help](common:help).
