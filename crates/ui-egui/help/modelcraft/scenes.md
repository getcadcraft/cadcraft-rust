---
title: Save views as scenes
summary: Save a camera and style as a scene, update and rename it, and step or play through scenes to present a design.
section: tasks
order: 120
keywords: scene, view, saved view, camera, presentation, animation, play, slideshow, page up, page down, tabs, walkthrough
---

A **scene** remembers a camera position, a style and what is hidden: where you are looking from, perspective or parallel, the face style, edges, the sun settings, hidden groups, tags turned off and the section cut. Scenes are saved in the `.modelcraft` file.

## Add a scene

1. Set up the view: orbit, zoom, pick a style, set the sun.
2. In the {{ui:Scenes}} panel on the right, click {{ui:Add scene}}.

   The scene appears in the list, named "Scene 1", "Scene 2" and so on, and as a tab across the top of the 3D view. The status bar says "Scene 1 saved: this camera, style and what is hidden."

## Go to a scene

- Click its tab over the 3D view, or its name in the {{ui:Scenes}} list.
- Or press {{key:PageDown}} for the next scene and {{key:PageUp}} for the previous one. {{ui:Next}} and {{ui:Previous}} under the list do the same.

The camera glides to the scene in a little over half a second. If your computer is set to reduce motion, it jumps there instead.

## Change, rename or delete a scene

Each scene in the list has three small buttons after its name:

- **Update** (the circular arrows): saves the current camera, style and what is hidden into that scene. "Scene 2 updated."
- **Rename** (the pencil): opens a "Rename scene" dialog. Type the new name and click {{ui:Save}}.
- **Delete** (the trash can): removes the scene. "Scene 2 deleted. Ctrl+Z brings it back."

Adding, updating, renaming and deleting scenes are steps in undo, like changes to the model.

## Present

1. Add two or more scenes.
2. Click {{ui:Play}} under the list. ModelCraft moves to the next scene every few seconds and goes back to the first after the last.
3. Click {{ui:Stop}} (the same button) or press {{key:Esc}} to stop.

For a sun study, add one scene per hour with only the {{ui:Time}} changed, then play them. See [Styles and sun shadows](topic:styles-shadows#a-sun-study).

To present without the bars, press {{key:Ctrl+Shift+F}} twice: full screen, then full screen with the bars hidden. The scene tabs stay over the view.

## What a scene shows

A scene remembers what was hidden when you saved it: groups and components you [hid](topic:hide-isolate), [tags](topic:tags) you turned off, and which [section plane](topic:sections) cuts. Going to the scene hides and shows them again. So one scene can show the whole house, the next the house with the roof's tag off, the next a section through it.

Hidden loose faces and edges are not part of scenes. Going to a scene does not make an undo step. Scenes saved by an earlier build of ModelCraft leave what is hidden as it is; click Update to save it into them.

## Export an animation

{{ui:Export animation}}, under the list, plays the scenes from the first to the last, moving the camera smoothly between them, and saves the film as a **WebM** video (two seconds' move and a short pause per scene). It records the view as you see it, at its size on screen, in [presentation view](topic:styles-shadows#presentation-view) (no axes, guides or selection). Press {{key:Esc}} or click the button again to stop without saving. WebM plays in browsers and VLC, and video editors convert it.

## Things to know

- Changing the style or moving the camera after going to a scene does not change the scene until you click Update.
- Scenes are not exported to Collada, OBJ, glTF, STL or IFC.
