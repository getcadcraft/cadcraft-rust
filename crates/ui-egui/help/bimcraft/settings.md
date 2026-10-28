---
title: BimCraft settings
summary: Choose whether rooms and spaces show, whether edges are drawn and which view a model opens in, and start placing with wall snap on.
section: reference
order: 5
keywords: settings, preferences, options, spaces, rooms, ifcspace, edges, lines, performance, large model, opening view, camera, top view, wall snap, snapping
---

Open {{ui:Settings}} with the gear in the app bar or {{key:Mod+,}}. The first tab, {{ui:BimCraft}}, holds the settings that belong to this app; {{ui:General}} holds the ones every CadCraft app shares (theme, toolbar, panels). BimCraft's settings exist only in the app: you change them here, and this browser remembers them. {{ui:Reset BimCraft settings}} at the bottom of the dialog puts them, and the shared ones, back to the start.

## Menus, keys and mouse

The menu style (BimCraft's own toolbar, a home-design menu bar, a BIM ribbon or a drafting command line), your own keys and menus, and what the wheel and mouse buttons do. See [Menus, keys and mouse](topic:menus-keys-and-mouse).

## When a model opens

- **Show rooms and spaces** (off at the start): rooms and spaces are see-through boxes that can hide the walls behind them, so they start hidden. The {{ui:Spaces}} button above the model still turns them on and off for the open model.
- **Edges**: {{ui:Edges on}} (the start) draws the edge lines that give the drafting look. {{ui:Small models only}} draws them only for models under 300,000 triangles, which keeps big models quicker to turn. {{ui:Edges off}} starts every model without them. The {{ui:Edges}} button still switches them for the open model. Above 1.5 million triangles edges are always off.

## Opening view

The camera a model opens with: {{ui:Fit}} (the start, the whole model from a corner), {{ui:Top}}, {{ui:Front}} or {{ui:Side}}. The buttons above the model change the view any time.

## Placing library items

**Start with wall snap on** (off at the start): when placing or pasting a library item starts, its back snaps to the nearest wall, facing the room. {{key:Tab}} or {{ui:Wall snap}} in the placing bar turns it off for the moment. A storey with no walls starts without it.

## Section box

{{ui:Room round the pick}} is how much space {{ui:Section box round the pick}} leaves on every side of the picked elements, in millimetres: 300 to start, up to 5000. See [Cut an IFC model with a section plane](topic:section-plane).

## Location

{{ui:Latitude and longitude in}} chooses the system {{ui:Measure}} › {{ui:Location}} gives latitudes and longitudes in: WGS 84 (EPSG:4326, the start, as GPS and web maps use), {{ui:The file's own datum}}, or one of ETRS89, NAD83, GDA94, GDA2020, NZGD2000, JGD2011 and CGCS2000. The file's own map system and datum are always shown too. See [Measure distances, face areas and locations in 3D](topic:measure).

## Corner indicator

What sits in the bottom-right corner of the 3D views (an IFC model, and a drawing switched to 3D): {{ui:View cube}} (the start), {{ui:Axis icon}} (a small X, Y and Z) or {{ui:None}}. It changes at once. The 2D drawing view never shows one.

On the view cube, click a face, an edge or a corner to look from that side, drag the cube to orbit, drag the ring to turn round, and click the house for the starting view.

## Your name on issues

Signed in, issues carry your account's name and BimCraft asks for nothing: this part shows {{ui:From your account (<name>)}}. {{ui:Use another name}} opens a box for a different name, which wins until you choose {{ui:Use my account name}}. Signed out, the issue form asks for a name as before. See [Issues and BCF files](topic:issues-bcf).

## Assistants

**Let AI assistants and scripts use BimCraft** (off at the start) lets an assistant on your computer reach the open model through CadCraft Studio: **Read** finds elements and reads their properties, **Change the view** isolates, hides and shows them. **Activity** lists its calls. See [Let an AI assistant work with your model](topic:assistants).

:::note
The other settings apply when a model opens or placing starts. A model already open keeps how it looks until you use the toolbar buttons or open it again.
:::

## Related

- [Copy, paste and duplicate library items](topic:copy-paste)
- [Place equipment and furniture from the library](topic:place-from-library)
- [The BimCraft screen](topic:workspace)
- [Let an AI assistant work with your model](topic:assistants)
