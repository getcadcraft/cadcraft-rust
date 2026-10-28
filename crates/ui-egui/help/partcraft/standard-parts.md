---
title: Open a standard part
summary: Start a part from a bolt, screw, nut, washer, pin, bearing or key sized from its standard, with every size of the standard's table as a configuration.
section: tasks
order: 107
keywords: standard part, standard parts, bolt, screw, hex, nut, washer, pin, dowel, bearing, ball bearing, key, parallel key, fastener, iso 4017, iso 4014, iso 4032, iso 7089, iso 2338, iso 15, din 6885, asme, template, library
---

PartCraft opens common mechanical parts ready made: the dimensions come from the standards' tables, and every size in the table is a configuration you can switch to.

## Open one

1. Click {{ui:Standard part}} in the File group.
2. In {{ui:Part}}, choose the kind, for example {{ui:Hex head screw (ISO 4017)}}.
3. In {{ui:Size}}, choose the size, for example `M10 × 40`.
4. Click {{ui:Open}}.

The part opens as a new, unsaved part. Save it to keep it ({{key:Mod+S}}).

| Part | Standard | Parameters |
|---|---|---|
| Hex head screw, hex bolt | ISO 4017, ISO 4014, ASME B18.2.1 (inch) | `d` thread size, `s` across flats, `k` head height, `l` length |
| Hex nut | ISO 4032, ASME B18.2.2 (inch) | `d`, `s`, `height` |
| Plain washer | ISO 7089 | `d1` inside, `d2` outside, `h` thickness |
| Parallel pin | ISO 2338 | `d`, `l` |
| Deep groove ball bearing | ISO 15 | `d` bore, `D` outside, `B` width |
| Parallel key | DIN 6885-1, form A | `b` width, `h` height, `l` length |

## Change the size

Each size is a configuration (see [Configurations and design tables](topic:design-tables)): open {{ui:Parameters}} and pick another one, or change a parameter, such as `l` for a longer screw, and save it as a configuration of your own.

## What is modelled

- Screws and bolts: the hexagon head and a plain shank. Nuts: the hexagon with a hole of the thread's nominal size. Threads are not modelled; add one with {{ui:Thread}} if you need it (see [Threads](topic:threads)).
- Pins: a plain cylinder; the end chamfers are left out.
- Bearings: the outside envelope only (a ring of the bore, the outside diameter and the width), which is what you need to place one.
- Keys: the round-ended key.

The part's material is steel, so the mass panel shows its weight. Gears are not here yet: PartCraft has no tooth feature, and a disc is not a gear. CadCraft draws gears by the drawing convention, and the library has them as drawings.
