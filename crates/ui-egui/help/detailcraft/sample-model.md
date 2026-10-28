---
title: Explore the sample frame
summary: Open the built-in two-bay frame and use it to learn what DetailCraft makes, from marks to NC files.
section: tasks
order: 20
keywords: sample, example, demo, frame, learn, tour, try
---

The sample frame is a small, complete model: every kind of thing DetailCraft builds appears at least once. It is the quickest way to learn the app and to test an export before you rely on it.

## Open it

1. Choose {{ui:Sample}} in the {{ui:File}} group.
2. If your current model has unsaved changes, choose {{ui:Save first}} or {{ui:Discard changes}}.
3. The status bar says "Sample frame: 6 columns with base plates, 4 beams with end plates, a brace, 7 footings with rebar."

The sample opens as a new model called "Sample frame". It does not change anything else, and it has nothing to undo.

## What is in it

- **Grids** 1, 2, 3 at 6000 mm and A, B at 7000 mm. **Levels** Level 1 at 0 and Level 2 at 4000.
- **Six columns**, W12x50, from Level 1 to Level 2 at every intersection, each on a 25 mm base plate with four anchor rods.
- **Four beams**, W16x26, at Level 2 along grid lines A and B, top of steel on the level, with a flush bolted end plate at both ends (eight in all).
- **One brace**, HSS4x4x1/4, from A/1 at Level 1 up to A/2 at Level 2.
- **Six pad footings**, 2000 x 2000 x 600, one under each column, with a hooked bottom mat both ways.
- **One more footing** beside the frame, 2400 x 1800 x 600, with six hooked starter bars standing up for a concrete pier.

## Things to try

1. Click a beam. Read its {{ui:Part mark}}, {{ui:Assembly}}, {{ui:Cut length}} and {{ui:Cut back}}: the cut length is shorter than the 6000 mm grid spacing because each end plate cuts the beam back by half the column depth plus the plate thickness.
2. Click a column. Under {{ui:Holes}} it lists the holes the end plates made in its flange ("8 holes from connections"; the middle columns have 16, one set in each flange).
3. In the {{ui:Part marks}} list on the left, see that all four beams share one mark: they are the same piece of steel. The corner columns share a mark too, even though their holes face opposite ways, because a column turned end for end is the same piece. The middle columns, with holes in both flanges, get another mark.
4. Click an end plate (or one of its bolts). The {{ui:Properties}} panel shows the plate, bolt and weld settings, the bolts it makes (with their length worked out from the grip) and any edge-distance warnings.
5. Open the {{ui:Bill of materials}} tab. Parts, assemblies (a beam with its two end plates is one assembly), and bolts and anchor rods are listed with weights. You should see six part marks: c1 (the two middle columns) and c2 (the four corner columns), both W12x50 at 3975 mm; b1, four W16x26 beams at 5650 mm; br1, the brace; p1, eight PL20x180 end plates; and p2, six PL25x400 base plates. The fasteners are 64 bolts 20 x 70 A325 and 24 anchor rods 20 x 575.
6. Open the {{ui:Bending schedule}} tab. The six identical footings share bar marks; the pier footing adds its own.
7. Click the brace and look at the {{ui:Output}} group: {{ui:Shop drawing}} is grayed out, because shop drawings cover I-shapes and plates only so far.
8. Choose {{ui:NC files}}. The zip holds a file for every I-shape and plate mark; the status bar lists the brace mark as not written.
9. Choose {{ui:IFC}} and open the file in any IFC viewer to check how your other software reads it.

## Use it as a starting point

The sample is an ordinary model. Change it, then rename it in the {{ui:Name}} field and save it under its new name with {{key:Mod+S}}.
