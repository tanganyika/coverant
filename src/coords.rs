//! Coordinates: decimal degrees, degrees-minutes-seconds and degrees + decimal
//! minutes, Maidenhead locators and Ordnance Survey (GB) grid references.
//!
//! OS grid conversion uses the 7-parameter Helmert transform between WGS84 and
//! OSGB36 and the National Grid transverse Mercator projection: good to about
//! 5 m, plenty for placing an antenna on a map.

use regex::Regex;
use std::sync::LazyLock;

// ------------------------------------------------------------------ Maidenhead

const LON_DIV: [f64; 5] = [20.0, 2.0, 2.0 / 24.0, 2.0 / 240.0, 2.0 / 240.0 / 24.0];
const LAT_DIV: [f64; 5] = [10.0, 1.0, 1.0 / 24.0, 1.0 / 240.0, 1.0 / 240.0 / 24.0];

pub fn to_maidenhead(lat: f64, lon: f64, chars: usize) -> String {
    let mut lon = (lon + 180.0).clamp(0.0, 359.9999999);
    let mut lat = (lat + 90.0).clamp(0.0, 179.9999999);
    let mut out = String::new();
    for i in 0..(chars / 2).min(5) {
        let a = (lon / LON_DIV[i]).floor() as u8;
        let b = (lat / LAT_DIV[i]).floor() as u8;
        lon -= a as f64 * LON_DIV[i];
        lat -= b as f64 * LAT_DIV[i];
        match i {
            0 => {
                out.push((b'A' + a) as char);
                out.push((b'A' + b) as char);
            }
            1 | 3 => {
                out.push((b'0' + a) as char);
                out.push((b'0' + b) as char);
            }
            _ => {
                out.push((b'a' + a) as char);
                out.push((b'a' + b) as char);
            }
        }
    }
    out
}

static RE_LOC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Ra-r]{2}([0-9]{2}([A-Xa-x]{2}([0-9]{2}([A-Xa-x]{2})?)?)?)?$").unwrap());

/// The centre of the square a locator names.
pub fn from_maidenhead(loc: &str) -> Option<(f64, f64)> {
    let loc = loc.trim();
    if !RE_LOC.is_match(loc) {
        return None;
    }
    let b: Vec<u8> = loc.to_ascii_uppercase().bytes().collect();
    let (mut lon, mut lat) = (-180.0, -90.0);
    let pairs = b.len() / 2;
    for i in 0..pairs {
        let (c1, c2) = (b[2 * i], b[2 * i + 1]);
        let (a, bb) = if i % 2 == 0 { (c1 - b'A', c2 - b'A') } else { (c1 - b'0', c2 - b'0') };
        lon += a as f64 * LON_DIV[i];
        lat += bb as f64 * LAT_DIV[i];
    }
    Some((lat + LAT_DIV[pairs - 1] / 2.0, lon + LON_DIV[pairs - 1] / 2.0))
}

// ------------------------------------------------------------------ OS grid

const AIRY_A: f64 = 6377563.396;
const AIRY_B: f64 = 6356256.909;
const WGS_A: f64 = 6378137.0;
const WGS_B: f64 = 6356752.314245;
const F0: f64 = 0.9996012717;
const N0: f64 = -100000.0;
const E0: f64 = 400000.0;
fn lat0() -> f64 {
    49f64.to_radians()
}
fn lon0() -> f64 {
    (-2f64).to_radians()
}

fn to_cart(lat: f64, lon: f64, a: f64, b: f64) -> (f64, f64, f64) {
    let (p, l) = (lat.to_radians(), lon.to_radians());
    let e2 = 1.0 - (b * b) / (a * a);
    let nu = a / (1.0 - e2 * p.sin().powi(2)).sqrt();
    (nu * p.cos() * l.cos(), nu * p.cos() * l.sin(), (1.0 - e2) * nu * p.sin())
}

fn from_cart(x: f64, y: f64, z: f64, a: f64, b: f64) -> (f64, f64) {
    let e2 = 1.0 - (b * b) / (a * a);
    let p = x.hypot(y);
    let mut lat = z.atan2(p * (1.0 - e2));
    for _ in 0..10 {
        let nu = a / (1.0 - e2 * lat.sin().powi(2)).sqrt();
        lat = (z + e2 * nu * lat.sin()).atan2(p);
    }
    (lat.to_degrees(), y.atan2(x).to_degrees())
}

