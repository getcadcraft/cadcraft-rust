---
title: Material and weight report by phase
summary: Cut lengths, counts, weights and paint areas for each phase, with its bolts and rebar, as a table, CSV or PDF.
section: tasks
order: 52
keywords: report, weight, tonnage, kg, paint, paint area, surface area, coating, cut length, material list, phase, bill of materials, csv, pdf
---

In the {{ui:Output}} group, open {{ui:More}} and choose {{ui:Weight report by phase}}, or open the {{ui:By phase}} tab under the view. It shows:

- **Summary by phase**: for each phase, the number of parts, the steel weight, the paint area, the bolts and anchor rods, and the rebar; then the totals.
- **One table per phase**: each part mark in it with its type, profile, material, cut length, count, weight each, weight, and paint area; then its bolts and anchor rods.

{{ui:Download CSV}} saves the same tables in one CSV file (`<name> weight report.csv`). {{ui:Download PDF}} makes a report on the sheet size from [DetailCraft settings](topic:settings#drawings), with the summary first, each phase after, long tables carried over to the next page, and every page numbered.

## How the figures are worked out

- **Weights** are gross, as in the bill of materials: the profile's weight per meter times the cut length. Holes and copes are not taken off.
- **Paint area** is the outside of the section along the cut length: for an I-shape every face of the flanges and web, for a plate both faces and the edges, for a tube the outside only. The ends and the holes are not counted.
- **Rebar** is the weight of the bars in the phase's footings, from the [bending schedule](topic:footings-and-rebar).
- A connection's plates and bolts count in the phase of its main member. See [Phases](topic:phases).

With [marks per phase](topic:phases#marks-per-phase) on, each phase has its own marks (1-b1, 2-b1). With it off, a mark can appear in more than one phase: each phase lists the pieces it has.
