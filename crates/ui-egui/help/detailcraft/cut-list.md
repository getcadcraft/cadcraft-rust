---
title: Cut list
summary: Nest the beams, columns, braces and stringers into stock lengths with saw kerf, and see how many bars to buy and the waste.
section: tasks
order: 46
keywords: cut list, nesting, stock length, bar, kerf, saw, drop, offcut, waste, combining, purchase, multiple
---

The {{ui:Cut list}} tab, under the view, works out how to cut the model's linear parts from stock bars: how many bars of each length to buy for each profile and material, what goes on each bar, and the waste.

## Set the stock

At the top of the tab:

- **Stock lengths (mm):** the lengths you can buy, separated by spaces, for example `6000 12000`, or 20 ft and 40 ft bars as `6096 12192`. Up to eight.
- **Saw kerf (mm):** what each cut takes (3 to start).
- **End trim (mm):** cut off the end of every bar before the first piece (0 to start).
- **Keep drops from (mm):** a drop at least this long goes back to stock; shorter is scrap (500 to start).

These are kept in this browser, like the other [settings](topic:settings).

## How it nests

- Every beam, column, brace and stair stringer, by part mark and count; plates are cut from sheet and are not nested here.
- Each profile and material on its own.
- Longest pieces first, each into the first bar it fits; a new bar starts as the longest stock length. Then every bar is bought as the shortest stock length that still takes its cuts.
- Each cut takes the kerf (none after a cut that ends exactly at the bar's end); each bar loses the end trim.
- A piece longer than every stock length is listed under **Longer than any stock**, not nested: splice it (see [Splices](topic:splices)) or add a longer stock length.

## Read it

- **Summary:** per profile and material, the pieces, the bars to buy (for example "2 × 12000, 1 × 6000"), the metres bought and used, and the waste.
- **Bars and their cuts:** every bar, its stock length, its cuts as part mark and length, and the drop, kept or scrap.

{{ui:Download CSV}} saves both tables as `<name> cut list.csv`.

DetailCraft does not nest plates into sheets, and does not mix profiles or materials on a bar.
