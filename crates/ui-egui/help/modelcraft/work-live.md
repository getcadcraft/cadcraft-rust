---
title: Model together, live
summary: What changes in ModelCraft while you are in a CadCraft Studio live session: everyone's groups and components in one model, who can reshape what at once, your own undo, and joining from a browser on the same network.
section: tasks
order: 135
keywords: live, studio live, together, collaborate, session, share live, join, group, component, push pull, undo, host
---

## What you will learn

What happens to a model while several people work on it at the same time. CadCraft Studio starts and joins the session (Share live, Join); this page is about the model itself.

## Work in your own group

Everyone edits the same model, and your own edits show at once. How changes merge depends on what you change:

- **Moving, naming, colouring or hiding placed groups and components** merges: two people moving two groups both keep their moves.
- **The geometry inside one group** (its edges and faces: drawing, push/pull, erasing) is one piece. When two people reshape the same group at the same moment, the second change is taken back and the status bar says so. Different groups never get in each other's way.
- **Loose geometry** (not in any group) is the model's own group: only one person can reshape it at a time.

So agree who works in which group, or make a group for each person's part before you start. Guides, dimensions and section planes merge one by one; scenes, tags, storeys and the drawing axes go to whoever changed them last.

## Undo undoes your own work

{{key:Mod+Z}} undoes your own last step and leaves everyone else's alone. When someone changed the same thing since, nothing is undone and the status bar says who.

## Join from a browser

A teammate without Studio can scan the picture under the host's code with a phone, tablet or computer on the same network. ModelCraft opens in the browser and fills with the host's model once the host lets you in. Invite links, for joining from another network, are not open yet.

## When the session ends

Only the host saves the model. Everyone else keeps an unsaved copy: save it if you want to keep it.
