//! The main window: an icon rail (Sites, Antenna, Coverage, Links, Horizon), the
//! selected tab's panel, and the map.

use crate::coords;
use crate::coverage::{self, Coverage, Progress};
use crate::itm::CLIMATES;
use crate::map::{self, Projection, Tiles, View, BASEMAPS};
use crate::model::{self, AntennaDef, Library, PlotDef, Receiver, Settings, Site, GROUNDS, MODES, PRESETS, SURROUNDINGS};
use crate::terrain::{self, Terrain};
use eframe::egui::{self, Color32, RichText, Vec2};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver as Rx, channel};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tab {
    Sites,
    Antenna,
    Coverage,
    Links,
    Horizon,
}

impl Tab {
    const ALL: [Tab; 5] = [Tab::Sites, Tab::Antenna, Tab::Coverage, Tab::Links, Tab::Horizon];
    fn label(self) -> &'static str {
        match self {
            Tab::Sites => "Sites",
            Tab::Antenna => "Antenna library",
            Tab::Coverage => "Coverage",
            Tab::Links => "Links between sites (next)",
            Tab::Horizon => "Radio horizon (next)",
        }
    }
    fn short(self) -> &'static str {
        match self {
            Tab::Sites => "Sites",
            Tab::Antenna => "Antennas",
            Tab::Coverage => "Coverage",
            Tab::Links => "Links",
            Tab::Horizon => "Horizon",
        }
    }
    fn icon(self) -> &'static str {
        match self {
            Tab::Sites => "📍",
            Tab::Antenna => "📡",
            Tab::Coverage => "🗺",
            Tab::Links => "📈",
            Tab::Horizon => "🌄",
        }
    }
    pub fn from_name(s: &str) -> Option<Tab> {
        let s = s.to_ascii_lowercase();
        Tab::ALL.into_iter().find(|t| t.short().to_ascii_lowercase().trim_end_matches('s') == s.trim_end_matches('s'))
    }
}

fn narrow(ctx: &egui::Context) -> bool {
    ctx.content_rect().width() < 720.0
}

/// Terrain being downloaded after a Save.
struct CacheJob {
    done: Arc<std::sync::atomic::AtomicUsize>,
    total: usize,
    stop: Arc<AtomicBool>,
}

struct Job {
    plot_id: String,
    /// the plot as it was when the run started
    def: PlotDef,
    progress: Arc<Progress>,
    rx: Rx<Result<Coverage, String>>,
    started: std::time::Instant,
}

struct RunInputs {
    site: Site,
    st: model::Station,
    settings: Settings,
    mode: String,
    levels: Vec<coverage::Level>,
    eirp: f64,
    geometry: String,
}

/// A plot ready to draw.
struct Plot {
    cov: Coverage,
    tex: Option<egui::TextureHandle>,
}

/// The answer to a click on a plot.
struct Popup {
    lat: f64,
    lon: f64,
}

pub struct Shot {
    path: PathBuf,
    requested: bool,
    done: Arc<AtomicBool>,
}

