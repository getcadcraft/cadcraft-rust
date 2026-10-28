---
title: Hide, isolate and unhide
summary: Put things out of the way to work between them, or show only what you are working on; hidden things stay hidden in the file.
section: tasks
order: 47
keywords: hide, unhide, unhide all, isolate, hide others, show, hidden, local view, visibility, out of the way
---

Hiding puts things out of the way without erasing them. A hidden face, edge, group or component is not drawn, and you cannot snap to it, click it or select it, so you can work on what is behind or between things.

## Hide

1. Select what to hide: faces, edges, groups or components.
2. {{ui:Edit}} › {{ui:More}} › {{ui:Hide}}.

For example, to work in the narrow gap between two groups standing close together, select the front one and hide it: clicks and snaps now reach the face behind it.

## Isolate

Select what you want to keep in view, then {{ui:Edit}} › {{ui:More}} › {{ui:Isolate (hide the rest)}}. Everything else in the model hides — or, while you are editing inside a group, everything else in that group. A selected face keeps its edges.

## Unhide all

{{ui:Edit}} › {{ui:More}} › {{ui:Unhide all}} shows everything again, in every group. {{ui:Entity info}} says how many things are hidden when nothing is selected, with an {{ui:Unhide all}} button there too.

The {{ui:Outliner}} shows hidden groups and components in italics, with a crossed-out eye; click the eye to show one again.

:::note
With Modeler keys, {{key:H}} hides, {{key:Shift+H}} isolates and {{key:Alt+H}} unhides all. ({{ui:Settings}} › {{ui:ModelCraft}} chooses the key style.)
:::

## Things to know

- What is hidden is saved in the ModelCraft file, and comes back hidden when you open it.
- Hidden things are still in the model: exports include them, and so do volumes and sizes in {{ui:Entity info}}.
- Each hide, isolate or unhide is one undo step.
