---
title: NC file fields
summary: What DetailCraft writes in each DSTV NC file, field by field, and how faces and coordinates are measured.
section: reference
order: 40
keywords: nc1, dstv, nc file, st block, ak, bo, header, faces, coordinates, cnc, format
---

Each `.nc1` file describes one part mark. Lines end with CR LF. Text fields carry plain ASCII; other characters become "?". The second line is a comment, `** DetailCraft <mark>.nc1`.

## Header (ST)

| Field | I-shape, channel | Angle | Rectangular HSS | Round HSS | Plate |
|---|---|---|---|---|---|
| Order | The model name | ← | ← | ← | ← |
| Drawing, piece | Part mark | ← | ← | ← | ← |
| Phase | The part's phase, or 1 | ← | ← | ← | ← |
| Steel quality | The material grade | ← | ← | ← | ← |
| Quantity | Number of parts with this mark | ← | ← | ← | ← |
| Profile | The profile name, for example W16x26 | ← | ← | ← | `PL<t>*<width>` |
| Profile code | I or U (channel) | L | M | RO | B |
| Length | Cut length | ← | ← | ← | Length |
| Height | Depth d | Leg | Height | Diameter | Plate width |
| Flange width | bf | Leg | Width | Diameter | 0 |
| Flange thickness | tf | t | Wall t | Wall t | 0 |
| Web thickness | tw | t | Wall t | Wall t | Plate thickness |
| Radius | Fillet radius (0 for AISC channels) | 0 | Outside corner radius | 0 | 0 |
| Weight per meter | kg/m | kg/m | kg/m | kg/m | kg/m² |
| Paint surface per meter | Outside perimeter, m²/m | ← | ← | ← | 2 (both faces, m²/m²) |
| Saw angles (4 fields) | 0 (square) | ← | ← | ← | ← |
| Text fields (4) | `-` | ← | ← | ← | ← |

Plates follow the standard's column "for the sheets". The standard has no column for round tubes: DetailCraft writes the diameter as both height and flange width and the wall as both thicknesses.

## Outer contours (AK)

- **I-shapes and channels:** the web (v), with a rectangular notch for each cope; each flange (o and u), shortened at an end where a cope is at least as deep as the flange.
- **Rectangular HSS:** both side walls (v and h), notched for copes, and the top and bottom walls (o and u).
- **Angles:** one rectangle per leg (v and u). Copes on angles are not written: such a part is listed as not written. An angle brace bolted to a [gusset](topic:brace-gussets) has its holes on the leg that is bolted: v for the leg along the part's u, u for the leg along its v.
- **Round HSS:** one rectangle on v, the tube seen from the front (length × diameter).
- **Plates:** one rectangle on face v.

Each contour is closed (the first point is repeated at the end) and runs counter-clockwise.

## Holes (BO)

One line per hole: face, x, y, diameter, depth 0 (through). All holes are included, the ones you typed and the ones connections made. A hole through the side walls of a rectangular HSS is written twice, once on v and once on h.

## Faces and coordinates

Each profile is placed as the standard's figure of profile positions draws it (DSTV 1998, p. 7): x along the part, Y across, Z up from the behind face, with the zero point at the outside corner. DetailCraft never mirrors a part to fit the figure.

| Face | I-shape | Channel | Angle | Rectangular HSS | Plate |
|---|---|---|---|---|---|
| v | Web, front | Web, seen from the toes' side | The leg along the part's u | Side wall at −u | The plate face |
| h | | | | Side wall at +u | |
| o | Top flange | **Bottom** flange | | Top wall | |
| u | Bottom flange | **Top** flange | The leg along the part's v | Bottom wall | |

- **x** runs along the part from its cut start.
- **I-shapes and rectangular HSS:** y on the web or side walls from the underside of the profile; y on a flange or the top and bottom walls from its edge on the behind side (the part's +u side).
- **Channels** lie on their back with the toes up, as in the standard's figure. So y on the web runs from the top flange's outside down, the top flange is face u and the bottom flange face o, and y on a flange runs from the back of the web toward the toe.
- **Angles:** y on each leg from the heel.
- **Plates:** y from the plate's −u edge.

Check how your machine software reads channel faces before cutting: some programs turn channels the other way up.

## Not written

Bevels, inner contours, markings (SI), saw angles other than square, slotted holes, holes in round tubes (DetailCraft has none), and copes on angles and round tubes.
