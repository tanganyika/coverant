//! Getting data in and out: a backup of everything in one file, and sites read from or
//! written to GPX, KML and GeoJSON.

use crate::model::{AntennaDef, LinkDef, Library, PlotDef, Settings, Site};
use serde::{Deserialize, Serialize};

// ------------------------------------------------------------------ backup

pub const BACKUP_KIND: &str = "coverant-backup";

/// Everything the user has entered, in one file. Coverage results aren't included:
/// they are worked out again from the plots.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Backup {
    pub kind: String,
    pub app_version: String,
    /// when it was made, "YYYY-MM-DD HH:MM" UTC
    pub created: String,
    #[serde(default)]
    pub sites: Vec<Site>,
    #[serde(default)]
    pub antennas: Vec<AntennaDef>,
    #[serde(default)]
    pub plots: Vec<PlotDef>,
    #[serde(default)]
    pub links: Vec<LinkDef>,
    #[serde(default)]
    pub settings: Option<Settings>,
}

impl Backup {
    pub fn new(lib: &Library, settings: &Settings) -> Backup {
        Backup {
            kind: BACKUP_KIND.into(),
            app_version: env!("CARGO_PKG_VERSION").into(),
            created: now_utc(),
            sites: lib.sites.clone(),
            antennas: lib.antennas.clone(),
            plots: lib.plots.clone(),
            links: lib.links.clone(),
            settings: Some(settings.clone()),
        }
    }
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap()
    }
    pub fn parse(text: &str) -> Result<Backup, String> {
        let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("This isn't a Coverant backup ({e})."))?;
        if v.get("kind").and_then(|k| k.as_str()) != Some(BACKUP_KIND) {
            return Err("This isn't a Coverant backup.".into());
        }
        let mut b: Backup = serde_json::from_value(v).map_err(|e| format!("The backup can't be read: {e}"))?;
        let mut lib = b.library();
        lib.migrate();
        (b.sites, b.antennas, b.plots, b.links) = (lib.sites, lib.antennas, lib.plots, lib.links);
        Ok(b)
    }
    pub fn library(&self) -> Library {
        Library { sites: self.sites.clone(), antennas: self.antennas.clone(), plots: self.plots.clone(), links: self.links.clone() }
    }
    pub fn summary(&self) -> String {
        let n = |k: usize, w: &str| format!("{k} {w}{}", if k == 1 { "" } else { "s" });
        format!("{} · {} · {} · {}", n(self.sites.len(), "site"), n(self.antennas.len(), "antenna"), n(self.plots.len(), "plot"), n(self.links.len(), "link"))
    }
}

/// What a merge added.
#[derive(Debug, Default, PartialEq)]
pub struct Merged {
    pub sites: usize,
    pub antennas: usize,
    pub plots: usize,
    pub links: usize,
}

impl Merged {
    pub fn total(&self) -> usize {
        self.sites + self.antennas + self.plots + self.links
    }
}

/// Add the backup's items that `lib` doesn't have (matched by id; where both have an item,
/// the one in `lib` is kept).
pub fn merge(lib: &mut Library, b: &Library) -> Merged {
    fn add<T: Clone>(mine: &mut Vec<T>, theirs: &[T], id: impl Fn(&T) -> &str) -> usize {
        let mut n = 0;
        for t in theirs {
            if !mine.iter().any(|m| id(m) == id(t)) {
                mine.push(t.clone());
                n += 1;
            }
        }
        n
    }
    Merged {
        sites: add(&mut lib.sites, &b.sites, |s| &s.id),
        antennas: add(&mut lib.antennas, &b.antennas, |a| &a.id),
        plots: add(&mut lib.plots, &b.plots, |p| &p.id),
        links: add(&mut lib.links, &b.links, |l| &l.id),
    }
}

/// "YYYY-MM-DD HH:MM", UTC.
pub fn now_utc() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let (y, m, d) = civil(secs.div_euclid(86400));
    let t = secs.rem_euclid(86400);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", t / 3600, t % 3600 / 60)
}

/// Days since 1970-01-01 to a date (Howard Hinnant's algorithm).
fn civil(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d)
}

// ------------------------------------------------------------------ site files

/// A site as found in a GPX, KML or GeoJSON file.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    /// height above sea level, when the file gives one
    pub ele: Option<f64>,
    pub notes: String,
    pub colour: Option<[u8; 3]>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Format {
    Gpx,
    Kml,
    GeoJson,
}

impl Format {
    pub const ALL: [Format; 3] = [Format::Gpx, Format::Kml, Format::GeoJson];
    pub fn name(self) -> &'static str {
        match self {
            Format::Gpx => "GPX",
            Format::Kml => "KML (Google Earth)",
            Format::GeoJson => "GeoJSON",
        }
    }
    pub fn ext(self) -> &'static str {
        match self {
            Format::Gpx => "gpx",
            Format::Kml => "kml",
            Format::GeoJson => "geojson",
        }
    }
}

