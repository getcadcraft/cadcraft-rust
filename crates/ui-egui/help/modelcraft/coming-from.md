---
title: Coming from other modelers
summary: How push/pull modeling habits map to ModelCraft, key by key, and the differences that catch people out.
section: switching
order: 10
keywords: switching, migrate, familiar, differences, push pull modeler, skp, shortcuts, habits, compare, measurements box, outliner, entity info
---

If you have modeled in SketchUp, ModelCraft will feel familiar: the same idea of faces and push/pull, the same tool letters, a value box you type into without clicking, inference with red, green and blue axes, groups, components, an Outliner, Entity Info and scenes. This page lists what maps across directly and, more importantly, what does not.

## Habits that carry over

| You are used to | In ModelCraft |
|---|---|
| Press Space for Select, L Line, R Rectangle, C Circle | The same: {{key:Space}}, {{key:L}}, {{key:R}}, {{key:C}} |
| Press P Push/Pull, M Move, Q Rotate, S Scale, E Eraser, T Tape Measure | The same: {{key:P}}, {{key:M}}, {{key:Q}}, {{key:S}}, {{key:E}}, {{key:T}} |
| Press O Orbit, H Pan, Z Zoom, Shift+Z Zoom Extents | The same: {{key:O}}, {{key:H}}, {{key:Z}}, {{key:Shift+Z}} |
| Press G to make a component | The same: {{key:G}}. {{key:Shift+G}} makes a group |
| Type into the Measurements box without clicking it | The same, in the value box at the bottom. Press {{key:Enter}} |
| Type `4m,3m` for a rectangle, `12s` for sides | The same |
| Type `3x` or `/3` after a Move copy | The same; `*3` also works |
| Type `[x,y,z]` or `<dx,dy,dz>` for a point or offset | The same |
| Type a bare number in an architectural-inch model and get inches | The same |
| Middle-drag to orbit, Shift+middle-drag to pan, wheel to zoom at the cursor | The same |
| Double-click with Push/Pull to repeat the distance | The same |
| Arrow keys to lock red, green, blue, and parallel | The same: {{key:Right}} red, {{key:Left}} green, {{key:Up}} blue, {{key:Down}} parallel |
| Hold Shift to lock an inference | The same |
| Ctrl (Option on a Mac) to copy while moving | {{key:Ctrl}}, {{key:Mod}} or {{key:Alt}}, on any computer |
| Shift to toggle, Ctrl to add, Shift+Ctrl to remove from the selection | The same keys; on a Mac use {{key:Mod}} where it says Ctrl |
| Left-to-right window, right-to-left crossing | The same |
| A Dimension tool to label lengths | {{key:D}}, and more: type a new length into a dimension and the model resizes to it. See [Dimension and resize](topic:dimensions) |
| Double-click a face for face plus edges; triple-click for all connected | The same |
| Double-click a group to edit it; Esc to close | The same |
| Draw a line across a face to split it; erase an edge between coplanar faces to heal them | The same |
| Push a window rectangle through a wall to cut it | The same |
| Back faces in blue-grey; Reverse Faces | The same color; {{ui:Reverse face}} in {{ui:Entity info}} |
| Scenes as tabs over the view, Page Up and Page Down | The same, with {{ui:Play}} in the {{ui:Scenes}} panel |
| Shadows from date, time, latitude and longitude, with north on green | The same, in the {{ui:Style}} panel |

## Differences that catch people out

### Saving is a download

There is no Save that overwrites the file you opened, no Save As dialog with a name, no autosave and no backup file. {{ui:Save}} ({{key:Mod+S}}) downloads a fresh `.modelcraft` file every time, usually to your downloads folder. Save often, and rename or move the files yourself. See [Open, save, import and export](topic:files).

### It imports .skp files, but does not save them

{{ui:Import}} reads SketchUp files with ModelCraft's own reader, in the page: groups, components, colors, textures on faces, tags and soft edges come across. See [File formats in detail](topic:formats#skp-files). ModelCraft cannot save `.skp`: to go back, export Collada from ModelCraft and import it in SketchUp.

