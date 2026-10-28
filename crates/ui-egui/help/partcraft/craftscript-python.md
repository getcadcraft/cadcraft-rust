---
title: Scripts in Python
summary: Write Script workspace scripts in Python 3: the cs object of built-in functions, arrays as numpy-like values with 0-based indexing, the shared workspace, printing, what is not there yet, and how Python is sealed off like CraftScript and Lua.
section: reference
order: 63
keywords: Python, Python 3, CPython, WebAssembly, sealed, safe, script, language, cs, ws, workspace, numpy, array, matrix, indexing, print, display, trust
---

Python is the third language of the [Script workspace](topic:craftscript). Choose **Python** in the language box above the editor (a `.py` file opens in Python by itself). Python, Lua and CraftScript share the same variables and the same built-in functions, so a Python script can use what a CraftScript script made, and the other way round.

The first Python run in a visit loads Python itself (CPython 3.14, about 7 MB) from this site, which takes a few seconds. Python is not part of {{ui:Save for offline}} yet: run one Python script while online first.

## Python is sealed off

Python scripts are sealed off like CraftScript and Lua scripts: they reach the part, the variables and the built-in functions, and nothing else. Each run is a fresh Python in its own sandbox with a small file tree in memory (its standard library and a `/tmp` folder that is gone after the run). It cannot reach the network, your files or the rest of the page, in the browser and on the command line alike. A script from someone else can still use a lot of time or memory: {{ui:Stop}} ends it.

## The essentials

```
A = cs.magic(3)              # every built-in function is cs.<name>
B = A @ A.T                  # @ is the matrix product, .T the transpose
C = A * 2 + 1                # + - * / ** are elementwise, as in numpy
print(A[0, 1], A[-1, -1])    # 0-based; negative counts from the end
row = A[1]                   # one index on a matrix: a row
big = A[A > 4]               # a logical mask picks elements
A[0, 0] = -5                 # assigning into an array
V, D = cs.call('eig', 2, A)  # several results: cs.call(name, how many, …)
```

- A 1 × 1 array arrives as a plain number; other arrays, sparse matrices, meshes, fields and models are **Values**.
- A Value has `shape` (a tuple), `T`, `type`, `numel()`, `copy()`, `tolist()`, `len()` (its rows), and `float()`, `int()` and `bool()` for a single element.
- Comparisons (`<`, `==`, …) are elementwise, as in numpy: they give a logical Value.
- Lists of numbers passed to a built-in function become arrays: `cs.sum([1, 2, 3])`. `cs.tensor([[1, 2], [3, 4]])` makes a matrix.
- A dict becomes a struct and a list of mixed things a cell, and back again.

Every global your script makes (a number, a Value, a dict, a list…) lands in the **Variables** tab when the run ends, even after an error. Names starting with `_`, functions, classes and modules stay in Python. `ws.name` reads or sets a variable directly, and `ws_names()` lists them.

`print(…)` writes to the console. `display(…)` (or `cs.display`) shows a mesh, a field or a plot, as in CraftScript.

## Call the analysis library

Library functions are called like built-in ones: `cs.box_mesh(20, 10, 10, 5, 'hex')`, `cs.call('modal_tet', 2, …)`. Write the name in the script as it is: PartCraft finds the library functions a script uses before it runs.

## Not there yet

- numpy, scipy and other packages: only Python's own standard library (`math`, `json`, `itertools`, `decimal`, `statistics`…) is here, without its windows, terminal and database parts (`tkinter`, `curses`, `sqlite3`).
- Sockets and anything else that would reach outside: Python has no network here.
- `time.sleep` returns at once.
- Folding in the editor, and calling a Python function from a CraftScript script.
- A run checks the instruction budget and Stop every 1000 lines; a long calculation inside one built-in function finishes first.
