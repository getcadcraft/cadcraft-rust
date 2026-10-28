---
title: A map on the ground under the model
summary: Lay a street map or aerial picture on the ground under a georeferenced IFC model, from map tiles, map services or a picture with a world file.
section: tasks
order: 28
keywords: map, map layers, basemap, aerial, satellite, site, ground, tiles, xyz, wms, wmts, map service, world file, georeference, ifcmapconversion, projected crs, epsg, site context
---

## What you'll do

See the model in its place: the site's street map or aerial picture on the ground under it.

## Steps

1. Open an IFC model that carries its place on the map (a map conversion, IfcMapConversion, with a projected coordinate system). Exporters write it when the project's coordinates (survey point, shared coordinates) are set.
2. Choose {{ui:Show}} › {{ui:Map layers…}}. The dialog says the model's system, for example EPSG:28356. A model without a map conversion cannot have map layers: the dialog says so.
3. Turn on {{ui:Map layers from the internet}}. Until then nothing is asked of any server; when it is on, the servers you add see the area round the model and your internet address, never your model.
4. In {{ui:Add a layer}} choose tiles (an address with `{z}`, `{x}` and `{y}`), a WMS, WMTS or ArcGIS service ({{ui:Get layers}}, then choose one), or a picture with its world file.
5. Close the dialog. The map lies on the ground just under the model's lowest point, over the model's footprint and as far again round it.

:::note
The map is on screen only; it is not saved in the IFC file. The list of layers is kept in this browser. Use only map services whose terms allow it.
:::
