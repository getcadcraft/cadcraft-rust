---
title: Roll back and forward through the history
summary: Step the rollback bar up and down one feature at a time to watch the part being built, insert features in the middle, and return to the end.
section: tasks
order: 92
keywords: rollback, roll back, roll forward, rollback bar, step through history, insert feature, freeze, history
---

The rollback bar decides how much of the history is built. Features below it are grayed and not built; new features go in at the bar.

## Step one feature at a time

With the part on screen (no sketch or drawing open):

| Do | Result |
|---|---|
| {{key:[}} or the up arrow button next to the part name | The bar moves up one feature |
| {{key:]}} or the down arrow button | The bar moves down one feature |
| {{key:End}} or the bar-to-the-end button | Every feature is built again |

The status bar says where the bar is, for example "Rollback bar after Sketch1: 1 of 2 features built". The buttons are off when the bar cannot move further that way.

You can also choose {{ui:Roll back to here}} on any feature, or click **Rollback bar · roll forward** in the history.

## Insert a feature in the middle

1. Step the bar back until it sits where the new feature belongs.
2. Add the feature: it goes in at the bar.
3. Press {{key:End}} to build the rest of the history on top of it.

Every move of the bar is a step you can undo with {{key:Mod+Z}}. The bar's place is saved in the file, so a part can be sent rolled back. The command line tool moves it too: `partcraft rollback part.json` and `partcraft forward part.json --end`.
