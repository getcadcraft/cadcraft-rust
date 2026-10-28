# Gaps against CadCraft 0.5

Last updated 2026-10-27 · Change: web speed (M15.3: web allocator, V8 trig in the display list); recovery copies and Recover your work? (M4.46); CadCraft's ribbon, toolbar and menus (M4.45); CadCraft's LISP (M4.40); Plot Open to print, Properties sections, window settings, lasso, VIEWBASE, SHEETSET (M4.30–M4.34); CadCraft's 3D tab (M4.25–M4.26); GeoTIFF tiles under one pixel refused (civil_raster fuzz timeout, M11.17); 3D commands and the 3D model in files (M4.24); constraints and parameters (M4.23); LIBRARY, INSERT File / Library (M4.22); PUBLISH and pictures in plots (M4.21); ModelCraft scenes' Play (M6.26); ModelCraft Render a picture (M6.25); ModelCraft Lights panel (M6.24); ModelCraft material textures (M6.23); speed S.4–S.8 (XATTACH, whole-drawing edits and undo on Carey, TakeoffCraft copy + paste, ModelCraft undo); ModelCraft sun shadows (M6.22); ModelCraft components that stretch (M6.21); local-time dates in every ported writer (M16.8); GeoTIFF reads capped at 1 GB (civil_raster fuzz OOM, M11.16); speed S.1–S.3 (marked PDF, section sheets PDF, 3D GLB faster than 0.5); ModelCraft solids, storeys, drawing axes (M6.20); ModelCraft copy / paste, duplicate, flip, 3D text (M6.19); TakeoffCraft 3D view (M9.23); TakeoffCraft revisions (M9.22); TakeoffCraft typical groups and markups (M9.21); TakeoffCraft arcs / circles, corner drags, multi-select, clipboard (M9.20); MarkupCraft BCF, compare overlay / line-up / sets (M9.19); MarkupCraft cut-outs, spaces, legends, links, punch list (M9.18); MarkupCraft picture / signature stamps and tool chest (M9.17); MarkupCraft clipboard, Markups list, replies, sheet rename, Pages menu (M9.16); visibility states in BEDIT (M4.20); pictures (M4.19); linked drawings and XCLIP (M4.18); BimCraft library items placed from the plan (M7.15), plan grips (M7.14); UCS and view turns, POLARANG, COUNT (M4.17); sheets and viewports, named views (M4.16); clipboard, QSELECT / SELECTSIMILAR / FIND, UNITS (M4.15); CivilCraft sheets, mass haul, WL.20 / WL.23, Properties edits, lasso (M11.11–M11.14); drafting edits share each object's extra data (M4.14); drafting commands, Properties panel and plotting (M4.10–M4.12); ModelCraft panels (M6.17); per-app gaps below. Ranked; shrink as features land.

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
   3D for the 2D commands, the drawing-units choice in Properties that scales the solids (M4.25–M4.26; the tab's point
   cloud, the view cube's compass and touch gestures not yet). The Plot dialog's Open to print (M4.30); the
   Properties panel's Custom (dynamic blocks), Attributes, Library, attribute definitions, viewport scale, sheet,
   current settings, View, Misc, point picks and Quick Properties (M4.31; not yet: P.12's choice of fields, PICKADD
   and Auto-hide buttons); SPLIT LINKVIEWS OPENIN3D LASSOMODE SHORTCUTS COMPARESETTINGS and lasso selection (M4.32;
   the window has no split panes, so SPLIT / LINKVIEWS say so as VPORTS does; OPENIN3D opens the window's 3D tab);
   VIEWBASE (M4.33); SHEETSET with its panel, publish, Update drawings, sheet list and archive (M4.34; desktop folders
   only: the web build cannot open a folder yet; drag-and-drop of rows is Alt+↑ / ↓). LISP: APPLOAD, (command …), DCL dialogs, ALIASEDIT REINIT DELBLOCKS
   (M4.40); the ribbon, toolbar and menus (M4.45); recovery copies and Recover your work? (M4.46). **Next, by everyday use:**
   POINTCLOUDATTACH POINTCLOUD (cloud-cad.js, pointcloud.js: needs raw LAS / LAZ records from io-civil), SWBOUT
   (swsketch.js). LAYER NEW OPEN SAVE SAVEAS DXFOUT HELP are the window's own buttons. Also missing: a DWG's own linked drawings found next to it
   when it is opened for editing (the viewer finds them; in the editor XREF Reload takes the file); objects with a dense linetype draw continuous past 400 dashes (`DISPLAY_MAX_DASHES`); DXF 2000 save runs
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
3. **Viewer details** vs CadCraft 0.5: the viewer finds a DWG's picture files only next to it, on the desktop (pictures
   plot when the drawing has them, M4.21), per-viewport frozen layers, print view /
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
   section sheets PDF now runs 1.4× the JS (S.2).
5. **ModelCraft** (M6.17: panels, components, 3D-print check, inference keys at parity): missing imported pictures as textures in the view (`img:` paints show their average colour), the scenes' WebM recording,
   terrain, the starter library, Blender key style. Copy / paste, duplicate, flip and 3D text: M6.19; solids, intersect
   faces, storeys, drawing axes: M6.20; components that stretch (part rules, stretch dialog): M6.21; sun shadows in the view: M6.22; material textures (view and swatches): M6.23; Lights panel and lamp editor: M6.24; Render a picture (Fast and High quality; thin glass, no lamp shadows in Fast): M6.25; the scenes' Play: M6.26.
6. **BimCraft** (M7.13: IFC edit + save, storey plan + PDF, Draw tools, schedules at parity; M7.14: plan grips — wall
   ends and middles, doors / windows slid along or onto walls — at parity; M7.15: library items (`*.item.json`) placed
   from the plan with turn, wall snap, typed X, Y and the item-number dialog, ifc-place.js at parity): no residential
   sync (res-follow.js: foundations, countertops, stairs, electrical, framing follow the walls; terrain only once the
   ground is loaded); no Library panel (LibraryCraft / feeds search, the open drawing's blocks, the Files… adapters for
   DWG / DXF / IFC / glTF / OBJ / RFA / CSV), swap / update of placed items, copy / paste of placements.
7. **PDF sheets: MarkupCraft and TakeoffCraft** (M8/M9): every 0.5 MarkupCraft drawing tool (M9.14) with select /
   move / grips / window, the text editor and the Properties panel, save marked-up PDF at parity; takeoff count / linear
   / area, exports, Auto count. Copy / paste, the Markups list and its exports, replies, sheet rename and the Pages menu
   (M9.16); picture and signature stamps and the tool chest (M9.17); cut-outs, spaces, legends, sheet links and the
   punch list (M9.18); BCF export / import, compare's picture overlay, line-up and whole-set compare (M9.19). Missing in
   MarkupCraft: auto-link (sheet callouts found in the drawings' text), Find text, slip-sheet, flatten, print,
   bookmarks, the custom columns editor, closing one PDF of a plan set; BCF snapshots are drawn by hayro (their
   pictures differ from 0.5's canvas).
   TakeoffCraft has its item / mark panels, assemblies, bid areas, alternates, deducts, slopes and cut-outs (M9.13);
   arcs and circles, corner and arc drags, several marks selected and the clipboard (M9.20); typical groups, notes,
   clouds and highlights (M9.21); revisions: compare, align, find changes, move the takeoff, flags (M9.22; pixel counts
   are hayro's, not pdf.js's); the 3D view, its stack, picks and GLB (M9.23); still missing there: the sheet's picture
   on the 3D ground, the 3D view's PNG and standard views; snapping to the drawing's linework, typed lengths, 45°.
   Copy + paste in place of 251 marks is 0.7× the JS (S.6: 2.1 → 0.9 ms against 0.6–0.8; what is left is the clip read
   back as a project, the undo snapshot of the whole state and the quantities, all on serde values).
8. **PartCraft** (M13): part view on the mesh kernel — open part / script, sample, 3D with face picking, history,
   parameters, undo, rollback, suppress, Save, STL / 3MF / STEP / IFC at byte parity; the sketcher (every 0.5 sketch
   tool, relations, dimensions, live drag) and the feature tools (extrude … shell, edge / face picking, edit, delete, move,
   rename) at byte parity (M13.3); rebuilds over 60 ms run on a thread and a parameter edit follows `configs` (M13.4).
   Missing: sketch Project (needs exact edges), sketch copy / paste between parts, the reference-geometry / sweep / loft /
   surface / body / sheet-metal / rib / thread tools of the More menu, assemblies, drawings (PDF), DXF of a sketch or
   face, measure / mass / section, design-table CSV import, the exact kernel's features (they say "not built", as 0.5's
   mesh build does), Spanish words.
   **Studio** (M16): the local control channel (M16.1) and the headless MCP server with 0.5's 22 tools (M16.2) are in; LISP scripts are in (M4.40; `drawing_plot` since M4.12, plot.js ported). The window's live tools are in (M16.3, drawing_plot M4.12) except view_screenshot and DetailCraft's (app_info only). Studio Live's protocol core passes the shared vectors (M16.4); its network side (WebSocket to a host or the relay, the PAKE handshake, sealed frames, snapshots, host handoff) and the app adapters are not built. The start screen is in (M16.5); Studio's suite parts (installs, updates, licence, assistants page, Live hosting UI) are not.
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

11. **Local-time dates (M16.8):** every date 0.5 writes in the user's local time is now written in the local time in the
   ported apps too: zip dates (MarkupCraft XLSX / BCF, TakeoffCraft XLSX, DetailCraft NC / DWG zips), the KISS header,
   CivilCraft LandXML date / time, and MarkupCraft's list / Properties dates. All harnesses show 0 differences under UTC,
   America/Denver, Asia/Kolkata and Pacific/Auckland, and with the clock set at a DST change. Still open: dates in parts
   that are not ported yet must take a `cc_clock::Zone` when they land (CadCraft sheet-set archive zip,
   apparel AAMA, ModelCraft print caption, PartCraft script `clock()`, CivilCraft drive sun offset, BimCraft folder
   dates, activity lists (the recovery dialog's times: M4.46); CivilCraft .dbf / GIS zips once the GIS export is in the app). Spec gap: years
   before 1 print as plain numbers in the English date text; the hub's relative times are in English only.

Closed: DWG reading R13–R2018 at full-model parity with 0.5 (M2, 2026-10-16); DXF reading and the 2D viewer (M3);
the command line, LINE … TRIM, undo, selection, DWG writing, CadCraft's DWG import and DWG / DXF saving at byte parity (M4).
