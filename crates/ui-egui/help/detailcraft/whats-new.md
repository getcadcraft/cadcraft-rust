---
title: What's new in 0.5
summary: A profile catalog of 1,259 sizes with S shapes, pipe and UPN channels, detailing one model together live in CadCraft Studio, menu styles with your own keys and mouse, Move on the M key, and a layout for phones.
section: start
order: 5
keywords: whats new, what's new, release notes, changelog, changes, 0.5, version, new features, profiles, catalog, sizes, s shapes, pipe, upn, live, together, studio, menu style, ribbon, drafting classic, steel ribbon, keys, mouse, move, phone, fold phone, tablet, settings
---

This is what changed in DetailCraft 0.5, compared with 0.4. Models still save in file format 2, so DetailCraft 0.4 and 0.5 open each other's files. A model that uses a size new in 0.5, such as an S shape, names a profile 0.4 does not have.

## New

### A much larger profile catalog

- **From 45 to 1,259 sizes.** W, S, C, equal-leg angles, rectangular and square HSS, round HSS and pipe from the AISC shapes database, and IPE, HEA, HEB and UPN from EN 10365. See [Profile catalog](topic:profile-catalog).
- **S shapes, pipe and UPN channels are new.** S shapes take holes, copes, end and base plates, NC files and shop drawings like the other I-shapes. UPN works like C channels, and pipe like round HSS.
- **Older models build the same parts.** The 45 sizes of 0.4 come out exactly as before. A model with a custom profile named like a size that is now in the catalog still opens, and its own profile wins.
- **KISS export** names S shapes `S` and UPN channels `C`. See [KISS export](topic:kiss-export).

### Detail one model together, live

- **Several people in one model.** In a CadCraft Studio live session, members, connections, footings, stairs, bolts and issues merge item by item. Two people changing different settings of one member both keep theirs; the same setting goes to the later change.
- **A connection on a member someone just deleted** is taken back, and the status bar says why.
- **Undo undoes your own last edit** and leaves everyone else's alone.
- Built parts, piece marks and the schedule are worked out in every window from the same model, so they agree. Only the host saves the model.
- See [Detail one structure together, live](topic:work-live).

### Menus, keys and mouse

- **Three menu styles.** {{ui:DetailCraft}} (the toolbar you know), {{ui:Drafting classic}} (a small ribbon and a command line: type BEAM, COLUMN, PLATE, ENDPLATE and more, then press {{key:Enter}}) and {{ui:Steel ribbon}} (tabs for modeling, detailing, drawings and fabrication).
- **Your own keys and mouse.** Give any command your own key, move commands between ribbon tabs, and set the wheel and the buttons that pan and orbit. See [Menus, keys and mouse](topic:menus-keys-and-mouse).

### Editing

- **Move on the M key.** {{ui:Edit}} › {{ui:More}} › {{ui:Move}} ({{key:M}}) moves the selection by two picked points, and like every command it can take another key in your own style. See [Select by window, move, rotate, mirror, change many](topic:select-and-change).

## Improved

- The help topics [Profile catalog](topic:profile-catalog), [Getting started with DetailCraft](topic:getting-started) and [Coming from other detailing software](topic:coming-from) describe the larger catalog. [Profile catalog](topic:profile-catalog) now says correctly that channels, HSS, pipe and angles get NC files.
- The help on AI assistants no longer names particular assistant programs; any program that speaks MCP works, as before.
- Messages from the DWG writer used for DWG sheets now come from the language file.

## In every app

- **CadCraft Studio is the desktop app.** From 0.5 the desktop download is CadCraft Studio: all eight apps in one install. The separate installer for each app stops. DetailCraft still runs in the browser, and as a browser edition you can download.
- **Installers are not signed yet.** Code signing comes with 0.6. On Windows, if SmartScreen stops the installer, choose {{ui:More info}}, then {{ui:Run anyway}}. The macOS disk image (Apple silicon and Intel, macOS 13 or later) is not signed or notarized: the first time, right-click the app and choose {{ui:Open}}. iPad, Android and iPhone come with 0.6.
- **Work together live.** In CadCraft Studio, {{ui:Live}} shares the file you have open: teammates on the same network join with a short code and edit with you, each with their own undo. Sessions on your own network are free, for everyone. Invite links over the internet aren't open yet. See [Work together live](common:work-together-live) and [Detail one structure together, live](topic:work-live).
- **Menus, keys and mouse.** In {{ui:Settings}} › {{ui:DetailCraft}} › {{ui:Menus, keys and mouse}}, pick a menu style ({{ui:DetailCraft}}, {{ui:Drafting classic}} or {{ui:Steel ribbon}}), give any command your own key, move commands between menus, set the wheel and the mouse buttons, and export or import a style as a `.ccinput.json` file. {{ui:Use this style in every app}} opens every app in its matching style. See [Menus, keys and mouse](topic:menus-keys-and-mouse).
- **A wider Settings dialog,** in two columns, so more fits without scrolling. See [DetailCraft settings](topic:settings).
- **Floating and bottom panels fit the window.** A floating panel stays above the status bar and is kept inside the window when it gets smaller, and a panel docked at the bottom never takes the whole drawing. See [Arrange the panels and use a second monitor](common:arrange-panels).
- **Phones and fold phones.** On a screen up to 760 px wide, or a phone held sideways, the model keeps the screen: panels and the reports ({{ui:Problems}}, {{ui:Bill of materials}}, {{ui:Bending schedule}}) open as sheets from a bar at the bottom, and the toolbar is one row that scrolls sideways. {{ui:Tools}} hides the toolbar and {{ui:Move}} puts it at the bottom. Turning the phone, or opening and closing a fold phone, keeps your work, the selection and an open dialog. The app bar's {{ui:More}} menu holds the version, Find and the links. Phones work, but DetailCraft is made for larger screens.
- **Touch tablets.** On a touch screen each toolbar is one row that scrolls sideways instead of wrapping onto several lines.
- **Spanish** stays a machine-made draft that has not been reviewed yet. Help topics are in English.

## Not in this version yet

- Angles with unequal legs are not in the catalog; an angle's two legs are drawn alike.
- S shapes, C and UPN channels are drawn with parallel flanges of the average thickness, not sloped flanges.
- In a live session, grids, levels and phases travel whole (the later change wins), and DetailCraft shows no other people's cursors or "is editing" labels yet.
- Still from 0.4: shop drawings of channels, angles and tubes; weld symbols other than fillets; portrait sheets, A0 and custom sizes; clip angles, slotted holes, and skewed or sloped end plates; straight stairs only.
- [Not built yet](topic:not-built-yet)
