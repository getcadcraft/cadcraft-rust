---
title: Load applications (APPLOAD)
summary: Load LISP routines and their dialog files, unload them, load them again from the history, and keep the ones you use in the Startup Suite.
section: tasks
order: 47
keywords: appload, ap, load application, lisp, lsp, dcl, routine, startup suite, unload, history, drop, load, customize, manage
---

## What you will learn

How to load a LISP routine (a `.lsp` file) so its commands work in CadCraft, how to unload it, and how to have it load every time CadCraft starts.

## Load a routine

1. Open {{ui:View}} › {{ui:More}} › {{ui:Load application…}}, or type {{cmd:APPLOAD}} (or `AP`) and press {{key:Enter}}.
2. Click {{ui:Load a file…}} and choose one or more `.lsp` files. A routine that opens a dialog usually comes with a `.dcl` file: choose it too.
3. The dialog says what each file made, for example "Loaded delblocks.lsp: 1 command, DELBLOCKS."
4. Click {{ui:Close}}, type the routine's command and press {{key:Enter}}.

You can also drop `.lsp` and `.dcl` files on the drawing, or on the Load applications dialog: they load at once.

The file is read here, in your browser. Nothing is uploaded.

:::tip
Press {{key:Enter}} at an empty command line to run the routine's command again, the same as any other command. Your command aliases can reach it too: see "Edit command aliases".
:::

## Unload a routine

In {{ui:Load applications}}, under {{ui:Loaded applications}}, click {{ui:Unload}} beside the file. The commands it made are gone until you load it again.

## Load a routine every time CadCraft starts

1. Load the routine.
2. Under {{ui:Loaded applications}}, click {{ui:Add to Startup Suite}} beside it.

From now on it loads by itself whenever CadCraft starts in this browser. To stop that, click {{ui:Remove}} beside it under {{ui:Startup Suite}}.

The Startup Suite keeps a copy of each file in this browser's storage. If you change the routine, load the new file and add it again.

## Load a file again from the history

{{ui:History}} lists the files you loaded in this browser, newest first. Click {{ui:Load}} beside one to load it again, without looking for it on your computer.

## Files CadCraft can't load

| File | What it is | What to do |
|---|---|---|
| `.fas`, `.vlx` | compiled or protected LISP | load the `.lsp` source instead |
| `.arx`, `.crx`, `.dbx` | native Windows programs | there is no way to run these in CadCraft |
| `.dvb` | a VBA project | there is no way to run these in CadCraft |
| `.craft`, `.lua`, `.py` | CraftScript scripts | open them in PartCraft's Script workspace |

The Load applications dialog says which of these a file is when you choose it.

## Stop a routine

Press {{key:Esc}}. At one of the routine's prompts, Esc cancels it the way the routine expects (its own clean-up runs). If it is busy and not asking anything, Esc stops it outright and says so; your loaded routines stay loaded.

:::note
Everything a routine changes in the drawing is one step: one {{key:Mod+Z}} takes it all back.
:::
