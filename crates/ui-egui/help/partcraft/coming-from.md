---
title: Coming from another parametric modeler
summary: How the habits of desktop parametric part modelers map to PartCraft, and the differences that trip experienced users up in the first hour.
section: switching
order: 10
keywords: switching, migrate, familiar, feature manager, design tree, smart dimension, boss extrude, cut extrude, hole wizard, mates, interference detection, rollback bar, property manager, mouse, differences
---

If you design parts in SolidWorks, most of PartCraft's ideas will be familiar: sketches on Front, Top and Right planes, relations and smart dimensions, a feature history with a rollback bar, and assemblies placed by mates. The way you reach them is different in places, and this first build does less. This page maps your habits and lists the surprises.

## Habits, mapped

| You are used to | In PartCraft |
|---|---|
| The design tree on the left | The history on the left: planes, then features, sketches indented under the feature that uses them |
| Front, Top and Right planes | The same three, through the origin. Front is the XY plane, Top is XZ, Right is YZ; Y points up |
| Sketch on a plane or face, then the confirmation corner to exit | {{ui:Sketch}} ({{key:S}}) on a selected plane or flat face; {{ui:Exit}} or {{key:Esc}} until it closes |
| Smart Dimension | {{ui:Dimension}} ({{key:D}}). Click one or two items, then where the number goes. Where you place it picks horizontal, vertical or aligned |
| Add Relations in the property panel | Select items, then the relation buttons in the right panel (Coincident, Horizontal, Parallel, Tangent, Equal and so on) |
| Automatic relations while sketching | Snaps add coincident, point-on-curve, horizontal and vertical relations |
| Blue / black / red sketch colors | The same idea: blue under defined, ink fully defined, red over defined. The chip shows the degrees of freedom left |
| Centerline for revolves | {{ui:Centerline}} ({{key:N}}); {{ui:Revolve}} uses the first centerline unless you pick another axis |
| Sketch fillet | {{ui:Fillet}} ({{key:F}}) in a sketch: click the corner, type the radius |
| Construction geometry toggle | {{ui:Construction}} ({{key:Q}}) |
| Extruded boss | {{ui:Extrude}}: Blind, Mid-plane, Up to face / next / vertex, Offset from face, second direction, taper, one contour (Pick regions) |
| Extruded cut | {{ui:Cut}}: starts as Through all; switch to Blind for a pocket |
| Revolved boss / cut | {{ui:Revolve}} / {{ui:More}} › {{ui:Revolved cut}} |
| Reference plane / axis / point | {{ui:More}} › {{ui:Plane}}, {{ui:Axis}}, {{ui:Point}} (see [Reference planes, axes and points](topic:reference-geometry)) |
| Swept / lofted boss or cut, draft | {{ui:More}} › {{ui:Draft}} on flat faces (see [Sweep, loft and draft](topic:sweep-loft-draft)). {{ui:Sweep}} and {{ui:Loft}} are not built yet in this version (they need an exact kernel) |
| Hole Wizard | {{ui:Hole}} at every loose sketch point: simple, counterbore, countersink (90° / 82°) or tapped ISO metric M2–M24, through or blind |
| Linear / circular pattern, mirror | {{ui:Linear}}, {{ui:Circular}}, {{ui:Mirror}} (features or the whole body) |
| Fillet and chamfer on edges | {{ui:Fillet}} / {{ui:Chamfer}}: click an edge, {{key:Shift}}+click more, then the button. One radius; equal, two-distance or distance-angle chamfers. In this version only straight edges between two flat faces, and rounds that meet at a corner are mitred. Round or bevel curved edges in the sketch ({{ui:Fillet}} in a sketch, {{key:F}}) |
| Shell | {{ui:Shell}}: click the flat faces to open, then the button. One wall thickness |
| Open a STEP file | Not built yet in this version (it needs an exact kernel) |
| Edit feature, then OK | Click the feature; change numbers on the right; it rebuilds as you type. No OK, no Cancel: use {{key:Mod+Z}} |
| Edit sketch | Double-click the sketch in the history |
| Rebuild | Automatic after every change. There is no rebuild key |
| Suppress / unsuppress | {{ui:Suppress}} / {{ui:Unsuppress}} in the right panel |
| Drag the rollback bar | {{ui:Roll back to here}} on a feature; click "Rollback bar · roll forward" to return |
| Drag a feature in the tree to reorder | {{ui:Move up}} / {{ui:Move down}} or {{key:Alt+Up}} / {{key:Alt+Down}}; refused with the reason when it would break a dependency |
| Edit dimensions by double-clicking on the model | Select the feature: its sketch's dimensions are listed in the right panel |
| Mates: coincident, concentric, distance, angle | {{ui:Joint}} ({{key:J}}): pick a flat face on each part; the joint puts them face to face. **Centre on** an axis acts like concentric; **Offset** like distance; **Angle** like angle |
| Fixed component | The first part is grounded; {{ui:Make it the grounded part}} changes it |
| Insert component | {{ui:Add copy}} or {{ui:Duplicate}} (another copy) or {{ui:Insert}} (from a part file) |
| Edit part in the assembly | {{ui:Edit part}}, then {{ui:Back}} |
| Interference detection | {{ui:Clash}} |
| Moving a component to see the motion | Drag the angle or offset slider of a revolute or slider joint; {{ui:Check the motion for clashes}} sweeps it. A part with no joint: **Move by hand** (arrows to drag, or typed values) |
| Measure / Inspect | {{ui:Views}}, then {{ui:Measure}} ({{key:M}}): distances, angles, radii, areas; {{ui:Mass properties}} |
| Exploded view, BOM | In an assembly with nothing selected: **Exploded views** and **Bill of materials** in the right panel |
| Mass properties | The **Part** box: volume, surface area, size, of the faceted solid (a little low on round parts). No mass: there are no materials yet |
| Save, Save As STEP | {{ui:Save}} downloads a `.partcraft.json`; {{ui:Export}} has STL, 3MF, faceted STEP, DXF and a PDF drawing (exact STEP is not built yet in this version) |
| Make a drawing from the part | {{ui:Drawing}}: an automatic A3 sheet with three views, isometric, one section, one detail, datums, feature control frames and finish symbols. Only overall sizes are dimensioned |
| Middle-drag to rotate | **Left-drag** rotates here. Middle- or right-drag pans. The wheel zooms at the pointer |
| View orientation, Normal To | {{ui:Views}} menu; {{ui:Normal to}} for a picked face, plane or sketch |
| Zoom to fit (F) | {{key:F}}, or {{ui:Fit}} (in a sketch, {{key:F}} is the Fillet tool) |
| Right-click menus | None. Right-drag pans; in a sketch, right-click ends a line chain |

