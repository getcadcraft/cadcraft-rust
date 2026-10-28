---
title: Compare two versions of a model
summary: Open two versions of the same IFC model and see which elements were added, removed and changed, matched by their GlobalIds, with a CSV of every change.
section: tasks
order: 34
keywords: compare, diff, difference, versions, revision, changes, changed, added, removed, moved, globalid, guid, what changed
---

## What you'll do

See what changed between the model you had and the one you were sent: which elements are new, which are gone, which moved or got new property values.

## Steps

1. Open the newer version, then click {{ui:Add a model…}} and choose the older one. See [See several IFC models together](topic:several-models).
2. In {{ui:Models}}, open {{ui:Compare versions}}.
3. Choose the {{ui:Older version}} and the {{ui:Newer version}}.
4. Click {{ui:Compare}}.
5. Click an element in a list to pick it and see its properties. Click {{ui:Download CSV}} for every change, property by property.
6. Click {{ui:Clear}} to go back to the normal view.

## What you should see

- **Added** elements in blue, from the newer version.
- **Removed** elements in red, from the older version.
- **Changed** elements in ochre, from the newer version, each saying what changed: properties, a move with how far, or a new shape.
- Everything that is the same, see-through.

## Good to know

- Elements are matched by their GlobalId. A program that gives elements new GlobalIds on every export makes every element look removed and added again.
- Positions are compared where the models sit in the scene. If one version was moved on purpose, line them up first with {{ui:Move}} in {{ui:Models}}.
- A GlobalId used twice in one model is left out of the comparison. [Check a model for common problems](topic:model-checks) finds those.

## Next

- [Issues and BCF files](topic:issues-bcf)
- [Add up quantities by property](topic:quantity-totals)
