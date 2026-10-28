---
title: Check marks and the bill of materials
summary: Read the part and assembly marks DetailCraft gives every piece, check the bill of materials, and download it as CSV.
section: tasks
order: 100
keywords: marks, numbering, part mark, assembly mark, piece mark, bom, bill of materials, material list, weights, bolt list, csv, numbering
---

DetailCraft marks the model by itself. There is no numbering command and nothing to run: after every change it works out which parts are the same piece of steel and gives them one mark.

## Where marks show

- The {{ui:Part marks}} list in the {{ui:Model}} panel: each mark with its profile, length and count. Click one to select the first part with that mark.
- The {{ui:Properties}} panel of a selected member: {{ui:Part mark}} and {{ui:Assembly}}. The panel heading also carries the mark, for example "Beam b1".
- The {{ui:Bill of materials}} tab, the CSV, the NC file names, the shop drawings and the IFC export (as each element's tag).

## How parts are marked

- **Part marks:** c for columns, b for beams, br for braces, p for plates (including end plates and base plates). Then a number: c1, c2, b1, p1 …
- **Assembly marks:** C, B, BR or P. An assembly is a main member and the plates welded to it, such as a beam with its two end plates, or a column with its base plate.
- Two parts share a mark when they have the same profile, material and cut length, and the same holes and copes, all within 1 mm, even if one is turned end for end or upside down relative to the other.
- Numbering runs columns first, then beams, braces and plates; within each, by profile name, then longest first.

The full rules, including which turns count for channels and angles: [Mark rules](topic:mark-rules).

:::warning
Marks are worked out fresh after every change, so they can renumber. Adding a part, changing a profile or adding a hole can move b3 to b4. Treat marks as final only when the model is finished, and make the BOM, NC files and shop drawings from the same saved model.
:::

## Read the bill of materials

Open the {{ui:Bill of materials}} tab at the bottom. It has three tables.

**Parts**, with the total steel weight in the heading:

| Column | Meaning |
|---|---|
| Part mark | Click it to select the first part with this mark |
| Type | beam, column, brace or plate |
| Profile | For example W16x26 or PL20x180 |
| Material | The grade you set |
| Length (mm) | Cut length, after connections |
| Count | How many |
| Unit weight (kg) | Profile mass per meter × length |
| Total weight (kg) | Unit weight × count |

The last row gives the total count and weight.

**Assemblies:** each assembly mark, its main part, all its parts (for example "b1 + p1 + p1"), count, unit weight and total weight.

**Bolts and anchor rods:** grouped by kind, diameter, length and grade, with a count.

:::note
Weights are gross: holes and copes are not taken off. Bolts, nuts, washers and welds are not in the steel weight.
:::

## Download it

Choose {{ui:Download CSV}} in the tab, or {{ui:BOM}} in the {{ui:Output}} group. The file `<name> BOM.csv` holds the three tables one after another, each under its title, with a blank line between. It opens in any spreadsheet program.

## Check before you send

1. Open the {{ui:Problems}} tab and clear every line. A connection that could not be built adds no plate, holes or bolts, so the BOM would be short.
2. Look for marks you did not expect. Two beams you think are the same but with different marks usually differ by a hole, a cope or a cut length; select each and compare.
3. Save the model, then make every export from that saved state.
