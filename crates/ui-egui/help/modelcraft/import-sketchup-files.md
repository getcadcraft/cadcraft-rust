---
title: Bring in an SKP model
summary: Import a .skp file with its groups, components, colors, textures, tags and soft edges, see what came in and what did not, and carry on modeling.
section: tasks
order: 101
keywords: skp, import, open skp, components, groups, tags, layers, materials, colors, textures, hidden, soft edges, 3d warehouse, migrate, switching
---

ModelCraft reads SketchUp files (`.skp`) with its own reader, in the page. Nothing is uploaded, and it works offline whenever ModelCraft does. Files from SketchUp 3 up to 2026 have been tested.

## Import the file

1. Click {{ui:Import}} in the {{ui:File}} group (or drop the `.skp` file on the 3D view).
2. Pick the file.

   The status bar says "Reading house.skp…" while it reads, then what came in, for example "Imported house.skp (SketchUp 25.0.575): 1240 faces, 6 components, in a group."

The whole model arrives as one group named after the file, selected, and the view zooms to it. One {{key:Mod+Z}} takes it all away again, tags included.

:::tip
Explode the import group ({{ui:Entity info}} › {{ui:Explode}}) when you want its contents loose in your model rather than in one group.
:::

## What comes in

- **Faces** with their holes, and **edges** that are not part of a face.
- **Groups** come in as groups, and **components** as components: every copy of a component is a placement of one definition, so editing one copy changes them all, as in SketchUp. Names come with them.
- **Colors.** A face's own material gives it its color. A material painted on a group or component becomes that placement's paint, so faces inside it with no color of their own take it, as in SketchUp. A component painted red in one place and blue in another stays one component.
- **Textures.** A face painted with a textured material comes in with its texture, laid on the face as in the file, and the {{ui:Shaded}} style shows it. The image is kept in the ModelCraft file when you save, and a [rendered picture](topic:render) shows it too. A textured material painted on a whole group or component gives that copy the material's color only.
- **Tags.** A group or component on a SketchUp tag gets a ModelCraft tag of the same name, listed in {{ui:Tags}}, shown or hidden as it was in SketchUp. If your model already has a tag with that name, that tag is used.
- **Hidden things come in hidden:** faces, edges, groups and components, and faces and edges on a hidden tag. You see what SketchUp showed, and nothing is lost: {{ui:Edit}} › {{ui:More}} › {{ui:Unhide all}} shows them. See [Hide, isolate and unhide](topic:hide-isolate).
- **Soft and smooth edges** stay soft, so curved surfaces still look smooth.
- **Sizes** come out right in any unit: SketchUp files are in inches inside, and ModelCraft converts them.

## What does not come in

The status bar says what was left out, with counts.

- **Some textures** keep only their material's color: a texture missing from the file, an image in a format browsers can't show (such as a Photoshop file), or a texture without a size. Faces with a projected texture come in with the texture placed approximately.
- **Tags on loose faces and edges.** ModelCraft tags groups and components only, so a face or edge with its own tag comes in untagged, and the status bar counts them. Make such faces into a group ({{key:Shift+G}}) and tag the group.
- **Images** placed in the model, text, dimensions, guides, section planes, scenes and styles.

## Problems

- "This is not a SketchUp file." or "This SketchUp file is damaged: …": the file is cut off or is something else with a `.skp` name. Save it again from SketchUp, or ask for a fresh copy.
- A file from a SketchUp version newer than ModelCraft knows is refused with the version it found.
- A model that is all images, text or dimensions says it has nothing to show.

## Take the model back

ModelCraft cannot save `.skp` files. Export Collada ({{ui:Export}} › `.dae`) and import that in SketchUp: groups, components and colors go across; textures go as their material's color. See [Export your model](topic:export-model).
