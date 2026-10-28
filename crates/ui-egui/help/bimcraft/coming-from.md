---
title: Coming from other BIM and drawing programs
summary: How habits from desktop BIM authoring and DWG viewing map to BimCraft, and the differences that trip people up.
section: switching
order: 10
keywords: switching, migrate, familiar, desktop bim, family, parameters, project browser, properties palette, view cube, xref, layer manager, differences, compare
---

If you know Revit, most of BimCraft will feel familiar in words and unfamiliar in scope. The single most important difference: **this build reads and inspects models and edits IFC data. It does not author like Revit yet.** You cannot draw walls, place families, make views or sheets, or save a project file. What you can do is open almost any building file on any computer with a browser, see what is in it, check a whole project folder, and correct the data in an IFC model.

## Habits mapped

| If you are used to | In BimCraft |
|---|---|
| Opening an `.rvt` in Revit to see the model | BimCraft opens the `.rvt` as a file card: release, project information, links, preview. For the 3D model, export IFC and open that. [Read a project or family file](topic:read-rvt-file) |
| Checking which release a file was saved in before opening it | The card says "saved in Revit …" at the top, without upgrading or touching the file |
| The Project Browser | No browser of views and sheets. An IFC model has {{ui:Storeys}} and {{ui:Elements}} lists; a drawing has sheet tabs |
| The Properties palette | The {{ui:Properties}} panel on the right, for the element you clicked. IFC property sets instead of parameter groups |
| Instance and type parameters | Instance property sets, and sets marked "(type)". A type change applies to every element of the type, as in Revit. [Change a type property for every element of a type](topic:type-vs-instance) |
| Edit Type | No dialog: click a value in a "(type)" set and type |
| Selecting many elements, filters, Select All Instances | One element at a time. Not built yet |
| The view cube and the navigation wheel | A view cube in the bottom-right corner, four buttons ({{ui:Fit}}, {{ui:Top}}, {{ui:Front}}, {{ui:Side}}) and free orbit. No navigation wheel |
| Shift and middle-drag to orbit, middle-drag to pan | Left-drag orbits, right-drag (or {{key:Shift}} and drag) pans, wheel zooms. The {{ui:BIM ribbon}} menu style switches to middle-drag pan and {{key:Shift}} with the middle button to orbit. [Menus, keys and mouse](topic:menus-keys-and-mouse) |
| Floor plans, sections, section boxes | One {{ui:Section}} plane at a time ({{key:X}}); no section box or 2D plans. [Cut an IFC model with a section plane](topic:section-plane) |
| Hide in view, isolate, visibility/graphics | {{ui:Storeys}} on and off, {{ui:Spaces}} and {{ui:Edges}}. No per-element hiding yet |
| Rooms | IFC spaces, hidden at first. Click {{ui:Spaces}} |
| Save, Save As, synchronize with central | {{ui:Save as IFC}} downloads a new IFC file. The original never changes. No central, no worksharing |
| Undo with {{key:Mod+Z}} | Works on IFC edits until you save. [Change IFC properties and save an edited copy](topic:edit-ifc-properties) |
| Manage Links | Open the project folder: every link is checked and marked **In this folder**, **Same name found**, **Library file** or **Not in this folder**. [Open a whole project folder](topic:open-project-folder) |
| Audit, or opening a file to see if it is corrupt | The folder report checks every copy and backup for being cut off, in seconds. [Check a folder for damaged copies](topic:check-folder-health) |
| Picking the right file from a messy folder | The {{ui:Start here}} box suggests one, with reasons |

If you mostly open models to check them, as in a model checking or IFC viewing program, pick the {{ui:Viewer}} menu style: the viewing and checking tools come first and the building tools wait in one menu. See [Menus, keys and mouse](topic:menus-keys-and-mouse).

## From viewing DWG files

If you open DWGs in AutoCAD or a DWG viewer:

| If you are used to | In BimCraft |
|---|---|
| Model and layout tabs at the bottom | Tabs above the drawing: {{ui:Model}} and one per sheet |
| ZOOM Extents, double-click the wheel | Double-click, {{key:F}} or {{ui:Fit}} |
| Pan with the middle button | Drag with any button |
| The layer manager | The {{ui:Layers}} list: tick to show, untick to hide, {{ui:All on}} and {{ui:All off}} |
| The xref manager | **Linked drawings** on the right, with **Shown** or **Not found**, and {{ui:Add linked drawings…}} |
| Plot preview | {{ui:Print view}} or {{key:P}}: black on white with line weights. It does not print |
| Data extraction of block attributes | The {{ui:Equipment}} tab for tagged foodservice drawings, with {{ui:Download CSV}} |
| Measure, draw, edit, save as | Not in BimCraft. The CadCraft drafting app draws and edits |

## Differences that trip people up

- **It runs in the browser, and the browser is careful with your files.** BimCraft can only read files and folders you hand it, by choosing them or dragging them in. It cannot open a file by itself, and it cannot save over one. Every save is a download.
- **Nothing is uploaded.** The words "upload" in some browsers' folder dialogs are the browser's own wording. The files stay on your computer.
- **IFC is the model format.** A project file opens as a card because its 3D content is undocumented. Treat IFC as the way models travel into BimCraft.
- **Edits are copy-on-write.** Changing one element's value never changes its neighbors, even when the exporter shared that value between them. Type properties are the deliberate exception.
- **No project, no views.** BimCraft shows one file at a time and remembers nothing about it after you close the tab, except the names of recent folders in Chrome and Edge.
- **Mac keys.** Where Revit's Windows shortcuts use Ctrl, BimCraft's shortcuts use {{key:Mod}}: Command on a Mac.
- **Your mouse habits.** Orbit is the left button, not Shift and the middle button, unless you pick the {{ui:BIM ribbon}} menu style or set the buttons yourself in [Menus, keys and mouse](topic:menus-keys-and-mouse).

See [What is not built yet](topic:not-built-yet) for what to use instead today.
