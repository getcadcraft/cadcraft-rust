---
title: Profile catalog
summary: Every built-in steel size, how profile names are written, where the dimensions come from, and how plates and custom sections are named.
section: reference
order: 20
keywords: profiles, sections, catalog, sizes, w shapes, wide flange, s shapes, channel, angle, hss, tube, pipe, ipe, hea, heb, upn, plate, aisc, en 10365, naming
---

DetailCraft has 1,259 built-in sizes, each copied from a published table and checked by the app's tests: the area worked out from the dimensions must match the published area, and the published weight must match that area times the density of steel. Plates and custom profiles are made from their dimensions.

## Naming

- **US shapes** are written the way the AISC Manual prints them, with a lowercase x: W12x50, C10x15.3, L4x4x1/2, HSS6x6x1/4, HSS6.625x0.280. Typing is not case-sensitive.
- **Pipe** is written as in the AISC database: Pipe4STD, Pipe4XS, Pipe4XXS (standard, extra strong, double extra strong).
- **European shapes** are written as in EN 10365: IPE200, HEA240, HEB300, UPN200 (no space).
- **Plates** are `PL<thickness>x<width>` in millimeters: PL20x200 is 20 mm thick and 200 mm wide. Any size from the {{ui:Plate}} tool works.
- **Custom profiles** have whatever name you give them, as long as it is not a catalog name and does not look like a plate name.

In the {{ui:Profile}} lists, each group's heading names its source, for example "W shapes — AISC Shapes Database v16.0".

## Sources

| Group | Source | Notes |
|---|---|---|
| W shapes, S shapes, Channels (C), Angles (L), HSS, Pipe | AISC Shapes Database v16.0 | Dimensions converted from inches to millimeters exactly; mass per meter from the published lb/ft |
| IPE, HEA, HEB, UPN | EN 10365:2017 | Millimeters and kg/m as published |
| Plates | Parametric | Mass from area × 7850 kg/m³ |
| Custom | Your dimensions | Mass from area × 7850 kg/m³ |

How shapes are drawn:

- **W shapes:** the AISC database gives no fillet radius, so DetailCraft draws the fillet as r = kdes − tf.
- **S shapes and channels (C, UPN):** parallel flanges of the published average thickness, square corners (the real flange is sloped).
- **Angles:** square corners.
- **Rectangular HSS:** the design wall thickness (0.93 × nominal, as AISC 360-22 B4.2 allows) with an outer corner radius of twice that.
- **Round HSS and Pipe:** design wall thickness (0.93 × nominal).
- **IPE, HEA, HEB:** the published root radius.

## The sizes

Every size the tables list in these groups, smallest first:

| Group | Sizes |
|---|---|
| W shapes (289) | W4x13 to W44x408 |
| S shapes (28) | S3x5.7 to S24x121 |
| Channels (32) | C3x3.5 to C15x50 |
| Angles, equal legs (61) | L2x2x1/8 to L12x12x1-3/8 |
| HSS, rectangular and square (525) | HSS1-1/2x1-1/2x1/8 to HSS34x10x1 |
| HSS, round (189) | HSS1.660x0.140 to HSS28.000x1.000 |
| Pipe (51) | Pipe1/2STD to Pipe26XS |
| IPE (18) | IPE80 to IPE600 |
| HEA (24) | HEA100 to HEA1000 |
| HEB (24) | HEB100 to HEB1000 |
| UPN (18) | UPN50 to UPN400 |

Angles with unequal legs are not in this list yet: DetailCraft draws an angle's two legs the same length. CadCraft can place them (see its {{ui:Library}} panel, {{ui:Standard}}).

Choose a placing tool and pick a size to see its depth, flange width, web and flange thickness, fillet radius, area (cm²), mass (kg/m) and source in the {{ui:Profile}} box of the {{ui:Properties}} panel.

## What each shape can do

| Shape | Holes | Copes | End plates and base plates | NC file | Shop drawing |
|---|---|---|---|---|---|
| I-shapes (W, S, IPE, HEA, HEB, custom I) | Web, top flange, bottom flange | Yes | End plates: beam and column. Base plates: yes | Yes | Yes |
| Channels (C, UPN) | Web, top flange, bottom flange | No | Base plates only | Yes | Not yet |
| Rectangular HSS | Side wall (web), top, bottom | No | Base plates only | Yes | Not yet |
| Round HSS and Pipe | Not yet | No | Base plates only | Yes | Not yet |
| Angles | Not yet | No | Base plates only | Yes | Not yet |
| Plates and custom flat bars | Plate face | No | No | Yes | Yes |

For a rectangular tube, a hole on the "web" face goes through both side walls. What each NC file holds is in [NC file fields](topic:nc1-fields).

## Need another size?

Add it as a [custom profile](topic:plates-and-profiles#add-a-custom-profile). The built-in list cannot be edited from the app yet.
