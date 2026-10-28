---
title: Paint with colors and materials
summary: Give faces, groups and components a solid color or a material such as brick, siding, shingles or wood, and keep them when you export.
section: tasks
order: 45
keywords: paint, color, colour, material, materials, texture, textures, brick, siding, shingles, roofing, wood, stone, tile, concrete, stucco, grass, gravel, glass, metal, paint bucket, fill, sample, eyedropper, default, glb, obj, mtl, collada
---

Colors help you tell parts of a model apart, and **materials** (brick, siding, roofing, wood, stone, tile, concrete, stucco, grass, glass, metal) make it look like what it is built of. Both paint the same way.

## Pick a color

The {{ui:Colors}} section on the right shows a row of solid colors, {{ui:Default}} (the one crossed by a line: no color) and a {{ui:+}} swatch for any other color. Click one to make it the current color. The Paint tool starts with it.

## Paint

1. Press {{key:B}} ({{ui:Paint}}), or click a color.
2. Click a face to paint it.
3. Click a group or a component to paint that copy: its faces that have no color of their own take this color. Faces you painted inside it keep theirs.

To paint several things at once, select them first and then click a color, or click any of them with {{ui:Paint}}. It is one undo step.

| Hold | And click a face | What gets painted |
|---|---|---|
| — | | That face |
| {{key:Ctrl}} | | Every face connected to it that has the same color |
| {{key:Shift}} | | Every face of that color in the group you are working in |
| {{key:Alt}} | | Nothing: you pick up that color instead |

To take a face back to no color, choose {{ui:Default}} and click it.

## Paint with a material

The {{ui:Materials}} section, under {{ui:Colors}}, lists the material library by kind: brick and block, siding, roofing, wood, stone, tile, concrete, stucco, ground, glass and metal. Hover a swatch for its name. Click one, then paint with it exactly as with a color: {{key:B}}, click faces, groups or components; or select first and click the material.

- In the **Shaded** style the faces show the material's texture. Hidden line, wireframe and x-ray show its color.
- Textures run **level along each face** and **up it**: brick courses stay level on every wall, shingles run down a roof's slope, boards on a floor follow the red axis. They are at their real size (a brick is about 215 mm long), so they look right at any distance.
- In a [rendered picture](topic:render) materials keep their texture, roughness and, for glass and metal, their shine.
- {{key:Alt}}-click with {{ui:Paint}} picks up a face's material too.

The textures are drawn by ModelCraft itself, so they work offline and are free to use for anything (CC0).

## Components and colors

A color painted on a face **inside** a component shows on every copy, because the copies share one definition. A color painted on a **copy** from outside (with the component closed) belongs to that copy only, so two copies of one component can have different colors. {{ui:Entity info}} shows the color of a selected face, group or component.

## Colors in files

- The ModelCraft file keeps every color and every material.
- Other formats get a **material's color** (its look from afar) in place of the texture: glTF, Collada, OBJ, DWG and DXF, and the 3D printing check. In glTF, Collada and OBJ the material keeps its name, such as `Material_brick-red`.
- **glTF (.glb)** and **Collada (.dae)**: each color becomes a material. A component shown in different colors is written once per color.
- **OBJ**: exporting downloads two files, the `.obj` and a `.mtl` materials file with the colors. Keep them in the same folder.
- **DWG** keeps the exact colors; **DXF** uses the nearest of the 255 standard CAD colors.
- **STL** has no colors.

:::tip
To model a screen, paint the frame charcoal and the glass blue after you [offset](topic:draw-faces) the front face.
:::