pub struct App {
    lib: Library,
    saved: Library,
    settings: Settings,
    saved_settings: Settings,
    last_save: std::time::Instant,
    /// the site, library antenna and coverage plot being edited
    selected: Option<usize>,
    sel_ant: Option<usize>,
    sel_plot: Option<usize>,
    tab: Tab,
    view: View,
    tiles: Tiles,
    terrains: HashMap<u32, Arc<Terrain>>,
    /// finished coverage, by plot id
    results: HashMap<String, Plot>,
    /// map size last frame, to fit a finished plot
    map_size: Vec2,
    /// plots whose site, antenna or model changed since they ran
    stale: std::collections::HashSet<String>,
    job: Option<Job>,
    status: String,
    error: Option<String>,
    picking: bool,
    dragging_pin: bool,
    loc_text: String,
    popup: Option<Popup>,
    confirm_delete: bool,
    confirm_delete_ant: bool,
    confirm_delete_plot: bool,
    /// ground height from the terrain data, by position
    heights: HashMap<(i64, i64), Option<f64>>,
    pending_heights: std::collections::HashSet<(i64, i64)>,
    height_rx: Rx<((i64, i64), Option<f64>)>,
    height_tx: std::sync::mpsc::Sender<((i64, i64), Option<f64>)>,
    show_about: bool,
    cache_job: Option<CacheJob>,
    notes_tx: std::sync::mpsc::Sender<String>,
    notes_rx: Rx<String>,
    ask_quit: bool,
    quit_anyway: bool,
    frame: u64,
    threads: usize,
    shot: Option<Shot>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, tab: Option<Tab>, shot: Option<PathBuf>) -> Self {
        let settings = model::load_settings();
        crate::theme::install(&cc.egui_ctx, &settings.theme);
        let lib = model::load_library();
        let sites = &lib.sites;
        let view = match settings.view {
            Some((lat, lon, zoom)) => View { lat, lon, zoom },
            None if !sites.is_empty() => View { lat: sites[0].lat, lon: sites[0].lon, zoom: 9.0 },
            None => View { lat: 54.0, lon: -2.5, zoom: 6.0 },
        };
        let mut results = HashMap::new();
        for p in &lib.plots {
            if let Some(cov) = Coverage::load(&p.id) {
                results.insert(p.id.clone(), Plot { cov, tex: None });
            }
        }
        let (height_tx, height_rx) = channel();
        let (notes_tx, notes_rx) = channel();
        let threads = rayon::current_num_threads();
        let n = sites.len();
        let (na, np) = (lib.antennas.len(), lib.plots.len());
        App {
            saved: lib.clone(),
            lib,
            saved_settings: settings.clone(),
            settings,
            last_save: std::time::Instant::now(),
            selected: if n > 0 { Some(0) } else { None },
            sel_ant: if na > 0 { Some(0) } else { None },
            sel_plot: if np > 0 { Some(0) } else { None },
            tab: tab.unwrap_or(Tab::Sites),
            view,
            tiles: Tiles::new(&cc.egui_ctx),
            terrains: HashMap::new(),
            results,
            map_size: Vec2::new(900.0, 700.0),
            stale: Default::default(),
            job: None,
            status: if n == 0 {
                "Ready. Press “New site”, or right-click the map where your site is.".into()
            } else {
                format!("Ready. {n} site{} loaded.", if n == 1 { "" } else { "s" })
            },
            error: None,
            picking: false,
            dragging_pin: false,
            loc_text: String::new(),
            popup: None,
            confirm_delete: false,
            confirm_delete_ant: false,
            confirm_delete_plot: false,
            heights: HashMap::new(),
            pending_heights: Default::default(),
            height_rx,
            height_tx,
            show_about: false,
            cache_job: None,
            notes_tx,
            notes_rx,
            ask_quit: false,
            quit_anyway: false,
            frame: 0,
            threads,
            shot: shot.map(|path| Shot { path, requested: false, done: Arc::new(AtomicBool::new(false)) }),
        }
    }

    fn terrain(&mut self, zoom: u32) -> Arc<Terrain> {
        self.terrains.entry(zoom).or_insert_with(|| Arc::new(Terrain::new(zoom))).clone()
    }

    // ------------------------------------------------------------------ saving

    fn autosave(&mut self) {
        if self.last_save.elapsed().as_secs_f32() < 0.8 {
            return;
        }
        self.settings.view = Some((self.view.lat, self.view.lon, (self.view.zoom * 10.0).round() / 10.0));
        if self.settings != self.saved_settings {
            if model::save_settings(&self.settings).is_ok() {
                self.saved_settings = self.settings.clone();
            }
            self.last_save = std::time::Instant::now();
        }
    }

    fn unsaved(&self) -> bool {
        self.lib != self.saved
    }

    fn save(&mut self) {
        match model::save_library(&self.lib) {
            Ok(()) => {
                self.saved = self.lib.clone();
                let l = &self.lib;
                self.status = format!(
                    "Saved {} · {} · {}.",
                    plural(l.sites.len(), "site"),
                    plural(l.antennas.len(), "antenna"),
                    plural(l.plots.len(), "coverage plot")
                );
                self.cache_terrain();
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// Download, in the background, the terrain around every site that asks for it.
    fn cache_terrain(&mut self) {
        let zoom = self.settings.model.terrain_zoom;
        let areas: Vec<(String, f64, f64, f64)> =
            self.lib.sites.iter().filter(|s| s.cache_terrain).map(|s| (s.name.clone(), s.lat, s.lon, s.cache_radius_km * 1000.0)).collect();
        if areas.is_empty() {
            return;
        }
        if let Some(j) = &self.cache_job {
            j.stop.store(true, Ordering::Relaxed);
        }
        let total = areas.iter().map(|a| terrain::tiles_around(a.1, a.2, a.3, zoom).len()).sum();
        let done = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (d2, s2, tx) = (done.clone(), stop.clone(), self.notes_tx.clone());
        std::thread::spawn(move || {
            let mut fetched = 0;
            let mut failed = None;
            for (name, lat, lon, r) in &areas {
                match terrain::cache_area(*lat, *lon, *r, zoom, &d2, &s2) {
                    Ok((f, _)) => fetched += f,
                    Err(e) => failed = Some(format!("Terrain for {name} couldn't all be downloaded: {e}")),
                }
                if s2.load(Ordering::Relaxed) {
                    return;
                }
            }
            let _ = tx.send(failed.unwrap_or_else(|| {
                if fetched == 0 {
                    "Terrain for the saved sites is already cached.".into()
                } else {
                    format!("Terrain cached: {fetched} new tile{} downloaded.", if fetched == 1 { "" } else { "s" })
                }
            }));
        });
        self.cache_job = Some(CacheJob { done, total, stop });
    }

    fn undo_changes(&mut self) {
        fn keep<T>(v: &[T], old: Option<String>, id: impl Fn(&T) -> &str) -> Option<usize> {
            old.and_then(|o| v.iter().position(|x| id(x) == o)).or(if v.is_empty() { None } else { Some(0) })
        }
        let site = self.selected.map(|i| self.lib.sites[i].id.clone());
        let ant = self.sel_ant.map(|i| self.lib.antennas[i].id.clone());
        let plot = self.sel_plot.map(|i| self.lib.plots[i].id.clone());
        self.lib = self.saved.clone();
        self.selected = keep(&self.lib.sites, site, |s| &s.id);
        self.sel_ant = keep(&self.lib.antennas, ant, |a| &a.id);
        self.sel_plot = keep(&self.lib.plots, plot, |p| &p.id);
        self.results.retain(|id, _| self.saved.plots.iter().any(|p| &p.id == id));
        self.picking = false;
        self.status = "Changes undone: back to the last save.".into();
    }

    /// Save and Undo buttons, and whether anything is waiting to be saved.
    fn save_bar(&mut self, ui: &mut egui::Ui) {
        let unsaved = self.unsaved();
        ui.horizontal(|ui| {
            let save = egui::Button::new(RichText::new("💾 Save").color(if unsaved { Color32::WHITE } else { ui.visuals().weak_text_color() }).strong())
                .fill(if unsaved { crate::theme::ACCENT } else { ui.visuals().widgets.inactive.weak_bg_fill })
                .min_size(Vec2::new(90.0, 28.0));
            if ui.add_enabled(unsaved, save).on_hover_text("Save sites, antennas and coverage plots (Ctrl+S)").clicked() {
                self.save();
            }
            if ui.add_enabled(unsaved, egui::Button::new("↶ Undo changes")).on_hover_text("Go back to the last save").clicked() {
                self.undo_changes();
            }
            if unsaved {
                ui.label(RichText::new("● Unsaved changes").color(Color32::from_rgb(217, 119, 6)));
            } else {
                ui.label(RichText::new("✔ All saved").weak());
            }
        });
    }

    // ------------------------------------------------------------------ sites

    fn new_site(&mut self, lat: f64, lon: f64, pick: bool) {
        let n = self.lib.sites.len() + 1;
        let palette = [[211, 47, 47], [25, 118, 210], [56, 142, 60], [245, 124, 0], [123, 31, 162], [0, 151, 167]];
        let site = Site { id: model::new_id(), name: format!("Site {n}"), lat, lon, colour: palette[(n - 1) % palette.len()], ..Default::default() };
        self.lib.sites.push(site);
        self.selected = Some(self.lib.sites.len() - 1);
        self.tab = Tab::Sites;
        self.picking = pick;
        self.loc_text.clear();
        self.status = if pick { "Click the map where the new site is.".into() } else { "New site added. Drag its pin to adjust it.".into() };
    }

    /// Delete a plot and its coverage, from the saved files too.
    fn delete_plot(&mut self, id: &str) {
        self.lib.plots.retain(|p| p.id != id);
        self.saved.plots.retain(|p| p.id != id);
        Coverage::delete(id);
        self.results.remove(id);
        self.settings.hidden.retain(|h| h != id);
    }

    fn write_saved(&mut self) {
        if let Err(e) = model::save_library(&self.saved) {
            self.error = Some(e);
        }
    }

    fn delete_selected(&mut self) {
        if let Some(i) = self.selected {
            let general = self.settings.model.radius_km;
            let s = self.lib.sites[i].clone();
            let removed = vec![(s.lat, s.lon, self.lib.terrain_area_km(&s, general) * 1000.0)];
            let ids: Vec<String> = self.lib.plots.iter().filter(|p| p.site_id == s.id).map(|p| p.id.clone()).collect();
            for id in &ids {
                self.delete_plot(id);
            }
            self.lib.sites.remove(i);
            self.saved.sites.retain(|x| x.id != s.id);
            self.write_saved();
            self.fix_plot_selection();
            // the site's cached terrain, apart from what other sites still use
            let kept: Vec<(f64, f64, f64)> = self.lib.sites.iter().map(|k| (k.lat, k.lon, self.lib.terrain_area_km(k, general) * 1000.0)).collect();
            let tx = self.notes_tx.clone();
            let name = s.name.clone();
            std::thread::spawn(move || {
                let n = terrain::cleanup(&removed, &kept);
                if n > 0 {
                    let _ = tx.send(format!("Removed {n} cached terrain tile{} that only {name} used.", if n == 1 { "" } else { "s" }));
                }
            });
            self.selected = if self.lib.sites.is_empty() { None } else { Some(i.min(self.lib.sites.len() - 1)) };
            self.status = if ids.is_empty() { format!("Deleted {}.", s.name) } else { format!("Deleted {} and its {}.", s.name, plural(ids.len(), "coverage plot")) };
        }
    }

    fn fix_plot_selection(&mut self) {
        let n = self.lib.plots.len();
        self.sel_plot = if n == 0 { None } else { Some(self.sel_plot.unwrap_or(0).min(n - 1)) };
    }

    // ------------------------------------------------------------------ antennas

    fn new_antenna(&mut self, from: Option<usize>) {
        let a = match from {
            Some(i) => AntennaDef { id: model::new_id(), name: format!("{} (copy)", self.lib.antennas[i].name), ..self.lib.antennas[i].clone() },
            None => AntennaDef { id: model::new_id(), ..Default::default() },
        };
        self.lib.antennas.push(a);
        self.sel_ant = Some(self.lib.antennas.len() - 1);
        self.confirm_delete_ant = false;
    }

    fn delete_antenna(&mut self, i: usize) {
        let a = self.lib.antennas.remove(i);
        self.saved.antennas.retain(|x| x.id != a.id);
        self.write_saved();
        let n = self.lib.antennas.len();
        self.sel_ant = if n == 0 { None } else { Some(i.min(n - 1)) };
        self.status = format!("Deleted the antenna {}.", a.name);
    }

    // ------------------------------------------------------------------ plots

    /// A new plot: a copy of plot `from`, else the selected site with the selected antenna.
    fn new_plot(&mut self, from: Option<usize>) {
        let p = match from {
            Some(i) => PlotDef { id: model::new_id(), name: format!("{} (copy)", self.lib.plots[i].name), ..self.lib.plots[i].clone() },
            None => {
                let Some(si) = self.selected.or(if self.lib.sites.is_empty() { None } else { Some(0) }) else {
                    self.status = "Add a site first.".into();
                    return;
                };
                if self.lib.antennas.is_empty() {
                    self.new_antenna(None);
                }
                let a = &self.lib.antennas[self.sel_ant.unwrap_or(0)];
                let s = &self.lib.sites[si];
                PlotDef { id: model::new_id(), name: format!("{} · {}", s.name, a.name), site_id: s.id.clone(), antenna_id: a.id.clone(), ..Default::default() }
            }
        };
        self.lib.plots.push(p);
        self.sel_plot = Some(self.lib.plots.len() - 1);
        self.confirm_delete_plot = false;
    }

    fn set_location(&mut self, lat: f64, lon: f64) {
        if let Some(i) = self.selected {
            self.lib.sites[i].lat = lat;
            self.lib.sites[i].lon = lon;
        }
    }

    /// Terrain height at a point: Some(result) once known, None while it is looked up.
    fn ground_height(&mut self, lat: f64, lon: f64) -> Option<Option<f64>> {
        let key = ((lat * 1e5).round() as i64, (lon * 1e5).round() as i64);
        while let Ok((k, h)) = self.height_rx.try_recv() {
            self.pending_heights.remove(&k);
            self.heights.insert(k, h);
        }
        if let Some(h) = self.heights.get(&key) {
            return Some(*h);
        }
        if self.pending_heights.insert(key) {
            let tx = self.height_tx.clone();
            let t = self.terrain(12);
            std::thread::spawn(move || {
                let _ = tx.send((key, t.height_at(lat, lon).ok()));
            });
        }
        None
    }

    // ------------------------------------------------------------------ coverage

    /// What a run of plot `i` uses, as things stand now.
    fn run_inputs(&self, i: usize) -> Option<RunInputs> {
        let p = &self.lib.plots[i];
        let site = self.lib.site(&p.site_id)?.clone();
        let ant = self.lib.antenna(&p.antenna_id)?;
        let st = p.station(ant);
        let mut settings = self.settings.clone();
        settings.model.radius_km = p.radius(&site, self.settings.model.radius_km);
        if let Some(req) = p.required_dbm() {
            for r in [&mut settings.handheld, &mut settings.mobile, &mut settings.base] {
                r.required_dbm = req;
            }
        }
        settings.model.clutter_db = p.clutter_db;
        let mut mode = if p.mode == "custom" { String::new() } else { format!("{} · {:.0} dBm needed · ", p.mode_label(), p.required_dbm().unwrap_or(0.0)) };
        mode += &format!("{:.0} % of spots · {}", settings.model.location_pct, p.surroundings_label().split(" (").next().unwrap_or(""));
        let levels = coverage::levels(&st, &settings);
        let (a, m) = (&st.antenna, &settings.model);
        let geometry = format!(
            "{:.6} {:.6} {:?} | {} {} {} {} {} {} {} {} {} | {} {} {} {} {} {} {} {} {} | {:?}",
            site.lat, site.lon, site.ground_m,
            st.freq_mhz, st.vertical, a.height_m, a.directional, a.azimuth, a.h_beamwidth, a.front_back_db, a.tilt, a.v_beamwidth,
            m.radius_km, m.detail, m.terrain_zoom, m.climate, m.ns, m.eps, m.sgm, m.time_pct, m.situation_pct,
            levels.iter().map(|l| (&l.name, l.height_m)).collect::<Vec<_>>()
        );
        Some(RunInputs { eirp: st.eirp_dbm(), site, st, settings, mode, levels, geometry })
    }

    /// Keep finished plots in step with power, gain, loss and "needs" changes, which only
    /// shift the signal, and note which ones need running again.
    fn sync_results(&mut self, ctx: &egui::Context) {
        let busy_pointer = ctx.input(|i| i.pointer.any_down());
        self.stale.clear();
        for i in 0..self.lib.plots.len() {
            let id = self.lib.plots[i].id.clone();
            let Some(inp) = self.run_inputs(i) else { continue };
            let Some(r) = self.results.get_mut(&id) else { continue };
            if r.cov.meta.geometry.is_empty() || r.cov.meta.geometry != inp.geometry {
                self.stale.insert(id);
                continue;
            }
            if !busy_pointer && (r.cov.differs(inp.eirp, &inp.levels) || r.cov.meta.mode != inp.mode) {
                if r.cov.adjust(inp.eirp, &inp.levels) {
                    r.cov.meta.mode = inp.mode;
                    r.tex = None;
                    let _ = r.cov.save(&id);
                } else {
                    self.stale.insert(id);
                }
            }
        }
    }

    /// The plots drawn on the map, bottom first: the selected one, or every ticked one with
    /// the selected one on top.
    fn on_map(&self) -> Vec<usize> {
        let sel = self.sel_plot.filter(|&i| self.results.contains_key(&self.lib.plots[i].id));
        if !self.settings.show_all_plots {
            return sel.into_iter().collect();
        }
        let mut v: Vec<usize> = (0..self.lib.plots.len())
            .filter(|&i| Some(i) != sel && self.results.contains_key(&self.lib.plots[i].id) && !self.settings.hidden.contains(&self.lib.plots[i].id))
            .collect();
        v.extend(sel);
        v
    }

    /// After the site selection changes: edit that site's plot if the current one is elsewhere.
    fn follow_site(&mut self) {
        let Some(si) = self.selected else { return };
        let sid = &self.lib.sites[si].id;
        if self.sel_plot.is_some_and(|p| &self.lib.plots[p].site_id == sid) {
            return;
        }
        if let Some(p) = self.lib.plots.iter().position(|p| &p.site_id == sid) {
            self.sel_plot = Some(p);
            self.confirm_delete_plot = false;
        }
    }

    fn start_plot(&mut self) {
        let Some(i) = self.sel_plot else { return };
        if self.job.is_some() {
            return;
        }
        let p = self.lib.plots[i].clone();
        let Some(RunInputs { site, st, settings, mode, geometry, .. }) = self.run_inputs(i) else {
            self.status = "Choose a site and an antenna for this plot first.".into();
            return;
        };
        let terrain = self.terrain(settings.model.terrain_zoom);
        let progress = Progress::new();
        let (tx, rx) = channel();
        let p2 = progress.clone();
        std::thread::spawn(move || {
            let r = coverage::run(&site, &st, &settings, &terrain, &p2).map(|mut c| {
                c.meta.mode = mode;
                c.meta.geometry = geometry;
                c
            });
            let _ = tx.send(r);
        });
        self.status = format!("Plotting {} on {} threads…", p.name, self.threads);
        self.job = Some(Job { plot_id: p.id.clone(), def: p, progress, rx, started: std::time::Instant::now() });
    }

    /// Centre the map on a plot and zoom so all of it shows.
    fn fit_plot(&mut self, lat: f64, lon: f64, radius_km: f64) {
        let px = (self.map_size.x.min(self.map_size.y) as f64 - 60.0).max(200.0);
        let mpp = 2.0 * radius_km * 1000.0 / px;
        let world = 2.0 * std::f64::consts::PI * terrain::EARTH_R * lat.to_radians().cos();
        self.view.lat = lat;
        self.view.lon = lon;
        self.view.zoom = (world / (256.0 * mpp)).log2().clamp(3.0, 16.0);
    }

    fn poll(&mut self) {
        let Some(job) = &self.job else { return };
        match job.rx.try_recv() {
            Ok(Ok(cov)) => {
                let id = job.plot_id.clone();
                let pd = job.def.clone();
                if !self.lib.plots.iter().any(|p| p.id == id) {
                    self.job = None; // deleted while it ran
                    return;
                }
                if let Err(e) = cov.save(&id) {
                    self.error = Some(format!("The plot worked but couldn't be saved: {e}"));
                }
                // a plot run counts as saving that plot (and the site and antenna it uses)
                match self.saved.plots.iter_mut().find(|x| x.id == id) {
                    Some(x) => *x = pd.clone(),
                    None => self.saved.plots.push(pd.clone()),
                }
                if !self.saved.sites.iter().any(|s| s.id == pd.site_id) {
                    if let Some(s) = self.lib.site(&pd.site_id).cloned() {
                        self.saved.sites.push(s);
                    }
                }
                if !self.saved.antennas.iter().any(|a| a.id == pd.antenna_id) {
                    if let Some(a) = self.lib.antenna(&pd.antenna_id).cloned() {
                        self.saved.antennas.push(a);
                    }
                }
                self.write_saved();
                self.status = format!(
                    "{}: {} radials, {:.0} m steps, done in {:.1} s.",
                    pd.name, cov.meta.n_rad, cov.meta.step_m, cov.meta.seconds
                );
                self.fit_plot(cov.meta.site_lat, cov.meta.site_lon, cov.meta.radius_km);
                self.settings.hidden.retain(|h| h != &id);
                self.results.insert(id, Plot { cov, tex: None });
                self.job = None;
            }
            Ok(Err(e)) => {
                self.status = if e == "Stopped" { "Plot stopped.".into() } else { "Plot failed.".into() };
                if e != "Stopped" {
                    self.error = Some(e);
                }
                self.job = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(_) => {
                self.status = "Plot stopped.".into();
                self.job = None;
            }
        }
    }

    // ------------------------------------------------------------------ menus

    fn menu(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New site").clicked() {
                    self.new_site(self.view.lat, self.view.lon, true);
                    ui.close();
                }
                if ui.add_enabled(self.unsaved(), egui::Button::new("Save").shortcut_text("Ctrl+S")).clicked() {
                    self.save();
                    ui.close();
                }
                ui.separator();
                if ui.button("Open data folder").clicked() {
                    let _ = std::fs::create_dir_all(model::data_dir());
                    open_folder(&model::data_dir());
                    ui.close();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Map", |ui| {
                for (i, b) in BASEMAPS.iter().enumerate() {
                    ui.radio_value(&mut self.settings.basemap, i, b.name);
                }
                ui.separator();
                ui.checkbox(&mut self.settings.grey_map, "Grey map (makes the coverage stand out)");
                ui.separator();
                if ui.button("Zoom to all sites").clicked() {
                    self.zoom_to_sites();
                    ui.close();
                }
            });
            ui.menu_button("View", |ui| {
                ui.label("Theme");
                let mut changed = false;
                changed |= ui.radio_value(&mut self.settings.theme, String::new(), "Follow the system").changed();
                changed |= ui.radio_value(&mut self.settings.theme, "light".into(), "Light").changed();
                changed |= ui.radio_value(&mut self.settings.theme, "dark".into(), "Dark").changed();
                if changed {
                    crate::theme::apply_preference(ui.ctx(), &self.settings.theme);
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("About Coverant").clicked() {
                    self.show_about = true;
                    ui.close();
                }
            });
        });
    }

    fn zoom_to_sites(&mut self) {
        if self.lib.sites.is_empty() {
            return;
        }
        let (mut a, mut b, mut c, mut d) = (90.0f64, -90.0f64, 180.0f64, -180.0f64);
        for s in &self.lib.sites {
            a = a.min(s.lat);
            b = b.max(s.lat);
            c = c.min(s.lon);
            d = d.max(s.lon);
        }
        self.view.lat = (a + b) / 2.0;
        self.view.lon = (c + d) / 2.0;
        let span = (b - a).max((d - c) * 0.6).max(0.05);
        self.view.zoom = (8.0 - (span / 0.5).log2()).clamp(3.0, 13.0);
    }

    /// The tabs: a rail down the left of a wide window, a bar along the bottom of a phone.
    fn nav(&mut self, ui: &mut egui::Ui) {
        let frame = egui::Frame::NONE.fill(ui.visuals().window_fill).inner_margin(egui::Margin::symmetric(4, 6));
        if narrow(ui.ctx()) {
            egui::Panel::bottom("nav").frame(frame).show(ui, |ui| {
                let w = ui.available_width() / Tab::ALL.len() as f32;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for t in Tab::ALL {
                        if nav_item(ui, self.tab == t, t.icon(), t.short(), Vec2::new(w, 50.0)).clicked() {
                            self.tab = t;
                        }
                    }
                });
            });
        } else {
            egui::Panel::left("nav").resizable(false).exact_size(84.0).frame(frame).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                for t in Tab::ALL {
                    if nav_item(ui, self.tab == t, t.icon(), t.short(), Vec2::new(76.0, 58.0)).on_hover_text(t.label()).clicked() {
                        self.tab = t;
                    }
                }
            });
        }
    }

    // ------------------------------------------------------------------ tabs

    fn sites_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Sites");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new(RichText::new("＋ New site").color(Color32::WHITE)).fill(crate::theme::ACCENT)).on_hover_text("Then click the map, or type the location below").clicked() {
                    self.new_site(self.view.lat, self.view.lon, true);
                }
            });
        });
        if self.lib.sites.is_empty() {
            ui.label(RichText::new("No sites yet. Press “New site”, or right-click the map where a site is.").weak());
        }
        let mut select = None;
        let mut goto = None;
        for (i, s) in self.lib.sites.iter().enumerate() {
            let sel = self.selected == Some(i);
            let fill = if sel { ui.visuals().selection.bg_fill } else { ui.visuals().window_fill };
            let r = egui::Frame::new()
                .fill(fill)
                .stroke(if sel { egui::Stroke::new(1.0, ui.visuals().selection.stroke.color) } else { ui.visuals().widgets.noninteractive.bg_stroke })
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(10, 6))
                .outer_margin(egui::Margin { bottom: 4, ..Default::default() })
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(12.0), egui::Sense::hover());
                        ui.painter().circle_filled(r.center(), 6.0, Color32::from_rgb(s.colour[0], s.colour[1], s.colour[2]));
                        ui.label(RichText::new(&s.name).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(coords::to_maidenhead(s.lat, s.lon, 6)).monospace().weak());
                        });
                    });
                });
            let resp = ui.interact(r.response.rect, ui.make_persistent_id(("site", i)), egui::Sense::click());
            if resp.clicked() {
                select = Some(i);
            }
            if resp.double_clicked() {
                goto = Some(i);
            }
        }
        if let Some(i) = select {
            if self.selected != Some(i) {
                self.picking = false;
                self.loc_text.clear();
                self.confirm_delete = false;
            }
            self.selected = Some(i);
            self.follow_site();
        }
        if let Some(i) = goto {
            self.view.lat = self.lib.sites[i].lat;
            self.view.lon = self.lib.sites[i].lon;
            self.view.zoom = self.view.zoom.max(11.0);
        }
        let Some(i) = self.selected else { return };
        ui.add_space(6.0);
        let mut find = false;
        let mut delete = false;
        card(ui, "Site", |ui| {
            egui::Grid::new("site_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Name");
                ui.add(egui::TextEdit::singleline(&mut self.lib.sites[i].name).desired_width(f32::INFINITY));
                ui.end_row();
                ui.label("Colour");
                let s = &mut self.lib.sites[i];
                ui.color_edit_button_srgb(&mut s.colour);
                ui.end_row();
            });
            ui.add_space(4.0);
            ui.label("Location");
            ui.horizontal(|ui| {
                let te = ui.add(
                    egui::TextEdit::singleline(&mut self.loc_text)
                        .hint_text("52.2053, 0.1218 · 52°12'19\"N 0°07'18\"E · JO02bf · TL 450 585")
                        .desired_width(ui.available_width() - 44.0),
                );
                if te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    find = true;
                }
                if ui.button("Go").clicked() {
                    find = true;
                }
            });
            ui.label(RichText::new("Lat/long (decimal or degrees, minutes, seconds), a Maidenhead locator or an OS grid reference. Then click the map or drag the pin to put it exactly in place.").small().weak());
            ui.horizontal(|ui| {
                if ui.selectable_label(self.picking, "🎯 Pick on map").on_hover_text("Then click the map where the site is").clicked() {
                    self.picking = !self.picking;
                }
                if ui.button("Centre map here").clicked() {
                    self.view.lat = self.lib.sites[i].lat;
                    self.view.lon = self.lib.sites[i].lon;
                    self.view.zoom = self.view.zoom.max(12.0);
                }
            });
            ui.add_space(4.0);
            let (lat, lon) = (self.lib.sites[i].lat, self.lib.sites[i].lon);
            let d = coords::describe(lat, lon);
            let h = self.ground_height(lat, lon);
            egui::Grid::new("where").num_columns(2).spacing([10.0, 3.0]).striped(true).show(ui, |ui| {
                let row = |ui: &mut egui::Ui, k: &str, v: String| {
                    ui.label(RichText::new(k).weak());
                    ui.label(RichText::new(v).monospace());
                    ui.end_row();
                };
                row(ui, "Lat, long", d.decimal.clone());
                row(ui, "DMS", d.dms.clone());
                row(ui, "Locator", format!("{}  ({})", d.locator, d.locator10));
                if let Some(g) = &d.osgrid {
                    row(ui, "OS grid ref", g.clone());
                }
                row(ui, "Terrain height", match h {
                    Some(Some(h)) => format!("{h:.0} m above sea level"),
                    Some(None) => "unavailable".into(),
                    None => "looking up…".into(),
                });
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let s = &mut self.lib.sites[i];
                let mut manual = s.ground_m.is_some();
                if ui.checkbox(&mut manual, "Set ground height myself").changed() {
                    s.ground_m = if manual { Some(h.flatten().unwrap_or(0.0).round()) } else { None };
                }
                if let Some(g) = &mut s.ground_m {
                    ui.add(egui::DragValue::new(g).speed(0.5).suffix(" m").range(-50.0..=9000.0));
                }
            });
            ui.horizontal(|ui| {
                let s = &mut self.lib.sites[i];
                ui.checkbox(&mut s.cache_terrain, "Download terrain when saved").on_hover_text("Keeps the terrain around this site on disk, so plots work offline");
                if s.cache_terrain {
                    ui.label("out to");
                    ui.add(egui::DragValue::new(&mut s.cache_radius_km).speed(1.0).range(1.0..=250.0).suffix(" km"));
                }
            });
            if self.lib.sites[i].cache_terrain {
                ui.label(RichText::new("This distance is also the default radius of coverage plots from this site.").small().weak());
            }
            ui.label("Notes");
            ui.add(egui::TextEdit::multiline(&mut self.lib.sites[i].notes).desired_rows(2).desired_width(f32::INFINITY));
            ui.add_space(6.0);
            ui.separator();
            self.save_bar(ui);
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                if self.confirm_delete {
                    let n = self.lib.plots.iter().filter(|p| p.site_id == self.lib.sites[i].id).count();
                    ui.label(if n == 0 { format!("Delete {}?", self.lib.sites[i].name) } else { format!("Delete {} and its {}?", self.lib.sites[i].name, plural(n, "coverage plot")) });
                    if ui.button(RichText::new("Delete").color(Color32::from_rgb(198, 40, 40))).clicked() {
                        delete = true;
                    }
                    if ui.button("Keep").clicked() {
                        self.confirm_delete = false;
                    }
                } else if ui.button("🗑 Delete site").clicked() {
                    self.confirm_delete = true;
                }
            });
        });
        if find {
            match coords::parse_location(&self.loc_text) {
                Ok((lat, lon, kind)) => {
                    self.set_location(lat, lon);
                    self.view.lat = lat;
                    self.view.lon = lon;
                    self.view.zoom = self.view.zoom.max(13.0);
                    self.picking = false;
                    self.status = format!("Found the {}. Click the map or drag the pin to fine-tune.", kind.name());
                }
                Err(e) => self.status = e,
            }
        }
        if delete {
            self.confirm_delete = false;
            self.delete_selected();
        }
    }

    fn antenna_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Antennas");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new(RichText::new("＋ New antenna").color(Color32::WHITE)).fill(crate::theme::ACCENT)).clicked() {
                    self.new_antenna(None);
                }
            });
        });
        ui.label(RichText::new("Your antennas, kept apart from the sites. A coverage plot puts one of them on a site, at a height and bearing you choose there.").small().weak());
        ui.add_space(4.0);
        let mut select = None;
        for (i, a) in self.lib.antennas.iter().enumerate() {
            let used = self.lib.plots.iter().filter(|p| p.antenna_id == a.id).count();
            let right = format!("{:.1} dBi{}", a.gain_dbi, if a.directional { " · beam" } else { "" });
            let sub = if used == 0 { "not used yet".to_string() } else { format!("in {}", plural(used, "plot")) };
            if list_row(ui, ("ant", i), self.sel_ant == Some(i), None, &a.name, &sub, &right).clicked() {
                select = Some(i);
            }
        }
        if let Some(i) = select {
            if self.sel_ant != Some(i) {
                self.confirm_delete_ant = false;
            }
            self.sel_ant = Some(i);
        }
        let Some(i) = self.sel_ant else {
            ui.label(RichText::new("No antennas yet. Press “New antenna”.").weak());
            return;
        };
        ui.add_space(6.0);
        let mut delete = false;
        let mut copy = false;
        let users: Vec<String> = self.lib.plots.iter().filter(|p| p.antenna_id == self.lib.antennas[i].id).map(|p| p.name.clone()).collect();
        card(ui, "Antenna", |ui| {
            let a = &mut self.lib.antennas[i];
            egui::Grid::new("ant").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Name");
                ui.add(egui::TextEdit::singleline(&mut a.name).desired_width(f32::INFINITY));
                ui.end_row();
                ui.label("Type");
                egui::ComboBox::from_id_salt("preset").selected_text(model::preset_label(&a.preset)).width(220.0).show_ui(ui, |ui| {
                    for p in PRESETS {
                        if ui.selectable_label(a.preset == p.key, p.label).clicked() {
                            let old_label = model::preset_label(&a.preset);
                            let renamed = a.name == old_label || a.name.is_empty();
                            a.apply_preset(p);
                            if renamed {
                                a.name = p.label.into();
                            }
                        }
                    }
                    if ui.selectable_label(a.preset == "custom", "Custom").clicked() {
                        a.preset = "custom".into();
                    }
                });
                ui.end_row();
                let before = (a.gain_dbi, a.directional, a.v_beamwidth, a.h_beamwidth, a.front_back_db);
                ui.label("Gain");
                ui.add(egui::DragValue::new(&mut a.gain_dbi).speed(0.1).range(-20.0..=40.0).suffix(" dBi"));
                ui.end_row();
                ui.label("Pattern");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut a.directional, false, "Omni");
                    ui.radio_value(&mut a.directional, true, "Directional");
                });
                ui.end_row();
                if a.directional {
                    ui.label("Horizontal beamwidth");
                    ui.add(egui::DragValue::new(&mut a.h_beamwidth).speed(1.0).range(5.0..=360.0).suffix("°"));
                    ui.end_row();
                    ui.label("Front/back ratio");
                    ui.add(egui::DragValue::new(&mut a.front_back_db).speed(0.5).range(0.0..=40.0).suffix(" dB"));
                    ui.end_row();
                }
                ui.label("Vertical beamwidth");
                ui.add(egui::DragValue::new(&mut a.v_beamwidth).speed(1.0).range(0.0..=180.0).suffix("°"));
                ui.end_row();
                if before != (a.gain_dbi, a.directional, a.v_beamwidth, a.h_beamwidth, a.front_back_db) {
                    a.preset = "custom".into();
                }
            });
            ui.label(RichText::new("Vertical beamwidth sets how fast the gain falls away above and below the main lobe (0 ignores it). Height, bearing and down-tilt belong to each plot.").small().weak());
            if !users.is_empty() {
                ui.label(RichText::new(format!("Used by: {}", users.join(", "))).small());
            }
            ui.add_space(6.0);
            ui.separator();
            self.save_bar(ui);
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                if ui.button("⧉ Copy").on_hover_text("A new antenna starting from this one").clicked() {
                    copy = true;
                }
                if self.confirm_delete_ant {
                    if users.is_empty() {
                        ui.label(format!("Delete {}?", self.lib.antennas[i].name));
                        if ui.button(RichText::new("Delete").color(Color32::from_rgb(198, 40, 40))).clicked() {
                            delete = true;
                        }
                    } else {
                        ui.label(RichText::new(format!("Used by {}: give them another antenna first.", plural(users.len(), "plot"))).color(Color32::from_rgb(217, 119, 6)));
                    }
                    if ui.button("Keep").clicked() {
                        self.confirm_delete_ant = false;
                    }
                } else if ui.button("🗑 Delete antenna").clicked() {
                    self.confirm_delete_ant = true;
                }
            });
        });
        if copy {
            self.new_antenna(Some(i));
        }
        if delete {
            self.confirm_delete_ant = false;
            self.delete_antenna(i);
        }
    }

    fn coverage_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Coverage plots");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new(RichText::new("＋ New plot").color(Color32::WHITE)).fill(crate::theme::ACCENT)).on_hover_text("The selected site with the selected antenna").clicked() {
                    self.new_plot(None);
                }
            });
        });
        if self.lib.plots.is_empty() {
            ui.label(RichText::new("No plots yet. A plot puts one of your antennas on one of your sites. Press “New plot”.").weak());
        }
        // the site chosen on the Sites tab or the map has no plot yet
        if let Some(si) = self.selected {
            let s = &self.lib.sites[si];
            if !self.lib.plots.iter().any(|p| p.site_id == s.id) {
                let name = s.name.clone();
                let mut make = false;
                egui::Frame::new().fill(ui.visuals().faint_bg_color).corner_radius(8).inner_margin(8).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!("{name} has no coverage plot yet."));
                        if ui.button(format!("＋ Plot for {name}")).clicked() {
                            make = true;
                        }
                    });
                });
                ui.add_space(4.0);
                if make {
                    self.new_plot(None);
                }
            }
        }
        ui.checkbox(&mut self.settings.show_all_plots, "Show every ticked plot on the map, not just the selected one");
        let mut select = None;
        let mut toggle = None;
        let mut zoom = None;
        for (i, p) in self.lib.plots.iter().enumerate() {
            let site = self.lib.site(&p.site_id);
            let colour = site.map(|s| s.colour);
            let sub = format!(
                "{} · {}",
                site.map_or("no site", |s| s.name.as_str()),
                self.lib.antenna(&p.antenna_id).map_or("no antenna", |a| a.name.as_str())
            );
            let done = self.results.contains_key(&p.id);
            let stale = self.stale.contains(&p.id);
            let all = self.settings.show_all_plots;
            let mut shown = !self.settings.hidden.contains(&p.id);
            let sel = self.sel_plot == Some(i);
            let r = list_row_with(ui, ("plot", i), sel, colour, &p.name, &sub, |ui| {
                if done && all {
                    if ui.checkbox(&mut shown, "").on_hover_text("Show on the map").changed() {
                        toggle = Some((p.id.clone(), shown));
                    }
                }
                if !done {
                    ui.label(RichText::new("not run").small().weak());
                } else if stale {
                    ui.label(RichText::new("⚠ out of date").small().color(Color32::from_rgb(217, 119, 6)))
                        .on_hover_text("The site, antenna, height, frequency or model changed since this plot ran: press Plot coverage");
                }
            });
            if r.clicked() {
                select = Some(i);
            }
            if r.double_clicked() {
                zoom = Some(i);
            }
        }
        if let Some((id, shown)) = toggle {
            if shown {
                self.settings.hidden.retain(|h| h != &id);
            } else {
                self.settings.hidden.push(id);
            }
        }
        if let Some(i) = select {
            if self.sel_plot != Some(i) {
                self.confirm_delete_plot = false;
                zoom = Some(i);
            }
            self.sel_plot = Some(i);
            if let Some(si) = self.lib.sites.iter().position(|s| s.id == self.lib.plots[i].site_id) {
                self.selected = Some(si);
            }
        }
        if let Some(i) = zoom {
            if let Some(m) = self.results.get(&self.lib.plots[i].id).map(|r| (r.cov.meta.site_lat, r.cov.meta.site_lon, r.cov.meta.radius_km)) {
                self.fit_plot(m.0, m.1, m.2);
            }
        }
        let Some(i) = self.sel_plot else { return };
        ui.add_space(6.0);
        let busy = self.job.is_some();
        let stale_sel = self.stale.contains(&self.lib.plots[i].id);
        let done_sel = self.results.contains_key(&self.lib.plots[i].id);
        let general = self.settings.model.radius_km;
        let mut copy = false;
        let mut delete = false;
        card(ui, "Plot", |ui| {
            let lib = &mut self.lib;
            let p = &mut lib.plots[i];
            egui::Grid::new("plotdef").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Name");
                ui.add(egui::TextEdit::singleline(&mut p.name).desired_width(f32::INFINITY));
                ui.end_row();
                ui.label("Site");
                let cur = lib.sites.iter().find(|s| s.id == p.site_id).map_or("Choose…".to_string(), |s| s.name.clone());
                egui::ComboBox::from_id_salt("plot_site").selected_text(cur).width(220.0).show_ui(ui, |ui| {
                    for s in &lib.sites {
                        ui.selectable_value(&mut p.site_id, s.id.clone(), &s.name);
                    }
                });
                ui.end_row();
                ui.label("Antenna");
                let cur = lib.antennas.iter().find(|a| a.id == p.antenna_id).map_or("Choose…".to_string(), |a| a.name.clone());
                egui::ComboBox::from_id_salt("plot_ant").selected_text(cur).width(220.0).show_ui(ui, |ui| {
                    for a in &lib.antennas {
                        ui.selectable_value(&mut p.antenna_id, a.id.clone(), &a.name);
                    }
                });
                ui.end_row();
            });
        });
        let directional = self.lib.antenna(&self.lib.plots[i].antenna_id).is_some_and(|a| a.directional);
        let site = self.lib.site(&self.lib.plots[i].site_id).cloned();
        let ant = self.lib.antenna(&self.lib.plots[i].antenna_id).cloned();
        card(ui, "Installation and radio", |ui| {
            let p = &mut self.lib.plots[i];
            egui::Grid::new("radio").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Antenna height");
                ui.add(egui::DragValue::new(&mut p.height_m).speed(0.25).range(0.5..=3000.0).suffix(" m above ground"));
                ui.end_row();
                if directional {
                    ui.label("Pointing");
                    ui.add(egui::DragValue::new(&mut p.azimuth).speed(1.0).range(0.0..=359.9).suffix("° true"));
                    ui.end_row();
                }
                ui.label("Down-tilt");
                ui.add(egui::DragValue::new(&mut p.tilt).speed(0.25).range(-30.0..=30.0).suffix("°"));
                ui.end_row();
                ui.label("Frequency");
                ui.add(egui::DragValue::new(&mut p.freq_mhz).speed(0.025).range(20.0..=20000.0).suffix(" MHz").max_decimals(4));
                ui.end_row();
                ui.label("Transmit power");
                ui.add(egui::DragValue::new(&mut p.power_w).speed(0.5).range(0.01..=100000.0).suffix(" W"));
                ui.end_row();
                ui.label("Feeder loss");
                ui.add(egui::DragValue::new(&mut p.feeder_loss_db).speed(0.1).range(0.0..=30.0).suffix(" dB"));
                ui.end_row();
                ui.label("Polarisation");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut p.vertical, true, "Vertical");
                    ui.radio_value(&mut p.vertical, false, "Horizontal");
                });
                ui.end_row();
            });
            if let Some(a) = &ant {
                let st = p.station(a);
                let eirp = st.eirp_dbm();
                ui.label(RichText::new(format!("TX {:.1} dBm · EIRP {:.1} dBm ({}) · ERP {}", st.tx_dbm(), eirp, watts(eirp), watts(eirp - 2.15))).small().weak());
            }
        });
        card(ui, "Mode", |ui| {
            let p = &mut self.lib.plots[i];
            egui::Grid::new("mode").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Mode");
                egui::ComboBox::from_id_salt("mode").selected_text(p.mode_label()).width(240.0).show_ui(ui, |ui| {
                    for m in MODES {
                        if ui.selectable_label(p.mode == m.key, m.label).clicked() {
                            p.set_mode(m);
                        }
                    }
                    if ui.selectable_label(p.mode == "custom", "Custom levels").on_hover_text("Each receive level uses its own “Needs”").clicked() {
                        p.mode = "custom".into();
                    }
                });
                ui.end_row();
                if p.mode == "custom" {
                    ui.label("Base receiver needs");
                    ui.add(egui::DragValue::new(&mut p.rx_sensitivity_dbm).speed(0.5).range(-170.0..=-50.0).suffix(" dBm"));
                    ui.end_row();
                } else {
                    let before = (p.bandwidth_hz, p.snr_db);
                    ui.label("Bandwidth");
                    ui.add(egui::DragValue::new(&mut p.bandwidth_hz).speed(50.0).range(10.0..=200000.0).suffix(" Hz"));
                    ui.end_row();
                    ui.label("SNR needed");
                    ui.add(egui::DragValue::new(&mut p.snr_db).speed(0.5).range(-40.0..=40.0).suffix(" dB"));
                    ui.end_row();
                    ui.label("Noise figure");
                    ui.add(egui::DragValue::new(&mut p.noise_figure_db).speed(0.25).range(0.0..=40.0).suffix(" dB"))
                        .on_hover_text("The receiver's noise figure plus any local noise above thermal. Raise it for a noisy town site.");
                    ui.end_row();
                    // keep the mode's name only while its numbers are the standard ones
                    if before != (p.bandwidth_hz, p.snr_db) {
                        if let Some(m) = MODES.iter().find(|m| m.key == p.mode.trim_end_matches('*')) {
                            let std = (m.bandwidth_hz, m.snr_db) == (p.bandwidth_hz, p.snr_db);
                            p.mode = if std { m.key.into() } else { format!("{}*", m.key) };
                        }
                    }
                }
            });
            ui.add_space(4.0);
            egui::Grid::new("reliab").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Surroundings");
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("clutter").selected_text(p.surroundings_label()).width(200.0).show_ui(ui, |ui| {
                        for (n, v) in SURROUNDINGS {
                            ui.selectable_value(&mut p.clutter_db, *v, *n);
                        }
                    });
                    ui.add(egui::DragValue::new(&mut p.clutter_db).speed(0.5).range(0.0..=40.0).suffix(" dB"));
                })
                .response
                .on_hover_text("Buildings and trees around hand portables and mobiles, which Longley-Rice's bare terrain doesn't see");
                ui.end_row();
                ui.label("Spots covered");
                let m = &mut self.settings.model;
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut m.location_pct).speed(1.0).range(50.0..=99.0).suffix(" %"));
                    ui.label(RichText::new(format!("+{:.1} dB", coverage::location_margin(m.location_pct))).weak());
                })
                .response
                .on_hover_text("At the edge of each colour, the share of spots where the signal is enough. 50 % is the median; 90 % is usual for planning.");
                ui.end_row();
            });
            if let Some(req) = p.required_dbm() {
                let floor = req - p.snr_db;
                ui.label(RichText::new(format!("Every receiver needs {req:.1} dBm ({:.2} µV pd): noise floor {floor:.1} dBm + {:.0} dB SNR.", dbm_to_uv(req), p.snr_db)).strong());
                if let Some(m) = MODES.iter().find(|m| m.key == p.mode.trim_end_matches('*')) {
                    ui.label(RichText::new(m.note).small().weak());
                }
            } else {
                ui.label(RichText::new("Each receive level below uses its own “Needs”.").small().weak());
            }
        });
        card(ui, "Run", |ui| {
            let m = &mut self.settings.model;
            let p = &mut self.lib.plots[i];
            egui::Grid::new("plot").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Radius");
                ui.horizontal(|ui| {
                    let def = site.as_ref().map_or(general, |s| s.default_radius(general));
                    let mut r = p.radius_km.unwrap_or(def);
                    if ui.add(egui::DragValue::new(&mut r).speed(1.0).range(2.0..=250.0).suffix(" km")).changed() {
                        p.radius_km = Some(r);
                    }
                    if p.radius_km.is_some_and(|x| x != def) {
                        if ui.small_button("↺").on_hover_text(format!("Back to the site's default ({def:.0} km)")).clicked() {
                            p.radius_km = None;
                        }
                    }
                });
                ui.end_row();
                ui.label("Detail");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut m.detail, 0, "Quick");
                    ui.radio_value(&mut m.detail, 1, "Normal");
                    ui.radio_value(&mut m.detail, 2, "Fine");
                });
                ui.end_row();
                ui.label("Terrain data");
                egui::ComboBox::from_id_salt("dem").selected_text(dem_label(m.terrain_zoom)).show_ui(ui, |ui| {
                    for z in [10, 11, 12] {
                        ui.selectable_value(&mut m.terrain_zoom, z, dem_label(z));
                    }
                });
                ui.end_row();
                ui.label("Overlay opacity");
                ui.add(egui::Slider::new(&mut self.settings.opacity, 0.1..=1.0).show_value(false));
                ui.end_row();
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if busy {
                    let job = self.job.as_ref().unwrap();
                    if ui.button("■ Stop").clicked() {
                        job.progress.cancel.store(true, Ordering::Relaxed);
                    }
                    ui.add(
                        egui::ProgressBar::new(job.progress.fraction())
                            .text(format!("{} · {:.0} s", job.progress.stage.get(), job.started.elapsed().as_secs_f32()))
                            .animate(true),
                    );
                } else if ui
                    .add_enabled(site.is_some() && ant.is_some(), egui::Button::new(RichText::new("▶ Plot coverage").color(Color32::WHITE).strong()).fill(crate::theme::ACCENT).min_size(Vec2::new(150.0, 30.0)))
                    .clicked()
                {
                    self.start_plot();
                }
            });
            if !busy {
                if stale_sel {
                    ui.label(RichText::new("⚠ The site, antenna, height, frequency or model changed since this plot ran. Press Plot coverage to update it.").small().color(Color32::from_rgb(217, 119, 6)));
                } else if done_sel {
                    ui.label(RichText::new("Power, feeder loss, gains, losses, mode, surroundings and spots covered update the map straight away.").small().weak());
                }
            }
            ui.add_space(4.0);
            ui.separator();
            self.save_bar(ui);
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                if ui.button("⧉ Copy").on_hover_text("A new plot starting from this one: try another antenna, or the same one at another site").clicked() {
                    copy = true;
                }
                if self.confirm_delete_plot {
                    ui.label(format!("Delete {}?", self.lib.plots[i].name));
                    if ui.button(RichText::new("Delete").color(Color32::from_rgb(198, 40, 40))).clicked() {
                        delete = true;
                    }
                    if ui.button("Keep").clicked() {
                        self.confirm_delete_plot = false;
                    }
                } else if ui.button("🗑 Delete plot").clicked() {
                    self.confirm_delete_plot = true;
                }
            });
        });
        if copy {
            self.new_plot(Some(i));
            return;
        }
        if delete {
            self.confirm_delete_plot = false;
            let p = self.lib.plots[i].clone();
            self.delete_plot(&p.id);
            self.write_saved();
            self.fix_plot_selection();
            self.status = format!("Deleted the plot {}.", p.name);
            return;
        }
        card(ui, "Receive levels", |ui| {
            ui.label(RichText::new("Each colour shows where that kind of station hears the site at or above the signal it needs. Hand portables are drawn on top.").small().weak());
            let st = ant.as_ref().map(|a| self.lib.plots[i].station(a)).unwrap_or_default();
            let needs = self.lib.plots[i].required_dbm();
            egui::Grid::new("levels").num_columns(6).spacing([6.0, 4.0]).show(ui, |ui| {
                for h in ["", "Station", "Height", "Gain", "Losses", "Needs"] {
                    ui.label(RichText::new(h).small().weak());
                }
                ui.end_row();
                let s = &mut self.settings;
                for (k, r) in [&mut s.handheld, &mut s.mobile, &mut s.base].into_iter().enumerate() {
                    level_row(ui, r, k == 2, &st, needs);
                }
            });
            ui.checkbox(&mut self.settings.base.use_site_antenna, "Base station uses this plot's antenna, height, feeder and receiver");
            let (c, lm) = (self.lib.plots[i].clutter_db, coverage::location_margin(self.settings.model.location_pct));
            ui.label(RichText::new(format!("Each run adds {lm:.1} dB to every level for {:.0} % of spots, and {c:.0} dB more to hand portables and mobiles for their surroundings. Losses here are the body (hand portable) and feeder. A typical rubber duck is about −5 dBi.", self.settings.model.location_pct)).small().weak());
        });
        egui::CollapsingHeader::new("Propagation model (Longley-Rice)").show(ui, |ui| {
            let m = &mut self.settings.model;
            egui::Grid::new("itm").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Climate");
                let cur = CLIMATES.iter().find(|c| c.0 as i32 == m.climate).map_or("?", |c| c.1);
                egui::ComboBox::from_id_salt("clim").selected_text(cur).width(220.0).show_ui(ui, |ui| {
                    for (k, n) in CLIMATES {
                        ui.selectable_value(&mut m.climate, k as i32, n);
                    }
                });
                ui.end_row();
                ui.label("Ground");
                let cur = GROUNDS.iter().find(|g| g.1 == m.eps && g.2 == m.sgm).map_or("Custom", |g| g.0);
                egui::ComboBox::from_id_salt("gnd").selected_text(cur).show_ui(ui, |ui| {
                    for g in GROUNDS {
                        if ui.selectable_label(g.1 == m.eps && g.2 == m.sgm, g.0).clicked() {
                            m.eps = g.1;
                            m.sgm = g.2;
                        }
                    }
                });
                ui.end_row();
                ui.label("Surface refractivity");
                ui.add(egui::DragValue::new(&mut m.ns).speed(1.0).range(250.0..=400.0).suffix(" N"));
                ui.end_row();
                for (lbl, v) in [("Time", &mut m.time_pct), ("Situation", &mut m.situation_pct)] {
                    ui.label(lbl);
                    ui.add(egui::DragValue::new(v).speed(1.0).range(1.0..=99.0).suffix(" %"));
                    ui.end_row();
                }
            });
            ui.label(RichText::new("50 % / 50 % / 50 % gives the median prediction. Raise them for a more cautious plot.").small().weak());
        });
        if let Some(p) = self.results.get(&self.lib.plots[i].id) {
            let m = &p.cov.meta;
            card(ui, "Last run", |ui| {
                for s in &m.stats {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), egui::Sense::hover());
                        ui.painter().rect_filled(r, 3.0, Color32::from_rgb(s.colour[0], s.colour[1], s.colour[2]));
                        ui.label(format!("{}: {:.0} km²", s.name, s.area_km2));
                    });
                }
                ui.label(
                    RichText::new(format!(
                        "EIRP {:.1} dBm · ground {:.0} m · {} radials of {:.0} m steps · terrain ~{:.0} m · {:.1} s",
                        m.eirp_dbm, m.site_ground_m, m.n_rad, m.step_m, m.dem_res_m, m.seconds
                    ))
                    .small()
                    .weak(),
                );
                ui.label(RichText::new("Click the map for the signal at any point.").small().weak());
            });
        }
    }

    fn later_tab(&mut self, ui: &mut egui::Ui, what: Tab) {
        ui.heading(what.label().trim_end_matches(" (next)"));
        ui.add_space(6.0);
        card(ui, "Coming next", |ui| {
            match what {
                Tab::Links => ui.label("Path profiles between two or more sites: the terrain between them, the Fresnel zone, and the predicted signal and margin, from the same terrain and Longley-Rice engine as the coverage plots."),
                _ => ui.label("The radio horizon from a site: how far it sees on each bearing, drawn on the map."),
            };
        });
    }

    // ------------------------------------------------------------------ map

    fn map_panel(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        // starting a drag on the selected pin moves the pin, not the map
        let pointer = ctx.input(|i| i.pointer.interact_pos());
        let pressed = ctx.input(|i| i.pointer.primary_pressed());
        let (resp, proj) = map::show(ui, &mut self.view, &mut self.tiles, self.settings.basemap, self.settings.grey_map, !self.dragging_pin);
        let painter = ui.painter_at(proj.rect);

        // coverage overlays, left off while sites are being placed so the map can be read
        let show_cov = self.tab != Tab::Sites;
        if !show_cov {
            self.popup = None;
        }
        let tint = Color32::from_white_alpha((self.settings.opacity * 255.0) as u8);
        self.map_size = proj.rect.size();
        let shown = if show_cov { self.on_map() } else { vec![] };
        for &pi in &shown {
            let s = &self.lib.plots[pi];
            let Some(p) = self.results.get_mut(&s.id) else { continue };
            let m = &p.cov.meta;
            let tex = p.tex.get_or_insert_with(|| {
                let img = egui::ColorImage::from_rgba_unmultiplied([m.img_w as usize, m.img_h as usize], p.cov.image.as_raw());
                ctx.load_texture(format!("cov{}", s.id), img, egui::TextureOptions::NEAREST)
            });
            let r = proj.global_rect(m.img_x0, m.img_y0, m.img_x0 + m.img_w as f64, m.img_y0 + m.img_h as f64, m.img_zoom);
            painter.image(tex.id(), r, egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0)), tint);
        }

        // pins
        let mut pin_rects = vec![];
        for (i, s) in self.lib.sites.iter().enumerate() {
            let tip = proj.to_screen(s.lat, s.lon);
            let sel = self.selected == Some(i);
            let r = map::pin(&painter, tip, Color32::from_rgb(s.colour[0], s.colour[1], s.colour[2]), sel);
            let g = painter.layout_no_wrap(s.name.clone(), egui::FontId::proportional(12.0), crate::theme::ink());
            let lr = egui::Rect::from_min_size(tip + Vec2::new(11.0, -30.0), g.size() + Vec2::new(8.0, 3.0));
            painter.rect_filled(lr, 4.0, crate::theme::label_bg());
            painter.galley(lr.min + Vec2::new(4.0, 1.5), g, crate::theme::ink());
            pin_rects.push((i, r));
        }
        if pressed {
            if let (Some(p), Some(sel)) = (pointer, self.selected) {
                self.dragging_pin = pin_rects.iter().any(|(i, r)| *i == sel && r.expand(4.0).contains(p));
            }
        }
        if self.dragging_pin {
            if resp.dragged() {
                if let Some(p) = resp.interact_pointer_pos() {
                    let (lat, lon) = proj.to_latlon(p + Vec2::new(0.0, 0.0));
                    self.set_location(lat, lon);
                }
            }
            if !ctx.input(|i| i.pointer.primary_down()) {
                self.dragging_pin = false;
            }
        }
        if self.picking {
            ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
        }

        // clicks
        if resp.clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                let (lat, lon) = proj.to_latlon(p);
                if self.picking && self.selected.is_some() {
                    self.set_location(lat, lon);
                    self.picking = false;
                    self.status = "Site placed. Drag the pin to fine-tune it.".into();
                } else if let Some((i, _)) = pin_rects.iter().rev().find(|(_, r)| r.contains(p)) {
                    self.selected = Some(*i);
                    self.follow_site();
                    self.popup = None;
                } else if show_cov {
                    self.popup = Some(Popup { lat, lon });
                }
            }
        }
        if resp.secondary_clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                let (lat, lon) = proj.to_latlon(p);
                self.new_site(lat, lon, false);
            }
        }
        self.popup_window(&ctx, &proj);
        let legend_levels = shown.last().and_then(|&i| self.results.get(&self.lib.plots[i].id)).map(|p| {
            let name = &self.lib.plots[shown[shown.len() - 1]].name;
            (if p.cov.meta.mode.is_empty() { name.clone() } else { format!("{name} · {}", p.cov.meta.mode) }, p.cov.meta.levels.clone())
        });
        let mut legend_levels = legend_levels;
        if !show_cov {
            legend_levels = None;
            if !self.results.is_empty() {
                let painter = ui.painter_at(proj.rect);
                let g = painter.layout_no_wrap("Coverage is hidden while you edit sites".into(), egui::FontId::proportional(12.0), crate::theme::ink_soft());
                let r = egui::Rect::from_min_size(proj.rect.right_top() + Vec2::new(-g.size().x - 22.0, 10.0), g.size() + Vec2::new(12.0, 6.0));
                painter.rect_filled(r, 6.0, crate::theme::label_bg());
                painter.galley(r.min + Vec2::new(6.0, 3.0), g, crate::theme::ink_soft());
            }
        }
        if let Some(levels) = legend_levels {
            legend(ui, &proj, &levels.0, &levels.1);
        }
        map::decorations(ui, &proj, &mut self.view, self.settings.basemap, " · Elevation: Mapzen/AWS Terrain Tiles", self.tiles.offline);
        let _ = terrain::ATTRIBUTION;
    }

    fn popup_window(&mut self, ctx: &egui::Context, proj: &Projection) {
        let Some(pp) = &self.popup else { return };
        let (lat, lon) = (pp.lat, pp.lon);
        let at = proj.to_screen(lat, lon);
        if !proj.rect.contains(at) {
            return;
        }
        let mut infos = vec![];
        for i in self.on_map().into_iter().rev() {
            let s = &self.lib.plots[i];
            if let Some(p) = self.results.get(&s.id) {
                if let Some(q) = p.cov.query(lat, lon) {
                    infos.push((s.name.clone(), q));
                }
            }
        }
        let mut close = false;
        egui::Area::new(egui::Id::unique("map_popup")).fixed_pos(at + Vec2::new(10.0, 10.0)).order(egui::Order::Foreground).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(coords::describe(lat, lon).locator10).monospace().strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("✕").clicked() {
                            close = true;
                        }
                    });
                });
                if infos.is_empty() {
                    ui.label(RichText::new("No coverage plot reaches here.").weak());
                }
                for (name, q) in &infos {
                    ui.separator();
                    ui.label(RichText::new(name).strong());
                    ui.label(RichText::new(format!("{:.2} km at {:.0}° · ground {:.0} m · {}", q.distance_km, q.bearing, q.ground_m, q.mode)).small().weak());
                    egui::Grid::new(("pq", name)).num_columns(4).spacing([12.0, 2.0]).show(ui, |ui| {
                        for h in ["", "dBm", "µV", "margin"] {
                            ui.label(RichText::new(h).small().weak());
                        }
                        ui.end_row();
                        for (n, c, s, _l, mg) in &q.levels {
                            ui.horizontal(|ui| {
                                let (r, _) = ui.allocate_exact_size(Vec2::splat(10.0), egui::Sense::hover());
                                ui.painter().rect_filled(r, 2.0, Color32::from_rgb(c[0], c[1], c[2]));
                                ui.label(n);
                            });
                            ui.label(RichText::new(format!("{s:.1}")).monospace());
                            let uv = dbm_to_uv(*s);
                            ui.label(RichText::new(if uv < 10.0 { format!("{uv:.2}") } else { format!("{uv:.0}") }).monospace());
                            let col = if *mg >= 0.0 { Color32::from_rgb(21, 128, 61) } else { Color32::from_rgb(185, 28, 28) };
                            ui.label(RichText::new(format!("{mg:+.1} dB")).monospace().color(col));
                            ui.end_row();
                        }
                    });
                }
            });
        });
        if close {
            self.popup = None;
        }
    }

    fn about(&mut self, ctx: &egui::Context) {
        let mut open = self.show_about;
        egui::Window::new("About Coverant").open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
            ui.heading(format!("Coverant {}", env!("CARGO_PKG_VERSION")));
            ui.label("Coverage planning for radio sites: Longley-Rice (ITM 1.2.2) over real terrain.");
            ui.add_space(6.0);
            ui.label(RichText::new(terrain::ATTRIBUTION).small());
            ui.label(RichText::new("Maps © OpenStreetMap contributors, OpenTopoMap (CC-BY-SA), Esri.").small());
            ui.add_space(6.0);
            ui.label(RichText::new(format!("Sites are kept in {}", model::data_dir().display())).small().weak());
        });
        self.show_about = open;
    }

    fn screenshot(&mut self, ctx: &egui::Context) {
        let Some(shot) = &mut self.shot else { return };
        ctx.request_repaint();
        if shot.done.load(Ordering::Relaxed) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if self.frame >= 90 && !shot.requested && self.job.is_none() {
            shot.requested = true;
            let path = shot.path.clone();
            let done = shot.done.clone();
            let c2 = ctx.clone();
            ctx.request_screenshot(move |img| {
                let [w, h] = img.size;
                let mut buf = Vec::with_capacity(w * h * 4);
                for p in &img.pixels {
                    buf.extend_from_slice(&p.to_srgba_unmultiplied());
                }
                if let Some(im) = image::RgbaImage::from_raw(w as u32, h as u32, buf) {
                    let _ = im.save(&path);
                }
                done.store(true, Ordering::Relaxed);
                c2.request_repaint();
            });
        }
    }

    /// For automated tests: choose a plot, and its site, as a click on the list does.
    pub fn select_plot(&mut self, i: usize) {
        if i < self.lib.plots.len() {
            self.sel_plot = Some(i);
            self.selected = self.lib.sites.iter().position(|s| s.id == self.lib.plots[i].site_id);
        }
    }

    /// For automated tests: press Save at start-up.
    pub fn save_on_start(&mut self) {
        self.save();
    }

    /// For automated screenshots: plot the selected site first.
    pub fn plot_on_start(&mut self) {
        self.start_plot();
    }

    /// For automated screenshots: the signal popup at a point.
    pub fn popup_at(&mut self, lat: f64, lon: f64) {
        self.popup = Some(Popup { lat, lon });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        crate::theme::frame(&ctx);
        self.frame += 1;
        self.poll();
        self.sync_results(&ctx);
        while let Ok(n) = self.notes_rx.try_recv() {
            self.status = n;
            if !self.status.starts_with("Removed") {
                self.cache_job = None;
            }
        }
        if self.cache_job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        if self.job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        }
        self.screenshot(&ctx);
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S)) && self.unsaved() {
            self.save();
        }
        if ctx.input(|i| i.viewport().close_requested()) && self.unsaved() && !self.quit_anyway {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.ask_quit = true;
        }
        egui::Panel::top("menu").show(ui, |ui| self.menu(ui));
        if narrow(&ctx) {
            self.nav(ui);
        }
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("⚙ {} threads", self.threads)).color(Color32::from_rgb(60, 90, 160)));
                if let Some(j) = &self.cache_job {
                    ui.separator();
                    let d = j.done.load(Ordering::Relaxed).min(j.total);
                    ui.add(egui::ProgressBar::new(d as f32 / j.total.max(1) as f32).desired_width(150.0).text(format!("⬇ terrain {d}/{}", j.total)))
                        .on_hover_text("Downloading the terrain around the saved sites");
                }
                ui.separator();
                if let Some(i) = self.selected {
                    let s = &self.lib.sites[i];
                    ui.label(RichText::new(format!("{}  {}", s.name, coords::to_maidenhead(s.lat, s.lon, 6))).strong());
                    ui.separator();
                }
                if self.unsaved() {
                    ui.label(RichText::new("● Unsaved").color(Color32::from_rgb(217, 119, 6))).on_hover_text("Press Save (Ctrl+S) to keep your changes");
                    ui.separator();
                }
                ui.add(egui::Label::new(&self.status).truncate());
            });
        });
        if !narrow(&ctx) {
            self.nav(ui);
        }
        egui::Panel::left("side").resizable(true).default_size(380.0).size_range(320.0..=560.0).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.add_space(6.0);
                match self.tab {
                    Tab::Sites => self.sites_tab(ui),
                    Tab::Antenna => self.antenna_tab(ui),
                    Tab::Coverage => self.coverage_tab(ui),
                    t => self.later_tab(ui, t),
                }
                ui.add_space(10.0);
            });
        });
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| self.map_panel(ui));
        self.about(&ctx);
        if self.ask_quit {
            egui::Window::new("Unsaved changes").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO).show(&ctx, |ui| {
                ui.label("Your sites have changes that haven't been saved.");
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(RichText::new("Save and quit").color(Color32::WHITE)).fill(crate::theme::ACCENT)).clicked() {
                        self.save();
                        if !self.unsaved() {
                            self.quit_anyway = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        self.ask_quit = false;
                    }
                    if ui.button("Quit without saving").clicked() {
                        self.quit_anyway = true;
                        self.ask_quit = false;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if ui.button("Cancel").clicked() {
                        self.ask_quit = false;
                    }
                });
            });
        }
        if let Some(e) = self.error.clone() {
            let mut open = true;
            egui::Window::new("Problem").collapsible(false).resizable(false).open(&mut open).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO).show(&ctx, |ui| {
                ui.label(e);
                if ui.button("OK").clicked() {
                    self.error = None;
                }
            });
            if !open {
                self.error = None;
            }
        }
        self.autosave();
    }
}

