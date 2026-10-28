---
title: Save views and walkthroughs
summary: Keep camera views of the model by name, go back to them, play them one after another, and record a walk through the building to play back.
section: tasks
order: 34
keywords: views, saved views, camera, cameras, viewpoint, scene, scenes, tour, presentation, walkthrough, walk through, fly through, record, play, path, animation
---

{{ui:Views}} on the toolbar keeps cameras of the 3D view in the model's file, so you and anyone you send it to can go straight back to them.

## Save a view

1. Set up the 3D view: orbit, zoom, hide what you don't want.
2. Click {{ui:Views}}, type a name (or keep the one offered) and click {{ui:Save view}} or press {{key:Enter}}.

Saving with a name that is already in the list updates that view. Click a view's name to go back to it; the camera moves there smoothly.

The arrows change the order, the pencil renames, the bin deletes. Each change is one step, so {{key:Ctrl+Z}} undoes it.

## Play the views

{{ui:Play the views}} moves from one view to the next in the list's order, stopping a moment at each. {{key:Esc}} or {{ui:Stop}} ends it.

## Record a walkthrough

1. Turn on [Walk](topic:walk) and start walking inside the building.
2. In {{ui:Views}}, click {{ui:Record a walk}}.
3. Walk with {{key:W}} {{key:A}} {{key:S}} {{key:D}} and look round with the mouse.
4. Click {{ui:Stop recording}}. The walk is saved as "Walk 1", with its length.

Click a walk to play it back as you walked it. {{key:Esc}} stops it.

## In the file

Views and walks are kept in the IFC as the property set `CadCraft_Views` on the project: plain IFC that other programs ignore. They are in the first model's coordinates, so they still fit after the model is reloaded or another model is added.

:::tip
To render a picture from a saved view, go to the view, then click [Render](topic:render).
:::
