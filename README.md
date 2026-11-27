# Coverant

A coverage planner for radio sites, in the spirit of VE2DBE's Radio Mobile, for Linux,
Windows and macOS. It is written in Rust with egui, with the same look as
[wireant](https://github.com/tanganyika/wireant), and the heavy calculations run on every
processor core.

## What it does (phase 1)

- **Sites.** Type a location as decimal lat/long, degrees-minutes-seconds, degrees and
  decimal minutes, a Maidenhead locator (4 to 10 characters) or an OS grid reference
  (`TL 450 585`, `TL4500058500`, or eastings,northings). Then click the map or drag the pin
  to put it exactly in place. Right-click the map to start a new site there. Every way of
  writing the position is shown, with the ground height from the terrain data.
  Each site can download the terrain around it when saved, out to a distance you choose,
  which is also the default radius of its plots. Deleting a site removes the terrain only
  it used.
- **Backup, import and export.** File → Back up everything writes sites, antennas, plots,
  links and settings to one file; Restore replaces what you have or adds to it, after
  keeping a copy. Sites can be imported from GPX, KML or GeoJSON (with a preview that skips
  ones you already have) and exported to any of the three.
- **An antenna library, apart from the sites.** Gain, omni or directional (horizontal
  beamwidth, front/back ratio) and vertical beamwidth. Presets fill in typical ground planes,
  dipoles, collinears, Yagis and panels.
- **Coverage plots as saved items.** Each plot puts one library antenna on one site, with its
  height, pointing and down-tilt there, and the radio: frequency, power, feeder loss,
  receiver sensitivity and polarisation. Copy a plot to try another antenna at the same site,
  or the same antenna at another site; show or hide each one on the map. When a run
  finishes the map zooms to fit it.
- **Modes.** Each plot has a mode (FM voice, FM good copy, digital voice, SSB, CW, FT8 or
  custom levels). The signal every receiver needs is the thermal noise in the mode's
  bandwidth, plus the noise figure, plus the SNR the mode needs; all three can be adjusted.
- **Realistic margins for 1.5 m stations.** Each plot has surroundings (open, rural,
  suburban, urban: 0, 6, 12, 20 dB) for hand portables and mobiles, and a share of spots
  covered (default 90 %, a 5.5 dB log-normal spread as in ITU-R P.1546). Hand portables
  default to a −5 dBi rubber duck with 4 dB body loss.
- **Coverage** over real terrain with the Longley-Rice Irregular Terrain Model
  (ITM 1.2.2, the model Radio Mobile and SPLAT! use), laid over the map in three colours:
  - **blue**: hand portables hear the site;
  - **magenta**: mobiles hear the site;
  - **green**: a base station with an antenna like the plot's own hears it.

  Click the map for the predicted signal (dBm and µV), path loss, margin and propagation
  mode for each kind of station.
- **Saving.** Changes to sites, antennas and plots are kept until you press **Save** (or Ctrl+S); **Undo changes** goes back to the last save, and quitting with unsaved changes asks first. Receive levels and model settings are remembered automatically.
- **Maps:** OpenStreetMap, OpenTopoMap or Esri satellite imagery, optionally in grey so the
  coverage stands out. Light and dark themes (View menu).

- **Links between two sites.** Pick a site, an antenna, a height, power and feeder loss for
  each end, and the frequency and mode. Coverant draws the terrain profile with the earth's
  bulge (k = 4/3), the line of sight and the first Fresnel zone (with the 60 % line), finds
  the worst clearance, and gives the Longley-Rice path loss and the signal and margin at
  each end. Hover over the profile for heights and clearance at any point. A calculated
  link updates as you change it.

- **Radio horizon.** Choose a site, its antenna height and the height of the receivers.
  The map shades the ground from which a receiver could see the antenna (over the curved
  earth, k = 4/3) and draws how far that reaches on each bearing. A 360° panorama under the
  map shows the skyline as seen from the antenna against the smooth-earth horizon; hover
  for the bearing, angle and distance.

## User guide

The app has a built-in guide: press F1, choose Help → User guide, or click the ? next to any
tab heading. The same pages are in [docs/](docs/01-getting-started.md) here on GitHub.

## Installing

Each [release](https://github.com/tanganyika/coverant/releases) has a Debian/Ubuntu package
(Ubuntu 22.04, Debian 12, Mint 21 or newer), a Linux tarball, a Windows zip and a macOS
(Apple Silicon) tarball. For the package:

```
sudo apt install ./coverant_0.4.0-1_amd64.deb
```

It adds Coverant to the applications menu (under Ham Radio / Science).

## Building and running

Install Rust from <https://rustup.rs>, then in this folder:

```
cargo run --release
```

The first build takes a few minutes. The program is then
`target/release/coverant` (`coverant.exe` on Windows), which can be copied anywhere.
On Linux, egui needs the usual X11/Wayland libraries (on Debian/Ubuntu:
`sudo apt install libxkbcommon-x11-0 libgl1`). `cargo install cargo-deb && cargo deb`
builds the .deb in `target/debian/`.

Pushing to GitHub builds Windows, macOS and Linux programs with GitHub Actions
(`.github/workflows/build.yml`); a `v*` tag makes a release with them.

## Getting sensible results

Longley-Rice predicts the median signal over the terrain. It does not know about
buildings, trees or a body holding the radio, so the receive-level table has a **Losses**
column for those allowances (the hand portable starts with 6 dB). **Needs** is the signal
that kind of station needs: −107 dBm ≈ 1 µV, −113 dBm ≈ 0.5 µV, −119 dBm ≈ 0.25 µV
(into 50 Ω).

Under *Propagation model* set the climate (UK: "Maritime temperate, over land"), ground,
surface refractivity, and time / location / situation percentages; raise those above 50 %
for a more cautious plot.

## Data

Terrain comes from the free AWS Terrain Tiles (SRTM, EU-DEM and national datasets; no sign-up).
Terrain and map tiles are downloaded as needed and cached, so areas you have looked at work
offline.

| What | Where |
|---|---|
| Sites, settings, plots | `~/.local/share/coverant`, `%APPDATA%\coverant`, `~/Library/Application Support/coverant` |
| Terrain and map tiles | `~/.cache/coverant`, `%LOCALAPPDATA%\coverant`, `~/Library/Caches/coverant` |

`COVERANT_DATA_DIR` and `COVERANT_CACHE_DIR` move them. File → Open data folder opens
the first. Set `COVERANT_RENDERER=glow` to use plain OpenGL if the default renderer won't
start (it falls back on its own too). On Linux the file windows use the desktop portal
(`xdg-desktop-portal`), which most desktops already have.

## Code

```
src/itm.rs       Longley-Rice ITM 1.2.2, point to point
src/terrain.rs   terrain tiles, sampling, Web Mercator and great-circle helpers
src/coverage.rs  radial coverage calculation, the map image, point queries
src/coords.rs    lat/long, DMS, Maidenhead and OS grid conversions
src/map.rs       the map widget: tiles, pan and zoom, pins
src/model.rs     sites, stations, receive levels, settings, saving
src/app.rs       the window: tab rail, panels, map
src/theme.rs     fonts, colours and widget style (shared with wireant)
```

`cargo test --release` checks the ITM code against 40 reference paths and the coordinate
conversions against known points.

Elevation data: Terrain Tiles (Mapzen / Tilezen, on the AWS Registry of Open Data). Maps ©
OpenStreetMap contributors, OpenTopoMap (CC-BY-SA), Esri. Fonts: Inter and JetBrains Mono
(SIL Open Font License).
