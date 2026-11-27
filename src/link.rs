//! A radio link between two sites: the terrain profile, line of sight over the curved
//! earth, the first Fresnel zone, the Longley-Rice path loss and the signal at each end.

use crate::coverage::pattern_loss;
use crate::itm::{self, ItmParams, Mode};
use crate::model::{ModelSettings, Site, Station};
use crate::terrain::{EARTH_R, Terrain, destination, distance_bearing};

/// Earth radius factor for refraction in the standard atmosphere.
pub const K_FACTOR: f64 = 4.0 / 3.0;

/// One end of a link as the calculation needs it.
pub struct End {
    pub site: Site,
    /// radio and antenna; directional antennas are aimed at the other end
    pub st: Station,
    /// the signal this end's receiver needs (dBm)
    pub required_dbm: f64,
}

/// One direction of the link.
#[derive(Clone, Debug)]
pub struct Direction {
    pub eirp_dbm: f64,
    /// loss of the sending and receiving antennas away from their main lobes
    pub pattern_db: f64,
    pub signal_dbm: f64,
    pub required_dbm: f64,
    pub margin_db: f64,
}

#[derive(Clone, Debug)]
pub struct LinkResult {
    pub dist_m: f64,
    pub bearing_ab: f64,
    pub bearing_ba: f64,
    /// metres between profile points
    pub xi: f64,
    /// ground height above sea level at each profile point, A to B
    pub z: Vec<f64>,
    /// antenna heights above sea level
    pub ant_a: f64,
    pub ant_b: f64,
    pub freq_mhz: f64,
    /// elevation of the line of sight from each end (degrees, + up), allowing for curvature
    pub elev_a: f64,
    pub elev_b: f64,
    pub free_space_db: f64,
    pub loss_db: f64,
    pub mode: Mode,
    pub ab: Direction,
    pub ba: Direction,
    /// least clearance of the line of sight above the terrain, as a fraction of the first
    /// Fresnel zone radius (negative = the terrain cuts the line), and where (m from A)
    pub worst_clearance: f64,
    pub worst_at_m: f64,
}

impl LinkResult {
    /// Bulge of the earth (m) at `x` metres from A, for the effective radius.
    pub fn bulge(&self, x: f64) -> f64 {
        x * (self.dist_m - x) / (2.0 * K_FACTOR * EARTH_R)
    }
    /// Height of the line of sight above sea level at `x`, on the flattened-earth drawing.
    pub fn los(&self, x: f64) -> f64 {
        self.ant_a + (self.ant_b - self.ant_a) * x / self.dist_m
    }
    /// First Fresnel zone radius (m) at `x`.
    pub fn fresnel(&self, x: f64) -> f64 {
        let lambda = 299.792458 / self.freq_mhz;
        (lambda * x * (self.dist_m - x) / self.dist_m).max(0.0).sqrt()
    }
    pub fn line_of_sight(&self) -> bool {
        self.worst_clearance >= 0.0
    }
}

