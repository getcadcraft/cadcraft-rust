---
title: Use CadCraft with an AI assistant
summary: Let your own AI assistant or script work with CadCraft: what it may do, how to connect any MCP client, and how you stay in charge (permissions, undo, the activity log, Disconnect).
section: tasks
order: 60
keywords: ai, assistant, agent, mcp, model context protocol, script, automation, command line, cli, json, token, permissions, disconnect, activity
---

## What you will learn

How to let an AI assistant (or a script you wrote) look at a drawing, run CadCraft's commands, plot and save, and how to decide what it is allowed to touch.

CadCraft has **no AI of its own** and sends nothing anywhere. You bring the assistant: any program that speaks the Model Context Protocol (MCP). CadCraft only gives it tools, and you choose which.

There are two ways in:

- **The drawing you have open.** The assistant works in this window, next to you, through CadCraft Studio on your computer. This is what {{ui:Settings}} › {{ui:CadCraft}} › {{ui:Assistants}} is for.
- **Drawings on your computer, without opening the app.** The CadCraft MCP server and the `cadcraft` command line read and write DWG and DXF files in folders you name. See "Work on files without opening CadCraft" below.

## Turn it on

1. Open {{ui:Settings}} (the gear in the app bar, or {{key:Mod+,}}) and go to the {{ui:CadCraft}} tab.
2. Under {{ui:Assistants}}, tick **Let AI assistants and scripts use CadCraft**. It is off until you do.
3. Choose what it may do. **Read** is ticked to start with; tick the others only when you need them.

| Permission | What the assistant can do |
|---|---|
| Read | Look at the drawing: describe it, list and find objects, see the selection, describe the view and take a picture of it, check it against layer and drafting rules |
| Edit | Run commands on the drawing and change the selection. Each action is one step you can undo |
| Save | Save the drawing (on the desktop, to its file; in a browser, as a download) |
| Export | Plot to PDF and make other files |

Tools outside the permissions you ticked are not offered to the assistant at all, and if it asks for one anyway, CadCraft refuses and writes it in the activity log. Turning the switch off ends every connection at once. {{ui:Reset CadCraft settings}} turns it off too.

## Connect your assistant

Assistants connect through **CadCraft Studio** on your computer: it runs a small server that only this computer can reach (address `127.0.0.1`), protected by a token that Studio makes each time it starts. Open Studio, choose {{ui:Connect an assistant}}, and copy the address and the token.

- In your assistant, add an MCP server of type HTTP with the address (it looks like `http://127.0.0.1:47615/mcp`) and the header `Authorization: Bearer <token>`. Studio shows the settings to paste.

{{ui:New token}} in Studio makes every connected assistant reconnect with the new token. Never share the token: anyone with it can use the apps you switched on, for as long as Studio is running.

:::note
A browser tab can't open a connection for an assistant: in the browser apps the switch has no effect until you use CadCraft Studio on your computer.
:::

## You stay in charge

- **Every action is one undo step**, labelled with the assistant's name for it, for example "Assistant: LINE". Press {{key:Mod+Z}} to take back the assistant's last action as a whole.
- **You see what it does.** Its commands appear in the command history with **[Assistant]** in front, and the drawing redraws as it works.
- **It waits for you.** While you are in the middle of a command, the assistant can read but not change the drawing; it is told to try again when you are done.
- **You know when one is connected.** A button in the app bar names the assistant while it is connected. Click it to see what it may do, or to **Disconnect** it.
- **Activity.** {{ui:Settings}} › {{ui:CadCraft}} › {{ui:Assistants}} › {{ui:Activity}} lists every call: when, which tool, done or refused, and the undo step it made. {{ui:Save the log}} keeps a copy.

## What an assistant can ask for

| Tool | Permission | What it does |
|---|---|---|
| `app_info`, `drawing_describe`, `view_describe` | Read | The drawing's name, units, layers, blocks, sheets; what is on screen |
| `objects_list`, `object_get`, `selection_get` | Read | Objects with their geometry, length and area; the selection |
| `drawing_check` | Read | Rules such as allowed layers, nothing on layer 0, colours ByLayer, text heights |
| `view_screenshot` | Read | A picture of the drawing as shown |
| `command_run` | Edit | Types at the command line, one prompt answer at a time, for example `LINE`, `0,0`, `@100,0`, Enter |
| `selection_set`, `undo` | Edit | Selects objects for the next command; undoes its own steps |
| `drawing_save` | Save | CadCraft's own {{ui:Save}} |
| `drawing_plot` | Export | A PDF of a sheet or of the model, at a scale or fitted |

Commands that need a dialog of their own (picking a file, the library, Find) are not run for an assistant: it is told to ask you.

## Work on files without opening CadCraft

The CadCraft MCP server (`cadcraft-mcp`) and command line (`cadcraft`) run CadCraft's own drawing code in Node.js, with no window. They need Node.js 20 or newer.

- **The MCP server** works only inside the folders you name, and only with the permissions you give it: `npx @getcadcraft/mcp --root ~/Drawings --allow read,edit`. Without `--root` it can't open any file; without `--allow` it can only read. It can also read IFC models, takeoffs and the markups in PDF sets.
- **The command line** is for your own scripts: `cadcraft info plan.dwg`, `cadcraft run cleanup.scr plan.dwg -o plan-clean.dwg`, `cadcraft convert plan.dwg plan.dxf`, `cadcraft plot plan.dwg plan.pdf --all`, `cadcraft check plan.dwg --rules rules.json`. Add `--json` for output a program can read.

:::note
If `npx @getcadcraft/mcp` says the package is not found, it has not been published yet. News on getcadcraft.com will say when it is.
:::

## If something doesn't work

- **The assistant sees no CadCraft tools:** check that the switch is on in this window, and that the window is open in Studio.
- **"needs the edit permission":** tick **Edit** under {{ui:Assistants}}, then ask again.
- **"401" or "token" errors:** the token changed (Studio restarted, or {{ui:New token}}). Copy the new one into the assistant's settings.
- **Its change was wrong:** press {{key:Mod+Z}}. One press takes back one whole action of the assistant.
