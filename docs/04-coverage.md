# Coverage plots

A coverage plot puts one antenna on one site and shows where three kinds of station hear it:

- **blue**: hand portables
- **magenta**: mobiles
- **green**: a base station (by default with the same antenna, height, feeder and receiver as the plot)

Hand portables are drawn on top. Plots are saved, and you can have as many as you like: copy one to try another antenna at the same site, or the same antenna at another site.

## Making and running a plot

- **New plot** uses the selected site and the selected antenna. If the site chosen on the Sites tab has no plot yet, a button offers to make one.
- **Installation and radio**: antenna height above ground, pointing (directional antennas), down-tilt, frequency, power, feeder loss and polarisation.
- **Run**: the **radius** (by default the site's terrain distance; ↺ puts it back), the **detail** and the **terrain data** resolution. Then press **Plot coverage**.

When a run finishes, the map zooms to show the whole plot. Click a plot in the list to select it and zoom to it.

## What the map shows

Only the selected plot is drawn. Tick **Show every ticked plot** to overlay several, with the selected one on top. **Overlay opacity** sets how strongly the colours show. Click the map for the signal, path loss, margin and propagation mode at that point.

## Mode: the signal each receiver needs

Pick the **Mode** and Coverant works out the signal needed as thermal noise in the bandwidth (−174 dBm/Hz) plus the noise figure plus the SNR the mode needs:

- FM voice (12 dB SINAD) about −116 dBm; FM good copy about −108 dBm
- digital voice (DMR, D-STAR, C4FM) about −118 dBm
- SSB about −124 dBm; CW about −135 dBm; FT8 about −154 dBm

You can change the bandwidth, SNR and noise figure. Raise the noise figure for a noisy town location. **Custom levels** lets each receive level have its own figure instead.

## Making it realistic

Longley-Rice sees only bare terrain, and by itself gives the median spot. Two settings bring it closer to real life:

- **Surroundings** adds loss for buildings and trees around hand portables and mobiles: open 0, rural 6, suburban 12 or urban 20 dB.
- **Spots covered** is the share of places at the edge of each colour where the signal is enough. 90 % adds 7 dB; 50 % is the median.

In **Receive levels**, set each station's height, antenna gain and losses (body loss for a hand portable, feeder for a mobile). A rubber duck is about −5 dBi.

## What updates at once and what needs a run

Power, feeder loss, gains, losses, mode, surroundings and spots covered update a finished plot straight away. Changing the site, antenna, height, pointing, tilt, frequency, polarisation, radius or the propagation model marks the plot **⚠ out of date**: press Plot coverage again.

## Propagation model

Under **Propagation model** you can set the climate, ground type, surface refractivity, and the time and situation percentages. The defaults (maritime temperate over land, average ground, 301 N-units, 50 %) suit most of the UK.
