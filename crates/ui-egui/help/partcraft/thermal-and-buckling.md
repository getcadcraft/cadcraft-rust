---
title: Heat, thermal stress and buckling
summary: Find the temperatures in a part from fixed temperatures, heat flux, heat power, convection and radiation (steady or over time), load a stress study with a temperature rise, and find the load at which a slender part buckles, with what each method can and cannot tell you.
section: tasks
order: 143
keywords: thermal, heat transfer, heat, temperature, conduction, convection, film coefficient, radiation, emissivity, heat flux, heat power, heat generation, heat sink, transient, time, cooling, heating, Crank-Nicolson, implicit Euler, heat balance, thermal stress, thermal stress over time, transient thermal stress, quench, thermal shock, stress history, worst time, thermal expansion, expansion coefficient, buckling, Euler buckling, load factor, critical load, column, slender, eigenvalue
---

These studies live in the Simulation workspace next to static stress and natural frequencies (see [Check stress and deflection](topic:simulation)). Pick them in the panel's **Type** box. They use the same mesh of small cubes and the same material list; every library material has a thermal conductivity, a specific heat and an expansion coefficient (typical handbook values, shown under **Properties**).

## Thermal: where does the heat go?

1. Choose **Thermal (heat transfer)** as the **Type**.
2. Add thermal conditions from the panel's **Add** list, then click their faces (as for supports and loads):
   - **Fixed temperature**: the face is held at a temperature, in °C.
   - **Heat flux**: W/m² flowing into the face (negative flows out). It acts on the face's true area, even where the cubes make a staircase.
   - **Heat power**: a total in watts, shared over the picked faces by area. Use it for a heater or a chip of known power.
   - **Convection**: air or liquid at a temperature carries heat away at h W/(m²·K). Typical h: still air 5–10, a fan 25–100, water 500–10,000. PartCraft does not compute h: there is no fluid flow in this study.
   - **Radiation**: the face radiates to surroundings at a temperature, with an emissivity from 0 to 1. It is **linearised** (the radiation is written as a coefficient that depends on the temperature and re-solved until it settles) and goes only to the surroundings: faces do not exchange heat with each other.
3. **Heat generation** (W/m³) heats the whole part from inside, for example a resistive part.
4. Press **Solve**. The part is coloured by temperature; choose **Heat flux** in **Show** to see where the heat flows. Point at the part to read both.

Steady heat flow needs somewhere for the heat to go: at least one fixed temperature, convection or radiation. A part that only takes heat in has no steady state; make it transient instead.

The table lists the lowest and highest temperature and where they are, the largest heat flux, and the **heat balance**: the heat into or out of each condition (a fixed temperature counts what it must supply or absorb), the heat generated, the totals in and out, and the **imbalance**. The imbalance should be a tiny fraction of the heat in; a large one means the result is not to be trusted.

### Over time (transient)

Tick **Transient** and give a **Start temperature** (the whole part starts there), a **Time span** and a number of **Time steps**. **Implicit Euler** is robust; **Crank–Nicolson** is more accurate for the same steps but can ripple after a sudden change. After solving, drag the **Time** slider to see the temperatures at each moment (the colour scale stays the same for every moment). The heat balance then includes the heat being stored.

## Thermal stress: what does a temperature change do to the part?

In a **Static stress** (or **Buckling**) study, set **Thermal load**:

- **Uniform temperature change**: the whole part warms (or cools, with a negative number) by that many kelvin.
- **Temperatures from thermal conditions**: add thermal conditions to the same study; the steady temperatures are solved first on the same mesh, then the part expands by α·(T − the **stress-free temperature**).
- **Temperatures over time (transient run)**: add thermal conditions, then give a **Start temperature**, **Time span**, **Time steps** and **Time stepping** as for a transient thermal study. The temperatures are solved over time on the same mesh (cubes or tetrahedra), and a static solve on each moment's temperatures gives that moment's stress. **Stress at**:
  - **The worst time** (the start): **Stress solves** moments spread evenly over the run, the start included (21 unless you change it, at most 51). The results open at the moment with the largest von Mises stress; the table names it, a small chart shows the peak stress against time, and the **Time** slider shows the stress at every moment solved (the colour scale stays that of the worst moment).
  - **One chosen time**: one moment, the time step nearest the **Time (s)** you give. A buckling study always takes one chosen time (the end of the run unless you set one).

A part that is quenched (dipped from hot into cold) or suddenly heated is stressed most while the inside and the surface differ most: often right at the start. A held temperature applies from the first instant, as a sudden change, so the stress at the very start depends on the element size (only a few elements see the change). Read it as "very high", and look at the slider's next moments for values the mesh resolves. The stress is quasi-static: no inertia, no vibration from the sudden change.

A part that is free to grow has no thermal stress: hold it just enough to stop it moving (for example three frictionless faces at right angles). Held between two walls it is squeezed: a steel bar warmed 50 K between rigid walls carries about 117 MPa. Thermal loads balance themselves, so the reactions still balance only the mechanical loads.

## Buckling: when does a slender part give way?

1. Choose **Buckling (linear)** as the **Type**, and add supports and loads as for a static study.
2. Set **Load factors to find** (how many buckling modes).
3. Press **Solve**. Each mode has a **load factor**: the multiple of your loads at which the ideal part buckles. A load factor of 3 means three times your loads; **a load factor below 1 means it buckles under the loads you applied**. The table also gives the total load at each factor. The mode shape shows how it buckles (its size means nothing).

**Linear buckling overestimates the real buckling load.** It assumes a perfect part: perfectly straight, with no residual stress, and elastic. Real parts have small imperfections and can yield first, and thin-walled shells can buckle at a fraction of the linear value. Use a generous margin, and treat a load factor near or below 3 for a thin shell as a warning.

## How far to trust it

PartCraft checks these solvers against hand formulas: a bar between two temperatures (exact), a cooling fin (within 0.3 K), a slab heating itself (exact), a cube cooling over time (within 0.1 % with Crank–Nicolson), a bar suddenly heated at one end, a free bar warmed (no stress, ΔL = αLΔT), a bar held between walls (σ = −EαΔT), a quenched plate over time against the classic series solution (within 1 % on cubes, 2.1 % on tetrahedra at the plate's centre; 0.5 % at its surface), a long run's last moment against the steady thermal stress (the same to a millionth), and Euler columns (within 1 % fixed–free, 1.5 % pinned–pinned). Keep in mind:

- **Constant material values.** Conductivity, specific heat and expansion do not change with temperature here; at a few hundred degrees they can change by 10–30 %. Stiffness does not change either.
- **Stress over time is quasi-static and elastic**: each moment is a static solve (no inertia), and no yielding, so a quench that would yield the surface shows a stress above yield instead.
- **Convection coefficients are your input**, not computed from an air or water flow.
- **No contact resistance**: the part is one solid.
- **Linear buckling only**: no imperfections, no yielding, no behaviour after buckling.
- The cube staircase and sharp inside corners affect thermal stresses the same way as other stresses (see [Check stress and deflection](topic:simulation)).
- This is a lab build: check parts that matter another way.

## Related

- [Check stress and deflection (Simulation)](topic:simulation)
- [What is not built yet](topic:not-built-yet)
