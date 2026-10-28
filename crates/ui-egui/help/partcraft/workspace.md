---
title: The PartCraft screen
summary: A tour of every bar and panel: the toolbar and how it changes, the history, the part facts, the 3D view, the properties panel and the status bar.
section: start
order: 20
keywords: screen, layout, toolbar, history, feature tree, properties, panel, status bar, interface, ui, tour, full screen
---

The screen has five parts: the toolbar across the top, the history on the left, the 3D view in the middle, the properties panel on the right and the status bar along the bottom. Above them all sits the CadCraft app bar.

## The app bar

The thin bar at the very top belongs to the CadCraft site, not to PartCraft itself. It shows the app name and where it runs, a version menu, **About PartCraft**, the screen-mode button and the Paper / Model theme switch.

- **Paper** is dark lines on a light sheet. **Model** is light lines on a dark background. Sketch colors follow the theme.
- {{ui:Settings}} (the gear, or {{key:Mod+,}}) has a {{ui:PartCraft}} tab: which mouse button turns the view, mouse wheel or trackpad, and relations while sketching. See [PartCraft settings](topic:settings).
- The screen-mode button, or {{key:Ctrl+Shift+F}}, cycles through normal, full screen, and full screen with the bars hidden. With the bars hidden, move the pointer to the top edge (or tap there) to bring them back. {{key:Esc}} returns to normal. This key is {{key:Ctrl+Shift+F}} on a Mac too, not ⌘.

## The toolbar

The toolbar is grouped. It changes with what you are doing, so it fits one row:

| Group | Buttons | Shown |
|---|---|---|
| File | {{ui:New}}, {{ui:Open}}, {{ui:Save}}, {{ui:Export}} (a menu) | Always |
| Sketch | {{ui:Sketch}} | When no sketch is open |
| Sketch | {{ui:Exit}}, {{ui:Line}}, {{ui:Rectangle}}, {{ui:Circle}}, {{ui:Arc}}, {{ui:Point}}, {{ui:Centerline}}, {{ui:Fillet}}, {{ui:Dimension}}, {{ui:Construction}} | Only while a sketch is open |
| Features | {{ui:Extrude}}, {{ui:Cut}}, {{ui:Revolve}}, {{ui:Hole}}, {{ui:Linear}}, {{ui:Circular}}, {{ui:Mirror}}, {{ui:Fillet}}, {{ui:Chamfer}}, {{ui:Shell}}, {{ui:More}} (a menu: Revolved cut, Plane, Axis, Point, Sweep, Loft, Draft) | On the part, not while a sketch or the drawing is open. Fillet and Chamfer work on straight edges between two flat faces; Shell and Draft on flat faces. Sweep and Loft are not built yet in this version (they need an exact kernel); clicking one says so |
| Features | {{ui:Done}} | Only while you pick edges or faces for a fillet, chamfer, shell or draft |
| Drawing | {{ui:Drawing}} | On the part (usable once the part has a solid) |
| Drawing | {{ui:Datum}}, {{ui:Frame}}, {{ui:Finish}}, {{ui:Detail}}, {{ui:PDF}}, {{ui:Back}} | Only on the drawing sheet |
| Assembly | {{ui:Assembly}} | On the part (usable once the part has a solid) |
| Assembly | {{ui:Add copy}}, {{ui:Insert}}, {{ui:Joint}}, {{ui:Clash}}, {{ui:Edit part}}, {{ui:Close}} | Only in an assembly |
| Assembly | {{ui:Back}} | While you edit a part from an assembly |
| Edit | {{ui:Undo}}, {{ui:Redo}}, {{ui:Copy}}, {{ui:Paste}}, {{ui:More}} (a menu: Cut, Copy, Paste, Paste in place, Duplicate, Delete, Select all, Select none) | Always; each item works on the sketch, the history or the assembly you are in |
| View | {{ui:Fit}}, {{ui:Normal to}}, {{ui:Views}} (a menu: Isometric, Front, Back, Top, Bottom, Right, Left) | Always |

Hover over a button to see what it does and its key. A grayed-out button cannot be used right now: for example the feature buttons while a sketch is open, or {{ui:Joint}} before the assembly has two parts.

The {{ui:More}} menu in the Edit group lists each editing action with its key. What copy and paste act on follows where you are; see [Copy, paste and duplicate](topic:copy-paste). A grayed-out item's tooltip says why it cannot act now.

The {{ui:Export}} menu holds {{ui:STL, for 3D printing}}, {{ui:3MF, for 3D printing}}, {{ui:STEP AP242 (exact solid)}}, {{ui:STEP AP214 (exact solid)}}, {{ui:STEP (faceted, mesh kernel)}}, {{ui:STEP AP214 (faceted, mesh kernel)}}, {{ui:DXF of the sketch or flat face}} and {{ui:Drawing (PDF): views, section, detail, GD&T}}. The two exact STEP items are grayed: they are not built yet in this version, so use {{ui:STEP (faceted, mesh kernel)}}.

## The history (left)

The panel on the left is the part's feature history, the list PartCraft rebuilds from top to bottom.

- The heading is the part's name ("Part1" until you rename it).
- **Front plane**, **Top plane** and **Right plane** come first. Click one to select it; it highlights in the view.
- Below them are your features in the order they were made. A sketch that a feature uses is indented under that feature.
- A flag at the right end of a row tells you its state:

