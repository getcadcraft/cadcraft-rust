---
title: Troubleshooting
summary: What DetailCraft's messages mean and what to do, from a missing 3D view to connections that will not build.
section: trouble
order: 10
keywords: problem, error, not working, help, fix, message, refused, webgl, blank, warning, cannot, issue
---

Most refusals are explained in the status bar at the bottom of the screen, or in the {{ui:Problems}} tab. Read those first. The messages below are quoted as DetailCraft shows them.

## The 3D view is missing

**"This browser could not start 3D graphics (WebGL). The model, reports and exports still work."**

The browser could not start WebGL, the 3D graphics DetailCraft draws with.

1. Update the browser to its latest version.
2. Turn on hardware acceleration and restart the browser (see below).
3. Update your graphics driver, or try another browser. Remote desktops and virtual machines often have no WebGL.
4. Meanwhile you can still work: place members with the {{ui:Add by grid}} form, read the reports, and export. Only clicking in the view needs WebGL.

:::chromium
To turn on hardware acceleration, open **Settings, System** and turn on **Use graphics acceleration when available**, then relaunch.
:::

:::firefox
To turn on hardware acceleration, open **Settings, General, Performance**, clear **Use recommended performance settings**, and check **Use hardware acceleration when available**. Restart Firefox.
:::

:::safari
Safari has WebGL on by default. Update macOS if the view still does not start, and check that no extension blocks it.
:::

More in [Browsers and computers](common:browsers).

## Clicking in the view does nothing

- **No intersection near the click.** Placing tools snap to grid intersections only. A click away from every dot does nothing. Move closer until the dot lights up.
- **The tool is {{ui:Select}} and you clicked empty space.** That clears the selection.
- **You dragged.** A press and release more than a few pixels apart is a drag: it turns or pans the view.
- **The part is cut away.** In {{ui:Plan}} and {{ui:Elevation}}, steel outside the cut is hidden and cannot be clicked. Switch to {{ui:3D}}.
- **The intersection is at the wrong level.** In {{ui:3D}}, intersections at several levels can overlap on screen; the click takes the nearest one. Use {{ui:Plan}} or {{ui:Elevation}}.

## Placing messages

| Message | What to do |
|---|---|
| There is no level above Level 2 for the column to reach. Add a level first. | Add a level in {{ui:Levels}}, or click a lower level |
| Pick a different intersection for the other end. | You clicked the same point twice |
| The two ends are the same point. | In {{ui:Add by grid}}, the "From" and "To" picks are the same |
| Add grid lines on the left to place members. | The model has no grid lines in one direction |
| Unknown profile "…" | The profile is not in the catalog or the model's custom profiles. Pick one from the list |
| Type a number. | A number field was left empty or has text in it |

## Connections will not build

### "This beam does not end on a column line."

End plates need the beam's end exactly on a vertical column's line, between its bottom and top (within 1 mm). Check the beam's {{ui:Ends (x, y, z mm)}} and the column's position.

### "This beam meets a column web or at an angle"

Or in {{ui:Problems}}: "Beams framing into a column web are not built yet: turn the column 90° or use another connection."

The beam frames into the column web. Select the column and set {{ui:Rotation (°)}} to 90, then try again. Skewed joints are not built.

### "End plates are built for level beams only (this beam slopes)."

Both ends of the beam must be at the same height.

### "End plates need the beam upright (rotation 0)."

Set the beam's {{ui:Rotation (°)}} back to 0.

### "End plates need an I-shape beam." / "End plates need an I-shape column."

Only I-shapes (W, IPE, HEA, HEB or a custom I) take end plates.

### "The beam end is not on the column line any more."

You moved the beam or the column after adding the connection. Move it back, or delete the connection.

### "Connections cut this member to nothing."

The connections at both ends cut the member back further than its length, for example a very short beam between two deep columns. Check the member's ends and the plate thickness.

### "No other joints like this one … are without a connection."

{{ui:Similar}} copies only to joints with the same beam profile, column profile and kind of connection, beam into a flange, that do not have a connection yet.

### "Both ends already have connections." / "This column already has a base plate."

Select the existing connection to change it, or delete it first.

## Warnings in the Problems tab

Warnings do not stop a connection or footing from building; they ask you to check it. The edge and spacing ones use the EN 1993-1-8 Table 3.3 minimums (edge 1.2 × hole, pitch 2.2 × hole, gauge 2.4 × hole). If your project uses other rules, the warning may not apply; DetailCraft does not design connections.

