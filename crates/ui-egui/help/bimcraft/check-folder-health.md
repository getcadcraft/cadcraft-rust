---
title: Check a folder for damaged copies, missing links and leftovers
summary: Use the folder report to find cut-off model files, pick the newest complete backup, see which links are missing, and spot lock and temp files.
section: tasks
order: 90
keywords: health check, damaged, corrupt, incomplete, truncated, backup, missing links, xref missing, audit, leftovers, lock files, dwl, bak, handover
---

## What you'll do

Check a project folder before you hand it over or start work on it: is every model complete, which backup is the last good one, and are all the linked files there?

## Steps: damaged copies

1. Open the project folder. See [Open a whole project folder](topic:open-project-folder).
2. Read the notes under the folder name. "2 files are incomplete or unreadable. See the models below." means there is something to look at.
3. In **Models**, look at the **Status** column. **Complete** is fine. An amber "1 incomplete" means one copy is bad but another is sound. A red **Incomplete** means no sound copy was found.
4. Open **All … copies** under a model with a problem. It lists the main file and every numbered backup, newest first, with size, date and status.
5. Read the note above the list, for example "Tower.rvt is incomplete (183.2 MB, should be at least 214.0 MB). The card shows the newest complete copy, Tower.0012.rvt (Mar 3, 2025)."
6. Click the copy you want to open. Only complete copies can be clicked.

## What the health check does

Every copy of every model gets a quick check of its header, 512 bytes each, so even a folder of large models is checked in seconds. A file whose own index points past its end was cut off while being copied, uploaded or synced. A cloud-drive file that is still "online only" looks the same.

The card for each model is read from the main file when it is sound, and from the newest sound backup when it is not.

## Steps: missing links

1. Look for the note "*n* linked files are not in this folder." under the folder name.
2. Scroll to **Linked and referenced files**. Missing links are listed first.
3. Read the **Where** column for each link:
   - **In this folder**: found at the path the model saved.
   - **Same name found**: a file with the same name is elsewhere in the folder. Check it is the right one.
   - **Library file**: a keynote or assembly code table. These come with the program on each computer, so they are not expected in the folder.
   - **Not in this folder**: not found anywhere in the folder.
4. Read **Used by** to see which models link each file.

BimCraft looks for each link first by its relative path, then by the end of its saved path (so `\\server\jobs\Tower\Cad\X-Refs\site.dwg` is found at `X-Refs/site.dwg` when you picked the `Cad` folder), then by file name anywhere in the folder.

## Steps: leftovers

1. Scroll to **Set aside** at the end of the report.
2. Read the three lines that may appear:
   - Housekeeping folders (`*_backup`, `*_log`, `Revit_temp`): not read.
   - Lock, log and backup leftovers: files such as `.dwl`, `.dwl2`, `.bak`, `.err`, `.log`, `.tmp`, `.sv$`. These are left by programs that crashed or were open when the folder was copied. They are usually safe to delete once nobody has the files open.
   - Other files: everything else BimCraft does not read.

## Common mistakes

- **Deleting the damaged main file before saving a good backup.** Copy the newest complete backup first, then deal with the damaged file.
- **Trusting "Same name found" blindly.** Two different drawings can share a name. Open both if in doubt.
- **Thinking a missing library file is a problem.** Keynote and assembly code tables live with the program on each computer.
- **The file count differs between browsers.** In Firefox and Safari, {{ui:Open a folder}} hands over every file, including those inside housekeeping folders, so the count (and the 20,000-file limit) includes them. Dragging the folder in skips them, as Chrome and Edge do. The report is otherwise the same.
