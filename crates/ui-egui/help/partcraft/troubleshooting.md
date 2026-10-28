---
title: Troubleshooting
summary: Problems you may meet in PartCraft, from the app not starting to features failing, sketches that will not solve, files that will not open and joints that misbehave, with the message you see and the fix.
section: trouble
order: 10
keywords: problem, error, help, not working, fails, failing, crash, blank, webgl, webassembly, kernel, not built, over defined, open profile, cannot open, nothing happens, wrong direction, slow
---

When something does not work, read the **status bar** at the bottom first: PartCraft says there why it refused or what it expects next. A failing feature also shows its message at the top of its right panel and when you hover its row in the history.

## The app does not start

### "This browser cannot show 3D (WebGL is off or missing), so PartCraft cannot run here."

**Cause:** the browser has 3D graphics (WebGL) turned off, blocked, or not supported by the graphics driver.

**Fix:**

:::chromium
Open `chrome://settings/system` (or `edge://settings/system` in Edge) and turn on "Use graphics acceleration when available", then restart the browser. If it is on, check `chrome://gpu` for WebGL status, and update your graphics driver.
:::

:::firefox
Open `about:support` and look at Graphics, WebGL. If it is blocked, update the graphics driver. In `about:config`, `webgl.disabled` must be `false`.
:::

:::safari
WebGL is always on in current Safari. Update macOS (Safari updates with it). If it still fails, try Chrome or Firefox on the same Mac to rule out the graphics hardware.
:::

Remote desktops and some virtual machines have no WebGL. Try on the computer itself.

### "The solid kernel did not load: …"

**Cause:** the geometry kernel (a WebAssembly program) could not be fetched or started. Usually the connection dropped while the app loaded, WebAssembly is turned off by a company policy or a browser hardening setting, or the browser is very old.

**Fix:** reload the page while online. If it keeps failing, try another current browser. In Firefox, `javascript.options.wasm` in `about:config` must be `true`. Offline, the app and its kernel load only if the app bar said **Offline ready** before you lost the connection ({{ui:Save for offline}} while online; see [Working offline](common:offline)).

### The offline button says "Could not save" or "Offline not available"

**Could not save**: some files did not download. Click it to try again while online; a slow or metered connection may need a second try. **Offline not available**: this browser window cannot keep an offline copy (private windows and some managed computers block it). Use a normal window, or keep the tab open.

### PartCraft is offline but out of date

Click **Offline ready** while online and choose {{ui:Save again (get the newest version)}}.

### "Sweep: not built yet. It needs an exact kernel, …"

The full message is "<Name>: not built yet. It needs an exact kernel, which this version of PartCraft does not have. Help › What is not built yet says what to do instead." You see it when you choose Sweep, Loft, Rib, a sheet metal flange, a surface feature, Combine, Move body, Scale body or Path pattern, and for the other things only an exact kernel does. Nothing is wrong with your browser or your part: this version of PartCraft builds every part with the mesh kernel. See [What is not built yet](topic:not-built-yet) for what to do instead, and [The mesh kernel](topic:solid-kernels).

### "Loading the solid kernel…" never goes away

Wait a little on a slow connection: the kernel and the 3D library are downloaded the first time. Then reload. If the page itself shows an error, see above.

## Nothing happens when I press a key

- Keys do not work while the cursor is in a box in a panel. Click in the 3D view or press {{key:Esc}} after typing (or {{key:Tab}} out).
- Arrow keys and {{key:Plus}}/{{key:Minus}} turn and zoom the view only after you click in the view.
- A dialog is open: answer it first.
- In a sketch, {{key:F}} is the Fillet tool, not Fit. Use the {{ui:Fit}} button.

## A toolbar button is grayed out

