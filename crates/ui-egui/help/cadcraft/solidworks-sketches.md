---
title: Move sketches to and from other part modelers
summary: Write a macro that rebuilds your drawing as one constrained sketch in a part modeler, and open a sketch exported from one.
section: tasks
order: 127
keywords: sketch, macro, swb, vba, export, part modeler, global variables, equations, relations, cadcraft-sw-export, swbout
---

:::warning
SolidWorks is not available where CadCraft is built, so neither direction has been tested in SolidWorks yet. What CadCraft writes is checked for structure only. Check the result before you rely on it.
:::

## Send a drawing as a sketch

1. Constrain and dimension the drawing (see [Constrain a drawing](topic:constraints)).
2. In the {{ui:Parametric}} group, open {{ui:More}} and choose {{ui:Macro for SolidWorks (.swb)}}, or type {{cmd:SWBOUT}}.

   A file named after the drawing, ending `.swb`, downloads.
3. In SolidWorks, choose **Tools > Macro > Run** and pick that file.

The macro makes a new part from your default part template, opens a sketch on the Front Plane and draws the model's lines, circles, arcs, points and polylines in it, with:

- your constraints as sketch relations,
- your dimensional constraints as dimensions with the same names (`d1@Sketch1`),
- your own parameters as global variables,
- every expression as an equation.

The part uses the drawing's length unit. Text, blocks, hatches, dimensions and sheets are not part of the sketch; the history lists what was left out. Expressions with no equation form there (`if`, `min`, `max`, `round`, `%` and a few others) are written as their current value, and the history says which. Anything the macro cannot make is written to a log file next to the macro, and a message sums up at the end.

## Open a sketch from a part

1. In SolidWorks, run the CadCraft export macro (download it in the PartCraft app: File > Export > Export macro to bring parts back) on the part. It writes a `.json` file in the `cadcraft-sw-export` format next to the part.
2. In CadCraft, choose {{ui:Open}} and pick that `.json` file.
3. When the part has several sketches, choose one.

The sketch becomes the drawing, in millimetres: its lines, arcs, circles and points, its relations as constraints, its dimensions as dimensional constraints named as they were (`D1`, `D2` …), its global variables as parameters, and the equations on its dimensions as their expressions. The history lists what was not carried over, such as relations CadCraft does not have. Where the sketch sits in the part is not kept: it is drawn flat, as seen on its plane.
