---
title: Tool list
summary: Every ModelCraft tool with its key, what each click does, and what you can type into the value box while it works.
section: reference
order: 20
keywords: tools, toolbar, commands, list, line, rectangle, circle, polygon, offset, arc, curve, spline, follow me, sweep, 2 point arc, arc from center, select, push pull, move, rotate, scale, eraser, tape, dimension, orbit, pan, zoom, place component
---

The status bar's tip always says what the current tool wants next. {{key:Esc}} cancels what a tool is in the middle of.

## Drawing tools

| Tool | Key | Clicks | Type in the value box |
|---|---|---|---|
| {{ui:Line}} | {{key:L}} | Start, then each next point. Stops when a face closes, or on {{key:Esc}} | A length toward the pointer; `[x,y,z]` a point; `<dx,dy,dz>` an offset ({{ui:Length}}) |
| {{ui:Rectangle}} | {{key:R}} | First corner (ground or a face), opposite corner | Width and depth: `4m,3m` ({{ui:Dimensions}}) |
| {{ui:Circle}} | {{key:C}} | Center (ground or a face), a point on the edge | Before the first click: sides, `24s` or `24` ({{ui:Sides}}). After: the radius ({{ui:Radius}}) |
| {{ui:Polygon}} | {{key:N}} | Center, a corner | Same as Circle; starts with 6 sides |
| {{ui:Arc}} | {{key:A}} | Start, end, then the bulge | The chord's length after the start ({{ui:Length}}); the bulge after the end ({{ui:Bulge}}); `12s` for segments |
| {{ui:Protractor}} | — | Center, 0° point, then the angle | The angle in degrees ({{ui:Angle}}) |
| {{ui:Follow me}} | — | Select a path, then click the face (or click the face, then an edge of the path) | — |
| {{ui:Paint}} | {{key:B}} | A face, group or component | — ({{key:Ctrl}} connected, {{key:Shift}} all of that color, {{key:Alt}} pick up a color) |
| {{ui:Offset}} | {{key:F}} | A face (or select a face or edges first), then the side and distance | The distance ({{ui:Distance}}); into the face is inward |
| {{ui:Curve}} | — | Points the curve passes through; {{key:Enter}} or a double-click finishes | A length along the pointer, `[x,y,z]` or `<dx,dy,dz>` for the next point |
| {{ui:Arc center}} | — | Center, start, then the end | The radius after the center; the angle in degrees after the start; `12s` for segments |

## Editing tools

| Tool | Key | Clicks | Type in the value box |
|---|---|---|---|
| {{ui:Select}} | {{key:Space}} | Click, double-click, triple-click, drag a box. See [Select things](topic:select) | With one dimension or edge selected: a new length, which resizes the model |
| {{ui:Push/pull}} | {{key:P}} | The face, then the distance. Double-click a face to repeat the last distance | A distance; negative goes the other way ({{ui:Distance}}) |
| {{ui:Move}} | {{key:M}} | From a point, to a point. Hold {{key:Ctrl}} or {{key:Alt}} to copy | A distance toward the pointer, `<dx,dy,dz>` or `[x,y,z]` ({{ui:Distance}}). After a copy: `3x` or `/3` |
| {{ui:Rotate}} | {{key:Q}} | Center (on a face to turn in its plane), start direction, end direction | The angle in degrees ({{ui:Angle}}) |
| {{ui:Scale}} | {{key:S}} | The point that stays, a reference point, the new size | A factor: `2`, `0.5` ({{ui:Scale}}) |
| {{ui:Eraser}} | {{key:E}} | Click or drag over edges, groups, guides and dimensions | nothing |
| {{ui:Tape}} | {{key:T}} | From an edge: a parallel guide line. From a point: measures and leaves a guide point | The offset ({{ui:Offset}}) or the distance ({{ui:Length}}) |
| {{ui:Dimension}} | {{key:D}} | Two corners (or one edge), then where the dimension line goes. See [Dimension and resize](topic:dimensions) | The offset ({{ui:Offset}}). Right after: a new length, which resizes the model |

{{ui:Move}}, {{ui:Rotate}} and {{ui:Scale}} work on the selection. If nothing is selected, they take what you click first (a face brings its edges).

## Camera tools

| Tool | Key | Drag to |
|---|---|---|
| {{ui:Orbit}} | {{key:O}} | Turn around the center of the view |
| {{ui:Pan}} | {{key:H}} | Slide the view |
| {{ui:Zoom}} | {{key:Z}} | Zoom: up is in, down is out |
| {{ui:Extents}} | {{key:Shift+Z}} | (a button, not a tool) Fit the whole model |

You rarely need these with a mouse: middle-drag orbits, right-drag pans and the wheel zooms with any tool. See [Orbit, pan and zoom](topic:orbit-pan-zoom).

## Organize

| Button | Key | Does |
|---|---|---|
| {{ui:Component}} | {{key:G}} | Makes the selection a component |
| {{ui:Group}} | {{key:Shift+G}} | Makes the selection a group |
| {{ui:Explode}} | none | Turns the selected groups and components back into loose geometry |

## Place component

Not on the toolbar: click {{ui:Place}} next to a component in the {{ui:Components}} panel. Click once where the component's origin goes; the tool then returns to {{ui:Select}}.
