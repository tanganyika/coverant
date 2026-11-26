//! Longley-Rice Irregular Terrain Model (ITM) v1.2.2, point-to-point mode.
//!
//! A straight port of the NTIA/ITS reference algorithm ("The ITS Irregular
//! Terrain Model, version 1.2.2: The Algorithm", G. A. Hufford). The terrain
//! profile is an array `z` of ground elevations (m above sea level), equally
//! spaced `xi` metres apart, from the transmitter (z[0]) to the receiver.

use num_complex::Complex64;

const THIRD: f64 = 1.0 / 3.0;

pub const CLIMATES: [(u8, &str); 7] = [
    (1, "Equatorial"),
    (2, "Continental subtropical"),
    (3, "Maritime subtropical"),
    (4, "Desert"),
    (5, "Continental temperate"),
    (6, "Maritime temperate, over land"),
    (7, "Maritime temperate, over sea"),
];

const BV1: [f64; 7] = [-9.67, -0.62, 1.26, -9.21, -0.62, -0.39, 3.15];
const BV2: [f64; 7] = [12.7, 9.19, 15.5, 9.05, 9.19, 2.86, 857.9];
const XV1: [f64; 7] = [144.9e3, 228.9e3, 262.6e3, 84.1e3, 228.9e3, 141.7e3, 2222.0e3];
const XV2: [f64; 7] = [190.3e3, 205.2e3, 185.2e3, 101.1e3, 205.2e3, 315.9e3, 164.8e3];
const XV3: [f64; 7] = [133.8e3, 143.6e3, 99.8e3, 98.6e3, 143.6e3, 167.4e3, 116.3e3];
const BSM1: [f64; 7] = [2.13, 2.66, 6.11, 1.98, 2.68, 6.86, 8.51];
const BSM2: [f64; 7] = [159.5, 7.67, 6.65, 13.11, 7.16, 10.38, 169.8];
const XSM1: [f64; 7] = [762.2e3, 100.4e3, 138.2e3, 139.1e3, 93.7e3, 187.8e3, 609.8e3];
const XSM2: [f64; 7] = [123.6e3, 172.5e3, 242.2e3, 132.7e3, 186.8e3, 169.6e3, 119.9e3];
const XSM3: [f64; 7] = [94.5e3, 136.4e3, 178.6e3, 193.5e3, 133.5e3, 108.9e3, 106.6e3];
const BSP1: [f64; 7] = [2.11, 6.87, 10.08, 3.68, 4.75, 8.58, 8.43];
const BSP2: [f64; 7] = [102.3, 15.53, 9.60, 159.3, 8.12, 13.97, 8.19];
const XSP1: [f64; 7] = [636.9e3, 138.7e3, 165.3e3, 464.4e3, 93.2e3, 216.0e3, 136.2e3];
const XSP2: [f64; 7] = [134.8e3, 143.7e3, 225.7e3, 93.1e3, 135.9e3, 152.0e3, 188.5e3];
const XSP3: [f64; 7] = [95.6e3, 98.6e3, 129.7e3, 94.2e3, 113.4e3, 122.7e3, 122.9e3];
const BSD1: [f64; 7] = [1.224, 0.801, 1.380, 1.000, 1.224, 1.518, 1.518];
const BZD1: [f64; 7] = [1.282, 2.161, 1.282, 20.0, 1.282, 1.282, 1.282];
const BFM1: [f64; 7] = [1.0, 1.0, 1.0, 1.0, 0.92, 1.0, 1.0];
const BFM2: [f64; 7] = [0.0, 0.0, 0.0, 0.0, 0.25, 0.0, 0.0];
const BFM3: [f64; 7] = [0.0, 0.0, 0.0, 0.0, 1.77, 0.0, 0.0];
const BFP1: [f64; 7] = [1.0, 0.93, 1.0, 0.93, 0.93, 1.0, 1.0];
const BFP2: [f64; 7] = [0.0, 0.31, 0.0, 0.19, 0.31, 0.0, 0.0];
const BFP3: [f64; 7] = [0.0, 2.00, 0.0, 1.79, 2.00, 0.0, 0.0];

