---
title: Build an assembly with joints
summary: Start an assembly from a part, insert copies and other part files, join them with rigid, revolute, slider, cylindrical, planar, ball and pin-in-slot joints on flat faces, center joints on axes, edit a part in place and save the assembly.
section: tasks
order: 110
keywords: assembly, mate, mates, joint, rigid, revolute, slider, cylindrical, planar, ball, ball joint, spherical, pin slot, pin in slot, insert component, grounded, fixed, concentric, coincident, edit part, in context, partcraft-asm
---

An assembly puts parts together. In PartCraft, **joints** place the parts: each joint sits on a flat face of each of two parts and puts them face to face. A revolute joint lets one part turn, a slider lets it slide, a rigid joint holds it.

## Start an assembly

1. Build a part with a solid (an empty part cannot go into an assembly: "Build a solid first, then put it into an assembly.").
2. Choose {{ui:Assembly}} in the toolbar.

The status bar says "Assembly started with Part1 as the grounded part. Insert more parts (Add copy, Insert, or Edit › Duplicate), then Joint them." The left panel now lists **Parts** and **Joints**, and the toolbar shows the assembly tools. The sketch and feature tools hide.

The first part is **grounded**: it never moves, and everything else is placed relative to it.

## Insert parts

- {{ui:Add copy}} inserts another copy of the selected part (or of the last one). Copies share one part: edit it and every copy changes.
- {{ui:Duplicate}} ({{key:Mod+D}}) in the Edit group does the same for the selected part. {{key:Mod+C}} and {{key:Mod+V}} copy a part and paste it beside the others, in this assembly or another one; {{key:Mod+Shift+V}} pastes it in the same place. See [Copy, paste and duplicate](topic:copy-paste).
- {{ui:Insert}} inserts a part from a `.partcraft.json` file on your computer. The part is copied into the assembly.

New parts appear 20 mm to the right of everything already there, and stay there until a joint places them. A part with no joint can also be moved by hand: typed values or arrows you drag (see [Parts list, exploded views and moving parts](topic:assembly-bom-explode)).

## Join two parts

1. Choose {{ui:Joint}} or press {{key:J}}. The right panel says **New joint**.
2. Click a flat face on the first part. It highlights.
3. Click a flat face on the second part.

The second part jumps so its face lies against the first part's face, facing it. The joint appears under **Joints**, for example "Revolute1", and the right panel shows its settings.

- If both faces were made by revolves, the joint starts as **revolute**; otherwise **rigid**. Change it after.
- Curved faces cannot be picked: "That face is curved. Pick a flat face (to turn about a hole or a revolve's axis, choose it in the joint afterwards)."
- {{key:Esc}} cancels a joint you have started (or click a part in the list on the left).

### Joint settings

| Setting | What it does |
|---|---|
| **Type** | One of the joints in the table below |
| **Angle (°)** | Turns the second part about the joint's axis (square to the faces) |
| **Offset (mm)** | Moves the second part along the axis, away from the face (negative values push into it) |
| **Along X**, **Along Y**, **Tilt about X**, **Tilt about Y**, **Along the slot**, **Slot direction** | The other values, shown for the joints that use them |
| **Centre on** (for each side) | Where the joint's axis crosses the face: "The face's own origin", the axis of a revolve ("Axis of Revolve1") or one hole's axis ("Hole1, hole 2") |

Every value a joint lets move has a drag slider under its box; the others are set once.

| Joint | What moves | Set once |
|---|---|---|
| **Rigid** | nothing | Angle, Offset |
| **Revolute** | Angle (a hinge) | Offset |
| **Slider** | Offset (a drawer) | Angle |
| **Cylindrical** | Angle and Offset (a shaft in a plain bearing) | |
| **Planar** | Along X, Along Y across the face, and Angle (a part sliding on a table) | |
| **Ball** | Angle (spin) and Tilt about X and Y, about the joint's centre (a ball joint) | |
| **Pin in slot** | Along the slot and Angle (a pin in a straight slot) | Slot direction: the slot's angle from the face's X |

The X and Y of a face are those of the first part's face; the joint's centre is set by **Centre on**.

"The face's own origin" is the part's origin projected onto the face, which is often not where you want the parts to line up. For round parts and pins, set **Centre on** to the revolve's or hole's axis on both sides: the axes then line up, like a concentric mate.

### Example: two flanges bolted face to face

1. Open the flange part (a revolve with a circular pattern of holes) and choose {{ui:Assembly}}.
2. Choose {{ui:Add copy}}.
3. Press {{key:J}}, click the flat face of the first flange's disc (the face the bolt holes start from), then the same face on the second flange. The faces were made by the revolve, so the joint is revolute.
4. Set **Centre on** to "Axis of Revolve1" for both sides. The flanges now share an axis.
5. Type `22.5` in **Angle (°)** to line up the bolt holes, or drag the slider.

## Work with the parts

Click a part in the view or in the list to select it. The right panel shows its part name, how many copies there are, its volume, its joints, and whether it is grounded, set by its joints, or "Not joined: stays where it was put". Buttons:

- {{ui:Edit this part}} (or {{ui:Edit part}} in the toolbar, or double-click the part in the list): opens the part for editing;
- {{ui:Insert a copy}};
- {{ui:Make it the grounded part}};
- {{ui:Remove}}: also removes its joints, after asking.

An assembly keeps at least one part: "An assembly keeps at least one part. Close it instead."

### Edit a part in place

1. Select a part and choose {{ui:Edit part}}. The part opens with its history; the heading says "Bracket (in Bracket assembly)".
2. Change it like any part.
3. Choose {{ui:Back}} in the toolbar. The assembly updates: every copy of that part changes, and joints on faces made by sketch edges follow.

Bringing the change back is one step in the assembly's undo list.

## Undo, save and close

- The assembly has its own undo list: {{key:Mod+Z}} and {{key:Mod+Y}} work on joints, inserts and settings.
- {{ui:Save}} or {{key:Mod+S}} downloads `<assembly name>.partcraft-asm.json`, with every part inside it. Open it later with {{ui:Open}}.
- {{ui:Close}} closes the assembly and returns to the part you started from. If the assembly is unsaved it asks: {{ui:Cancel}}, {{ui:Discard the assembly}} or {{ui:Save first}}.
- To rename the assembly, click empty space so nothing is selected and change **Name** in the right panel.

## Joint problems

- A part no joint reaches is listed as **free** and the panel says "Bracket2 is not joined to the grounded part, so it stays where it was put."
- Joints must form a tree from the grounded part. A joint that closes a loop is not solved: "Revolute3 closes a loop and is off by 2.000 mm. Joints in a loop are not solved yet; remove one."
- If a face or axis a joint uses is gone after editing a part: "The face this joint uses is gone from Bracket." Remove the joint and make it again.

Next: [Check an assembly for clashes and motion](topic:check-assembly).