### No right-click menu

Right-drag pans; a right-click does nothing. Commands you would find in the context menu are in {{ui:Entity info}} ({{ui:Reverse face}}, {{ui:Edit}}, {{ui:Explode}}, {{ui:Make unique}}, {{ui:Make component}}) and in the toolbar's {{ui:Organize}} group.

### Units live in the status bar

There is no Model Info or template choice. The unit picker is at the bottom right. A new model starts in feet and inches in the US and in millimeters elsewhere, from your browser's language setting. Decimals always take a period: `2.5m`, never `2,5m`, because the comma always separates values.

### Paint has its own materials

{{ui:Paint}} ({{key:B}}) paints solid colors and the materials in the {{ui:Materials}} section: brick, siding, roofing, wood, stone and more, drawn by ModelCraft. You can't load your own image as a material yet; textures that come in with imported files are kept. See [Paint with colors and materials](topic:paint).

### Scale is uniform, without grips

Scale does not show a box with grips. Click the point that stays put, a reference point, then the new size or a typed factor. It always scales evenly in all directions. To stretch in one direction, push/pull or move a face, retype a group's width, depth or height in {{ui:Entity info}}, or [resize with a dimension](topic:dimensions).

### Rotate picks its plane from where you click

There is no protractor that sticks to faces as you hover with arrow keys to pick the plane. Click the center on a face to turn in that face's plane, or on the ground to turn about the vertical. The arrow keys orbit the view before the first click.

### Inference from points you pass over

ModelCraft snaps to axis directions from **your start point**, and to points, edges and faces under the pointer. It does not track a direction from a point you merely hovered (the dotted "from point" inference). To line up with a far corner, lock the axis with an arrow key and hover that corner: the locked point slides to line up with it. Or place a [guide](topic:guides-measure).

### Parallel uses the edge you passed over before starting

The parallel and perpendicular inference uses the last edge the pointer crossed **before the first click**, not edges you hover while drawing. See [Inference and snapping](topic:inference#parallel-and-perpendicular).

### Copies of a group are linked

Copying a group with {{ui:Move}} makes a second placement of the same group, so editing one changes both, as with components. SketchUp makes groups independent. See [Troubleshooting](topic:troubleshooting#editing-one-group-changed-another).

### Space does not pan

{{key:Space}} picks {{ui:Select}}, as in SketchUp. In some other web apps holding Space pans; here it doesn't. Right-drag pans instead.

### Two-finger scroll zooms

On a trackpad, set {{ui:Settings}} › {{ui:ModelCraft}} › Scrolling to Trackpad: two fingers orbit, {{key:Shift}} and two fingers pan, a pinch zooms. See [Orbit, pan and zoom](topic:orbit-pan-zoom#on-a-trackpad).

If your hands know Blender's keys rather than SketchUp's, pick the {{ui:Modeler}} menu style: {{key:G}}, {{key:R}} and {{key:S}} act on the selection at once, {{key:E}} extrudes (push/pull), {{key:Shift+D}} duplicates, and a header menu bar (File, Edit, View, Select, Add, Object, Mesh) replaces the toolbar. Coming from SketchUp, the {{ui:Push-pull}} style gives a menu bar like its own over the same tool letters. See [Menus, keys and mouse](topic:menus-keys-and-mouse).

### Solids pushed into each other do not cut

Push/pull cuts through the face behind it in the same object, but two separate solids pushed into each other don't intersect. There is no Intersect Faces and no Solid Tools.

### What is missing

Freehand, Trim, Split and Outer Shell, your own image materials, extensions, and a layout tool for sheets are not in this build. [Not built yet](topic:not-built-yet) lists what to do instead.

## Shortcuts the browser keeps

ModelCraft runs in a browser, so a few shortcuts belong to the browser, not the app. {{key:Mod+N}} opens a browser window (use {{ui:New}}), and {{key:Mod+W}} closes the tab. If you have unsaved changes, the browser asks before closing. See [Keys, mice and trackpads](common:keys#shortcuts-the-browser-keeps).