/// Parameters that stay the same for every path in a plot.
#[derive(Clone, Copy, Debug)]
pub struct ItmParams {
    pub fmhz: f64,
    pub eps: f64,
    pub sgm: f64,
    pub ens0: f64,
    /// 0 = horizontal, 1 = vertical
    pub pol: i32,
    pub klim: i32,
    /// standard normal deviates for time, location and situation (0 = median)
    pub zt: f64,
    pub zl: f64,
    pub zc: f64,
    pub mdvar: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    LineOfSight = 0,
    Diffraction = 1,
    Troposcatter = 2,
}

/// Inverse complementary normal distribution: the standard normal deviate
/// exceeded with probability q.
pub fn qerfi(q: f64) -> f64 {
    let (c0, c1, c2) = (2.515516698, 0.802853, 0.010328);
    let (d1, d2, d3) = (1.432788, 0.189269, 0.001308);
    let x = 0.5 - q;
    let t = (0.5 - x.abs()).max(0.000001);
    let t = (-2.0 * t.ln()).sqrt();
    let v = t - ((c2 * t + c1) * t + c0) / (((d3 * t + d2) * t + d1) * t + 1.0);
    if x < 0.0 { -v } else { v }
}

#[inline]
fn dim(x: f64, y: f64) -> f64 {
    if x > y { x - y } else { 0.0 }
}

fn aknfe(v2: f64) -> f64 {
    if v2 < 5.76 {
        6.02 + 9.11 * v2.sqrt() - 1.27 * v2
    } else {
        12.953 + 4.343 * v2.ln()
    }
}

fn fht(x: f64, pk: f64) -> f64 {
    if x < 200.0 {
        let w = -pk.ln();
        if pk < 1e-5 || x * w.powi(3) > 5495.0 {
            let mut f = -117.0;
            if x > 1.0 {
                f += 17.372 * x.ln();
            }
            f
        } else {
            2.5e-5 * x * x / pk - 8.686 * w - 15.0
        }
    } else {
        let mut f = 0.05751 * x - 4.343 * x.ln();
        if x < 2000.0 {
            let w = 0.0134 * x * (-0.005 * x).exp();
            f = (1.0 - w) * f + w * (17.372 * x.ln() - 117.0);
        }
        f
    }
}

fn h0f(r: f64, et: f64) -> f64 {
    const A: [f64; 5] = [25.0, 80.0, 177.0, 395.0, 705.0];
    const B: [f64; 5] = [24.0, 45.0, 68.0, 80.0, 105.0];
    let mut it = et as i64;
    let q;
    if it <= 0 {
        it = 1;
        q = 0.0;
    } else if it >= 5 {
        it = 5;
        q = 0.0;
    } else {
        q = et - it as f64;
    }
    let it = it as usize;
    let x = (1.0 / r).powi(2);
    let mut h = 4.343 * ((A[it - 1] * x + B[it - 1]) * x + 1.0).ln();
    if q != 0.0 {
        h = (1.0 - q) * h + q * 4.343 * ((A[it] * x + B[it]) * x + 1.0).ln();
    }
    h
}

fn ahd(td: f64) -> f64 {
    if td <= 10e3 {
        133.4 + 0.332e-3 * td - 4.343 * td.ln()
    } else if td <= 70e3 {
        104.6 + 0.212e-3 * td - 1.086 * td.ln()
    } else {
        71.8 + 0.157e-3 * td + 2.171 * td.ln()
    }
}

fn curv(c1: f64, c2: f64, x1: f64, x2: f64, x3: f64, de: f64) -> f64 {
    (c1 + c2 / (1.0 + ((de - x2) / x3).powi(2))) * (de / x1).powi(2) / (1.0 + (de / x1).powi(2))
}

