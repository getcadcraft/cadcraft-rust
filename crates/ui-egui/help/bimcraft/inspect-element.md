---
title: Pick an element and read its properties
summary: Click any element in the 3D model to highlight it and read its name, type, tag, GlobalId, property sets and quantities.
section: tasks
order: 30
keywords: properties, pick, select, click, element, parameters, property sets, pset, quantities, globalid, guid, type, inspect
---

## What you'll do

Pick elements in an IFC model and read everything the file says about them: attributes, property sets, type properties and quantities.

## Steps

1. Open an IFC model.
2. Click an element, for example a door. Click without dragging: a drag of more than a few pixels orbits instead of picking.
3. Look at the element: it is highlighted in blue.
4. Look at the {{ui:Properties}} panel on the right. At the top are the element's kind (such as "Door" or "WallStandardCase") and its name.
5. Read the first block: **Name**, **Type**, **Object type**, **Tag**, **Description**, **GlobalId** and **Line**.
6. Scroll down to the property sets. The first three are open; click the name of any other set to open it.
7. Click another element to switch to it.
8. Press {{key:Esc}}, or click empty background, to clear the selection.

## What you should see

- The highlighted element and its data on the right.
- Property sets in alphabetical order. Sets that come from the element's **type** have " (type)" after their name and the note "Type property: a change here applies to every element of this type."
- Quantity sets (such as lengths, areas and volumes) listed with the other sets. Quantities are read-only.
- Values you can change look like buttons. [Change IFC properties and save an edited copy](topic:edit-ifc-properties) shows how.

[Properties panel fields](topic:properties-panel) explains each field.

## Getting at hidden elements

1. To reach something inside the building, untick the storeys above it in the {{ui:Storeys}} list.
2. To reach a wall behind a room volume, turn {{ui:Spaces}} off.
3. Click the element.

## Common mistakes

- **Dragging a little while clicking.** BimCraft treats that as an orbit and does not pick. Hold the mouse still.
- **Expecting units next to numbers.** Values are shown as stored in the file, rounded to four decimals, without a unit symbol. Lengths in a metric export are usually millimeters or meters; check the exporter's settings.
- **Looking for a parameter by its familiar name.** Exporters rename and regroup parameters into IFC property sets such as `Pset_WallCommon`. Look through each set; custom parameters often sit in a set named after the exporting program or your company.
- **Expecting a search box.** Click to pick an element; {{key:Mod}}+click or {{key:Shift}}+click picks more, and {{key:Mod+A}} picks every element shown. The {{ui:Elements}} list on the left only counts elements by kind; it is not clickable.
- **Picking on a project file card.** A file card has no elements to pick: "Element properties need the 3D model. Open an IFC export of this file to see them."
