---
title: Move parts to and from other part modelers
summary: Send a part to another part modeler as a macro that rebuilds it with a live feature tree. The named STEP and opening the feature list another modeler wrote out are not built yet in this version.
section: tasks
order: 88
keywords: macro, not built, swb, vba, feature tree, round trip, export macro, cadcraft-sw.json, named step, ap214, global variables, equations, native part
---

PartCraft exchanges parts with SolidWorks both ways, without a file format nobody else can read:

- **Out:** a macro (`.swb`, plain text) that SolidWorks runs to build the part itself, feature by feature, with its sketches, relations, dimensions, global variables and equations. You get a native part with a live feature tree, not a dead solid.
- **Back:** a macro you run inside SolidWorks writes the part's features and equations to a `.cadcraft-sw.json` file. Opening that file in PartCraft is not built yet in this version (it needs an exact kernel).

:::warning
None of this has been tested in SolidWorks yet. SolidWorks runs only on Windows with a licence, and PartCraft has never run these macros in it. The macros were written from the SolidWorks API documentation and checked for structure here. Expect to report problems; the log the macro writes says which step failed.
:::

PartCraft cannot write a `.sldprt` file: that format is closed. The macro is the way to get a native part.

## Send a part as a macro

1. Open the part.
2. Choose {{ui:Export}}, then {{ui:Macro for SolidWorks (.swb)}}. PartCraft downloads `<part>.swb`; the status bar says it was "written without the exact kernel".
3. A dialog says how many features, global variables and equations the macro builds, and lists anything it leaves out. No STEP goes beside it in this version: use {{ui:Export}}, then {{ui:STEP (faceted, mesh kernel)}} if you need one.
4. In SolidWorks, choose **Tools > Macro > Run**, pick the `.swb` file and wait. It makes a new part from your default part template, sets millimetres and builds every feature.
5. At the end a message says how many steps were done, how many failed and how many matched PartCraft's body. The full log, `<part> macro log.txt`, is written next to the macro.

What the macro builds:

| PartCraft | In the new part |
|---|---|
| Sketch on the Front, Top or Right plane | A sketch on that plane, with its lines, arcs, circles, points, relations and dimensions (named as in PartCraft, such as `k7`) |
| Sketch on a face | A sketch on the same face, picked by a point on it |
| Sketch on a fixed plane | A reference plane through three points, and the sketch on it |
| Extrude, cut (blind, through all, mid-plane, two directions, taper) | Boss-extrude or cut-extrude |
| Revolve, revolved cut | Revolve about the same centerline |
| Hole (simple, counterbore, countersink, tapped) | One or two cuts with their own sketches (a tapped hole is its tap drill; no thread) |
| Linear and circular pattern, mirror | Linear pattern, circular pattern, mirror (features or the body) |
| Fillet, chamfer, shell | Fillet, chamfer and shell on the same edges and faces, picked by points |
| Parameters and equations | Global variables and equations on the renamed dimensions |
| Suppressed features, the rollback bar | Suppressed the same way; the rollback bar in the same place |

After each solid feature the macro compares the body's volume and size with PartCraft's. When they differ, it turns that feature's direction round once and checks again; if that is no better it turns it back and writes "Check" in the log.

### What is left out

The dialog and the top of the macro list everything not carried over, with the reason. Today that is: imported solids (insert the STEP instead), sweeps, lofts, drafts, reference axes and points, "up to" end conditions, extruding only some contours, and anything that depends on them. Splines become curves through their points. This version has no exact kernel, so sketches on faces and the features that pick edges or faces are left out too.

## Send a named STEP

Not built yet in this version: {{ui:STEP for SolidWorks (named, AP214)}} needs an exact kernel, and choosing it says "STEP for SolidWorks: not built yet. …". With an exact kernel, the file carries the part's name, millimetres and the part colour, and stands Y up, the way SolidWorks shows parts (the plain {{ui:STEP AP214 (exact solid)}} export stands Z up). Letters with accents lose them in the name, and other characters outside plain ASCII become `_`.

## Bring a part back

Opening the file in PartCraft (step 3) is not built yet in this version: it says "Opening a SolidWorks export: not built yet. …". The CadCraft drafting app still opens the file and takes its sketches.

1. Choose {{ui:File}} > {{ui:Export}} > {{ui:Export macro to bring parts back (.swb)}} to download `CadCraftExport.swb`, and put it next to your parts.
2. In SolidWorks, open the part, choose **Tools > Macro > Run** and pick `CadCraftExport.swb`. It writes `<part>.cadcraft-sw.json` next to the part and changes nothing in it.
3. In PartCraft, choose {{ui:Open}} and pick that file, or drop it on the 3D view.
4. With an exact kernel, PartCraft rebuilds the part feature by feature and shows what it did: for example "5 of 5 features carried over from SolidWorks (Flange.SLDPRT)", each feature it could not carry ("Not carried over: Sweep1 (Sweep).") and whether the volume matches the one SolidWorks wrote.
5. Save it as a PartCraft part with {{ui:Save}}.

Sketches land on a base plane when they sit on one, on the matching flat face of the part when there is one (so they follow later edits), and on a fixed plane otherwise. Edges and faces of fillets, chamfers and shells are found from points and kept as named references. Global variables become parameters (names with spaces get underscores: "Bolt count" becomes `Bolt_count`) and equations become equations on the same dimensions. Nothing is guessed: what has no counterpart is listed.

The CadCraft drafting app opens the same file and takes its sketches.

:::note
The format is `cadcraft-sw-export` version 1, plain JSON in millimetres and degrees. A file from a newer macro, or one that is not valid, is refused with the reason.
:::
