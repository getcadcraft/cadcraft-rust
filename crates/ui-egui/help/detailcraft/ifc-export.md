---
title: Export IFC for coordination
summary: Write an IFC4 file of the whole model, steel, bolts, footings and rebar, to share with architects, engineers and other software.
section: tasks
order: 130
keywords: ifc, ifc4, ifc2x3, bim, export, import, import ifc, holes, bolts, loose bolts, openings, fasteners, coordination, model exchange, step, openbim, clash, viewer
---

The IFC export writes the whole model as one IFC4 file that BIM software and IFC viewers can open.

## Export

1. Choose {{ui:IFC}} in the {{ui:Output}} group.
2. The browser downloads `<name>.ifc`. The status bar says what went in, for example "IFC4 exported: 25 parts, 88 fasteners, 7 footings, 132 bars."

If the model has no parts and no footings, the status bar says "Nothing to export yet."

## What is in the file

| In DetailCraft | In the IFC file |
|---|---|
| Each level | A building storey (IfcBuildingStorey), under one project, site and building |
| Grids | An IfcGrid with its axes and labels |
| Columns, beams, braces | IfcColumn, IfcBeam, IfcMember (brace), each an extrusion of a standard profile shape with its real dimensions |
| Plates, end plates, base plates | IfcPlate |
| Holes and copes | Openings (IfcOpeningElement) cut from the part |
| A member and its welded plates | An assembly (IfcElementAssembly) tagged with the assembly mark |
| Bolts and anchor rods | IfcMechanicalFastener with diameter and length |
| Footings | IfcFooting (pad footing), with the concrete text as its material |
| Each bar | IfcReinforcingBar with its bar mark, diameter, area and length, swept along its true centerline with real arcs at the bends |

Each part's tag is its part mark. Each element also carries a "DetailCraft" property set with its marks, profile and weight. Each element sits in the highest storey at or below its lowest point. Materials are grouped by grade.

## Stable ids for re-exports

Every element's IFC id (GlobalId) is worked out from the model name and the element's own id in the model. Export the same model twice and the ids match, so software that compares versions can tell what changed.

:::warning
Renaming the model changes every id. Keep the model name the same between issues of a project.
:::

## Check it

DetailCraft's IFC files are read back by an independent IFC reader in its test suite. Programs differ in how they read IFC, so open the file in the software your team uses before you rely on it. CadCraft's BimCraft app opens IFC files too.

## Not yet

- Bringing connections, footings and rebar in from an IFC file. {{ui:Import IFC}} brings in beams, columns, members, plates, grids, levels, holes, copes and bolts, and lists what it left out.
- Analytical or structural-analysis data.
- Anything other than the IFC4 reference view.

## Import

{{ui:Import IFC}} in the {{ui:File}} group (or dropping an `.ifc` file on the view) makes a new model from an IFC2X3 or IFC4 file:

- **Beams, columns, members (braces) and plates** whose body is one extruded profile DetailCraft knows, matched to the catalog or kept as a custom profile; **grids** and **levels** from the file's grid and storeys.
- **Holes:** every round opening (IfcOpeningElement voiding a part that came in) square to one of the part's faces: an I-shape's or channel's web or flanges, a tube's walls, a plate, an angle's legs. A hole through both walls of a tube comes in once.
- **Copes:** a rectangular opening that takes the full width of an I-shape or channel from one end, down from the top or up from the bottom.
- **Bolts:** every IfcMechanicalFastener or IfcFastener, with its diameter, length, grade and kind (bolt or anchor rod), as a **loose bolt**. Loose bolts show in the view, the bill of materials and the IFC export, and stay where they are when you move members. The {{ui:Properties}} panel, with nothing selected, counts them and has {{ui:Remove loose bolts}} for when you add connections instead.

The report lists everything else by class with the reason it was left out: walls and slabs, footings and bars, slotted holes, holes in round tubes, connections (an IFC file carries their plates and bolts, not the rule that made them).
