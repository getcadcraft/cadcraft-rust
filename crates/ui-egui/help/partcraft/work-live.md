---
title: Model one part together, live
summary: What changes in PartCraft while you are in a CadCraft Studio live session: everyone's features in one tree, each window building the same solid, your own undo, and joining from a browser on the same network.
section: tasks
order: 199
keywords: live, studio live, together, collaborate, session, share live, join, feature tree, history, rebuild, undo, host
---

## What you will learn

What happens to a part while several people model it at the same time. CadCraft Studio starts and joins the session (Share live, Join); this page is about the part itself.

## Everyone models at once

Everyone works on the same feature tree: sketches, extrudes, holes, patterns, fillets, parameters. Your own edits show at once; other people's appear a moment after they make them. Only the tree travels: each window builds the solid itself, and PartCraft's kernel builds the same solid from the same tree in every window.

- **Two people change one feature:** different settings (one the depth, the other the end condition) are both kept; the same setting goes to the later change.
- **A sketch** travels as a whole: when two people edit one sketch at once, the later edit wins. Agree who draws which sketch.
- **A feature built on one someone just deleted** (an extrude of a removed sketch, a hole on a removed face) is taken back, and the status bar says why.
- **Reordering the tree** is checked: if two people drag features at the same moment, the second move is taken back.

## Undo undoes your own work

{{key:Mod+Z}} undoes your own last edit and leaves everyone else's alone. When someone changed the same thing since, nothing is undone and the status bar says who.

## Join from a browser

A teammate without Studio can scan the picture under the host's code with a phone, tablet or computer on the same network. PartCraft opens in the browser, fills with the host's part once the host lets you in, and builds it. Invite links, for joining from another network, are not open yet.

## When the session ends

Only the host saves the part. Everyone else keeps an unsaved copy: save it if you want to keep it.

:::note
Assemblies aren't shared in a session yet: open the part itself to work on it together.
:::