- Feature buttons are grayed while a sketch is open. Close the sketch first.
- Sketch tools work only inside a sketch.
- {{ui:Undo}} and {{ui:Redo}} are grayed when there is nothing to undo or redo.
- {{ui:Assembly}} needs a part with a solid. {{ui:Joint}} needs two parts in the assembly. {{ui:Edit part}} needs a part selected.
- {{ui:STEP AP242 (exact solid)}} and {{ui:STEP AP214 (exact solid)}} are always grayed: exact STEP is not built yet in this version. Use {{ui:STEP (faceted, mesh kernel)}}.
- {{ui:Sweep}}, {{ui:Loft}} and the other features that need an exact kernel are not grayed: they stay clickable, their tooltips end with "not built yet (needs an exact kernel)", and clicking one says so in the status bar. See [What is not built yet](topic:not-built-yet).
- {{ui:Drawing}} needs a part with a solid, and is grayed while a sketch is open.
- {{ui:Export}} items are grayed in an assembly: assemblies cannot be exported yet. Close it, or edit a part, to export.

## Sketch problems

### The sketch will not let me add a relation

"No relation fits this selection. Select one or two items (Shift+click adds)." The relation needs a different selection. See [Sketch relations and dimensions](topic:sketch-relations) for what each needs.

### "Over defined: … cannot all be met. Undo, or delete one of them."

**Cause:** the new relation or dimension contradicts the ones already there. The sketch did not move.

**Fix:** {{key:Mod+Z}}, or delete one of the red items in the right panel's list (choose **Delete** beside it).

### "Over defined: … is already implied. Delete it."

**Cause:** the relation adds nothing: others already force it (for example Parallel on two sides of a rectangle, or a dimension that is already fixed). There are no driven dimensions in this build.

**Fix:** delete it. The feature builds anyway, with a warning.

### "Those items cannot be dimensioned together."

You clicked, for example, a point and a circle. Dimension to the circle's center point instead.

### "A fillet needs a corner where exactly two lines meet." / "That radius is too big for these lines."

Click exactly on the corner point of two solid lines. Use a smaller radius than the shorter line allows.

### The geometry jumps back when I drag it

The drag would break a relation or a dimension, so PartCraft puts it back. Only blue (under defined) geometry can move.

### "This sketch cannot open: Needs Extrude2, which is failing."

The sketch sits on a face of a feature that is not built. Fix that feature first, or unsuppress it, or roll forward.

## Feature problems

