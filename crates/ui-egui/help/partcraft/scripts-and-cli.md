---
title: Scripts and the command line
summary: The PartCraft script format (.pcscript) that writes a whole part as readable text, and the partcraft command line tool that builds, changes, rolls back and checks parts without the screen.
section: reference
order: 60
keywords: script, pcscript, text, command line, cli, terminal, batch, automate, check, lint, build, export, version control, diff, parametric script
---

Careful modeling is more about discipline than about buttons. A part written as text can be read, reviewed, compared line by line and rebuilt by a script. PartCraft goes both ways: any part can be written as a script, and a script builds the part.

## In the app

On the {{ui:Parameters}} tab, under **Script**:

- {{ui:Save as script}} downloads the open part as `<name>.pcscript`.
- {{ui:Open script}} builds a part from a `.pcscript` file. A mistake is reported with its line number and nothing is replaced.

## The script format

```
pcscript 1
part "Bracket"
  resolution 64

param width = 120
  note Plate width
param thick = 8
param bolt = 8.5
param margin = bolt * 1.5

sketch Plate
  on top
  point a 0, 0
  point b 120, 0
  point c 120, 60
  point d 0, 60
  line bottom a b
  line right b c
  line top c d
  line left d a
  coincident a O
  horizontal bottom
  horizontal top
  vertical left
  vertical right
  distance bottom = width
  distance left 60

extrude Base
  sketch Plate
  depth = thick

sketch HoleCentres
  on face Base end
  point h1 12, 12
  hdistance O h1 = margin
  vdistance O h1 = margin

hole Bolts
  sketch HoleCentres
  d = bolt
  through
```

The rules:

- A line at the left edge starts a statement: `pcscript`, `part`, `param`, a feature type (`sketch`, `extrude`, `revolve`, `hole`, `lpattern`, `cpattern`, `mirror`, `fillet`, `chamfer`, `shell`, `import` and the newer types) followed by its name, or `rollback <feature>`.
- Indented lines are the settings of the statement above. A setting on its own (`through`, `reverse`, `suppressed`) means yes.
- A number setting is a number, or `=` and an expression, which makes an equation: `depth = thick`.
- In a sketch: `on` names the plane (`front`, `top`, `right`, `face Base end`, `face Revolve1 side l3`); geometry is `point <name> x, y`, `line <name> <point> <point>`, `circle <name> <centre> <radius>`, `arc <name> <centre> <start> <end>` (counter-clockwise) or `spline <name> fit|control <points>`; relations are their type and the names they join (`O` is the origin); a dimension ends with its value or `= expression`.
- Optional settings of a geometry or relation line go in square brackets at the end, separated by `;`: `[construction]`, `[side -1; label 5, 3]`.
- `#` starts a comment (at the start of a line or after a space). Names with spaces go in "double quotes".
- Feature ids count from f1 in the order written. Edge and face references use them, for example `edge f2.side:l3 f2.end [at 50, 0, 10]`.
- Settings the format has no words for are written `json` and a value; they are read back as they are.

Saved from the app, the script includes every setting, so it opens as the same part. An imported STEP solid is written inside the script from the app, and as a separate `.step` file named by `step-file` from the command line.

## The command line tool

The tool runs on a computer with Node.js 24, from a copy of the CadCraft source: `node tools/partcraft-cli/partcraft.mjs <command>`. It is a developer tool and uses the same modules as the app. Parts are `.partcraft.json` files or `.pcscript` scripts.

| Command | What it does |
|---|---|
| `info <part>` | Features, parameters, rollback bar, the built solid's volume and size |
| `params <part>` | Every parameter and equation with its value |
| `set <part> width=150 Base.depth==thick` | Forward: change parameters, link numbers (`==` or an expression), set plain numbers |
| `drive <part> Base.depth=12` | Backward: solve the parameter behind a driven number (`--prefer width` picks one) |
| `build <part> --out x.step` | Writes `.step`, `.stl`, `.3mf`, `.json`, `.pcscript` or a macro (`.swb`) |
| `build <part> --table sizes.csv --out-dir sizes` | One file per design table row |
| `script <in.pcscript> -o part.json` | Script to part |
| `to-script <part> -o part.pcscript` | Part to script |
| `rollback <part> [<feature>]`, `forward <part> [--end]` | Move the rollback bar in the file |
| `check <part> [--strict]` | The discipline check (below) |

`build` also takes `--set name=value` (for this build only) and `--upto <feature>`. Every command takes `--json`. Exit codes: 0 fine, 1 a problem was found, 2 the command was typed wrong. A script is never rewritten in place: give `-o` to save a changed copy.

## The check

`check` reports what makes a part hard to change later:

| Finding | Counts as |
|---|---|
| A sketch that is over defined (conflicting or repeated relations) | error |
| A feature that does not build | error |
| A parameter or equation with an error, or parameters that use each other | error |
| A sketch that is under defined, with its degrees of freedom | warning |
| Numbers typed straight into features and dimensions that no parameter drives | warning |
| A parameter nothing uses | warning |
| A suppressed feature; the rollback bar not at the end | warning |

`--strict` makes every warning an error, so a build script can refuse a part that is not fully driven.

:::note
The `.swb` macro output is for the part modeler most shops use: run inside that program, it builds the part there. It has not been run in that program here (it is not available on our servers), so treat it as untested.
:::
