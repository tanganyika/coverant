# Maps, terrain and your data

## Maps

The **Map** menu chooses OpenStreetMap, OpenTopoMap or Esri satellite imagery. **Grey map** makes the coverage colours stand out. **Zoom to all sites** fits every site on screen. The **View** menu switches between light and dark themes or follows the system.

## Terrain

Heights come from the Mapzen/AWS Terrain Tiles (SRTM, EU-DEM and others), at about 25, 50 or 100 m resolution (set on the Coverage tab). The data includes some buildings and trees in places. Tiles are cached, so plots work offline once an area has been downloaded.

## Where things are kept

- Sites, antennas, plots, links and settings: `~/.local/share/coverant` on Linux, `%APPDATA%\coverant` on Windows, `~/Library/Application Support/coverant` on macOS. **File → Open data folder** opens it.
- Terrain and map tiles: `~/.cache/coverant`, `%LOCALAPPDATA%\coverant` or `~/Library/Caches/coverant`.

The environment variables `COVERANT_DATA_DIR` and `COVERANT_CACHE_DIR` move them. To move your work to another computer, use **File → Back up everything…** and restore the backup there. Copying the data folder also works. See **Backup, import and export**.

## If it won't start

Set `COVERANT_RENDERER=glow` to use plain OpenGL if the graphics driver won't run the default renderer. On Linux, egui needs `libxkbcommon-x11-0` and `libgl1`.