// ---------------------------------------------------------------------- helpers

/// A section of a panel: a rounded card with a heading.
fn card(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(ui.visuals().window_fill)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(12, 8))
        .outer_margin(egui::Margin { left: 0, right: 0, top: 0, bottom: 8 })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new(title).heading());
            ui.add_space(2.0);
            body(ui);
        });
}

fn plural(n: usize, what: &str) -> String {
    format!("{n} {what}{}", if n == 1 { "" } else { "s" })
}

/// One entry of a list (sites, antennas, plots): a dot, a name, a line under it and a note on the right.
fn list_row(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, sel: bool, colour: Option<[u8; 3]>, title: &str, sub: &str, right: &str) -> egui::Response {
    list_row_with(ui, id, sel, colour, title, sub, |ui| {
        ui.label(RichText::new(right).small().weak());
    })
}

fn list_row_with(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, sel: bool, colour: Option<[u8; 3]>, title: &str, sub: &str, right: impl FnOnce(&mut egui::Ui)) -> egui::Response {
    let fill = if sel { ui.visuals().selection.bg_fill } else { ui.visuals().window_fill };
    let r = egui::Frame::new()
        .fill(fill)
        .stroke(if sel { egui::Stroke::new(1.0, ui.visuals().selection.stroke.color) } else { ui.visuals().widgets.noninteractive.bg_stroke })
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(10, 5))
        .outer_margin(egui::Margin { bottom: 4, ..Default::default() })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                if let Some(c) = colour {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(12.0), egui::Sense::hover());
                    ui.painter().circle_filled(r.center(), 6.0, Color32::from_rgb(c[0], c[1], c[2]));
                }
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(RichText::new(title).strong());
                    if !sub.is_empty() {
                        ui.label(RichText::new(sub).small().weak());
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
            });
        });
    ui.interact(r.response.rect, ui.make_persistent_id(id), egui::Sense::click())
}

