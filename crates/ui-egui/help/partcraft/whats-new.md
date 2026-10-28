---
title: What's new in 0.5
summary: PartCraft 0.5 adds threads you can print, standard bolts, nuts, washers, pins, bearings and keys, a 3D printing check, IFC export with every property, interference you can filter, scripts beside the model, sealed Python, menu styles, and a layout for phones.
section: start
order: 5
keywords: whats new, what's new, release notes, changelog, changes, 0.5, version, new features, thread, modelled thread, standard part, bolt, nut, washer, bearing, 3d printing, stl, 3mf, ifc, interference, parameters used by, script panel, python, cpython, magnetic, electromagnet, menu style, feature tree, keys, mouse, settings, phone, fold phone, tablet, studio, live
---

This is what changed in PartCraft 0.5, compared with 0.4. PartCraft still builds every part with the mesh kernel, as in 0.4: see [The mesh kernel](topic:solid-kernels).

## New

### Modeling

- **Threads.** {{ui:Features}} › {{ui:More}} › {{ui:Thread}} puts a thread on a round face you pick, a shaft or a bore: ISO metric M1.6 to M64 or trapezoidal Tr8×1.5 to Tr60×9, sized to the face or chosen, over the whole face or a length from the open end, right or left hand. It stays a drawn (cosmetic) thread with its callout unless you tick **Modelled**, which cuts the real helical thread with a clearance (0.1 mm to start with) so printed bolts and nuts fit. Tapped holes can be modelled the same way. See [Threads on shafts and bores, drawn or modelled](topic:threads).
- **Standard parts.** {{ui:Standard part}} in the File group opens a hex head screw or bolt (metric or inch), a hex nut, a plain washer, a parallel pin, a deep groove ball bearing or a parallel key as a new part, with every size of its standard as a configuration. See [Open a standard part](topic:standard-parts).
- **Type to change a feature.** Select a feature, or add one, and type a number: it goes into the feature's main value, such as an extrude's depth. {{key:Enter}} sets it. See [Change sizes without opening the sketch](topic:change-sizes).

### Parameters and scripts

- **See where a parameter is used.** Under each parameter on the {{ui:Parameters}} tab: "Used by:" and the features, parameters, configurations and scripts that use it, or "Not used yet." Click a feature to go to it. See [Drive a part with parameters](topic:parameters).
- **Scripts read the part's parameters** with `param('width')` and `params()` (`cs.param` and `cs.params` in Lua and Python). Each script's tab lists the parameters it reads, and marks names the part does not have. The command-line tool's `run --part` passes them too.
- **Script while you model.** The Design workspace's right panel has a Script tab with the script editor: the feature tree still selects, double-click edits a sketch, the view turns, and {{key:F5}} runs. {{ui:Pop out}} moves the panel to a window of its own, for a second screen; {{ui:Put back}} returns it. See [Write and run scripts](topic:craftscript).
- **Python is sealed off.** Python now runs on CPython built for WebAssembly. Like CraftScript and Lua, a Python script reaches only the part, the variables and the built-in functions: no network, no files. Each run starts fresh, and the first Python run loads about 7 MB instead of about 13 MB. Scripts work as before. See [Scripts in Python](topic:craftscript-python).
- **Magnetic fields in 2-D.** A new tutorial works out an electromagnet's field with the analysis library: the field in the gap, the inductance, the pull and where the core saturates. See [Magnetic fields in 2-D (tutorial)](topic:magnetic-fields-2d).

### Assemblies and files

- **Check for 3D printing.** {{ui:Export}} › {{ui:Check for 3D printing (STL or 3MF)…}} checks a part or a whole assembly for a slicer: holes in the surface, edges shared by more than two faces, faces turned the wrong way, shells inside out or passing through themselves. It gives the size, volume, triangles, the area on the bed and the overhang, then downloads STL or 3MF. {{ui:Settings}} › {{ui:PartCraft}} › {{ui:3D printer}} sets your bed size. See [Get a part ready for 3D printing](topic:print-3d).
- **IFC 4 export with every property.** {{ui:Export}} › {{ui:IFC 4 (with every property)}} writes a part or an assembly for building and plant models: the shape, the material, the part's properties, every parameter, the title block and quantities. An assembly writes each part once, and each copy keeps its own values. See [Export for printing, STEP and DXF](topic:export-files).
- **Interference you can filter.** Interference results now have kinds: {{ui:Overlap}}, {{ui:Thread fit (cosmetic threads)}}, {{ui:Thread depth mismatch}} and the new {{ui:Touching faces}}. Show or hide each kind, or {{ui:Hide this pair}}. The filters are saved with the assembly, and each change is one undo step. See [Check an assembly for clashes and motion](topic:check-assembly).
- **Add-ons can extend PartCraft:** commands, a panel tab, Properties sections, importers and exporters. PartCraft comes with none yet.

