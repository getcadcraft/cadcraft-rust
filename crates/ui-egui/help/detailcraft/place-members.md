---
title: Place columns, beams and braces
summary: Pick a profile, material and position, then click grid intersections or type them in to place steel members.
section: tasks
order: 40
keywords: column, beam, brace, member, place, profile, material, rotation, position, offset, insert, add by grid
---

Members are placed on grid intersections: a column on one, a beam or a brace between two. Set up [grids and levels](topic:grids-and-levels) first.

## Choose a tool and its options

1. Choose {{ui:Column}} ({{key:C}}), {{ui:Beam}} ({{key:B}}) or {{ui:Brace}} ({{key:R}}) in the {{ui:Place}} group.
2. The {{ui:Properties}} panel on the right becomes "Place a column" (or beam, or brace) and shows the options for the next member:
   - {{ui:Profile}}: pick from the list. Sizes are grouped by catalog (W shapes, Channels, Angles, HSS, IPE, HEA, HEB, and your Custom profiles). See [Profile catalog](topic:profile-catalog).
   - {{ui:Material}}: A992, A36, A572 Gr 50, A500 Gr C, S235, S275 or S355.
   - {{ui:Position}}: where the section sits on the line between the two points: {{ui:Top of steel on the line}}, {{ui:Centered on the line}} or {{ui:Bottom on the line}}.
   - {{ui:Rotation (°)}}: turns the section about the member's own axis.
   - {{ui:Up to}} (columns only): {{ui:The next level up}}, or a level name.
3. Below the options, a {{ui:Profile}} box lists the chosen section's dimensions, area, mass per meter and the catalog it comes from.

The defaults are W12x50 in A992 for columns (centered), W16x26 in A992 for beams (top of steel on the line) and HSS6x6x1/4 in A500 Gr C for braces (centered). The options stay set until you change them.

## Place by clicking

### A column

1. Choose {{ui:Plan}} and pick the level the column starts at.
2. Choose {{ui:Column}}.
3. Move the pointer near an intersection. The nearest dot lights up and the status bar names it.
4. Click. The column runs from that level to the next level up (or to the {{ui:Up to}} level). The status bar says, for example, "Column W12x50 at A/1, Level 1 to Level 2."
5. Keep clicking to place more. Press {{key:Esc}} when you are done.

If there is no level above the one you clicked, the status bar says "There is no level above Level 2 for the column to reach. Add a level first."

### A beam

1. Choose {{ui:Plan}} and pick the level of the beam.
2. Choose {{ui:Beam}}.
3. Click the first intersection. The status bar says "From A/1 at Level 2. Now click the other end (Esc cancels)." A dashed line follows the pointer.
4. Click the second intersection. The beam appears.
5. Click again to start the next beam, or press {{key:Esc}}.

{{key:Esc}} after the first click cancels just that point; a second {{key:Esc}} puts the tool away.

### A brace

A brace runs between two intersections at different levels, so it is easiest in {{ui:Elevation}}.

1. Choose {{ui:Elevation}} and pick the lettered grid line the brace is on.
2. Choose {{ui:Brace}}.
3. Click the bottom end, then the top end.

Elevations exist only for lettered grid lines. For a brace in a frame along a numbered line (from A to B), use {{ui:3D}} or the {{ui:Add by grid}} form below.

:::touch
On a touch screen, tap close to the intersection: a tap picks the nearest dot within a short distance and does nothing if no dot is near. There is no highlight before you tap, so check the status bar message after each tap. One-finger drags turn or pan the view instead of placing.
:::

## Place by grid, without clicking

Every placing tool has an {{ui:Add by grid}} form at the bottom of the {{ui:Properties}} panel. It works with the keyboard alone and does not need the 3D view.

1. Choose the tool, for example {{ui:Beam}}.
2. In {{ui:Add by grid}}, pick {{ui:From grid (letter)}}, {{ui:From grid (number)}} and {{ui:From level}}.
3. Pick {{ui:To grid (letter)}}, {{ui:To grid (number)}} and {{ui:To level}}.
4. Choose {{ui:Add}}.

For a column or a footing the form has one point only: {{ui:Grid (letter)}}, {{ui:Grid (number)}} and {{ui:Level}}.

## Change a member after placing it

1. Press {{key:Esc}} (or choose {{ui:Select}}, {{key:V}}) and click the member.
2. In the {{ui:Properties}} panel, change {{ui:Name}}, {{ui:Profile}}, {{ui:Material}}, {{ui:Position}} or {{ui:Rotation (°)}}.
3. To move it, edit the x, y and z of {{ui:Start}} or {{ui:End}} under {{ui:Ends (x, y, z mm)}}. The grid name of each end shows underneath, or its coordinates when it is off the grid.

Each change is one undo step. Below the fields you see the member's {{ui:Part mark}}, {{ui:Assembly}}, {{ui:Cut length}}, {{ui:Weight}} (gross: holes and copes are not taken off) and which catalog the profile comes from.

Members cannot be dragged or arrayed yet. To make another member like it, copy and paste it, or duplicate it ({{key:Mod+D}}): see [Copy, paste and duplicate](topic:copy-paste).

## Delete a member

Select it and press {{key:Delete}} or {{key:Backspace}}, or choose {{ui:Delete}}. Connections on it go too. {{key:Mod+Z}} brings everything back.

## Things to know

- **Which way a column faces.** At {{ui:Rotation (°)}} 0, a column's flanges face along x, toward the neighboring numbered grid lines. A beam from grid 1 to grid 2 frames into a flange; a beam from A to B frames into the web. End plates are built only into flanges, so if your beams run from A to B, give those columns a rotation of 90.
- **Beams at a level hang below it** with the default {{ui:Top of steel on the line}}. Choose {{ui:Centered on the line}} if your levels are member centerlines.
- **Columns run level to level.** For a column through two storeys, set {{ui:Up to}} to the higher level.
- **Sloped beams** are allowed (pick ends at different levels), but end plates are not built on them yet.

Next: [Connect beams to columns with end plates](topic:end-plates).