fn level_row(ui: &mut egui::Ui, r: &mut Receiver, base: bool, st: &model::Station, needs: Option<f64>) {
    ui.checkbox(&mut r.enabled, "");
    ui.horizontal(|ui| {
        ui.color_edit_button_srgb(&mut r.colour);
        ui.label(&r.name);
    });
    if base && r.use_site_antenna {
        for t in [
            format!("{:.1} m", st.antenna.height_m),
            format!("{:.1} dBi", st.antenna.gain_dbi),
            format!("{:.1} dB", st.feeder_loss_db),
            format!("{:.0} dBm", st.rx_sensitivity_dbm),
        ] {
            ui.label(RichText::new(t).weak());
        }
    } else {
        ui.add(egui::DragValue::new(&mut r.height_m).speed(0.1).range(0.5..=3000.0).suffix(" m"));
        ui.add(egui::DragValue::new(&mut r.gain_dbi).speed(0.1).range(-20.0..=40.0).suffix(" dBi"));
        ui.add(egui::DragValue::new(&mut r.loss_db).speed(0.1).range(0.0..=60.0).suffix(" dB"));
        match needs {
            Some(n) => {
                ui.label(RichText::new(format!("{n:.0} dBm")).weak()).on_hover_text("Set by the mode");
            }
            None => {
                ui.add(egui::DragValue::new(&mut r.required_dbm).speed(0.5).range(-170.0..=-30.0).suffix(" dBm"));
            }
        }
    }
    ui.end_row();
}

