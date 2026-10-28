---
title: PartCraft settings
summary: Choose which mouse button turns the view, make a trackpad swipe turn and pan the part, turn automatic relations while sketching on or off, and set how many threads the simulation solvers use.
section: reference
order: 15
keywords: settings, preferences, options, mouse, middle button, wheel button, orbit, rotate, turn, trackpad, touchpad, scroll, swipe, pinch, relations, constraints, auto constrain, automatic relations, snap, coincident, threads, solver threads, cores, processors, multi-threaded, faster solve, parallel
---

Open {{ui:Settings}} with the gear in the app bar or {{key:Mod+,}}. The first tab, {{ui:PartCraft}}, holds the settings that belong to this app; {{ui:General}} holds the ones every CadCraft app shares (theme, toolbar, panels). PartCraft's settings exist only in the app: you change them here, and this browser remembers them. {{ui:Reset PartCraft settings}} puts everything back to the start.

## Turning the view

- **Left button** (the start): drag with the left button to turn the part; drag with the right or middle button to pan.
- **Middle button**: press the wheel and drag to turn the part; hold {{key:Shift}} (or {{key:Ctrl}}, or ⌘) with the middle button, or drag with the right button, to pan. The left button only picks faces, edges and planes, so a click that moves a little never turns the part. Many CAD programs work this way.

The wheel zooms at the pointer either way. In a sketch, the right and middle buttons pan as before.

## Scrolling: mouse wheel or trackpad

- **Work it out** (the start): PartCraft watches the scrolling. Sideways or fine-grained scrolling, as a trackpad sends, turns the part; whole wheel notches zoom.
- **Mouse wheel**: scrolling always zooms toward the pointer.
- **Trackpad**: swipe with two fingers to turn the part, hold {{key:Shift}} and swipe to pan, pinch to zoom. In a sketch a two-finger swipe pans the sketch.

A pinch zooms in every choice. If "Work it out" guesses wrong for your mouse or trackpad, pick the one you have. More in [Trackpad, touch and small screens](topic:trackpad-and-touch).

## Sketching: add relations while drawing

**Add relations while drawing** (on at the start): a click that snaps to a point makes the new point coincident with it, a click on a curve puts the point on that curve, and a line drawn nearly level or upright becomes horizontal or vertical. See [Relations and dimensions](topic:relations-dimensions).

Turn it off when you want to place geometry near other geometry without tying it to it. The pointer still snaps to points and curves, but nothing is joined: lines drawn in a chain do not share their ends, so a drag can pull them apart. Add the relations you want yourself from the right panel. A closed profile still extrudes when its ends meet.

:::note
The setting changes only what new drawing adds. Relations already in a sketch stay, and the Fillet tool still makes its tangent arc and radius.
:::

## Corner indicator

What shows in the corner of the 3D view: **View cube** (the start; top right — click a face, edge or corner to look from there, drag it to orbit, drag its compass ring to turn the model), **Axis icon** (a small X, Y, Z marker at the bottom left that turns with the view: X red, Y green, Z blue) or **None**. Neither shows while you sketch or on the drawing sheet. See [The view cube](topic:view-cube).

## Solver threads

How many threads the simulation solvers may use. **Automatic** (the start) takes one fewer than the computer's logical processors (at most 16), so the page stays responsive while a study solves; or pick a number. Large voxel studies, buckling, stress over time and flow solve faster on several threads (about 2× on 4 threads for 80,000 cubes in our timings, more on an idle computer); models under about 20,000 unknowns stay on one thread, because starting the threads costs more than they save. A threaded solve gives the same answer as one thread (to rounding: the sums are added in a different order). Each study's results list **N threads** or **1 thread** with the reason under **Solver**.

Threads need the page to be **cross-origin isolated** (the browser lets workers share memory only then). The lab page is; where it is not, the setting says so ("This page is not cross-origin isolated, so solvers run on one thread whatever is chosen here.") and every solve uses one thread. Everything else works the same, and the answers are the same.

Laminar flow studies use the threads too (about 2.5× on 4 threads for 54,000 fluid cells, the same answer to the last digit). The quadratic-tetrahedron solver and heat transfer run on one thread: threading them was measured slower on our test computer (their solvers are limited by memory speed and by a step that cannot be split).

### On your own web server

If you run PartCraft from your own server, it works as served. For the solver threads, have the server send two headers with PartCraft's page:

```
Cross-Origin-Opener-Policy: same-origin
Cross-Origin-Embedder-Policy: require-corp
```

On Apache, in the folder's `.htaccess` (needs mod_headers):

```
Header set Cross-Origin-Opener-Policy "same-origin"
Header set Cross-Origin-Embedder-Policy "require-corp"
```

On nginx, in the `location` that serves PartCraft:

```
add_header Cross-Origin-Opener-Policy "same-origin" always;
add_header Cross-Origin-Embedder-Policy "require-corp" always;
```

Reload the page: Settings › Solver threads then offers **Automatic** with your computer's count. The second header makes the browser refuse anything the page loads from another site that does not allow it, which is fine for PartCraft on its own (it loads everything from its own folder).
