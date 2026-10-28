---
title: Cut an IFC model with a section plane
summary: Slice the 3D model from the top, front or side, or keep only what is inside a section box or a volume of several boxes with tilted faces, to see inside; move the cut by dragging or typing, flip it, and pick what the cut shows.
section: tasks
order: 25
keywords: section, section plane, section box, grips, drag, margin, selection box, clip box, cut, clipping, slice, plan cut, floor plan, elevation, inside, interior, flip, offset, section volume, stepped section, L-shaped, tilt, sloping cut, oblique plane, extra plane
---

## What you'll do

Cut through an IFC model with one plane to see inside it: from the top for a plan, or from the front or side for a section through the building.

## Steps

1. Open an IFC model. See [Open an IFC model and look around in 3D](topic:open-ifc-model).
2. Click {{ui:Section}} in the toolbar, or press {{key:X}}. The model is cut halfway up, and a bar appears under the toolbar.
3. Choose where to cut from: {{ui:Top}}, {{ui:Front}} or {{ui:Side}}. Front and Side also turn the view to look at the cut.
4. Move the cut: drag the **Offset** slider, or type a distance in metres in the box and press {{key:Enter}}. You can type millimetres too, for example `1200 mm`.
5. Click {{ui:Flip}} to keep the other side of the cut.
6. Click an element on the cut to see its properties.
7. Click {{ui:Off}}, click {{ui:Section}} again, or press {{key:X}} to take the cut away.

## A section box

Click {{ui:Box}} in the same bar to keep only what is inside a box. Each of {{ui:X}}, {{ui:Y}} and {{ui:Z}} has two sliders: where the box starts and where it ends, in metres from the model's edge. The values show beside them.

- {{ui:Box round the pick}} fits the box round the elements you picked and zooms to them. It leaves 300 mm to spare on every side; change that in {{ui:Settings}} › {{ui:BimCraft}} › {{ui:Room round the pick}}.
- With elements picked, {{ui:Show}} › {{ui:Section box round the pick}} or {{key:Shift+X}} does the same in one step, even when no cut is on.
- **Grips** on the box move it with the pointer: drag a square in the middle of a face to move that face, one in the middle of an edge to move the two faces that meet there, or one at a corner to move the three. The sliders follow. In {{ui:Top}} a corner moves the box in plan, in {{ui:Front}} or {{ui:Side}} in elevation.
- {{ui:Whole model}} opens the box to the whole model again.
- {{ui:Top}}, {{ui:Front}} or {{ui:Side}} go back to one plane.

## A volume of several boxes, tilted faces and extra planes

A box is not always the shape you need: a stepped section, an L-shaped wing, or a cut along a wall that is not square to the grid.

1. In {{ui:Box}}, click {{ui:Add a box}}. The cut now keeps what is inside **any** of the boxes, so two boxes make a stepped or L-shaped volume. The list beside it says which box the sliders change ({{ui:Box 1}}, {{ui:Box 2}}…); {{ui:Remove this box}} takes the chosen one away. A volume holds up to 8 boxes.
2. Open {{ui:Tilt and planes}} to shape the chosen box:
   - Choose a {{ui:Face}} and type how far it leans each way, in degrees (up to 80). The face turns about its middle; the other faces stay where the sliders put them. {{ui:Square}} puts it back.
   - {{ui:Add a plane}} cuts the box with one more plane. {{ui:Faces}} is the way the cut-away side faces, in degrees from X towards Y; {{ui:Slope}} is how far that turns upward (0 for an upright plane, 90 for a level one); {{ui:Offset}} is how far the plane is from the middle of the box, in metres. × removes it. A box takes up to 6 extra planes.
3. Where the volume cuts through solids, the cut faces are filled as before, also where two boxes meet in a step.

:::tip
If the outline disappears and nothing is left, the tilts or planes leave nothing inside that box; the bar says so. Tilt a face back or change a plane's offset.
:::

## A building turned in its site

Many models put the building at an angle to the file's axes (true north, shared coordinates), so {{ui:Front}}, {{ui:Side}} and the box cut it at a slant. When the walls run at an angle, the section bar shows {{ui:Square to the building}}: click it and the planes and the box run along the building's own walls (the status line says by how much it is turned). Click it again for the model's axes. Turning the section off starts the next one on the model's axes.

## What you should see

- **Top** keeps what is below the cut, so you look down into the rooms. The offset is measured up from the bottom of the model.
- **Front** keeps what is behind the cut, seen from the front; **Side** keeps what is beyond it, seen from the side. The offset is measured from the front or the side of the model.
- A thin colored outline shows where the plane is.
- Where the plane cuts through a wall, slab or other solid, the cut face is filled in a dark color, like a drawn section. {{ui:Fill}} in the bar turns this off and on.
- The view cube in the bottom-right corner turns the model to look at the cut from any side.
- Clicking picks what you can see: anything cut away cannot be picked.

## Common mistakes

- **Nothing changed.** The cut may be above the roof or outside the model. Drag the slider, or type a smaller offset.
- **The offset jumped back.** A distance beyond the model is moved to its end, and the status line says so.
- **An issue shows only part of my volume.** An issue (BCF) keeps one box with its tilted faces and planes: the first box of a volume. The status line says so when you make the issue.
- **The cut is hollow.** {{ui:Fill}} in the bar is off: click it to fill the cut faces of walls, slabs and other solids.
- **A filled cut looks speckled or misses a piece.** Filling needs closed solids; an element whose shape in the file is not closed cannot be filled cleanly.

## Next

- [Show and hide storeys, spaces and edges](topic:storeys-and-spaces)
- [See a DWG or DXF drawing in 3D](topic:dwg-in-3d) — drawings in 3D have the same section plane
