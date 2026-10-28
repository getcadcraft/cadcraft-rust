---
title: The analysis library
summary: The library of readable analysis scripts in the Script workspace: what is in each group, opening a function read-only, its header with method, references and a check, making your own copy, and how a copy takes over from the library function.
section: tasks
order: 146
keywords: library, craftlib, analysis, electromagnet, static, modal, thermal, topology optimization, top88, SIMP, electromagnetics, electrostatics, magnetostatics, fluid, lattice Boltzmann, LBM, mesh, voxel, tetrahedra, references, check, verification, copy, shadow
---

The analysis library is a set of scripts, each a function you can read from top to bottom: how the stiffness matrix is assembled, where the boundary conditions go, which solver runs, how the results are worked out. Nothing in it is hidden in a black box; only the fast inner loops (element matrices, sparse solvers, meshing) are built in.

## Start with an example

The **Examples** group holds five short scripts that use the library end to end. Click one, press {{ui:Open (read-only)}}, then {{key:F5}}: each clears the view, shows a result on the canvas and a plot in the sub-displays.

| Example | What it shows |
|---|---|
| Cantilever stress | A steel bar 100 × 10 × 10 mm under a 100 N tip load: von Mises stress on the deformed bar, and the tip deflection against beam theory at four element sizes (plotted) |
| MBB beam topology | The classic 60 × 20 topology optimization: the design forming, the compliance falling, the final layout (94 iterations, compliance 203.19) |
| Capacitor field | Two plates 4 mm wide, 1 mm apart: potential and field, the capacitance with fringing against the ideal value and Palmer's formula |
| Channel flow | Lattice Boltzmann flow past a post in a channel: the velocity field and the convergence history |
| Electromagnet | A C-shaped iron core with an air gap and a coil: the flux density, the field in the gap against the magnetic-circuit formula, the inductance and the pull across the gap. Walked through in [Magnetic fields in 2-D](topic:magnetic-fields-2d) |

## Groups

| Group | Functions | What they do |
|---|---|---|
| Meshing | `box_mesh`, `cylinder_mesh`, `voxel_mesh`, `tet_mesh`, `mesh_quality`, `nodes_at`, `face_nodes` | Boxes and cylinders as hexahedra, cubes or tetrahedra; cubes or tetrahedra of any model (the part, a body, a loaded STL); element volumes and quality; picking nodes on a plane or a face |
| Static stress | `static_voxel`, `static_tet` | Linear static stress on cubes or on 10-node tetrahedra: displacements, stresses, von Mises, reactions |
| Vibration | `modal_tet` | Natural frequencies and mode shapes |
| Heat | `heat_steady`, `heat_transient`, `heat_system` | Steady and transient conduction with fixed temperatures, heat input, generation, flux and convection |
| Topology optimization | `top88`, `top99`, `topopt3d` | The published 88- and 99-line 2-D codes, and the same method in 3-D on cubes |
| Electromagnetics | `poisson2d`, `magnetostatic2d` | 2-D electrostatics (potential, field, capacitance, charge) and magnetostatics (vector potential, flux density) |
| Fluid flow | `lbm2d`, `lbm3d` | Lattice Boltzmann flow: 2-D written out in the script, 3-D on PartCraft's own flow kernel |

Every function's header gives its exact arguments. Units are those of PartCraft's studies unless the header says otherwise: millimetres, newtons, megapascals, tonnes per cubic millimetre for density, watts and kelvin for heat. The electromagnetic and lattice Boltzmann functions work in any consistent units; their headers say how.

## How the library is checked

Each function's header ends with a **check**: a small case with a known answer. PartCraft's tests run every one of them, and where PartCraft has its own study for the same job the script's answer is compared with that study on the same mesh:

- the cantilever's tip deflection against beam theory (within 0.2 % with cubes, 0.05 % with tetrahedra), and against the **Simulation** workspace's solvers on the same mesh (the same to many digits);
- the cantilever's first frequency against beam theory (0.24 % lower, as expected for a short beam) and against **Simulation**'s modal solver;
- heat: a heated plane wall exactly, a small body cooling as the lumped-capacity formula says, a slab's temperature dying away as theory says;
- topology optimization: `top88` reproduces the paper's 94 iterations and compliance 203.1925 (and 218.8032 with the density filter); `topopt3d` ends at the same compliance as the **Generative** workspace with the same settings;
- a capacitor's capacitance and a long solenoid's field against their textbook values, and the electromagnet's gap field against the magnetic-circuit formula;
- channel flow against the exact parabolic profile.

## Read a function

1. Open the **Library** tab of the script panel.
2. Click a function. Its header shows: what it computes, the method, units, assumptions and limits, the **references** (book or paper, with equation numbers) and a **check** — a small case with a known answer that PartCraft's own tests run.
3. {{ui:Open (read-only)}} opens the whole script in a tab with a dashed border. You can run it, select and run parts of it, but not change it.

## Change one

Press {{ui:Make a copy}} (or **Make a copy to edit** above a read-only tab). The copy is a script in your part with the same name, so it is saved with the part, and **every call to that function now uses your copy** — the editor says so above the script. Rename the copy to keep both.

## Use one

Call a library function from any script by its name, like a built-in function:

```
xPhys = top88(60, 20, 0.5, 3.0, 1.5, 1);
```

Results come back as values in the workspace; plots and fields it shows appear in the view and the sub-displays.

A static analysis of the open part, for example, takes a few lines:

```
v = voxel_mesh(part(), 2);                   % 2 mm cubes
X = meshNodes(v);
bc = struct('fixed', nodes_at(v, 2, min(X(:, 2))));   % hold the bottom
top = nodes_at(v, 2, max(X(:, 2)));
bc.loads = {struct('nodes', top, 'force', [0 -500 0])};   % 500 N down on the top
r = static_voxel(v, 210000, 0.3, bc);       % steel
```

`r` holds the displacements, stresses and reactions; the von Mises stress is drawn on the deformed part. For curved parts use `tet_mesh` and `static_tet`, which can also load and hold whole faces (`face_nodes`).

:::note
The library's functions are written in CraftScript, and a Lua script calls them through `cs` like the built-in functions: `cs.static_tet(t, 200000, 0.3, bc)` (see [Lua scripts](topic:craftscript-lua)). Each function solves one material and one load case; the headers list what else is not covered.
:::
