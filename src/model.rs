//! Sites, their radio and antenna, the receive levels and the propagation settings,
//! and where they are kept between runs:
//! ~/.local/share/radio-sites, %APPDATA%\radio-sites or
//! ~/Library/Application Support/radio-sites (`RADIOSITES_DATA_DIR` overrides it).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Antenna {
    pub preset: String,
    pub gain_dbi: f64,
    pub height_m: f64,
    pub directional: bool,
    pub azimuth: f64,
    pub h_beamwidth: f64,
    pub front_back_db: f64,
    /// degrees, positive = down
    pub tilt: f64,
    /// 0 = ignore the vertical pattern
    pub v_beamwidth: f64,
}

impl Default for Antenna {
    fn default() -> Self {
        Antenna {
            preset: "collinear_2x5_8".into(),
            gain_dbi: 4.5,
            height_m: 10.0,
            directional: false,
            azimuth: 0.0,
            h_beamwidth: 60.0,
            front_back_db: 20.0,
            tilt: 0.0,
            v_beamwidth: 40.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Station {
    pub freq_mhz: f64,
    pub power_w: f64,
    pub feeder_loss_db: f64,
    pub rx_sensitivity_dbm: f64,
    pub vertical: bool,
    pub antenna: Antenna,
}

impl Default for Station {
    fn default() -> Self {
        Station { freq_mhz: 145.0, power_w: 25.0, feeder_loss_db: 2.0, rx_sensitivity_dbm: -116.0, vertical: true, antenna: Antenna::default() }
    }
}

impl Station {
    pub fn tx_dbm(&self) -> f64 {
        10.0 * (self.power_w.max(1e-9) * 1000.0).log10()
    }
    pub fn eirp_dbm(&self) -> f64 {
        self.tx_dbm() - self.feeder_loss_db + self.antenna.gain_dbi
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Site {
    pub id: String,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    /// metres above sea level; None = from the terrain data
    pub ground_m: Option<f64>,
    pub notes: String,
    pub colour: [u8; 3],
    pub station: Station,
}

impl Default for Site {
    fn default() -> Self {
        Site { id: String::new(), name: "New site".into(), lat: 0.0, lon: 0.0, ground_m: None, notes: String::new(), colour: [211, 47, 47], station: Station::default() }
    }
}

pub fn new_id() -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{:x}", t ^ (std::process::id() as u128) << 64)
}

pub struct Preset {
    pub key: &'static str,
    pub label: &'static str,
    pub gain: f64,
    pub directional: bool,
    pub v_bw: f64,
    pub h_bw: f64,
    pub fb: f64,
}

pub const PRESETS: &[Preset] = &[
    Preset { key: "quarter_wave", label: "¼ wave ground plane", gain: 2.0, directional: false, v_bw: 80.0, h_bw: 0.0, fb: 0.0 },
    Preset { key: "dipole", label: "Half-wave dipole / J-pole", gain: 2.15, directional: false, v_bw: 78.0, h_bw: 0.0, fb: 0.0 },
    Preset { key: "collinear_2x5_8", label: "Collinear 2 × ⅝ (e.g. X-30)", gain: 4.5, directional: false, v_bw: 40.0, h_bw: 0.0, fb: 0.0 },
    Preset { key: "collinear_3x5_8", label: "Collinear 3 × ⅝ (e.g. X-50)", gain: 6.0, directional: false, v_bw: 30.0, h_bw: 0.0, fb: 0.0 },
    Preset { key: "collinear_big", label: "Long collinear (e.g. X-200/X-300)", gain: 8.0, directional: false, v_bw: 16.0, h_bw: 0.0, fb: 0.0 },
    Preset { key: "yagi_3", label: "3-element Yagi", gain: 7.5, directional: true, v_bw: 90.0, h_bw: 70.0, fb: 15.0 },
    Preset { key: "yagi_6", label: "6-element Yagi", gain: 11.0, directional: true, v_bw: 45.0, h_bw: 40.0, fb: 20.0 },
    Preset { key: "panel", label: "Panel / sector (90°)", gain: 12.0, directional: true, v_bw: 15.0, h_bw: 90.0, fb: 25.0 },
];

pub fn preset_label(key: &str) -> &'static str {
    PRESETS.iter().find(|p| p.key == key).map_or("Custom", |p| p.label)
}

/// One kind of receiving station drawn on the coverage map.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Receiver {
    pub name: String,
    pub enabled: bool,
    pub height_m: f64,
    pub gain_dbi: f64,
    /// feeder, body and clutter allowances
    pub loss_db: f64,
    pub required_dbm: f64,
    pub colour: [u8; 3],
    /// base level only: take height, gain, feeder and sensitivity from the site
    pub use_site_antenna: bool,
}

impl Default for Receiver {
    fn default() -> Self {
        Receiver { name: String::new(), enabled: true, height_m: 1.5, gain_dbi: 0.0, loss_db: 0.0, required_dbm: -113.0, colour: [79, 143, 247], use_site_antenna: false }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ModelSettings {
    pub radius_km: f64,
    /// 0 quick, 1 normal, 2 fine
    pub detail: u8,
    pub terrain_zoom: u32,
    pub climate: i32,
    pub ns: f64,
    pub eps: f64,
    pub sgm: f64,
    pub time_pct: f64,
    pub location_pct: f64,
    pub situation_pct: f64,
}

impl Default for ModelSettings {
    fn default() -> Self {
        ModelSettings { radius_km: 40.0, detail: 1, terrain_zoom: 11, climate: 6, ns: 301.0, eps: 15.0, sgm: 0.005, time_pct: 50.0, location_pct: 50.0, situation_pct: 50.0 }
    }
}

pub const GROUNDS: &[(&str, f64, f64)] = &[
    ("Average ground", 15.0, 0.005),
    ("Poor ground", 4.0, 0.001),
    ("Good ground", 25.0, 0.02),
    ("City / industrial", 5.0, 0.001),
    ("Fresh water", 81.0, 0.01),
    ("Sea water", 81.0, 5.0),
];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub handheld: Receiver,
    pub mobile: Receiver,
    pub base: Receiver,
    pub model: ModelSettings,
    pub opacity: f32,
    /// "light", "dark", or empty to follow the system
    pub theme: String,
    pub basemap: usize,
    pub grey_map: bool,
    /// map view at exit: lat, lon, zoom
    pub view: Option<(f64, f64, f64)>,
    /// site ids whose coverage is hidden
    pub hidden: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            handheld: Receiver { name: "Hand portable".into(), height_m: 1.5, gain_dbi: 0.0, loss_db: 6.0, required_dbm: -113.0, colour: [79, 143, 247], ..Default::default() },
            mobile: Receiver { name: "Mobile".into(), height_m: 1.5, gain_dbi: 3.0, loss_db: 1.0, required_dbm: -113.0, colour: [224, 64, 224], ..Default::default() },
            base: Receiver { name: "Base station".into(), height_m: 10.0, gain_dbi: 4.5, loss_db: 2.0, required_dbm: -116.0, colour: [63, 191, 95], use_site_antenna: true, ..Default::default() },
            model: ModelSettings::default(),
            opacity: 0.55,
            theme: String::new(),
            basemap: 0,
            grey_map: true,
            view: None,
            hidden: vec![],
        }
    }
}

impl Settings {
    /// The receive levels in drawing order (hand portable on top).
    pub fn levels(&self) -> [&Receiver; 3] {
        [&self.handheld, &self.mobile, &self.base]
    }
}

// ------------------------------------------------------------------ storage

pub fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("RADIOSITES_DATA_DIR") {
        return PathBuf::from(d);
    }
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("radio-sites")
}

fn read<T: for<'a> Deserialize<'a> + Default>(name: &str) -> T {
    std::fs::read_to_string(data_dir().join(name)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn write<T: Serialize>(name: &str, v: &T) -> Result<(), String> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Can't create {}: {e}", dir.display()))?;
    let p = dir.join(name);
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(v).unwrap()).map_err(|e| format!("Can't save {}: {e}", p.display()))?;
    std::fs::rename(&tmp, &p).map_err(|e| format!("Can't save {}: {e}", p.display()))
}

pub fn load_sites() -> Vec<Site> {
    read("sites.json")
}
pub fn save_sites(s: &[Site]) -> Result<(), String> {
    write("sites.json", &s)
}
pub fn load_settings() -> Settings {
    read("settings.json")
}
pub fn save_settings(s: &Settings) -> Result<(), String> {
    write("settings.json", s)
}
