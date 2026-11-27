# Getting started

Coverant plans radio sites. It predicts where a station can be heard, works out links between two sites, and shows how far a site can see, using Longley-Rice (ITM 1.2.2) over real terrain.

## The window

- **Tabs** down the left: **Sites**, **Antennas**, **Coverage**, **Links** and **Horizon**. Each tab has its own panel next to the map.
- **The map** fills the rest. Drag to move it, scroll or use **+** and **−** to zoom. Right-click anywhere to start a new site there.
- **The status bar** at the bottom shows the selected site, any unsaved changes, terrain downloads and what Coverant is doing.

## A first coverage plot in five steps

1. On **Sites**, press **New site** and click the map where the site is, or type its position (lat/long, a locator such as `IO92vh`, or an OS grid reference) and press **Go**.
2. Leave **Download terrain when saved** ticked and set how far out, then press **Save** (Ctrl+S). The terrain downloads in the background.
3. On **Antennas**, check the antenna in the library or press **New antenna** and pick a type.
4. On **Coverage**, press **New plot**. It puts the selected antenna on the selected site. Set the height, power, frequency and **Mode**.
5. Press **Plot coverage**. When it finishes, the map zooms to show the whole plot. Click the map for the signal at any point.

## Saving

Sites, antennas, plots and links are kept until you press **Save** (or Ctrl+S). **Undo changes** goes back to the last save, and quitting with unsaved changes asks first. Running a plot also saves that plot. Map position, receive levels and model settings are remembered on their own.

## Help

Press **F1** or use **Help → User guide** at any time. The **?** next to each tab's heading opens the page for that tab.
