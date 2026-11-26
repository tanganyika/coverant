//! The main window: an icon rail (Sites, Antenna, Coverage, Links, Horizon), the
//! selected tab's panel, and the map.

use crate::coords;
use crate::coverage::{self, Coverage, Progress};
use crate::itm::CLIMATES;
use crate::map::{self, Projection, Tiles, View, BASEMAPS};
use crate::model::{self, Receiver, Settings, Site, GROUNDS, PRESETS};
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
            Tab::Antenna => "Antenna and radio",
            Tab::Coverage => "Coverage",
            Tab::Links => "Links between sites (next)",
            Tab::Horizon => "Radio horizon (next)",
        }
    }
    fn short(self) -> &'static str {
        match self {
            Tab::Sites => "Sites",
            Tab::Antenna => "Antenna",
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
        Tab::ALL.into_iter().find(|t| t.short().to_ascii_lowercase() == s)
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
    site_id: String,
    progress: Arc<Progress>,
    rx: Rx<Result<Coverage, String>>,
    started: std::time::Instant,
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
    sites: Vec<Site>,
    saved_sites: Vec<Site>,
    settings: Settings,
    saved_settings: Settings,
    last_save: std::time::Instant,
    selected: Option<usize>,
    tab: Tab,
    view: View,
    tiles: Tiles,
    terrains: HashMap<u32, Arc<Terrain>>,
    plots: HashMap<String, Plot>,
    job: Option<Job>,
    status: String,
    error: Option<String>,
    picking: bool,
    dragging_pin: bool,
    loc_text: String,
    popup: Option<Popup>,
    confirm_delete: bool,
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
        let sites = model::load_sites();
        let view = match settings.view {
            Some((lat, lon, zoom)) => View { lat, lon, zoom },
            None if !sites.is_empty() => View { lat: sites[0].lat, lon: sites[0].lon, zoom: 9.0 },
            None => View { lat: 54.0, lon: -2.5, zoom: 6.0 },
        };
        let mut plots = HashMap::new();
        for s in &sites {
            if let Some(cov) = Coverage::load(&s.id) {
                plots.insert(s.id.clone(), Plot { cov, tex: None });
            }
        }
        let (height_tx, height_rx) = channel();
        let (notes_tx, notes_rx) = channel();
        let threads = rayon::current_num_threads();
        let n = sites.len();
        App {
            saved_sites: sites.clone(),
            sites,
            saved_settings: settings.clone(),
            settings,
            last_save: std::time::Instant::now(),
            selected: if n > 0 { Some(0) } else { None },
            tab: tab.unwrap_or(Tab::Sites),
            view,
            tiles: Tiles::new(&cc.egui_ctx),
            terrains: HashMap::new(),
            plots,
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
        self.sites != self.saved_sites
    }

    fn save(&mut self) {
        match model::save_sites(&self.sites) {
            Ok(()) => {
                self.saved_sites = self.sites.clone();
                self.status = format!("Saved {} site{}.", self.sites.len(), if self.sites.len() == 1 { "" } else { "s" });
                self.cache_terrain();
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// Download, in the background, the terrain around every site that asks for it.
    fn cache_terrain(&mut self) {
        let zoom = self.settings.model.terrain_zoom;
        let areas: Vec<(String, f64, f64, f64)> =
            self.sites.iter().filter(|s| s.cache_terrain).map(|s| (s.name.clone(), s.lat, s.lon, s.cache_radius_km * 1000.0)).collect();
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
        let id = self.selected.map(|i| self.sites[i].id.clone());
        self.sites = self.saved_sites.clone();
        self.selected = id.and_then(|id| self.sites.iter().position(|s| s.id == id)).or(if self.sites.is_empty() { None } else { Some(0) });
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
            if ui.add_enabled(unsaved, save).on_hover_text("Save all sites (Ctrl+S)").clicked() {
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
        let n = self.sites.len() + 1;
        let palette = [[211, 47, 47], [25, 118, 210], [56, 142, 60], [245, 124, 0], [123, 31, 162], [0, 151, 167]];
        let site = Site { id: model::new_id(), name: format!("Site {n}"), lat, lon, colour: palette[(n - 1) % palette.len()], ..Default::default() };
        self.sites.push(site);
        self.selected = Some(self.sites.len() - 1);
        self.tab = Tab::Sites;
        self.picking = pick;
        self.loc_text.clear();
        self.status = if pick { "Click the map where the new site is.".into() } else { "New site added. Drag its pin to adjust it.".into() };
    }

    fn delete_selected(&mut self) {
        if let Some(i) = self.selected {
            let s = self.sites.remove(i);
            self.saved_sites.retain(|x| x.id != s.id);
            if let Err(e) = model::save_sites(&self.saved_sites) {
                self.error = Some(e);
            }
            Coverage::delete(&s.id);
            // the site's cached terrain, apart from what other sites still use
            let general = self.settings.model.radius_km;
            let removed = vec![(s.lat, s.lon, s.terrain_area_km(general) * 1000.0)];
            let kept: Vec<(f64, f64, f64)> = self.sites.iter().map(|k| (k.lat, k.lon, k.terrain_area_km(general) * 1000.0)).collect();
            let tx = self.notes_tx.clone();
            let name = s.name.clone();
            std::thread::spawn(move || {
                let n = terrain::cleanup(&removed, &kept);
                if n > 0 {
                    let _ = tx.send(format!("Removed {n} cached terrain tile{} that only {name} used.", if n == 1 { "" } else { "s" }));
                }
            });
            self.plots.remove(&s.id);
            self.selected = if self.sites.is_empty() { None } else { Some(i.min(self.sites.len() - 1)) };
            self.status = format!("Deleted {}.", s.name);
        }
    }

    fn set_location(&mut self, lat: f64, lon: f64) {
        if let Some(i) = self.selected {
            self.sites[i].lat = lat;
            self.sites[i].lon = lon;
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

    fn start_plot(&mut self) {
        let Some(i) = self.selected else { return };
        if self.job.is_some() {
            return;
        }
        let site = self.sites[i].clone();
        let mut settings = self.settings.clone();
        settings.model.radius_km = self.sites[i].plot_radius(self.settings.model.radius_km);
        let terrain = self.terrain(settings.model.terrain_zoom);
        let progress = Progress::new();
        let (tx, rx) = channel();
        let p2 = progress.clone();
        std::thread::spawn(move || {
            let r = coverage::run(&site, &settings, &terrain, &p2);
            let _ = tx.send(r);
        });
        self.status = format!("Plotting coverage for {} on {} threads…", self.sites[i].name, self.threads);
        self.job = Some(Job { site_id: self.sites[i].id.clone(), progress, rx, started: std::time::Instant::now() });
    }

    fn poll(&mut self) {
        let Some(job) = &self.job else { return };
        match job.rx.try_recv() {
            Ok(Ok(cov)) => {
                let id = job.site_id.clone();
                if let Err(e) = cov.save(&id) {
                    self.error = Some(format!("The plot worked but couldn't be saved: {e}"));
                }
                let name = self.sites.iter().find(|s| s.id == id).map(|s| s.name.clone()).unwrap_or_default();
                self.status = format!(
                    "Coverage for {name}: {} radials, {:.0} m steps, done in {:.1} s.",
                    cov.meta.n_rad, cov.meta.step_m, cov.meta.seconds
                );
                self.settings.hidden.retain(|h| h != &id);
                self.plots.insert(id, Plot { cov, tex: None });
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
                if ui.add_enabled(self.unsaved(), egui::Button::new("Save sites").shortcut_text("Ctrl+S")).clicked() {
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
                if ui.button("About Radio Sites").clicked() {
                    self.show_about = true;
                    ui.close();
                }
            });
        });
    }

    fn zoom_to_sites(&mut self) {
        if self.sites.is_empty() {
            return;
        }
        let (mut a, mut b, mut c, mut d) = (90.0f64, -90.0f64, 180.0f64, -180.0f64);
        for s in &self.sites {
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
        if self.sites.is_empty() {
            ui.label(RichText::new("No sites yet. Press “New site”, or right-click the map where a site is.").weak());
        }
        let mut select = None;
        let mut goto = None;
        for (i, s) in self.sites.iter().enumerate() {
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
                            if self.plots.contains_key(&s.id) {
                                let mut shown = !self.settings.hidden.contains(&s.id);
                                if ui.checkbox(&mut shown, "").on_hover_text("Show this site's coverage").changed() {
                                    if shown {
                                        self.settings.hidden.retain(|h| h != &s.id);
                                    } else {
                                        self.settings.hidden.push(s.id.clone());
                                    }
                                }
                            }
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
        }
        if let Some(i) = goto {
            self.view.lat = self.sites[i].lat;
            self.view.lon = self.sites[i].lon;
            self.view.zoom = self.view.zoom.max(11.0);
        }
        let Some(i) = self.selected else { return };
        ui.add_space(6.0);
        let mut find = false;
        let mut delete = false;
        card(ui, "Site", |ui| {
            egui::Grid::new("site_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Name");
                ui.add(egui::TextEdit::singleline(&mut self.sites[i].name).desired_width(f32::INFINITY));
                ui.end_row();
                ui.label("Colour");
                let s = &mut self.sites[i];
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
                    self.view.lat = self.sites[i].lat;
                    self.view.lon = self.sites[i].lon;
                    self.view.zoom = self.view.zoom.max(12.0);
                }
            });
            ui.add_space(4.0);
            let (lat, lon) = (self.sites[i].lat, self.sites[i].lon);
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
                let s = &mut self.sites[i];
                let mut manual = s.ground_m.is_some();
                if ui.checkbox(&mut manual, "Set ground height myself").changed() {
                    s.ground_m = if manual { Some(h.flatten().unwrap_or(0.0).round()) } else { None };
                }
                if let Some(g) = &mut s.ground_m {
                    ui.add(egui::DragValue::new(g).speed(0.5).suffix(" m").range(-50.0..=9000.0));
                }
            });
            ui.horizontal(|ui| {
                let s = &mut self.sites[i];
                ui.checkbox(&mut s.cache_terrain, "Download terrain when saved").on_hover_text("Keeps the terrain around this site on disk, so plots work offline");
                if s.cache_terrain {
                    ui.label("out to");
                    ui.add(egui::DragValue::new(&mut s.cache_radius_km).speed(1.0).range(1.0..=250.0).suffix(" km"));
                }
            });
            if self.sites[i].cache_terrain {
                ui.label(RichText::new("This distance is also the radius of this site's coverage plots.").small().weak());
            }
            ui.label("Notes");
            ui.add(egui::TextEdit::multiline(&mut self.sites[i].notes).desired_rows(2).desired_width(f32::INFINITY));
            ui.add_space(6.0);
            ui.separator();
            self.save_bar(ui);
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                if self.confirm_delete {
                    ui.label(format!("Delete {}?", self.sites[i].name));
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
        ui.heading("Antenna and radio");
        let Some(i) = self.selected else {
            ui.label(RichText::new("Choose or add a site first.").weak());
            return;
        };
        ui.label(RichText::new(&self.sites[i].name).strong());
        ui.add_space(4.0);
        let st = &mut self.sites[i].station;
        card(ui, "Radio", |ui| {
            egui::Grid::new("radio").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Frequency");
                ui.add(egui::DragValue::new(&mut st.freq_mhz).speed(0.025).range(20.0..=20000.0).suffix(" MHz").max_decimals(4));
                ui.end_row();
                ui.label("Transmit power");
                ui.add(egui::DragValue::new(&mut st.power_w).speed(0.5).range(0.01..=100000.0).suffix(" W"));
                ui.end_row();
                ui.label("Feeder loss");
                ui.add(egui::DragValue::new(&mut st.feeder_loss_db).speed(0.1).range(0.0..=30.0).suffix(" dB"));
                ui.end_row();
                ui.label("Receiver sensitivity");
                ui.add(egui::DragValue::new(&mut st.rx_sensitivity_dbm).speed(0.5).range(-150.0..=-50.0).suffix(" dBm"));
                ui.end_row();
                ui.label("Polarisation");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut st.vertical, true, "Vertical");
                    ui.radio_value(&mut st.vertical, false, "Horizontal");
                });
                ui.end_row();
            });
            let eirp = st.eirp_dbm();
            ui.label(
                RichText::new(format!(
                    "TX {:.1} dBm · EIRP {:.1} dBm ({}) · ERP {} · RX {:.2} µV pd",
                    st.tx_dbm(),
                    eirp,
                    watts(eirp),
                    watts(eirp - 2.15),
                    dbm_to_uv(st.rx_sensitivity_dbm)
                ))
                .small()
                .weak(),
            );
        });
        let a = &mut st.antenna;
        card(ui, "Antenna", |ui| {
            egui::Grid::new("ant").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Type");
                egui::ComboBox::from_id_salt("preset").selected_text(model::preset_label(&a.preset)).width(220.0).show_ui(ui, |ui| {
                    for p in PRESETS {
                        if ui.selectable_label(a.preset == p.key, p.label).clicked() {
                            a.preset = p.key.into();
                            a.gain_dbi = p.gain;
                            a.directional = p.directional;
                            a.v_beamwidth = p.v_bw;
                            if p.directional {
                                a.h_beamwidth = p.h_bw;
                                a.front_back_db = p.fb;
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
                ui.label("Height above ground");
                ui.add(egui::DragValue::new(&mut a.height_m).speed(0.25).range(0.5..=3000.0).suffix(" m"));
                ui.end_row();
                ui.label("Pattern");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut a.directional, false, "Omni");
                    ui.radio_value(&mut a.directional, true, "Directional");
                });
                ui.end_row();
                if a.directional {
                    ui.label("Azimuth");
                    ui.add(egui::DragValue::new(&mut a.azimuth).speed(1.0).range(0.0..=359.9).suffix("° true"));
                    ui.end_row();
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
                ui.label("Down-tilt");
                ui.add(egui::DragValue::new(&mut a.tilt).speed(0.25).range(-30.0..=30.0).suffix("°"));
                ui.end_row();
                if before != (a.gain_dbi, a.directional, a.v_beamwidth, a.h_beamwidth, a.front_back_db) {
                    a.preset = "custom".into();
                }
            });
            ui.label(RichText::new("Vertical beamwidth sets how fast the gain falls away above and below the main lobe (0 ignores it); down-tilt moves the main lobe down.").small().weak());
        });
        self.save_bar(ui);
    }

    fn coverage_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Coverage");
        let Some(i) = self.selected else {
            ui.label(RichText::new("Choose or add a site first.").weak());
            return;
        };
        ui.label(RichText::new(&self.sites[i].name).strong());
        ui.add_space(4.0);
        let busy = self.job.is_some();
        card(ui, "Plot", |ui| {
            let m = &mut self.settings.model;
            egui::Grid::new("plot").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Radius");
                ui.horizontal(|ui| {
                    let site = &mut self.sites[i];
                    let mut r = site.plot_radius(m.radius_km);
                    if ui.add(egui::DragValue::new(&mut r).speed(1.0).range(2.0..=250.0).suffix(" km")).changed() {
                        if site.cache_terrain || site.plot_radius_km.is_some() {
                            site.plot_radius_km = Some(r);
                        } else {
                            m.radius_km = r;
                        }
                    }
                    if site.cache_terrain && site.plot_radius_km.is_some_and(|p| p != site.cache_radius_km) {
                        if ui.small_button("↺").on_hover_text(format!("Back to the site's terrain distance ({:.0} km)", site.cache_radius_km)).clicked() {
                            site.plot_radius_km = None;
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
                } else if ui.add(egui::Button::new(RichText::new("▶ Plot coverage").color(Color32::WHITE).strong()).fill(crate::theme::ACCENT).min_size(Vec2::new(150.0, 30.0))).clicked() {
                    self.start_plot();
                }
            });
        });
        card(ui, "Receive levels", |ui| {
            ui.label(RichText::new("Each colour shows where that kind of station hears the site at or above the signal it needs. Hand portables are drawn on top.").small().weak());
            let st = self.sites[i].station.clone();
            egui::Grid::new("levels").num_columns(6).spacing([6.0, 4.0]).show(ui, |ui| {
                for h in ["", "Station", "Height", "Gain", "Losses", "Needs"] {
                    ui.label(RichText::new(h).small().weak());
                }
                ui.end_row();
                let s = &mut self.settings;
                for (k, r) in [&mut s.handheld, &mut s.mobile, &mut s.base].into_iter().enumerate() {
                    level_row(ui, r, k == 2, &st);
                }
            });
            ui.checkbox(&mut self.settings.base.use_site_antenna, "Base station uses this site's antenna height, gain, feeder and receiver");
            ui.label(RichText::new("Longley-Rice knows nothing of buildings, trees or a body holding the radio: put allowances for them in Losses. −107 dBm ≈ 1 µV, −113 ≈ 0.5 µV, −119 ≈ 0.25 µV.").small().weak());
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
                for (lbl, v) in [("Time", &mut m.time_pct), ("Location", &mut m.location_pct), ("Situation", &mut m.situation_pct)] {
                    ui.label(lbl);
                    ui.add(egui::DragValue::new(v).speed(1.0).range(1.0..=99.0).suffix(" %"));
                    ui.end_row();
                }
            });
            ui.label(RichText::new("50 % / 50 % / 50 % gives the median prediction. Raise them for a more cautious plot.").small().weak());
        });
        if let Some(p) = self.plots.get(&self.sites[i].id) {
            let m = &p.cov.meta;
            card(ui, "Last plot", |ui| {
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
        for s in &self.sites {
            if !show_cov || self.settings.hidden.contains(&s.id) {
                continue;
            }
            let Some(p) = self.plots.get_mut(&s.id) else { continue };
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
        for (i, s) in self.sites.iter().enumerate() {
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
        let mut legend_levels = None;
        for s in &self.sites {
            if let Some(p) = self.plots.get(&s.id) {
                if !self.settings.hidden.contains(&s.id) {
                    legend_levels = Some(p.cov.meta.levels.clone());
                    break;
                }
            }
        }
        if !show_cov {
            legend_levels = None;
            if !self.plots.is_empty() {
                let painter = ui.painter_at(proj.rect);
                let g = painter.layout_no_wrap("Coverage is hidden while you edit sites".into(), egui::FontId::proportional(12.0), crate::theme::ink_soft());
                let r = egui::Rect::from_min_size(proj.rect.right_top() + Vec2::new(-g.size().x - 22.0, 10.0), g.size() + Vec2::new(12.0, 6.0));
                painter.rect_filled(r, 6.0, crate::theme::label_bg());
                painter.galley(r.min + Vec2::new(6.0, 3.0), g, crate::theme::ink_soft());
            }
        }
        if let Some(levels) = legend_levels {
            legend(ui, &proj, &levels);
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
        for s in &self.sites {
            if self.settings.hidden.contains(&s.id) {
                continue;
            }
            if let Some(p) = self.plots.get(&s.id) {
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
        egui::Window::new("About Radio Sites").open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
            ui.heading(format!("Radio Sites {}", env!("CARGO_PKG_VERSION")));
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
                    let s = &self.sites[i];
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

fn level_row(ui: &mut egui::Ui, r: &mut Receiver, base: bool, st: &model::Station) {
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
        ui.add(egui::DragValue::new(&mut r.required_dbm).speed(0.5).range(-150.0..=-30.0).suffix(" dBm"));
    }
    ui.end_row();
}

fn legend(ui: &mut egui::Ui, proj: &Projection, levels: &[coverage::Level]) {
    let painter = ui.painter_at(proj.rect);
    let font = egui::FontId::proportional(12.0);
    let lines: Vec<String> = levels.iter().map(|l| format!("{} ≥ {:.0} dBm", l.name, l.required_dbm)).collect();
    let w = lines.iter().map(|t| painter.layout_no_wrap(t.clone(), font.clone(), Color32::WHITE).size().x).fold(0.0, f32::max) + 34.0;
    let h = lines.len() as f32 * 19.0 + 12.0;
    let r = egui::Rect::from_min_size(proj.rect.right_bottom() - Vec2::new(w + 12.0, h + 30.0), Vec2::new(w, h));
    painter.rect_filled(r, 8.0, crate::theme::label_bg());
    painter.rect_stroke(r, 8.0, ui.visuals().widgets.noninteractive.bg_stroke, egui::StrokeKind::Inside);
    for (k, (l, t)) in levels.iter().zip(lines).enumerate() {
        let y = r.top() + 8.0 + k as f32 * 19.0;
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
