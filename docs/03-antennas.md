# Antennas

The antenna library holds antennas on their own, apart from any site. A coverage plot or a link puts one of them on a site, at a height and bearing you choose there.

## Making an antenna

Press **New antenna** and choose a **Type**. The presets fill in typical figures:

- ¼ wave ground plane, half-wave dipole or J-pole
- collinears (2 × ⅝, 3 × ⅝, long collinears such as the X-200 or X-300)
- 3- and 6-element Yagis
- a 90° panel

Change any number and the type becomes **Custom**. Rename it to something you'll recognise.

## The figures

- **Gain** in dBi, at the peak of the main lobe.
- **Pattern**: omni, or directional with a **horizontal beamwidth** and a **front/back ratio**.
- **Vertical beamwidth** sets how fast the gain falls away above and below the main lobe. High-gain collinears have a narrow one, so ground close to a tall mast can get less signal. Set it to 0 to ignore the vertical pattern.

The pattern is modelled as 12 × (angle ÷ beamwidth)² dB, limited by the front/back ratio and a 20 dB floor.

**Copy** makes a new antenna from this one. An antenna that a plot or link uses can't be deleted until they use another one. The list shows how many plots use each antenna.
