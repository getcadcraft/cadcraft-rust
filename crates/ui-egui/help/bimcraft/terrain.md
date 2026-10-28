---
title: The site's ground, the building pad and paving
summary: Bring the ground into the model from survey points, contours, a LAS / LAZ scan, a GeoTIFF or a few spot elevations; cut the house's building pad in with its slopes and cut / fill; drape driveways, walks and patios on it; hand it to CivilCraft as LandXML.
section: tasks
order: 25
keywords: terrain, ground, site, topography, survey, points, PNEZD, contours, LAS, LAZ, LiDAR, GeoTIFF, DEM, elevation, spot elevation, grade, grading, pad, building pad, cut, fill, earthwork, daylight, slope, driveway, walk, patio, paving, LandXML, CivilCraft
---

## What you'll do

Put the house on its site. The ground becomes part of the model: shown in 3D, cut in sections and elevations, with contours on the plan. A building pad is cut into it under the house, driveways and walks follow it, and footings step down with it.

## Bring the ground in

1. Open {{ui:Build}} and choose {{ui:Terrain}}.
2. Either:
   - **Open a survey file…**: survey points (PNEZD, PENZD or plain easting, northing, elevation; comma, tab or space separated), contours in a DXF or DWG (polylines at their elevation, 3D polylines), a LAS or LAZ scan (ground points kept, thinned to about one per square meter), or a GeoTIFF or ASCII elevation grid. Choose the file's units first.
   - Or type a **Spot elevation** (from this level's floor, so `-1'` is a foot below it) and click spots on the plan, change the elevation and click more, then **Make the ground**. Three spots are enough to start.

Survey coordinates far from the model (state plane, UTM) are moved onto the house, and the elevations are set so the ground at the house is about 8" under the first floor. **Raise the ground** in **Properties** corrects that by any amount.

## The building pad

1. Choose {{ui:Building pad}}.
2. Set the **Margin** around the house, how far the **pad sits below the floor** (8" under a slab, more under a crawlspace) and the **Slopes** out to the existing ground (`2:1` = two feet out for each foot up or down).
3. Point inside the house and click.

The ground is flattened under the house and out to the margin, then slopes up into the hillside (cut) and down to the low side (fill) until it meets the existing ground. **Properties** of the ground and of the pad give the cut and fill volumes. Move the walls and the pad follows.

## Driveways, walks and patios

Choose {{ui:Paving}}, pick **Walk**, **Driveway** or **Patio**, and click the corners. Click the first corner again, double-click or press {{key:Enter}} to finish. The paving follows the finished grade; change its kind or thickness in **Properties**.

## Footings step to the grade

With the ground in the model, stem walls and crawlspace footings follow it: along each wall the footing stays its depth under the ground outside, stepping in whole 8" steps where the ground falls (see [Foundations under the walls](topic:foundations)).

## Hand it to CivilCraft

**LandXML for CivilCraft** (in the Terrain row, or the ground's **Properties**) downloads the existing ground and the finished grade as a LandXML file. Open it in CivilCraft for real grading, drainage and earthwork.

## Good to know

- The ground needs an IFC4 model, like every new building.
- Contours show on the plan of the ground level at the interval set in the ground's **Properties**; every fifth is heavier.
- The ground is saved as an IFC terrain element (a closed triangulated surface, so sections cut it); the pad and paving are saved with it. Delete the ground to start again — its pad and paving go with it.
