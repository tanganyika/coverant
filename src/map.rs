//! The slippy map: OpenStreetMap, OpenTopoMap or Esri imagery tiles, downloaded
//! in the background and kept in a cache folder; drag to pan, scroll or pinch
//! to zoom. Everything drawn on top (pins, coverage) uses a [`Projection`].

use crate::terrain::{cache_root, http_get, lonlat_to_px, metres_per_px, px_to_lonlat, TILE};
use eframe::egui::{self, Color32, ColorImage, Pos2, Rect, Sense, TextureHandle, Vec2};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

pub struct Basemap {
    pub name: &'static str,
    url: &'static str,
    pub attribution: &'static str,
    pub max_zoom: u8,
    key: &'static str,
}

pub const BASEMAPS: [Basemap; 3] = [
    Basemap { name: "Street map", url: "https://tile.openstreetmap.org/{z}/{x}/{y}.png", attribution: "© OpenStreetMap contributors", max_zoom: 19, key: "osm" },
    Basemap { name: "Topographic", url: "https://tile.opentopomap.org/{z}/{x}/{y}.png", attribution: "© OpenStreetMap contributors, SRTM · © OpenTopoMap (CC-BY-SA)", max_zoom: 17, key: "topo" },
    Basemap { name: "Satellite", url: "https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}", attribution: "Imagery © Esri, Maxar, Earthstar Geographics", max_zoom: 19, key: "esri" },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Key {
    map: u8,
    grey: bool,
    z: u8,
    x: u32,
    y: u32,
}

enum Tile {
    Ready(TextureHandle, u64),
    Failed,
}

/// Map tiles: textures for the ones on screen, and the workers fetching the rest.
pub struct Tiles {
    tiles: HashMap<Key, Tile>,
    queue: Arc<Mutex<VecDeque<Key>>>,
    in_flight: Arc<Mutex<HashSet<Key>>>,
    done: Receiver<(Key, Option<ColorImage>)>,
    frame: u64,
    pub offline: bool,
}

impl Tiles {
    pub fn new(ctx: &egui::Context) -> Self {
        let queue: Arc<Mutex<VecDeque<Key>>> = Default::default();
        let in_flight: Arc<Mutex<HashSet<Key>>> = Default::default();
        let (tx, rx): (Sender<(Key, Option<ColorImage>)>, _) = channel();
        for _ in 0..6 {
            let (q, f, tx, ctx) = (queue.clone(), in_flight.clone(), tx.clone(), ctx.clone());
            std::thread::spawn(move || loop {
                let next = q.lock().unwrap().pop_front();
                if let Some(k) = next {
                    f.lock().unwrap().insert(k);
                }
                let Some(k) = next else {
                    std::thread::sleep(std::time::Duration::from_millis(30));
                    continue;
                };
                let img = fetch(k);
                f.lock().unwrap().remove(&k);
                if tx.send((k, img)).is_err() {
                    break;
                }
                ctx.request_repaint();
            });
        }
        Tiles { tiles: HashMap::new(), queue, in_flight, done: rx, frame: 0, offline: false }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let mut failures = 0;
        while let Ok((k, img)) = self.done.try_recv() {
            match img {
                Some(img) => {
                    let tex = ctx.load_texture(format!("tile{:?}", k), img, egui::TextureOptions::LINEAR);
                    self.tiles.insert(k, Tile::Ready(tex, self.frame));
                    self.offline = false;
                }
                None => {
                    self.tiles.insert(k, Tile::Failed);
                    failures += 1;
                }
            }
        }
        if failures > 0 && !self.tiles.values().any(|t| matches!(t, Tile::Ready(..))) {
            self.offline = true;
        }
        // keep the most recently drawn few hundred
        if self.tiles.len() > 600 {
            let mut v: Vec<(Key, u64)> = self.tiles.iter().map(|(k, t)| (*k, if let Tile::Ready(_, f) = t { *f } else { 0 })).collect();
            v.sort_by_key(|(_, f)| *f);
            for (k, _) in v.iter().take(self.tiles.len() - 400) {
                self.tiles.remove(k);
            }
        }
    }

    fn get(&mut self, k: Key) -> Option<&TextureHandle> {
        match self.tiles.get_mut(&k) {
            Some(Tile::Ready(t, f)) => {
                *f = self.frame;
                Some(t)
            }
            _ => None,
        }
    }
}

fn fetch(k: Key) -> Option<ColorImage> {
    let b = &BASEMAPS[k.map as usize];
    let path = cache_root().join("map").join(b.key).join(k.z.to_string()).join(k.x.to_string()).join(format!("{}.img", k.y));
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => {
            let url = b.url.replace("{z}", &k.z.to_string()).replace("{x}", &k.x.to_string()).replace("{y}", &k.y.to_string());
            let bytes = http_get(&url).ok()?;
            if let Some(d) = path.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(&path, &bytes);
            bytes
        }
    };
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    let mut px = img.into_raw();
    if k.grey {
        for p in px.chunks_exact_mut(4) {
            let l = (0.30 * p[0] as f32 + 0.59 * p[1] as f32 + 0.11 * p[2] as f32) * 0.92 + 18.0;
            let l = l.min(255.0) as u8;
            p[0] = l;
            p[1] = l;
            p[2] = l;
        }
    }
    Some(ColorImage::from_rgba_unmultiplied([w, h], &px))
}

