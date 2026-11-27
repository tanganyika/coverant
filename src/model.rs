//! Sites, the antenna library, coverage plots (a site with an antenna and a radio),
//! the receive levels and the propagation settings, and where they are kept between runs:
//! ~/.local/share/coverant, %APPDATA%\coverant or
//! ~/Library/Application Support/coverant (`COVERANT_DATA_DIR` overrides it).

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
    /// the radio and antenna of older versions, read once to make the first plots
    #[serde(skip_serializing)]
    pub station: Option<Station>,
    /// download the terrain around the site when it is saved
    pub cache_terrain: bool,
    /// how far out (km); also the default radius of this site's coverage plots
    pub cache_radius_km: f64,
    /// older versions' radius chosen on the Coverage tab (now on each plot)
    #[serde(skip_serializing)]
    pub plot_radius_km: Option<f64>,
}

impl Default for Site {
    fn default() -> Self {
        Site {
            id: String::new(),
            name: "New site".into(),
            lat: 0.0,
            lon: 0.0,
            ground_m: None,
            notes: String::new(),
            colour: [211, 47, 47],
            station: None,
            cache_terrain: true,
            cache_radius_km: 40.0,
            plot_radius_km: None,
        }
    }
}

impl Site {
    /// Default radius for coverage plots from this site: the terrain download distance,
    /// else the general default.
    pub fn default_radius(&self, general_km: f64) -> f64 {
        if self.cache_terrain { self.cache_radius_km } else { general_km }
    }
}

/// An antenna in the library, independent of any site until a plot uses it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AntennaDef {
    pub id: String,
    pub name: String,
    pub preset: String,
    pub gain_dbi: f64,
    pub directional: bool,
    pub h_beamwidth: f64,
    pub front_back_db: f64,
    /// 0 = ignore the vertical pattern
    pub v_beamwidth: f64,
}

impl Default for AntennaDef {
    fn default() -> Self {
        let a = Antenna::default();
        AntennaDef {
            id: String::new(),
            name: preset_label(&a.preset).into(),
            preset: a.preset,
            gain_dbi: a.gain_dbi,
            directional: a.directional,
            h_beamwidth: a.h_beamwidth,
            front_back_db: a.front_back_db,
            v_beamwidth: a.v_beamwidth,
        }
    }
}

impl AntennaDef {
    pub fn apply_preset(&mut self, p: &Preset) {
        self.preset = p.key.into();
        self.gain_dbi = p.gain;
        self.directional = p.directional;
        self.v_beamwidth = p.v_bw;
        if p.directional {
            self.h_beamwidth = p.h_bw;
            self.front_back_db = p.fb;
        }
    }
    fn same_as(&self, o: &AntennaDef) -> bool {
        (self.preset.as_str(), self.gain_dbi, self.directional, self.h_beamwidth, self.front_back_db, self.v_beamwidth)
            == (o.preset.as_str(), o.gain_dbi, o.directional, o.h_beamwidth, o.front_back_db, o.v_beamwidth)
    }
}

/// A coverage plot: one site, one library antenna, and how it is installed and driven there.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PlotDef {
    pub id: String,
    pub name: String,
    pub site_id: String,
    pub antenna_id: String,
    pub height_m: f64,
    pub azimuth: f64,
    /// degrees, positive = down
    pub tilt: f64,
    pub freq_mhz: f64,
    pub power_w: f64,
    pub feeder_loss_db: f64,
    pub rx_sensitivity_dbm: f64,
    pub vertical: bool,
    /// None = the site's default radius
    pub radius_km: Option<f64>,
    /// a key from MODES; "custom" leaves each receive level's own "needs" alone
    pub mode: String,
    pub bandwidth_hz: f64,
    /// signal-to-noise ratio the mode needs, in that bandwidth
    pub snr_db: f64,
    /// the receiver's noise figure plus any local noise above thermal
    pub noise_figure_db: f64,
    /// buildings and trees around the hand portables and mobiles (dB)
    pub clutter_db: f64,
}

impl Default for PlotDef {
    fn default() -> Self {
        let st = Station::default();
        PlotDef {
            id: String::new(),
            name: String::new(),
            site_id: String::new(),
            antenna_id: String::new(),
            height_m: st.antenna.height_m,
            azimuth: 0.0,
            tilt: 0.0,
            freq_mhz: st.freq_mhz,
            power_w: st.power_w,
            feeder_loss_db: st.feeder_loss_db,
            rx_sensitivity_dbm: st.rx_sensitivity_dbm,
            vertical: true,
            radius_km: None,
            mode: MODES[0].key.into(),
            bandwidth_hz: MODES[0].bandwidth_hz,
            snr_db: MODES[0].snr_db,
            noise_figure_db: 6.0,
            clutter_db: SURROUNDINGS[1].1,
        }
    }
}

