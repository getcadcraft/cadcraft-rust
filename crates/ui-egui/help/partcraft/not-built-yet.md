---
title: What is not built yet
summary: Everything this PartCraft build does not do yet, from twisting sweeps to inch display, drawing dimensions and sub-assemblies, with what to do instead today.
section: trouble
order: 20
keywords: missing, not yet, limits, limitations, roadmap, exact kernel, fillet, chamfer, shell, sweep, loft, step import, simulation, manufacture, twist sweep, loft end tangency, face fillet, equations, inches, drawing dimensions, sub-assemblies, cloud, tablet, phone
---

PartCraft is an early build. This page lists what it does not do yet, so you do not search for it, and what to do instead today. Nothing here has a date.

## Needs an exact kernel

This version builds every part with the mesh kernel (see [The mesh kernel](topic:solid-kernels)). Everything below needs an exact geometry kernel, which this version does not have yet. Parts saved with these features still open: the features stay in the history, marked as not built, and are kept when you save.

| Not built yet | Instead, today |
|---|---|
| {{ui:Fillet}} and {{ui:Chamfer}} on round edges; fillets whose radius changes along the edge; rounds blended smoothly where they meet (the mesh kernel mitres them) | Fillet and Chamfer work on straight edges between flat faces; round other corners in the sketch before you extrude ({{ui:Fillet}} in a sketch, {{key:F}}) |
| {{ui:Shell}} opening curved faces; walls of different thicknesses | Open flat faces; add material with extrudes where a wall must be thicker |
| {{ui:Sweep}}, {{ui:Loft}}, {{ui:Draft}} on curved faces; extruding up to a face, the next face or a vertex; tapered extrudes | Extrude or revolve, in steps where needed; extrude to a depth you measure |
| {{ui:Rib}}, {{ui:Base flange}}, {{ui:Edge flange}} and flat patterns | Extrude the plate or the rib's profile |
| Surfaces: finishing a {{ui:Form}} into the part, {{ui:Patch}}, {{ui:Loft surface}}, {{ui:Gordon surface}}, {{ui:Stitch}}, {{ui:Thicken}}, {{ui:Offset surface}}, {{ui:Extend surface}}, {{ui:Trim surface}} | Not available |
| Several bodies: {{ui:Combine}}, {{ui:Move body}}, {{ui:Scale body}}, {{ui:Path pattern}}, features that make a new body | One solid; patterns and mirror copy features |
| Sketches, planes, axes and points on faces that only an exact kernel can find (a face made by a fillet, a hole wall, a face of an imported solid) | Sketch on base planes, reference planes and the flat faces of extrudes and revolves |
| A generative design outcome opened as a part or added to it (it becomes an imported solid) | Download the outcome as STL or STEP |
| Opening STEP files, exact STEP export (AP242, AP214), STEP for other part modelers, opening a part modeler's export | {{ui:Export}} › {{ui:STEP (faceted, mesh kernel)}}, STL or 3MF; the macro for another part modeler still writes |
| True arcs in drawing views | Views and section views are drawn from the facets |
| Exact volumes, areas and distances | {{ui:Measure}} gives the faceted solid's numbers, a little low on round parts; a finer **Curves** setting brings them closer |

Some rows in the tables below suggest one of these features as the way round something else. Those suggestions apply once an exact kernel is back.

## Features

