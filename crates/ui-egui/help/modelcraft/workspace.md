---
title: The ModelCraft screen
summary: A tour of the toolbar, the panels on each side, the 3D view and the status bar, by their on-screen names.
section: start
order: 20
keywords: screen, layout, toolbar, panels, outliner, entity info, style, scenes, status bar, value box, units, interface, tour
---

From top to bottom: the app bar, the toolbar, the three columns (left panels, 3D view, right panels) and the status bar.

## The app bar

The slim bar at the very top belongs to every CadCraft app. It has the app name and version, {{ui:About ModelCraft}}, the screen-mode button and the {{ui:Paper}} / {{ui:Model}} switch.

- {{ui:Paper}} is the light theme and {{ui:Model}} the dark one. The 3D view changes its colors to match; your model does not change.
- The screen-mode button (it reads {{ui:Full screen}} at first) and {{key:Ctrl+Shift+F}} cycle through three modes: normal, full screen, and full screen with the bars hidden. With the bars hidden, point at the top edge of the screen (or tap it) to bring them back. {{key:Esc}} leaves full screen.

:::note
{{key:Ctrl+Shift+F}} uses the Control key on every computer, a Mac too. On iPhone, where browsers have no full screen, the button only hides and shows the bars.
:::

## The toolbar

The toolbar is split into labeled groups. Hover a button to see its name and key.

| Group | Buttons |
|---|---|
| File | {{ui:New}}, {{ui:Open}}, {{ui:Save}}, {{ui:Import}}, {{ui:Export}} (a menu of formats) |
| Draw | {{ui:Line}} ({{key:L}}), {{ui:Rectangle}} ({{key:R}}), {{ui:Circle}} ({{key:C}}), {{ui:Polygon}} ({{key:N}}) |
| Modify | {{ui:Select}} ({{key:Space}}), {{ui:Push/pull}} ({{key:P}}), {{ui:Move}} ({{key:M}}), {{ui:Rotate}} ({{key:Q}}), {{ui:Scale}} ({{key:S}}), {{ui:Eraser}} ({{key:E}}), {{ui:Tape}} ({{key:T}}) |
| Organize | {{ui:Component}} ({{key:G}}), {{ui:Group}} ({{key:Shift+G}}), {{ui:Explode}} |
| Edit | {{ui:Undo}} ({{key:Mod+Z}}), {{ui:Redo}} ({{key:Mod+Y}}), {{ui:Copy}} ({{key:Mod+C}}), {{ui:Paste}} ({{key:Mod+V}}), {{ui:More}} (a menu: cut, copy, paste, paste in place {{key:Mod+Shift+V}}, duplicate {{key:Mod+D}}, delete, select all, select none, and the key style) |
| Camera | {{ui:Orbit}} ({{key:O}}), {{ui:Pan}} ({{key:H}}), {{ui:Zoom}} ({{key:Z}}), {{ui:Extents}} ({{key:Shift+Z}}), {{ui:Views}} (a menu) |

The keys shown are the push-pull ones; with the Modeler style picked in [ModelCraft settings](topic:settings), the tooltips show those keys instead. The tool you are using is shown pressed. On a narrow screen the toolbar scrolls sideways.

The {{ui:Views}} menu has {{ui:Iso}}, {{ui:Top}}, {{ui:Front}}, {{ui:Right}}, {{ui:Back}}, {{ui:Left}} and {{ui:Bottom}}, each fitted to the model, and {{ui:Perspective}}, which switches between perspective and parallel projection.

## The 3D view

The middle of the screen. The red, green and blue lines are the axes: red and green lie on the ground, blue points up, and **north is the green axis**. Where they meet is the origin.

- When you start, a card titled "Start modeling" lists the first few keys. It goes away when you pick a drawing tool or click in the view.
- While a tool is working, the view shows **inference markers**: small colored shapes where the pointer snaps (a green circle at an endpoint, a cyan one at a midpoint, a red square on an edge, a blue diamond on a face). See [Inference and snapping](topic:inference).
- Selected things turn blue. A face seen from behind is blue-grey.
- When you open a group to edit it, everything outside it fades, and the axes move to the group's own axes.
- When a model has scenes, a row of scene tabs appears across the top of the view.
- Drop a file anywhere on the view to open or import it.

