---
title: Mark rules
summary: Exactly when two parts or two assemblies share a mark, how marks are numbered, and how footing and bar marks work.
section: reference
order: 30
keywords: marks, numbering, part mark, assembly mark, piece mark, prefix, tolerance, identical parts, bar marks, footing marks
---

DetailCraft recalculates every mark after every change. Nothing is stored, so the marks always describe the model as it is now. See [Check marks and the bill of materials](topic:marks-and-bom) for how to use them.

## Part marks

Two parts share a part mark when all of these match:

1. The part type (beam, column, brace or plate).
2. The profile name (not case-sensitive).
3. The material (not case-sensitive).
4. The cut length, within 1 mm.
5. Every hole: face, position along and across, within 1 mm, and diameter within 0.1 mm.
6. Every cope: end, side, length and depth, within 1 mm.

The member's {{ui:Name}} does not count. Neither do its position in the model, its rotation, nor which way it was drawn, as long as the part itself is the same.

### Turned-over parts

A part is compared in every way it could be turned over that maps its section onto itself. If any of those makes it match another part, they share a mark.

| Shape | Turns that count |
|---|---|
| I-shapes, rectangular HSS, plates | End for end (about the vertical axis), end for end (about the horizontal axis), and upside down about its own length |
| Channels | End for end about the horizontal axis only, so the web stays on the same side |
| Angles, round HSS | None: compared as placed |

So a beam with a hole near its left end matches a beam with the same hole near its right end, and a column with holes in one flange matches a column with the same holes in the other flange.

### Numbering

| Type | Part prefix | Assembly prefix |
|---|---|---|
| Column | c | C |
| Beam | b | B |
| Brace | br | BR |
| Plate (placed, end plate or base plate) | p | P |

Marks are numbered from 1 within each prefix. The order is: columns, then beams, braces and plates; within a type, by profile name in natural order (W8x10 before W12x26); then longer parts first.

Because numbering follows that order, adding or changing one part can shift the numbers of others.

## Assembly marks

- Every part that is not welded to another part is the main part of its own assembly.
- End plates are welded to their beam, and base plates to their column, so they belong to that member's assembly.
- Two assemblies share a mark when their main parts share a part mark and their attached parts have the same marks at the same places (within 1 mm), allowing the same turns as the main part.
- The assembly prefix follows the main part: a beam with two end plates is a B assembly.

Plates you place with the {{ui:Plate}} tool are not welded to anything, so each is its own P assembly.

## Footing marks

Footings with the same length, width, depth, concrete text, bottom mat and starter bars share a mark: F1, F2 … in the order they were placed. Position and rotation do not count.

## Bar marks

Bars with the same size, shape and dimensions A, B and C (rounded to the millimeter) share a mark across all footings. The mark is the bar number, F, and a counter per bar size: 5F1, 5F2, 6F1. A long-way bar and a short-way bar of a square footing are the same bar and share a mark; the schedule's {{ui:Used for}} column lists both uses.
