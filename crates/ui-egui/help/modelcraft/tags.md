---
title: Tags: show and hide sets of things
summary: Give groups and components a tag, such as Furniture or Walls, and hide or show everything with that tag at once.
section: tasks
order: 48
keywords: tags, tag, layers, layer, show, hide, visibility, organize, furniture, walls, untagged, rename tag, delete tag
---

A tag is a name you give groups and components, such as **Furniture**, **Walls** or **Site**, so that you can hide or show everything with that tag at once. Tags are what CAD programs call layers. For hiding one thing at a time, see [Hide, isolate and unhide](topic:hide-isolate).

## Make a tag

1. In {{ui:Tags}} on the right, click {{ui:Add tag}}.
2. Type its name and press {{key:Enter}}.

To rename a tag, click its name in {{ui:Tags}}, type the new one and press {{key:Enter}}. Two tags cannot share a name.

## Tag groups and components

1. Select one or more groups or components.
2. In {{ui:Entity info}}, choose the tag in {{ui:Tag}}. {{ui:New tag…}} makes a new one and tags the selection with it.

{{ui:Untagged}} takes the tag away. The number next to each tag in {{ui:Tags}} says how many groups and components have it.

Tag groups and components, not loose faces and edges: draw the things you want to tag as groups first (select them, {{key:Shift+G}}).

## Hide and show

Click the eye next to a tag. Everything with that tag goes out of view, wherever it is in the model (also inside other groups), and clicks and snaps go past it. Click the eye again to show it. {{ui:Untagged}} things are always shown.

If you are editing inside a group whose tag you hide, the group closes.

## Delete a tag

Click the bin next to it. What had the tag becomes {{ui:Untagged}}, and shows.

## Things to know

- Tags, their names and which are hidden are saved in the ModelCraft file. Copying a tagged group into another model brings its tag along.
- Hiding and showing a tag, and every other change to tags, is one undo step.
- Exports include everything, hidden tags too, as they do for hidden things.
- Layers in DWG and DXF site plans do not come in as tags yet: the whole drawing comes in as one group.