#[allow(clippy::too_many_arguments)]
fn helmert(x: f64, y: f64, z: f64, tx: f64, ty: f64, tz: f64, s_ppm: f64, rxs: f64, rys: f64, rzs: f64) -> (f64, f64, f64) {
    let s = s_ppm * 1e-6;
    let (rx, ry, rz) = ((rxs / 3600.0).to_radians(), (rys / 3600.0).to_radians(), (rzs / 3600.0).to_radians());
    (
        tx + (1.0 + s) * x - rz * y + ry * z,
        ty + rz * x + (1.0 + s) * y - rx * z,
        tz - ry * x + rx * y + (1.0 + s) * z,
    )
}

fn meridional_arc(phi: f64) -> f64 {
    let (a, b) = (AIRY_A, AIRY_B);
    let n = (a - b) / (a + b);
    let (n2, n3) = (n * n, n * n * n);
    let (dp, sp) = (phi - lat0(), phi + lat0());
    let ma = (1.0 + n + 1.25 * n2 + 1.25 * n3) * dp;
    let mb = (3.0 * n + 3.0 * n2 + 2.625 * n3) * dp.sin() * sp.cos();
    let mc = (1.875 * n2 + 1.875 * n3) * (2.0 * dp).sin() * (2.0 * sp).cos();
    let md = (35.0 / 24.0) * n3 * (3.0 * dp).sin() * (3.0 * sp).cos();
    b * F0 * (ma - mb + mc - md)
}

/// WGS84 latitude/longitude to OS National Grid easting and northing (m).
pub fn latlon_to_os_en(lat: f64, lon: f64) -> (f64, f64) {
    let (x, y, z) = to_cart(lat, lon, WGS_A, WGS_B);
    let (x, y, z) = helmert(x, y, z, -446.448, 125.157, -542.060, 20.4894, -0.1502, -0.2470, -0.8421);
    let (lat, lon) = from_cart(x, y, z, AIRY_A, AIRY_B);
    let (phi, lam) = (lat.to_radians(), lon.to_radians());
    let e2 = 1.0 - (AIRY_B * AIRY_B) / (AIRY_A * AIRY_A);
    let (sp, cp, tp) = (phi.sin(), phi.cos(), phi.tan());
    let nu = AIRY_A * F0 / (1.0 - e2 * sp * sp).sqrt();
    let rho = AIRY_A * F0 * (1.0 - e2) / (1.0 - e2 * sp * sp).powf(1.5);
    let eta2 = nu / rho - 1.0;
    let i = meridional_arc(phi) + N0;
    let ii = nu / 2.0 * sp * cp;
    let iii = nu / 24.0 * sp * cp.powi(3) * (5.0 - tp * tp + 9.0 * eta2);
    let iiia = nu / 720.0 * sp * cp.powi(5) * (61.0 - 58.0 * tp * tp + tp.powi(4));
    let iv = nu * cp;
    let v = nu / 6.0 * cp.powi(3) * (nu / rho - tp * tp);
    let vi = nu / 120.0 * cp.powi(5) * (5.0 - 18.0 * tp * tp + tp.powi(4) + 14.0 * eta2 - 58.0 * tp * tp * eta2);
    let dl = lam - lon0();
    let n = i + ii * dl.powi(2) + iii * dl.powi(4) + iiia * dl.powi(6);
    let e = E0 + iv * dl + v * dl.powi(3) + vi * dl.powi(5);
    (e, n)
}

