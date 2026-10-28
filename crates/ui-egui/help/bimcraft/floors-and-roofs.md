---
title: Rooms, floors, ceilings and roofs
summary: Rooms appear as walls close them; give a room or the whole building a floor and a ceiling, and put a hip or gable roof on the walls. They all follow when the walls move.
section: tasks
order: 19
keywords: room, rooms, space, area, perimeter, floor, slab, subfloor, ceiling, roof, hip roof, gable roof, pitch, overhang, eave, ridge, valley, 6/12
---

## What you'll do

Finish the shell of the building. Rooms come by themselves from the walls; floors, ceilings and the roof take one click each. Because they are found from the walls, they follow when the walls move.

## Rooms

Every space closed in by drawn walls is a room. It appears as soon as the last wall closes it, with a number (101, 102 … on the first level, 201 … on the next) and a name (**Room 1**, **Room 2** …), and its area shows in the middle of it on the plan.

- With {{ui:Select}} on, click inside a room to pick it. In **Properties** change its **Name** (for example `Kitchen`) and **Number**; its area and perimeter are shown.
- Delete a wall between two rooms and they become one; draw a wall across a room and it becomes two. A room keeps its name and number while the walls round it move.
- A room can't be deleted on its own: it is the space the walls make.

Areas are measured inside the wall faces. Rooms are saved as IFC spaces with their floor area, perimeter, height and volume, so the schedules read them (see [Add up quantities, list elements and schedule spaces](topic:quantity-totals)).

## Floors

1. Click {{ui:Floor}}.
2. Choose **This room** or **The whole building**, the **Kind** (wood floor or concrete slab) and its thickness.
3. Point inside a room (or anywhere inside the building). What the floor would cover is highlighted with its area.
4. Click. The floor's top is the level's floor; it goes down by its thickness. A whole-building floor reaches the outside faces of the walls.

## Ceilings

1. Click {{ui:Ceiling}}.
2. Choose **This room** or **The whole building**, and its **Height** above the floor.
3. Point inside and click.

## A roof

1. On the level whose walls carry the roof, click {{ui:Roof}}.
2. Type the **Pitch** as a rise over 12 (`6/12`) or in degrees (`30`), and the **Overhang**.
3. Choose the **Ends**: **Hip roof** (every side slopes), **Gables on the short sides** or **Gables on the long sides** (a building with four sides), or **Shed**.
4. Point inside the building: the eave and the ridge show on the plan. Click.

The roof sits on top of the walls, on the level above them, and its underside meets the outside face of the walls. Gable ends are filled with a gable wall as thick as the wall below. An L-, T- or U-shaped building gets a hip roof whose wings meet in valleys.

## Change them

Pick a floor, ceiling or roof (in the 3D view, the model tree, or a roof by clicking inside its outline on the level above) and change it in **Properties**: a floor's kind, thickness and how far its top is above the level; a ceiling's height and thickness; a roof's pitch, overhang, ends and thickness. Each change is one undo step.

## Good to know

- Moving, adding or deleting walls makes the rooms, floors, ceilings and the roof over them again.
- A roof fits any outline. To give each wall its own pitch, gable or shed end, a raised plate, fascia and soffit, or to draw a single roof plane, see [Roofs: a pitch for each wall, gables, sheds and roof planes](topic:roofs).
- {{key:Delete}} deletes a picked floor, ceiling or roof.
