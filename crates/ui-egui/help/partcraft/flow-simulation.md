---
title: Simulate fluid flow (Flow study)
summary: Flow through the channels inside a part or around it in a box: inlets, outlets, air, water or your own fluid, then speed and pressure on a movable cut plane, streamlines, surface pressure, pressure drop, mass balance, drag and lift, with what the laminar method can and cannot tell you.
section: tasks
order: 145
keywords: flow, CFD, fluid, computational fluid dynamics, flow simulation, air, water, viscosity, density, Reynolds number, laminar, turbulent, pressure drop, head loss, inlet, outlet, lid, cap, velocity, flow rate, volume flow, litres per minute, static pressure, streamlines, cut plane, section plane, drag, lift, drag coefficient, lift coefficient, frontal area, external flow, internal flow, pipe, channel, manifold, nozzle, lattice Boltzmann, residual, convergence
---

A **Flow** study works out how a fluid moves through the channels inside your part (**internal**) or around it (**external**). By default it is **laminar, incompressible, isothermal and Newtonian**: smooth flow of a fluid whose density and viscosity stay fixed, with no heat, no turbulence and no gravity. An **experimental** turbulent model is also there, for qualitative patterns only (see below). It works on the named faces the mesh kernel gives the part (see [The mesh kernel](topic:solid-kernels)).

Choose **Simulation** in the **Workspace** box, press **New study** if there is none, and set **Type** to **Flow (CFD)**. **Flow model** is **Laminar** (the default) or **Turbulent (Spalart–Allmaras, experimental)**: see **Turbulent flow (experimental)** below. The structural settings of the study are kept, so you can switch back. The flow settings are saved in the part file and every change can be undone.

## Pick the fluid and the cells

- **Fluid**: air or water at 20 °C, or **Custom** with your own density (kg/m³) and dynamic viscosity (Pa·s). Air uses the ideal gas law and Sutherland's law (1.204 kg/m³, 1.813 × 10⁻⁵ Pa·s); water uses the IAPWS values (998.21 kg/m³, 1.0016 mPa·s).
- **Cell size**: the fluid space is filled with equal cubes. Leave it empty for an automatic size; the panel shows the grid. Every opening and channel should be **at least 8 to 10 cells across** for useful numbers; smaller cells are more accurate and slower (twice as fine is about 16 times the work).
- **Walls**: **Interpolated** follows curved walls between cells (the default); **Bounce-back** treats walls as the cube staircase.

## Flow inside the part

The fluid space must be closed. Add an **Inlet** and an **Outlet** in the panel, then click their faces in the view:

1. **The flat face around an opening** — the rim at a pipe end, a port face. PartCraft closes the opening's hole there with a lid. Every hole in that face belongs to that opening, so when an inlet and an outlet open on the same face, model a thin cap over one of them instead.
2. **The inner face of a cap** — as in other CAD tools: model a thin plate (an extrude) over the opening and pick its face that touches the fluid.

For an inlet set a **velocity** (m/s, uniform across the opening, into the part) or a **volume flow** (L/min). For an outlet set the **static pressure** (Pa, gauge). If an opening is left open, or a wall is thinner than a cell, the inside joins the outside and the solve stops with "No closed fluid space".

## Flow around the part

Set **Flow** to **External**, the **Speed**, the direction it flows towards, and how far the box reaches upstream, downstream and to the sides (empty: 1, 3 and 1 times the part's largest size). The box face the flow enters by is an inflow, the face it leaves by an outflow at 0 Pa, and the faces along the flow slip. A box that is too small squeezes the flow past the part and raises drag. **Reference area** is the area drag and lift coefficients use (empty: the part's frontal area seen along the flow); lift is measured along +Y unless you pick another direction.

## Solve and read the results

