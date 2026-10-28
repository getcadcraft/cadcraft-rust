---
title: Soften and smooth edges
summary: Hide the edges between faces that meet at a shallow angle, so curved shapes read as one smooth surface, in ModelCraft and in exported files.
section: tasks
order: 52
keywords: soften, smooth, soft edges, smooth shading, hide edges, facets, faceted, curved, normals, harden, angle
---

A curved shape in ModelCraft is made of flat faces. **Soft** edges between those faces are not drawn, and the faces they join are shaded as one smooth surface, so a 24-sided cylinder looks round.

When you pull a circle up with {{ui:Push/pull}}, its sides are softened for you. For other shapes, soften by angle.

## Soften by angle

1. Select what to soften: faces and edges, or a group or component (all of its edges).
2. In {{ui:Entity info}}, type an angle in {{ui:Soften up to}}, such as `20`, and click {{ui:Soften}}.

Every edge in the selection whose two faces turn by that angle or less becomes soft; edges with a sharper turn become hard. A 12-sided prism's sides turn by 30° and its rims by 90°, so `31` softens only the sides.

{{ui:Harden all}} makes every edge in the selection hard again. Each is one undo step.

## Things to know

- Only edges between exactly two faces are softened. Loose edges stay as they are.
- Smooth shading works across soft edges only: a face with no soft edge shades flat.
- Soft edges are kept in the ModelCraft file. **glTF (.glb)** and **OBJ** exports carry the smooth shading (a normal per corner), so other programs show the same surface. Collada and STL export flat faces.
- Soft edges are hidden in every face style, {{ui:X-ray}} and {{ui:Wireframe}} too. To see them, select the shape and click {{ui:Harden all}}; undo brings the softening back.
