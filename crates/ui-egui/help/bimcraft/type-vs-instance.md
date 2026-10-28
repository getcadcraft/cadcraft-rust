---
title: Change a type property for every element of a type
summary: Tell type properties from instance properties, change a value on the type so every element follows, and check that one element's own edit stays its own.
section: tasks
order: 50
keywords: type, instance, type properties, family type, shared, copy on write, all elements, bulk edit, manufacturer, model
---

## What you'll do

Set the manufacturer on a door type so every door of that type shows it, then give one door its own fire rating without touching the others. This shows the difference between type and instance properties in BimCraft.

## Type or instance?

- An **instance** property belongs to one element: its tag, its mark, its room, its own fire rating.
- A **type** property belongs to the type that many elements share: manufacturer, model, width of a door type.

In the {{ui:Properties}} panel, sets that come from the type have " (type)" after their name and the line "Type property: a change here applies to every element of this type." Every other set belongs to the element itself. **Type** in the first block shows the type's name.

## Steps: change the type

1. Open an IFC model and click a door.
2. Note its **Type** in the first block of the {{ui:Properties}} panel, for example "Single-Flush 900 x 2100".
3. Scroll to a set whose name ends in "(type)".
4. Click the value you want to change, for example **Manufacturer**.
5. Type the new value and press {{key:Enter}}.
6. Look at the {{ui:Changes}} list: the entry ends with "(type)".
7. Click another door of the same type.
8. Check the same value: it shows the new value too.

## Steps: change one element only

1. Click one door.
2. Open a set without "(type)" in its name, for example `Pset_DoorCommon`.
3. Click a value, such as **FireRating**, type a new value and press {{key:Enter}}.
4. Click the door next to it, of the same type.
5. Check the same value: it still shows the old value.

Even if the exporter shared that property set between both doors, BimCraft gave the first door its own copy before changing it. See [Why your edit changes only one element](topic:edit-ifc-properties#why-your-edit-changes-only-one-element).

## Save

1. Click {{ui:Save as IFC}} or press {{key:Mod+S}}.
2. Open the saved copy in BimCraft, or in any IFC viewer, to check both changes.

## Common mistakes

- **Not every model has types.** Some exporters write no type objects, or put everything on the instance. Then there are no "(type)" sets, and each element must be changed on its own.
- **The same value appears in a type set and an instance set.** Some exporters write a parameter in both places. Other programs may read either one; change both if you need them to agree.
- **Expecting to pick "all doors of this type" and edit them together.** There is no multi-select in this build. A type property is the way to change many elements at once.
