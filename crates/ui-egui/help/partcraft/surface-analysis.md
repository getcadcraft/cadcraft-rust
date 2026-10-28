---
title: Zebra stripes and curvature
summary: Check how smoothly the faces of any part join, with zebra stripes that follow the view or a colour map of mean curvature, from the Views menu.
section: tasks
order: 97
keywords: zebra, zebra stripes, reflection lines, curvature, curvature map, mean curvature, surface quality, continuity, tangent, g1, g2, smooth, crease, highlight lines
---

Two quick looks at how smooth a part's faces are, on any part in the Design workspace (the Form workspace has its own, for the form you are editing: see [Freeform shapes with Form](topic:form-surfaces)).

## Zebra stripes

Open {{ui:Views}} and choose {{ui:Zebra stripes}}. The part shows the reflection of a striped light, as a polished surface would:

- Where two faces join **tangentially** (a fillet on a flat, a smooth Gordon surface), the stripes run on across the join with a bend.
- Where they meet at a **crease**, the stripes break and jump.
- Where the curvature also matches (G2), the stripes flow on without even a kink.

Turn the view (drag, or a view-cube face): the stripes are drawn again from the new direction. **Stripes** sets how many there are.

## Curvature

Choose {{ui:Curvature}} in the same menu. The part is coloured by its **mean curvature**: blue where it is hollow (concave), pale where it is flat, red where it bulges (convex). The card says at what curvature the colours reach full strength, in 1/mm and as a radius. Sharp edges count as very curved, so they show strongly; look at the faces between them.

## The card

Both open the analysis card at the bottom left of the view, which also holds the [draft analysis](topic:draft-analysis). Switch between them with **Show**. Choosing the open one again in {{ui:Views}}, {{ui:Close}} or {{key:Esc}} turns it off. The colours follow the part as you change it; nothing is saved in the part.