## The left panels

**{{ui:Outliner}}** lists the groups and components in the model, nested the way they are nested in the model. A component used more than once shows how many times, such as "Window (4)".

- Click a name to select it. Double-click to open it for editing.
- **Find by name** (shown once there are two or more groups) keeps only the rows whose name matches, with the groups they are in. {{key:Esc}} clears it.
- The **eye** at the end of a row hides or shows that group or component (as {{ui:Edit}} › {{ui:More}} › {{ui:Hide}} does). A tag icon means it is hidden by its [tag](topic:tags).
- **Rename in place:** click a row, then press {{key:F2}}, type, and press {{key:Enter}} ({{key:Esc}} keeps the old name).
- **Drag a row onto another group** to move it inside that group; drag it onto the {{ui:Outliner}} heading to move it to the top level. It stays where it is in the model, and keeps its color, tag and name. A group cannot go inside itself. Each move is one undo step.
- While you edit inside a group, a line reads "Editing: Model › …", and a {{ui:Close group}} button appears. It closes every open level at once.

**{{ui:Components}}** lists every component definition with how many copies are placed ("3×") and a {{ui:Place}} button that adds another copy where you click.

## The right panels

**{{ui:Entity info}}** describes what is selected. Its heading changes to say what that is:

| Heading | Shows | Buttons |
|---|---|---|
| Model (nothing selected) | Faces, edges, groups, components and how many are placed, guides, overall size | {{ui:Delete all guides}} (when there are guides) |
| Face | Area, number of holes | {{ui:Reverse face}} |
| Edge | Length | none |
| Group or Component | Name (type to rename), how many times it is placed, whether it is a closed solid, its volume, its size | {{ui:Edit}}, {{ui:Explode}}, {{ui:Make unique}} (a component placed more than once), {{ui:Make component}} (a group) |
| Guide point or guide line | nothing | {{ui:Delete}} |
| Several things | How many faces, edges, groups and guides, the total area of the faces | {{ui:Make component (G)}}, {{ui:Make group (Shift+G)}} |

**{{ui:Style}}** sets how the model is drawn: {{ui:Faces}} ({{ui:Shaded}}, {{ui:Hidden line}}, {{ui:Wireframe}}, {{ui:X-ray}}), {{ui:Show edges}}, and {{ui:Sun shadows}} with the date, time and place of the sun. See [Styles and sun shadows](topic:styles-shadows).

**{{ui:Scenes}}** saves views: {{ui:Add scene}}, the list of scenes, and {{ui:Previous}}, {{ui:Play}} and {{ui:Next}}. See [Save views as scenes](topic:scenes).

## The status bar

Along the bottom, from left to right:

1. **The tip**: what the tool wants next, or what the pointer is snapping to ("Endpoint", "On red axis").
2. **Messages**: what just happened ("Undid push/pull.", "Imported site.dxf: 1697 edges on the ground, in a group.") or why something did not work.
3. **The value box**: its label changes with the tool ({{ui:Length}}, {{ui:Dimensions}}, {{ui:Radius}}, {{ui:Sides}}, {{ui:Distance}}, {{ui:Angle}}, {{ui:Scale}}, {{ui:Offset}}). It shows the live size while you work, and takes what you type. See [Type exact sizes in the value box](topic:value-box).
4. **The key style**: {{ui:Keys: Push-pull}} or {{ui:Keys: Modeler}} switches the keyboard shortcuts and mouse buttons at once (the same as in [ModelCraft settings](topic:settings)).
5. **{{ui:Snap}} and {{ui:Collide}}**: snapping to points, edges and axes, and solids stopping where they touch. Crossed out = off. Click to switch. See [ModelCraft settings](topic:settings).
6. **The units picker**: the unit lengths are shown in. See [Get started](topic:getting-started#check-the-units-first).

## On a narrow screen

Below about 760 pixels wide (a phone, or a narrow window), the columns stack: the toolbar, then the 3D view, the status bar, the right panels and the left panels. Scroll the page to reach the panels.