/// Least squares line fit to z (n_last+1 points, spacing xi) between x1 and x2.
fn zlsq1(z: &[f64], n_last: usize, xi: f64, x1: f64, x2: f64) -> (f64, f64) {
    let xn = n_last as f64;
    let mut xa = (dim(x1 / xi, 0.0) as i64) as f64;
    let mut xb = xn - (dim(xn, x2 / xi) as i64) as f64;
    if xb <= xa {
        xa = dim(xa, 1.0);
        xb = xn - dim(xn, xb + 1.0);
    }
    let mut ja = xa as usize;
    let jb = xb as usize;
    let n = jb as i64 - ja as i64;
    xa = xb - xa;
    let mut x = -0.5 * xa;
    xb += x;
    let mut a = 0.5 * (z[ja] + z[jb]);
    let mut b = 0.5 * (z[ja] - z[jb]) * x;
    let mut i = 2;
    while i <= n {
        ja += 1;
        x += 1.0;
        a += z[ja];
        b += z[ja] * x;
        i += 1;
    }
    a /= xa;
    b = b * 12.0 / ((xa * xa + 2.0) * xa);
    (a - b * xb, a + b * (xn - xb))
}

/// Interdecile range of terrain heights between x1 and x2 (delta h).
fn dlthx(z: &[f64], n_last: usize, xi: f64, x1: f64, x2: f64) -> f64 {
    let mut xa = x1 / xi;
    let mut xb = x2 / xi;
    if xb - xa < 2.0 {
        return 0.0;
    }
    let ka = ((0.1 * (xb - xa + 8.0)) as i64).clamp(4, 25) as usize;
    let n = 10 * ka - 5;
    let kb = n - ka + 1;
    let sn = (n - 1) as f64;
    let mut s = vec![0.0f64; n];
    xb = (xb - xa) / sn;
    let mut k = (xa + 1.0) as usize;
    xa -= k as f64;
    for sj in s.iter_mut() {
        while xa > 0.0 && k < n_last {
            xa -= 1.0;
            k += 1;
        }
        *sj = z[k] + (z[k] - z[k - 1]) * xa;
        xa += xb;
    }
    let (mut za, zb) = zlsq1(&s, n - 1, 1.0, 0.0, sn);
    let zb = (zb - za) / sn;
    for sj in s.iter_mut() {
        *sj -= za;
        za += zb;
    }
    s.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let d = s[ka - 1] - s[kb - 1];
    d / (1.0 - 0.8 * (-(x2 - x1) / 50.0e3).exp())
}

#[allow(clippy::too_many_arguments)]
fn alos(d: f64, dh: f64, he0: f64, he1: f64, zgnd: Complex64, wn: f64, emd: f64, aed: f64, wls: f64) -> f64 {
    let q = (1.0 - 0.8 * (-d / 50e3).exp()) * dh;
    let s = 0.78 * q * (-(q / 16.0).powf(0.25)).exp();
    let q = he0 + he1;
    let sps = q / (d * d + q * q).sqrt();
    let spsc = Complex64::new(sps, 0.0);
    let mut r = (spsc - zgnd) / (spsc + zgnd) * (-(wn * s * sps).min(10.0)).exp();
    let q = r.norm_sqr();
    if q < 0.25 || q < sps {
        r *= (sps / q).sqrt();
    }
    let mut alosv = emd * d + aed;
    let mut q = wn * he0 * he1 * 2.0 / d;
    if q > 1.57 {
        q = 3.14 - 2.4649 / q;
    }
    let c = Complex64::new(q.cos(), -q.sin()) + r;
    alosv = (-4.343 * c.norm_sqr().ln() - alosv) * wls + alosv;
    alosv
}

#[allow(clippy::too_many_arguments)]
fn ascat(
    d: f64, h0s: &mut f64, the0: f64, the1: f64, gme: f64, wn: f64, he0: f64, he1: f64, ad: f64,
    rr: f64, etq: f64, tha: f64, ens: f64,
) -> f64 {
    let h0;
    if *h0s > 15.0 {
        h0 = *h0s;
    } else {
        let th = the0 + the1 + d * gme;
        let r2 = 2.0 * wn * th;
        let r1 = r2 * he0;
        let r2 = r2 * he1;
        if r1 < 0.2 && r2 < 0.2 {
            return 1001.0;
        }
        let mut ss = (d - ad) / (d + ad);
        let mut q = rr / ss;
        ss = ss.max(0.1);
        q = q.max(0.1).min(10.0);
        let z0 = (d - ad) * (d + ad) * th * 0.25 / d;
        let et = (etq * (-(z0 / 8.0e3).min(1.7).powf(6.0)).exp() + 1.0) * z0 / 1.7556e3;
        let ett = et.max(1.0);
        let mut h = (h0f(r1, ett) + h0f(r2, ett)) * 0.5;
        h += h.min((1.38 - ett.ln()) * ss.ln() * q.ln() * 0.49);
        h = dim(h, 0.0);
        if et < 1.0 {
            h = et * h
                + (1.0 - et)
                    * 4.343
                    * (((1.0 + 1.4142 / r1) * (1.0 + 1.4142 / r2)).powi(2) * (r1 + r2)
                        / (r1 + r2 + 2.8284))
                        .ln();
        }
        if h > 15.0 && *h0s >= 0.0 {
            h = *h0s;
        }
        h0 = h;
    }
    *h0s = h0;
    let th = tha + d * gme;
    ahd(th * d) + 4.343 * (47.7 * wn * th.powi(4)).ln() - 0.1 * (ens - 301.0) * (-th * d / 40e3).exp() + h0
}

