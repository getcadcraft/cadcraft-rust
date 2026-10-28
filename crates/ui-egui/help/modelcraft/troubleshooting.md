---
title: Troubleshooting
summary: Symptoms, causes and fixes, from a blank 3D view to imports at the wrong size, with the messages ModelCraft shows.
section: trouble
order: 10
keywords: problem, trouble, error, not working, blank, webgl, graphics, slow, message, import failed, wrong size, cannot select, touch, keys not working, lost work
---

## The 3D view is blank

**Symptom:** the middle of the screen stays empty, or shows only the background color, and clicking tools does nothing.

**Cause:** ModelCraft draws with WebGL 2, the browser's 3D graphics. It is missing when the browser is old, when hardware acceleration is turned off, or when the graphics driver is blocked.

**Fix:**

:::chromium
1. Open the browser's settings and search for "graphics acceleration".
2. Turn on "Use graphics acceleration when available" and relaunch the browser.
3. If it is already on, type `chrome://gpu` in the address bar (`edge://gpu` in Edge) and look for "WebGL2" in the list. If it says disabled, update your graphics driver.
:::

:::firefox
1. Open **Settings**, then **General**, and scroll to "Performance".
2. Untick "Use recommended performance settings", then tick "Use hardware acceleration when available". Restart Firefox.
3. If it still fails, type `about:support` in the address bar and look at the "Graphics" section for WebGL 2.
:::

:::safari
Update macOS and Safari. Recent versions of Safari always have WebGL 2 on. If you changed experimental features under **Safari**, then **Settings**, then **Advanced** (or **Feature Flags**), reset them.
:::

On a work computer, remote desktop or virtual machine, 3D graphics may be blocked by policy. Try another computer. See [Browsers and computers](common:browsers).

## Keys do nothing, or type into a box

**Cause:** the keyboard focus is in a panel field: the {{ui:Faces}} list in {{ui:Style}}, a sun setting, the {{ui:Name}} field in {{ui:Entity info}}, or the units picker. Letters and arrow keys then go to that field.

**Fix:** click in the 3D view, then press the key again. Keys also do nothing while a dialog is open.

## Typing a number does nothing

- **No direction yet.** A typed length goes the way the pointer points from the first click. Move the pointer a little in the direction you want, then type.
- **Wrong moment.** Most tools take a typed value only after the first click: click the first corner, the face or the point to move from first. {{ui:Circle}} and {{ui:Polygon}} take a number of sides before the first click.
- **"Type a length such as 3m, 10'6" or 4m,3m."** ModelCraft could not read it. Check for a decimal comma (`2,5`): use a period.

## A shape starts in the wrong place on a touch screen

**Symptom:** with a finger, the first rectangle is fine, but the next rectangle, circle or line starts where your previous touch ended instead of where you touched.

**Cause:** a bug in this build: on a touch screen the tool remembers the last snap point from the previous gesture.

**Fix:** tap the tool's button in the toolbar again before each new shape. That starts the tool fresh, and the first point lands where you touch.

## Editing one group changed another

**Symptom:** you copied a group with {{ui:Move}} and {{key:Ctrl}}, opened one copy and edited it, and the other copy changed too. The {{ui:Outliner}} shows both with the same name.

**Cause:** in this build, copying a group makes a second placement of the same group, as with a component. ModelCraft's {{ui:Make unique}} button is only offered for components.

**Fix:** select the copy you want to change, click {{ui:Make component}} in {{ui:Entity info}}, then {{ui:Make unique}}. That copy now has its own contents. Or {{ui:Explode}} it.

## Push/pull does not do what I expected

- **"Push/pull works on a face. Click a face."** You clicked an edge or empty space. Hover until the face highlights, then click.
- **The face is inside a group.** Double-click the group to open it first.
- **A window did not cut through the wall.** The rectangle must be drawn on the wall's face, not in front of it, and the wall must have a back face. See [Cut an opening through a wall](topic:openings#if-it-did-not-cut-through).
- **Two boxes overlap without merging.** Solids pushed into each other do not cut or join each other in this build.

## A line or shape did not make a face

A face fills in only when the edges close a flat loop. Check that the last point snapped to the first ("Endpoint"), and that all the edges lie in one plane. A loop that is slightly out of plane stays as lines. Draw one of its edges again, snapping to the corners.

## I can't select or erase a face

- With {{ui:Select}}, a click near an edge takes the edge. Click nearer the middle of the face.
- The {{ui:Eraser}} does not pick faces. Select the face and press {{key:Delete}}, or erase one of its edges.
- If it belongs to a group or component, the click selects the whole thing. Double-click to open it.

## "Select something to move first, or click on it."

