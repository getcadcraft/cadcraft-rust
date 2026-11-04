# Gaps against CadCraft 0.5

Last updated 2026-11-04 · Change: BimCraft finished (M7.20–M7.24: the Build menu's residential tools, terrain / pad / paving, Sheets, Takeoff, equipment schedule, credits; sign-in and Check for updates stay web-limited); ModelCraft finished on main (M6.27–M6.35: film, render lamps and glass, entourage, pictures as textures, terrain, starter library, modeler keys, view marks, Settings › ModelCraft, view cube, touch, m3d_tools fuzz finds); BimCraft Library panel, swap / update, copy / paste of placements and elements (M7.18–M7.19); BimCraft library item adapters (M7.17); residential sync (M7.16); owner decisions (add-ons out of scope; map servers and grid downloads web only); the 0.7 look (M17: theme, Archivo, app bar, CadCraft's window, the start screen; CadCraft's split panes, corner indicator, My tools with CUIx, SHEETSET on the web (M4.57–M4.60); BimCraft's DWG view extras in the viewer: equipment schedule, 3D with picking and section, dynamic blocks, similar-name pictures, the desktop project folder, help reader context (M3.9–M3.10); CadCraft drafting (M4.52–M4.56: dense linetypes on screen, view cube steps, Settings › CadCraft and the Properties head, input styles, LWDISPLAY); CivilCraft speed (M11.23–M11.25: point files, GeoJSON read / write, GIS import / export faster than 0.5); the viewer's details, Settings, help panel, es-MX in every app (M3.6–M3.8); CivilCraft's remaining 0.5 pieces (M11.18–M11.22: superelevation, drainage, legal descriptions, settings, import dialog, draft bar, drive / sun, GIS layers and export, coordinates, map layers); point clouds under the drawing (M4.50) and SWBOUT (M4.51): the command table is complete; CadCraft's 3D tab at parity under random 3D input, model read with the file, view cube compass, touch (M4.27–M4.29); web speed (M15.3: web allocator, V8 trig in the display list); recovery copies and Recover your work? (M4.46); CadCraft's ribbon, toolbar and menus (M4.45); CadCraft's LISP (M4.40); Plot Open to print, Properties sections, window settings, lasso, VIEWBASE, SHEETSET (M4.30–M4.34); CadCraft's 3D tab (M4.25–M4.26); GeoTIFF tiles under one pixel refused (civil_raster fuzz timeout, M11.17); 3D commands and the 3D model in files (M4.24); constraints and parameters (M4.23); LIBRARY, INSERT File / Library (M4.22); PUBLISH and pictures in plots (M4.21); ModelCraft scenes' Play (M6.26); ModelCraft Render a picture (M6.25); ModelCraft Lights panel (M6.24); ModelCraft material textures (M6.23); speed S.4–S.8 (XATTACH, whole-drawing edits and undo on Carey, TakeoffCraft copy + paste, ModelCraft undo); ModelCraft sun shadows (M6.22); ModelCraft components that stretch (M6.21); local-time dates in every ported writer (M16.8); GeoTIFF reads capped at 1 GB (civil_raster fuzz OOM, M11.16); speed S.1–S.3 (marked PDF, section sheets PDF, 3D GLB faster than 0.5); ModelCraft solids, storeys, drawing axes (M6.20); ModelCraft copy / paste, duplicate, flip, 3D text (M6.19); TakeoffCraft 3D view (M9.23); TakeoffCraft revisions (M9.22); TakeoffCraft typical groups and markups (M9.21); TakeoffCraft arcs / circles, corner drags, multi-select, clipboard (M9.20); MarkupCraft BCF, compare overlay / line-up / sets (M9.19); MarkupCraft cut-outs, spaces, legends, links, punch list (M9.18); MarkupCraft picture / signature stamps and tool chest (M9.17); MarkupCraft clipboard, Markups list, replies, sheet rename, Pages menu (M9.16); visibility states in BEDIT (M4.20); pictures (M4.19); linked drawings and XCLIP (M4.18); BimCraft library items placed from the plan (M7.15), plan grips (M7.14); UCS and view turns, POLARANG, COUNT (M4.17); sheets and viewports, named views (M4.16); clipboard, QSELECT / SELECTSIMILAR / FIND, UNITS (M4.15); CivilCraft sheets, mass haul, WL.20 / WL.23, Properties edits, lasso (M11.11–M11.14); drafting edits share each object's extra data (M4.14); drafting commands, Properties panel and plotting (M4.10–M4.12); ModelCraft panels (M6.17); per-app gaps below. Ranked; shrink as features land.

1. **Drafting vs 0.5** (after M4.10–M4.12). 0.5's command table (`commands.js`, `layertools.js`, `propcmds.js`,
   `pcommands.js`, `commands3d.js`, `pictures-cmd.js`, `xclip.js`, `cloud-cad.js`, `flatview.js`: 190 commands), ranked by
   everyday use. **In, at kdiff parity** (same prompts, printed lines, previews, drawing, undo labels): LINE … ZOOM (M4);
   HATCH (pick-inside boundaries with islands, Select objects, pattern / scale / angle), DIMLINEAR DIMALIGNED DIMANGULAR
   DIMRADIUS DIMDIAMETER DIMCONTINUE DIMBASELINE, DIMSCALE DIMTXT DIMASZ DIMDEC DIMGAP DIMEXE DIMEXO DIMLUNIT, FILLET
   CHAMFER (polyline, trim, multiple, Shift), EXPLODE, STRETCH, ARRAY ARRAYRECT ARRAYPOLAR, BREAK BREAKATPOINT, LENGTHEN,
   JOIN, MATCHPROP, XLINE RAY, MTEXT, REVCLOUD, MLEADER, TEXTEDIT, BLOCK ATTDEF INSERT BEDIT BCLOSE PURGE, DUPLICATE
   SELECTALL DESELECT SELECT, DIST AREA MEASUREGEOM ID LIST, LAYISO LAYUNISO LAYOFF LAYFRZ LAYMCUR LAYON LAYTHW LAYMRG
   LAYDEL RENAME LINETYPE LTSCALE LWEIGHT, PROPERTIES PROPERTIESCLOSE QPMODE, PLOT (M4.10–M4.12); COPYCLIP CUTCLIP
   COPYBASE PASTECLIP PASTEORIG (one clipboard for every drawing the window opens; Ctrl+C / X / V, Ctrl+Shift+V), QSELECT
   SELECTSIMILAR FIND (with the window's Quick select and Find and replace dialogs), UNITS (M4.15); LAYOUT MVIEW VPORTS
   MSPACE PSPACE VPLAYER VIEWLABEL NAMEDVIEW, the sheets they make saving byte-identically in DWG 2000–2018, DXF 2000 and
   DXF R12 (M4.16; in the window MSPACE shows the model through the viewport's window, and VPORTS in the model says split
   panes are the browser's). UCS PLAN TURNLEFT TURNRIGHT TURNWORLD, LWDISPLAY POLARANG, COUNT (M4.17; the window turns
   its view to the UCS and has polar tracking on F10; LWDISPLAY says lineweights on screen are the browser's: the window
   draws one width). XATTACH XREF XCLIP (M4.18; clipped references clip on screen, in plots and in extents; a linked
   drawing comes through the window's file picker). IMAGEATTACH IMAGE IMAGECLIP (M4.19; pictures draw in the window and
   the viewer, PNG and JPEG). BVSTATE BVHIDE BVSHOW (M4.20; BEDIT opens a dynamic block on one of its visibility states,
   BCLOSE keeps it dynamic and its references follow renamed and deleted states). PUBLISH (M4.21; pictures plot,
   with the Plot dialog's dpi cap). LIBRARY, INSERT File and Library (M4.22; the Library panel has Files… and the
   credits; LibraryCraft, other feeds and the Standard parts are getcadcraft.com's). The parametric set: GEOMCONSTRAINT
   DIMCONSTRAINT and their GC* / DC* commands, DELCONSTRAINT AUTOCONSTRAIN PARAMETERS -PARAMETERS CONSTRAINTBAR
   DCDISPLAY, constraints through MIRROR / TRIM / the clipboard and in DWG / DXF files, the Parameters panel (M4.23).
   The 3D commands EXTRUDE (Taper, Path, Delete originals) SWEEP REVOLVE BOX WEDGE CYLINDER EXPORT3D STLOUT OBJOUT
   GLBOUT, the 3D model read from and saved to DWG / DXF (block CADCRAFT_3D, 3DFACEs on CC-3D) (M4.24). The 3D tab
   with PRESSPULL 3DORBIT VIEW VSCURRENT UNION INTERSECT SUBTRACT and MOVE COPY ROTATE ERASE ZOOM on solids, clicks in
   3D for the 2D commands, the drawing-units choice in Properties that scales the solids, the view cube's compass and
   touch (M4.25–M4.29; the tab's point cloud: M4.50). The Plot dialog's Open to print (M4.30); the
   Properties panel's Custom (dynamic blocks), Attributes, Library, attribute definitions, viewport scale, sheet,
   current settings, View, Misc, point picks and Quick Properties (M4.31; P.12's choice of fields, PICKADD and
   Auto-hide: M4.54); SPLIT LINKVIEWS OPENIN3D LASSOMODE SHORTCUTS COMPARESETTINGS and lasso selection (M4.32;
   the split panes themselves: M4.57; OPENIN3D opens the window's 3D tab); VIEWBASE (M4.33); SHEETSET with its panel,
   publish, Update drawings, sheet list and archive (M4.34; on the web through the browser's folder picker or its
   read-only fallback: M4.60; drag-and-drop of rows is Alt+↑ / ↓). LISP: APPLOAD, (command …), DCL dialogs, ALIASEDIT REINIT DELBLOCKS
   (M4.40); the ribbon, toolbar and menus (M4.45); recovery copies and Recover your work? (M4.46); POINTCLOUDATTACH
   POINTCLOUD with the cloud in the 2D view, its snap and the 3D tab (M4.50; the picture is made again on every pan /
   zoom), SWBOUT (M4.51) — the whole command table is in. LAYER NEW OPEN SAVE SAVEAS DXFOUT HELP are the window's own
   buttons. Linetypes dash on screen as on 0.5's canvas (20,000 pieces per run, M4.52); the 3D tab's view cube
   steps (M4.53); Settings › CadCraft, the Properties head's PICKADD / Auto-hide / field choice (M4.54); input styles
   with Customize, Export / Import, keys and mouse (M4.55); LWDISPLAY on screen (M4.56); split panes with VPORTS in
   the model, SPLIT, LINKVIEWS, saved in the VPORT table (M4.57); Settings › CadCraft's corner indicator (M4.58); My
   tools with CUIx import (M4.59). A DWG's linked drawings are
   not looked for next to it when it opens for editing — 0.5 does not either (XREF Reload takes the file there too).
   Also missing: a pane showing another open drawing (0.5's Tile; the window holds one drawing), My tools' SVG icons
   drawn (kept, shown as the star), the point cloud picture's overscan (made again on every pan / zoom: a shifted
   picture is not provably a full redraw: docs/progress.md, 2026-11-03), "Use this style in every app"; the
   ribbon's Add-ons tab shows My tools only: 0.5's JavaScript add-ons (Electrical, HVAC, Piping, Apparel) and their
   buttons are **out of scope** for the fork's 0.5 (owner, 2026-11-03); the
   full display build of a big drawing is 2–3× the 0.5 view's walk (each edit is faster); DXF 2000 save runs
   1.0–1.1× the JS on Carey (the 0.7× was before M5.1; S.5: what is left is spread over the save model's dynamic
   values — building them 48 %, writing 37 %, freeing 11 % — next: write DXF 2000 straight from the typed objects).
   Whole-drawing edits on Carey (S.4, S.8: block objects shared copy-on-write, undo / redo move objects instead of
   copying them, the MOVE family reads objects by id) run 1.5–2.6× the JS: SELECT ALL + MOVE 1.6×, EXPLODE ALL 1.5×,
   ERASE ALL + UNDO 1.7×, XATTACH 1.8×, MATCHPROP 2.1×, ARRAY 2.6×; on ar1all MOVE is 1.3× and XATTACH 1.1× (the cost
   left is one copy of each moved object's geometry and its layer name).
2. **Web speed**: after M15.3 (wasmalloc in place of dlmalloc, whose memory growth cost Chromium a GC per step) the
   browser reads Carey 2.3× faster than 0.5's JS on a first read and opens it to the first frame 2.4× faster; warm reads
   run at about native single-thread speed. Left: the desktop's threads (wasm threads need a per-thread allocator,
   roadmap § Wasm threads); small drawings are dominated by first-call code compile (+9 % code with wasmalloc).
3. **Viewer details** vs CadCraft 0.5 (M3.7: print view, layers frozen per viewport, the Drawing panel, Find, recent
   files, Settings, the help panel, pinch / pan; M3.9–M3.10: BimCraft's DWG view extras — the equipment schedule tab
   with its CSV, the 3D view of model space with picking for Properties and the section plane, the dynamic-blocks list,
   "Shown, similar name" pictures, the desktop's project folder (the drawing's folder and its subfolders, or one opened),
   the help's reader context — all at dwgviewdiff parity). Left: text in the 3D view is drawn over the view, not hidden
   behind faces (0.5 draws texture quads with depth), and in the app's UI font rather than Helvetica; the 3D view has
   the standard-view bar but not 0.5's corner view cube; the web build cannot walk folders (0.5's folder picker /
   drop of a folder: pictures and links are added by hand there); translated help (0.5 ships none yet). Touch is
   checked through egui's multi-touch events only, not on a tablet.
4. **CivilCraft app view** (M11): drawing tools, typed traverse / deed calls, grips, point moves and Delete (M11.7),
   the profile panel with PVI add / drag / delete and the PI / PVI tables (M11.8), the section panel and sample lines
   (M11.9), point selection with copy / cut / paste / duplicate (M11.10), cross-section sheets (PDF) and mass haul
   (M11.11), feature lines in the profile and the curve "why" fixes (M11.12), the Properties edits (M11.13), the lasso
   with window / crossing / fence and the renumber settings (M11.14) are in at byte parity (cvtooldiff 76/76).
   Then (M11.18–M11.22, cvtooldiff-civil2 / cvuidiff-civil2, 0 differences in UTC and America/Denver): superelevation's
   per-curve table and diagram, storm drainage (the Drainage block, structure / pipe rows, the Catchment tool, Size
   pipes), a parcel's legal description Copy / Download, Settings › CivilCraft's other fields kept between runs with the
   mass haul factor (desktop: civilcraft.json), the Import points dialog (latitude / longitude through the project's
   system), touch's draft bar, the 3D view's Drive panel and Sun menu, GIS layers (import dialog, the left panel,
   picked features, GIS › Export with zoned dates), the Coordinates window (search, the systems the site fits) and
   lat/lon in the status bar, map layers (pictures with world files, tiles, WMS, WMTS, ArcGIS) carried by the PDF and DWG.
   Then (M11.26–M11.31, cvtooldiff-civil3, 0 differences in UTC, America/Denver and es-MX): the Coordinates window in
   full (Ground / Local, .prj files, grid shift files, converting the project's coordinates), askCrs (the system picker
   for GIS files, latitude / longitude point files and pictures without a .prj), GIS layers moved with the system and
   Use for the surface, terrain (elevation grids, LiDAR; the surface out as a GeoTIFF), DWG / DXF linework with block
   points, RGB GeoTIFF pictures, map layers draped in 3D, the assistant tools in the control channel, touch's long press.
   Map tiles, WMS / WMTS / ArcGIS map servers and cdn.proj.org grid-shift downloads are **web only (owner,
   2026-11-03)**: the desktop app has no HTTP client by decision. Missing: a single picked point is the selected point, so
   finishing a line drops it (0.5 keeps it picked); GeoTIFF pictures are kept as the app's own PNG / JPEG (0.5: the
   browser canvas's). Speed: reading 500,000 point rows 2.0× the JS, the 33 MB GeoJSON read 1.3×, its import 1.8×, GIS
   export 1.4–2.4×; a 4000² DEM import 1.0×; section sheets PDF runs 1.4× the JS (S.2).
5. **ModelCraft** (M6.17–M6.35): every 0.5 feature in, at mcuidiff parity in en and es-MX (panels, components,
   3D-print check, inference keys M6.17; copy / paste, flip, 3D text M6.19; solids, storeys, axes M6.20; components
   that stretch M6.21; sun shadows M6.22; material textures M6.23; Lights M6.24; Render a picture M6.25 with lamp
   shadows, glowing bulbs and bending glass M6.28; Play M6.26; the WebM film M6.27 / M6.32; the entourage M6.29;
   imported pictures as textures M6.30; terrain, the starter library, modeler keys, the Views menu, Snap / Collide
   M6.31; view marks M6.33; Settings › ModelCraft kept between runs, the view cube / axis icon, trackpad scrolling and
   touch, axes and guide lines in the 3D scene M6.35). Left, by decision or limit: pictures are 256² texture layers,
   34 a scene (0.5: the picture at its own size; a bigger array costs GPU memory the web build cannot spare); the
   numpad's views work from scripts (`key:7@Numpad7`) but not from the keyboard (egui 0.37 reports the numpad's
   digits as the row's: Key::Num7 for both, on desktop and web); touch has no loupe above the finger; the film is VP8
   key frames only (a few MB a minute).
6. **BimCraft**: done for 0.5 apart from what needs the library service (M7.13–M7.24): IFC edit + save, the storey
   plan + PDF, Draw tools, schedules, plan grips, library items placed from the plan, residential sync, the Library
   panel (This file, Files… adapters; LibraryCraft and feeds on the web), swap / update, copy / paste of placements and
   elements; the Build menu's residential tools (roof plane, foundation, countertop, stair, railing, framing with show /
   see through, terrain from survey text / DXF / DWG contours / LAS / LAZ / GeoTIFF / spots, building pad, paving,
   section line, electrical with switch legs) with their plan symbols, previews, Properties, Delete and the Lumber /
   Devices / Circuits schedules; LandXML; Sheets (plans, elevations, sections, schedules, framing; set PDF); Takeoff
   (the takeoff inside the PDF for TakeoffCraft); the IFC equipment schedule and Library credits. Still web-limited:
   the library sign-in and Check for updates (the library service; no desktop HTTP client by decision). The Takeoff
   hand-off opens TakeoffCraft in a tab on the web in 0.5; the fork saves / downloads the PDF (TakeoffCraft opens it).
7. **PDF sheets: MarkupCraft and TakeoffCraft** (M8/M9): done for 0.5 (M9.13–M9.28): every MarkupCraft tool, panel,
   export, compare, BCF, Find text, sheet-callout links, bookmarks, custom columns, slip-sheet, flattened copy, print
   and Close the set; every TakeoffCraft panel, revisions, the 3D view (camera, standard views, the sheet on the ground,
   PNG, levels), snapping to the drawing's linework, typed lengths and 45°. What stays different, by the renderer: BCF
   snapshot pictures and Find changes' pixel counts are hayro's, not pdf.js's canvas (the BCF data and the marks flagged
   match; a pixel match would need pdf.js itself); the 3D PNG is the fork's own view. 0.5 has no "close one PDF of a
   set": its Close closes the set, as the fork's now does.
8. **PartCraft** (M13): part view on the mesh kernel — open part / script, sample, 3D with face picking, history,
   parameters, undo, rollback, suppress, Save, STL / 3MF / STEP / IFC at byte parity; the sketcher (every 0.5 sketch
   tool, relations, dimensions, live drag) and the feature tools (extrude … shell, edge / face picking, edit, delete, move,
   rename) at byte parity (M13.3); rebuilds over 60 ms run on a thread and a parameter edit follows `configs` (M13.4).
   Measure, Mass properties and the Section view (M13.5); the More menu, reference geometry, draft, thread and the
   design extras (M13.6); the Edit menu with copy / paste between parts and sketch Project (M13.7); DXF of a sketch or
   face, design-table CSV import (M13.8); drawings with GD&T, section, detail, hole table, title block, revisions and
   PDF (M13.9); draft / zebra / curvature analysis (M13.10); assemblies: copies, inserts, joints, limits, motion and
   interference checks, edit in place, Edit menu, save / open (M13.11); part properties, BOM, exploded views, typed
   moves and the assembly's exports (M13.12); es-MX at parity in every scenario. Missing: the drag gizmo (moving a part
   or an explode step by dragging in the view); base planes in the view are in (M13.13); the exact kernel's features (sweep, loft, surfaces, bodies, sheet metal, rib) say
   "not built", as 0.5's mesh build does.
   **Studio** (M16): the local control channel (M16.1) and the headless MCP server with 0.5's 22 tools (M16.2) are in; LISP scripts are in (M4.40; `drawing_plot` since M4.12, plot.js ported). The window's live tools are in (M16.3, drawing_plot M4.12) except view_screenshot and DetailCraft's (app_info only). Studio Live's protocol core passes the shared vectors (M16.4); its network side (WebSocket to a host or the relay, the PAKE handshake, sealed frames, snapshots, host handoff) and the app adapters are not built. The start screen is in (M16.5); Studio's suite parts (installs, updates, licence, assistants page, Live hosting UI) are not.
   **DetailCraft** (M14.1, M14.3): view, properties, reports, clash, every 0.5 export, and the modelling tools (placing on
   grid intersections with snapping and Add by grid, connections and their parameters with Apply to similar, copy / cut /
   paste / duplicate, move / rotate / mirror, Change on all of them, window / crossing / lasso selection, grids, levels,
   phases, holes, copes, custom profiles, Settings, plan and elevation views) at parity with the frozen JS doing the same.
   Missing: dragging selected parts to move them (D2), the view cube and axis icon, a plan / elevation that clips parts
   crossing its band exactly (one section plane; parts wholly outside are hidden), the drawings dialog's preview and issue
   log, the clipboard between windows only through the system clipboard (no localStorage), settings kept between runs,
   touch long-press, es-MX labels; the bar's view buttons (Iso, Top, Front, Right) are plain English.
   Missing: sketch Project (needs exact edges), sketch copy / paste between parts, the reference-geometry / sweep / loft /
   surface / body / sheet-metal / rib / thread tools of the More menu, assemblies, drawings (PDF), DXF of a sketch or
   face, measure / mass / section, design-table CSV import, the exact kernel's features (they say "not built", as 0.5's
   mesh build does), Spanish words.
   **Studio** (M16): the local control channel (M16.1) and the headless MCP server with 0.5's 22 tools (M16.2) are in; LISP scripts are in (M4.40; `drawing_plot` since M4.12, plot.js ported). The window's live tools are all in (M16.3, M16.10). Studio Live (M16.4, M16.9, M16.11–M16.16): the protocol core, the session server, the SPAKE2 handshake and sealed frames byte for byte 0.5's, our own LAN WebSocket, `cadcraft-cli live-server`; all eight apps co-edit (0.5's live.js adapters) with the 0.5 page and between Rust windows, on the fork's server and on 0.5's; LAN discovery (mDNS `_ccstudio._tcp`, found by and finding 0.5 Studio's mdns-sd), the join QR, signed-in names checked offline against the pinned keys, the Live window in Studio's words. Not built: the relay (wss://, the internet path: web only, no desktop HTTP / TLS client by decision) and invite links; getting a signed-in name of one's own (needs the site's `POST /api/v1/live/assertion`: the desktop has no HTTP client, so joiners from the fork join unticked; hosts check the ticks others bring); the browser edition's own WebSocket joiner; host handoff in the window; CadCraft's in-drawing presence (others' cursors and selections), holds (LIVEHOLD) and padlocks (LIVELOCK / LIVEUNLOCK) and the same-file join; one-time codes. The start screen is in (M16.5); Studio's suite parts (installs, updates, licence, assistants page) are not.
   **DetailCraft** (M14.1, M14.3–M14.7): view, properties, reports, clash, every 0.5 export, the modelling tools,
   dragging selected parts (D2), the view cube / axis icon, plan / elevation cut exactly, the Drawings dialog with its
   preview and issue log, the shop drawing dialog, bolts picked in 3D, settings kept between runs, the touch long press,
   edits on big models at 1.3–2× the JS, es-MX at parity with the frozen JS doing the same. Missing: the clipboard
   between windows only through the system clipboard (no localStorage).
9. **Words** (M3.6–M3.7): every app speaks en or es-MX (0.5's catalogs unchanged; Settings or the start screen
   switches; the desktop keeps it, the web keeps it in the browser or follows `?lang=` / the browser's language). es-MX
   parity: kdiff 4426/4426, mkuidiff 17/17, cvuidiff all, cvtooldiff 76/76 + 2084/2084, dcuidiff 58/58, pcuidiff all,
   tkopsdiff 61/61 + 262/262, mcuidiff 1200/1200 + 1243/1243 (harness copies `*-viewer.mjs`). Still plain English, no
   0.5 key: the "Rust test version" tag, the UI_COMMANDS descriptions (agent docs, not on screen), the PDF view's status
   line (sheet n of m · zoom · render ms), timing lines (DetailCraft / BimCraft / PartCraft "Timings"), the 3D test
   scene and ModelCraft-import status lines ("Opened … in n ms"), "Saved {path}" after an export, bimedit's IFC
   attribute names (ObjectType, NominalValue: IFC's own). A drawing already open keeps the command line's language
   until it is opened again (0.5 reloads the page to change language). Hub relative times follow es-MX
   (Intl.RelativeTimeFormat's words); MarkupCraft's stamp dates too.

10. Accessibility: full screen-reader support is a **known issue, skipped** (owner, 2026-10-15). Icon buttons get
   accesskit labels where it is free. The desktop build leaves out eframe's `accesskit` bridge (2026-10-17): on Linux it
   pulled zbus crates RustSec lists as unmaintained (RUSTSEC-2026-0370/0371/0372, no upgrade); it comes back with the
   screen-reader work.

11. **Local-time dates (M16.8):** every date 0.5 writes in the user's local time is now written in the local time in the
   ported apps too: zip dates (MarkupCraft XLSX / BCF, TakeoffCraft XLSX, DetailCraft NC / DWG zips), the KISS header,
   CivilCraft LandXML date / time, and MarkupCraft's list / Properties dates. All harnesses show 0 differences under UTC,
   America/Denver, Asia/Kolkata and Pacific/Auckland, and with the clock set at a DST change. Still open: dates in parts
   that are not ported yet must take a `cc_clock::Zone` when they land (CadCraft sheet-set archive zip,
   ModelCraft print caption, PartCraft script `clock()`, CivilCraft drive sun offset, BimCraft folder
   dates, activity lists (the recovery dialog's times: M4.46); CivilCraft .dbf / GIS zips once the GIS export is in the app). Spec gap: years
   before 1 print as plain numbers in the English date text; the hub's relative times are in English only.

Closed: DWG reading R13–R2018 at full-model parity with 0.5 (M2, 2026-10-16); DXF reading and the 2D viewer (M3);
the command line, LINE … TRIM, undo, selection, DWG writing, CadCraft's DWG import and DWG / DXF saving at byte parity (M4).

12. **The 0.7 look (M17)**: Tokens from 0.7's tokens.css (paper / model, plus medium light, medium dark and high
    contrast), Archivo, the app bar (mark, swatch, name™, test chip, bordered buttons, Paper | Model), the status bar
    and its toggles, CadCraft's whole window (0.7 toolbar groups that wrap, "Layers and drawing" with blocks / sheet
    set / drawing, the Properties grip, tabs, the command area, the grid and axis icon, the Start drawing card) and
    the start screen laid out as the website's /apps page are in (docs/ui-notes.md). The other
    apps' toolbars and docks are 0.7's since M17.2–M17.6 (ModelCraft, MarkupCraft, DetailCraft, PartCraft, BimCraft's
    IFC view, CivilCraft, TakeoffCraft); still the old look: the panels' section heads (themed egui widgets; fields, tabs and buttons are 0.7's since
    M17.10–M17.11), the dialogs' inner layouts (their titles, backdrop and primary buttons are 0.7's since M17.12). Not in
    CadCraft's window yet: the snap kinds kept between runs, the command history's first lines ("CadCraft is
    ready…").