fn legend(ui: &mut egui::Ui, proj: &Projection, mode: &str, levels: &[coverage::Level]) {
    let painter = ui.painter_at(proj.rect);
    let font = egui::FontId::proportional(12.0);
    let lines: Vec<String> = levels.iter().map(|l| format!("{} ≥ {:.0} dBm", l.name, l.required_dbm)).collect();
    let mut w = lines.iter().map(|t| painter.layout_no_wrap(t.clone(), font.clone(), Color32::WHITE).size().x).fold(0.0, f32::max) + 34.0;
    let top = if mode.is_empty() { 0.0 } else { 19.0 };
    if !mode.is_empty() {
        w = w.max(painter.layout_no_wrap(mode.to_string(), font.clone(), Color32::WHITE).size().x + 20.0);
    }
    let h = lines.len() as f32 * 19.0 + 12.0 + top;
    let r = egui::Rect::from_min_size(proj.rect.right_bottom() - Vec2::new(w + 12.0, h + 30.0), Vec2::new(w, h));
    painter.rect_filled(r, 8.0, crate::theme::label_bg());
    painter.rect_stroke(r, 8.0, ui.visuals().widgets.noninteractive.bg_stroke, egui::StrokeKind::Inside);
    if !mode.is_empty() {
        painter.text(egui::Pos2::new(r.left() + 10.0, r.top() + 8.0), egui::Align2::LEFT_TOP, mode, egui::FontId::proportional(12.0), crate::theme::ink_soft());
    }
    for (k, (l, t)) in levels.iter().zip(lines).enumerate() {
        let y = r.top() + 8.0 + top + k as f32 * 19.0;
        painter.rect_filled(egui::Rect::from_min_size(egui::Pos2::new(r.left() + 10.0, y + 2.0), Vec2::splat(12.0)), 3.0, Color32::from_rgb(l.colour[0], l.colour[1], l.colour[2]));
        painter.text(egui::Pos2::new(r.left() + 28.0, y), egui::Align2::LEFT_TOP, t, font.clone(), crate::theme::ink());
    }
}