"a hole is off the end of the part" means a hole's x is less than 0 or more than the cut length. Remember x is from the cut start, after connections cut the part back.

## Footing changes are refused

| Message | What to do |
|---|---|
| The cover leaves no room for bars in this footing. | Make the footing bigger or the cover smaller |
| The footing is too shallow for two layers of bars inside the cover. | Make it deeper or reduce the cover |
| The starter bars do not fit inside the footing with 75 mm cover. | Fewer starters, closer spacing, or a bigger footing |
| Bar spacing must be 50–1000 mm. | Type a spacing in that range |
| Length must be 300–50000 mm. | Footing sizes have limits: length and width 300 to 50000, depth 150 to 10000 |

## Grids and levels are refused

| Message | What to do |
|---|---|
| Numbered grids: give one spacing between each pair of lines (4). | Count your gaps: five lines need four spacings |
| Lettered grids: spacings must be positive millimeters. | No zeros, negatives or text |
| Numbered grids: grid labels must be different. | Two lines have the same label |
| At most 60 grid lines each way. | Split the model |
| Keep at least one level. | You removed every level row |
| Every level needs a name. / Two levels are called "…". | Give each level a different name |

## Opening a file fails

| Message | Cause |
|---|---|
| DetailCraft opens its own models (.detailcraft.json). IFC import is not built yet. | The file is not a `.json` file |
| This is not a DetailCraft model (not JSON). | The file is damaged or is some other JSON |
| This is not a DetailCraft model. | A JSON file from another program |
| This model was saved by a newer DetailCraft (format 2). | Open it in the newer DetailCraft that saved it |
| That file is too big to be a DetailCraft model. | Over 50 MB |

When a file opens with warnings such as "2 members were not valid and left out.", the rest of the model is fine. Do not save over the original until you have checked what was left out.

## Exports

| Message | Cause |
|---|---|
| Nothing to export yet. | {{ui:IFC}} with an empty model |
| No I-shape or plate parts to write NC files for. | Only channels, angles or tubes in the model |
| Not written (only I-shapes and plates so far): … | Those marks are not I-shapes or plates; the rest were written |
| No footings, so no bars to schedule. | {{ui:Bending}} with no footings |
| … character(s) outside the PDF font printed as "?". | The model name or a grade has characters the PDF font lacks |

{{ui:Shop drawing}} grayed out: select a part first. Channels, angles and tubes have no shop drawing yet.

## Downloads do not appear

Every save and export is a browser download. Check your browser's downloads list. If the browser asks before downloading several files, or blocks downloads from this site, allow them. In Safari, check the downloads button in the toolbar.

## Keys do nothing

- The cursor is in a text or number field: tool keys and {{key:Delete}} are ignored there so you can type. Press {{key:Tab}} or click the view.
- A dialog is open: close it first.
- The browser has the keyboard (for example the address bar): click inside DetailCraft.
- {{key:Ctrl+Shift+F}} uses Control even on a Mac.

## Offline problems

The {{ui:Save for offline}} button in the app bar shows what state the offline copy is in. How saving works: [Working offline](common:offline).

| The button reads | What it means | What to do |
|---|---|---|
| {{ui:Save for offline}} | No offline copy in this browser yet | Choose it while online |
| Saving 5 of 20 | Saving is under way | Wait for {{ui:Offline ready}}; keep the tab open |
| {{ui:Offline ready}} | DetailCraft, its 3D view, every export and this help work with no connection | Nothing. Click it for {{ui:Save again (get the newest version)}} or {{ui:Remove the offline copy}} |
| {{ui:Could not save}} | Some files did not download | Check your connection and click it to try again |
| {{ui:Offline not available}} | This window cannot keep an offline copy: private windows and some managed computers block it | Use a normal window, or work online |

If you are offline and the button does not read {{ui:Offline ready}}, do not reload or close the tab: what has loaded keeps working. Save your model with {{key:Mod+S}} and wait until you are online again.

## Unsaved work

If the browser asks whether to leave the page, DetailCraft has unsaved changes. Choose to stay, then press {{key:Mod+S}}. DetailCraft cannot recover a model after the tab is closed.

## Still stuck

Note what you did and the exact message, and tell us through the [support page](https://getcadcraft.com/support).