/// Read the points in a GPX, KML or GeoJSON file (told apart by what's in it).
pub fn read_sites(bytes: &[u8]) -> Result<Vec<Found>, String> {
    if bytes.starts_with(b"PK") {
        return Err("This looks like a KMZ (zipped KML). Unzip it and import the doc.kml inside.".into());
    }
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim_start_matches('\u{feff}').trim_start();
    let found = if text.starts_with('{') || text.starts_with('[') {
        read_geojson(text)?
    } else if text.starts_with('<') {
        let doc = roxmltree::Document::parse_with_options(text, roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() })
            .map_err(|e| format!("The file can't be read: {e}"))?;
        match doc.root_element().tag_name().name() {
            "gpx" => read_gpx(&doc),
            "kml" => read_kml(&doc),
            other => return Err(format!("Coverant reads GPX, KML and GeoJSON; this file is <{other}>.")),
        }
    } else {
        return Err("Coverant reads GPX, KML and GeoJSON; this file isn't any of them.".into());
    };
    let found: Vec<Found> = found.into_iter().filter(|f| f.lat.abs() <= 90.0 && f.lon.abs() <= 180.0 && f.lat.is_finite() && f.lon.is_finite()).collect();
    if found.is_empty() {
        return Err("No points were found in the file. Coverant reads waypoints, placemarks and points, not tracks or areas.".into());
    }
    Ok(found)
}

fn child_text<'a>(n: roxmltree::Node<'a, 'a>, name: &str) -> Option<&'a str> {
    n.children().find(|c| c.is_element() && c.tag_name().name() == name).and_then(|c| c.text()).map(str::trim).filter(|t| !t.is_empty())
}

fn read_gpx(doc: &roxmltree::Document) -> Vec<Found> {
    let mut out = vec![];
    for kind in ["wpt", "rtept"] {
        for n in doc.descendants().filter(|n| n.is_element() && n.tag_name().name() == kind) {
            let (Some(lat), Some(lon)) = (n.attribute("lat").and_then(|v| v.trim().parse().ok()), n.attribute("lon").and_then(|v| v.trim().parse().ok())) else { continue };
            let notes = [child_text(n, "desc"), child_text(n, "cmt")].into_iter().flatten().collect::<Vec<_>>().join("\n");
            out.push(Found {
                name: child_text(n, "name").unwrap_or("").into(),
                lat,
                lon,
                ele: child_text(n, "ele").and_then(|v| v.parse().ok()),
                notes,
                colour: None,
            });
        }
        if !out.is_empty() {
            break;
        }
    }
    out
}

fn read_kml(doc: &roxmltree::Document) -> Vec<Found> {
    let mut out = vec![];
    for pm in doc.descendants().filter(|n| n.is_element() && n.tag_name().name() == "Placemark") {
        for pt in pm.descendants().filter(|n| n.is_element() && n.tag_name().name() == "Point") {
            let Some(c) = child_text(pt, "coordinates") else { continue };
            let v: Vec<f64> = c.split_whitespace().next().unwrap_or("").split(',').filter_map(|x| x.trim().parse().ok()).collect();
            if v.len() < 2 {
                continue;
            }
            // a height only means height above sea level in "absolute" mode
            let absolute = child_text(pt, "altitudeMode") == Some("absolute");
            out.push(Found {
                name: child_text(pm, "name").unwrap_or("").into(),
                lat: v[1],
                lon: v[0],
                ele: if absolute { v.get(2).copied() } else { None },
                notes: child_text(pm, "description").map(strip_html).unwrap_or_default(),
                colour: None,
            });
        }
    }
    out
}

