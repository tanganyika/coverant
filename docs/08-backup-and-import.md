# Backup, import and export

Everything for this is on the **File** menu.

## Backing up

**File → Back up everything…** writes one file holding your sites, antennas, coverage plots, links and settings. Keep it somewhere safe, or use it to move your work to another computer.

Coverage results aren't in the backup, because they can be big. After a restore, run the plots again on the Coverage tab.

## Restoring

**File → Restore from backup…** opens a backup and shows what's in it and when it was made. Then choose one of these:

- **Replace everything with the backup**: your sites, antennas, plots and links are swapped for the backup's. Tick **Restore the settings too** to bring back the receivers, model settings and map choices as well.
- **Add what's in the backup to what I have**: anything you don't already have is added. Where both have the same item, yours is kept.

Before it changes anything, Coverant saves a copy of what you have now in the `backups` folder inside the data folder (**File → Open data folder**). If a restore wasn't what you wanted, restore that copy.

## Importing sites

**File → Import sites…**, or **Import…** at the top of the Sites tab, reads sites from these files:

- **GPX**: waypoints, or route points if there are no waypoints. Tracks are skipped.
- **KML** from Google Earth and similar: placemarks that are points. Lines and areas are skipped. For a KMZ file, unzip it and import the `doc.kml` inside.
- **GeoJSON**: Point and MultiPoint features. The name comes from a `name` or `title` property, and the notes from `notes` or `description`.

Coverant lists what it found before adding anything. Points that match a site you already have are unticked: either the same name within 50 m, or any point within 5 m. You can also choose:

- **Use the file's heights as the ground height.** This is off by default, so heights come from the terrain data. GPS heights are often tens of metres out.
- **Download the terrain around each site when saved.** This is on for small files and off for files with more than ten points.

Imported sites aren't saved until you press **Save**, so **Undo changes** removes them if the import wasn't right.

## Exporting sites

**File → Export sites** writes all your sites as GPX, KML or GeoJSON, with their names, notes, colours and ground heights where you've set them. Use it to load your sites into a GPS, Google Earth or a GIS program.

## If no file window appears

On Linux the file window comes from the desktop's portal service. Most desktops have it already. If nothing opens, install `xdg-desktop-portal` with the version for your desktop, such as `xdg-desktop-portal-gtk` or `xdg-desktop-portal-kde`.
