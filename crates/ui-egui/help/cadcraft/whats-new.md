---
title: What's new in 0.5
summary: Drawing together live in CadCraft Studio, working inside viewports with layers frozen per viewport, construction lines, a UCS and a turned view, tangent circles, buttons, keys and menu styles of your own, dynamic blocks that stay dynamic, pictures, clipping, point clouds, map layers, steel sections and standard parts, three new add-ons, and a layout for phones.
section: start
order: 5
keywords: whats new, what's new, release notes, changelog, changes, 0.5, version, new features, live, studio, viewport, vplayer, freeze in viewport, xline, ray, ucs, turned view, tangent, my tools, shortcuts, menu style, ribbon, drafting classic, dynamic blocks, pictures, xclip, text mask, point cloud, map layers, steel, standard parts, apparel, hvac, piping, phone, fold phone, desktop, signing
---

This is what changed in CadCraft 0.5, compared with 0.4. The list is grouped by area, with the most important first.

## New

### Drawing together, live

- **Several people in one drawing.** In a CadCraft Studio live session everyone's edits show at once. When two people change different properties of one object, both changes are kept; when they change the same one, the later change wins. See [Work on one drawing together, live](topic:work-live).
- **See who is where.** Other people's cursors show with their names and the command they are running, their selections are outlined in their color, and an object someone holds reads "Ana is editing".
- **Your own undo.** {{key:Mod+Z}} undoes your own last command and leaves everyone else's work alone.
- **Locks and holds.** The host can lock objects or a whole layer ({{cmd:LIVELOCK}}, {{cmd:LIVEUNLOCK}}). Anyone in the session can hold a layer or an area while they work there ({{cmd:LIVEHOLD}}: `L` a layer, `A` two corners, `R` releases): other people's edits there are taken back and they are told who is working there.
- **Join with the same file.** If you have the very file the host opened, join from your own CadCraft window in Studio with {{ui:Join here}}: only the changes made since it was opened travel, so a large drawing is ready at once.
- **Assistants as their own participants.** In Studio, an AI assistant connected to your window joins the session as "Assistant (via your name)", let in by the host, with its own undo.
- **Join from a browser** on the same network by scanning the picture under the host's code. CadCraft asks for your name and opens the host's drawing.

### Sheets and viewports

