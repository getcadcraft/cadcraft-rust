---
title: Play a construction schedule on the model
summary: Open a task list as a CSV, link each task to elements by a property, and move through the dates to see what is built, in progress and not started yet.
section: tasks
order: 36
keywords: 4d, schedule, construction sequence, timeline, phasing, programme, gantt, tasks, dates, simulation, progress, built, csv
---

## What you'll do

Watch the building go up: each task in your schedule linked to the elements it builds, shown day by day.

## Steps

1. Open an IFC model and open {{ui:4D schedule}} in the left panel.
2. Click {{ui:Download a template}} for a task list with a week per storey, and change it in a spreadsheet. Or write your own.
3. Click {{ui:Open a task list…}} and choose the CSV.
4. Drag the date slider, or click {{ui:Play}}.

## The task list

A CSV with a header row and the columns **Task**, **Start**, **Finish**, **Field** and **Value**. Commas or semicolons both work.

| Task | Start | Finish | Field | Value |
|---|---|---|---|---|
| Level 1 | 2026-11-02 | 2026-11-13 | Storey | Level 1 |
| Roof | 2026-11-16 | 2026-11-20 | Class | IfcRoof |
| Roof | 2026-11-16 | 2026-11-20 | Pset_WallCommon.IsExternal | Yes |

- Write dates as YYYY-MM-DD.
- A row links the task to every element whose **Field** has exactly that **Value**. The field is a property such as Storey, Class, Name, Type, Material, Tag or GlobalId, or any property written as its set and name: `Pset_WallCommon.FireRating`.
- Repeat a task on more rows, with the same dates, to link more elements to it.

## What you should see

- **Built**: elements whose tasks are all finished, in their own colors.
- **In progress**: elements with a task under way that day, in ochre. A task is under way from its start date through its finish date.
- **Not started**: see-through.
- Elements in no task stay as they are. The list shows how many elements each task found. Rows that could not be read, or found nothing, are listed with their line numbers.

## Next

- [Find elements by class, name or property](topic:find)
- [Color elements by a property](topic:color-by-property)
