//! Coverage prediction, Radio Mobile style: terrain profiles along many radials
//! from the site, the Longley-Rice loss at every step for each kind of
//! receiving station, then a Web-Mercator image for the map.

use crate::itm::{self, ItmParams, Mode};
use crate::model::{Antenna, Receiver, Settings, Site, Station};
use crate::terrain::{EARTH_R, Terrain, destination, distance_bearing, lonlat_to_px, metres_per_px, px_to_lonlat};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

const K_FACTOR: f64 = 4.0 / 3.0;

/// A receive level resolved to numbers (the base level may copy the plot's antenna).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Level {
    pub name: String,
    pub height_m: f64,
    pub gain_dbi: f64,
    pub loss_db: f64,
    pub required_dbm: f64,
    pub colour: [u8; 3],
}

pub fn levels(st: &Station, s: &Settings) -> Vec<Level> {
    let mut out = vec![];
    for (i, r) in s.levels().into_iter().enumerate() {
        if !r.enabled {
            continue;
        }
        let Receiver { name, height_m, gain_dbi, loss_db, required_dbm, colour, .. } = r.clone();
        let mut l = Level { name, height_m, gain_dbi, loss_db, required_dbm, colour };
        if i == 2 && r.use_site_antenna {
            l.height_m = st.antenna.height_m;
            l.gain_dbi = st.antenna.gain_dbi;
            l.loss_db = st.feeder_loss_db;
            l.required_dbm = st.rx_sensitivity_dbm;
        }
        out.push(l);
    }
    out
}