/// Basic transmission loss (dB) over terrain profile `z` (spacing `xi` m)
/// between antennas `h1` (at z[0]) and `h2` (at the far end), heights above
/// ground in metres.
pub fn p2p_loss(z: &[f64], xi: f64, h1: f64, h2: f64, p: &ItmParams) -> (f64, Mode) {
    let n_last = z.len() - 1;
    let dist = n_last as f64 * xi;
    let wn = p.fmhz / 47.7;

    // qlrps
    let ja = (3.0 + 0.1 * n_last as f64) as i64;
    let jb = n_last as i64 - ja + 6;
    let mut zsys = 0.0;
    let mut cnt = 0;
    let mut i = ja - 1;
    while i < jb {
        let ii = (i - 2).clamp(0, n_last as i64) as usize;
        zsys += z[ii];
        cnt += 1;
        i += 1;
    }
    zsys /= cnt.max(1) as f64;
    let mut ens = p.ens0;
    if zsys != 0.0 {
        ens *= (-zsys / 9460.0).exp();
    }
    let gme = 157e-9 * (1.0 - 0.04665 * (ens / 179.3).exp());
    let zq = Complex64::new(p.eps, 376.62 * p.sgm / wn);
    let mut zgnd = (zq - 1.0).sqrt();
    if p.pol != 0 {
        zgnd /= zq;
    }

    // hzns
    let (hg0, hg1) = (h1, h2);
    let za = z[0] + hg0;
    let zb = z[n_last] + hg1;
    let qc = 0.5 * gme;
    let q = qc * dist;
    let mut the1 = (zb - za) / dist;
    let mut the0 = the1 - q;
    the1 = -the1 - q;
    let mut dl0 = dist;
    let mut dl1 = dist;
    if n_last >= 2 {
        let mut sa = 0.0;
        let mut sb = dist;
        let mut wq = true;
        for &zi in &z[1..n_last] {
            sa += xi;
            sb -= xi;
            let q = zi - (qc * sa + the0) * sa - za;
            if q > 0.0 {
                the0 += q / sa;
                dl0 = sa;
                wq = false;
            }
            if !wq {
                let q = zi - (qc * sb + the1) * sb - zb;
                if q > 0.0 {
                    the1 += q / sb;
                    dl1 = sb;
                }
            }
        }
    }

    // qlrpfl
    let xl0 = (15.0 * hg0).min(0.1 * dl0);
    let xl1 = dist - (15.0 * hg1).min(0.1 * dl1);
    let dh = dlthx(z, n_last, xi, xl0, xl1);
    let (he0, he1);
    if dl0 + dl1 > 1.5 * dist {
        let (za_, zb_) = zlsq1(z, n_last, xi, xl0, xl1);
        let mut h0 = hg0 + dim(z[0], za_);
        let mut h1_ = hg1 + dim(z[n_last], zb_);
        dl0 = (2.0 * h0 / gme).sqrt() * (-0.07 * (dh / h0.max(5.0)).sqrt()).exp();
        dl1 = (2.0 * h1_ / gme).sqrt() * (-0.07 * (dh / h1_.max(5.0)).sqrt()).exp();
        let q = dl0 + dl1;
        if q <= dist {
            let q = (dist / q).powi(2);
            h0 *= q;
            h1_ *= q;
            dl0 = (2.0 * h0 / gme).sqrt() * (-0.07 * (dh / h0.max(5.0)).sqrt()).exp();
            dl1 = (2.0 * h1_ / gme).sqrt() * (-0.07 * (dh / h1_.max(5.0)).sqrt()).exp();
        }
        let q = (2.0 * h0 / gme).sqrt();
        the0 = (0.65 * dh * (q / dl0 - 1.0) - 2.0 * h0) / q;
        let q = (2.0 * h1_ / gme).sqrt();
        the1 = (0.65 * dh * (q / dl1 - 1.0) - 2.0 * h1_) / q;
        he0 = h0;
        he1 = h1_;
    } else {
        let (za_, _) = zlsq1(z, n_last, xi, xl0, 0.9 * dl0);
        let (_, zb_) = zlsq1(z, n_last, xi, dist - 0.9 * dl1, xl1);
        he0 = hg0 + dim(z[0], za_);
        he1 = hg1 + dim(z[n_last], zb_);
    }

    // lrprop
    let dls0 = (2.0 * he0 / gme).sqrt();
    let dls1 = (2.0 * he1 / gme).sqrt();
    let dlsa = dls0 + dls1;
    let dla = dl0 + dl1;
    let tha = (the0 + the1).max(-dla * gme);

    // adiff(0)
    let mut q = hg0 * hg1;
    let qk = he0 * he1 - q;
    q += 10.0;
    let wd1 = (1.0 + qk / q).sqrt();
    let xd1 = dla + tha / gme;
    let mut q = (1.0 - 0.8 * (-dlsa / 50e3).exp()) * dh;
    q *= 0.78 * (-(q / 16.0).powf(0.25)).exp();
    let afo = (2.171 * (1.0 + 4.77e-4 * hg0 * hg1 * wn * q).ln()).min(15.0);
    let qk = 1.0 / zgnd.norm();
    let mut aht = 20.0;
    let mut xht = 0.0;
    for (dlj, hej) in [(dl0, he0), (dl1, he1)] {
        let a = 0.5 * dlj * dlj / hej;
        let wa = (a * wn).powf(THIRD);
        let pk = qk / wa;
        let q = (1.607 - pk) * 151.0 * wa * dlj / a;
        xht += q;
        aht += fht(q, pk);
    }
    let adiff = |d: f64| -> f64 {
        let th = tha + d * gme;
        let ds = d - dla;
        let q = 0.0795775 * wn * ds * th * th;
        let ad = aknfe(q * dl0 / (ds + dl0)) + aknfe(q * dl1 / (ds + dl1));
        let a = ds / th;
        let wa = (a * wn).powf(THIRD);
        let pk = qk / wa;
        let q = (1.607 - pk) * 151.0 * wa * th + xht;
        let ar = 0.05751 * q - 4.343 * q.ln() - aht;
        let q = (wd1 + xd1 / d) * ((1.0 - 0.8 * (-d / 50e3).exp()) * dh * wn).min(6283.2);
        let wd = 25.1 / (25.1 + q.sqrt());
        ar * wd + (1.0 - wd) * ad + afo
    };
    let xae = (wn * gme * gme).powf(-THIRD);
    let d3 = dlsa.max(1.3787 * xae + dla);
    let d4 = d3 + 2.7574 * xae;
    let a3 = adiff(d3);
    let a4 = adiff(d4);
    let emd = (a4 - a3) / (d4 - d3);
    let aed = a3 - emd * d3;

    let mut aref;
    let mode;
    if dist < dlsa {
        let wls = 0.021 / (0.021 + wn * dh / dlsa.max(10e3));
        let d2 = dlsa;
        let a2 = aed + d2 * emd;
        let mut d0 = 1.908 * wn * he0 * he1;
        let d1;
        if aed >= 0.0 {
            d0 = d0.min(0.5 * dla);
            d1 = d0 + 0.25 * (dla - d0);
        } else {
            d1 = (-aed / emd).max(0.25 * dla);
        }
        let a1 = alos(d1, dh, he0, he1, zgnd, wn, emd, aed, wls);
        let mut wq = false;
        let mut ak1 = 0.0;
        let mut ak2 = 0.0;
        if d0 < d1 {
            let a0 = alos(d0, dh, he0, he1, zgnd, wn, emd, aed, wls);
            let q = (d2 / d0).ln();
            ak2 = (((d2 - d0) * (a1 - a0) - (d1 - d0) * (a2 - a0))
                / ((d2 - d0) * (d1 / d0).ln() - (d1 - d0) * q))
                .max(0.0);
            wq = aed >= 0.0 || ak2 > 0.0;
            if wq {
                ak1 = (a2 - a0 - ak2 * q) / (d2 - d0);
                if ak1 < 0.0 {
                    ak1 = 0.0;
                    ak2 = dim(a2, a0) / q;
                    if ak2 == 0.0 {
                        ak1 = emd;
                    }
                }
            }
        }
        if !wq {
            ak1 = dim(a2, a1) / (d2 - d1);
            ak2 = 0.0;
            if ak1 == 0.0 {
                ak1 = emd;
            }
        }
        let ael = a2 - ak1 * d2 - ak2 * d2.ln();
        aref = ael + ak1 * dist + ak2 * dist.ln();
        mode = Mode::LineOfSight;
    } else {
        let mut ad = dl0 - dl1;
        let mut rr = he1 / he0;
        if ad < 0.0 {
            ad = -ad;
            rr = 1.0 / rr;
        }
        let etq = (5.67e-6 * ens - 2.32e-3) * ens + 0.031;
        let d5 = dla + 200e3;
        let d6 = d5 + 200e3;
        let mut h0s = -15.0;
        let a6 = ascat(d6, &mut h0s, the0, the1, gme, wn, he0, he1, ad, rr, etq, tha, ens);
        let a5 = ascat(d5, &mut h0s, the0, the1, gme, wn, he0, he1, ad, rr, etq, tha, ens);
        let (ems, aes, dx);
        if a5 < 1000.0 {
            ems = (a6 - a5) / 200e3;
            dx = dlsa.max((dla + 0.3 * xae * (47.7 * wn).ln()).max((a5 - aed - ems * d5) / (emd - ems)));
            aes = (emd - ems) * dx + aed;
        } else {
            ems = emd;
            aes = aed;
            dx = 10.0e6;
        }
        if dist > dx {
            aref = aes + ems * dist;
            mode = Mode::Troposcatter;
        } else {
            aref = aed + emd * dist;
            mode = Mode::Diffraction;
        }
    }
    aref = aref.max(0.0);

    // avar
    let kl = if (1..=7).contains(&p.klim) { (p.klim - 1) as usize } else { 4 };
    let (cv1, cv2, yv1, yv2, yv3) = (BV1[kl], BV2[kl], XV1[kl], XV2[kl], XV3[kl]);
    let (csm1, csm2, ysm1, ysm2, ysm3) = (BSM1[kl], BSM2[kl], XSM1[kl], XSM2[kl], XSM3[kl]);
    let (csp1, csp2, ysp1, ysp2, ysp3) = (BSP1[kl], BSP2[kl], XSP1[kl], XSP2[kl], XSP3[kl]);
    let (csd1, zd) = (BSD1[kl], BZD1[kl]);
    let (cfm1, cfm2, cfm3) = (BFM1[kl], BFM2[kl], BFM3[kl]);
    let (cfp1, cfp2, cfp3) = (BFP1[kl], BFP2[kl], BFP3[kl]);
    let mut kdv = p.mdvar;
    let ws = kdv >= 20;
    if ws {
        kdv -= 20;
    }
    let wl = kdv >= 10;
    if wl {
        kdv -= 10;
    }
    if !(0..=3).contains(&kdv) {
        kdv = 0;
    }
    let q = (0.133 * wn).ln();
    let gm = cfm1 + cfm2 / ((cfm3 * q).powi(2) + 1.0);
    let gp = cfp1 + cfp2 / ((cfp3 * q).powi(2) + 1.0);
    let dexa = (18e6 * he0).sqrt() + (18e6 * he1).sqrt() + (575.7e12 / wn).powf(THIRD);
    let de = if dist < dexa { 130e3 * dist / dexa } else { 130e3 + dist - dexa };
    let vmd = curv(cv1, cv2, yv1, yv2, yv3, de);
    let sgtm = curv(csm1, csm2, ysm1, ysm2, ysm3, de) * gm;
    let sgtp = curv(csp1, csp2, ysp1, ysp2, ysp3, de) * gp;
    let sgtd = sgtp * csd1;
    let tgtd = (sgtp - sgtd) * zd;
    let sgl = if wl {
        0.0
    } else {
        let q = (1.0 - 0.8 * (-dist / 50e3).exp()) * dh * wn;
        10.0 * q / (q + 13.0)
    };
    let vs0 = if ws { 0.0 } else { (5.0 + 3.0 * (-de / 100e3).exp()).powi(2) };
    let (mut zt, mut zl, zc) = (p.zt, p.zl, p.zc);
    match kdv {
        0 => {
            zt = zc;
            zl = zc;
        }
        1 => zl = zc,
        2 => zl = zt,
        _ => {}
    }
    let sgt = if zt < 0.0 {
        sgtm
    } else if zt <= zd {
        sgtp
    } else {
        sgtd + tgtd / zt
    };
    let vs = vs0 + (sgt * zt).powi(2) / (7.8 + zc * zc) + (sgl * zl).powi(2) / (24.0 + zc * zc);
    let (yr, sgc) = match kdv {
        0 => (0.0, (sgt * sgt + sgl * sgl + vs).sqrt()),
        1 => (sgt * zt, (sgl * sgl + vs).sqrt()),
        2 => ((sgt * sgt + sgl * sgl).sqrt() * zt, vs.sqrt()),
        _ => (sgt * zt + sgl * zl, vs.sqrt()),
    };
    let mut avar1 = aref - vmd - yr - sgc * zc;
    if avar1 < 0.0 {
        avar1 = avar1 * (29.0 - avar1) / (29.0 - 10.0 * avar1);
    }
    let fs = free_space_loss(p.fmhz, dist);
    (avar1 + fs, mode)
}