| Flag | Meaning |
|---|---|
| (none) | Built, and a sketch is fully defined |
| − | A sketch that is under defined (it can still move) |
| + | A sketch that is over defined |
| ⚠ | Built, with a warning (hover to read it) |
| ⚠ error | The feature failed. Hover to read why, or click it |
| ⚠ not built | The feature needs another one that failed or is off |
| off | Suppressed |

- Click a feature to edit it on the right. Double-click a sketch to open it.
- When you roll back, a **Rollback bar · roll forward** row sits in the list. Features below it are grayed and not built.
- With a row focused, {{key:Up}} and {{key:Down}} move between rows, and {{key:Alt+Up}} / {{key:Alt+Down}} move the feature itself, where the history allows.

In an assembly the same panel lists **Parts** (the first is marked "grounded") and **Joints** instead.

### Part facts

Under the history, the **Part** box lists the solid's **Volume**, **Surface area**, **Size (X × Y × Z)**, **Through holes**, **Bodies**, **Triangles**, the number of **Features** and how many are **Failing**. Volumes and areas are of the faceted solid built by the mesh kernel, a little under the true value for round shapes (see [Features and their options](topic:features#curves-resolution)).

In an assembly the box is headed **Assembly**: Parts, Instances, Joints, Not joined and Interference.

## The 3D view (middle)

The view shows the solid, the base planes (when the part is empty or you are choosing where to sketch) and any sketch not yet used by a feature.

- Drag to turn the part, right-drag or middle-drag to pan, roll the wheel to zoom at the pointer. Details in [Keys and mouse](topic:keyboard).
- Hover a face, edge or plane to see it light up. Click to pick it. Faces highlight whole and edges are drawn and picked whole. {{key:Shift}}+click collects several edges or faces. They stay marked.
- The view cube at the top right turns with the part: click a face, edge or corner to look from there, drag it to turn, the house button goes back to the start. See [The view cube](topic:view-cube).
- When a part is empty, a box headed **Start a part** lists the first steps.
- Drop a `.partcraft.json` or `.partcraft-asm.json` file anywhere on the view to open it. Opening `.step` and `.stp` files is not built yet in this version.

### While you pick edges or faces

After {{ui:Fillet}}, {{ui:Chamfer}} or {{ui:Shell}} with nothing picked (or {{ui:Pick edges}} on such a feature), the part shows as it was before that feature. Click edges or faces to add or drop them; their marks show what is in. {{ui:Done}} or {{key:Esc}} finishes. See [Round, bevel and hollow](topic:fillet-chamfer-shell).

### On the drawing sheet

{{ui:Drawing}} replaces the 3D view with the A3 drawing sheet. Wheel zooms, right or middle drag pans, {{key:F}} fits. See [Make a drawing](topic:drawing-pdf).

### While a sketch is open

The view turns to look straight at the sketch plane and a 2D sketch layer appears on top. The part shows only the features made before this sketch. A strip at the top left shows the sketch's state chip ("Fully defined", "Under defined · 3 degrees of freedom" and so on) and its name, such as "Sketch2 on a face of Extrude1".

## The properties panel (right)

The panel on the right shows whatever is selected, and its heading names it:

- **Part** (nothing selected): the part's **Name** and the **Curves** setting.
- A plane: a {{ui:Sketch on it}} button.
- A **Face**: what kind it is (Flat face, Cylinder with its radius and axis, Cone, Sphere or Torus), its area, which feature made it, and {{ui:Sketch on this face}}, {{ui:Normal to}} and {{ui:Export face as DXF}} for a flat face.
- An **Edge**: its kind (straight, a circle with its centre and radius, or a curve), its length and the two faces it lies between (such as "Extrude1 end face and Extrude1 side"), with {{ui:Fillet}} and {{ui:Chamfer}} buttons.
- Several edges or faces picked with {{key:Shift}}: how many, with {{ui:Fillet}} and {{ui:Chamfer}} for edges, and {{ui:Clear}}.
- A feature: its numbers and options, the dimensions of its sketch, then {{ui:Suppress}}, {{ui:Roll back to here}}, {{ui:Move up}}, {{ui:Move down}}, {{ui:Delete}} and its **Name**.
- An open sketch: its state, the relations you can add to the selection, the list of relations and dimensions, **Add by coordinates**, and {{ui:Exit sketch}}, {{ui:Fit}} and {{ui:Export as DXF}}.
- On the drawing sheet: the section and detail settings, or the selected GD&T symbol.
- In an assembly: the assembly, the selected part, or the selected joint.

Every number box takes effect when you press {{key:Enter}} or {{key:Tab}}, or click away. The part rebuilds at once; there is no OK button. Use {{key:Mod+Z}} to go back.

## The status bar (bottom)

- On the left, the pointer's coordinates in the sketch (only while a sketch is open).
- In the middle, messages: what to do next, and why something was refused. Read it when nothing seems to happen.
- Near the right, the kernel chip: always "Mesh kernel" in this version. Hover it for what that means. See [The mesh kernel](topic:solid-kernels).
- On the right, a chip: "Rebuilding…" while the part rebuilds, then "Rebuilt in 120 ms" when all is well, or "1 feature failing". In an assembly it says "Placed in … ms" or counts joint problems.

## On a narrow screen

Up to 760 pixels wide, such as on a phone, the view keeps the screen: the toolbar is one row that scrolls sideways, and the panels open as sheets from a bar at the bottom. {{ui:Tools}} hides the toolbar and {{ui:Move}} puts it at the bottom. See [Trackpad, touch and small screens](topic:trackpad-and-touch).
