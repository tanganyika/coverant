//! File menu work: backing up and restoring everything, and importing and exporting sites.

use super::*;
use crate::transfer::{self, Backup, Format, Found};

/// Sites read from a file, waiting for the user to pick which to add.
pub struct ImportState {
    file: String,
    found: Vec<Found>,
    pick: Vec<bool>,
    dup: Vec<bool>,
    use_heights: bool,
    terrain: bool,
}

/// A backup read from a file, waiting for the user to say how to restore it.
pub struct RestoreState {
    file: String,
    backup: Backup,
    replace: bool,
    settings: bool,
}

const PALETTE: [[u8; 3]; 6] = [[211, 47, 47], [25, 118, 210], [56, 142, 60], [245, 124, 0], [123, 31, 162], [0, 151, 167]];

fn file_name(p: &std::path::Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
}

impl App {
    pub(super) fn io_menu(&mut self, ui: &mut egui::Ui) {
        if ui.button("Import sites…").on_hover_text("Waypoints and placemarks from GPX, KML or GeoJSON").clicked() {
            ui.close();
            self.pick_import();
        }
        ui.add_enabled_ui(!self.lib.sites.is_empty(), |ui| {
            ui.menu_button("Export sites", |ui| {
                for f in Format::ALL {
                    if ui.button(format!("{}…", f.name())).clicked() {
                        ui.close();
                        self.export_sites(f);
                    }
                }
            });
        });
        ui.separator();
        if ui.button("Back up everything…").on_hover_text("Sites, antennas, plots, links and settings in one file").clicked() {
            ui.close();
            self.backup();
        }
        if ui.button("Restore from backup…").clicked() {
            ui.close();
            self.pick_restore();
        }
    }

    // ------------------------------------------------------------------ backup

    fn backup(&mut self) {
        let name = format!("coverant-backup-{}.json", &transfer::now_utc()[..10]);
        let Some(path) = rfd::FileDialog::new().set_title("Back up Coverant").add_filter("Coverant backup", &["json"]).set_file_name(&name).save_file() else { return };
        let b = Backup::new(&self.lib, &self.settings);
        match std::fs::write(&path, b.to_json()) {
            Ok(()) => self.status = format!("Backed up {} to {}.", b.summary(), file_name(&path)),
            Err(e) => self.error = Some(format!("Can't write {}: {e}", path.display())),
        }
    }

    fn pick_restore(&mut self) {
        if let Some(path) = rfd::FileDialog::new().set_title("Restore a Coverant backup").add_filter("Coverant backup", &["json"]).add_filter("All files", &["*"]).pick_file() {
            self.open_restore(&path);
        }
    }

    /// For automated tests: press the dialog's main button.
    pub fn accept_dialog(&mut self) {
        self.accept = true;
    }

    /// Read a backup and ask how to restore it.
    pub fn open_restore(&mut self, path: &std::path::Path) {
        match std::fs::read_to_string(path).map_err(|e| format!("Can't read {}: {e}", path.display())).and_then(|t| Backup::parse(&t)) {
            Ok(backup) => {
                let settings = backup.settings.is_some();
                self.restore = Some(RestoreState { file: file_name(path), backup, replace: true, settings });
            }
            Err(e) => self.error = Some(e),
        }
    }