{{ui:Move}}, {{ui:Rotate}} and {{ui:Scale}} need something to work on: select it first, or start the tool by clicking on the thing itself (not on empty space or a guide).

## "Select some geometry first (drag a window, or triple-click to select everything connected)."

{{key:G}} and {{key:Shift+G}} need a selection. Guides cannot be grouped.

## The arrow keys orbit instead of locking

Locks work only after the first click of a drawing or editing tool. {{ui:Push/pull}} has no locks, and in {{ui:Rotate}} they do not change the turn. At other times the arrow keys orbit the view.

## Esc in full screen

In full screen, the browser may use the first {{key:Esc}} to leave full screen. Press {{key:Esc}} again to cancel the tool.

## No middle button

Press {{key:O}} for {{ui:Orbit}} and drag, or use the arrow keys. See [Orbit, pan and zoom](topic:orbit-pan-zoom#without-a-middle-button).

## Imports

| Message | What to do |
|---|---|
| "ModelCraft imports Collada (.dae), OBJ, glTF (.glb/.gltf), DWG and DXF. … is none of these." | Export the model from the other program in one of these formats |
| "This file is over 300 MB." | Simplify or split the model in the other program |
| "…too big to model with here (over 2 million vertices)." | The same |
| "This glTF file keeps its data in a separate file…" | Export it as a single `.glb` |
| "This is not a Collada (.dae) file…" or "…not well-formed XML…" | The file is damaged or not Collada. Export it again |
| "This Collada file has no geometry ModelCraft can import." | It holds only cameras, lights or references to other files |
| "This is a binary DXF. Save it as ASCII DXF and try again." | Save it again from the CAD program as ASCII |
| "This is not a DWG drawing…" | The file is not a DWG, or it is damaged |
| "This drawing could not be read (…). It may be damaged, or saved in a DWG version the reader does not handle…" | The brackets give the reason. Save it as DXF, or as a DWG from AutoCAD 2000 to 2018, from the CAD program |
| "This drawing reaches more than 10,000 km from its origin…" | Move the drawing near 0,0 in the CAD program |

### The import is the wrong size or lying on its side

- **OBJ:** OBJ has no unit; ModelCraft reads meters. A millimeter model comes in 1000 times too big: [scale](topic:rotate-scale#fix-an-import-that-came-in-the-wrong-size) it by `0.001`; an inch model by `0.0254`.
- **OBJ on its side:** ModelCraft expects OBJ files with Y up. A file written with Z up comes in lying down. Rotate it 90° about the red axis: click the {{ui:Rotate}} center on a face that faces along red, or open the file in its program and export with Y up.
- **DWG or DXF:** you picked the wrong unit in the "Drawing units" dialog. Undo and import again.

## Opening files

| Message | What to do |
|---|---|
| "This file is not a ModelCraft model (it is not JSON)." | Use {{ui:Import}} for other formats |
| "This is not a ModelCraft file we can open: …" | The file is damaged or was changed by hand |
| "This model was saved by a newer ModelCraft…" | Open it in the newer version |
| "This file is over 200 MB." | The model is too large to open here |

## I lost work

ModelCraft does not save by itself. When you close or reload a tab with unsaved changes, the browser asks first ("Leave site?" or "Reload site?"); choose to stay and save. New and Open ask too ("Unsaved changes"). If the browser or computer crashed, the work since your last save is gone. Save often with {{key:Mod+S}}.

## My downloads folder fills up with copies

Each save is a new download, named `Untitled (1).modelcraft`, `Untitled (2).modelcraft` and so on. Delete the old ones, or set the browser to ask where to save each file and overwrite the old file yourself. See [Open, save, import and export](topic:files#save-a-model).

## It gets slow

- Every step of undo keeps a full copy of the model, so on a very large import each edit takes longer. Explode or erase what you don't need, save, and reopen: reopening also clears the undo history.
- Sun shadows cost the most. Untick {{ui:Sun shadows}} while you model.

## No shadows

{{ui:Sun shadows}} is off, the sun is down at the time you chose ("The sun is down at this time."), or the style is {{ui:Wireframe}}, which draws no shadows.

## "This browser did not allow full screen."

Some browsers and some embedded views refuse full screen. Use the browser's own full screen instead ({{pick:mac=Control-Command-F|windows linux=F11}}).

## ModelCraft does not open without a connection

ModelCraft opens offline only after it was saved for offline use in that browser: the app bar's button must read {{ui:Offline ready}}. Open it while online and click {{ui:Save for offline}}. If the button reads {{ui:Could not save}}, click it again on a better connection; {{ui:Offline not available}} means this window (a private window, or a managed computer) cannot keep a copy. See [Working offline](common:offline).
