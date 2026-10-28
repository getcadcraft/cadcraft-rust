---
title: Framing walls, floors and roofs
summary: Generate studs, plates, headers, joists, rafters or trusses from the model and keep them with it; they are made again when the walls, openings, floors or roof change, and the Lumber schedule is the cut list.
section: tasks
order: 24
keywords: framing, frame, stud, studs, plate, top plate, header, king stud, jack stud, trimmer, cripple, sill, blocking, corner, California corner, tee, joist, rim joist, band joist, trimmer, rafter, hip rafter, valley rafter, jack rafter, ridge board, ceiling joist, collar tie, truss, lumber, cut list, board feet, 2x4, 2x6, spacing, on center
---

## What you'll do

Frame the building from what you drew. BimCraft lays out the studs, plates and headers in the walls, the joists in the floors and the rafters (or trusses) in the roof, sizes them from labelled default tables, keeps them with the model and makes them again whenever what they frame changes. The **Lumber** schedule is the cut list.

## Frame it

1. On a level, open {{ui:Build}} and choose {{ui:Framing}}.
2. Choose the **spacing** (16" or 24" on center; 400 or 600 mm), the **corners** (three-stud or California), **Rafters** or **Trusses** for roofs, and whether walls get a row of **blocking** at mid-height.
3. Click **Frame every wall**, or click one wall, a floor, or a roof (on the level the roof sits on) to frame just that.

Turn **Show framing** off to hide every framing piece, like a layer; **See through** hides the walls, floors and roofs that are framed so you can look at the framing alone.

## What you get

- **Walls**: a bottom plate (cut at doors), a double top plate, studs on the layout from the wall's start, king and jack studs and a header at every door and window, cripples above headers and below windows, a rough sill under windows, corners where walls meet (the wall that runs through takes the corner studs), two backer studs where another wall tees in, and blocking between studs.
- **Floors**: joists across the short way of the floor, rim joists around the edge, and doubled trimmers and headers around a stairwell, with the joists through it cut short (tail joists).
- **Roofs**: common rafters square to each eave, jack rafters where hips and valleys cut them, hip and valley rafters, ridge boards, ceiling joists on the plates and collar ties — or simple trusses at the spacing, one piece each.

## Change it

Pick any framing piece in the 3D view or the model tree. **Properties** shows its framing: change the spacing, the corners, the blocking or the roof framing. Moving walls, doors and windows, or changing the floor or the roof, makes the framing again by itself.

To keep a piece you placed on purpose when the framing is made again, pick it and choose **Keep this piece when the framing is made again**. A kept piece stays where it is; when the framing round it changes it is marked so you can check it.

## The cut list

In the Schedules panel choose **Lumber**: every framing piece grouped by size and by the stock length you'd buy (whole 2' lengths from 8'; 600 mm steps from 2400 in millimetres), with the number of pieces and the board feet. **CSV** downloads it.

:::warning
Header, joist, rafter and ceiling joist sizes come from labelled default tables for a typical one- or two-story house, not engineering. **Properties** says when a span or an opening is beyond its table. A designer or engineer checks spans, loads and connections.
:::

## What's saved

Each framed wall, floor or roof is an IFC element assembly ("Framing") of IFC members (studs, plates, rafters, collar ties, blocking, trusses) and beams (headers, joists, rims, ridge boards), each with its size, length and board feet.