    pub(super) fn restore_window(&mut self, ctx: &egui::Context) {
        let Some(r) = &mut self.restore else { return };
        let mut open = true;
        let mut go = false;
        let mut cancel = false;
        let busy = self.job.is_some() || self.link_job.is_some() || self.hz_job.is_some();
        egui::Window::new("Restore from backup").open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.label(RichText::new(&r.file).strong());
            ui.label(format!("Made {} UTC by Coverant {}", r.backup.created, r.backup.app_version));
            ui.label(r.backup.summary());
            ui.add_space(8.0);
            ui.radio_value(&mut r.replace, true, "Replace everything with the backup");
            ui.radio_value(&mut r.replace, false, "Add what's in the backup to what I have");
            ui.label(
                RichText::new(if r.replace {
                    "Your sites, antennas, plots and links now are swapped for the backup's."
                } else {
                    "Anything you don't have is added. Where both have the same item, yours is kept."
                })
                .small()
                .weak(),
            );
            if r.backup.settings.is_some() {
                ui.add_enabled_ui(r.replace, |ui| ui.checkbox(&mut r.settings, "Restore the settings too (receivers, model, map)"));
            }
            ui.add_space(4.0);
            ui.label(RichText::new("A copy of what you have now is saved first, in the backups folder inside the data folder.").small().weak());
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let label = if r.replace { "Replace" } else { "Add" };
                if ui.add_enabled(!busy, egui::Button::new(RichText::new(label).color(Color32::WHITE)).fill(crate::theme::ACCENT)).on_disabled_hover_text("Wait for the calculation to finish").clicked() {
                    go = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        if go || std::mem::take(&mut self.accept) {
            let r = self.restore.take().unwrap();
            self.apply_restore(r, ctx);
        } else if cancel || !open {
            self.restore = None;
        }
    }

    fn apply_restore(&mut self, r: RestoreState, ctx: &egui::Context) {
        // keep what's here now, in case the restore wasn't wanted
        let dir = model::data_dir().join("backups");
        let stamp = transfer::now_utc().replace([' ', ':'], "-");
        let safety = dir.join(format!("before-restore-{stamp}.json"));
        if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&safety, Backup::new(&self.lib, &self.settings).to_json())) {
            self.error = Some(format!("Can't keep a copy of your data before restoring ({e}), so nothing was changed."));
            return;
        }
        let incoming = r.backup.library();
        let what = if r.replace {
            self.lib = incoming;
            if let (true, Some(mut s)) = (r.settings, r.backup.settings.clone()) {
                s.view = self.settings.view;
                self.settings = s;
                crate::theme::apply_preference(ctx, &self.settings.theme);
            }
            format!("Restored {} from {}.", r.backup.summary(), r.file)
        } else {
            let m = transfer::merge(&mut self.lib, &incoming);
            if m.total() == 0 {
                format!("Nothing to add from {}: you already have everything in it.", r.file)
            } else {
                format!(
                    "Added {}, {}, {} and {} from {}.",
                    plural(m.sites, "site"),
                    plural(m.antennas, "antenna"),
                    plural(m.plots, "plot"),
                    plural(m.links, "link"),
                    r.file
                )
            }
        };
        self.after_library_change();
        self.save();
        self.status = what;
    }

    /// After the whole library changed: reload saved coverage and start the lists again.
    fn after_library_change(&mut self) {
        self.results.clear();
        for p in &self.lib.plots {
            if let Some(cov) = Coverage::load(&p.id) {
                self.results.insert(p.id.clone(), Plot { cov, tex: None });
            }
        }
        self.link_results.clear();
        self.hz_results.clear();
        self.stale.clear();
        let first = |n: usize| if n > 0 { Some(0) } else { None };
        self.selected = first(self.lib.sites.len());
        self.sel_ant = first(self.lib.antennas.len());
        self.sel_plot = first(self.lib.plots.len());
        self.sel_link = first(self.lib.links.len());
        self.confirm_delete = false;
        self.confirm_delete_ant = false;
        self.confirm_delete_plot = false;
        self.confirm_delete_link = false;
        self.picking = false;
        self.popup = None;
        self.zoom_to_sites();
    }

    // ------------------------------------------------------------------ site files

    pub(super) fn pick_import(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Import sites")
            .add_filter("GPX, KML, GeoJSON", &["gpx", "kml", "geojson", "json"])
            .add_filter("All files", &["*"])
            .pick_file()
        {
            self.open_import(&path);
        }
    }

    /// Read a site file and show what's in it.
    pub fn open_import(&mut self, path: &std::path::Path) {
        match std::fs::read(path).map_err(|e| format!("Can't read {}: {e}", path.display())).and_then(|b| transfer::read_sites(&b)) {
            Ok(found) => {
                let dup: Vec<bool> = found.iter().map(|f| transfer::already_have(&self.lib.sites, f)).collect();
                let pick = dup.iter().map(|d| !d).collect();
                let terrain = found.len() <= 10;
                self.import = Some(ImportState { file: file_name(path), found, pick, dup, use_heights: false, terrain });
            }
            Err(e) => self.error = Some(e),
        }
    }

    pub(super) fn import_window(&mut self, ctx: &egui::Context) {
        let Some(im) = &mut self.import else { return };
        let mut open = true;
        let (mut go, mut cancel) = (false, false);
        let n = im.pick.iter().filter(|p| **p).count();
        egui::Window::new("Import sites").open(&mut open).collapsible(false).resizable(true).default_height(420.0).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.label(RichText::new(&im.file).strong());
            let dups = im.dup.iter().filter(|d| **d).count();
            ui.label(format!(
                "{} found{}.",
                plural(im.found.len(), "point"),
                if dups > 0 { format!(", {dups} already in your sites and not ticked") } else { String::new() }
            ));
            ui.horizontal(|ui| {
                if ui.small_button("Tick all").clicked() {
                    im.pick.iter_mut().for_each(|p| *p = true);
                }
                if ui.small_button("Untick all").clicked() {
                    im.pick.iter_mut().for_each(|p| *p = false);
                }
            });
            egui::ScrollArea::vertical().max_height(260.0).auto_shrink([false, true]).show(ui, |ui| {
                for (i, f) in im.found.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut im.pick[i], "");
                        let name = if f.name.is_empty() { "(no name)" } else { f.name.as_str() };
                        ui.label(RichText::new(name).strong());
                        if im.dup[i] {
                            ui.label(RichText::new("already have").small().color(Color32::from_rgb(217, 119, 6)));
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if let Some(e) = f.ele {
                                ui.label(RichText::new(format!("{e:.0} m")).monospace().weak());
                            }
                            ui.label(RichText::new(format!("{:.5}, {:.5}", f.lat, f.lon)).monospace().weak());
                        });
                    });
                }
            });
            ui.separator();
            if im.found.iter().any(|f| f.ele.is_some()) {
                ui.checkbox(&mut im.use_heights, "Use the file's heights as the ground height")
                    .on_hover_text("Leave this off to take the ground height from the terrain data. GPS heights are often tens of metres out.");
            }
            ui.checkbox(&mut im.terrain, "Download the terrain around each site when saved");
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(n > 0, egui::Button::new(RichText::new(format!("Import {}", plural(n, "site"))).color(Color32::WHITE)).fill(crate::theme::ACCENT)).clicked() {
                    go = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        if go || std::mem::take(&mut self.accept) {
            let im = self.import.take().unwrap();
            self.apply_import(im);
        } else if cancel || !open {
            self.import = None;
        }
    }

    fn apply_import(&mut self, im: ImportState) {
        let first = self.lib.sites.len();
        for (f, _) in im.found.iter().zip(&im.pick).filter(|(_, p)| **p) {
            let n = self.lib.sites.len() + 1;
            self.lib.sites.push(Site {
                id: model::new_id(),
                name: if f.name.is_empty() { format!("Site {n}") } else { f.name.clone() },
                lat: f.lat,
                lon: f.lon,
                ground_m: if im.use_heights { f.ele } else { None },
                notes: f.notes.clone(),
                colour: f.colour.unwrap_or(PALETTE[(n - 1) % PALETTE.len()]),
                cache_terrain: im.terrain,
                ..Default::default()
            });
        }
        let added = self.lib.sites.len() - first;
        self.tab = Tab::Sites;
        self.selected = Some(first);
        let pts: Vec<(f64, f64)> = self.lib.sites[first..].iter().map(|s| (s.lat, s.lon)).collect();
        self.zoom_to(&pts);
        self.status = format!("Imported {} from {}. Press Save to keep them.", plural(added, "site"), im.file);
    }

    fn export_sites(&mut self, f: Format) {
        let name = format!("coverant-sites.{}", f.ext());
        let Some(path) = rfd::FileDialog::new().set_title("Export sites").add_filter(f.name(), &[f.ext()]).set_file_name(&name).save_file() else { return };
        match std::fs::write(&path, transfer::write_sites(&self.lib.sites, f)) {
            Ok(()) => self.status = format!("Exported {} to {}.", plural(self.lib.sites.len(), "site"), file_name(&path)),
            Err(e) => self.error = Some(format!("Can't write {}: {e}", path.display())),
        }
    }
}
