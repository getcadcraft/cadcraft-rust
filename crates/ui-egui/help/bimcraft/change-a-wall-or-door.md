---
title: Change a wall, door or window
summary: Click a drawn wall, door or window and change its type, size, height or place in Properties, or drag it in the plan; each change is one undo step.
section: tasks
order: 18
keywords: change, edit, properties, wall type, thickness, length, height, base offset, top, level, flip, outside, door size, window size, sill, distance, move, grip, drag, change type, several
---

## What you'll do

Change what you drew: pick a wall, door or window, then type a new number in **Properties** or drag it in the plan. The plan and the 3D view follow, the corners join again, and doors and windows stay in their walls.

## Change a wall

1. With {{ui:Draw}} on and {{ui:Select}} on, click the wall in the plan (or in the 3D view).
2. **Properties** shows a box with the wall's numbers. Change one and press {{key:Enter}}:
   - **Wall type**: another of the starter types. The thickness and the layers change with it.
   - **Thickness**: any thickness. The type keeps its other layers and its main layer (the studs or the block) takes the difference, for example "Exterior 2×6, 200 mm".
   - **Length**: the wall's end moves along its line. A wall joined at that end comes with it.
   - **Wall height**: **Up to the level above** follows that level (typing a height then sets how far below or above it the top is), **Fixed height** keeps the number you type.
   - **Base offset**: how far above the level the wall starts. Its top stays where it was.
   - **Outside**: {{ui:Flip}} puts the outside layers on the other face.
3. Each change is one step in **Changes**; {{ui:Undo}} takes it back.

## Drag a wall in the plan

When one wall is picked, it shows three grips: a square at each end and a circle in the middle.

- Drag an end to make the wall longer, shorter or turn it. It snaps like the Wall tool; {{key:Shift}} keeps it square.
- Drag the middle to move the whole wall. Walls joined to its ends stretch to follow.
- Doors and windows stay in the wall. A wall too short for what it holds is refused with "It doesn't fit in that wall."

## Change a door or window

1. Click the door or window in the plan.
2. In **Properties**: **Type** (another door or window of the same kind), **Width**, **Height**, **Sill** (windows), **From the start** and **From the end** (the distance from each end of the wall face to its edge), **Hinges** {{ui:Flip}}, and **Swings to** (doors) or **Frame side** (windows) {{ui:Flip}}.
3. Or drag it along its wall, or onto another wall: see [Doors and windows](topic:doors-and-windows).

## Change several at once

Pick several walls (or several doors, or several windows) with {{key:Shift}}+click or {{key:Mod}}+click. **Properties** offers what they share: **Change type…**, the height, the size. One change sets all of them, and one {{ui:Undo}} takes all of it back.

## Good to know

- The numbers show in the units chosen in the **Levels** panel.
- Walls, doors and windows from other programs show their IFC data in **Properties** as before, without this box.