/// Where the map is looking.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub lat: f64,
    pub lon: f64,
    pub zoom: f64,
}

/// Converts between screen points and latitude/longitude for one frame.
#[derive(Clone, Copy)]
pub struct Projection {
    pub rect: Rect,
    cx: f64,
    cy: f64,
    zoom: f64,
}

impl Projection {
    fn new(rect: Rect, v: &View) -> Self {
        let (cx, cy) = lonlat_to_px(v.lon, v.lat, v.zoom);
        Projection { rect, cx, cy, zoom: v.zoom }
    }
    pub fn to_screen(&self, lat: f64, lon: f64) -> Pos2 {
        let (x, y) = lonlat_to_px(lon, lat, self.zoom);
        self.rect.center() + Vec2::new((x - self.cx) as f32, (y - self.cy) as f32)
    }
    pub fn to_latlon(&self, p: Pos2) -> (f64, f64) {
        let d = p - self.rect.center();
        let (lon, lat) = px_to_lonlat(self.cx + d.x as f64, self.cy + d.y as f64, self.zoom);
        (lat, lon)
    }
    /// A rectangle given in global pixels at another zoom level, on screen.
    pub fn global_rect(&self, x0: f64, y0: f64, x1: f64, y1: f64, at_zoom: f64) -> Rect {
        let s = 2f64.powf(self.zoom - at_zoom);
        let c = self.rect.center();
        let p = |x: f64, y: f64| Pos2::new(c.x + (x * s - self.cx) as f32, c.y + (y * s - self.cy) as f32);
        Rect::from_min_max(p(x0, y0), p(x1, y1))
    }
    pub fn zoom(&self) -> f64 {
        self.zoom
    }
}

