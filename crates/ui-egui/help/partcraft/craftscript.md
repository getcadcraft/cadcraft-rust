---
title: Write and run scripts (Script workspace)
summary: The Script workspace step by step: write a CraftScript or Lua script, run it with F5, read the console, keep results in the variables for the next script, show meshes, fields and plots, stop a run, and save scripts with the part.
section: tasks
order: 145
keywords: script, scripting, script while modelling, pop out, second screen, second monitor, param, params, parameters in scripts, CraftScript, Lua, code, program, editor, console, REPL, command line, run, F5, run selection, stop, variables, workspace, plot, field, mesh, legend, sub-display, analysis, automation, matrix, numerical
---

**Preview.** Scripting is new in PartCraft 0.3 and is a preview: the language, the library and the way scripts are saved may still change, and scripts written now may need small edits later. Tell us what breaks.

The Script workspace runs your own calculations next to the part: small sums, whole analyses, or the scripts in the [analysis library](topic:craftscript-library). Scripts are written in **CraftScript** (a matrix language much like MATLAB, see [The CraftScript language](topic:craftscript-language)) or in **Lua** (see [Scripts in Lua](topic:craftscript-lua)) or **Python** (see [Scripts in Python](topic:craftscript-python)).

Choose **Script** in the **Workspace** box at the left of the toolbar. The part is not changed. The right panel becomes the script panel (wider than usual; drag its edge to change it, or move it to a second screen with its {{ui:⋯}} menu, see [Arrange the panels](common:arrange-panels)).

## Script while you model

You do not have to leave the Design workspace to write a script. In Design, the right panel's {{ui:Script}} tab holds the same editor, console, variables and library, with the same scripts. The feature tree and the view keep working: click a feature to select it, double-click a sketch to edit it, turn the view, then go back to the {{ui:Script}} tab and press {{key:F5}}.

What a script **shows** (meshes, fields, plots, tables) is not drawn over your model in Design: the console says so, and the Script workspace has it all when you switch to it.

{{ui:Pop out}}, beside {{ui:Run}}, moves the whole panel to a window of its own, for example on a second screen, so the editor and the full view sit side by side. {{ui:Put back}} docks it again. See [Arrange the panels](common:arrange-panels).

## Run your first script

1. Type in the editor, for example:

   ```
   x = linspace(0, 2*pi, 50);
   y = sin(x);
   plot(x, y, 'title', 'A sine')
   disp(max(y))
   ```

2. Press {{key:F5}} (or {{ui:Run}}). The console under the editor shows what the script printed, and "Done in … s". The plot appears in the **sub-displays** along the bottom of the 3D view.
3. A line without a semicolon prints its value, as in MATLAB: `a = 3` prints `a = 3`; `a = 3;` prints nothing.
4. To run only part of a script, select the lines and press {{key:Mod+Enter}} ({{ui:Selection}}). With nothing selected it runs the line the cursor is on.
5. The line at the bottom of the console (after `>>`) runs one statement as you press {{key:Enter}}. {{key:Up}} and {{key:Down}} bring back earlier lines.

## Find, fold and complete

- **Find and replace.** Press {{key:Mod+F}} (or {{key:Mod+H}} to replace too). A bar opens above the code: type what to find; the matches light up and the count says where you are. The buttons beside the box match case (**Aa**), find whole words only (**W**) and use a regular expression (**.\***; in the replacement, `$1` puts back what the first group matched). {{ui:Replace}} replaces the current match, {{ui:All}} replaces every match as one step of {{key:Mod+Z}}.
- **Folding.** Click the ▾ beside a line number, or press {{key:Mod+Shift+[}} with the cursor on a `function`, `if`, `for`, `while`, `switch` or `try` line (Lua: `function`, `do`, `if`, `repeat`), to tuck its inner lines away; the line says how many. ▸ or {{key:Mod+Shift+]}} opens it again. Folding only hides lines: Run, Save, copying and the line numbers all still count every line, and typing on a folded line opens it first.
- **Completions.** Type two letters of a name, or press {{key:Mod+Space}}: a list shows the variables you have, the language's functions (with their arguments), keywords and names already in the script. Choose with the arrows and press {{key:Enter}} or {{key:Tab}}.

## When something goes wrong

