---
title: Phases
summary: Split the model into phases (erection sequences or deliveries), show or hide each phase, put parts in a phase, and number marks per phase.
section: tasks
order: 46
keywords: phase, phases, phase manager, erection sequence, delivery, lot, sequence, hide phase, show phase, filter, numbering, mark prefix, per phase
---

A phase is a group of parts that are made, delivered or erected together: an erection sequence, a delivery, a lot. Every member and footing is in one phase. A connection is in the phase of its main member: the beam of an end plate, the column of a base plate. A new model has one phase.

## The Phases panel

The {{ui:Phases}} section of the {{ui:Model}} panel lists every phase:

- the checkbox shows or hides the phase in the view;
- the number, and a name you can type (empty = "Phase 2" and so on);
- how many parts are in it;
- the {{ui:×}} button removes a phase. Only an empty phase can go: put its parts in another phase first.

{{ui:Add phase}} adds the next number and makes it the phase new parts go in.

## Put parts in a phase

- **New parts:** {{ui:New parts go in}} chooses the phase every member and footing you place from now on goes in.
- **One part:** select it and change {{ui:Phase}} in the {{ui:Properties}} panel.
- **Many parts:** select them (a window, {{key:Shift}}+click, {{key:Mod+A}}) and change {{ui:Phase}} at the top of the {{ui:Properties}} panel. It shows **Varies** when they are in different phases. One change is one undo step.

Copies keep the phase of the part they were copied from, also when pasted into another model (the phase is added there).

## Show and hide phases

A hidden phase is not drawn in the view, cannot be clicked or caught by a selection window, and {{key:Mod+A}} leaves it out. It is still in the bill of materials, the reports, the drawings and every export. Hiding is only for the view: it is not saved and has no undo.

## Marks per phase

With **Number marks per phase** on (it starts on) and parts in more than one phase, each phase is numbered on its own and its marks start with the phase number: part marks 1-b1, 2-b1, assembly marks 2-B1, footings 2-F1. Two identical beams in different phases then have different marks, because they are made and shipped at different times. With one phase in use, marks have no prefix. Turn it off to number the whole model as one, so identical pieces share a mark whatever their phase.

The phase is written into the IFC export (property set DetailCraft, Phase).

:::note
Models with phases are saved in DetailCraft's file format 2. The 0.3 release of DetailCraft cannot open them; this one opens older files, with every part in phase 1.
:::
