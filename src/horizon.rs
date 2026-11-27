//! The radio horizon from a site: on every bearing, how high the skyline stands as seen
//! from the antenna (over the curved earth, k = 4/3), how far away it is, and which ground
//! a receiver at a given height could see.

use crate::coverage::Progress;
use crate::link::K_FACTOR;
use crate::model::Site;
use crate::terrain::{EARTH_R, Terrain, destination, distance_bearing, lonlat_to_px, metres_per_px, px_to_lonlat};
use rayon::prelude::*;
use std::sync::atomic::Ordering;

pub struct Inputs {
    pub site: Site,
    /// antenna height above ground (m)
    pub height_m: f64,
    /// height above ground of the receivers that count as "seeing" the site (m)
    pub target_m: f64,
    pub radius_km: f64,
    pub terrain_zoom: u32,
}

pub struct Horizon {
    pub lat: f64,
    pub lon: f64,
    pub ground_m: f64,
    pub height_m: f64,
    pub radius_m: f64,
    pub step_m: f64,
    pub n_steps: usize,
    /// per bearing (n_rad of them, evenly from north): skyline elevation (degrees, + up),
    /// distance to the point that makes it (m) and that point's ground height
    pub angle: Vec<f64>,
    pub dist: Vec<f64>,
    pub sky_ground: Vec<f64>,
    /// distance to the last visible ground point on each bearing (m)
    pub reach: Vec<f64>,
    /// [bearing][step] can a receiver at target height see the antenna?
    pub visible: Vec<bool>,
    /// elevation of the smooth-earth horizon (degrees) for this antenna height
    pub smooth_angle: f64,
    pub smooth_dist_m: f64,
    pub visible_km2: f64,
    pub image: image::RgbaImage,
    pub img_zoom: f64,
    pub img_x0: f64,
    pub img_y0: f64,
}

impl Horizon {
    pub fn n_rad(&self) -> usize {
        self.angle.len()
    }
    /// Index of the bearing nearest `b` degrees.
    pub fn at(&self, b: f64) -> usize {
        ((b.rem_euclid(360.0) / 360.0 * self.n_rad() as f64).round() as usize) % self.n_rad()
    }
}

pub fn compute(inp: &Inputs, terrain: &Terrain, prog: &Progress) -> Result<Horizon, String> {
    let (lat0, lon0) = (inp.site.lat, inp.site.lon);
    let radius = inp.radius_km.clamp(1.0, 300.0) * 1000.0;
    prog.stage.set("Fetching terrain");
    let grid = terrain.grid_around(lat0, lon0, radius, &|d, n| {
        prog.done.store(d, Ordering::Relaxed);
        prog.total.store(n, Ordering::Relaxed);
    })?;
    let res = terrain.resolution_m(lat0);
    let n_steps = ((radius / res) as usize).clamp(100, 3000);
    let xi = radius / n_steps as f64;
    let n_rad = 1440;
    let ground0 = inp.site.ground_m.unwrap_or_else(|| grid.sample(lat0, lon0));
    let h0 = ground0 + inp.height_m.max(0.5);
    let ae = K_FACTOR * EARTH_R;
    // slope (tan of elevation) from the antenna to a height `z` at distance `d`, over the curved earth
    let slope = |z: f64, d: f64| (z - h0) / d - d / (2.0 * ae);

    prog.stage.set("Tracing the skyline");
    prog.done.store(0, Ordering::Relaxed);
    prog.total.store(n_rad, Ordering::Relaxed);
    let per: Vec<(f64, f64, f64, f64, Vec<bool>)> = (0..n_rad)
        .into_par_iter()
        .map(|r| {
            let brg = r as f64 * 360.0 / n_rad as f64;
            let z = grid.radial(lat0, lon0, brg, xi, n_steps);
            let mut best = f64::NEG_INFINITY;
            let (mut best_d, mut best_z) = (xi, z[1]);
            let mut vis = vec![false; n_steps + 1];
            vis[0] = true;
            let mut reach = 0.0;
            for k in 1..=n_steps {
                let d = k as f64 * xi;
                // a receiver here sees the antenna if nothing nearer rises above the line to it
                if slope(z[k] + inp.target_m, d) >= best {
                    vis[k] = true;
                    reach = d;
                }
                let s = slope(z[k], d);
                if s > best {
                    best = s;
                    best_d = d;
                    best_z = z[k];
                }
            }
            prog.done.fetch_add(1, Ordering::Relaxed);
            (best.atan().to_degrees(), best_d, best_z, reach, vis)
        })
        .collect();
    if prog.cancel.load(Ordering::Relaxed) {
        return Err("Stopped".into());
    }
    let mut h = Horizon {
        lat: lat0,
        lon: lon0,
        ground_m: ground0,
        height_m: inp.height_m,
        radius_m: radius,
        step_m: xi,
        n_steps,
        angle: per.iter().map(|p| p.0).collect(),
        dist: per.iter().map(|p| p.1).collect(),
        sky_ground: per.iter().map(|p| p.2).collect(),
        reach: per.iter().map(|p| p.3).collect(),
        visible: per.into_iter().flat_map(|p| p.4).collect(),
        smooth_angle: -(2.0 * inp.height_m.max(0.5) / ae).sqrt().to_degrees(),
        smooth_dist_m: (2.0 * ae * inp.height_m.max(0.5)).sqrt(),
        visible_km2: 0.0,
        image: image::RgbaImage::new(1, 1),
        img_zoom: 0.0,
        img_x0: 0.0,
        img_y0: 0.0,
    };
    prog.stage.set("Drawing the map");
    draw(&mut h);
    Ok(h)
}

