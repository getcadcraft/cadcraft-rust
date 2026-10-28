---
title: Let an AI assistant work with your model
summary: Turn on assistants in Settings so an assistant on your computer can find elements, read their properties and isolate them in the view, through CadCraft Studio. Off until you turn it on.
section: tasks
order: 60
keywords: ai, assistant, mcp, script, automation, studio, find elements, isolate, properties, permissions, disconnect, activity
---

## What you will learn

How to let an AI assistant (any assistant that speaks MCP) look into the IFC model you have open, what it can and cannot do, and how to stop it.

## What an assistant can do in BimCraft

| It asks to | What happens | Needs |
|---|---|---|
| Find elements | It gets a list of elements by class, storey, name or property value, as **Find** does | Read |
| Read properties | It gets one element's attributes, type, property sets and quantities, as **Properties** shows them | Read |
| Isolate, see-through, hide or show | The 3D view changes as if you had used the {{ui:Show}} menu | Change the view |

An assistant never changes the model itself: it cannot delete, move, add or save anything in BimCraft, and it cannot open files. You open the model; it works with the one that is open.

## Turn it on

Assistants reach BimCraft through CadCraft Studio on your computer; the plain browser has no way in.

1. Open {{ui:Settings}} and choose the **BimCraft** tab.
2. Under **Assistants**, tick **Let AI assistants and scripts use BimCraft**.
3. **Read** is ticked for you. Tick **Change the view** too if the assistant may isolate and hide elements.
4. Connect the assistant in Studio (Studio shows the address and the settings to paste into it).

## While an assistant is connected

A note at the top of the right panel says which assistant is connected and what it can do. Click {{ui:Disconnect}} to end its session at once; it has to connect again to continue.

**Activity**, under **Assistants** in Settings, lists every call the assistant made, newest first, with whether it worked.

:::tip
Ask in plain words, for example "isolate the external walls on Level 1" or "which doors have no fire rating?". The assistant finds the elements and shows them in your view.
:::

## Turn it off

Untick **Let AI assistants and scripts use BimCraft**. Every assistant is disconnected straight away, and the setting is kept for next time.

## See also

- [Find elements by class, name or property](topic:find)
- [Hide, isolate and see through elements](topic:hide-and-isolate)
- [BimCraft settings](topic:settings)
