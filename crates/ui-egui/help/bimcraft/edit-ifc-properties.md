---
title: Change IFC properties and save an edited copy
summary: Change an element's name, tag, description or a property value, undo, and download a new IFC file while the original stays as it was.
section: tasks
order: 40
keywords: edit, change, properties, parameters, tag, name, rename, mark, save as, download, undo, copy, ifc, write
---

## What you'll do

Correct data in an IFC model: rename an element, fix its tag, change a property value. Then save a new IFC file with your changes. The file you opened is never touched.

## What you can change

| Field | Editable |
|---|---|
| Name, Object type, Tag, Description | Yes |
| Single property values (text, numbers, Yes/No) | Yes |
| Type properties (sets marked "(type)") | Yes, and the change applies to every element of that type |
| Type, GlobalId, Line | No |
| Quantities (lengths, areas, volumes, counts) | No |
| Lists, enumerations and other multi-part values | No |
| Geometry, position, adding or deleting elements | No |

## Steps

1. Open an IFC model and click the element you want to change.
2. In the {{ui:Properties}} panel, click the value you want to change. It turns into a text box. An empty value shows as *empty*.
3. Type the new value.
4. Press {{key:Enter}}, or click somewhere else, to keep it. Press {{key:Esc}} to cancel.
5. Look at the value: it is now bold with a blue bar beside it, which marks an edited value.
6. Look at the {{ui:Changes}} section on the left: the change is listed as *element*: *field*, old value to new value.
7. Repeat for other values or other elements.
8. Click {{ui:Save as IFC}} (it shows how many changes are waiting, for example "Save as IFC (3)"), or press {{key:Mod+S}}.
9. Find "*original name* (edited).ifc" in your downloads folder.

## What you should see

The status line says "Saved *name* (edited).ifc with *n* changes (… bytes). Your original file is unchanged." The {{ui:Changes}} list empties, ready for the next round.

## Kinds of values

- **Text**: anything you type is kept as is. Clearing Name, Object type, Tag or Description removes the value.
- **Numbers**: type a plain number, such as `2.4` or `1200`, in the file's own units. No unit symbols. Anything else shows "Use a number."
- **Whole numbers** (counts, integers): "Use a whole number." if you type a decimal.
- **Yes/No**: the value becomes a menu with Yes and No.

Each value keeps its IFC type, so a number stays a number and other programs read it correctly.

## Undo

1. Click {{ui:Undo}} in the {{ui:Changes}} section, or press {{key:Mod+Z}} while you are not typing in a field.
2. Repeat to undo more. Each undo removes the newest change.

You cannot undo past a save: after saving, the list starts again. The saved file keeps everything.

## Why your edit changes only one element

Exporters save space by sharing data. One property set may be shared by twenty doors, and one value ("Fire rating: 60 min") may be listed in ninety sets. If BimCraft simply changed the shared value, every element that shares it would change too.

So BimCraft copies before it writes:

- If the property set you edit is shared with other elements, your element gets its own copy of the set, and only the copy changes.
- If the value is shared with other sets, your set gets its own copy of the value.

The neighbors keep their old data. This is called *copy-on-write*. It adds a few small entries to the file, which every IFC program reads normally.

**Type properties are the exception on purpose.** A set marked "(type)" belongs to the element's type, and in IFC every element of that type reads it from there. Changing it changes the type, so every element of the type shows the new value. The {{ui:Changes}} list marks such edits with "(type)". See [Change a type property for every element of a type](topic:type-vs-instance).

## Warnings before you lose work

- Opening another file while you have unsaved changes asks "You have *n* unsaved changes to *file*. Discard them?" Click Cancel to keep working, then save.
- Closing or reloading the tab with unsaved changes makes the browser ask whether to leave.

## Common mistakes

- **Typing units.** `2400 mm` is refused. Type `2400`.
- **Changing a type property when you meant one element.** Look for the "(type)" note before you type. If you want one element to differ, change an instance property or its Tag or Name instead.
- **Expecting the original file to change.** Saving always downloads a new file. Replace the original yourself if that is what you want, and keep a copy.
- **Pressing {{key:Mod+S}} with no changes.** BimCraft only saves when there is something to save; otherwise the browser's own "Save page" may open. Close it.
- **Saving twice and expecting two small files.** Each save holds every edit since you opened the model.
- **Expecting edits to go back into a project file.** BimCraft writes IFC only. Bring the edited IFC into your authoring program the way you import any IFC.