impl PlotDef {
    /// The radio and antenna as the coverage engine wants them.
    pub fn station(&self, a: &AntennaDef) -> Station {
        Station {
            freq_mhz: self.freq_mhz,
            power_w: self.power_w,
            feeder_loss_db: self.feeder_loss_db,
            rx_sensitivity_dbm: self.required_dbm().unwrap_or(self.rx_sensitivity_dbm),
            vertical: self.vertical,
            antenna: Antenna {
                preset: a.preset.clone(),
                gain_dbi: a.gain_dbi,
                height_m: self.height_m,
                directional: a.directional,
                azimuth: self.azimuth,
                h_beamwidth: a.h_beamwidth,
                front_back_db: a.front_back_db,
                tilt: self.tilt,
                v_beamwidth: a.v_beamwidth,
            },
        }
    }
    /// The signal every receiver needs for this plot's mode: thermal noise in the
    /// bandwidth, plus the noise figure, plus the SNR the mode needs. None for "custom".
    pub fn required_dbm(&self) -> Option<f64> {
        (self.mode != "custom").then(|| -174.0 + 10.0 * self.bandwidth_hz.max(1.0).log10() + self.noise_figure_db + self.snr_db)
    }
    pub fn surroundings_label(&self) -> String {
        SURROUNDINGS.iter().find(|s| s.1 == self.clutter_db).map_or(format!("Custom, {:.0} dB", self.clutter_db), |s| s.0.into())
    }
    pub fn mode_label(&self) -> String {
        match MODES.iter().find(|m| m.key == self.mode.trim_end_matches('*')) {
            Some(m) if self.mode.ends_with('*') => format!("{} (adjusted)", m.label),
            Some(m) => m.label.into(),
            None => "Custom levels".into(),
        }
    }
    pub fn set_mode(&mut self, m: &Mode) {
        self.mode = m.key.into();
        self.bandwidth_hz = m.bandwidth_hz;
        self.snr_db = m.snr_db;
    }
    pub fn radius(&self, site: &Site, general_km: f64) -> f64 {
        self.radius_km.unwrap_or(site.default_radius(general_km))
    }
}

/// Everything the user edits and saves.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Library {
    pub sites: Vec<Site>,
    pub antennas: Vec<AntennaDef>,
    pub plots: Vec<PlotDef>,
}

impl Library {
    pub fn site(&self, id: &str) -> Option<&Site> {
        self.sites.iter().find(|s| s.id == id)
    }
    pub fn antenna(&self, id: &str) -> Option<&AntennaDef> {
        self.antennas.iter().find(|a| a.id == id)
    }
    /// The area whose terrain belongs to a site: what it downloads and what its plots cover.
    pub fn terrain_area_km(&self, s: &Site, general_km: f64) -> f64 {
        let c = if s.cache_terrain { s.cache_radius_km } else { 0.0 };
        self.plots.iter().filter(|p| p.site_id == s.id).map(|p| p.radius(s, general_km)).fold(c, f64::max)
    }

