---
title: Cut an opening through a wall
summary: Draw a window or door on a wall with thickness and push it through, so the wall gets a real hole.
section: tasks
order: 40
keywords: window, door, opening, hole, wall, cut, push through, void, thickness, facade
---

An opening needs a wall with two sides: a box, or a wall you made by pulling a long thin rectangle up. A single face standing on its own has nothing to push through to.

## Make a wall to practice on

1. Press {{key:R}}, click a corner on the ground and type `6m,0.3m` (or `20',12"`), then {{key:Enter}}.
2. Press {{key:P}}, click the rectangle and type `3m` (or `10'`), then {{key:Enter}}.

You have a wall 300 mm (12 in) thick.

## Draw the opening

1. Orbit so you can see the long side of the wall.
2. Press {{key:R}}.
3. Hover the wall's face, then click where the window's lower corner goes. The tip says "On face".
4. Type the window's width and height, such as `1.2m,1.5m` (or `4',5'`), and press {{key:Enter}}.

   The window is a face of its own inside the wall face, which now has a hole around it.

To place it at an exact distance from the corner and the floor, use [guides](topic:guides-measure) first: with {{ui:Tape}}, drag guide lines off the wall's bottom and end edges, then start the rectangle where two guides cross (the tip says "On guide").

## Push it through

1. Press {{key:P}}.
2. Click the window face and push it into the wall.
3. Move until it stops at the back face, then click. Or type the wall thickness, `300` (or `12`), and press {{key:Enter}}.

   ModelCraft stops at the face behind and opens it: the wall now has a hole straight through, with four side faces (jambs, head and sill) lining it. {{ui:Entity info}} still calls the wall a closed solid when it is grouped.

If you push less than the full thickness, you get a recess instead of a hole.

## A door

Draw the rectangle starting on the bottom edge of the wall face (the tip says "On edge" or "Endpoint"), so the opening reaches the floor. Push it through the same way.

## Many windows of the same size

1. Make the first opening.
2. For the next ones, draw the rectangles, then double-click each one with {{ui:Push/pull}}: a double-click repeats the last distance. See [Push and pull](topic:push-pull#repeat-the-last-distance).

## If it did not cut through

- The rectangle must be drawn **on** the wall's face. If it was drawn in the air in front of it, it is a separate face: undo and draw it again, starting on the face.
- The push stops at the first face behind the opening. If something else is in the way inside the wall (an inner face left from earlier drawing), it stops there; erase that face's edges and try again.
- Geometry in a separate group is not cut. Open the group that holds the wall and draw the opening inside it.
