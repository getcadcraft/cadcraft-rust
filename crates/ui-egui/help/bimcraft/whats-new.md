---
title: What's new in 0.5
summary: BimCraft starts to draw buildings: walls, doors and windows, rooms, floors and roofs, a residential set with stairs, framing, terrain and sheets, rendered pictures and materials, point clouds and map layers, work together live, menu styles, and phones.
section: start
order: 5
keywords: whats new, what's new, release notes, changelog, changes, 0.5, version, new features, new building, walls, doors, windows, rooms, roofs, foundations, stairs, railings, framing, terrain, sheets, electrical, takeoff, render, paint, materials, views, walkthrough, section box, point clouds, map layers, live, menu style, phone, studio
---

This is what changed in BimCraft 0.5, compared with 0.4. BimCraft starts to draw buildings: everything you draw is saved as plain IFC4, so other BIM programs open it.

## New

### Draw a building

- **Start a new building.** {{ui:File}} › {{ui:New building}} makes a blank model with a site, a building and its levels. Rename levels, change their heights and add more in the {{ui:Levels}} panel. See [Start a new building](topic:start-a-new-building).
- **Walls.** Click from point to point on the plan of a level, or type lengths such as `12'6"`. Walls snap to ends, corners and other walls, come in nine starter types with their layers, and clean up their own corners and crossings. See [Draw walls](topic:draw-walls).
- **Doors and windows.** Point at a wall and click, or type the distance from a corner. The opening cuts itself, and moving or deleting the wall takes its doors and windows along. See [Doors and windows](topic:doors-and-windows).
- **Click to change it.** Properties for a drawn wall, door or window, {{ui:Change type…}} for several at once, and grips on the plan. See [Change a wall, door or window](topic:change-a-wall-or-door).
- **Rooms, floors, ceilings and roofs.** Rooms appear as walls close them, with their number and area, and floors, ceilings and roofs follow the walls when they change. See [Rooms, floors, ceilings and roofs](topic:floors-and-roofs).
- **Schedules and a plan PDF.** Door, window, wall and room schedules that change as you draw, dimensions on the plan, and the plan of a level as a PDF at a standard scale. See [Schedules, dimensions and a PDF of the plan](topic:plan-schedules-and-pdf).

### Residential design