/// Loss of the antenna (dB, ≥ 0) away from its peak, by azimuth from
/// boresight and elevation angle: 12·(angle/beamwidth)² dB, limited by the
/// front/back ratio and a 20 dB side-lobe floor.
pub fn pattern_loss(a: &Antenna, rel_az: f64, elev: f64) -> f64 {
    let av = if a.v_beamwidth > 0.0 { (12.0 * ((elev + a.tilt) / a.v_beamwidth).powi(2)).min(20.0) } else { 0.0 };
    if a.directional {
        let rel = (rel_az + 180.0).rem_euclid(360.0) - 180.0;
        let ah = (12.0 * (rel / a.h_beamwidth.max(1.0)).powi(2)).min(a.front_back_db);
        (ah + av).min(a.front_back_db.max(20.0))
    } else {
        av
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Stats {
    pub name: String,
    pub area_km2: f64,
    pub colour: [u8; 3],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Meta {
    /// the operating mode the levels were set for (empty in older plots)
    #[serde(default)]
    pub mode: String,
    pub site_lat: f64,
    pub site_lon: f64,
    pub radius_km: f64,
    pub n_rad: usize,
    pub n_steps: usize,
    pub step_m: f64,
    pub dem_res_m: f64,
    pub site_ground_m: f64,
    pub eirp_dbm: f64,
    pub seconds: f64,
    pub levels: Vec<Level>,
    pub stats: Vec<Stats>,
    /// image placement: global Web-Mercator pixels at `img_zoom`
    pub img_zoom: f64,
    pub img_x0: f64,
    pub img_y0: f64,
    pub img_w: u32,
    pub img_h: u32,
    pub created: u64,
}

/// A finished plot: the picture plus what's needed to answer "what's the signal here?".
pub struct Coverage {
    pub meta: Meta,
    pub image: image::RgbaImage,
    /// [level][radial][step] received signal, dBm
    pub signal: Vec<f32>,
    pub loss: Vec<f32>,
    pub mode: Vec<i8>,
    /// [radial][step] ground height
    pub z: Vec<f32>,
}

pub struct Progress {
    pub stage: parking::Stage,
    pub done: AtomicUsize,
    pub total: AtomicUsize,
    pub cancel: AtomicBool,
}

pub mod parking {
    use std::sync::Mutex;
    /// The stage name, readable from the UI thread.
    #[derive(Default)]
    pub struct Stage(Mutex<String>);
    impl Stage {
        pub fn set(&self, s: &str) {
            *self.0.lock().unwrap() = s.to_string();
        }
        pub fn get(&self) -> String {
            self.0.lock().unwrap().clone()
        }
    }
}

impl Progress {
    pub fn new() -> Arc<Self> {
        Arc::new(Progress { stage: Default::default(), done: AtomicUsize::new(0), total: AtomicUsize::new(1), cancel: AtomicBool::new(false) })
    }
    pub fn fraction(&self) -> f32 {
        self.done.load(Ordering::Relaxed) as f32 / self.total.load(Ordering::Relaxed).max(1) as f32
    }
}

/// Coverage of `st` (radio and antenna) installed at `site`, out to `settings.model.radius_km`.
pub fn run(site: &Site, st: &Station, settings: &Settings, terrain: &Terrain, prog: &Progress) -> Result<Coverage, String> {
    let t0 = std::time::Instant::now();
    let m = &settings.model;
    let (lat0, lon0) = (site.lat, site.lon);
    let radius = m.radius_km.clamp(1.0, 300.0) * 1000.0;
    let h_tx = st.antenna.height_m.max(0.5);
    let eirp = st.eirp_dbm();
    let levels = levels(st, settings);
    if levels.is_empty() {
        return Err("No receive levels are switched on".into());
    }

    prog.stage.set("Fetching terrain");
    let grid = terrain.grid_around(lat0, lon0, radius, &|d, n| {
        prog.done.store(d, Ordering::Relaxed);
        prog.total.store(n, Ordering::Relaxed);
    })?;
    let dem_res = terrain.resolution_m(lat0);
    let (step_f, rad_f) = match m.detail {
        0 => (2.0, 4.0),
        2 => (0.8, 1.6),
        _ => (1.25, 2.5),
    };
    let n_steps = ((radius / (step_f * dem_res)) as usize).clamp(50, 1500);
    let xi = radius / n_steps as f64;
    let n_rad = ((2.0 * std::f64::consts::PI * radius / (xi * rad_f)) as usize).clamp(360, 3600).div_ceil(8) * 8;
    let npts = n_steps + 1;

    prog.stage.set("Reading terrain profiles");
    let site_ground = site.ground_m.unwrap_or_else(|| grid.sample(lat0, lon0));
    let z: Vec<Vec<f64>> = (0..n_rad)
        .into_par_iter()
        .map(|r| {
            let mut p = grid.radial(lat0, lon0, r as f64 * 360.0 / n_rad as f64, xi, n_steps);
            p[0] = site_ground;
            p
        })
        .collect();

    prog.stage.set("Running Longley-Rice");
    prog.done.store(0, Ordering::Relaxed);
    prog.total.store(n_rad, Ordering::Relaxed);
    let params = ItmParams {
        fmhz: st.freq_mhz,
        eps: m.eps,
        sgm: m.sgm,
        ens0: m.ns,
        pol: if st.vertical { 1 } else { 0 },
        klim: m.climate,
        zt: itm::qerfi(m.time_pct / 100.0),
        zl: itm::qerfi(m.location_pct / 100.0),
        zc: itm::qerfi(m.situation_pct / 100.0),
        mdvar: 12,
    };
    let k_min = 2.max((200.0 / xi).ceil() as usize);
    let nl = levels.len();
    let ae = K_FACTOR * EARTH_R;
    // per radial: (signal[level][step], loss[level][step], mode[step])
    let per_rad: Vec<(Vec<f32>, Vec<f32>, Vec<i8>)> = (0..n_rad)
        .into_par_iter()
        .map(|r| {
            let mut sig = vec![f32::NAN; nl * npts];
            let mut los = vec![f32::NAN; nl * npts];
            let mut mode = vec![-1i8; npts];
            if prog.cancel.load(Ordering::Relaxed) {
                return (sig, los, mode);
            }
            let zr = &z[r];
            let az = r as f64 * 360.0 / n_rad as f64;
            let rel = az - st.antenna.azimuth;
            for k in 1..npts {
                let d = k as f64 * xi;
                for (li, lv) in levels.iter().enumerate() {
                    let h_rx = lv.height_m.max(0.5);
                    let (l, md) = if k < k_min {
                        (itm::free_space_loss(st.freq_mhz, d), Mode::LineOfSight)
                    } else {
                        itm::p2p_loss(&zr[..=k], xi, h_tx, h_rx, &params)
                    };
                    if li == 0 {
                        mode[k] = md as i8;
                    }
                    let elev = (((zr[k] + h_rx) - (site_ground + h_tx)).atan2(d) - d / (2.0 * ae)).to_degrees();
                    let s = eirp - pattern_loss(&st.antenna, rel, elev) - l + lv.gain_dbi - lv.loss_db;
                    sig[li * npts + k] = s as f32;
                    los[li * npts + k] = l as f32;
                }
            }
            prog.done.fetch_add(1, Ordering::Relaxed);
            (sig, los, mode)
        })
        .collect();
    if prog.cancel.load(Ordering::Relaxed) {
        return Err("Stopped".into());
    }
    // rearrange to [level][radial][step]
    let mut signal = vec![f32::NAN; nl * n_rad * npts];
    let mut loss = vec![f32::NAN; nl * n_rad * npts];
    let mut mode = vec![-1i8; n_rad * npts];
    for (r, (s, l, md)) in per_rad.into_iter().enumerate() {
        for li in 0..nl {
            let dst = (li * n_rad + r) * npts;
            signal[dst..dst + npts].copy_from_slice(&s[li * npts..(li + 1) * npts]);
            loss[dst..dst + npts].copy_from_slice(&l[li * npts..(li + 1) * npts]);
        }
        mode[r * npts..(r + 1) * npts].copy_from_slice(&md);
    }
    let zf: Vec<f32> = z.iter().flat_map(|p| p.iter().map(|&v| v as f32)).collect();

    // ---- picture in Web Mercator
    prog.stage.set("Drawing the map");
    let mut zoom = 14.0;
    let (wx, ex) = loop {
        let (wl, wlat) = destination(lat0, lon0, 270.0, radius);
        let (el, elat) = destination(lat0, lon0, 90.0, radius);
        let w = lonlat_to_px(wlat, wl, zoom).0;
        let e = lonlat_to_px(elat, el, zoom).0;
        if e - w <= 1600.0 || zoom <= 6.0 {
            break (w, e);
        }
        zoom -= 1.0;
    };
    let (nl_, nlon) = destination(lat0, lon0, 0.0, radius);
    let (sl_, slon) = destination(lat0, lon0, 180.0, radius);
    let ny = lonlat_to_px(nlon, nl_, zoom).1;
    let sy = lonlat_to_px(slon, sl_, zoom).1;
    let (x0, x1, y0, y1) = (wx.floor(), ex.ceil(), ny.floor(), sy.ceil());
    let (w, h) = ((x1 - x0) as u32, (y1 - y0) as u32);
    let mut img = image::RgbaImage::new(w, h);
    let mut counts = vec![0usize; nl];
    let rows: Vec<(Vec<u8>, Vec<usize>)> = (0..h)
        .into_par_iter()
        .map(|py| {
            let mut row = vec![0u8; w as usize * 4];
            let mut cnt = vec![0usize; nl];
            for px in 0..w {
                let (lon, lat) = px_to_lonlat(x0 + px as f64 + 0.5, y0 + py as f64 + 0.5, zoom);
                let (d, b) = distance_bearing(lat0, lon0, lat, lon);
                let ki = (d / xi).round() as usize;
                if ki > n_steps || ki == 0 {
                    continue;
                }
                let ri = (b / (360.0 / n_rad as f64)).round() as usize % n_rad;
                let mut best = None;
                for li in (0..nl).rev() {
                    let s = signal[(li * n_rad + ri) * npts + ki];
                    if s as f64 >= levels[li].required_dbm {
                        best = Some(li);
                        cnt[li] += 1;
                    }
                }
                if let Some(li) = best {
                    let c = levels[li].colour;
                    row[px as usize * 4..px as usize * 4 + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
                }
            }
            (row, cnt)
        })
        .collect();
    for (py, (row, cnt)) in rows.into_iter().enumerate() {
        img.as_mut()[py * w as usize * 4..(py + 1) * w as usize * 4].copy_from_slice(&row);
        for (a, b) in counts.iter_mut().zip(cnt) {
            *a += b;
        }
    }
    let px_area = metres_per_px(lat0, zoom).powi(2) / 1e6;
    let stats = levels.iter().zip(&counts).map(|(l, &c)| Stats { name: l.name.clone(), area_km2: c as f64 * px_area, colour: l.colour }).collect();
    let meta = Meta {
        mode: String::new(),
        site_lat: lat0,
        site_lon: lon0,
        radius_km: radius / 1000.0,
        n_rad,
        n_steps,
        step_m: xi,
        dem_res_m: dem_res,
        site_ground_m: site_ground,
        eirp_dbm: eirp,
        seconds: t0.elapsed().as_secs_f64(),
        levels,
        stats,
        img_zoom: zoom,
        img_x0: x0,
        img_y0: y0,
        img_w: w,
        img_h: h,
        created: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
    };
    Ok(Coverage { meta, image: img, signal, loss, mode, z: zf })
}

/// What the plot says about one point.
pub struct PointInfo {
    pub distance_km: f64,
    pub bearing: f64,
    pub ground_m: f64,
    pub mode: &'static str,
    /// (name, colour, signal dBm, path loss dB, margin dB)
    pub levels: Vec<(String, [u8; 3], f64, f64, f64)>,
}

impl Coverage {
    pub fn query(&self, lat: f64, lon: f64) -> Option<PointInfo> {
        let m = &self.meta;
        let (d, b) = distance_bearing(m.site_lat, m.site_lon, lat, lon);
        let ki = (d / m.step_m).round() as usize;
        if ki == 0 || ki > m.n_steps {
            return None;
        }
        let npts = m.n_steps + 1;
        let ri = (b / (360.0 / m.n_rad as f64)).round() as usize % m.n_rad;
        let mode = match self.mode[ri * npts + ki] {
            0 => "line of sight",
            1 => "diffraction",
            2 => "troposcatter",
            _ => "–",
        };
        let levels = m
            .levels
            .iter()
            .enumerate()
            .map(|(li, l)| {
                let s = self.signal[(li * m.n_rad + ri) * npts + ki] as f64;
                (l.name.clone(), l.colour, s, self.loss[(li * m.n_rad + ri) * npts + ki] as f64, s - l.required_dbm)
            })
            .collect();
        Some(PointInfo { distance_km: d / 1000.0, bearing: b, ground_m: self.z[ri * npts + ki] as f64, mode, levels })
    }

    // -------------------------------------------------------------- saving

    fn dir() -> std::path::PathBuf {
        crate::model::data_dir().join("coverage")
    }

    pub fn save(&self, id: &str) -> Result<(), String> {
        let dir = Self::dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        self.image.save(dir.join(format!("{id}.png"))).map_err(|e| e.to_string())?;
        std::fs::write(dir.join(format!("{id}.json")), serde_json::to_string_pretty(&self.meta).unwrap()).map_err(|e| e.to_string())?;
        // the numbers behind the picture, as 0.1 dB / 0.5 m steps
        let mut b = Vec::with_capacity(self.signal.len() * 4 + self.mode.len() + self.z.len() * 2);
        let q = |v: f32, s: f32| -> i16 { if v.is_nan() { i16::MIN } else { (v * s).round().clamp(-32767.0, 32767.0) as i16 } };
        for &v in &self.signal {
            b.extend_from_slice(&q(v, 10.0).to_le_bytes());
        }
        for &v in &self.loss {
            b.extend_from_slice(&q(v, 10.0).to_le_bytes());
        }
        b.extend(self.mode.iter().map(|&m| m as u8));
        for &v in &self.z {
            b.extend_from_slice(&q(v, 2.0).to_le_bytes());
        }
        std::fs::write(dir.join(format!("{id}.bin")), b).map_err(|e| e.to_string())
    }

    pub fn load(id: &str) -> Option<Coverage> {
        let dir = Self::dir();
        let meta: Meta = serde_json::from_str(&std::fs::read_to_string(dir.join(format!("{id}.json"))).ok()?).ok()?;
        let image = image::open(dir.join(format!("{id}.png"))).ok()?.to_rgba8();
        let b = std::fs::read(dir.join(format!("{id}.bin"))).ok()?;
        let npts = meta.n_steps + 1;
        let ns = meta.levels.len() * meta.n_rad * npts;
        let nz = meta.n_rad * npts;
        if b.len() != ns * 4 + nz * 3 {
            return None;
        }
        let rd = |o: usize, s: f32| -> f32 {
            let v = i16::from_le_bytes([b[o], b[o + 1]]);
            if v == i16::MIN { f32::NAN } else { v as f32 / s }
        };
        let signal = (0..ns).map(|i| rd(i * 2, 10.0)).collect();
        let loss = (0..ns).map(|i| rd(ns * 2 + i * 2, 10.0)).collect();
        let mode = b[ns * 4..ns * 4 + nz].iter().map(|&m| m as i8).collect();
        let z = (0..nz).map(|i| rd(ns * 4 + nz + i * 2, 2.0)).collect();
        Some(Coverage { meta, image, signal, loss, mode, z })
    }

    pub fn delete(id: &str) {
        for ext in ["png", "json", "bin"] {
            let _ = std::fs::remove_file(Self::dir().join(format!("{id}.{ext}")));
        }
    }
}