Press **Solve**. The solve runs in the background with the step count, the residual and a residual plot; **Stop** ends it. It stops when the residual (how much the velocity field still changes per step) is below the target and the pressure drop or drag has settled. If it reaches the step limit first the panel says **Not converged** in red: the numbers are the last state, not a steady answer.

- **Cut plane**: speed or static pressure on a plane through the model; choose its normal (X, Y or Z) and drag the slider to move it. Hover it to read the values.
- **Streamlines**: from the inlets (internal) or from a rake across the inflow end of the box in the cut plane (external).
- **Part**: solid, see-through (to see the plane inside), surface pressure, or hidden.
- **Table**: Reynolds number, pressure drop (mean static pressure at the inlets minus at the outlets), the flow and mean pressure of each opening, the force on the part, drag and lift with their coefficients and reference area (external), the mass balance (inflow against outflow), the mesh and the solver.

## Turbulent flow (experimental)

**Flow model › Turbulent (Spalart–Allmaras, experimental)** solves the time-averaged turbulent flow (steady RANS): the Spalart–Allmaras model gives an eddy viscosity in every cell, and the cells next to walls use Spalding's law of the wall instead of resolving the thin layer there. It is fully turbulent everywhere (no laminar regions, no transition).

**It did not pass its accuracy checks.** The panel, the results and the table say so on every turbulent study, and the warning cannot be closed. Use it for **qualitative flow patterns** — where flow separates, recirculates or speeds up — **not for design numbers**.

| Check | Reference | Result |
|---|---|---|
| Flat channel friction, first cell at y⁺ 49 / 98 | Dean's correlation | +3.3 % / +4.7 % |
| Resolved-wall channel (y⁺ ≈ 3), Re_τ 180: bulk velocity | Moser, Kim & Mansour DNS | −1 % |
| Flat channel, y⁺ 7–15 or 190–380 | Dean's correlation | +9 to +22 % |
| Pipe friction through the cube cells, fully developed, 10–30 cells across | Colebrook | 30 to 34 % too high |
| Reattachment behind a backward-facing step, 6–14 cells per step height | Driver & Seegmiller, 6.26 step heights | 21 to 39 % too short |

Results improve only slowly with finer cells. After solving, the table shows the **first-cell y⁺** (min, mean and max over the wall cells) and warns outside the checked range of 1 to 400; aim for about 30 to 300. **Run time:** the automatic cells are sized for a few minutes on one laptop core at most (the panel shows the worst case for the grid and step limit; a small pipe settles in under 20 seconds). Halving the cell size costs about 16 times as long. Forces on the part come from a momentum balance over the openings or the box.

**Work on it so far:** the wall cells now match their own shear stress to the wall law, which brought the pipe and step errors down to the figures above without changing the flat-channel results. That was not enough for design numbers. The next step is finer cells near the walls.

## What it can and cannot tell you

- **The laminar model** is laminar only. In pipes the flow turns turbulent above a Reynolds number of about 2,000 to 4,000; behind blunt bodies the wake starts to shed vortices above about 50. The panel warns at these points. A laminar result for turbulent flow underestimates pressure drop and gets drag wrong. The experimental turbulent model (above) covers turbulent flow qualitatively; its numbers are not reliable yet.
- **Method:** the lattice Boltzmann method (D3Q19, two-relaxation-time collision) run to a steady state. It is slightly compressible inside; PartCraft keeps density changes to a few per cent. If the flow is too fast for the cells it refuses to start and says how much smaller the cells must be.
- **Checked against:** Hagen–Poiseuille flow in a pipe through a part (pressure gradient within 1 % at 20 cells across), the lid-driven cavity of Ghia et al. (within 0.75 % of the lid speed), and the Schäfer–Turek cylinder benchmark (drag within 1 % at 10 cells per diameter).
- Not modelled: heat transfer, turbulence with design accuracy (the turbulent model is experimental), free surfaces, cavitation, gravity, moving parts, non-Newtonian fluids, rough walls. Lab build: check important results another way.
