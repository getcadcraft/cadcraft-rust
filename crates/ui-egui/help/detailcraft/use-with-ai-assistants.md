---
title: Use DetailCraft with an AI assistant
summary: Let your own AI assistant or script read the model, its parts, problems and clashes, add members and connections, save and export: permissions, connecting through CadCraft Studio, and the activity log.
section: tasks
order: 118
keywords: ai, assistant, agent, mcp, model context protocol, script, automation, parts, clashes, connections, permissions, disconnect, activity, undo
---

## What you will learn

How to let an AI assistant (or a script you wrote) work with DetailCraft, and how to decide what it is allowed to touch.

DetailCraft has **no AI of its own** and sends nothing anywhere. You bring the assistant: any program that speaks the Model Context Protocol (MCP). DetailCraft only gives it tools, and you choose which.

## Turn it on

1. Open {{ui:Settings}} (the gear in the app bar, or {{key:Mod+,}}) and go to the {{ui:DetailCraft}} tab.
2. Under {{ui:Assistants}}, tick **Let AI assistants and scripts use DetailCraft**. It is off until you do.
3. Choose what it may do: **Read** is ticked to start with; tick **Edit**, **Save** or **Export** only when you need them.

Tools outside the permissions you ticked are not offered to the assistant, and if it asks for one anyway, DetailCraft refuses and writes it in the activity log. Turning the switch off ends every connection at once.

## Connect your assistant

Assistants connect through **CadCraft Studio** on your computer: Studio runs a small server only this computer can reach, protected by a token it makes each time it starts. In Studio, choose {{ui:Connect an assistant}} and copy the address and the token into your assistant (Studio shows the exact settings to paste). Never share the token.

:::note
A browser tab can't open a connection for an assistant: in the browser apps the switch has no effect until you use CadCraft Studio on your computer.
:::

## You stay in charge

- **Every change is one undo step**, labelled as the assistant's (for example "Assistant: add beam", "Assistant: add gussets"). {{key:Mod+Z}} takes back the assistant's last change as a whole. The assistant's own `undo` only takes back its own steps, never yours, unless it asks for that explicitly.
- **It waits for you.** While you are placing a member, moving parts or pasting, DetailCraft refuses the assistant's changes and tells it to try again.
- **You know when one is connected.** A button in the app bar names the assistant while it is connected; click it to see what it may do, or to **Disconnect** it.
- **Activity.** {{ui:Settings}} › {{ui:DetailCraft}} › {{ui:Assistants}} › {{ui:Activity}} lists every call: when, which tool, done or refused. {{ui:Save the log}} keeps a copy.

## What an assistant can ask for

All lengths are millimetres, points `[x, y, z]` with z up, as in the model.

| Tool | Permission | What it does |
|---|---|---|
| `app_info`, `model_describe` | Read | The model open: grids, levels, phases, members by type and profile, connections, footings, stairs, parts, steel weight, problems |
| `parts_list` | Read | Part marks with profile, material, cut length, count, weight, phase and assembly marks; by type, profile, mark or phase |
| `problems_list` | Read | What the {{ui:Problems}} tab lists, each with what it is about |
| `clash_check` | Read | The [clash check](topic:clash-check), with its tolerance and phases |
| `selection_get` | Read | What you have selected |
| `selection_set` | Edit | Selects members, connections, footings or stairs (by id or part mark) to show them to you; changes nothing |
| `member_add` | Edit | A beam, column, brace or plate between two points |
| `connection_add` | Edit | End plates, shear tabs, base plates, splices or gussets where the given members meet, as the {{ui:Connect}} buttons do |
| `undo` | Edit | Takes back the assistant's last steps |
| `model_save` | Save | DetailCraft's own Save |
| `model_export` | Export | IFC, NC files, BOM, bending schedule, KISS file or cut list, as the {{ui:Output}} buttons make them |

## If something doesn't work

- **The assistant sees no DetailCraft tools:** check that the switch is on in this window, and that the window is open in Studio.
- **"needs the edit permission"** (or save, export): tick it under {{ui:Assistants}}, then ask again.
- **"The person is placing a member":** finish what you are doing (or press {{key:Esc}}), then let the assistant try again.
- **"401" or "token" errors:** the token changed (Studio restarted, or {{ui:New token}}). Copy the new one into the assistant's settings.