| Not built yet | Instead, today |
|---|---|
| Face fillets, full-round fillets, fillets with radii set at points along an edge | Constant or linear variable {{ui:Fillet}}; several fillet features |
| Shells with different wall thicknesses | One {{ui:Shell}} thickness; add material with extrudes where a wall must be thicker |
| Draft on freeform (spline) faces | {{ui:Draft}} works on flat, cylindrical and conical faces |
| Twisting sweeps, thin-wall and surface sweeps; end tangency on the Loft feature; a loft with one guide rail; spline paths | {{ui:Sweep}} with a fixed orientation or an end scale; a loft with two or more guide rails: see [Sweep, loft and draft](topic:sweep-loft-draft) |
| Ribs from arcs or splines; ribs grown square to the sketch (from a top view); draft on ribs; several ribs from one sketch | {{ui:Rib}} from one chain of lines in the rib's own plane: see [Ribs and gussets](topic:rib) |
| "Up to" a curved surface, "up to next" stopping at a curved face | Up to a flat face or a reference plane |
| Counterdrilled holes, inch and pipe threads, tapered threads, thread run-outs and chamfered thread starts | Counterbore with a small depth; ISO metric and trapezoidal threads, cosmetic or modelled: see [Threads on shafts and bores, drawn or modelled](topic:threads) |
| Thin-feature extrudes | Draw both walls of the profile |
| Pattern copies skipped one by one, fill patterns | Use separate features; patterns along a curve: {{ui:Path pattern}} |
| Copying patterns, mirrors, fillets, chamfers, shells and imported solids | Copy the sketch or feature they are built on, or pattern it |
| Exact (G1) tangency at a form's extraordinary points; merging a form's patches into larger faces | Raise the form's Level: the approximated patches get smaller; see [Freeform shapes with Form](topic:form-surfaces) |
| A form made of several separate closed pieces in one feature; bridges between forms | One closed piece per Form feature; join them with Join |
| Picking surface bodies in the view (except trim and patch points); surface fillets; untrim; G2 patches; Gordon surfaces tangent to curved neighbours (tangent to planes and flat faces is built) | Choose them by name in the feature's panel; thicken first, then fillet the solid |
| A body list in the history; leaving hidden bodies out of exports | Rename and hide bodies in the part panel's body list; delete or suppress the feature that made a body to leave it out |
| A non-uniform scale that keeps cylinders and cones analytic; non-uniform scaling of offset surfaces | {{ui:Scale body}} makes curved faces close B-splines (within 0.0000004 mm); a uniform scale stays exact |
| Combining surface bodies; Minkowski sums | {{ui:Stitch}} or {{ui:Thicken}} first, then {{ui:Combine}} |
| Sheet metal: hems, jogs, miter and contour flanges; flanges on curved edges or on a bent base flange's faces; sketched bends; corner closures; forming tools; converting a solid to sheet metal; the flat of a bent base flange with cuts in it; bend tables | {{ui:Base flange}} (a plate or a bent profile), {{ui:Edge flange}} on a plate's straight edges with bend reliefs, and the flat pattern with a K-factor: see [Sheet metal](topic:sheet-metal) |
| Draft analysis on assemblies; finding the parting line; wall-thickness analysis | {{ui:Draft analysis}} on the part in Design: see [Check draft for moulding and casting](topic:draft-analysis); open each part of an assembly to check it |

## Sketches

| Not built yet | Instead, today |
|---|---|
| Ellipses, tangent arcs, conics, splines with a set degree or weights (NURBS) | Splines (cubic), arcs and circles with Tangent relations |
| A point on a spline (a relation), curvature (G2) relations | Put a spline's own point where it must pass |
| Offset copies of splines that follow later changes; offsets of chains with sharp spline corners | Offset again after the change; round the corner first |
| Pattern copies that stay linked to the original; patterns along a curve | Pattern again after a change |
| Projecting free-form or tilted circular edges, projected geometry that follows the part | Project straight and parallel circular edges, then dimension |
| Driven (reference) dimensions | Read sizes from the panel or the part facts |
| Collinear relations | Parallel with a 0 distance |
| Pasting into another program (the system clipboard) | Export DXF, STEP, STL or 3MF |
| Typing arcs, relative or polar coordinates | Type lines and circles; draw arcs with the Arc tool |
| Selecting geometry without a pointer | Needs a click or a tap |

## Parameters and units

