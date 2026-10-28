---
title: Configurations and design tables
summary: Keep several sizes of one part as named configurations saved in the part file, switch between them with one click, bring them in or out as a CSV design table, or build them all from the command line.
section: tasks
order: 106
keywords: design table, configurations, configuration, sizes, family of parts, csv, spreadsheet, variants, table driven, iparts, part family
---

A **configuration** is one size of the part: a named set of values for its parameters. The part keeps its configurations in its own file, so they travel with it. A **design table** is the same thing as a CSV file, one row per configuration.

Configurations set **parameters**, so add those first (see [Drive a part with parameters](topic:parameters)). A configuration stores the parameters that are plain numbers; parameters worked out from others (such as `area = w * d`) follow on their own.

## Save a configuration

1. On the {{ui:Parameters}} tab, set the parameters for one size of the part.
2. Under **Configurations**, type a name (for example `Large`) and press {{ui:Save current values}} or {{key:Enter}}.

The new configuration is marked **active**. Change a parameter afterwards and it says **active, changed since**, so you know the part no longer matches it.

## Switch, update, rename, delete

- {{ui:Apply}} sets the parameters to that configuration's values and rebuilds the part.
- {{ui:Update}} saves the current values into that configuration.
- To rename one, type the new name in its box and press {{key:Enter}}.
- {{ui:Delete}} removes the configuration. The parameters keep their values.

Each of these is one step: {{key:Mod+Z}} goes back. If you rename a parameter, every configuration follows the new name; if you delete one, it drops out of them.

The part file is saved with the configurations and with whichever one was active. The part opens with the values it was saved with.

## Bring a design table in or out

A design table is a CSV file with a column for each parameter and a row for each configuration:

```
name,width,thick,bolt
small,100,6,6.6
medium,140,8,8.5
large,200,12,13
```

The first column may be called `name`, `config` or `configuration`; every other column must be the name of a parameter of the part. Cells are numbers, with units if you like (`8in`). An empty cell leaves that parameter out of the row.

- {{ui:Import CSV}} adds each row as a configuration. A row with the name of an existing configuration replaces it. Importing is one step.
- {{ui:Export CSV}} writes the configurations out. With none yet, it writes one row of the current values, a starting point for a spreadsheet.

A configuration name that a spreadsheet would read as a formula (starting with `=`, `+`, `-` or `@`) is written with a leading `'`.

## Every size at once

The command line tool builds one file per row of a CSV design table:

```
node tools/partcraft-cli/partcraft.mjs build bracket.pcscript --table sizes.csv --out-dir sizes --format step
```

Export the configurations as CSV first, then pass that file with `--table`. See [Scripts and the command line](topic:scripts-and-cli). A part saved as a script keeps its configurations in a `configs json …` line.
