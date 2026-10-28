---
title: Components that stretch
summary: Make one copy of a component wider, deeper or taller while its parts keep their sizes: a longer table with the same legs, a taller bookshelf with its shelves spread out, a railing whose posts repeat.
section: tasks
order: 41
keywords: dynamic component, stretch, resize component, parametric, named sizes, repeat, array, legs, shelves, railing, rules, variables
---

Scaling a component makes everything in it bigger, legs and rails too. **Stretch this copy** changes a component's width, depth or height while each part follows a rule: a table top grows, its legs keep their thickness and stay at the ends.

It works on components made of parts, that is, groups or components inside the component, as every [starter library](topic:starter) thing is.

## Stretch a copy

1. Select the component.
2. In {{ui:Entity info}}, click {{ui:Stretch this copy…}}.
3. Type the new {{ui:Width (red)}}, {{ui:Depth (green)}} and {{ui:Height (blue)}}, and click {{ui:Stretch}}.

Only that copy changes: it becomes a component of its own, which remembers the original it came from. Stretch it again at any time, and it is worked out afresh from the original, so nothing drifts. Other copies of the original stay as they were.

## The rules

Open the component (double-click it) and select one of its parts. {{ui:Entity info}} shows {{ui:When the component stretches}}, with a rule along red, green and blue:

| Rule | What the part does | For example |
|---|---|---|
| {{ui:Stretch}} | Grows with the component; its distances to both sides stay | A table top |
| {{ui:Stay at the start}} / {{ui:Stay at the end}} | Keeps its size and its distance to that side | Legs |
| {{ui:Move in proportion}} | Keeps its size; its middle moves in proportion | Shelves, a door handle |
| {{ui:Scale in proportion}} | Its place and size both in proportion | The steps of a stair |
| {{ui:Repeat}} | Copies of it, every so often ({{ui:Every}}), from where it starts to the far side | The posts of a railing |

{{ui:Automatic}} (the start) works one out and shows which: {{ui:Stretch}} when the part spans the component, {{ui:Move in proportion}} when it sits about in the middle, otherwise it stays at the side it is nearer to.

## Things to know

- The parts' own shapes are shared, so a stretched copy is light. Editing a part changes it in every copy.
- A stretched copy does not follow later changes to the original's rules or layout: stretch it again to bring them in.
- Faces drawn loose in the component (not in a part) keep their size; make them a part to have them follow.
- Rules and what each copy was stretched from are saved in the ModelCraft file.