| Not built yet | Instead, today |
|---|---|
| Building a part's saved configurations from the command line by name; configurations that suppress features or change materials | Export them as a CSV design table and pass it with `--table`; configurations set parameters only: see [Configurations and design tables](topic:design-tables) |
| Equations on the numbers of the newest features' own panels (plane, sweep, draft and similar) | Write them in a script or with `partcraft set` ([Scripts and the command line](topic:scripts-and-cli)) |
| A part shown in inches | Sizes show in millimeters; type `2in` in a number box or an expression and it becomes 50.8 |

## Files

| Not built yet | Instead, today |
|---|---|
| Editing an imported STEP solid's own features (it arrives without history) | Add or cut material with your own features |
| Reading colors, names, assembly structure and PMI from STEP | Not available |
| glTF, OBJ, IGES, Parasolid, DWG export | STL or 3MF for meshes; STEP for solids; DXF for 2D |
| Opening STL, IGES, DXF or other programs' own files | Ask for STEP |
| Assembly structure (a tree with part names) in exported STEP | Assembly STEP is one compound of the placed solids |
| Saving to a folder of your choice without the browser's download | Turn on "ask where to save" in the browser's download settings |

## Drawings

| Not built yet | Instead, today |
|---|---|
| Dimensions beyond the three overall sizes (hole sizes, positions, tolerances on sizes) | Mark up the PDF, or export DXF and dimension it in a 2D CAD program |
| More than one section or detail; aligned, broken-out and auxiliary views | One section and one detail per drawing |
| Free text, notes and tables on the sheet (other than the title and revision blocks) | The finish note, or text added on the PDF |
| First-angle projection, moving views, portrait sheets | Third angle, automatic layout, landscape sheets A4–A0 and ANSI A–E |
| Live section view: several planes or a section box at once, an outline round the cut, saving the section in the file | One plane at a time from {{ui:Views}} › {{ui:Section view}}; see [Look inside with a section view](topic:section-view) |
| Assembly drawings with more views, dimensions or balloons you can move | One isometric (or exploded) view with automatic balloons and the parts list |
| Copy and paste of GD&T and finish symbols | Place each with Datum, Frame or Finish |

## Assemblies

| Not built yet | Instead, today |
|---|---|
| Joints in a loop (more than one path to the grounded part) | Keep the joints a tree; remove one joint of the loop |
| Gear, rack, cam and screw joints; a ball joint limited to a cone; joints on curved faces or between points | Rigid, revolute, slider, cylindrical, planar, ball and pin-in-slot joints on flat faces, each value with its own limits: see [Build an assembly](topic:build-assembly) |
| Exploded steps that turn parts, and dragging parts that joints place | Straight explode steps; remove a joint to move a part by hand |
| Sub-assemblies | One level of parts only |
| Gears, cams, contact | Not available |

## Manufacture

| Not built yet | Instead, today |
|---|---|
| 3D roughing and finishing of curved faces, 3D adaptive clearing, rest machining; adaptive passes written as arcs | 2.5-axis face, pocket, 2D adaptive, contour, drill and chamfer: see [Make toolpaths and G-code](topic:manufacture) |
| 4- and 5-axis toolpaths, turning, probing | Set up one side at a time with the tool axis on the Setup tab |
| Holder and shank collision checks | Watch the flute-length warnings and check in your control's simulator |
| Editing posts or adding other controls | Fanuc-style, GRBL or LinuxCNC; adapt the downloaded file by hand |
| Inch G-code (G20) | Millimetres (G21) only |

## Simulation and generative design

