---
title: Folder report: roles, statuses and links
summary: Every label in the folder report, what it means, and how the Start here suggestion is chosen.
section: reference
order: 40
keywords: folder report, roles, status, complete, incomplete, unreadable, linked, background, starter, central, family, backups only, start here, link status
---

## Copy status

Every copy of a model, main file and numbered backups alike, gets a header check.

| Label | Meaning |
|---|---|
| **Complete** | The file's header and index fit inside the file |
| **Incomplete** | The file is shorter than its own index says: it was cut off while being copied, uploaded or synced, or it is a cloud file that is still "online only" |
| **Unreadable** | The file is not a valid compound file, or could not be read |

In the **Status** column a model shows **Complete**, an amber "*n* incomplete" when at least one copy is bad but another is sound, or red **Incomplete** when no copy is sound.

## Backups

Files named `Name.0001.rvt`, `Name.0002.rvt` and so on are numbered backups of `Name.rvt`. They are grouped under their model, newest first, in **All … copies**. The card is read from the main file when it is complete, else from the newest complete backup ("newest complete").

## Roles

Shown in italics under a model's name.

| Role | Meaning |
|---|---|
| Linked by *other model*: background | Another model in the folder links this one, so it is most likely someone else's background, such as a consultant's model |
| Starter or untitled file | A name like Blank, Template, Default, Untitled or Project1: a starting point, or a file never properly named |
| Central model | A workshared central model. Work happens in a local copy, not here |
| Family (a part) | An `.rfa` or `.rft` file: a component, not the project |
| Backups only | The main file is gone and only numbered backups remain |

## How Start here is chosen

BimCraft applies the reasoning an experienced user would:

1. Candidates are readable project files (`.rvt`, `.rte`) that are not linked by another model and not starters.
2. Centrals and backup-only models count only when nothing better exists.
3. Of the candidates, the one saved most recently wins.

The box lists its reasons, and warns when a file that looks like the obvious choice is damaged, when other models are background, and when the pick is a central model. It is a best guess; check with whoever last worked on the project.

## Link status

| Label | Meaning |
|---|---|
| **In this folder** | Found at the saved relative path, or at the end of the saved full path inside the folder |
| **Same name found** | Not at its path, but a file with the same name is in the folder. Check it is the right one |
| **Library file** | A keynote or assembly code table. These come with the program on each computer |
| **Not in this folder** | Not found anywhere in the folder |

The **Links found** column in **Models** shows how many of a model's links were found, out of those expected in the folder (library files are not counted).

## Set aside

| Line | Meaning |
|---|---|
| Revit housekeeping folders (not read) | `*_backup`, `*_log` and `Revit_temp` folders, plus hidden folders and `__MACOSX` |
| Lock, log and backup leftovers | `.dwl`, `.dwl2`, `.bak`, `.err`, `.log`, `.tmp`, `.sv$`, `.ac$`, `.lck` and `.slog` files |
| Other files | Everything else (the first 40 are named) |
