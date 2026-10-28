---
title: Check an assembly for clashes and motion
summary: Find parts that overlap with Clash, drag any joint's values to watch the motion, and sweep a joint through its range to find where parts collide.
section: tasks
order: 120
keywords: interference, interference detection, clash, collision, overlap, thread fit, thread depth mismatch, touching faces, hide, filter, motion, motion study, sweep, range of motion, check
---

Two checks run on the parts as they are placed: **Clash** looks for overlapping material right now, and the **motion check** moves one joint through its range and looks for overlaps along the way.

## Check for interference

1. Open or build an assembly (see [Build an assembly](topic:build-assembly)).
2. Choose {{ui:Clash}}.

The status bar reports the result:

- "No interference between 3 parts (40 ms)." Nothing overlaps.
- "Interference: Flange1 and Flange2: Overlap, 2.5 mm³." Each pair, what kind of result it is and the volume they share.

The **Assembly** box on the left shows **Interference**: "None", "2 results" (the ones shown), or "Not checked". With nothing selected, the right panel lists each result under **Interference**.

The result is cleared whenever the assembly changes. Run {{ui:Clash}} again after moving a joint. Every pair of parts is checked against every other, so large assemblies take longer.

### Kinds of results

| Kind | Means |
|---|---|
| **Overlap** | The parts share volume: a real clash. |
| **Thread depth mismatch** | The shared volume is inside one part's thread but beyond the other's: a bolt threaded deeper than its hole, or a plain shank in a tapped hole. |
| **Thread fit (cosmetic threads)** | The shared volume is inside both parts' threads: a bolt in a tapped hole. Cosmetic threads are drawn at the thread's full size, so they always overlap a little. |
| **Touching faces** | No shared volume, but faces in contact (within 0.001 mm). |

Modelled threads (see [Threads on shafts and bores, drawn or modelled](topic:threads)) are real geometry: what they share is an **Overlap**.

### Hide what you do not need to see

Under **Interference**, **Show** has a box for each kind: clear **Thread fit** and **Touching faces** to see only the clashes that matter. **Hide this pair** beside a result hides that pair of parts whatever kind it is. Hidden results are counted ("2 results hidden by these filters"), and **Show the hidden pairs again** brings the pairs back. The status bar counts hidden results too.

The filters are saved with the assembly, and each change is one {{key:Mod+Z}} step. **Sweep a joint for clashes** (below) uses them too. An assembly with filters is saved as version 4, which earlier PartCraft versions refuse to open instead of losing the filters.

## Watch the motion

1. Select a joint that moves (any but rigid) in the list on the left.
2. Drag the slider under one of its values, such as **Angle (°)** or **Offset (mm)**. The part moves as you drag.
3. Let go. The new position is kept and can be undone with {{key:Mod+Z}}.

You can also type an angle or offset in the box and press {{key:Enter}}.

## Sweep a joint for clashes

1. Select a joint that moves.
2. For a joint with several values (cylindrical, planar, ball, pin in slot), choose which one to sweep in **Check moves**.
3. Choose {{ui:Check the motion for clashes}}.

PartCraft moves that value through 25 positions in 24 equal steps, the others staying as they are: an angle from −180° to 180° (a tilt from −90° to 90°), a length from minus to plus the size of the biggest part. It reports:

- "No clash anywhere for Angle (°) from -180 to 180 (25 positions checked)."
- "6 of 25 positions clash, first at Angle (°) 45 (12.4 mm³)." The first position that clashes and the overlap there.

The button is grayed for rigid joints, which do not move. The sweep checks the positions only, not the path between them: a thin part could pass through another between two steps.

:::note
Contact and gravity are not built. Without limits the sweep covers the whole range; give the value **Limits** (see [Parts list, exploded views and moving parts](topic:assembly-bom-explode)) and the sweep covers only those, so a hinge that opens 90° is checked over 90°.
:::
