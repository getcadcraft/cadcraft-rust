---
title: Find elements by class, name or property
summary: Find every element that matches one or more rules — its class, name, type, storey, material or any property value — pick them all, and keep the rules as a named search set.
section: tasks
order: 22
keywords: find, search, filter, query, rule, search set, selection set, saved search, property, value, class, material, pick all
---

## What you'll do

Pick every element of one kind or with one value in a single step: every door, every wall on Level 2, every element whose fire rating is EI60, every wall that costs more than a set amount.

## Steps

1. Open an IFC model.
2. Open {{ui:Find}} in the left panel.
3. In the first rule, choose a property (the element's own ones first, such as {{ui:Class}} and {{ui:Storey}}, then the model's property sets), how to compare, and a value. The value box suggests the values the model has.
4. To narrow it down, click {{ui:Add a rule}}. Choose {{ui:Every rule}} to need all of them, or {{ui:Any rule}} for one of them.
5. Click {{ui:Find}}, or press {{key:Enter}} in a value box. What matches is picked.
6. To keep the rules, click {{ui:Save as a set}}, type a name and click it again. Click a set's name later to find with it.

## What you should see

- The matches picked in the 3D view and the model tree, and {{ui:Properties}} saying how many by class. Press {{key:I}} to show only them, {{key:H}} to hide them.
- Words match without caring about capitals. Numbers compare as numbers: "is more than 1000" means the value, not the text.
- {{ui:has a value}} and {{ui:has no value}} need no value; they find what is filled in, or missing.
- Saved sets stay in this browser for every model you open.

## Common mistakes

- **Nothing is found.** A rule needs a value it can match: look at the suggestions in the value box.
- **A property is missing from the list.** No element with a shape in this model has it.

## Next

- [Color elements by a property](topic:color-by-property)
- [Hide, isolate and see through elements](topic:hide-and-isolate)