/// OS National Grid easting and northing to WGS84 latitude/longitude.
pub fn os_en_to_latlon(e: f64, n: f64) -> (f64, f64) {
    let e2 = 1.0 - (AIRY_B * AIRY_B) / (AIRY_A * AIRY_A);
    let mut phi = lat0();
    let mut m = 0.0;
    for _ in 0..100 {
        phi += (n - N0 - m) / (AIRY_A * F0);
        m = meridional_arc(phi);
        if (n - N0 - m).abs() < 0.00001 {
            break;
        }
    }
    let (sp, cp, tp) = (phi.sin(), phi.cos(), phi.tan());
    let nu = AIRY_A * F0 / (1.0 - e2 * sp * sp).sqrt();
    let rho = AIRY_A * F0 * (1.0 - e2) / (1.0 - e2 * sp * sp).powf(1.5);
    let eta2 = nu / rho - 1.0;
    let sec = 1.0 / cp;
    let vii = tp / (2.0 * rho * nu);
    let viii = tp / (24.0 * rho * nu.powi(3)) * (5.0 + 3.0 * tp * tp + eta2 - 9.0 * tp * tp * eta2);
    let ix = tp / (720.0 * rho * nu.powi(5)) * (61.0 + 90.0 * tp * tp + 45.0 * tp.powi(4));
    let x = sec / nu;
    let xi = sec / (6.0 * nu.powi(3)) * (nu / rho + 2.0 * tp * tp);
    let xii = sec / (120.0 * nu.powi(5)) * (5.0 + 28.0 * tp * tp + 24.0 * tp.powi(4));
    let xiia = sec / (5040.0 * nu.powi(7)) * (61.0 + 662.0 * tp * tp + 1320.0 * tp.powi(4) + 720.0 * tp.powi(6));
    let de = e - E0;
    let phi = phi - vii * de.powi(2) + viii * de.powi(4) - ix * de.powi(6);
    let lam = lon0() + x * de - xi * de.powi(3) + xii * de.powi(5) - xiia * de.powi(7);
    let (x, y, z) = to_cart(phi.to_degrees(), lam.to_degrees(), AIRY_A, AIRY_B);
    let (x, y, z) = helmert(x, y, z, 446.448, -125.157, 542.060, -20.4894, 0.1502, 0.2470, 0.8421);
    from_cart(x, y, z, WGS_A, WGS_B)
}

const GRID: &[u8] = b"ABCDEFGHJKLMNOPQRSTUVWXYZ"; // no I

pub fn en_to_gridref(e: f64, n: f64, digits: usize) -> Option<String> {
    let (e100, n100) = ((e / 100000.0).floor() as i64, (n / 100000.0).floor() as i64);
    if !(0..=6).contains(&e100) || !(0..=12).contains(&n100) {
        return None;
    }
    let l1 = (19 - n100) - (19 - n100) % 5 + (e100 + 10) / 5;
    let l2 = (19 - n100) * 5 % 25 + e100 % 5;
    let half = digits / 2;
    let div = 10f64.powi(5 - half as i32);
    let ee = ((e % 100000.0) / div).floor() as i64;
    let nn = ((n % 100000.0) / div).floor() as i64;
    Some(format!("{}{} {:0w$} {:0w$}", GRID[l1 as usize] as char, GRID[l2 as usize] as char, ee, nn, w = half))
}

static RE_GRID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([A-HJ-Z])([A-HJ-Z])\s?(\d+)\s?(\d*)$").unwrap());

/// Easting and northing of the centre of the square a grid reference names.
pub fn gridref_to_en(r: &str) -> Result<(f64, f64), String> {
    let s = r.trim().to_ascii_uppercase();
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let c = RE_GRID.captures(&s).ok_or("Not an OS grid reference")?;
    let idx = |m: &str| GRID.iter().position(|&g| g == m.as_bytes()[0]).unwrap() as i64;
    let (l1, l2) = (idx(&c[1]), idx(&c[2]));
    let e100 = (l1 - 2).rem_euclid(5) * 5 + l2 % 5;
    let n100 = (19 - (l1 / 5) * 5) - l2 / 5;
    let (g3, g4) = (&c[3], &c[4]);
    let (es, ns) = if !g4.is_empty() && g3.len() == g4.len() {
        (g3.to_string(), g4.to_string())
    } else {
        let all = format!("{g3}{g4}");
        if all.len() % 2 == 1 {
            return Err("An OS grid reference needs an even number of digits".into());
        }
        let h = all.len() / 2;
        (all[..h].to_string(), all[h..].to_string())
    };
    if es.is_empty() || es.len() > 5 {
        return Err("An OS grid reference has 2 to 10 digits".into());
    }
    let scale = 10f64.powi(5 - es.len() as i32);
    let half = if scale > 1.0 { scale / 2.0 } else { 0.0 };
    let e = e100 as f64 * 100000.0 + es.parse::<f64>().unwrap() * scale + half;
    let n = n100 as f64 * 100000.0 + ns.parse::<f64>().unwrap() * scale + half;
    Ok((e, n))
}

