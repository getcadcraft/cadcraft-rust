---
title: Map layers under the drawing
summary: Show street maps, aerial pictures or a picture with a world file under the model space, placed by the drawing's coordinate system.
section: tasks
order: 89
keywords: map, map layers, maplayers, ml, geomap, basemap, aerial, satellite, tiles, xyz, wms, wmts, map service, world file, coordinate system, epsg, state plane, utm, georeference, site plan, geolocation
---

## What you will make

A site plan drawn over an aerial picture or a street map, each in the right place.

## Tell CadCraft where the drawing is

1. Set the drawing's units in {{ui:Properties}} › {{ui:Units}} (for example feet or meters).
2. Type {{cmd:MAPLAYERS}} (or `ML`) and press {{key:Enter}}.
3. Under {{ui:Where the drawing is}}, type a name or an EPSG code in {{ui:Find a system}} (for example `2277` or `Texas Central`) and choose it in {{ui:System}}.
4. If the drawing is drawn in that system's own coordinates, leave the placement at 0. If not, give one drawing point ({{ui:Drawing X}}, {{ui:Drawing Y}}), where that point is on the map grid ({{ui:Easting}}, {{ui:Northing}}), and how far the drawing is turned from east ({{ui:Turned (°)}}).
5. Click {{ui:Use this system}}.

## Add a layer

1. Turn on {{ui:Map layers from the internet}} at the top of the dialog. Until then nothing is asked of any server; when it is on, the servers you add see the area on screen and your internet address, never your drawing.
2. In {{ui:Add a layer}}, choose what it comes from:
   - **Tiles (XYZ)**: a tile address with `{z}`, `{x}` and `{y}`, a name and the credit its provider asks for.
   - **WMS**, **WMTS** or **ArcGIS**: the service's address, then {{ui:Get layers}} and choose one.
   - **A picture**: a PNG or JPEG with its world file (and its `.prj` when there is one).
3. The layer shows under the drawing. Its credit shows at the bottom right of the drawing.

Each layer in the list can be hidden, made more or less see-through, moved up or down, or removed.

:::note
Map layers are drawn on screen only: they are not saved in the DWG or DXF and they do not plot. The system and the list of layers are kept in this browser; pictures from files last until the page closes. Use only map services whose terms allow it, and keep their credit visible.
:::
