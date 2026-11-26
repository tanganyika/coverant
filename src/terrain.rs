//! Terrain heights from the AWS Terrain Tiles (Mapzen's "Terrarium" PNG tiles,
//! built from SRTM, EU-DEM, national LIDAR and others), downloaded once into a
//! cache folder. Also the Web-Mercator and great-circle helpers the map and the
//! coverage plot share.

use rayon::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

pub const TILE: f64 = 256.0;
pub const EARTH_R: f64 = 6371008.8;
const URL: &str = "https://s3.amazonaws.com/elevation-tiles-prod/terrarium";
pub const ATTRIBUTION: &str = "Elevation: Terrain Tiles (Mapzen/Tilezen on AWS Open Data) from SRTM, EU-DEM, \
     GMTED2010, ETOPO1 and national datasets.";

// ------------------------------------------------------------------ web mercator

/// Global Web-Mercator pixel coordinates at a zoom level.
pub fn lonlat_to_px(lon: f64, lat: f64, zoom: f64) -> (f64, f64) {
    let n = TILE * 2f64.powf(zoom);
    let lat = lat.clamp(-85.0511, 85.0511);
    let x = (lon + 180.0) / 360.0 * n;
    let s = lat.to_radians().sin();
    let y = (0.5 - ((1.0 + s) / (1.0 - s)).ln() / (4.0 * std::f64::consts::PI)) * n;
    (x, y)
}

pub fn px_to_lonlat(x: f64, y: f64, zoom: f64) -> (f64, f64) {
    let n = TILE * 2f64.powf(zoom);
    let lon = x / n * 360.0 - 180.0;
    let lat = (std::f64::consts::PI * (1.0 - 2.0 * y / n)).sinh().atan().to_degrees();
    (lon, lat)
}

/// Ground size of one pixel (m) at a latitude and zoom.
pub fn metres_per_px(lat: f64, zoom: f64) -> f64 {
    2.0 * std::f64::consts::PI * EARTH_R * lat.to_radians().cos() / (TILE * 2f64.powf(zoom))
}

// ------------------------------------------------------------------ great circle

pub fn destination(lat: f64, lon: f64, bearing_deg: f64, dist_m: f64) -> (f64, f64) {
    let (p1, l1, b) = (lat.to_radians(), lon.to_radians(), bearing_deg.to_radians());
    let d = dist_m / EARTH_R;
    let p2 = (p1.sin() * d.cos() + p1.cos() * d.sin() * b.cos()).asin();
    let l2 = l1 + (b.sin() * d.sin() * p1.cos()).atan2(d.cos() - p1.sin() * p2.sin());
    (p2.to_degrees(), (l2.to_degrees() + 540.0).rem_euclid(360.0) - 180.0)
}

