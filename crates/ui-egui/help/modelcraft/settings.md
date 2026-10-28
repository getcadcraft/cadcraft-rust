---
title: ModelCraft settings
summary: Pick the key style you know, make a trackpad orbit and pan without the camera buttons, and turn snapping and solid collisions on or off.
section: reference
order: 5
keywords: settings, preferences, controls, key style, keymap, shortcuts, blender, trackpad, touchpad, scroll, orbit, pinch, snap, snapping, collide, collision, overlap, intersect, pass through
---

Open {{ui:Settings}} with the gear in the app bar or {{key:Mod+,}}. The first tab, {{ui:ModelCraft}}, holds the settings that belong to this app; {{ui:General}} holds the ones every CadCraft app shares (theme, toolbar, panels). ModelCraft's settings exist only in the app: you change them here, and this browser remembers them.

## Controls: which keys and mouse buttons

Two key styles. Switch them here, with {{ui:Keys: Push-pull}} / {{ui:Keys: Modeler}} at the bottom right of the view, or at the end of the toolbar's {{ui:More}} menu:

- **Push-pull style** (the start; the letters of push-pull modelers such as SketchUp): a letter picks a tool — {{key:L}} line, {{key:R}} rectangle, {{key:P}} push/pull, {{key:M}} move, {{key:Q}} rotate, {{key:S}} scale, {{key:G}} make component. See [Keys and mouse actions](topic:keyboard).
- **Modeler style** (the keys of mesh modelers such as Blender): select first, then act on the selection straight away. {{key:G}} moves it, {{key:R}} rotates it and {{key:S}} scales it from where the pointer is — no first click. While it moves, {{key:X}}, {{key:Y}} or {{key:Z}} locks the red, green or blue axis, a typed number sets the distance (or degrees, or factor), and a click or {{key:Enter}} puts it down; {{key:Esc}} or a right-click puts it back. Then you are back in {{ui:Select}}.

The key style is the same choice as the menu style: the Modeler key style is the {{ui:Modeler}} menu style, and picking one picks the other. See [Menus, keys and mouse](topic:menus-keys-and-mouse). The full Modeler list is in [Keys and mouse actions](topic:keyboard#modeler-keys). The tooltips on the toolbar and the start card follow the style you pick.

## Scrolling: mouse wheel or trackpad

- **Work it out** (the start): ModelCraft watches the scrolling. Sideways or fine-grained scrolling, as a trackpad sends, orbits; whole wheel notches zoom.
- **Mouse wheel**: scrolling always zooms toward the pointer.
- **Trackpad**: swipe with two fingers to orbit, hold {{key:Shift}} and swipe to pan, pinch to zoom. You never need the {{ui:Orbit}}, {{ui:Pan}} or {{ui:Zoom}} buttons.

A pinch zooms in every choice. If "Work it out" guesses wrong for your mouse or trackpad, pick the one you have.

:::tip
In the Modeler style you can also orbit by holding {{key:Alt}} and dragging, and pan with {{key:Alt+Shift}} and a drag — handy on a laptop without a middle button.
:::

## Snapping

**Snap to points, edges and axes** (on at the start) pulls the pointer to endpoints, midpoints, edges, guides and the red, green and blue axes — see [Inference](topic:inference). Turn it off when you want to place something freely near other geometry without it jumping. Faces, the ground and the axis locks still work with snapping off.

The {{ui:Snap}} button at the bottom right of the view does the same for this session's work (it is saved too). In the Modeler style, {{key:Shift+Tab}} switches it.

## Solids stop where they touch

**Stop solids where they touch** (on at the start) keeps a wall from being dragged through another wall. With {{ui:Move}} (and {{key:G}}, and paste-then-move with {{key:Mod+D}}) the selection slides up to the other solid and stops touching it; with {{ui:Push/pull}} a face stops where it meets another solid. Resting on something and sliding along it are fine — only passing into the inside is stopped. Things that already overlap when you start can still be pulled apart.

To push something through on purpose, click {{ui:Collide}} at the bottom right (or clear the setting), make the move, and click it again.

:::note
Collisions are checked for moves and push/pull. {{ui:Rotate}} and {{ui:Scale}} do not check them yet, and loose geometry that touches still joins up, as it always has: keep separate things in [groups](topic:groups-components) so they stay separate.
:::

## Corner indicator

What shows in the corner of the 3D view: **View cube** (the start; top right — click a face, edge or corner to look from there, drag it to orbit, drag its compass ring to turn the model), **Axis icon** (a small X, Y, Z marker at the bottom left that turns with the view: X red, Y green, Z blue) or **None**. See [The view cube](topic:view-cube).
