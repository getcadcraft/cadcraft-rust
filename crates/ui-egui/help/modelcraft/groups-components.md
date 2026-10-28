---
title: Groups and components
summary: Keep objects from sticking together, edit one copy to change them all, and use the Outliner and Components panels.
section: tasks
order: 70
keywords: group, component, make group, make component, explode, make unique, outliner, instances, copies, block, nested, edit group, place
---

Loose geometry in ModelCraft sticks together: lines that touch split each other, and faces that share an edge move together. That is what makes drawing on faces work, but it also means a box drawn against another box merges with it. **Groups** and **components** keep things apart.

- A **group** wraps some geometry so it acts as one object. Nothing outside it can stick to it.
- A **component** is a group that can be placed many times. Every copy shares the same contents: edit one and they all change. Use components for anything repeated: windows, columns, desks, trees.

## Make a group or a component

1. Select the geometry. A triple-click on a face selects the whole connected object; a window drag selects everything in the box.
2. Press {{key:Shift+G}} for a group, or {{key:G}} for a component. You can also click {{ui:Group}} or {{ui:Component}} in the toolbar's {{ui:Organize}} group, or {{ui:Make group (Shift+G)}} or {{ui:Make component (G)}} in {{ui:Entity info}}.

   The status bar says "Grouped as Group#1." or "Made component Component#1.", the new object is selected, and it appears in the {{ui:Outliner}}.

3. To name it, type a name in the {{ui:Name}} field of {{ui:Entity info}} and press {{key:Enter}} or click away.

Selecting a face brings its edges along. If nothing is selected, the status bar says "Select some geometry first (drag a window, or triple-click to select everything connected)."

You can also group groups: select several groups and press {{key:Shift+G}}. The {{ui:Outliner}} shows the nesting.

:::note
The key letters match SketchUp's defaults: {{key:G}} makes a component, {{key:Shift+G}} a group.
:::

## Edit inside a group or component

1. Double-click it (in the view or in the {{ui:Outliner}}), or select it and click {{ui:Edit}} in {{ui:Entity info}}.

   The rest of the model fades, the axes move to the group's own axes, and the {{ui:Outliner}} reads "Editing: Model › Group#1". The status bar says "Editing Group#1. Esc or double-click outside closes it."

2. Draw, push/pull, move and erase as usual. Everything you do stays inside the group.
3. Close it: press {{key:Esc}} (twice if something is selected), double-click empty space, or click {{ui:Close group}} in the {{ui:Outliner}}.

While a component is open, every copy of it changes as you work.

## Place more copies of a component

- Copy it with {{ui:Move}} and {{key:Ctrl}} (see [Move and copy](topic:move-copy)). Each copy is another placement of the same component.
- Or click {{ui:Place}} next to it in the {{ui:Components}} panel, then click where it goes. Its origin, the lowest corner of the box around what you selected when you made it, lands where you click. The tool then returns to {{ui:Select}}.

{{ui:Components}} shows how many copies are placed ("4×"), and the {{ui:Outliner}} shows the count after the name.

## Make one copy different

Select the copy and click {{ui:Make unique}} in {{ui:Entity info}} (shown when the component is placed more than once). That copy gets its own definition, named like the original with a new number. Edit it without touching the others.

## Turn a group into a component

Select the group and click {{ui:Make component}} in {{ui:Entity info}}. It gets a new name, such as Component#2, even if you had named the group; type the name again in {{ui:Name}}.

## Explode

Select a group or component and click {{ui:Explode}} (in the toolbar or in {{ui:Entity info}}). Its contents become loose geometry in the place it was, and stick to what they touch. Groups inside it stay groups.

## Solid and volume

When you select a group or component, {{ui:Entity info}} shows **Solid: Yes** if its faces close up with no gaps (every edge is shared by exactly two faces) and there are no groups nested inside it. Then it also shows the **volume**. A box, a wall with a window cut through it, and a pulled cylinder are solids; a box with its top erased is not.

## When the last copy goes

Erasing the last copy of a component also removes it from the {{ui:Components}} panel. {{key:Mod+Z}} brings it back.

## Things to know

- Copies of a **group** made with Move share their contents in this build, like component copies do: editing one changes the others. To make one independent, click {{ui:Make component}}, then {{ui:Make unique}}. See [Troubleshooting](topic:troubleshooting#editing-one-group-changed-another).
- Components do not cut openings in walls or glue to faces, and there is no library of ready-made components. You make your own, or import them.
- Imported files arrive as a group. Collada files keep their own groups and components inside it.