/// Draw the map tiles, handle pan and zoom. `pan` is false while something on
/// the map (a pin) is being dragged instead.
pub fn show(ui: &mut egui::Ui, view: &mut View, tiles: &mut Tiles, basemap: usize, grey: bool, pan: bool) -> (egui::Response, Projection) {
    let ctx = ui.ctx().clone();
    tiles.frame += 1;
    tiles.poll(&ctx);
    let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let bm = &BASEMAPS[basemap.min(BASEMAPS.len() - 1)];
    let max_z = bm.max_zoom as f64;

    // pan
    if pan && resp.dragged_by(egui::PointerButton::Primary) {
        let d = resp.drag_delta();
        let (cx, cy) = lonlat_to_px(view.lon, view.lat, view.zoom);
        let (lon, lat) = px_to_lonlat(cx - d.x as f64, cy - d.y as f64, view.zoom);
        view.lat = lat.clamp(-85.0, 85.0);
        view.lon = lon;
    }
    // zoom about the pointer
    if let Some(hp) = resp.hover_pos() {
        let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
        let mut dz = scroll as f64 / 120.0 + (pinch as f64).log2();
        if resp.double_clicked() {
            dz += 1.0;
        }
        if dz != 0.0 {
            let before = Projection::new(rect, view).to_latlon(hp);
            view.zoom = (view.zoom + dz).clamp(2.0, max_z);
            let after = Projection::new(rect, view).to_latlon(hp);
            view.lat = (view.lat + before.0 - after.0).clamp(-85.0, 85.0);
            view.lon += before.1 - after.1;
        }
    }
    view.zoom = view.zoom.clamp(2.0, max_z);
    let proj = Projection::new(rect, view);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, if crate::theme::dark() { Color32::from_rgb(38, 42, 48) } else { Color32::from_rgb(226, 229, 233) });

    // tiles
    let tz = view.zoom.round().clamp(0.0, max_z) as u8;
    let scale = 2f64.powf(view.zoom - tz as f64);
    let (cx, cy) = lonlat_to_px(view.lon, view.lat, tz as f64);
    let size = TILE * scale;
    let c = rect.center();
    let n = 1u32 << tz;
    let tx0 = ((cx - (rect.width() as f64 / 2.0) / scale) / TILE).floor() as i64;
    let tx1 = ((cx + (rect.width() as f64 / 2.0) / scale) / TILE).floor() as i64;
    let ty0 = (((cy - (rect.height() as f64 / 2.0) / scale) / TILE).floor() as i64).max(0);
    let ty1 = (((cy + (rect.height() as f64 / 2.0) / scale) / TILE).floor() as i64).min(n as i64 - 1);
    let mut wanted = vec![];
    for ty in ty0..=ty1 {
        for txr in tx0..=tx1 {
            let tx = txr.rem_euclid(n as i64) as u32;
            let screen = Rect::from_min_size(
                Pos2::new(c.x + ((txr as f64 * TILE - cx) * scale) as f32, c.y + ((ty as f64 * TILE - cy) * scale) as f32),
                Vec2::splat(size as f32 + 0.5),
            );
            let k = Key { map: basemap as u8, grey, z: tz, x: tx, y: ty as u32 };
            if let Some(t) = tiles.get(k) {
                painter.image(t.id(), screen, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                continue;
            }
            if !tiles.tiles.contains_key(&k) {
                wanted.push(k);
            }
            // a coarser tile stretched until this one arrives
            for up in 1..=5u8 {
                if up > tz {
                    break;
                }
                let pk = Key { z: tz - up, x: tx >> up, y: (ty as u32) >> up, ..k };
                if let Some(t) = tiles.get(pk) {
                    let f = 1.0 / (1u32 << up) as f32;
                    let (ox, oy) = ((tx & ((1 << up) - 1)) as f32 * f, ((ty as u32) & ((1 << up) - 1)) as f32 * f);
                    painter.image(t.id(), screen, Rect::from_min_size(Pos2::new(ox, oy), Vec2::splat(f)), Color32::WHITE);
                    break;
                }
            }
        }
    }
    {
        let flight = tiles.in_flight.lock().unwrap();
        let mut q = tiles.queue.lock().unwrap();
        q.clear();
        // nearest the centre first
        wanted.retain(|k| !flight.contains(k));
        wanted.sort_by_key(|k| {
            let dx = k.x as f64 * TILE + 128.0 - cx;
            let dy = k.y as f64 * TILE + 128.0 - cy;
            (dx * dx + dy * dy) as i64
        });
        drop(flight);
        q.extend(wanted);
    }
    (resp, proj)
}