:::tip
If your hands know SolidWorks, pick the {{ui:Feature tree}} menu style in {{ui:Settings}} › {{ui:PartCraft}} › {{ui:Menus, keys and mouse}}: a ribbon of Features, Sketch, Surfaces, Evaluate, Drawing, Assembly and View tabs, the middle button orbits, {{key:Shift}} and the middle button pan, and the left button only picks. See [Menus, keys and mouse](topic:menus-keys-and-mouse).
:::

## Differences that trip people up

**The whole sketch is the profile.** Extrude and Revolve use every closed loop in the sketch. You cannot pick one region or one contour of a sketch for a feature. Use one sketch per feature.

**Which sketch a feature takes.** A feature uses the sketch you just closed or the one selected in the history. With nothing selected it takes the newest sketch no feature uses yet. If you get the wrong one, select the right sketch and choose the feature again.

**Changes apply as you type.** There is no feature dialog with OK and Cancel. Every number rebuilds at once, and {{key:Mod+Z}} is the way back. The undo list covers sketch edits, features and settings alike.

**Over defined is never allowed to stand quietly.** There are no driven (reference) dimensions. A dimension that the other relations already fix is reported as over defined ("is already implied. Delete it."). A conflicting one leaves the geometry untouched and turns red. Delete one, or undo.

**Sweeps, lofts and exact STEP are not built yet; fillets, chamfers, shells and drafts have limits.** This version of PartCraft builds every part with a mesh kernel; the status bar chip always says "Mesh kernel". Fillets and chamfers work on straight edges between two flat faces, and where rounds meet at a corner they are mitred, not blended. Shells open, and drafts tilt, flat faces only. Features that need an exact kernel stay clickable, but say "not built yet" when chosen. See [The mesh kernel](topic:solid-kernels) and [What is not built yet](topic:not-built-yet).

**Two buttons called Fillet.** In a sketch, {{ui:Fillet}} ({{key:F}}) rounds a sketch corner. On the part, the {{ui:Fillet}} in the Features group rounds solid edges (straight edges between flat faces in this version). There is no sweep or loft in this version: see [What is not built yet](topic:not-built-yet).

**Holes need loose points.** {{ui:Hole}} drills at points made with the Point tool. Circle centers and line ends do not count.

**Through all is for cuts only.** A boss needs a depth. There are no "up to vertex / surface / next" end conditions yet.

**The solid is faceted.** The mesh kernel builds round faces from flat facets, so the view, volumes, STL, 3MF and STEP exports are all made of them: **Curves** in the part's panel makes them finer.

**Millimeters only.** There is no inch setting yet. Multiply inch sizes by 25.4 when you type them.

**Y is up.** The Front plane faces you and Top looks down on the part, as you expect, but the world's up axis is Y. STL and 3MF exports are turned to Z up; the faceted STEP keeps Y up.

**Mates are faces, not geometry pairs.** A joint always starts from two flat faces put face to face. For a pin in a hole, pick the flat face around the hole and the pin's shoulder, then set **Centre on** to the hole's axis and the pin's revolve axis. Parts not reached by joints stay where they were inserted; you cannot drag them. Joints in a loop are not solved.

**Saving is a download.** There is no file you keep overwriting. Each save is a new file in your downloads folder. Name the part first, and keep your files in a project folder.

**No context menus, no command manager tabs.** Everything is on the toolbar and the right panel, and the toolbar changes with what you are doing: the sketch tools appear only in a sketch, the assembly tools only in an assembly.

**Keys are single letters inside a sketch.** {{key:L}}, {{key:R}}, {{key:C}}, {{key:A}}, {{key:P}}, {{key:N}}, {{key:F}}, {{key:D}}, {{key:Q}} and {{key:S}} pick tools with no modifier. See [Keys and mouse](topic:keyboard).
