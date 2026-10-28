---
title: Magnetic fields in 2-D (tutorial)
summary: Work out the magnetic field of an electromagnet with the analysis library: run the example, read the flux density, the gap field, the inductance and the pull, change the design, and build your own model of coils and iron.
section: tasks
order: 147
keywords: magnetic, magnetostatics, magnetic field, flux density, tesla, electromagnet, solenoid, coil, ampere turns, iron core, air gap, permeability, inductance, magnetic force, reluctance, magnetic circuit, vector potential, electromagnetics, finite element, fem, motor, actuator, transformer, magnetostatic2d
---

PartCraft's analysis library can work out the magnetic field of coils and iron in 2-D: a cross-section of something that is long and the same along its length, such as an electromagnet, a transformer core, a busbar or a motor's cross-section. This tutorial runs the library's electromagnet example, reads what it says, changes it, and then shows how to model your own shape.

It uses the Script workspace and the [analysis library](topic:craftscript-library). You do not need to know the CraftScript language to follow it; the example says what each line does.

## What is modelled

A C-shaped iron core, 100 × 80 mm outside and 20 mm thick, with a 2 mm air gap in its right leg. A coil of 100 turns carrying 10 A (1000 ampere-turns) is wound round its left leg. Everything is long in the direction out of the screen, so the answers are **per metre of depth**: multiply by the real depth for a real magnet.

The library's `magnetostatic2d` solves for the magnetic vector potential on a grid of 1 mm squares in a box of air round the magnet, then works out the flux density **B** (in tesla) from it.

## Run the example

1. Choose **Script** in the **Workspace** box at the left of the toolbar.
2. Open the **Library** tab of the script panel, then **Examples** › **Electromagnet: field, gap, inductance**.
3. Click {{ui:Open (read-only)}}, then press {{key:F5}}.

In a second or two the canvas shows the flux density, and the console prints:

```
B in the gap 0.5427 T; magnetic circuit 0.5516 T (-1.6 %: fringing and leakage)
inductance 0.1539 H per metre of depth; the circuit gives 0.1103 H/m, missing the flux that leaks past the gap
stored energy 7.694 J/m; |B| peaks at 1.97 T in the window's inner corners (real steel would saturate there)
pull across the gap about 2343 N per metre of depth (117.2 N for 50 mm deep)
```

The plot below the canvas is the vertical flux density B_y along the core's middle line, from left to right: about −0.55 T in the left leg (the flux runs down it), almost nothing in the window, and +0.55 T in the right leg and across the gap (the flux comes back up). The same flux passes through the iron and the gap, so the two legs and the gap show nearly the same value.

## Read the results

- **The field in the gap** is 0.543 T. A hand calculation treats the core as one magnetic circuit, B = μ0 N I / (g + l / μr), where g is the gap, l the length of the path through the iron and μr the iron's relative permeability; it gives 0.552 T. The field is a little lower than the hand calculation because the flux bulges out round the gap (fringing) and some flux takes a short cut across the window (leakage). With a smaller gap or a finer grid the two come closer.
- **The inductance** of the 100-turn coil is 0.154 H per metre of depth, from the energy stored in the field. The circuit formula gives less (0.110 H/m) because it counts only the flux through the gap, not the flux that leaks past it.
- **The pull** that closes the gap is B² w / (2 μ0) for a gap face w wide: about 2300 N per metre, or 117 N for a magnet 50 mm deep.
- **The peak** of 1.97 T sits in the inner corners of the window, where the flux turns sharply. The model treats the iron as linear; real electrical steel saturates at about 1.5 to 2 T, so treat values above that as a warning, not an answer.

:::note
The library's own tests run this example and check the gap field against the magnetic-circuit formula on every change to PartCraft. The method, its references and its other checks (a long solenoid, an iron-cored solenoid) are in the header of `magnetostatic2d`: click it in the library.
:::

## Change the design

1. Click {{ui:Make a copy}} above the read-only script. The copy is saved with your part.
2. Change a value near the top and press {{key:F5}} again. Try:
   - `g = 0.001;` — half the gap: the gap field nearly doubles, and so does the pull (it goes with B²).
   - `mur = 200;` — a poorer core: more of the ampere-turns are spent in the iron.
   - `N = 200;` — twice the turns: twice the field, four times the inductance.
   - `h = 0.0005;` — a finer grid: closer answers, about four times the time.

Each run replaces the canvas. The variables the script made (such as `Bgap`, `L`, `F`) stay in the **Variables** list for your own sums.

## Model your own shape

The example builds everything from three lists, one value per grid square, which you can make for any shape drawn with straight lines along the grid:

- **Where the iron is:** `iron` is true for the squares inside the core. The example builds it from rectangles: `frame & ~window & ~gap` means "in the frame, not in the window, not in the gap".
- **The permeability:** `1 + (mur - 1) * iron` is 1 in air and `mur` in the iron. Give several materials their own values the same way.
- **The current:** `J` is the current density in amperes per square metre, positive out of the screen, negative into it. A coil has two sides with opposite signs; the example works out `J0` so that one side carries N I.

Then `r = magnetostatic2d(m, permeability, J)` solves it. `r.B` is the flux density in each square (two columns, x and y), `r.Bmag` its size, `r.energy` the stored energy per metre. `plot(x, y)` draws a line.

The library works in metres, and the view in millimetres, so a field shown as it is would be drawn a thousand times too small. The example shows it on a copy of the grid in millimetres (`mm = quadGrid(…)`, then `show(field(mm, …))`): copy those two lines.

By default the edge of the box holds the field in: no flux leaves it. Make the box at least two or three times the size of the magnet so that this does not squeeze the field. For a symmetric magnet you can model half of it: the header of `magnetostatic2d` says which edge condition to use on a line of symmetry.

## What it does not do yet

- **Saturation.** The iron is linear: double the current, double the field, without limit.
- **Permanent magnets**, **eddy currents** and **losses** (iron loss, copper loss).
- **Moving parts and torque.** No rotor turning in a stator, no torque against angle.
- **Round shapes.** The grid is made of equal squares, so curved outlines come out stepped.
- **3-D.** Everything is a cross-section of something long.

Motor design (a rotating rotor, torque and losses in 2-D) is being looked at for a later version. See [Not built yet](topic:not-built-yet).
