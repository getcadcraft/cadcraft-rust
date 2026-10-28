---
title: Measure distances, angles and mass
summary: Click two things in the view for the minimum distance, angle and ΔX ΔY ΔZ; read an edge's length, a hole's diameter, a face's area or a point's coordinates; get volume, mass, centre of mass and inertia of a part or an assembly.
section: tasks
order: 125
keywords: measure, distance, minimum distance, angle, length, radius, diameter, area, coordinates, delta, inspect, mass properties, mass, weight, center of mass, centre of mass, inertia, moment of inertia, density, material
---

**Measure** works on the model in every workspace (Design, Form, Simulation, Generative, Manufacture) and in an assembly. It is in the **View** group's {{ui:Views}} menu, or press {{key:M}}. A card opens over the top left of the view.

## Measure one thing

Click a vertex, an edge or a face. The card shows:

- **A point** (a vertex, an edge midpoint or a circle centre): its X, Y, Z.
- **An edge**: straight, circle or curve, and its length; for a circle also the radius, the diameter and the centre.
- **A face**: flat, cylinder, cone and so on, and its area; a flat face's normal; a cylinder's radius and diameter (a hole's size). Areas are of the facets the mesh kernel builds.
- **The whole body**: hold {{key:Shift}} and click the part. You get its volume, area, mass and inertia (see below).

The pointer snaps to vertices, edge midpoints and circle centres within 10 pixels; a small blue dot shows the snap before you click. To pick a hole's centre, point at the middle of the hole; to pick its rim (the circle), point at the rim.

## Measure between two things

Click a second item. The card adds:

- **Minimum distance** and the two closest points, drawn in the view with a line between them.
- **ΔX, ΔY, ΔZ** from the first closest point to the second.
- **Angle** between two flat faces (between their normals, 0–180°), between two straight edges or axes (0–90°), or between an edge and a face.
- **Gap between the planes** for two parallel flat faces, **Centre to centre** for two circles or holes, **Axis to axis** for two parallel holes or cylinders.

A third click starts a new pair. {{key:Esc}} clears the picks; {{key:Esc}} again (or {{ui:Close}}) closes Measure. {{ui:Copy}} puts the results on the clipboard as tab-separated text, ready to paste into a spreadsheet.

### How exact it is

PartCraft builds the part with the mesh kernel, so distances are worked out on the facets of the mesh and the card says **approximate: of the facets**: right for flat faces and between vertices, slightly off for curved ones. A finer **Curves** setting brings them closer. Exact measuring on the true surfaces needs an exact kernel: not built yet in this version.

## Mass properties

Choose {{ui:Mass properties}} in the card or in the {{ui:Views}} menu. On a part, pick a **Material** in the card (steel, aluminum, plastics and others with their usual densities). In an assembly each part uses the material set in its properties (select the part, then **Material**), and the result is for all the parts together.

The card shows mass, volume, area, centre of mass, the inertia tensor (Ixx, Iyy, Izz and the products Ixy, Iyz, Ixz, in kg·mm², about the centre of mass along the world axes) and the principal moments. Without a material it shows the same per unit density (mm⁵) and no mass.

The numbers are exact for the tessellated solid the mesh kernel builds (curved faces as facets), so a round part comes out a little low, and the card says so. Material densities are typical handbook values; a real alloy or grade can differ by a few per cent.

## Limits

- Snaps can find a point hidden behind other geometry; check the card says what you meant.
- Measure is closed when you open a sketch or the drawing sheet, or switch workspace.
