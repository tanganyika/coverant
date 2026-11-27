# Links

A link joins two sites and works out whether they can talk to each other.

## Setting up a link

1. Press **New link**. It starts with the selected site and another one.
2. For **End A** and **End B**, choose the site, the antenna, its height, the transmit power and the feeder loss.
3. Set the **frequency**, **polarisation** and **mode** (which sets the signal each receiver needs, as on the Coverage tab).
4. Press **Calculate link**.

Once a link has been worked out, it updates as you change it. Links are saved with your sites. Directional antennas are taken as pointed at each other.

## The path profile

The profile appears under the map:

- **brown**: the terrain, with the earth's bulge added for an effective radius of 4/3 of the real one (normal refraction)
- **dashed blue curve**: sea level on the curved earth
- **straight line**: the line of sight between the two antennas, green if clear and red if the terrain cuts it
- **blue curves**: the first Fresnel zone, with a dashed line at 60 % of it
- **orange ring**: the point of worst clearance, also marked on the map

Hover over the profile for the distance, ground height, bulge, line-of-sight height, clearance and Fresnel radius at any point.

## The results

- distance, bearings both ways and the elevation angle from each end
- **worst clearance** as a percentage of the first Fresnel zone. A link wants at least 60 % for a clean line of sight; below 0 % the terrain blocks it
- **free-space loss** and the **Longley-Rice path loss** over the real terrain
- for each direction: EIRP, the signal at the far end in dBm and µV, and the **margin** over what the receiver needs. Green is 10 dB or more, amber is under 10 dB, and red means the signal is too weak.

Longley-Rice may call a blocked path "line of sight" because it judges by smooth-earth horizons. The profile and clearance show what the terrain really does.
