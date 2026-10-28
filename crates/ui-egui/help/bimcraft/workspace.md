---
title: The BimCraft screen
summary: A tour of every bar, panel and button, by its on-screen name, and how the screen changes for each kind of file.
section: start
order: 20
keywords: screen, layout, panels, toolbar, interface, ui, tour, properties panel, browser, app bar
---

The BimCraft screen has a bar across the top and three areas under it: a panel on the left, the stage in the middle and the {{ui:Properties}} panel on the right. What appears in each area depends on what you opened: an IFC model, a project or family file, a drawing or a folder.

## The app bar

The thin bar across the top belongs to the CadCraft site, not to BimCraft itself. From left to right:

- The CadCraft mark: goes to the CadCraft home page.
- The app name and a version menu. In the staff build it shows which build you are running and lets you switch to a released version.
- {{ui:About BimCraft}}: the app's page on the site.
- {{ui:Settings}}: BimCraft's own settings and the ones every app shares. See [BimCraft settings](topic:settings).
- {{ui:Full screen}}: cycles the screen modes. See [Screen modes](topic:workspace#screen-modes) below.
- {{ui:Paper}} and {{ui:Model}}: the light and dark themes. The 3D view and the 2D drawings follow the theme you pick.

## The left panel

The left panel always starts with **Open a file or folder**:

- The drop area, {{ui:Choose a file, or drop a file or folder here}}. Click it to choose one file, or drop a file or folder on it (or anywhere on the page).
- {{ui:Open a folder}}: choose a whole project folder. See [Open a whole project folder](topic:open-project-folder).
- {{ui:Recent folders}}: appears in Chrome and Edge after you open a folder with the folder picker or by dropping it. Click {{ui:Reopen}} and the folder name to open it again, or the small × to forget it.

Under it, sections appear as they apply:

| Section | When it shows | What is in it |
|---|---|---|
| {{ui:Changes}} | An IFC model is open | Your unsaved edits, newest first, with {{ui:Undo}} and {{ui:Save as IFC}} |
| {{ui:File}} | A model or file card is open | Facts about the file: size, schema or release, project, author, element count |
| {{ui:Drawing}} | A DWG or DXF is open | Format, units, entity and layer counts, sheets, linked drawings, equipment |
| {{ui:Folder}} | A folder is open | File and model counts, size, problems found |
| {{ui:Back to the folder}} | You opened a file from a folder | Returns to the folder report |
| {{ui:Layers}} | A drawing is open | A checkbox per layer, with {{ui:All on}} and {{ui:All off}} |
| {{ui:Storeys}} | An IFC model is open | A checkbox per storey, top storey first |
| {{ui:Elements}} | An IFC model is open | How many elements of each kind the model holds (the 30 most common) |

## The stage

The middle of the screen shows the open file.

- **Empty**: "Open a building model." with {{ui:Choose a file}} and {{ui:Open a folder}}, and a short description of each format.
- **IFC model**: the 3D view with a toolbar above it: {{ui:Fit}}, {{ui:Top}}, {{ui:Front}}, {{ui:Side}}, then {{ui:Edges}} and {{ui:Spaces}}; the edit commands {{ui:Undo}}, {{ui:Redo}}, {{ui:Copy}}, {{ui:Paste}} and {{ui:More}} (see [Copy, paste and duplicate library items](topic:copy-paste)); then {{ui:Library}}, {{ui:Equipment}}, {{ui:Updates}} and {{ui:Credits}} for library items, and {{ui:Section}} (see [Cut an IFC model with a section plane](topic:section-plane)). A view cube sits in the bottom-right corner. See [Open an IFC model and look around in 3D](topic:open-ifc-model). {{ui:Settings}}, {{ui:General}} chooses icons, labels or both on these buttons and their order.
- **Project or family file**: the file card, with the preview picture, project information and linked files. See [Read a project or family file](topic:read-rvt-file).
- **DWG or DXF**: a row of tabs ({{ui:Model}}, then one per sheet, then {{ui:Equipment}} when the drawing has tagged equipment, {{ui:3D}}, {{ui:Fit}} and {{ui:Print view}}), the drawing, and live X and Y coordinates in a corner. See [View a DWG or DXF drawing](topic:view-dwg) and [See a DWG or DXF drawing in 3D](topic:dwg-in-3d).
- **Folder**: the folder report, with the {{ui:Start here}} box and tables of models, links, IFC models and drawings. See [Open a whole project folder](topic:open-project-folder).

The **status line** at the bottom of the stage tells you what BimCraft is doing ("Reading the model…", "Building geometry: 1200 of 5400 elements") and what went wrong, in red, when something fails. Messages about success fade after a few seconds.

## The Properties panel

The right panel shows:

- For an IFC model: the element you clicked, with its name, type, tag, GlobalId and every property set. Values you can change look like buttons. See [Properties panel fields](topic:properties-panel).
- For a drawing: how to move around, the drawing's linked drawings and images and whether each was found, and a list of what is **Not drawn yet**.
- For a file card or folder: a short note on what to open next.

## On a small screen

When the window is narrower than a laptop screen, BimCraft stacks its areas: the stage on top, then the panels underneath. Scroll the page to reach the panels. On a phone they form one column; on a tablet, two.

## Screen modes

The {{ui:Full screen}} button in the app bar, or {{key:Ctrl+Shift+F}} on every platform, cycles three modes:

1. Normal.
2. Full screen, with the bars still showing.
3. Full screen with the app bar hidden. Move the pointer to the top edge, or tap there, to bring it back.

Press {{key:Esc}} to go back to normal. Browsers without full screen support (such as on an iPhone) switch only between normal and hidden bars.

## Related

- [Keys and mouse](topic:keyboard)
- [Files BimCraft opens and saves](topic:files)