/// Descriptions in KML are often HTML: keep the words.
fn strip_html(s: &str) -> String {
    let s = s.replace("<br>", "\n").replace("<br/>", "\n").replace("<br />", "\n");
    let mut out = String::new();
    let mut tag = false;
    for ch in s.chars() {
        match ch {
            '<' => tag = true,
            '>' => tag = false,
            c if !tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&nbsp;", " ").trim().to_string()
}

fn read_geojson(text: &str) -> Result<Vec<Found>, String> {
    use serde_json::Value;
    let v: Value = serde_json::from_str(text).map_err(|e| format!("The GeoJSON can't be read: {e}"))?;
    fn prop_str(p: &Value, keys: &[&str]) -> Option<String> {
        keys.iter().find_map(|k| match p.get(*k)? {
            Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
    }
    fn point(c: &Value, props: &Value, out: &mut Vec<Found>) {
        let Some(a) = c.as_array() else { return };
        let (Some(lon), Some(lat)) = (a.first().and_then(Value::as_f64), a.get(1).and_then(Value::as_f64)) else { return };
        let ele = ["ele", "elevation", "altitude", "height_asl"].iter().find_map(|k| props.get(*k).and_then(Value::as_f64)).or_else(|| a.get(2).and_then(Value::as_f64));
        out.push(Found {
            name: prop_str(props, &["name", "Name", "NAME", "title", "label", "callsign"]).unwrap_or_default(),
            lat,
            lon,
            ele,
            notes: prop_str(props, &["notes", "description", "desc", "comment"]).unwrap_or_default(),
            colour: prop_str(props, &["colour", "color", "marker-color"]).and_then(|h| parse_hex(&h)),
        });
    }
    fn geometry(g: &Value, props: &Value, out: &mut Vec<Found>) {
        match g.get("type").and_then(Value::as_str) {
            Some("Point") => point(&g["coordinates"], props, out),
            Some("MultiPoint") => {
                for c in g["coordinates"].as_array().into_iter().flatten() {
                    point(c, props, out);
                }
            }
            Some("GeometryCollection") => {
                for g in g["geometries"].as_array().into_iter().flatten() {
                    geometry(g, props, out);
                }
            }
            _ => {}
        }
    }
    fn any(v: &Value, out: &mut Vec<Found>) {
        match v.get("type").and_then(Value::as_str) {
            Some("FeatureCollection") => {
                for f in v["features"].as_array().into_iter().flatten() {
                    any(f, out);
                }
            }
            Some("Feature") => geometry(&v["geometry"], &v["properties"], out),
            Some(_) => geometry(v, &Value::Null, out),
            None => {
                for f in v.as_array().into_iter().flatten() {
                    any(f, out);
                }
            }
        }
    }
    let mut out = vec![];
    any(&v, &mut out);
    Ok(out)
}

fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([b(0)?, b(2)?, b(4)?])
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The sites as a file in `f`.
pub fn write_sites(sites: &[Site], f: Format) -> String {
    let mut o = String::new();
    match f {
        Format::Gpx => {
            o.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<gpx version=\"1.1\" creator=\"Coverant\" xmlns=\"http://www.topografix.com/GPX/1/1\">\n");
            for s in sites {
                o.push_str(&format!("  <wpt lat=\"{:.7}\" lon=\"{:.7}\">\n", s.lat, s.lon));
                if let Some(g) = s.ground_m {
                    o.push_str(&format!("    <ele>{g:.1}</ele>\n"));
                }
                o.push_str(&format!("    <name>{}</name>\n", xml(&s.name)));
                if !s.notes.is_empty() {
                    o.push_str(&format!("    <desc>{}</desc>\n", xml(&s.notes)));
                }
                o.push_str("  </wpt>\n");
            }
            o.push_str("</gpx>\n");
        }
        Format::Kml => {
            o.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<kml xmlns=\"http://www.opengis.net/kml/2.2\">\n<Document>\n  <name>Coverant sites</name>\n");
            for s in sites {
                let [r, g, b] = s.colour;
                o.push_str("  <Placemark>\n");
                o.push_str(&format!("    <name>{}</name>\n", xml(&s.name)));
                if !s.notes.is_empty() {
                    o.push_str(&format!("    <description>{}</description>\n", xml(&s.notes)));
                }
                o.push_str(&format!("    <Style><IconStyle><color>ff{b:02x}{g:02x}{r:02x}</color></IconStyle></Style>\n"));
                match s.ground_m {
                    Some(h) => o.push_str(&format!("    <Point><altitudeMode>absolute</altitudeMode><coordinates>{:.7},{:.7},{h:.1}</coordinates></Point>\n", s.lon, s.lat)),
                    None => o.push_str(&format!("    <Point><coordinates>{:.7},{:.7}</coordinates></Point>\n", s.lon, s.lat)),
                }
                o.push_str("  </Placemark>\n");
            }
            o.push_str("</Document>\n</kml>\n");
        }
        Format::GeoJson => {
            let features: Vec<serde_json::Value> = sites
                .iter()
                .map(|s| {
                    let [r, g, b] = s.colour;
                    let mut coords = vec![round7(s.lon), round7(s.lat)];
                    if let Some(h) = s.ground_m {
                        coords.push(h);
                    }
                    serde_json::json!({
                        "type": "Feature",
                        "geometry": {"type": "Point", "coordinates": coords},
                        "properties": {
                            "name": s.name,
                            "notes": s.notes,
                            "ele": s.ground_m,
                            "marker-color": format!("#{r:02x}{g:02x}{b:02x}"),
                            "locator": crate::coords::to_maidenhead(s.lat, s.lon, 6),
                        }
                    })
                })
                .collect();
            o = serde_json::to_string_pretty(&serde_json::json!({"type": "FeatureCollection", "features": features})).unwrap();
            o.push('\n');
        }
    }
    o
}

fn round7(x: f64) -> f64 {
    (x * 1e7).round() / 1e7
}

/// True if `f` is already in the list: the same name within 50 m, or anything within 5 m.
pub fn already_have(sites: &[Site], f: &Found) -> bool {
    sites.iter().any(|s| {
        let d = crate::terrain::distance_bearing(s.lat, s.lon, f.lat, f.lon).0;
        d < 5.0 || (d < 50.0 && s.name.trim().eq_ignore_ascii_case(f.name.trim()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(name: &str, lat: f64, lon: f64, ground: Option<f64>) -> Site {
        Site { id: crate::model::new_id(), name: name.into(), lat, lon, ground_m: ground, notes: "On the hill & by the mast".into(), ..Default::default() }
    }

    #[test]
    fn every_format_reads_back_what_it_wrote() {
        let sites = vec![site("Alpha <1>", 52.123_456_7, -1.234_567_8, Some(212.0)), site("Bravo", 51.5, 0.25, None)];
        for f in Format::ALL {
            let got = read_sites(write_sites(&sites, f).as_bytes()).unwrap();
            assert_eq!(got.len(), 2, "{f:?}");
            assert_eq!(got[0].name, "Alpha <1>", "{f:?}");
            assert!((got[0].lat - 52.123_456_7).abs() < 1e-6 && (got[0].lon + 1.234_567_8).abs() < 1e-6, "{f:?}");
            assert_eq!(got[0].ele, Some(212.0), "{f:?}");
            assert_eq!(got[1].ele, None, "{f:?}");
            assert_eq!(got[0].notes, "On the hill & by the mast", "{f:?}");
        }
    }

    #[test]
    fn reads_files_from_other_programs() {
        let gpx = r#"<?xml version="1.0"?><gpx xmlns="http://www.topografix.com/GPX/1/0" version="1.0">
            <wpt lat="53.0" lon="-2.0"><ele>300</ele><name>GB3XX</name><cmt>repeater</cmt></wpt>
            <trk><trkseg><trkpt lat="1" lon="1"/></trkseg></trk></gpx>"#;
        let g = read_sites(gpx.as_bytes()).unwrap();
        assert_eq!((g.len(), g[0].name.as_str(), g[0].ele, g[0].notes.as_str()), (1, "GB3XX", Some(300.0), "repeater"));

        let kml = r#"<kml xmlns="http://earth.google.com/kml/2.1"><Document><Folder>
            <Placemark><name>Mast</name><description><![CDATA[<b>Tall</b><br>one]]></description>
            <Point><coordinates> -3.5,54.25,0 </coordinates></Point></Placemark>
            <Placemark><name>Area</name><Polygon/></Placemark></Folder></Document></kml>"#;
        let k = read_sites(kml.as_bytes()).unwrap();
        assert_eq!(k.len(), 1);
        assert_eq!((k[0].lat, k[0].lon, k[0].ele), (54.25, -3.5, None));
        assert_eq!(k[0].notes, "Tall\none");

        let gj = r##"{"type":"Feature","properties":{"title":"Beacon","marker-color":"#ff8000"},"geometry":{"type":"MultiPoint","coordinates":[[0.1,51.1],[0.2,51.2,40]]}}"##;
        let j = read_sites(gj.as_bytes()).unwrap();
        assert_eq!(j.len(), 2);
        assert_eq!((j[1].name.as_str(), j[1].ele, j[1].colour), ("Beacon", Some(40.0), Some([255, 128, 0])));

        assert!(read_sites(b"PK\x03\x04").unwrap_err().contains("KMZ"));
        assert!(read_sites(br#"{"type":"FeatureCollection","features":[]}"#).is_err());
    }

    #[test]
    fn backup_round_trip_and_merge() {
        let mut lib = Library { sites: vec![site("Alpha", 52.0, -1.0, None)], ..Default::default() };
        lib.antennas.push(AntennaDef { id: "a1".into(), ..Default::default() });
        let b = Backup::parse(&Backup::new(&lib, &Settings::default()).to_json()).unwrap();
        assert_eq!(b.library(), lib);
        assert_eq!(b.settings, Some(Settings::default()));
        assert!(Backup::parse("{}").is_err());

        let mut other = Library { sites: vec![site("Bravo", 51.0, 0.0, None)], ..Default::default() };
        other.antennas.push(AntennaDef { id: "a1".into(), name: "changed".into(), ..Default::default() });
        let m = merge(&mut other, &lib);
        assert_eq!(m, Merged { sites: 1, ..Default::default() });
        assert_eq!(other.antennas[0].name, "changed");
        assert!(already_have(&other.sites, &Found { name: "alpha".into(), lat: 52.0002, lon: -1.0, ele: None, notes: String::new(), colour: None }));
    }

    #[test]
    fn dates() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20_419), (2025, 11, 27));
        assert_eq!(civil(11_016), (2000, 2, 29));
    }
}