pub fn compute(a: &End, b: &End, m: &ModelSettings, terrain: &Terrain) -> Result<LinkResult, String> {
    let (dist, brg_ab) = distance_bearing(a.site.lat, a.site.lon, b.site.lat, b.site.lon);
    if dist < 50.0 {
        return Err("The two sites are less than 50 m apart.".into());
    }
    let brg_ba = distance_bearing(b.site.lat, b.site.lon, a.site.lat, a.site.lon).1;
    let (mlat, mlon) = destination(a.site.lat, a.site.lon, brg_ab, dist / 2.0);
    let grid = terrain.grid_around(mlat, mlon, dist / 2.0 + 500.0, &|_, _| {})?;
    let res = terrain.resolution_m(mlat);
    let n = ((dist / (res * 0.75)) as usize).clamp(50, 4000);
    let xi = dist / n as f64;
    let mut z = grid.radial(a.site.lat, a.site.lon, brg_ab, xi, n);
    if let Some(g) = a.site.ground_m {
        z[0] = g;
    }
    if let Some(g) = b.site.ground_m {
        z[n] = g;
    }
    let (ha, hb) = (a.st.antenna.height_m.max(0.5), b.st.antenna.height_m.max(0.5));
    let freq = a.st.freq_mhz;
    let params = ItmParams {
        fmhz: freq,
        eps: m.eps,
        sgm: m.sgm,
        ens0: m.ns,
        pol: if a.st.vertical { 1 } else { 0 },
        klim: m.climate,
        zt: itm::qerfi(m.time_pct / 100.0),
        zl: itm::qerfi(m.location_pct / 100.0),
        zc: itm::qerfi(m.situation_pct / 100.0),
        mdvar: 12,
    };
    let (loss, mode) = itm::p2p_loss(&z, xi, ha, hb, &params);
    let fsl = itm::free_space_loss(freq, dist);

    let (ant_a, ant_b) = (z[0] + ha, z[n] + hb);
    let ae = K_FACTOR * EARTH_R;
    let elev_a = ((ant_b - ant_a) / dist - dist / (2.0 * ae)).atan().to_degrees();
    let elev_b = ((ant_a - ant_b) / dist - dist / (2.0 * ae)).atan().to_degrees();

    let mut r = LinkResult {
        dist_m: dist,
        bearing_ab: brg_ab,
        bearing_ba: brg_ba,
        xi,
        z,
        ant_a,
        ant_b,
        freq_mhz: freq,
        elev_a,
        elev_b,
        free_space_db: fsl,
        loss_db: loss,
        mode,
        ab: dir(a, b, 0.0, 0.0, loss),
        ba: dir(b, a, 0.0, 0.0, loss),
        worst_clearance: f64::INFINITY,
        worst_at_m: dist / 2.0,
    };
    // directional antennas are pointed straight at each other; the vertical pattern still counts
    let pa = |e: &End, elev: f64| {
        let mut ant = e.st.antenna.clone();
        ant.azimuth = 0.0;
        pattern_loss(&ant, 0.0, elev)
    };
    let (pla, plb) = (pa(a, elev_a), pa(b, elev_b));
    r.ab = dir(a, b, pla, plb, loss);
    r.ba = dir(b, a, plb, pla, loss);
    for k in 1..n {
        let x = k as f64 * xi;
        let ground = r.z[k] + r.bulge(x);
        let c = (r.los(x) - ground) / r.fresnel(x).max(0.01);
        if c < r.worst_clearance {
            r.worst_clearance = c;
            r.worst_at_m = x;
        }
    }
    Ok(r)
}

fn dir(from: &End, to: &End, pattern_tx: f64, pattern_rx: f64, loss: f64) -> Direction {
    let eirp = from.st.eirp_dbm();
    let signal = eirp - pattern_tx - loss + to.st.antenna.gain_dbi - pattern_rx - to.st.feeder_loss_db;
    Direction { eirp_dbm: eirp, pattern_db: pattern_tx + pattern_rx, signal_dbm: signal, required_dbm: to.required_dbm, margin_db: signal - to.required_dbm }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresnel_and_bulge() {
        let r = LinkResult {
            dist_m: 20_000.0, bearing_ab: 0.0, bearing_ba: 180.0, xi: 100.0, z: vec![0.0; 201], ant_a: 30.0, ant_b: 30.0,
            freq_mhz: 145.0, elev_a: 0.0, elev_b: 0.0, free_space_db: 0.0, loss_db: 0.0, mode: Mode::LineOfSight,
            ab: Direction { eirp_dbm: 0.0, pattern_db: 0.0, signal_dbm: 0.0, required_dbm: 0.0, margin_db: 0.0 },
            ba: Direction { eirp_dbm: 0.0, pattern_db: 0.0, signal_dbm: 0.0, required_dbm: 0.0, margin_db: 0.0 },
            worst_clearance: 0.0, worst_at_m: 0.0,
        };
        // mid-path bulge with k = 4/3: d²/(8kR) = 4e8 / (8 × 8.495e6) ≈ 5.9 m
        assert!((r.bulge(10_000.0) - 5.89).abs() < 0.05, "{}", r.bulge(10_000.0));
        // first Fresnel zone at mid-path, 145 MHz over 20 km: √(2.0675 × 5000) ≈ 101.7 m
        assert!((r.fresnel(10_000.0) - 101.7).abs() < 0.5, "{}", r.fresnel(10_000.0));
        assert_eq!(r.fresnel(0.0), 0.0);
    }
}
