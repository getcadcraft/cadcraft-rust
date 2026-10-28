---
title: Solid tools: union, subtract, intersect
summary: Join closed groups and components into one, cut one out of another, or keep only what they share.
section: tasks
order: 46
keywords: solid, solids, solid inspector, not solid, fix solid, hole, stray edge, inside out, union, join, combine, merge, subtract, difference, cut, hole, intersect, intersection, boolean, outer shell
---

The solid tools combine **solids**: groups and components whose faces close a volume, with every edge joining exactly two faces. {{ui:Entity info}} says {{ui:Solid}}: {{ui:Yes}} for one, and shows its volume.

They are in the {{ui:Solids}} menu in the {{ui:Organize}} group.

## When something is not a solid

Select the group or component. When it is not a solid, {{ui:Entity info}} says why, for example "Not a solid: 4 edges border a hole · 1 stray edge":

| Problem | What it means |
|---|---|
| Edges border a hole | A face is missing |
| Edges shared by three faces or more | A face divides the inside, or two shapes touch along an edge |
| Faces inside the shape | A wall left inside the volume |
| Stray edges | Lines that bound no face |
| Faces turned inside out | Their front faces in, against their neighbours |

{{ui:Show problems}} opens it with those edges and faces selected, so you can see them. {{ui:Fix}} mends the simple cases in one undo step: it removes stray edges and faces inside the shape, turns faces the right way out, and closes holes whose rim lies flat. Anything it cannot mend safely (a hole that is not flat, an edge shared by many faces) it leaves for you, and says what is left.

## Union

1. Select two or more solids (click one, {{key:Ctrl}}+click the others).
2. {{ui:Solids}} › {{ui:Union}}.

They become one solid, in one group. Where they overlapped, the faces inside are gone.

## Subtract

1. {{ui:Solids}} › {{ui:Subtract}}.
2. Click the solid to **take away** (the cutter).
3. Click the solid to **cut**.

What is left of the second solid stays; the cutter is used up. For example, to make a hole through a block, draw a post through it, make the post a group, then subtract the post from the block.

## Intersect

Select two or more solids, then {{ui:Solids}} › {{ui:Intersect}}. Only the part they all share is kept.

## Things to know

- The result is a new group. It keeps the name, color and tag of the first solid (for Subtract: the one you cut). Colors painted on its faces are not kept.
- Flat sides of the result are single faces. Round sides come out as the flat facets they were drawn with.
- If a group or component is not a solid, the tool says which one and changes nothing. Look for a missing face or a stray face inside it.
- Inside an open group, the tools work on the groups and components in that group.
- Each tool is one undo step.
- The first time you use a solid tool, its engine (manifold-3d) loads, which can take a moment.