// ------------------------------------------------------------------ DMS

pub fn to_dms(v: f64, is_lat: bool) -> String {
    let hemi = match (is_lat, v >= 0.0) {
        (true, true) => 'N',
        (true, false) => 'S',
        (false, true) => 'E',
        (false, false) => 'W',
    };
    let a = v.abs();
    let mut d = a.floor() as i64;
    let mut m = ((a - d as f64) * 60.0).floor() as i64;
    let mut s = (a - d as f64 - m as f64 / 60.0) * 3600.0;
    if s >= 59.995 {
        s = 0.0;
        m += 1;
    }
    if m == 60 {
        m = 0;
        d += 1;
    }
    format!("{d}°{m:02}'{s:05.2}\"{hemi}")
}

static RE_ANGLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)^\s*([NSEW])?\s*(-?\d+(?:\.\d+)?)\s*(?:[°d:\s]\s*(\d+(?:\.\d+)?)\s*(?:['′m:\s]\s*(\d+(?:\.\d+)?)\s*(?:["″s]|'')?)?)?\s*['′]?\s*([NSEW])?\s*$"#).unwrap()
});

fn parse_angle(t: &str) -> Result<(f64, char), String> {
    let c = RE_ANGLE.captures(t).ok_or_else(|| format!("Can't read the angle '{}'", t.trim()))?;
    let hemi = c.get(1).or(c.get(5)).map(|m| m.as_str().to_ascii_uppercase().chars().next().unwrap()).unwrap_or(' ');
    let deg = &c[2];
    let mut v = deg.trim_start_matches('-').parse::<f64>().unwrap();
    if let Some(m) = c.get(3) {
        v += m.as_str().parse::<f64>().unwrap() / 60.0;
    }
    if let Some(s) = c.get(4) {
        v += s.as_str().parse::<f64>().unwrap() / 3600.0;
    }
    let neg = deg.starts_with('-') || hemi == 'S' || hemi == 'W';
    Ok((if neg { -v } else { v }, hemi))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Decimal,
    Dms,
    Maidenhead,
    OsGrid,
    OsEastingNorthing,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Decimal => "decimal degrees",
            Kind::Dms => "degrees, minutes, seconds",
            Kind::Maidenhead => "Maidenhead locator",
            Kind::OsGrid => "OS grid reference",
            Kind::OsEastingNorthing => "OS easting, northing",
        }
    }
}

static RE_GRID_LOOSE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-HJ-Za-hj-z]{2}\s?\d+(\s?\d+)?$").unwrap());
static RE_TWO_NUM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(-?\d+(?:\.\d+)?)\s*[,;\s]\s*(-?\d+(?:\.\d+)?)$").unwrap());
static RE_SPLIT_NS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(.+?[NS])\s*,?\s*(.+)$").unwrap());
static RE_SPLIT_PRE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^([NS].+?)\s*,?\s*([EW].+)$").unwrap());

