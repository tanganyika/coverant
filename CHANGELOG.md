# Changelog

## 0.4.1 (2026-11-27)

- **Windows start-up fixes.** Coverant uses DirectX 12 on Windows rather than letting wgpu
  pick Vulkan, which old Intel drivers crash on. If the graphics driver still crashes it
  while starting, the next start uses OpenGL on its own. The program now links the C runtime
  in, so it starts without the Visual C++ Redistributable.
- **Windows installer** (`coverant-<version>-windows-x86_64-setup.exe`): installs per user
  without administrator rights (or for everyone), adds a Start menu entry and optional desktop
  shortcut, upgrades in place, and uninstalls from Settings → Apps. The program carries its
  icon in Explorer and the taskbar. The portable .zip is still there.
- The Linux menu entry no longer appears twice on desktops that file it under both Science
  and Education.
- Start-up failures and crashes are shown in a message box and written to
  `startup-error.txt` in the cache folder.

## 0.4.0 (2026-11-27)

- **Backup and restore.** File → Back up everything writes sites, antennas, plots, links and
  settings to one file. Restore replaces what you have or adds to it, and keeps a copy of
  your data first.
- **Import and export sites** as GPX, KML or GeoJSON. Imports show a preview, and points
  that match sites you already have are unticked.
- **User guide** in the app (F1, the Help menu, or the ? next to each tab), and in `docs/`.
- **Linux APT repository** on GitHub Pages, so new versions come with system updates.
- Install notes for the unsigned Windows and macOS builds.

## 0.3.0 (2026-11-27)

- **Radio horizon tab**: what a site can see, as a map overlay and a 360° skyline panorama,
  with the smooth-earth horizon for comparison.

## 0.2.0 (2026-11-27)

- **Links tab**: the terrain profile between two sites, line of sight over the curved earth
  (k = 4/3), the first Fresnel zone, Longley-Rice path loss, and the signal and margin at
  each end.

## 0.1.1 (2026-11-27)

- The map shows the selected plot only, or every ticked plot if you choose.
- Choosing a site selects its plot.
- Power, loss and required-signal changes apply straight away. Plots whose site, antenna or
  model changed are marked out of date.

## 0.1.0 (2026-11-26)

- First release: sites (lat/long, DMS, Maidenhead, OS grid), terrain download per site, an
  antenna library, and coverage plots using Longley-Rice (ITM 1.2.2) with hand portable,
  mobile and base station levels.
- Modes (FM, digital voice, SSB, CW, FT8) set the signal needed; realistic margins for
  hand portables and mobiles.
- Debian package, app icon and menu entry.
