---
title: Rebar reference
summary: Bar sizes, hook dimensions, shape codes, cut lengths and the bending schedule columns DetailCraft uses.
section: reference
order: 50
keywords: rebar, bar sizes, hooks, bend diameter, shape codes, cut length, bending schedule, bar list, astm a615, aci 318
---

## Bar sizes

From ASTM A615/A615M Table 1 (inch-pound designations). Millimeters and kg/m are exact conversions of the published inch and lb/ft values. The lists show each size as, for example, "#5 (15.9 mm)".

| Bar | Diameter (mm) | Mass (kg/m) | Area (mm²) | Inside bend diameter | 90° hook leg, out to out (mm) |
|---|---|---|---|---|---|
| #3 | 9.5 | 0.560 | 71 | 6 db | 152 |
| #4 | 12.7 | 0.994 | 129 | 6 db | 203 |
| #5 | 15.9 | 1.552 | 200 | 6 db | 254 |
| #6 | 19.1 | 2.235 | 284 | 6 db | 305 |
| #7 | 22.2 | 3.042 | 387 | 6 db | 356 |
| #8 | 25.4 | 3.973 | 510 | 6 db | 406 |
| #9 | 28.7 | 5.060 | 645 | 8 db | 487 |
| #10 | 32.3 | 6.404 | 819 | 8 db | 548 |
| #11 | 35.8 | 7.907 | 1006 | 8 db | 609 |

In [DetailCraft settings](topic:settings) ({{ui:Rebar}}), **ASTM bar names** can show these bars by their soft-metric A615M names instead: #3 = #10, #4 = #13, #5 = #16, #6 = #19, #7 = #22, #8 = #25, #9 = #29, #10 = #32, #11 = #36 (the same bars; ACI 315-99 Table 1 lists both).

### Metric bars (BS 4449)

| Bar | Diameter (mm) | kg/m | Min. bend radius r (mm) | Hook leg = end projection P (mm) |
|---|---|---|---|---|
| H8 | 8 | 0.395 | 16 | 115 |
| H10 | 10 | 0.617 | 20 | 120 |
| H12 | 12 | 0.888 | 24 | 125 |
| H16 | 16 | 1.578 | 32 | 130 |
| H20 | 20 | 2.466 | 70 | 190 |
| H25 | 25 | 3.853 | 87 | 240 |
| H32 | 32 | 6.313 | 112 | 305 |
| H40 | 40 | 9.865 | 140 | 380 |

The mass is the nominal section times 7850 kg/m³. The radius and end projection are the minimums of BS 8666:2020 (its table of minimum dimensions; the former diameter is 2r). Footings in BS bars get hook legs of P and cut lengths by the BS 8666 formulas below. Choosing European steel in settings starts new footings with H16 mats and H20 starter bars in C30/37 concrete.

## Hooks

ASTM bars: standard 90° hooks from ACI 318-19 Table 25.3.1: inside bend diameter 6 db for #3 to #8 and 8 db for #9 to #11, with a straight extension of 12 db past the bend. The hook leg, measured out to out, is half the bend diameter + db + 12 db.

If a mat bar's hook does not fit in the footing depth inside the cover, DetailCraft shortens the leg to fit and lists a warning.

## Shapes

DetailCraft stores three shapes. The bending schedule prints them as DetailCraft's codes, as ACI 315 types or as BS 8666 shape codes: choose **Bending schedule shape codes** in [DetailCraft settings](topic:settings) ({{ui:Rebar}}).

| Code | Shape | Dimensions |
|---|---|---|
| S | Straight | A = length |
| L | One 90° bend | A = main leg, B = hook leg, both out to out |
| U | Two 90° bends on the same side | A = middle, B and C = the two legs, out to out |

Mat bars with {{ui:End hooks}} on are U; without, S. Starter bars with {{ui:Hook at the bottom}} on are L; without, S.

| DetailCraft | BS 8666:2020 | ACI 315-99 (Fig. 10) |
|---|---|---|
| S | 00, L = A | no type number |
| L | 11, L = A + B − 0.5r − d | no type number: the figure has no bar with one hook |
| U | 21, A and C the legs, B the middle: L = A + B + C − r − 2d | Type 2, hooks A and G, straight B |

With BS 8666 or ACI 315 codes the schedule's A, B and C columns follow that standard's lettering (the middle of a U is B).

## Cut length

For BS 4449 bars the cut length is the BS 8666 formula of the bar's shape code (above). For ASTM bars it is the exact length of the bar's centerline: the straight parts plus the bends as arcs at half the bend diameter plus half the bar diameter. It is not worked out from a table of bend deductions, so it can differ by a few millimeters from a schedule that uses one.

## Layout

- Bottom mat: bars along L form the lower layer, bars along B sit on them. Each set is spread evenly across the footing inside the cover, at the spacing you give or closer, with at least two bars.
- Starter bars: a grid of bars along L and along B at the spacings you give, centered on the footing, standing on the mat and projecting above the top by the projection you give. Each hook points outward.

## Checks

| Check | Rule | Result |
|---|---|---|
| Room for bars | Cover on both sides leaves space for the bars | Refused if not |
| Depth for two layers | Depth minus two covers holds both mat layers | Refused if not |
| Starters fit | The starter grid fits inside the cover | Refused if not |
| Clear spacing | At least the larger of 25.4 mm and db (ACI 318-19 25.2.1; aggregate size not checked) | Warning |
| Hook depth | Hook leg fits in the depth inside the cover | Warning, leg shortened |
| Starter hooks | Hooks stay out of the cover | Warning |

## Bending schedule columns

The {{ui:Bending schedule}} tab and its CSV have these columns: Bar mark, Size, Shape, A (mm), B (mm), C (mm), Count, Cut length (mm), Total length (m), kg/m, Weight each (kg), Total weight (kg), Used for. The last row totals the count, length and weight.
