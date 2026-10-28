---
title: Parts list, exploded views and moving parts
summary: Give assembly parts a number, description and material, read and download the bill of materials, make exploded views with steps or automatically, move parts that are not joined by hand, set joint limits, and export the whole assembly as STL, 3MF or a bill of materials.
section: tasks
order: 115
keywords: bom, bill of materials, parts list, part number, material, density, csv, exploded view, explode, collapse, trail lines, move part, drag, gizmo, translate, rotate, joint limits, assembly export, assembly step, assembly drawing, balloons
---

These tools are in the right panel of an assembly (see [Build an assembly](topic:build-assembly)).

## Part properties

Select a part in the list on the left. Under **Part properties** set:

- **Part number** (up to 40 characters) and **Description** (up to 200).
- **Material**: a common material with its density, or **Custom density…** to type one in g/cm³. The panel then shows the part's mass.

Properties belong to the part, so every copy of it shares them. They are saved in the assembly file.

## Bill of materials

With nothing selected ({{key:Esc}}), the panel shows the **Bill of materials**: one row per part with its item number, quantity (how many copies), part number, name, material and mass each. Item numbers follow the order the parts first appear in the list. Masses use each part's volume (of the faceted solid the mesh kernel builds, a little low on round parts) and its material; a part with no material has no mass, and then there is no total.

{{ui:Download CSV}} saves the list as a CSV file (also **Export › Bill of materials (CSV)**). It opens in any spreadsheet; text that starts with = or + gets a leading apostrophe so a spreadsheet does not run it as a formula.

## Exploded views

An exploded view pulls the parts apart for pictures. It never changes the joints or where the parts really are: saving, clash checks and exports use the real positions.

1. With nothing selected, choose {{ui:New view}} under **Exploded views**.
2. Under **Add a step**, tick the parts the step moves, choose a **Direction** (+X … −Z, the normal of a face you click, or **Drag an arrow in the view**) and a **Distance**, then {{ui:Add step}}. With **Drag**, drag the arrow and let go: the step takes the direction and distance you dragged.
3. Add more steps. They play one after another.
4. Drag the slider between **Assembled** and **Exploded** to animate the view. Dashed trail lines run from each part's rest position to where it is now.

{{ui:Auto (radial)}} moves every part except the grounded one away from the assembly's middle. {{ui:Auto (joints)}} moves each joined part (with everything joined below it) out along its joint's face normal. You can keep several named views; pick one in **View**, or **Assembled** to put the parts back. Views are saved in the assembly file.

## Move a part by hand

Only a part with no joint to the grounded part moves by hand (the grounded part is fixed; joints place the rest). Select it and, under **Move by hand**:

- Type **ΔX, ΔY, ΔZ** and choose {{ui:Move}}, or choose an axis and an **Angle** and {{ui:Turn}} (about the part's middle).
- {{ui:Drag to move}} shows arrows on the part: drag an arrow to move along that axis, or a square to move in a plane. {{ui:Drag to turn}} shows rings to turn it. Each drag is one undo step. {{key:Esc}} hides the arrows.

## Joint limits

Select a joint that moves and, under **Limits**, tick **Hold … between limits** for any of its values, then set its **Minimum** and **Maximum**. Each value is held inside its own limits, its slider covers just that range, and **Check the motion for clashes** sweeps only between them. A cylindrical joint can limit its angle, its offset or both. Changing the joint's type drops limits on values the new type does not have.

## Export the whole assembly

In an assembly the **Export** menu writes the whole assembly with every part where it is placed:

- **STEP AP242 / AP214** and **Drawing (PDF)** are not built yet in this version: they need an exact kernel, so they are grayed. Export each part's faceted STEP from the part instead.
- **STL**: one mesh of all the parts. **3MF**: each part once, placed once per copy.
- **Bill of materials (CSV)**.

## Limits

- The assembly drawing and the assembly STEP are not built yet in this version (they need an exact kernel). Exploded views are for pictures on screen.
