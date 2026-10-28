---
title: Place plates and add custom profiles
summary: Place loose plates by thickness and width, and define your own I, channel, angle, tube or flat bar sections.
section: tasks
order: 50
keywords: plate, flat, custom profile, user profile, parametric, section, shape, cross section, pl
---

## Place a plate

A plate is placed like a beam: it runs between two intersections, with its width across and its thickness up.

1. Choose {{ui:Plate}} in the {{ui:Place}} group, or press {{key:P}}.
2. In the {{ui:Properties}} panel, set {{ui:Thickness (mm)}} and {{ui:Width (mm)}}. The default is 20 x 200 in A36.
3. Set {{ui:Material}} and {{ui:Rotation (°)}} if needed. A rotation of 90 stands the plate on edge.
4. Click two intersections, or use the {{ui:Add by grid}} form.

The plate's profile name is written `PL<thickness>x<width>`, for example PL20x200. Its length is the distance between the two points.

To resize a placed plate, select it and change {{ui:Thickness (mm)}} or {{ui:Width (mm)}}.

Plates you place are parts of their own (mark p, assembly P). End plates and base plates made by connections are plates too, but you change them through their connection; see [End plates](topic:end-plates).

## Add a custom profile

When the section you need is not in the catalog, make it from its dimensions. A custom profile belongs to the model and is saved with it.

1. In the {{ui:Model}} panel on the left, open {{ui:Custom profile}}.
2. Type a {{ui:Name}}, for example `W8x18-custom` or `RHS 120x80x5`. It cannot be a catalog name or look like a plate name.
3. Pick a {{ui:Shape}}: {{ui:I-shape}}, {{ui:Channel}}, {{ui:Equal angle}}, {{ui:Rectangular tube}}, {{ui:Round tube}} or {{ui:Flat bar}}.
4. Fill in the dimensions that appear, all in millimeters:

| Shape | Dimensions |
|---|---|
| I-shape | Depth d, Flange width bf, Web tw, Flange tf, Fillet radius r (may be 0) |
| Channel | Depth d, Flange width bf, Web tw, Flange tf (square corners) |
| Equal angle | Width b (each leg), Thickness t |
| Rectangular tube | Height h, Width b, Thickness t (outer corner radius 2t) |
| Round tube | Diameter D, Thickness t |
| Flat bar | Width b, Thickness t |

5. Choose {{ui:Add profile}}.

The status bar says "Profile "…" added; it is in every profile list under Custom." It appears in the list under the form and at the end of every {{ui:Profile}} list.

DetailCraft checks that the shape can exist. You will see messages such as "The web and flanges do not fit inside the depth and width.", "The fillet radius is too big for the flange." or "The wall is too thick." Fix the numbers and choose {{ui:Add profile}} again.

### What custom profiles get

- Mass per meter from the area × 7850 kg/m³ (catalog sizes use the published weight instead).
- Custom I-shapes take holes, copes, end plates, NC files and shop drawings like catalog I-shapes.
- Custom flat bars behave as plates.
- Custom channels and tubes take holes; angles and round tubes do not yet.

You cannot edit or remove a custom profile once it is added (use {{key:Mod+Z}} straight away if you made a mistake). Give a corrected one a new name.