| Message | Cause | Fix |
|---|---|---|
| "Make a sketch first: pick a plane, Sketch, draw a closed profile, Exit." | No unused sketch with curves, and none selected | Draw a sketch, or select the sketch to use in the history |
| "The sketch has no closed profile to extrude." (or "to revolve") | No closed loop of solid lines | Close the gaps; switch construction lines back with {{key:Q}} |
| "The profile is open: an end at (x, y) is not joined to anything." | A gap at that point | Open the sketch, drag the end onto its neighbor so it snaps |
| "The profile branches at (x, y): more than two ends meet there." | Three or more lines meet at one point | Delete the extra line, or switch it to construction |
| "A closed loop needs at least three sides or a curve." | Two lines back and forth | Redraw the loop |
| "Through all is for cuts. Use a depth for a boss." | A boss set to Through all | Set End to Blind or Mid-plane |
| "There is nothing to cut on that side of the sketch. Try the other direction." | The cut points away from the material | Tick Reverse direction |
| "There is nothing to cut yet." / "There is nothing to drill yet." | No solid before this feature | Make a boss first, or move the feature down |
| "Draw a centerline (a construction line) in the sketch to revolve about." | Revolve without an axis | Add a centerline, or choose a line in Axis |
| "The profile crosses the axis. Keep it on one side of the centerline." | Profile on both sides of the axis | Redraw half the profile |
| "Put sketch points where the holes go (Point tool), then make the hole." | No loose points in the sketch | Add points with {{key:P}} |
| "The counterbore must be wider than the hole." | Counterbore ⌀ not bigger than Diameter | Make the counterbore wider |
| "The counterbore must be shallower than the hole." | Counterbore deeper than a blind hole | Make it shallower, or the hole deeper |
| "Choose the features to copy." | A pattern or mirror with nothing ticked | Tick a feature under Features to copy |
| "Extrude2 has no solid to copy." | The copied feature failed or is off | Fix or unsuppress it |
| "Needs Sketch3, which is failing." | A feature it depends on is not built | Fix the feature it names |
| "The face it sits on is gone (Extrude1 is not built)." | The sketch's face no longer exists | Fix that feature, or delete the sketch and sketch again |
| "Nothing is left of the part after this." | A cut removed all material | Check the cut's size and direction |
| "The solid kernel could not build this (…)." or "The solid kernel could not combine this with the body." | The kernel refused the geometry | Undo, or change the sizes slightly; faces that exactly touch can cause it |
| "Sweep: not built yet. It needs an exact kernel, which this version of PartCraft does not have." (also Loft, Rib and the others) | A feature made in an earlier build that only an exact kernel can build. It stays in the history and is saved back unchanged | Suppress or delete it; see [What is not built yet](topic:not-built-yet) for what to do instead |
| "A 5 mm round does not fit here (…)." / "A 2 mm chamfer …" | The size is too big for a face next to the edge, or the corner cannot be blended | Smaller size, or fewer edges per feature |
| "Fillet: in this version only straight edges between two flat faces (the mesh kernel builds them). Round or bevel curved edges in the sketch." (also Chamfer) | A picked edge is round, such as a hole rim, or lies on a curved face | {{ui:Pick edges}} and remove it; round or bevel that corner in the sketch instead |
| "A fillet whose radius changes along the edge: not built yet (it needs an exact kernel). Give it one radius." | **Variable radius** is ticked | Untick it and give one radius |
| "Fillet by the mesh kernel: where rounded edges meet they are mitred, not blended." (⚠, a warning) | Picked edges meet at a corner; the rounds meet there in a sharp crease | Nothing to fix: the part is built. Fillet fewer edges if the crease matters |
| "An edge it rounds is gone: the edge between Extrude1 and Cut2." | An earlier change removed the edge | {{ui:Pick edges}}, remove the red edge, pick the new one |
| "Pick the edges in the view, then add them." | The fillet or chamfer has no edges | {{ui:Pick edges}} |
| "Pick the faces to remove (open), then shell." | The shell has no open faces | {{ui:Pick faces}} |
| "The shell could not be made with this thickness (…)." | The wall is thicker than a thin part of the model or a fillet radius | A thinner wall, or larger fillets |
| "Shell: in this version it opens flat faces only (the mesh kernel builds it)." | An open face is curved | {{ui:Pick faces}} and remove it; open only flat faces |
| "Draft: in this version flat faces only (the mesh kernel builds it)." | A drafted face is a cylinder, cone or freeform face | Remove it; draft only flat faces |
| "A face it opens is gone (…)." | An earlier change removed the face | {{ui:Pick faces}} again |
| "Fillet1 changes the body's edges or walls; it cannot be copied. …" | A pattern or mirror copies a fillet, chamfer or shell | Untick it; pattern the cuts first, then fillet |
| "It sits on a face only an exact kernel can find: not built yet in this version." | A sketch on a face the mesh kernel cannot find, such as one made in an earlier build | Sketch on a base plane or a flat face of an extrude, and position it with dimensions |
| "It uses a face or edge of the solid, which only an exact kernel can find: not built yet in this version." | A reference plane, axis or point on a face or edge of the solid | Make it from base planes, sketch points or coordinates instead |
| "Extruding up to a face, the next face or a vertex: not built yet (it needs an exact kernel)." / "A taper: not built yet (it needs an exact kernel)." | An end condition or taper only an exact kernel builds | Use a Blind or Mid-plane depth you measure; leave Taper at 0 |

## Picking problems

- "That face is curved. Pick a flat face or a plane to sketch on." Sketches need a flat face. Sketch on a base plane and position the sketch with dimensions instead.
- A face says "Fixed where picked": it was not made by a sketch edge (for example a face of a pattern copy), so a sketch on it stays put when the part changes.
- Edges are hard to hit: zoom in. An edge wins over the face next to it when the pointer is right on it.

## File problems

