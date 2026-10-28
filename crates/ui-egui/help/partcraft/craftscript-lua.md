---
title: Scripts in Lua
summary: Write Script workspace scripts in Lua 5.4: the cs table of built-in functions, arrays as values with indexing and arithmetic, the shared ws workspace, printing and showing results, and what is switched off.
section: reference
order: 62
keywords: Lua, Lua 5.4, script, language, cs table, ws, workspace, userdata, tensor, array, matrix, print, display, sandbox
---

Lua is the second language of the [Script workspace](topic:craftscript). Choose **Lua** in the language box above the editor (a `.lua` file opens in Lua by itself). Lua and CraftScript share the same variables and the same built-in functions, so a Lua script can use what a CraftScript script made, and the other way round.

## The essentials

```
-- every built-in function is in the cs table
local K = cs.zeros(3, 3)
K:set(1, 1, 4)                 -- 1-based, like CraftScript
print(K(1, 1), #K)             -- an element; the number of elements

-- the workspace: the same variables every script sees
ws.total = K(1, 1) * 2
print(ws.total)

-- several results from one function
local V, D = cs.call('eig', 2, cs.tensor({{2, 1}, {1, 2}}))
```

- `cs.<name>(…)` calls any built-in function and returns its first result; `cs.call(name, n, …)` returns `n` results.
- `cs.tensor({1, 2, 3})` makes a row, `cs.tensor({{1, 2}, {3, 4}})` a matrix.
- Arrays, sparse matrices, meshes and fields are values with methods: `t(i, j)` or `t:get(i, j)`, `t:set(i, j, v)`, `t:shape()`, `#t`, `t:T()` (transpose), `t:totable()`. `+ - * / ^` mean what they mean in CraftScript (`*` is the matrix product); `t:times(u)` multiplies element by element.
- A number that is a 1×1 array arrives as a plain Lua number.
- `ws.name` reads and writes the shared workspace. Global variables your script leaves behind also land there when it ends.
- `print(…)` writes to the console; `cs.show(f)`, `cs.plot(y)` and the other display functions work as in CraftScript.

## Call the analysis library

The analysis library's functions (the Library tab) and the CraftScript functions your part's own scripts define are reached through `cs` too:

```lua
local t = cs.box_mesh(100, 10, 10, 5, 'tet10')
local bc = cs.struct({ fixedFaces = 5, loads = { cs.struct({ face = 6, force = cs.tensor({ 0, -100, 0 }) }) } })
local r = cs.static_tet(t, 200000, 0.3, bc)
print(r.maxDisp)
local nodes, tris = cs.call('face_nodes', 2, t, 6)   -- two outputs
```

They are found before the script starts, so write the name in the script: `cs.static_tet(` or `cs.call('static_tet', …)`. A name put together while the script runs only reaches the built-in functions. A struct built with `cs.struct({ … })` is a CraftScript struct, and a Lua list of them (`{ cs.struct(…), … }`) is a cell, which is what the library's settings expect. If something goes wrong inside a library function, the message names the function and its line, and points at your line that called it.

## Switched off

Scripts cannot reach files, the network or the page: `io`, `os` (except the clock), `require`, `dofile`, `loadfile` and `debug` are removed. Use the Script workspace's Open and the variables to bring data in. The step budget and Stop work as for CraftScript.