pub fn free_space_loss(fmhz: f64, dist_m: f64) -> f64 {
    32.45 + 20.0 * fmhz.log10() + 20.0 * (dist_m.max(1.0) / 1000.0).log10()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(klim: i32) -> ItmParams {
        ItmParams { fmhz: 145.0, eps: 15.0, sgm: 0.005, ens0: 301.0, pol: 1, klim, zt: 0.0, zl: 0.0, zc: 0.0, mdvar: 12 }
    }

    #[test]
    fn flat_30km_matches_reference() {
        // Reference value from the Python/numba port, itself checked against itmlogic.
        let z = vec![50.0; 301];
        let (l, m) = p2p_loss(&z, 100.0, 20.0, 1.5, &params(5));
        assert!((l - 152.26957998994766).abs() < 1e-6, "{l}");
        assert_eq!(m, Mode::Diffraction);
    }

    #[test]
    fn hill_adds_loss() {
        let mut z = vec![50.0; 201];
        let (open, _) = p2p_loss(&z, 100.0, 20.0, 1.5, &params(6));
        for (k, i) in (95..106).enumerate() {
            z[i] = 150.0 + 10.0 * k as f64;
        }
        let (hill, _) = p2p_loss(&z, 100.0, 20.0, 1.5, &params(6));
        assert!(hill > open + 5.0);
    }
}

#[cfg(test)]
mod reference {
    use super::*;

    /// 40 random profiles, frequencies, heights, climates and variabilities whose
    /// losses came from the earlier Python version (itself identical to `itmlogic`).
    #[test]
    fn matches_python_reference() {
        let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!("testdata_itm.json")).unwrap();
        for c in cases {
            let z: Vec<f64> = c["z"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
            let g = |k: &str| c[k].as_f64().unwrap();
            let p = ItmParams {
                fmhz: g("f"), eps: 15.0, sgm: 0.005, ens0: 301.0, pol: g("pol") as i32, klim: g("klim") as i32,
                zt: g("zt"), zl: g("zt"), zc: g("zc"), mdvar: 12,
            };
            let (l, m) = p2p_loss(&z, g("xi"), g("h1"), g("h2"), &p);
            assert!((l - g("loss")).abs() < 1e-6, "loss {l} vs {}", g("loss"));
            assert_eq!(m as i64, c["mode"].as_i64().unwrap());
        }
    }
}