    /// Turn the per-site radio and antenna of older versions into library antennas and plots
    /// (a plot keeps its site's id, so its saved coverage still belongs to it). True if anything changed.
    pub fn migrate(&mut self) -> bool {
        let mut changed = false;
        for i in 0..self.sites.len() {
            let Some(st) = self.sites[i].station.take() else { continue };
            changed = true;
            let site = &self.sites[i];
            let a = &st.antenna;
            let mut def = AntennaDef {
                id: new_id(),
                name: preset_label(&a.preset).into(),
                preset: a.preset.clone(),
                gain_dbi: a.gain_dbi,
                directional: a.directional,
                h_beamwidth: a.h_beamwidth,
                front_back_db: a.front_back_db,
                v_beamwidth: a.v_beamwidth,
            };
            if def.preset == "custom" || !PRESETS.iter().any(|p| p.key == def.preset) {
                def.name = format!("{} antenna", site.name);
            }
            let antenna_id = match self.antennas.iter().find(|x| x.same_as(&def)) {
                Some(x) => x.id.clone(),
                None => {
                    let id = def.id.clone();
                    self.antennas.push(def);
                    id
                }
            };
            if !self.plots.iter().any(|p| p.id == site.id) {
                self.plots.push(PlotDef {
                    id: site.id.clone(),
                    name: site.name.clone(),
                    site_id: site.id.clone(),
                    antenna_id,
                    height_m: a.height_m,
                    azimuth: a.azimuth,
                    tilt: a.tilt,
                    freq_mhz: st.freq_mhz,
                    power_w: st.power_w,
                    feeder_loss_db: st.feeder_loss_db,
                    rx_sensitivity_dbm: st.rx_sensitivity_dbm,
                    vertical: st.vertical,
                    radius_km: site.plot_radius_km,
                    // the old plots used each receive level's own "needs"
                    mode: "custom".into(),
                    ..Default::default()
                });
            }
            self.sites[i].plot_radius_km = None;
        }
        if self.antennas.is_empty() {
            self.antennas.push(AntennaDef { id: new_id(), ..Default::default() });
            changed = true;
        }
        changed
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

/// Operating modes and what they need to hold a contact: the receiver's noise bandwidth and
/// the signal-to-noise ratio in it.
pub struct Mode {
    pub key: &'static str,
    pub label: &'static str,
    pub bandwidth_hz: f64,
    pub snr_db: f64,
    pub note: &'static str,
}

pub const MODES: &[Mode] = &[
    Mode { key: "fm", label: "FM voice", bandwidth_hz: 11000.0, snr_db: 12.0, note: "about 12 dB SINAD: readable through some noise" },
    Mode { key: "fm_good", label: "FM voice, good copy", bandwidth_hz: 11000.0, snr_db: 20.0, note: "near full quieting, comfortable for a net" },
    Mode { key: "dv", label: "Digital voice (DMR, D-STAR, C4FM)", bandwidth_hz: 9000.0, snr_db: 10.0, note: "about 5 % bit errors: the edge before audio breaks up" },
    Mode { key: "ssb", label: "SSB voice", bandwidth_hz: 2400.0, snr_db: 10.0, note: "a workable SSB contact; 3 to 6 dB is just copyable" },
    Mode { key: "cw", label: "CW", bandwidth_hz: 500.0, snr_db: 6.0, note: "ear copy in a 500 Hz filter" },
    Mode { key: "ft8", label: "FT8", bandwidth_hz: 2500.0, snr_db: -20.0, note: "WSJT-X reports SNR in 2500 Hz; decodes reach about −21 dB" },
];

/// Allowances for what is around a receiver at 1.5 m, beyond what Longley-Rice's bare terrain
/// sees. Roughly the Okumura-Hata differences from open land, rounded down.
pub const SURROUNDINGS: &[(&str, f64)] = &[
    ("Open (moorland, sea, open fields)", 0.0),
    ("Rural (hedges, trees, villages)", 6.0),
    ("Suburban", 12.0),
    ("Urban (town centre)", 20.0),
];

/// Spread of the signal from place to place around a 1.5 m receiver (ITU-R P.1546).
pub const LOCATION_SIGMA_DB: f64 = 5.5;

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
    /// base level only: take height, gain, feeder and sensitivity from the plot
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
    /// share of spots at the edge of the plot where the signal is enough (a log-normal
    /// margin on top of Longley-Rice, which in point-to-point mode leaves this out)
    pub location_pct: f64,
    pub situation_pct: f64,
    /// extra loss for hand portables and mobiles from buildings and trees around them,
    /// set from the plot for each run
    #[serde(skip)]
    pub clutter_db: f64,
}

impl Default for ModelSettings {
    fn default() -> Self {
        ModelSettings { radius_km: 40.0, detail: 1, terrain_zoom: 11, climate: 6, ns: 301.0, eps: 15.0, sgm: 0.005, time_pct: 50.0, location_pct: 90.0, situation_pct: 50.0, clutter_db: 0.0 }
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
    /// draw every ticked plot at once rather than just the selected one
    pub show_all_plots: bool,
    /// map view at exit: lat, lon, zoom
    pub view: Option<(f64, f64, f64)>,
    /// site ids whose coverage is hidden
    pub hidden: Vec<String>,
    /// bumped when the defaults change, so older settings files are brought up to date
    /// (missing in the first version's files)
    #[serde(default)]
    pub version: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            handheld: Receiver { name: "Hand portable".into(), height_m: 1.5, gain_dbi: -5.0, loss_db: 4.0, required_dbm: -113.0, colour: [79, 143, 247], ..Default::default() },
            mobile: Receiver { name: "Mobile".into(), height_m: 1.5, gain_dbi: 3.0, loss_db: 1.0, required_dbm: -113.0, colour: [224, 64, 224], ..Default::default() },
            base: Receiver { name: "Base station".into(), height_m: 10.0, gain_dbi: 4.5, loss_db: 2.0, required_dbm: -116.0, colour: [63, 191, 95], use_site_antenna: true, ..Default::default() },
            model: ModelSettings::default(),
            opacity: 0.55,
            theme: String::new(),
            basemap: 0,
            grey_map: true,
            show_all_plots: false,
            view: None,
            hidden: vec![],
            version: SETTINGS_VERSION,
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
    if let Some(d) = std::env::var_os("COVERANT_DATA_DIR") {
        return PathBuf::from(d);
    }
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    adopt_old_folder(&base.join("radio-sites"), base.join("coverant"))
}

/// The program was called Radio Sites at first: move its folder to the new name once.
pub fn adopt_old_folder(old: &std::path::Path, new: PathBuf) -> PathBuf {
    if !new.exists() && old.is_dir() {
        let _ = std::fs::rename(old, &new);
    }
    new
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
const SETTINGS_VERSION: u32 = 2;

pub fn load_settings() -> Settings {
    let mut s: Settings = read("settings.json");
    if s.version < 2 {
        // version 1 was too kind to hand portables: a 0 dBi rubber duck, and the median spot
        let h = &mut s.handheld;
        if (h.gain_dbi, h.loss_db) == (0.0, 6.0) {
            h.gain_dbi = -5.0;
            h.loss_db = 4.0;
        }
        if s.model.location_pct == 50.0 {
            s.model.location_pct = 90.0;
        }
    }
    s.version = SETTINGS_VERSION;
    s
}
pub fn save_settings(s: &Settings) -> Result<(), String> {
    write("settings.json", s)
}

/// Sites, antennas and plots, with older files brought up to date (and saved again if they were).
pub fn load_library() -> Library {
    let mut lib = Library { sites: load_sites(), antennas: read("antennas.json"), plots: read("plots.json") };
    if lib.sites.iter().any(|s| s.station.is_some()) {
        let d = data_dir();
        let _ = std::fs::copy(d.join("sites.json"), d.join("sites-before-antenna-library.json"));
    }
    if lib.migrate() {
        let _ = save_library(&lib);
    }
    lib
}
pub fn save_library(l: &Library) -> Result<(), String> {
    write("antennas.json", &l.antennas)?;
    write("plots.json", &l.plots)?;
    write("sites.json", &l.sites)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_sites_become_antennas_and_plots() {
        let old = r#"[{"id":"a1","name":"Hill","lat":52.0,"lon":0.1,"plot_radius_km":30.0,
            "station":{"freq_mhz":433.5,"power_w":10.0,"antenna":{"preset":"yagi_6","gain_dbi":11.0,"height_m":15.0,"directional":true,"azimuth":90.0}}},
            {"id":"b2","name":"Tower","lat":52.1,"lon":0.2,"station":{"antenna":{"preset":"yagi_6","gain_dbi":11.0,"height_m":20.0,"directional":true}}}]"#;
        let mut lib = Library { sites: serde_json::from_str(old).unwrap(), ..Default::default() };
        assert!(lib.migrate());
        assert_eq!(lib.plots.len(), 2);
        assert_eq!(lib.plots[0].id, "a1");
        assert_eq!(lib.plots[0].freq_mhz, 433.5);
        assert_eq!(lib.plots[0].azimuth, 90.0);
        assert_eq!(lib.plots[0].radius_km, Some(30.0));
        assert_eq!(lib.plots[1].height_m, 20.0);
        // the same antenna at both sites is one library entry
        assert_eq!(lib.antennas.len(), 1);
        assert_eq!(lib.plots[0].antenna_id, lib.plots[1].antenna_id);
        let st = lib.plots[0].station(&lib.antennas[0]);
        assert_eq!(st.antenna.gain_dbi, 11.0);
        assert_eq!(st.antenna.height_m, 15.0);
        // sites are written without the old fields
        let j = serde_json::to_string(&lib.sites).unwrap();
        assert!(!j.contains("station") && !j.contains("plot_radius_km"));
        assert!(!lib.migrate());
    }
}

#[cfg(test)]
mod mode_tests {
    use super::*;

    #[test]
    fn mode_sets_required_signal() {
        let mut p = PlotDef::default();
        let fm = p.required_dbm().unwrap();
        assert!((fm - (-174.0 + 40.414 + 6.0 + 12.0)).abs() < 0.01, "{fm}");
        p.set_mode(MODES.iter().find(|m| m.key == "ft8").unwrap());
        let ft8 = p.required_dbm().unwrap();
        assert!(ft8 < fm - 30.0, "{ft8}");
        p.mode = "custom".into();
        assert!(p.required_dbm().is_none());
        assert_eq!(p.station(&AntennaDef::default()).rx_sensitivity_dbm, p.rx_sensitivity_dbm);
    }

    #[test]
    fn old_settings_files_have_version_zero() {
        let s: Settings = serde_json::from_str(r#"{"model":{"location_pct":50.0}}"#).unwrap();
        assert_eq!(s.version, 0);
        assert_eq!(Settings::default().version, SETTINGS_VERSION);
    }
}
