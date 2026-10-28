---
title: Write your own analysis
summary: Build an analysis as scripts in the Script workspace: mesh the part, assemble and solve, show the field and a convergence plot, check the result against a hand calculation, and keep the steps as separate scripts that share variables.
section: tasks
order: 147
keywords: own analysis, custom analysis, FEA script, finite element, assemble, stiffness, boundary conditions, solve, pcg, sparse, field, post-process, convergence, verification, hand calculation, check, workflow
---

An analysis in the Script workspace is a few short scripts that hand variables to each other. Start from a [library](topic:craftscript-library) function that is close to what you need, make a copy, and change it; or build one up as below.

## 1. A mesh

```
model = part();                  % the part you are modelling, as a model
m = tetmesh(model, 4);           % tetrahedra about 4 mm across
show(m)
```

Check the mesh in the view before going on: {{key:F}} fits it, the {{ui:eye}} in **Shown** hides the part.

## 2. Assemble and solve

Write the element loop in the script (it reads like the textbook), and leave the heavy parts to the built-in functions: element matrices, sparse assembly from triplets, and the conjugate gradient solver with a preconditioner. A library function such as `static_tet` (once the library has it) is a complete example to copy from.

## 3. Show the result

```
f = field(m, vm, 'node', 'von Mises', 'MPa');  % one value per node
show(f, 'deform', u, 'scale', 100)
plot(residuals, 'title', 'Solver residual', 'id', 'conv')
```

- The legend's range can be typed in or set back to {{ui:Auto}}.
- Click the mesh to read the values at a node.
- A plot that keeps the same `'id'` is updated in place, so print and plot inside your loop to watch it converge.

## 4. Check it

Compare with something you know: a cantilever's tip deflection against PL³/3EI, a plate with a hole against its stress concentration factor, the reactions against the loads. Write the check as its own script so it runs again after every change:

```
tip = max(abs(u(:)));
expected = P * L^3 / (3 * E * I);
fprintf('tip %g mm, beam theory %g mm (%.1f %%)\n', tip, expected, 100 * (tip / expected - 1))
```

## 5. Keep it

- Each step can be its own script tab: they share the variables, so run them one after the other.
- Scripts are saved in the part. Tick **Save the variables with the part** on the Variables tab to keep the results too (up to 32 MB).
- {{ui:Save}} in the Script group downloads a script to share it; {{ui:Open}} brings one in.
