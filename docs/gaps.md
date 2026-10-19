# Gaps against CadCraft 0.5

Last updated 2026-10-19 · Change: sheets and viewports, named views (M4.16); clipboard, QSELECT / SELECTSIMILAR / FIND, UNITS (M4.15); CivilCraft sheets, mass haul, WL.20 / WL.23, Properties edits, lasso (M11.11–M11.14); drafting edits share each object's extra data (M4.14); drafting commands, Properties panel and plotting (M4.10–M4.12); ModelCraft panels (M6.17); per-app gaps below. Ranked; shrink as features land.

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
   panes are the browser's). **Next, by everyday use:** PUBLISH; UCS PLAN TURNLEFT TURNRIGHT TURNWORLD; LWDISPLAY POLARANG; COUNT; XATTACH XREF XCLIP; IMAGEATTACH IMAGE IMAGECLIP; BVSTATE BVHIDE BVSHOW (BEDIT opens a
   dynamic block without
   its visibility states); LIBRARY and INSERT's File / Library options; the parametric set (GEOMCONSTRAINT DIMCONSTRAINT
   DELCONSTRAINT AUTOCONSTRAIN PARAMETERS CONSTRAINTBAR DCDISPLAY); 3D (EXTRUDE … GLBOUT); SHEETSET, VIEWBASE,
   POINTCLOUDATTACH POINTCLOUD, SPLIT LINKVIEWS OPENIN3D LASSOMODE SHORTCUTS COMPARESETTINGS SWBOUT. LAYER NEW OPEN SAVE
   SAVEAS DXFOUT HELP are the window's own buttons. The Properties panel (M4.11) has General, Geometry, Text, Pattern,
   the dimension sections and Misc; it lacks the dynamic-block Custom section, Attributes, Library, the sheet / view
   sections, Quick Properties and the point pick buttons. The Plot dialog (M4.12) lacks pictures, Open / print and
   PUBLISH. Also missing: the ribbon / menus, constraints, recovery snapshots, linked drawings in edit mode (the viewer
   shows them); objects with a dense linetype draw continuous past 400 dashes (`DISPLAY_MAX_DASHES`); DXF 2000 save is
   slower than the JS (0.7×). Whole-drawing edits (M4.14, extra data shared copy-on-write) now run 1.2–7.4× the JS on
   the ar1all floor plan and the Carey Foodservice drawing; the slimmest margins are EXPLODE ALL (1.2×) and ERASE ALL +
   UNDO (1.2×) on Carey, where each object still gets one geometry copy per change (its undo op and the map share the
   extra keys, not the geometry: a polyline's points, a hatch's loops).
2. **Web speed**: the browser build reads only 1.2–1.5× faster than 0.5's JS (native 2.6–4.2×). Wasm threads were built
   and measured slower (one global allocator lock; roadmap § Wasm threads); allocation measured not to be the cost; next: profile the decoder.
3. **Viewer details** vs CadCraft 0.5: raster images (IMAGE) not drawn, per-viewport frozen layers, print view /
   lineweights, find, recent files, settings, help panel, language files (en / es-MX), touch gestures.
4. **CivilCraft app view** (M11): drawing tools, typed traverse / deed calls, grips, point moves and Delete (M11.7),
   the profile panel with PVI add / drag / delete and the PI / PVI tables (M11.8), the section panel and sample lines
   (M11.9), point selection with copy / cut / paste / duplicate (M11.10), cross-section sheets (PDF) and mass haul
   (M11.11), feature lines in the profile and the curve "why" fixes (M11.12), the Properties edits (M11.13), the lasso
   with window / crossing / fence and the renumber settings (M11.14) are in at byte parity (cvtooldiff 76/76).
   Missing: superelevation's per-curve table and diagram, the drainage (catchment) rows of structures and pipes, a
   parcel's legal description copy / download buttons, the mass haul factor kept between runs (0.5 keeps it in
   localStorage; here per session) and Settings' other fields (labels, corner, exaggeration, lat/lon, internet,
   assistants), the import dialog (format is detected), GIS layers and basemaps (so exports carry no map pictures), the
   catchment tool, touch's draft bar; a 500k-point survey zoomed out draws every point cross (fast, but a carpet);
   section sheets PDF runs 0.5× the JS (io-civil's PDF page writer: layout 3.7 ms, writing ~125 ms on 107 sections).
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
   parameters, undo, rollback, suppress, Save, STL / 3MF / STEP / IFC at byte parity; the sketcher (every 0.5 sketch
   tool, relations, dimensions, live drag) and the feature tools (extrude … shell, edge / face picking, edit, delete, move,
   rename) at byte parity (M13.3); rebuilds over 60 ms run on a thread and a parameter edit follows `configs` (M13.4).
   Missing: sketch Project (needs exact edges), sketch copy / paste between parts, the reference-geometry / sweep / loft /
   surface / body / sheet-metal / rib / thread tools of the More menu, assemblies, drawings (PDF), DXF of a sketch or
   face, measure / mass / section, design-table CSV import, the exact kernel's features (they say "not built", as 0.5's
   mesh build does), Spanish words.
   **Studio** (M16): the local control channel (M16.1) and the headless MCP server with 0.5's 22 tools (M16.2) are in; missing there: LISP scripts (`drawing_plot` is in since M4.12, plot.js ported). The window's live tools are in (M16.3, drawing_plot M4.12) except view_screenshot and DetailCraft's (app_info only). Studio Live's protocol core passes the shared vectors (M16.4); its network side (WebSocket to a host or the relay, the PAKE handshake, sealed frames, snapshots, host handoff) and the app adapters are not built. The start screen is in (M16.5); Studio's suite parts (installs, updates, licence, assistants page, Live hosting UI) are not.
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