- **Work inside a viewport.** Double-click inside a viewport ({{cmd:MSPACE}}, `MS`) to edit the model through it; double-click outside it ({{cmd:PSPACE}}, `PS`) to go back to the sheet. Pan and zoom change the viewport's view unless it is locked. See [Set up a sheet with a title block and viewports](topic:sheets-and-viewports).
- **Freeze a layer in one viewport.** With a viewport selected or entered, the Layers panel freezes a layer in that viewport only. {{cmd:VPLAYER}} does several layers and viewports at once. Layers a DWG freezes in one viewport are left out of that viewport on screen and in plots, and kept when you save.
- **Viewport layer overrides** from a DWG (a layer's color, linetype, lineweight and transparency in one viewport) are drawn, plotted and saved back in DWG.
- **View labels** that follow a viewport's view name ({{cmd:VIEWLABEL}}, `VL`), and {{cmd:VPVIEW}} sets a viewport from a named view.
- **Sheet numbers.** Sheet tabs can show numbers only ({{ui:Settings}} › {{ui:CadCraft}} › {{ui:Sheet tabs}}), and a sheet set can number its sheets from fields with a rule such as `{DISCIPLINE}-{LEVEL}{SEQ:2}`. See [Sheet sets](topic:sheet-sets).

### Drawing tools

- **Construction lines and rays.** {{cmd:XLINE}} (through points, horizontal, vertical, at an angle, bisecting or offset) and {{cmd:RAY}}. They cross the whole view, snap, trim into rays and lines, and stay out of Zoom Extents. They open from and save to DWG and DXF. See [Construction lines and rays](topic:construction-lines).
- **A UCS and a turned view.** {{cmd:UCS}} and {{cmd:PLAN}}; {{ui:View}} › {{ui:More}} turns the view a quarter turn left or right and back, without moving anything in the drawing. Typed points, ortho, polar tracking, the grid, new text and linear dimensions follow the UCS. The UCS, named UCSs, the view's turn and turned sheet viewports open and save in DWG and DXF. See [Turn the view and use your own coordinates (UCS)](topic:ucs-and-turned-view).
- **Tangent circles.** {{cmd:CIRCLE}} `T` (Tan, Tan, Radius), and {{ui:Draw}} › {{ui:More}} › Circle: Tan, Tan, Tan. See [Command reference](topic:commands).
- **Tangent object snap** in {{cmd:LINE}}, {{cmd:ARC}} and two- and three-point circles: a line from tangent to tangent touches both circles. See [Snap to points on objects](topic:object-snaps).
- **Compare settings** ({{cmd:COMPARESETTINGS}}, `CMPSET`): two dimension styles, text styles, layers, sheets or unit settings side by side, in one drawing or across two, with the differences marked and a value copied across in one click. See [Compare settings](topic:compare-settings).

### Your own buttons, keys and menu styles

- **My tools.** {{ui:Settings}} › {{ui:CadCraft}} › {{ui:My tools}} makes toolbar buttons that run macros in the classic drafting macro syntax (`^C^C`, `;`, `\` to wait for a click), each with an icon and a key. Export and import them, or import the toolbars, panels and keys of a `.cuix` menu file; commands CadCraft lacks are listed, never guessed. See [Buttons of your own (My tools)](topic:my-tools).
- **Keys for any command.** {{cmd:SHORTCUTS}} ({{ui:View}} › {{ui:More}} › {{ui:Keyboard shortcuts…}}) gives a key a whole command line, such as `ZOOM E`. Keys CadCraft already uses are refused.
- **Menu styles.** {{ui:Settings}} › {{ui:CadCraft}} › {{ui:Menus, keys and mouse}} offers three styles: {{ui:CadCraft}} (the toolbar you know), {{ui:Drafting classic}} (a ribbon over CadCraft's own command line and aliases; the middle button pans and a right-click is Enter) and {{ui:Ribbon}} (plain task tabs). Keys you made in 0.4 are carried once into your own style, "My CadCraft keys", and work as before. See [Menus, keys and mouse](topic:menus-keys-and-mouse).

### Dynamic blocks

- Dynamic blocks from a DWG **open under their own names**, not as `*U8` and the like.
- **Switch a visibility state** in Properties › {{ui:Custom}} or with the block's visibility grip, in one undo step. Their other values show read only.
- **Edit visibility states in the block editor** ({{cmd:BVSTATE}}, {{cmd:BVHIDE}}, {{cmd:BVSHOW}}) on blocks that already have a visibility parameter.
- **They stay dynamic** when you save as DWG in the drawing's own version. In a DXF from another program, each block shows only its current state. See [Dynamic blocks](topic:dynamic-blocks).

### Pictures, text masks, clipping and links

- **Pictures.** Place PNG and JPEG pictures ({{cmd:IMAGEATTACH}}, `IAT`), clip them ({{cmd:IMAGECLIP}}), and give a DWG's pictures their files ({{cmd:IMAGE}}). They are drawn on screen, saved as pictures in DWG, and printed in PDFs at their own resolution. See [Pictures in a drawing](topic:pictures).
- **A background mask behind text** ({{cmd:TEXTMASK}}) hides the lines under a label on screen and in PDFs. See [Add text and labels](topic:text).
- **Clip blocks and linked drawings** ({{cmd:XCLIP}}) to a rectangle or a polygon, on screen, in plots and in DWG and DXF 2000. Binding a link can keep only its clipped part.
- **Linked drawings keep their layer settings** through reload, a DWG save and nesting, and their layers keep names such as `Plan|Walls`. See [Files: what opens, what saves](topic:files).

### Point clouds and map layers

- **Point clouds** ({{cmd:POINTCLOUDATTACH}}): LAS, LAZ, XYZ or PTS under the drawing. A building-size scan starts as a plan slice to trace as-built walls, with Node snap on the points; the cloud shows on the 3D tab too. See [Trace as-built lines over a point cloud](topic:point-clouds).
- **Map layers under model space** ({{cmd:MAPLAYERS}}, `ML`): street maps, aerial tiles, web map services or a picture with a world file, placed by the drawing's coordinate system. Nothing is fetched until you switch the internet on. See [Map layers under the drawing](topic:map-layers).

### The library's Standard tab

- **Steel sections** (US W, S, C, L, HSS and pipe shapes; European IPE, HEA, HEB and UPN) placed as a section, an elevation or a 3D member ({{cmd:STEELMEMBER}}).
- **Standard parts** (ISO and ASME bolts, nuts, washers, pins, bearings, keys and gears) placed as blocks with size, standard and material as attributes, and a parts list to draw or download ({{cmd:PARTSLIST}}). See [Place steel sections and standard parts](topic:steel-and-standard-parts).

### Add-ons

- **Three new add-ons, switched off at first:** Apparel (pattern pieces, seam allowance, sizes and grading, tiled printing, DXF-AAMA), HVAC (duct runs, sizing and a duct schedule) and Piping (pipe runs, fittings, valves, slope for drains and a pipe schedule). Switch them on in {{ui:Settings}} › {{ui:General}} › {{ui:Add-ons}}; each has its own help topics. Duct and pipe schedules can go to TakeoffCraft.
- **Electrical** saves part of a schematic as a design block to place again with new tags and wire numbers ({{cmd:ELSAVEBLOCK}}, {{cmd:ELDESIGNBLOCK}}), and uses another symbol library's blocks as Electrical symbols ({{cmd:ELIMPORTSYM}}).

## Changed

- **The 3D tab** carries ModelCraft's newer modeling core: {{cmd:PRESSPULL}} can land level with a parallel face you point at, and the camera fits small parts. See [Model in 3D](topic:3d-solids).
- **Short windows.** When the drawing would get too short, the toolbar drops its labels, then becomes one row that scrolls sideways, so the drawing and its sheet tabs stay in view. Long sheet names are cut short on their tabs; rest the pointer on a tab for the whole name.
- **Phones.** The drawing keeps about two thirds of the screen, and the command line and the touch key row stay above the on-screen keyboard.
- Messages about damaged or unsupported DWG and DXF files appear in your language.

## Fixed

- Viewports on sheets showed every layer. Layers frozen in one viewport now stay frozen there, on screen, in plots and in saved files.
- The sheet tabs sat under the toolbar and could not be clicked on short laptop windows. Every tab is reachable now.
- Construction lines in an opened DWG disappeared without a word. They now open, draw and save.
- Turned sheet viewports were shown square to the sheet. They are drawn turned now.
- Pictures showed only as a frame with the file name. They are drawn now.
- Linked drawings' layers were renamed `Plan$0$Walls` when saved. They stay `Plan|Walls` while the link is a link.
- {{key:Mod+S}} right after {{ui:Save as}} could still save in the old format.
- Screen readers now read each Properties field with its label.

## In every app

- **CadCraft Studio is the desktop app.** From 0.5 the desktop download is CadCraft Studio: all eight apps in one install. The separate installer for each app stops. CadCraft still runs in the browser, and as a browser edition you can download.
- **Installers are not signed yet.** Code signing comes with 0.6. On Windows, if SmartScreen stops the installer, choose {{ui:More info}}, then {{ui:Run anyway}}. The macOS disk image (Apple silicon and Intel, macOS 13 or later) is not signed or notarized: the first time, right-click the app and choose {{ui:Open}}. iPad, Android and iPhone come with 0.6.
- **Work together live.** In CadCraft Studio, {{ui:Live}} shares the file you have open: teammates on the same network join with a short code and edit with you, each with their own undo. Sessions on your own network are free, for everyone. Invite links over the internet aren't open yet. See [Work together live](common:work-together-live) and [Work on one drawing together, live](topic:work-live).
- **Menus, keys and mouse.** In {{ui:Settings}} › {{ui:CadCraft}} › {{ui:Menus, keys and mouse}}, pick {{ui:CadCraft}}, {{ui:Drafting classic}} or {{ui:Ribbon}}, give any command your own key, move commands between menus, set the wheel and the mouse buttons, and export or import a style as a `.ccinput.json` file. {{ui:Use this style in every app}} opens every app in its matching style. See [Menus, keys and mouse](topic:menus-keys-and-mouse).
- **A wider Settings dialog,** in two columns, so more fits without scrolling. See [CadCraft settings](topic:settings).
- **Floating and bottom panels fit the window.** A floating panel stays above the command line and the status bar and shrinks with a small window; on a short window a panel docked at the bottom leaves the drawing room. See [Arrange the panels and use a second monitor](common:arrange-panels).
- **Phones and fold phones.** On a screen up to 760 px wide, or a phone held sideways, the drawing keeps the screen: panels open as sheets from a bar at the bottom, and the toolbar is one row that scrolls sideways. {{ui:Tools}} hides the toolbar and {{ui:Move}} puts it at the bottom. Turning the phone, or opening and closing a fold phone, keeps your work, the selection and an open dialog. The app bar's {{ui:More}} menu holds the version, Find and the links. Phones work, but CadCraft is made for larger screens.
- **Touch tablets.** On a touch screen each toolbar is one row that scrolls sideways instead of wrapping onto several lines.
- **Spanish** stays a machine-made draft that has not been reviewed yet. Help topics are in English.

## Not in this version yet

- Changing a dynamic block's distances, angles or flips, and making a new visibility parameter. A dynamic block stays dynamic only when saved as DWG in its own version.
- View labels save as plain text, and a viewport's view name is not written to files yet.
- Clipped references are still picked and zoomed by their whole size; no inverted clips.
- Viewport layer overrides are not kept in DXF, and pictures in DXF save as their frames.
- Point clouds: no E57, and a cloud is not saved with the drawing or plotted. Map layers are kept in this browser and are not plotted.
- Properties shows world coordinates, not UCS ones.
- From 0.4, still: hatches that follow their boundary, gradients, fonts and formatting inside text, splines (they open as polylines), and path or associative arrays.
- [Not built yet](topic:not-built-yet)
