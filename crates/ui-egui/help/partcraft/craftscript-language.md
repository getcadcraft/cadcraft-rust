---
title: The CraftScript language
summary: CraftScript in one page: matrices, ranges and indexing, operators, control flow, functions, structs and cells, comments, echo, errors and the step budget, and how it differs from MATLAB.
section: reference
order: 61
keywords: CraftScript, language, syntax, matrix, array, vector, tensor, index, end, colon, range, operator, element-wise, transpose, if, for, while, switch, try, catch, function, anonymous function, struct, cell, comment, echo, semicolon, MATLAB, Octave, port
---

CraftScript is a matrix language written to read like MATLAB, so most classic numerical codes (the 88-line topology optimization code, for example) run with only small changes. It runs in the [Script workspace](topic:craftscript) and from the command line.

## Values

| Type | Example | Notes |
|---|---|---|
| Number, array | `a = 3`, `v = [1 2 3]`, `A = [1 2; 3 4]` | Every number is an array (a 1×1 one). Arrays are stored column by column, up to 8 dimensions |
| Logical | `A > 2`, `true` | Arrays of true/false; usable as indices |
| Text | `'beam'`, `"beam"` | |
| Sparse matrix | `sparse(i, j, v, n, n)` | For stiffness matrices |
| Mesh, field, model | `hexGrid(…)`, `part()`, `load('part.stl')` | What `show` draws |
| Struct | `s.E = 210e3` | Named fields |
| Cell | `c = {1, 'two', [3 4]}` | A list of any values |
| Function handle | `f = @(x) x.^2` | |

## Arrays and indexing

- Ranges: `1:5`, `0:0.1:1`. `linspace(a, b, n)`, `zeros(n, m)`, `ones`, `eye`, `rand`.
- Indexing starts at 1: `A(2, 3)`, `v(end)`, `A(:, 1)` (a whole column), `A(:)` (everything as one column), `v(v > 0)` (logical indexing).
- Assigning past the end grows the array, as in MATLAB: `v(10) = 1`.

## Operators

- `+ - * / \ ^` are matrix operations (`A \ b` solves); `.* ./ .^` work element by element. `'` transposes.
- Comparisons `== ~= < <= > >=`; logic `& | ~` (element by element) and `&& ||` (short-circuit).

## Control flow and functions

```
for k = 1:10
  if mod(k, 2) == 0
    continue
  elseif k > 7
    break
  end
end
while err > 1e-6, … end
switch kind
  case 'steel'
    E = 210e3;
  otherwise
    E = 70e3;
end
try
  x = A \ b;
catch e
  disp(e.message)
end
```

Functions go in a script after its commands, or in their own script tab named after the function:

```
function [k, m] = beam(E, I, L)
  k = 3 * E * I / L^3;
  m = 0;
end
```

A name is looked up in this order: a variable, a script in this part with that name, a library function, a built-in function.

## Echo, comments, output

- A statement without `;` prints its value; `;` keeps it quiet.
- `%` starts a comment; `%{` and `%}` on lines of their own comment out the lines between them.
- `disp(x)`, `fprintf('%g\n', x)` and `display(x)` print to the console.

## Errors and limits

- Errors say what went wrong in plain words and give the line (and the chain of function calls).
- Each run has a step budget (Run options in the panel); an endless loop stops with a message, or press Stop.
- Arrays hold up to 134 million values each.

## Not like MATLAB

- Plot commands put their options in the call: `plot(x, y, 'title', 'Error', 'xlabel', 'Step')`. `figure`, `hold`, `axis`, `title(…)` and similar are accepted and do nothing.
- `input` and `keyboard` are not supported: a script cannot stop to ask a question.
- There are no toolboxes; the [analysis library](topic:craftscript-library) and the built-in functions are what there is.
