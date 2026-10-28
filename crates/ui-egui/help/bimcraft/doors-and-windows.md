---
title: Doors and windows
summary: Put doors and windows in drawn walls: point at a wall, see the swing and the distances to the corners, click; the wall opening cuts itself.
section: tasks
order: 17
keywords: door, doors, window, windows, opening, swing, hinge, hand, flip, sliding door, pocket door, garage door, double door, casement, single hung, double hung, slider, fixed window, sill, header, bifold, closet door, French doors, barn door, sidelites, entry door, awning, picture window, transom
---

## What you'll do

Put doors and windows in walls you drew ([Draw walls](topic:draw-walls)). Point at a wall and the door or window shows where it would go, which way it swings and how far it is from each corner. Click, and the opening cuts itself out of the wall in the plan and in 3D.

## Place a door

1. With {{ui:Draw}} on, click {{ui:Door}} (or press {{key:D}}).
2. Choose the type: **Single swing door**, **Double swing door**, **Sliding door**, **Pocket door**, **Garage door**, **Bifold closet door**, **French doors**, **Barn door (on the wall)** or **Entry door with sidelites**. Its usual size fills **W** and **H**; type others if you like, for example `2'-8"` or `800`.
3. Point at a wall. The door's symbol follows the pointer along the wall, and the distance from each end of that face shows on the plan.
4. The door swings to the side of the wall the pointer is on. Press {{key:H}} (or click {{ui:Flip}}) to put the hinges at the other end.
5. Click. The door is placed and the tool stays on for the next one. Point at the middle of a wall and the door centers itself.

To place it at an exact distance, point at the wall near the corner you measure from, type the distance (for example `2'` or `600`) and press {{key:Enter}}. The distance runs from the nearer end of the wall face to the edge of the door.

## Place a window

1. Click {{ui:Window}} (or press {{key:N}}).
2. Choose the type: **Fixed window**, **Single hung window**, **Double hung window**, **Casement window**, **Sliding window**, **Awning**, **Picture window** or **Transom**, and its **W**, **H** and **Sill** (the height of the bottom of the window above the floor).
3. Point at a wall and click, or type a distance and press {{key:Enter}} as for doors. A window sits on the outside of the wall.

## Move, flip, delete

- With {{ui:Select}} on, drag a door or window along its wall. Drag it onto another wall to move it there.
- {{key:Delete}} deletes the picked doors and windows; the wall closes up.
- Moving or deleting a wall moves or deletes what is in it. When a wall's start moves along its own line, its doors and windows keep their place.
- {{ui:Undo}} and {{ui:Redo}} take each placement, move or delete back and forth.

## What gets refused

- **It doesn't fit in that wall.** The door or window is wider than the wall, or would stick out of its end. Try a shorter width or a longer wall.
- **It is taller than the wall.** The sill plus the height is above the top of the wall.

## What is saved

Each door or window is an IFC door or window with its size, operation (for example single swing left), type, `Pset_DoorCommon` or `Pset_WindowCommon` and base quantities, filling an IFC opening in its wall. The wall's net side area and volume leave the opening out. Other BIM programs open it as a door in a wall.

## Good to know

- The plan shows doors open at 90° with their swing, sliding doors as two panels, pocket doors sliding into the wall, garage doors as a dashed line overhead, and windows with their frame and glass lines.
- The width is the overall width of the frame; the door leaf is the width less the two jambs.
- Copying drawn doors and windows is not built yet: place another one.
