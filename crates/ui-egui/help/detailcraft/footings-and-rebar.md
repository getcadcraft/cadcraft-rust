---
title: Place footings with rebar
summary: Place rectangular pad footings with a bottom mat and starter bars, check them, and download the bending schedule.
section: tasks
order: 90
keywords: footing, pad footing, foundation, rebar, reinforcement, bars, mat, starters, dowels, bending schedule, bar list, concrete, hooks, cover
---

DetailCraft reinforces one kind of concrete element: the rectangular pad footing. Each footing gets a bottom mat both ways and, if you want them, starter bars standing up out of it. Bars are US sizes #3 to #11.

## Place a footing

1. Choose {{ui:Plan}} and pick the level of the footing top.
2. Choose {{ui:Footing}} in the {{ui:Place}} group, or press {{key:F}}.
3. Set the footing in the {{ui:Properties}} panel (see below), or keep the defaults.
4. Click a grid intersection. The footing is centered on it with its top at the level. The status bar says, for example, "Footing 2000 x 2000 x 600 under A/1."

You can also use the {{ui:Add by grid}} form at the bottom of the panel.

The footing is drawn see-through so you can see its bars, which are drawn as solid tubes.

## Footing settings

| Field | Default | Notes |
|---|---|---|
| Length L (mm) | 2000 | Along x; 300 to 50000 |
| Width B (mm) | 2000 | Along y; 300 to 50000 |
| Depth H (mm) | 600 | 150 to 10000 |
| Concrete | 4000 psi | Text, printed in the IFC export |

**Bottom mat**

| Field | Default | Notes |
|---|---|---|
| Long bars | #5 | Bars running along L, the lower layer |
| Spacing (mm) | 250 | The largest spacing allowed; bars are spread evenly at this or closer |
| Short bars | #5 | Bars running along B, laid on the long bars |
| Spacing (mm) | 250 | As above |
| Cover (mm) | 75 | Clear cover on every face |
| End hooks | On | Turns both ends of each mat bar up in a standard 90° hook (shape U); off gives straight bars (shape S) |

**Starter bars**

| Field | Default | Notes |
|---|---|---|
| Starter bars | On | Turn them off for a footing without starters |
| Bar | #6 | |
| Bars along L / Bars along B | 2 / 2 | A grid of bars, centered on the footing |
| Spacing along L / Spacing along B | 250 / 250 | Between bar centers |
| Projection above top | 600 | How far the bars stand above the footing top |
| Hook at the bottom | On | A 90° hook pointing outward, sitting on the mat (shape L) |

Under the fields, DetailCraft shows the hook leg for the long bars, for example "Hook leg for #5: 254 mm out to out."

## Change a placed footing

Select the footing (click its concrete away from any column, or one of its bars). You get the same fields plus {{ui:Center x}}, {{ui:Center y}}, {{ui:Top z}} and {{ui:Rotation (°)}}. Below them a table lists the bars of this footing with their marks, counts and lengths, and the concrete volume in cubic meters.

Each change is checked before it is kept. A change that cannot be built is refused and the status bar says why, for example "The cover leaves no room for bars in this footing.", "The footing is too shallow for two layers of bars inside the cover." or "The starter bars do not fit inside the footing with 75 mm cover."

Changes that can be built but need a look are listed under the footing and in the {{ui:Problems}} tab:

- "#11 bars at 59.6 mm leave less than 35.8 mm clear (ACI 318-19 25.2.1)." The clear gap between bars must be at least the bar diameter and at least 25.4 mm.
- "#5 hooks need 254 mm of depth; only 150 mm fits inside the cover." The hook legs are then cut short to fit, so they are no longer standard hooks.
- "The starter hooks reach into the cover."
- "Starter bars are closer than the minimum clear spacing."

## Bar marks and the bending schedule

Identical bars (same size, shape and dimensions to the millimeter) share a bar mark across all footings. Marks are written `<bar number>F<n>`: 5F1, 5F2 for #5 bars, 6F1 for the first #6 bar, and so on. Footings with the same size, concrete, mat and starters share a footing mark: F1, F2 …

1. Open the {{ui:Bending schedule}} tab at the bottom. Each row is one bar mark: size, shape, dimensions A, B and C, count, cut length, total length, mass per meter, weight each, total weight, and what the bar is used for.
2. Choose {{ui:Download CSV}} in the tab, or {{ui:Bending}} in the {{ui:Output}} group's {{ui:More}} menu, for `<name> bending schedule.csv`.

Shapes are DetailCraft's own codes, not a standard's: S straight, L one bend, U two bends. Dimensions are out to out; the cut length is the exact centerline length through the bends. See [Rebar reference](topic:rebar-reference).

## What DetailCraft does not check

- Development and lap lengths, and whether the starter bars are long enough for the column above.
- The aggregate-size part of the clear spacing rule (it does not know the mix).
- Anything about the column or base plate above: footings and steel are separate.

See also [What is not built yet](topic:not-built-yet).