## Improved

- A tidier {{ui:Features}} › {{ui:More}} menu with group headings. Tools the mesh kernel can't build are dimmed, and the menu scrolls in a short window. The first solid you make fills the view.
- The command-line tool opens parts that carry scripts or studies. See [Scripts and the command line](topic:scripts-and-cli).
- The toolbar keeps to one row on a 1440 px window again: {{ui:Standard part}} and {{ui:Normal to}} show as icons below 1600 px, and {{ui:Standard part}} is hidden inside a sketch, since it opens a new part.

## Fixed

- Tapped holes showed no thread: no dashed circles in the view and no note on the drawing. They show again.
- {{key:Enter}} did nothing in the panel's number boxes. It now sets the value.
- {{ui:Features}} › {{ui:More}} ran off a short window. It scrolls now.

## In every app

- **CadCraft Studio is the desktop app.** From 0.5 the desktop download is CadCraft Studio: all eight apps in one install. The separate installer for each app stops. PartCraft still runs in the browser, and as a browser edition you can download.
- **Installers are not signed yet.** Code signing comes with 0.6. On Windows, if SmartScreen stops the installer, choose {{ui:More info}}, then {{ui:Run anyway}}. The macOS disk image (Apple silicon and Intel, macOS 13 or later) is not signed or notarized: the first time, right-click the app and choose {{ui:Open}}. iPad, Android and iPhone come with 0.6.
- **Work together live.** In CadCraft Studio, {{ui:Live}} shares the file you have open: teammates on the same network join with a short code and edit with you, each with their own undo. In PartCraft everyone builds one part's feature tree, and every window builds the same solid. Sessions on your own network are free, for everyone. Invite links over the internet aren't open yet. See [Work together live](common:work-together-live) and [Model one part together, live](topic:work-live).
- **Menus, keys and mouse.** In {{ui:Settings}} › {{ui:PartCraft}} › {{ui:Menus, keys and mouse}}, pick a menu style ({{ui:PartCraft}} or {{ui:Feature tree}}, a ribbon), give any command your own key, move commands between menus, set the wheel and the mouse buttons, and export or import a style as a `.ccinput.json` file. {{ui:Use this style in every app}} opens every app in its matching style. See [Menus, keys and mouse](topic:menus-keys-and-mouse).
- **A wider Settings dialog,** in two columns, so more fits without scrolling. See [PartCraft settings](topic:settings).
- **Floating and bottom panels fit the window.** A floating panel stays above the status bar and shrinks with a small window; on a short window a panel docked at the bottom leaves the view room. See [Arrange the panels and use a second monitor](common:arrange-panels).
- **Phones and fold phones.** On a screen up to 760 px wide, or a phone held sideways, the view keeps the screen: panels open as sheets from a bar at the bottom, the toolbar is one row that scrolls sideways, and the view cube sits small in a corner. {{ui:Tools}} hides the toolbar and {{ui:Move}} puts it at the bottom. Turning the phone, or opening and closing a fold phone, keeps your work, the selection and an open dialog. The app bar's {{ui:More}} menu holds the version, Find and the links. Phones work, but PartCraft is made for larger screens. See [Trackpad, touch and small screens](topic:trackpad-and-touch).
- **Touch tablets.** On a touch screen each toolbar is one row that scrolls sideways instead of wrapping onto several lines.
- **Spanish** stays a machine-made draft that has not been reviewed yet. Help topics are in English.

## Not in this version yet

- Still the mesh kernel: sweeps, lofts and surfaces, several bodies, sheet metal, ribs, rounds on curved edges, shells and drafts on curved faces, STEP import and exact STEP still say "Not built yet".
- Threads: a left-hand modelled thread is the mirror image of a right-hand one. The standard parts carry no threads (add a Thread feature). No gears yet, and no inch keys or inch washers.
- Working live: assemblies are not shared (an assembly opened during a session is your own copy), a sketch travels whole, so two people in one sketch at once means the later edit wins.
- Python packages (numpy and others), and Python in Save for offline.
- Cloud saving.
- [What is not built yet](topic:not-built-yet)
