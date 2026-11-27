# Sites

A site is a place: a position, a ground height and a name. Antennas and radios are not part of a site, so you can try any antenna at any site.

## Adding a site

- Press **New site**, then click the map where it is. Or right-click the map to put a new site straight there.
- Or type the position in **Location** and press **Go**. Coverant understands:
  - decimal lat/long: `52.2053, 0.1218`
  - degrees, minutes and seconds: `52°12'19"N 0°07'18"E`
  - Maidenhead locators of 4 to 10 characters: `JO02bf`
  - OS grid references: `TL 450 585`, `TL4500058500`, or eastings,northings
- Or press **Import…** to read sites from a GPX, KML or GeoJSON file. See **Backup, import and export**.
- Drag the selected site's pin to fine-tune it. **Pick on map** lets you click a new position.

The panel shows the position in every format and the ground height from the terrain data. Tick **Set ground height myself** if you know better, for example on a roof or a tower.

## Terrain download

With **Download terrain when saved** ticked, pressing Save fetches the terrain out to the distance you set, so plots work offline later. The status bar shows the progress. That distance is also the default radius of this site's coverage plots.

## Deleting a site

**Delete site** asks first. It also deletes the site's coverage plots and links, and removes cached terrain that no other site uses.

Coverage overlays are hidden while you are on this tab, so the map stays readable.