/// The tab rail's buttons, as in wireant.
fn nav_item(ui: &mut egui::Ui, selected: bool, icon: &str, label: &str, size: Vec2) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let v = ui.visuals();
        let accent = v.selection.stroke.color;
        let pill = rect.shrink2(Vec2::new(4.0, 2.0));
        if selected {
            ui.painter().rect_filled(pill, 10.0, v.selection.bg_fill);
        } else if resp.hovered() {
            ui.painter().rect_filled(pill, 10.0, v.widgets.hovered.weak_bg_fill);
        }
        let fg = if selected { accent } else { v.widgets.inactive.fg_stroke.color };
        let c = rect.center();
        ui.painter().text(c - Vec2::new(0.0, 8.0), egui::Align2::CENTER_CENTER, icon, egui::FontId::proportional(20.0), fg);
        let mut font = egui::FontId::proportional(11.0);
        let w = ui.painter().layout_no_wrap(label.to_owned(), font.clone(), fg).size().x;
        if w > size.x - 4.0 {
            font.size *= (size.x - 4.0) / w;
        }
        ui.painter().text(c + Vec2::new(0.0, 13.0), egui::Align2::CENTER_CENTER, label, font, fg);
    }
    resp
}

fn dem_label(z: u32) -> &'static str {
    match z {
        10 => "~100 m (fastest)",
        12 => "~25 m (largest download)",
        _ => "~50 m",
    }
}

fn dbm_to_uv(dbm: f64) -> f64 {
    (50.0 * 10f64.powf((dbm - 30.0) / 10.0)).sqrt() * 1e6
}

fn watts(dbm: f64) -> String {
    let w = 10f64.powf((dbm - 30.0) / 10.0);
    if w >= 100.0 { format!("{w:.0} W") } else if w >= 1.0 { format!("{w:.1} W") } else { format!("{:.0} mW", w * 1000.0) }
}

fn open_folder(p: &std::path::Path) {
    #[cfg(target_os = "windows")]
    let cmd = "explorer";
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let cmd = "xdg-open";
    let _ = std::process::Command::new(cmd).arg(p).spawn();
}
