---
title: Hide, isolate and see through elements
summary: Pick several elements, hide them, show only them, or show them with everything else see-through, then bring everything back.
section: tasks
order: 24
keywords: hide, isolate, see-through, transparent, ghost, x-ray, show all, unhide, select several, multiple selection, ctrl click, shift click, visibility
---

## What you'll do

Clear the view down to what you are looking at: hide elements that are in the way, show only the ones you picked, or keep everything else on screen as a faint outline so you still know where you are.

## Steps

1. Open an IFC model. See [Open an IFC model and look around in 3D](topic:open-ifc-model).
2. Click an element to pick it. To pick more, hold {{key:Mod}} or {{key:Shift}} and click each one. Clicking a picked element that way takes it out again.
3. Open {{ui:Show}} in the toolbar and choose what to do, or press its key:
   - {{ui:Hide}} ({{key:H}}) hides the picked elements.
   - {{ui:Isolate}} ({{key:I}}) shows only the picked elements.
   - {{ui:See-through}} ({{key:T}}) shows the picked elements as they are and everything else see-through.
4. Click {{ui:Show all}} in the same menu, or press {{key:Shift+H}}, to show every element again.
5. To cut away everything round the picked elements instead, click {{ui:Section box round the pick}} in the same menu, or press {{key:Shift+X}}. See [Cut an IFC model with a section plane](topic:section-plane).

## What you should see

- With several elements picked, {{ui:Properties}} says how many and lists them by class. Click one on its own to see its properties.
- Hidden elements cannot be picked: a click lands on whatever is behind them.
- See-through elements cannot be picked either, so you can click straight through walls to what you isolated.
- Edges hide with their element, and turn faint with it.
- Hiding, isolating and see-through stay as they are while you edit, place library items or cut the model with a section plane. Opening another model shows everything again.

## Common mistakes

- **The menu items are greyed out.** Nothing is picked. Click an element first; point at an item to read why it cannot act.
- **Isolate showed a wall I had hidden.** Isolating shows exactly what you picked, hidden or not.
- **A whole storey is missing.** That is the {{ui:Storeys}} list, not {{ui:Show all}}: tick the storey again.

## Next

- [Show and hide storeys, spaces and edges](topic:storeys-and-spaces)
- [Cut an IFC model with a section plane](topic:section-plane)