- An error names the problem and its line: the line is marked red in the editor, and **Line n** in the console takes you there.
- A loop that never ends: press {{ui:Stop}} (toolbar or panel). The script stops at once; the variables keep the values they had before that run.
- Every run also has a step budget (2 billion steps by default; change it under **Run options** in the panel). A run that passes it stops with a message.

## Variables carry from script to script

Every top-level variable a script makes lands in the **Variables** tab (name, type, size, smallest and largest value) and stays there for the next script, in either language. That is how one script meshes, the next solves, and a third plots.

- Click a variable to show it: meshes, fields and models in the 3D view, arrays as a table (plus a line plot for a vector or a colour map for a matrix) in the sub-displays.
- {{ui:Save as JSON}}, {{ui:Save as CSV}} (a table of numbers) and {{ui:Save as HDF5}} download it; {{ui:Delete}} removes it; {{ui:Clear all}} empties the workspace.
- Tick **Save the variables with the part** to keep them in the part file (up to 32 MB; bigger ones stay for this visit only). When you open such a part, the panel offers to load them.

## Meshes, fields and plots

- `show(m)` draws a mesh; `show(f)` colours a field on its mesh with a legend. The **Shown** list at the top left of the view hides ({{ui:eye}}) or removes each item, and hides or shows the part itself. Under the legend, type a range (**From**, **to**) or press {{ui:Auto}}; a vector or stress field also offers which component to show.
- `show(f, 'deform', u, 'scale', 50)` draws the mesh moved by a displacement field `u`, enlarged 50 times (without `scale`, about 8 % of the model's size).
- Click a shown mesh: the nearest node, the element and the field values there appear in the sub-displays (**Picked**).
- `plot`, `imagesc` and `table` put a line plot, a colour map of a 2-D array or a table in the sub-displays. A plot with the same `'id'` (or title) as an earlier one replaces it, so a loop can update one convergence plot; `'append', true` adds a curve to it. {{ui:Clear}} empties the sub-displays; the arrow folds them away.
- With two or more plots, {{ui:Side by side}} shows them all at once (the newest six); clicking a table's or picture's tab shows that one alone again.
- Every plot has {{ui:PNG}}, {{ui:SVG}} and {{ui:CSV}} under it. PNG and SVG print it on white in the paper colours, whatever the theme (PNG 1600 × 1000 pixels; SVG scales without blurring); CSV holds its numbers with every digit: one column of x and a column per curve when the curves share x, otherwise one row per point with the curve's name.
- {{key:F}} or {{ui:Fit}} fits the view to what is shown.

## Files

- {{ui:Open}} in the Script group (or drop files on the view or the panel): `.craft`, `.lua` and `.py` files open as new scripts (they do not run); data files (STL, OBJ, CSV, JSON, NPY, HDF5) load into the variables under their file name.
- {{ui:Save}} in the Script group downloads the current script as a file.
- {{ui:New}} in the Script group (or {{ui:+}} by the tabs) starts another script. Double-click a tab to rename it; its {{ui:×}} removes it (you can undo that).

## Read the part's parameters

A script reads the part's parameters (see [Drive a part with parameters](topic:parameters)) by name:

```
w = param('width');     % one parameter's value, in mm (or degrees, or a plain number)
p = params();           % all of them, as a struct: p.width, p.height
```

In Lua and Python they are `cs.param('width')` and `cs.params()`. Scripts only read parameters: change them on the {{ui:Parameters}} tab. Under the script's tab, a line lists the parameters it reads with `param('…')`, and marks any the part does not have; the {{ui:Parameters}} tab lists the script under each one. A name built while the script runs, such as `param(name)`, is not listed.

## Scripts are saved in the part

Every script in the tabs is kept in the part, so {{ui:Save}} in the File group saves them with it and changes to them can be undone. Opening a part **never runs its scripts**: the console says how many it carries, and they run only when you press Run. A part from someone else can hold any script: read it before you run it. Scripts run in the background on this computer with no network access and cannot touch the page.

## Related

- [The CraftScript language](topic:craftscript-language)
- [Scripts in Lua](topic:craftscript-lua)
- [Scripts in Python](topic:craftscript-python)
- [The analysis library](topic:craftscript-library)
- [Write your own analysis](topic:craftscript-own-analysis)
