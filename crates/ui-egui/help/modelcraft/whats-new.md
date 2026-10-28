---
title: What's new in 0.5
summary: Import .skp models with their textures, check a model for 3D printing, render pictures with sky, sun, materials and lamps, entourage, push/pull to a face's level, menu styles, live sessions in CadCraft Studio, and phones.
section: start
order: 5
keywords: whats new, what's new, release notes, changelog, changes, 0.5, version, new features, skp, import, textures, 3d printing, stl, 3mf, render, picture, path tracing, materials, lamps, lights, entourage, trees, people, cars, push pull, on face, menu styles, keys, mouse, live, studio, phone, fold phone, tablet, settings
---

This is what changed in ModelCraft 0.5, compared with 0.4.

## New

### Files

- **Import .skp models.** {{ui:Import}} (or drop the file on the view) reads SketchUp files with ModelCraft's own reader, in the page: faces, groups, components, colors, tags (shown or hidden as in the file), hidden things and soft edges. A component painted different colors in different places stays one component. The import is one undo step, and the status bar says what came in and what did not. See [Bring in an SKP model](topic:import-sketchup-files).
- **Textures come in.** Faces painted with a textured material in a `.skp` file, and base color textures in a `.glb` file, keep their texture. The ModelCraft file keeps the images when you save. See [File formats in detail](topic:formats).
- **Check for 3D printing.** {{ui:Export}} › {{ui:Check for 3D printing (STL or 3MF)…}} checks the selection, or everything shown, for holes, edges shared by three or more faces, faces turned the wrong way, shells inside out and shells that pass through themselves. It shows the size, the volume and the overhang, then downloads STL or 3MF of exactly what it checked. 3MF is a new export, with colors. See [Get a model ready for 3D printing](topic:print-3d).

### Pictures

- **Render a picture.** {{ui:Export}} › {{ui:Render a picture…}} makes a daylight picture of the view: a sky, the sun, soft shadows, shading in corners, glass and metal looks, four skies, three qualities and sizes up to 4K. {{ui:Stop}} keeps the picture so far. It is made on your own computer; nothing is uploaded. The settings are saved in the file. See [Render a picture of the model](topic:render).
- **High quality.** {{ui:Mode}} › {{ui:High quality (path traced)}}: light bounces round rooms, glass bends light, and metal and mirrors reflect the model. It builds up step by step within a time limit you choose.
- **Materials.** The {{ui:Materials}} section under {{ui:Colors}} has 34 materials in 11 groups: brick and block, siding, roofing, wood, stone, tile, concrete, stucco, grass and gravel, glass and metal. Paint them on like a color. The {{ui:Shaded}} style shows them at real size, level on walls and down roof slopes. They are drawn by ModelCraft and free to use (CC0). See [Paint with colors and materials](topic:paint).
- **Lamps.** The {{ui:Lights}} section adds a {{ui:Lamp}}, a {{ui:Spotlight}} or a {{ui:Light panel}}, with brightness, color, beam and size. Lamps are saved in the file and light the pictures, at night too. See [Light a model with lamps](topic:lights).
- **Entourage.** The [starter library](topic:starter) has trees, shrubs, a hedge, a potted plant, people and cars for site plans and pictures (CC0).

### Working together

- **Model together, live.** In CadCraft Studio several people edit one model at once. Each person works in their own group, and moving placed groups never clashes. Loose geometry, not in a group, is reshaped by one person at a time. See [Model together, live](topic:work-live).

## Improved

- **DWG site plans name their dynamic blocks.** Each comes in as the drawing shows it, and the status bar lists the states. See [Model on a site plan from a DWG or DXF](topic:site-plan).
- **Clearer key names.** The key choice under {{ui:Keys}} and in {{ui:Settings}} now reads "Push-pull" and "Modeler". See [Keys and mouse actions](topic:keyboard).

## Fixed

- **Push/pull can stop level with another face.** While you push or pull, point at a face parallel to the one you are moving, such as the top of the box a post stands on: the tip says "On face", and the face ends level with it. When the face ends flush again, its sides go away and the outline you drew stays on the face. See [Push and pull faces into solids](topic:push-pull#pull-to-the-height-of-something-else).
- **Very small models fit the view.** Fitting the view to a model under 10 cm, such as a circuit board, framed it as if it were 1 m across. It now fits models down to 1 cm.

## In every app

- **CadCraft Studio is the desktop app.** From 0.5 the desktop download is CadCraft Studio: all eight apps in one install. The separate installer for each app stops. ModelCraft still runs in the browser, and as a browser edition you can download.
- **Installers are not signed yet.** Code signing comes with 0.6. On Windows, if SmartScreen stops the installer, choose {{ui:More info}}, then {{ui:Run anyway}}. The macOS disk image (Apple silicon and Intel, macOS 13 or later) is not signed or notarized: the first time, right-click the app and choose {{ui:Open}}. iPad, Android and iPhone come with 0.6.
- **Work together live.** In CadCraft Studio, {{ui:Live}} shares the file you have open: teammates on the same network join with a short code and edit with you, each with their own undo. Sessions on your own network are free, for everyone. Invite links over the internet aren't open yet. See [Work together live](common:work-together-live) and [Model together, live](topic:work-live).
- **Menus, keys and mouse.** In {{ui:Settings}} › {{ui:ModelCraft}} › {{ui:Menus, keys and mouse}}, pick a menu style ({{ui:ModelCraft}}, {{ui:Push-pull}} or {{ui:Modeler}}), give any command your own key, move commands between menus, set the wheel and the mouse buttons, and export or import a style as a `.ccinput.json` file. {{ui:Use this style in every app}} opens every app in its matching style. See [Menus, keys and mouse](topic:menus-keys-and-mouse).
- **A wider Settings dialog,** in two columns, so more fits without scrolling. See [ModelCraft settings](topic:settings).
- **Floating and bottom panels fit the window.** A floating panel stays above the status bar and shrinks with a small window; on a short window a panel docked at the bottom leaves the view room. See [Arrange the panels and use a second monitor](common:arrange-panels).
- **Phones and fold phones.** On a screen up to 760 px wide, or a phone held sideways, the view keeps the screen: panels open as sheets from a bar at the bottom, and the toolbar is one row that scrolls sideways. {{ui:Tools}} hides the toolbar and {{ui:Move}} puts it at the bottom. The view cube sits small in a corner. Turning the phone, or opening and closing a fold phone, keeps your work, the selection and an open dialog. The app bar's {{ui:More}} menu holds the version, Find and the links. Phones work, but ModelCraft is made for larger screens.
- **Touch tablets.** On a touch screen each toolbar is one row that scrolls sideways instead of wrapping onto several lines.
- **Spanish** stays a machine-made draft that has not been reviewed yet. Help topics are in English.

## Not in this version yet

- Saving `.skp` files, and text, dimensions, scenes, styles and section planes from them. Faces and edges with a tag of their own come in untagged.
- Textures from Collada files, your own image materials, and textures in exported files (they get each material's color and name).
- Trim, Split and Outer shell; more than one section cut at a time; DWG and DXF layers as tags.
- A rendered walkthrough video.
- [Not built yet](topic:not-built-yet)
