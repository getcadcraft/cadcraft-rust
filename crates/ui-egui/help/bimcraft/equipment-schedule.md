---
title: Build an equipment schedule and export CSV
summary: Turn the item tags in a foodservice drawing into an equipment schedule, filter it, jump to each item on its sheet, and download it as CSV.
section: tasks
order: 140
keywords: equipment, schedule, foodservice, kitchen, item tags, kcl, attributes, bom, bill of materials, csv, spreadsheet, export, data extraction
---

## What you'll do

Read the equipment list out of a kitchen or foodservice drawing, check it, find each item on the plan, and download it as a spreadsheet file.

This works on drawings whose equipment tags carry their data as block attributes, as kitchen-design libraries such as KCL do: each tag holds the item number (`KCL-ITEM`), quantity, category, manufacturer, model, electrical and plumbing data, and remarks. Other drawings have no {{ui:Equipment}} tab.

## Steps

1. Open the drawing, ideally from its project folder so its linked drawings load too.
2. Click the {{ui:Equipment}} tab above the drawing. Its label shows how many items are on sheets, for example "Equipment (88)".
3. Read the schedule: **Item**, **Qty**, **Equipment**, **Manufacturer**, **Model**, **Electrical**, **Plumbing** and **Where**.
4. Pick what to list in the menu above the table: **Every sheet**, one sheet, **Not on any sheet**, or **Everything tagged in the drawing**. Each choice shows its count.
5. Type in the filter box ("Filter: category, maker, model…") to narrow the list, for example `refrigerator` or `208`.
6. Click an item's sheet name in the **Where** column.
7. Look at the drawing: BimCraft switches to that sheet, zooms in on the item and draws a ring round it. The status line says "Item *n*, *category* is ringed on *sheet*."
8. Click {{ui:Equipment}} to go back to the schedule.
9. Click {{ui:Download CSV}}.

## What you should see

- A file named "*drawing name* equipment.csv" (with the sheet's name in brackets when you listed one sheet) in your downloads folder. It opens in Excel, Numbers, LibreOffice or Google Sheets.
- The status line: "Downloaded *file name* (*n* items)."

The CSV has these columns: Item, Qty, Category, Manufacturer, Model, Electrical, Plumbing, Remarks, Electrical remarks, Plumbing remarks, Sheets, Placed (how many tags). It downloads exactly the rows the table shows, after the menu and the filter.

## How the schedule is built

- It is read from the **tags themselves**, not from the schedule table drawn on the sheet. The note above the table says how many tags were read.
- One row is one item number with one set of data. Its quantity is the **sum** of its tags, the way a printed schedule counts it: shelving tagged "5" in one place and "3" in another is 8, shown as "5 + 3".
- **Where** lists the sheets that show each item, worked out from what each sheet's viewports show, which is what prints. "Model only" means no sheet shows it.
- **Number used twice** marks one item number on two different pieces of equipment. The row has an amber mark.
- Tags no sheet shows are kept apart under **Not on any sheet**. They are often left over from earlier plans or other jobs, and are never mixed into **Every sheet**.
- **Electrical** shows amps, kW, HP, volts, phase, connection and plug as tagged. **Plumbing** shows connection sizes: CW cold water, HW hot water, DD direct drain, ID indirect drain, and gas. Other tag fields are not shown yet.

## Common mistakes

- **No {{ui:Equipment}} tab.** The drawing has no `KCL-ITEM` tags, or its equipment is drawn without data. There is nothing to read.
- **Counts differ from the printed schedule.** A printed schedule may list spare numbers or use shorter names than the tags. Trust the tags for what is placed; check differences with the designer.
- **Opening the CSV shows odd characters.** The file is UTF-8. Open it with your spreadsheet's import command and choose UTF-8 if the program asks.
- **A cell starts with an apostrophe.** Values that begin with `=`, `+`, `-` or `@` get a leading `'` so spreadsheets do not run them as formulas.
- **{{key:F}} and {{key:P}} do nothing.** They work on the drawing, not on the schedule. Click a sheet tab first.
