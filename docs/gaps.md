# Gaps against CadCraft 0.5

Last updated 2026-10-18 · Change: drafting commands and the Properties panel (M4.10, M4.11); CivilCraft tools and panels (M11.7–M11.10), BimCraft Draw (M7.13), MarkupCraft / TakeoffCraft panels (M9.13–15); per-app gaps below. Ranked; shrink as features land.
Last updated 2026-10-18 · Change: drafting commands (M4.10); CivilCraft tools and panels (M11.7–M11.10), BimCraft Draw (M7.13), MarkupCraft / TakeoffCraft panels (M9.13–15); per-app gaps below. Ranked; shrink as features land.
Last updated 2026-10-18 · Change: ModelCraft panels (M6.17); CivilCraft tools and panels (M11.7–M11.10), BimCraft Draw (M7.13), MarkupCraft / TakeoffCraft panels (M9.13–15); per-app gaps below. Ranked; shrink as features land.

1. **Drafting vs 0.5** (after M4.10, M4.11). 0.5's command table (`commands.js`, `layertools.js`, `propcmds.js`,
   `pcommands.js`, `commands3d.js`, `pictures-cmd.js`, `xclip.js`, `cloud-cad.js`, `flatview.js`: 190 commands), ranked by
   everyday use. **In, at kdiff parity** (same prompts, printed lines, previews, drawing, undo labels): LINE … ZOOM (M4);
   HATCH (pick-inside boundaries with islands, Select objects, pattern / scale / angle), DIMLINEAR DIMALIGNED DIMANGULAR
   DIMRADIUS DIMDIAMETER DIMCONTINUE DIMBASELINE, DIMSCALE DIMTXT DIMASZ DIMDEC DIMGAP DIMEXE DIMEXO DIMLUNIT, FILLET
   CHAMFER (polyline, trim, multiple, Shift), EXPLODE, STRETCH, ARRAY ARRAYRECT ARRAYPOLAR, BREAK BREAKATPOINT, LENGTHEN,
   JOIN, MATCHPROP, XLINE RAY, MTEXT, REVCLOUD, MLEADER, TEXTEDIT, BLOCK ATTDEF INSERT BEDIT BCLOSE PURGE, DUPLICATE
   SELECTALL DESELECT SELECT, DIST AREA MEASUREGEOM ID LIST, LAYISO LAYUNISO LAYOFF LAYFRZ LAYMCUR LAYON LAYTHW LAYMRG
   LAYDEL RENAME LINETYPE LTSCALE LWEIGHT, PROPERTIES PROPERTIESCLOSE QPMODE (M4.10, M4.11). **Next, by everyday
   use:** PLOT; COPYCLIP CUTCLIP COPYBASE PASTECLIP PASTEORIG (clipboard.js); QSELECT SELECTSIMILAR FIND; UNITS; LAYOUT MVIEW
   VPORTS MSPACE PSPACE VPLAYER VIEWLABEL; PUBLISH; NAMEDVIEW; UCS PLAN TURNLEFT TURNRIGHT TURNWORLD; LWDISPLAY POLARANG;
   COUNT; XATTACH XREF XCLIP; IMAGEATTACH IMAGE IMAGECLIP; BVSTATE BVHIDE BVSHOW (BEDIT opens a dynamic block without
   its visibility states); LIBRARY and INSERT's File / Library options; the parametric set (GEOMCONSTRAINT DIMCONSTRAINT
   DELCONSTRAINT AUTOCONSTRAIN PARAMETERS CONSTRAINTBAR DCDISPLAY); 3D (EXTRUDE … GLBOUT); SHEETSET, VIEWBASE,
   POINTCLOUDATTACH POINTCLOUD, SPLIT LINKVIEWS OPENIN3D LASSOMODE SHORTCUTS COMPARESETTINGS SWBOUT. LAYER NEW OPEN SAVE
   SAVEAS DXFOUT HELP are the window's own buttons. The Properties panel (M4.11) has General, Geometry, Text, Pattern,
   the dimension sections and Misc; it lacks the dynamic-block Custom section, Attributes, Library, the sheet / view
   sections, Quick Properties and the point pick buttons. Also missing: the ribbon / menus, constraints, recovery snapshots, linked drawings in edit mode (the viewer
   shows them); on drawings whose objects carry many extra fields MOVE / ARRAY / MATCHPROP / ERASE + UNDO run 0.1–0.6× the JS (every object's extra JSON is deep-copied per preview and per change; the JS shares it: share extras in cc-cad); objects with a dense linetype draw continuous past 400 dashes (`DISPLAY_MAX_DASHES`); DXF 2000 save is
   slower than the JS (0.7×).
2. **Web speed**: the browser build reads only 1.2–1.5× faster than 0.5's JS (native 2.6–4.2×). Wasm threads were built
   and measured slower (one global allocator lock; roadmap § Wasm threads); allocation measured not to be the cost; next: profile the decoder.
3. **Viewer details** vs CadCraft 0.5: raster images (IMAGE) not drawn, per-viewport frozen layers, print view /
   lineweights, find, recent files, settings, help panel, language files (en / es-MX), touch gestures.
4. **CivilCraft app view** (M11): drawing tools, typed traverse / deed calls, grips, point moves and Delete (M11.7),
   the profile panel with PVI add / drag / delete and the PI / PVI tables (M11.8), the section panel and sample lines
   (M11.9), point selection with copy / cut / paste / duplicate (M11.10) are in at byte parity (cvtooldiff 52/52).
   Missing: cross-section sheets (PDF) and mass haul from the section panel, feature lines in the profile (WL.20), the
   curve "why" fixes (WL.23), the other Properties edits (names, pad elevation and slopes, corridor, pipes, feature
   line grades …), the lasso (a Shift+drag that bends) and its window / crossing / fence modes, renumber settings,
   the import dialog (format is detected), GIS layers and basemaps (so exports carry no map pictures), the catchment
   tool, touch's draft bar; a 500k-point survey zoomed out draws every point cross (fast, but a carpet).
5. **ModelCraft** (M6.17: panels, components, 3D-print check, inference keys at parity): missing the Render dialog
   (looks, path tracing, lamps' Lights panel and editor), sun shadows drawn in the view (the settings are kept and saved),
   procedural material textures (swatches and faces show the material's colour), components that stretch (partRules,
   stretch dialog), the scenes' Play and WebM recording, copy / paste and duplicate, flip, solids tools, terrain,
   text, storeys, drawing axes, the starter library, Blender key style.
6. **BimCraft** (M7.13: IFC edit + save, storey plan + PDF, Draw tools, schedules at parity): no grips (dragging wall ends, sliding doors / windows), no residential sync (res-follow.js), no
   Library items placed from the plan.
7. **PDF sheets: MarkupCraft and TakeoffCraft** (M8/M9): every 0.5 MarkupCraft drawing tool (M9.14) with select /
   move / grips / window, the text editor and the Properties panel, save marked-up PDF at parity; takeoff count / linear
   / area, exports, Auto count. Missing in MarkupCraft: picture and signature stamps, tool chest presets, cut-outs in
   areas, spaces, legends, links, replies, punch list, markups list, copy / paste, sheet rename, pages menu, BCF,
   compare's picture overlay / line-up / whole-set compare, closing one PDF of a plan set (several PDFs and compare by
   changed lines are in, M9.15).
   TakeoffCraft has its item / mark panels, assemblies, bid areas, alternates, deducts, slopes and cut-outs (M9.13);
   still missing there: arcs and circles in runs and outlines, dragging mark corners, multi-select, groups (typicals),
   notes / clouds, the 3D view, revisions.
8. **PartCraft** (M13): part view on the mesh kernel — open part / script, sample, 3D with face picking, history,
   parameters, undo, rollback, suppress, Save, STL / 3MF / STEP / IFC at byte parity. Missing: the sketcher and every
   tool that makes or edits features (new sketch / extrude / hole … , picking edges and faces for them, delete, move,
   rename), assemblies, drawings (PDF), DXF of a sketch or face, measure / mass / section, configurations (a parameter
   edit does not follow `configs` yet), the exact kernel's features (they say "not built", as 0.5's mesh build does),
   rebuilds on a thread (a big part blocks the window), Spanish words. **Studio** (M16): the local control channel (M16.1) and the headless MCP server with 0.5's 22 tools (M16.2) are in; missing there: `drawing_plot` (plot.js) and LISP scripts. The window's live tools are in (M16.3) except drawing_plot, view_screenshot and DetailCraft's (app_info only). Studio Live's protocol core passes the shared vectors (M16.4); its network side (WebSocket to a host or the relay, the PAKE handshake, sealed frames, snapshots, host handoff) and the app adapters are not built. The start screen is in (M16.5); Studio's suite parts (installs, updates, licence, assistants page, Live hosting UI) are not.
   **DetailCraft** (M14.1): view, member properties with undo, reports, clash check and every 0.5 export at parity. Missing: placing
   members / plates / footings / stairs, connection tools and parameters (end plate … gusset, apply to similar), holes and
   copes editors, grids / levels / custom profiles, phases add / rename / remove, copy / paste, move / rotate / mirror,
   window and lasso selection, plan and elevation views, the drawings dialog's preview and issue log, the Settings dialog
   (`detail.setting` only), es-MX labels; several of the bar's labels are plain English (view buttons, Steel sample).
   parameters, undo, rollback, suppress, Save, STL / 3MF / STEP / IFC at byte parity; the sketcher (every 0.5 sketch
   tool, relations, dimensions, live drag) and the feature tools (extrude … shell, edge / face picking, edit, delete, move,
   rename) at byte parity (M13.3). Missing: sketch Project (needs exact edges), sketch copy / paste between parts, the
   reference-geometry / sweep / loft / surface / body / sheet-metal / rib / thread tools of the More menu, assemblies, drawings (PDF), DXF of a sketch or face, measure / mass / section, configurations (a parameter
   edit follows `configs` since M13.4; design-table CSV import is not built), the exact kernel's features (they say "not built", as 0.5's mesh build does),
   Spanish words (rebuilds over 60 ms run on a thread since M13.4). **Studio** (M16): the local control channel (M16.1) and the headless MCP server with 0.5's 22 tools (M16.2) are in; missing there: `drawing_plot` (plot.js) and LISP scripts. The window's live tools are in (M16.3) except drawing_plot, view_screenshot and DetailCraft's (app_info only). Studio Live's protocol core passes the shared vectors (M16.4); its network side (WebSocket to a host or the relay, the PAKE handshake, sealed frames, snapshots, host handoff) and the app adapters are not built. The start screen is in (M16.5); Studio's suite parts (installs, updates, licence, assistants page, Live hosting UI) are not.
   rebuilds on a thread (a big part blocks the window), Spanish words. **Studio** (M16): the local control channel (M16.1) and the headless MCP server with 0.5's 22 tools (M16.2) are in; missing there: `drawing_plot` (plot.js) and LISP scripts. The window's live tools are in (M16.3) except drawing_plot, view_screenshot and DetailCraft's (app_info only). Studio Live's protocol core passes the shared vectors (M16.4); its network side (WebSocket to a host or the relay, the PAKE handshake, sealed frames, snapshots, host handoff) and the app adapters are not built. The start screen is being built.
   rebuilds on a thread (a big part blocks the window), Spanish words. **Studio** (M16): the local control channel (M16.1) and the headless MCP server with 0.5's 22 tools (M16.2) are in; missing there: `drawing_plot` (plot.js) and LISP scripts. The window's live tools are in (M16.3) except drawing_plot, view_screenshot and DetailCraft's (app_info only). Live and the start screen are being built.
   rebuilds on a thread (a big part blocks the window), Spanish words. **Studio** (M16): the local control channel (M16.1) and the headless MCP server with 0.5's 22 tools (M16.2) are in; missing there: `drawing_plot` (plot.js) and LISP scripts. Live tools in the window, Live and the start screen are being built.
   rebuilds on a thread (a big part blocks the window), Spanish words. **Studio** (M16): the local control channel for assistants is in (M16.1, `cadcraft --control`); the MCP tools, Live and the start screen are being built.
   rebuilds on a thread (a big part blocks the window), Spanish words. **Studio**: in progress (M16).
   **DetailCraft** (M14.1, M14.3): view, properties, reports, clash, every 0.5 export, and the modelling tools (placing on
   grid intersections with snapping and Add by grid, connections and their parameters with Apply to similar, copy / cut /
   paste / duplicate, move / rotate / mirror, Change on all of them, window / crossing / lasso selection, grids, levels,
   phases, holes, copes, custom profiles, Settings, plan and elevation views) at parity with the frozen JS doing the same.
   Missing: dragging selected parts to move them (D2), the view cube and axis icon, a plan / elevation that clips parts
   crossing its band exactly (one section plane; parts wholly outside are hidden), the drawings dialog's preview and issue
   log, the clipboard between windows only through the system clipboard (no localStorage), settings kept between runs,
   touch long-press, es-MX labels; the bar's view buttons (Iso, Top, Front, Right) are plain English.
9. Panel buttons in the new views are partly plain English rather than 0.5 catalog words (MarkupCraft's tool bar and
   TakeoffCraft's export row use the catalogs).
10. Accessibility: full screen-reader support is a **known issue, skipped** (owner, 2026-10-15). Icon buttons get
   accesskit labels where it is free. The desktop build leaves out eframe's `accesskit` bridge (2026-10-17): on Linux it
   pulled zbus crates RustSec lists as unmaintained (RUSTSEC-2026-0370/0371/0372, no upgrade); it comes back with the
   screen-reader work.

Closed: DWG reading R13–R2018 at full-model parity with 0.5 (M2, 2026-10-16); DXF reading and the 2D viewer (M3);
the command line, LINE … TRIM, undo, selection, DWG writing, CadCraft's DWG import and DWG / DXF saving at byte parity (M4).
