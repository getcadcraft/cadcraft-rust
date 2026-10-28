---
title: Push and pull faces into solids
summary: Turn a face into a box, make a building taller or shorter, add a bay or a recess, and repeat a distance with a double-click.
section: tasks
order: 30
keywords: push pull, extrude, height, solid, box, mass, taller, recess, bump out, repeat, double-click, volume, level, on face, flush, same height
---

{{ui:Push/pull}} ({{key:P}}) moves a face along its own direction: up and down for a floor, in and out for a wall. What happens depends on the face.

## Make a solid from a face

1. Draw a face, for example a rectangle on the ground ([Draw faces](topic:draw-faces)).
2. Press {{key:P}}.
3. Hover the face. It highlights.
4. Click the face, then move the pointer. The face rises (or sinks) and sides grow from its edges. The value box ({{ui:Distance}}) and the tip show the distance.
5. Click to finish, or type a distance and press {{key:Enter}}: `3m`, `10'`, `2700` in a millimeter model.

   You now have a closed solid. Select it all and group it (see [Groups and components](topic:groups-components)): {{ui:Entity info}} then shows "Solid: Yes" and its volume.

A lone face can go either way: pull it up for a mass above the ground, push it down for a pit or a foundation.

:::tip
Instead of clicking twice, press on the face, drag, and release. On a touch screen that is the natural way: press, drag, lift.
:::

## Pull to the height of something else

While you pull, hover a corner, a midpoint, an edge or a guide of another part of the model. The face snaps to that height and a marker shows the point. Use it to make two blocks the same height, or to stop at a guide line you placed with the [tape measure](topic:guides-measure).

You can also hover a face that is parallel to the one you are pushing, such as the top of another block or the top of the box a post stands on. The tip says "On face", and the face you are pushing ends level with it. For example, push the inner face of a pipe down until it is level with the box under the pipe.

A face at an angle to the one you are pushing doesn't set a level. The distance follows the pointer. So does a face behind the face you are pushing.

When a pushed face ends flush with the faces around it again, its sides go away. No zero-height sides are left over, and the outline you drew stays on the face, so you can push it again later.

## Make a box taller or shorter

Push/pull the top of a box. Because the sides already run straight up from it, the top simply moves: the box gets taller or shorter, and no new edges are made.

- Pushing the top all the way down to the bottom removes the box. ModelCraft stops at zero height; it never turns the box inside out.
- The same goes for any face whose neighbors all run along its direction, such as the end of a wall.

## Add a bay or a recess

1. Draw a rectangle on the side of a box (start it on the face; it stays in the face's plane).
2. Press {{key:P}} and pull the new rectangle **out**. It adds material: a bay, a chimney, a canopy.
3. Or push it **in**. It cuts a recess: a porch, a niche, a set-back entrance.

When the sides of the new part line up with faces already there, they merge into one face, so you don't get stray lines on a flat wall.

## Push all the way through

Pushing a face into a solid stops at the first face behind it. When it reaches that face, it cuts an opening through. That is how you make windows and doors in a wall with thickness. See [Cut an opening through a wall](topic:openings).

## Type a distance

After clicking the face, type the distance and press {{key:Enter}}.

- The typed distance goes the way you were moving. If you haven't moved yet, it goes toward the front of the face (out of a solid, up from the ground).
- Type a negative number, such as `-500`, to go the other way.
- If you change your mind before clicking, press {{key:Esc}}. The face goes back.

## Repeat the last distance

After one push/pull, double-click another face with {{ui:Push/pull}} still selected. It moves by the same distance. This is the fast way to give several walls or several floors the same thickness or height.

ModelCraft remembers the distance until you switch to another tool.

## Things to know

- Push/pull works on one face at a time. If you click something that is not a face, the status bar says "Push/pull works on a face. Click a face."
- To push/pull a face inside a group or component, open it first (double-click it).
- Two solids pushed into each other do not cut or join each other: their faces just pass through. See [Not built yet](topic:not-built-yet).