/// Great-circle distance (m) and initial bearing (degrees true).
pub fn distance_bearing(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> (f64, f64) {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dl = (lon2 - lon1).to_radians();
    let dp = p2 - p1;
    let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    let d = 2.0 * EARTH_R * a.min(1.0).sqrt().asin();
    let y = dl.sin() * p2.cos();
    let x = p1.cos() * p2.sin() - p1.sin() * p2.cos() * dl.cos();
    (d, (y.atan2(x).to_degrees() + 360.0).rem_euclid(360.0))
}

// ------------------------------------------------------------------ downloads

pub fn http_get(url: &str) -> Result<Vec<u8>, String> {
    let mut r = ureq::get(url)
        .header("User-Agent", concat!("radio-sites/", env!("CARGO_PKG_VERSION"), " (amateur radio coverage planner)"))
        .call()
        .map_err(|e| format!("Could not download {url}: {e}"))?;
    r.body_mut().with_config().limit(8 << 20).read_to_vec().map_err(|e| format!("Could not download {url}: {e}"))
}

/// The program's cache folder (terrain and map tiles). `RADIOSITES_CACHE_DIR` overrides it.
pub fn cache_root() -> PathBuf {
    if let Some(d) = std::env::var_os("RADIOSITES_CACHE_DIR") {
        return PathBuf::from(d);
    }
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("radio-sites")
}

// ------------------------------------------------------------------ elevation

/// Heights of one 256 × 256 tile (m above sea level; sea and below-sea land are 0).
type Heights = Vec<f32>;

pub struct Terrain {
    pub zoom: u32,
    tiles: Mutex<HashMap<(u32, u32), std::sync::Arc<Heights>>>,
}

impl Terrain {
    pub fn new(zoom: u32) -> Self {
        Terrain { zoom, tiles: Mutex::new(HashMap::new()) }
    }

    fn path(&self, x: u32, y: u32) -> PathBuf {
        cache_root().join("terrarium").join(self.zoom.to_string()).join(x.to_string()).join(format!("{y}.png"))
    }

    fn load(&self, x: u32, y: u32) -> Result<(std::sync::Arc<Heights>, bool), String> {
        if let Some(t) = self.tiles.lock().unwrap().get(&(x, y)) {
            return Ok((t.clone(), false));
        }
        let path = self.path(x, y);
        let (bytes, downloaded) = match std::fs::read(&path) {
            Ok(b) => (b, false),
            Err(_) => {
                let url = format!("{URL}/{}/{x}/{y}.png", self.zoom);
                let b = http_get(&url)?;
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let tmp = path.with_extension("part");
                if std::fs::write(&tmp, &b).is_ok() {
                    let _ = std::fs::rename(&tmp, &path);
                }
                (b, true)
            }
        };
        let img = image::load_from_memory(&bytes).map_err(|e| format!("Bad elevation tile {x}/{y}: {e}"))?.to_rgb8();
        let h: Heights = img
            .pixels()
            .map(|p| (p[0] as f32 * 256.0 + p[1] as f32 + p[2] as f32 / 256.0 - 32768.0).max(0.0))
            .collect();
        let h = std::sync::Arc::new(h);
        let mut map = self.tiles.lock().unwrap();
        if map.len() > 2000 {
            map.clear();
        }
        map.insert((x, y), h.clone());
        Ok((h, downloaded))
    }

    /// Fetch every tile covering a box, in parallel, then stitch them into a grid.
    /// `progress(done, total)` is called as tiles arrive.
    pub fn grid(&self, lat_min: f64, lon_min: f64, lat_max: f64, lon_max: f64, progress: &(dyn Fn(usize, usize) + Sync)) -> Result<Grid, String> {
        let z = self.zoom as f64;
        let (xa, ya) = lonlat_to_px(lon_min, lat_max, z);
        let (xb, yb) = lonlat_to_px(lon_max, lat_min, z);
        let (tx0, tx1) = ((xa / TILE).floor() as u32, (xb / TILE).floor() as u32);
        let (ty0, ty1) = ((ya / TILE).floor() as u32, (yb / TILE).floor() as u32);
        let (ntx, nty) = ((tx1 - tx0 + 1) as usize, (ty1 - ty0 + 1) as usize);
        let jobs: Vec<(u32, u32)> = (ty0..=ty1).flat_map(|ty| (tx0..=tx1).map(move |tx| (tx, ty))).collect();
        let done = AtomicUsize::new(0);
        let total = jobs.len();
        let pool = rayon::ThreadPoolBuilder::new().num_threads(8).build().map_err(|e| e.to_string())?;
        let loaded: Vec<Result<((u32, u32), std::sync::Arc<Heights>), String>> = pool.install(|| {
            jobs.par_iter()
                .map(|&(tx, ty)| {
                    let r = self.load(tx, ty).map(|(h, _)| ((tx, ty), h));
                    progress(done.fetch_add(1, Ordering::Relaxed) + 1, total);
                    r
                })
                .collect()
        });
        let w = ntx * 256;
        let h = nty * 256;
        let mut data = vec![0f32; w * h];
        for r in loaded {
            let ((tx, ty), t) = r?;
            let (ox, oy) = (((tx - tx0) * 256) as usize, ((ty - ty0) * 256) as usize);
            for row in 0..256 {
                data[(oy + row) * w + ox..(oy + row) * w + ox + 256].copy_from_slice(&t[row * 256..row * 256 + 256]);
            }
        }
        Ok(Grid { data, w, h, x0: tx0 as f64 * TILE, y0: ty0 as f64 * TILE, zoom: z })
    }

    pub fn grid_around(&self, lat: f64, lon: f64, radius_m: f64, progress: &(dyn Fn(usize, usize) + Sync)) -> Result<Grid, String> {
        let dlat = (radius_m / EARTH_R).to_degrees() * 1.02 + 0.002;
        let dlon = dlat / lat.to_radians().cos().max(0.01);
        self.grid(lat - dlat, lon - dlon, lat + dlat, lon + dlon, progress)
    }

    /// Height of one point (m above sea level).
    pub fn height_at(&self, lat: f64, lon: f64) -> Result<f64, String> {
        Ok(self.grid_around(lat, lon, 60.0, &|_, _| {})?.sample(lat, lon))
    }

    pub fn resolution_m(&self, lat: f64) -> f64 {
        metres_per_px(lat, self.zoom as f64)
    }
}

/// Stitched heights with bilinear sampling by latitude and longitude.
pub struct Grid {
    data: Vec<f32>,
    w: usize,
    h: usize,
    x0: f64,
    y0: f64,
    zoom: f64,
}

impl Grid {
    pub fn sample(&self, lat: f64, lon: f64) -> f64 {
        let (x, y) = lonlat_to_px(lon, lat, self.zoom);
        let x = (x - self.x0 - 0.5).clamp(0.0, self.w as f64 - 1.001);
        let y = (y - self.y0 - 0.5).clamp(0.0, self.h as f64 - 1.001);
        let (ix, iy) = (x.floor() as usize, y.floor() as usize);
        let (fx, fy) = ((x - ix as f64) as f32, (y - iy as f64) as f32);
        let a = |r: usize, c: usize| self.data[r * self.w + c];
        let top = a(iy, ix) * (1.0 - fx) + a(iy, ix + 1) * fx;
        let bot = a(iy + 1, ix) * (1.0 - fx) + a(iy + 1, ix + 1) * fx;
        (top * (1.0 - fy) + bot * fy) as f64
    }

    /// Heights every `step` metres along a bearing, from the site out to `n` steps.
    pub fn radial(&self, lat: f64, lon: f64, bearing: f64, step: f64, n: usize) -> Vec<f64> {
        (0..=n)
            .map(|k| {
                let (la, lo) = destination(lat, lon, bearing, k as f64 * step);
                self.sample(la, lo)
            })
            .collect()
    }
}