- **Roofs over any outline,** with a pitch for each wall, gables, sheds and roof planes. See [Roofs: a pitch for each wall, gables, sheds and roof planes](topic:roofs).
- **Foundations:** slab on grade, stem walls, crawlspace or basement, with footings that follow the walls. See [Foundations under the walls](topic:foundations).
- **Stairs and railings:** straight, L, U and winder stairs sized from the height between levels, with handrails and the stairwell cut in the floor above. See [Stairs and railings](topic:stairs-and-railings).
- **Kitchens, baths and furniture:** a residential library of cabinets, appliances, fixtures and furniture that snap to walls, countertops, and more door and window types. See [Kitchens, baths and furniture from the residential set](topic:kitchens-and-baths).
- **Framing** of walls, floors and roofs, with a lumber cut list. The sizes are labeled defaults: a designer or engineer checks them. See [Framing walls, floors and roofs](topic:framing).
- **Terrain:** the ground from survey points, contours, a scan or spot heights, a building pad with cut and fill, and paving. See [The site's ground, the building pad and paving](topic:terrain).
- **Elevations, sections and sheets:** a drawing set made from the model as one PDF. See [Elevations, sections and sheets](topic:sheets).
- **The electrical plan:** outlets, switches and lights with circuits, and their schedules. See [The electrical plan](topic:electrical-plan).
- **Takeoff:** the model bar's {{ui:Takeoff}} hands walls, roofs, footings, openings, cabinets, devices and the cut list to TakeoffCraft. See [Quantities to TakeoffCraft](topic:takeoff).

### Pictures and materials

- **Render** makes a daylight picture of the 3D view, on your own computer. {{ui:High quality (path traced)}} traces the light for glass, mirrors and metal. Light fixtures in the model light the picture. See [Render a picture of the model](topic:render).
- **Paint** puts materials such as brick, siding, roofing and wood on a whole element or one side, saved in the IFC. See [Paint materials onto walls, floors and roofs](topic:paint-materials).
- **Views** saves camera views by name, plays them in order and records walkthroughs. See [Save views and walkthroughs](topic:views-and-walkthroughs).
- **Trees, people and cars** in the library, for pictures of the site.

### Models you receive

- **Section volumes:** a section box of several boxes, faces that lean and extra planes, and a section box round what you pick, with grips. See [Cut an IFC model with a section plane](topic:section-plane).
- **Copy property values,** one at a time, all of them or as CSV. See [Properties panel fields](topic:properties-panel).
- **Latitude and longitude** of any point on a georeferenced model, from {{ui:Measure}}. See [Measure distances, face areas and locations in 3D](topic:measure).
- **Save and open color schemes** from the color legend. See [Color elements by a property](topic:color-by-property).
- **Point clouds:** open LAS, LAZ, XYZ or PTS on their own or beside a model, cut slices, and measure to points. See [Show a point cloud beside the model](topic:point-clouds).
- **Map layers:** a street map, aerial pictures or a map service on the ground under a georeferenced model. Nothing is asked of a map server until you turn it on. See [A map on the ground under the model](topic:map-layers).
- **Dynamic blocks in drawings:** BimCraft lists each block's visibility states, and the library offers each state as its own item. See [View a DWG or DXF drawing](topic:view-dwg).

### Working together and your name

- **Work on one model together.** In CadCraft Studio, several people rename, set properties, place, draw and remove elements in one IFC model at once. See [Work on one model together, live](topic:work-live).
- **Your name on issues comes from your account.** Signed in, issues carry your account's name with no question. {{ui:Settings}} › {{ui:BimCraft}} › {{ui:Your name on issues}} shows {{ui:From your account (<name>)}} and {{ui:Use another name}}. See [BimCraft settings](topic:settings#your-name-on-issues).

## Changed

- **From a project file to IFC:** the topic on which IFC export settings open best is rewritten. See [From a project file to BimCraft through IFC](topic:rvt-to-ifc).
- **Five menu styles:** BimCraft's own toolbar, {{ui:Home design}}, {{ui:BIM ribbon}}, {{ui:Drafting classic}} and {{ui:Viewer}}, for opening and checking models rather than drawing them. See [Menus, keys and mouse](topic:menus-keys-and-mouse).
- Messages about damaged or unsupported drawings now come in your language.

## Fixed

- A door copied in a model turned on its site lined up with the model's axes, not with the building. The copy now lines up with the wall.
- Sections of a turned building cut along the model's axes. {{ui:Square to the building}} in the section bar turns the planes and the box to the walls.
- Section caps painted open elements, such as tree foliage, in the cut color, and a section box round the whole model could speckle the outer walls. Only closed elements are capped now.
- {{key:Esc}} in Settings stopped {{ui:Measure}}.

## In every app

- **CadCraft Studio is the desktop app.** From 0.5 the desktop download is CadCraft Studio: all eight apps in one install. The separate installer for each app stops. BimCraft still runs in the browser, and as a browser edition you can download.
- **Installers are not signed yet.** Code signing comes with 0.6. On Windows, if SmartScreen stops the installer, choose {{ui:More info}}, then {{ui:Run anyway}}. The macOS disk image (Apple silicon and Intel, macOS 13 or later) is not signed or notarized: the first time, right-click the app and choose {{ui:Open}}. iPad, Android and iPhone come with 0.6.
- **Work together live.** In CadCraft Studio, {{ui:Live}} shares the file you have open: teammates on the same network join with a short code and edit with you, each with their own undo. Sessions on your own network are free, for everyone. Invite links over the internet aren't open yet. See [Work together live](common:work-together-live) and [Work on one model together, live](topic:work-live).
- **Menus, keys and mouse.** In {{ui:Settings}} › {{ui:BimCraft}} › {{ui:Menus, keys and mouse}}, pick a menu style, give any command your own key, move commands between menus, set the wheel and the mouse buttons, and export or import a style as a `.ccinput.json` file. {{ui:Use this style in every app}} opens every app in its matching style. The command finder ({{key:Mod+K}}) shows each command's key. See [Menus, keys and mouse](topic:menus-keys-and-mouse).
- **A wider Settings dialog,** in two columns, so more fits without scrolling. See [BimCraft settings](topic:settings).
- **Floating and bottom panels fit the window.** A floating panel stays above the status bar and shrinks with a small window; on a short window a panel docked at the bottom leaves the view room. See [Arrange the panels and use a second monitor](common:arrange-panels).
- **Phones and fold phones.** On a screen up to 760 px wide, or a phone held sideways, the view keeps the screen: panels open as sheets from a bar at the bottom, and the toolbar and the {{ui:Draw}} bar are one row each that scrolls sideways. {{ui:Tools}} hides the toolbar and {{ui:Move}} puts it at the bottom. Turning the phone, or opening and closing a fold phone, keeps your work, the selection and an open dialog. The app bar's {{ui:More}} menu holds the version, Find and the links. Phones work, but BimCraft is made for larger screens.
- **Touch tablets.** On a touch screen each toolbar is one row that scrolls sideways instead of wrapping onto several lines.
- **Spanish** stays a machine-made draft that has not been reviewed yet. Help topics are in English.

## Not in this version yet

- A 3D preview of project files: export IFC from them. See [From a project file to BimCraft through IFC](topic:rvt-to-ifc).
- Curved walls, copying drawn walls, doors and windows, and changing walls another program made.
- Dormers, curved roofs and spiral stairs; dimensions and tags on elevations and sections.
- Textures in the live 3D view (rendered pictures show them), E57 point clouds, and saving a point cloud's place with the model.
- In a live session, one model per session.
- [What is not built yet](topic:not-built-yet)
