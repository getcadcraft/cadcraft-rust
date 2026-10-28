---
title: Get started with ModelCraft
summary: What ModelCraft is for, how to open it, and your first box from a rectangle to a saved model.
section: start
order: 10
keywords: start, first model, begin, tutorial, introduction, box, massing, 3d, push pull, quick start
---

ModelCraft is a 3D modeler for early design: massing studies, room layouts, quick building forms and the shadows they cast. You draw flat shapes, pull them up into solids, and keep working on them by drawing on their faces. It runs in your web browser, and your models stay on your computer.

If you have used SketchUp, most of what you know carries over: the same tool letters, push/pull, a value box for exact sizes, groups and components. [Coming from other modelers](topic:coming-from) lists what is different.

## Open ModelCraft

1. Open ModelCraft in a current version of Chrome, Edge, Firefox or Safari. See [Browsers and computers](common:browsers).
2. Wait for the 3D view to appear: a light (or dark) area in the middle with a card titled "Start modeling".

The screen has a toolbar along the top, the {{ui:Outliner}} and {{ui:Components}} on the left, the 3D view in the middle, {{ui:Entity info}}, {{ui:Style}} and {{ui:Scenes}} on the right, and a status bar along the bottom with the value box and the units. [The ModelCraft screen](topic:workspace) explains each part.

:::note
ModelCraft needs WebGL, the 3D drawing feature of your browser. If the middle of the screen stays empty and the tools do nothing, see [Troubleshooting](topic:troubleshooting#the-3d-view-is-blank).
:::

## Check the units first

Look at the bottom right corner. The units picker shows what lengths are measured in.

- A new model starts in **feet and inches** when your browser is set to the United States (also Liberia and Myanmar), and in **millimeters** everywhere else.
- To change it, choose another unit in the picker: {{ui:ft and in}}, {{ui:in}}, {{ui:m}}, {{ui:cm}} or {{ui:mm}}. The model itself does not change size; only how lengths are shown and how bare numbers are read.

A number you type without a unit uses the model's unit. In a feet-and-inches model a bare number is **inches**, so `120` means 10 feet.

## Draw your first box

1. Press {{key:R}} for the {{ui:Rectangle}} tool, or click {{ui:Rectangle}} in the {{ui:Draw}} group of the toolbar.
2. Click once on the ground near the middle of the view. That is the first corner.
3. Move the pointer away. A rectangle follows it, and the value box at the bottom (labeled {{ui:Dimensions}}) shows its size.
4. Type the size and press {{key:Enter}}: for example `4m,3m`, or `16',12'` in feet and inches. You don't need to click in the value box first; typing a number sends it there.

   You get a flat rectangle on the ground. It is a **face**: a surface with edges around it.

5. Press {{key:P}} for {{ui:Push/pull}}.
6. Click the rectangle, move the pointer up, and click again. Or click the rectangle and type a height, such as `3m` or `10'`, then press {{key:Enter}}.

   The rectangle becomes a box with a top, a bottom and four sides.

7. Press {{key:Space}} to go back to {{ui:Select}}, then click the top of the box. {{ui:Entity info}} on the right shows its area.

:::tip
You can also drag instead of clicking twice: press on the first corner, drag, and release on the second. Both ways work for every drawing tool.
:::

## Look around

- **Orbit:** drag with the middle mouse button (the wheel), or press {{key:O}} and drag with the left button.
- **Pan:** drag with the right button, or with the middle button while you hold {{key:Shift}}.
- **Zoom:** turn the wheel. It zooms toward whatever is under the pointer.
- **See everything:** press {{key:Shift+Z}}, or click {{ui:Extents}}.

On a trackpad or a touch screen the gestures are different. See [Orbit, pan and zoom](topic:orbit-pan-zoom).

## Undo

Press {{key:Mod+Z}} to undo and {{key:Mod+Y}} to redo, or use {{ui:Undo}} and {{ui:Redo}} in the toolbar. ModelCraft remembers the last 200 steps while the model is open. The history is not saved in the file.

## Save your model

1. Click {{ui:Save}} in the {{ui:File}} group, or press {{key:Mod+S}}.
2. The browser downloads a file named after your model, such as `Untitled.modelcraft`. The status bar says where it went: "Saved Untitled.modelcraft to your downloads."

**Saving is a download.** Each save makes a new copy in your downloads folder; it does not overwrite the file you opened. To keep a tidy project folder, move or rename the file there yourself. [Open, save, import and export](topic:files) explains why and what else you can do.

:::warning
ModelCraft does not save on its own. If you close the tab with unsaved changes, the browser asks first ("Leave site?"). Choose to stay, then save.
:::

## Get help

Press {{key:F1}} or click {{ui:Help}} in the app bar to open this help beside the model. It works with no connection. Press {{key:F1}} again, or {{key:Esc}} inside the help, to close it. See [Using this help](common:help).

## Where to go next

- [Draw faces with lines, rectangles and circles](topic:draw-faces)
- [Push and pull faces into solids](topic:push-pull)
- [Type exact sizes in the value box](topic:value-box)
- [Every key and mouse action](topic:keyboard)