| Message | Fix |
|---|---|
| 'PartCraft opens its own .partcraft.json files, exports from the CadCraft macro for SolidWorks (.cadcraft-sw.json) and STEP files. "bracket.igs" is none of these.' | Only those files are recognized, and opening STEP is not built yet in this version |
| '"part.stp" is not a STEP file (it does not start with ISO-10303-21).' | The file is not STEP text |
| "That STEP file is too big (over 60 MB) to import here." | Simplify it at the source |
| "Opening STEP files: not built yet. It needs an exact kernel, …" | STEP import is not built yet in this version. Model the part from its drawing or dimensions; see [What is not built yet](topic:not-built-yet) |
| "That file is too big to be a PartCraft part." on your own saved part | A large STEP solid imported in an earlier build made the part file over 50 MB. Use an earlier copy without it |
| "That file is too big to be a PartCraft part." | The file is over 50 MB; it is not a PartCraft file |
| "This is not a PartCraft file (it is not JSON)." / "This is not a PartCraft file." | The file is damaged or another program's |
| "This file was saved by a newer PartCraft (file version …); this one reads version …" | Open it in the newer PartCraft |
| "Extrude2: the depth is bad." (or another value) | The file was edited by hand or damaged. Use an earlier copy |
| Insert in an assembly says "Part1.partcraft.json has no solid to insert." | The part file has no solid. Open it, build a solid, save, and insert again |

**My saved file is not where I expected.** Saving is a download: look in your browser's downloads folder, or open the downloads list from the browser's toolbar. See [Where your files go](common:your-files).

**I opened a part but the assembly is still on screen.** The part loaded behind the assembly. Choose {{ui:Close}} to close the assembly and see the part. Close assemblies before opening part files.

## Drawing problems

| Message | Fix |
|---|---|
| "Build a solid first, then make its drawing." | Build the part first |
| "No room on the sheet for: detail. Make the detail smaller or drop a view." | Smaller **Detail radius** or **Detail scale**, or remove the section |
| "A detail is taken from the front, top, right or section view." | Click on one of those views |
| "Click on one of the views." | The click was on empty paper |
| "Position needs at least one datum reference." and other frame messages | Set the datums the panel asks for, or choose another characteristic |
| "The drawing views could not be made: …" | Undo the last change. If it keeps happening, save, reload and reopen the part |

## Assembly problems

| Message | Fix |
|---|---|
| "Build a solid first, then put it into an assembly." | The part has no solid yet |
| "Insert a second part first (Add copy, Insert, or Edit › Duplicate)." | A joint needs two parts |
| "Pick a face on a different part." | Both faces of a joint must be on different parts |
| "That face is curved. Pick a flat face …" | Joints start from flat faces. Then use **Centre on** for a hole or revolve axis |
| "Bracket2 is not joined to the grounded part, so it stays where it was put." | Add a joint from it to a part that is placed |
| "Revolute3 closes a loop and is off by 2.000 mm. Joints in a loop are not solved yet; remove one." | Remove one joint of the loop |
| "The face this joint uses is gone from Bracket." / "The axis this joint uses is gone from Bracket." | The part was edited. Remove the joint and make it again |
| "The axis runs along the face, so it cannot set the joint's centre." | Choose an axis that crosses the face, or "The face's own origin" |
| "An assembly keeps at least one part. Close it instead." | Use {{ui:Close}} |

**The parts are joined but in the wrong spot.** Set **Centre on** for both sides of the joint to the hole or revolve axes that should line up, then adjust **Angle (°)** and **Offset (mm)**.

## It is slow

- Set **Curves** to Normal or Coarse while modeling; switch to Fine before exporting.
- Large pattern counts (hundreds of copies) take time to rebuild on every change. Suppress the pattern while you work on other features.
- Clash and the motion check compare every pair of parts; big assemblies take longer.
- Close other heavy tabs. PartCraft runs on your own computer's processor and graphics.

## Still stuck

Save your file first ({{key:Mod+S}}), then reload the page and open it again. If a feature still fails, select it, read its message, and try {{key:Mod+Z}} back to the last good state.