| Not built yet | Instead, today |
|---|---|
| Nonlinear studies: yielding, large deflections, contact between parts, fatigue | Linear static and natural frequencies: see [Check stress and deflection](topic:simulation); keep the safety factor well above 1 |
| Nonlinear buckling (imperfections, post-buckling, snap-through) | Linear buckling load factors with a generous margin: see [Heat, thermal stress and buckling](topic:thermal-and-buckling) |
| Radiation between faces (view factors), contact resistance, temperature-dependent materials, inertia in thermal stress (thermal shock waves) | Radiation to the surroundings, steady or quasi-static thermal stress over time, constant properties: see [Heat, thermal stress and buckling](topic:thermal-and-buckling) |
| Automatic refinement for transient heat and studies with a thermal load; making the mesh coarser where the error is small; a target on a local quantity (the peak stress at one spot) | **Refine automatically** for static, buckling, natural-frequency and steady thermal studies, on the error over the whole part: see [Check stress and deflection](topic:simulation); otherwise lower the element size |
| Bearing and bolt loads, remote forces, moments, springs, prescribed displacements | Forces and pressures on faces, gravity; fixed and frictionless faces |
| Simulation of assemblies; materials that differ by direction (printed layers, composites) | One part, one isotropic material |
| Several solver threads for tetrahedral studies, heat transfer and generative design; any threads on a page that is not cross-origin isolated | Voxel studies, buckling, stress over time and laminar flow use the threads where the page allows it: see [PartCraft settings](topic:settings) |
| Turbulent flow with design accuracy (the Spalart–Allmaras model is in as experimental: pipe friction 30 to 34 % high, backward-step reattachment 21 to 39 % short; planned: finer cells near the walls), heat in flow, unsteady (time-accurate) flow, free surfaces, non-Newtonian fluids, rotating parts | Laminar, steady, isothermal flow of a Newtonian fluid, or the experimental turbulent model for qualitative patterns: see [Simulate fluid flow](topic:flow-simulation); the panel warns when the Reynolds number is past laminar |
| Stress limits and manufacturing rules (overhangs, milling directions) in generative design; several materials in one shape | Stiffest shape for a volume; check the outcome in Simulation: see [Grow a shape from loads](topic:generative-design) |
| An outcome as an editable (smooth, parametric) solid | STL, faceted STEP or a part with the outcome as an imported faceted solid |
| Electric machine design: a rotor turning in a stator, torque against angle, saturating steel, permanent magnets, eddy currents and iron and copper losses; magnetic fields in 3-D or on curved meshes | Linear 2-D magnetostatics on a grid in the analysis library: coils, iron, gaps, inductance and pull: see [Magnetic fields in 2-D](topic:magnetic-fields-2d) |

## Scripts

| Not built yet | Instead, today |
|---|---|
| Hover help, breakpoints and stepping in the script editor | Find, fold and complete are in (see [Write and run scripts](topic:craftscript)); for the rest, your own editor: {{ui:Save}} and {{ui:Open}} in the Script group move scripts in and out; built-in functions are listed in [The CraftScript language](topic:craftscript-language) |
| Breakpoints and stepping through a script | `disp` and `fprintf` along the way; {{key:Mod+Enter}} runs a few selected lines at a time |
| Fields and plots a script shows drawn in the Design workspace; scripts changing the part's parameters | They show in the Script workspace; scripts read parameters with `param` (see [Write and run scripts](topic:craftscript)), change them on the {{ui:Parameters}} tab |
| Pictures and tables side by side with plots; editing a plot (zoom, axis limits) before saving it | Side by side shows the plots; save a plot as PNG, SVG or CSV and finish it elsewhere |
| numpy and other Python packages; Python offline; folding Python in the editor; Pascal Script | Python's standard library, CraftScript and Lua: see [Scripts in Python](topic:craftscript-python) |
| Passing Lua functions to built-in functions; calling a library function from Lua by a name built while the script runs | Write the library function's name in the script (`cs.static_tet(…)`), or call it from a CraftScript script |

## The app

| Not built yet | Instead, today |
|---|---|
| Cloud saving, sharing and version history | Not available yet. Save files to your computer and keep versions by name |
| PartCraft as an installed app on iPad, Android and iPhone | Use PartCraft in the device's browser; on the desktop it runs in a browser or in CadCraft Studio |
| Context (right-click) menus | Toolbar and right panel |
| Working fully offline from a fresh start | Only when the app has been saved for offline use: see [Working offline](common:offline) |

Report what you miss most through the site's support page: it shapes what comes next.