/// The visible area as a map image (Web Mercator), and its size.
fn draw(h: &mut Horizon) {
    let (lat0, lon0, radius) = (h.lat, h.lon, h.radius_m);
    let mut zoom = 14.0;
    let (x0, x1) = loop {
        let (wl, wlo) = destination(lat0, lon0, 270.0, radius);
        let (el, elo) = destination(lat0, lon0, 90.0, radius);
        let (w, e) = (lonlat_to_px(wlo, wl, zoom).0, lonlat_to_px(elo, el, zoom).0);
        if e - w <= 1600.0 || zoom <= 6.0 {
            break (w.floor(), e.ceil());
        }
        zoom -= 1.0;
    };
    let (nl, nlo) = destination(lat0, lon0, 0.0, radius);
    let (sl, slo) = destination(lat0, lon0, 180.0, radius);
    let (y0, y1) = (lonlat_to_px(nlo, nl, zoom).1.floor(), lonlat_to_px(slo, sl, zoom).1.ceil());
    let (w, hgt) = ((x1 - x0) as u32, (y1 - y0) as u32);
    let n_rad = h.n_rad();
    let npts = h.n_steps + 1;
    let rows: Vec<(Vec<u8>, usize)> = (0..hgt)
        .into_par_iter()
        .map(|py| {
            let mut row = vec![0u8; w as usize * 4];
            let mut cnt = 0;
            for px in 0..w {
                let (lon, lat) = px_to_lonlat(x0 + px as f64 + 0.5, y0 + py as f64 + 0.5, zoom);
                let (d, b) = distance_bearing(lat0, lon0, lat, lon);
                let k = (d / h.step_m).round() as usize;
                if k > h.n_steps {
                    continue;
                }
                let r = ((b / 360.0 * n_rad as f64).round() as usize) % n_rad;
                let c: [u8; 4] = if h.visible[r * npts + k] {
                    cnt += 1;
                    [255, 196, 0, 255]
                } else {
                    [40, 40, 60, 150]
                };
                row[px as usize * 4..px as usize * 4 + 4].copy_from_slice(&c);
            }
            (row, cnt)
        })
        .collect();
    let mut img = image::RgbaImage::new(w, hgt);
    let mut count = 0;
    for (py, (row, c)) in rows.into_iter().enumerate() {
        img.as_mut()[py * w as usize * 4..(py + 1) * w as usize * 4].copy_from_slice(&row);
        count += c;
    }
    h.visible_km2 = count as f64 * metres_per_px(lat0, zoom).powi(2) / 1e6;
    h.image = img;
    h.img_zoom = zoom;
    h.img_x0 = x0;
    h.img_y0 = y0;
}

#[cfg(test)]
mod tests {
    #[test]
    fn smooth_earth_horizon_distance() {
        // 10 m antenna, k = 4/3: √(2 × 8.495e6 × 10) ≈ 13.0 km
        let ae = super::K_FACTOR * super::EARTH_R;
        let d = (2.0 * ae * 10.0f64).sqrt();
        assert!((d - 13_035.0).abs() < 50.0, "{d}");
    }
}