/// Scale bar, zoom buttons and attribution, drawn last.
pub fn decorations(ui: &mut egui::Ui, proj: &Projection, view: &mut View, basemap: usize, extra_attr: &str, offline: bool) {
    let rect = proj.rect;
    let painter = ui.painter_at(rect);
    let ink = crate::theme::ink();
    let bg = crate::theme::label_bg();
    // attribution
    let attr = format!("{}{}", BASEMAPS[basemap.min(2)].attribution, extra_attr);
    let galley = painter.layout_no_wrap(attr, egui::FontId::proportional(10.5), crate::theme::ink_soft());
    let ar = Rect::from_min_size(rect.right_bottom() - galley.size() - Vec2::new(10.0, 6.0), galley.size() + Vec2::new(8.0, 4.0));
    painter.rect_filled(ar, 4.0, bg);
    painter.galley(ar.min + Vec2::new(4.0, 2.0), galley, ink);
    if offline {
        let g = painter.layout_no_wrap("Map tiles can't be downloaded (offline?)".into(), egui::FontId::proportional(12.0), ink);
        let r = Rect::from_center_size(rect.center_top() + Vec2::new(0.0, 22.0), g.size() + Vec2::new(16.0, 8.0));
        painter.rect_filled(r, 6.0, bg);
        painter.galley(r.min + Vec2::new(8.0, 4.0), g, ink);
    }
    // scale bar
    let (lat, _) = proj.to_latlon(rect.center());
    let mpp = metres_per_px(lat, proj.zoom());
    let target = mpp * 120.0;
    let nice = [1.0, 2.0, 5.0];
    let mut len_m = 1.0;
    'o: for p in 0..8 {
        for n in nice {
            let v = n * 10f64.powi(p);
            if v > target {
                break 'o;
            }
            len_m = v;
        }
    }
    let len_px = (len_m / mpp) as f32;
    let base = rect.left_bottom() + Vec2::new(12.0, -14.0);
    let sr = Rect::from_min_max(base - Vec2::new(4.0, 18.0), base + Vec2::new(len_px + 4.0, 6.0));
    painter.rect_filled(sr, 4.0, bg);
    let stroke = egui::Stroke::new(1.5, ink);
    painter.line_segment([base, base + Vec2::new(len_px, 0.0)], stroke);
    painter.line_segment([base, base - Vec2::new(0.0, 5.0)], stroke);
    painter.line_segment([base + Vec2::new(len_px, 0.0), base + Vec2::new(len_px, -5.0)], stroke);
    let label = if len_m >= 1000.0 { format!("{} km", len_m / 1000.0) } else { format!("{len_m} m") };
    painter.text(base + Vec2::new(len_px / 2.0, -4.0), egui::Align2::CENTER_BOTTOM, label, egui::FontId::proportional(11.0), ink);
    // zoom buttons
    let br = Rect::from_min_size(rect.left_top() + Vec2::new(10.0, 10.0), Vec2::new(30.0, 62.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(br));
    child.spacing_mut().item_spacing.y = 2.0;
    if child.add(egui::Button::new(egui::RichText::new("+").size(16.0)).min_size(Vec2::new(30.0, 30.0))).on_hover_text("Zoom in").clicked() {
        view.zoom = (view.zoom + 1.0).round().min(19.0);
    }
    if child.add(egui::Button::new(egui::RichText::new("−").size(16.0)).min_size(Vec2::new(30.0, 30.0))).on_hover_text("Zoom out").clicked() {
        view.zoom = (view.zoom - 1.0).round().max(2.0);
    }
}

/// A map pin with its tip at `tip`.
pub fn pin(painter: &egui::Painter, tip: Pos2, colour: Color32, selected: bool) -> Rect {
    let r = if selected { 9.5 } else { 8.0 };
    let c = tip - Vec2::new(0.0, r * 2.2);
    let outline = egui::Stroke::new(if selected { 2.0 } else { 1.2 }, if selected { crate::theme::ACCENT } else { Color32::from_gray(30) });
    let pts = vec![tip, c + Vec2::new(-r * 0.86, r * 0.5), c + Vec2::new(r * 0.86, r * 0.5)];
    painter.add(egui::Shape::convex_polygon(pts, colour, egui::Stroke::NONE));
    painter.circle(c, r, colour, outline);
    painter.line_segment([c + Vec2::new(-r * 0.86, r * 0.5), tip], outline);
    painter.line_segment([c + Vec2::new(r * 0.86, r * 0.5), tip], outline);
    painter.circle_filled(c, r * 0.38, Color32::WHITE);
    Rect::from_center_size(c, Vec2::splat(r * 2.4)).union(Rect::from_center_size(tip, Vec2::splat(4.0)))
}