/// Read a location typed in any of the supported forms.
pub fn parse_location(text: &str) -> Result<(f64, f64, Kind), String> {
    let t = text.trim();
    if t.is_empty() {
        return Err("Type a location first".into());
    }
    if let Some((lat, lon)) = from_maidenhead(t) {
        return Ok((lat, lon, Kind::Maidenhead));
    }
    if RE_GRID_LOOSE.is_match(t) {
        let (e, n) = gridref_to_en(t)?;
        let (lat, lon) = os_en_to_latlon(e, n);
        return Ok((lat, lon, Kind::OsGrid));
    }
    if let Some(c) = RE_TWO_NUM.captures(t) {
        let a: f64 = c[1].parse().unwrap();
        let b: f64 = c[2].parse().unwrap();
        if a.abs() <= 90.0 && b.abs() <= 180.0 {
            return Ok((a, b, Kind::Decimal));
        }
        if (0.0..=700000.0).contains(&a) && (0.0..=1300000.0).contains(&b) {
            let (lat, lon) = os_en_to_latlon(a, b);
            return Ok((lat, lon, Kind::OsEastingNorthing));
        }
        return Err("Those numbers are out of range for a latitude/longitude or OS easting/northing".into());
    }
    let mut parts: Option<(String, String)> = None;
    if let Some(c) = RE_SPLIT_NS.captures(t) {
        if c[2].to_ascii_uppercase().contains(['E', 'W']) {
            parts = Some((c[1].to_string(), c[2].to_string()));
        }
    }
    if parts.is_none() {
        if let Some(c) = RE_SPLIT_PRE.captures(t) {
            parts = Some((c[1].to_string(), c[2].to_string()));
        }
    }
    if parts.is_none() {
        if let Some((a, b)) = t.split_once(',') {
            parts = Some((a.to_string(), b.to_string()));
        }
    }
    let (a, b) = parts.ok_or(
        "Couldn't read that location. Try 52.2053, 0.1218 or 52°12'19\"N 0°07'18\"E or JO02bf or TL 450 585.",
    )?;
    let (mut lat, h1) = parse_angle(&a)?;
    let (mut lon, h2) = parse_angle(&b)?;
    if matches!(h1, 'E' | 'W') && matches!(h2, 'N' | 'S') {
        std::mem::swap(&mut lat, &mut lon);
    }
    if lat.abs() > 90.0 || lon.abs() > 180.0 {
        return Err("Latitude or longitude out of range".into());
    }
    Ok((lat, lon, Kind::Dms))
}

/// Every way of writing a position, for display.
#[derive(Clone, Debug, Default)]
pub struct Described {
    pub decimal: String,
    pub dms: String,
    pub locator: String,
    pub locator10: String,
    pub osgrid: Option<String>,
}

pub fn describe(lat: f64, lon: f64) -> Described {
    let osgrid = if (49.0..=61.5).contains(&lat) && (-9.5..=2.5).contains(&lon) {
        let (e, n) = latlon_to_os_en(lat, lon);
        en_to_gridref(e, n, 10)
    } else {
        None
    };
    Described {
        decimal: format!("{lat:.5}, {lon:.5}"),
        dms: format!("{} {}", to_dms(lat, true), to_dms(lon, false)),
        locator: to_maidenhead(lat, lon, 6),
        locator10: to_maidenhead(lat, lon, 10),
        osgrid,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ben_nevis() {
        let (lat, lon, k) = parse_location("NN 16667 71283").unwrap();
        assert_eq!(k, Kind::OsGrid);
        assert!((lat - 56.79685).abs() < 2e-4 && (lon + 5.00360).abs() < 2e-4, "{lat} {lon}");
        assert!(describe(lat, lon).osgrid.unwrap().starts_with("NN 1666"));
        let (lat2, _, k) = parse_location("216667,771283").unwrap();
        assert_eq!(k, Kind::OsEastingNorthing);
        assert!((lat - lat2).abs() < 1e-6);
    }

    #[test]
    fn locators() {
        let (lat, lon, _) = parse_location("IO92wh").unwrap();
        assert_eq!(to_maidenhead(lat, lon, 6), "IO92wh");
        assert!((lat - 52.3125).abs() < 1e-9 && (lon + 0.125).abs() < 1e-9);
    }

    #[test]
    fn dms_forms() {
        let a = parse_location("52°12'19\"N 0°07'18\"W").unwrap();
        let b = parse_location("N52 12.3167 W000 07.3").unwrap();
        let c = parse_location("52 12 19 N, 0 7 18 W").unwrap();
        assert!((a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4 && a.1 < 0.0);
        assert!((a.0 - c.0).abs() < 1e-6 && (a.1 - c.1).abs() < 1e-6);
        let d = parse_location("-33.86, 151.21").unwrap();
        assert_eq!(d.2, Kind::Decimal);
    }

    #[test]
    fn greenwich() {
        let (lat, lon, _) = parse_location("51 28 40.1N 0 0 5.3W").unwrap();
        let g = describe(lat, lon).osgrid.unwrap();
        assert!(g.starts_with("TQ 3888"), "{g}");
    }
}
