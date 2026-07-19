//! Flow regime classifier. Pure, closed-form (one bisection), testable.
//! Near-horizontal: Taitel-Dukler 1976 mechanistic transitions from the
//! equilibrium stratified level + Kelvin-Helmholtz criterion.
//! Steep pipes (|sin theta| > 0.6): void-fraction thresholds
//! bubbly -> slug -> churn -> annular.

use crate::closures::{MU_G, MU_L};
use crate::eos::ALPHA_EPS;

pub const PI: f64 = std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Regime {
    StratifiedSmooth = 0,
    StratifiedWavy = 1,
    Intermittent = 2, // plug + slug
    Annular = 3,
    DispersedBubble = 4,
    Bubbly = 5,
    Churn = 6,
    SingleLiquid = 7,
    SingleGas = 8,
}

/// Stratified-geometry helper: dimensionless quantities at level h~ = h/D.
/// Returns (A_l~, A_g~, S_l~, S_g~, S_i~).
fn strat_geom(h: f64) -> (f64, f64, f64, f64, f64) {
    let gam = (2.0 * h - 1.0).clamp(-1.0, 1.0);
    let ac = gam.acos();
    let si = (1.0 - gam * gam).max(0.0).sqrt();
    let al = 0.25 * (PI - ac + gam * si);
    let ag = 0.25 * PI - al;
    (al, ag, PI - ac, ac, si)
}

/// Taitel-Dukler combined momentum balance residual at level h~.
/// Root in h~ is the equilibrium stratified level.
fn td_residual(h: f64, x2: f64, y: f64) -> f64 {
    let n = 0.2;
    let (al, ag, sl, sg, si) = strat_geom(h);
    let (al, ag) = (al.max(1e-8), ag.max(1e-8));
    let ul = 0.25 * PI / al; // u_l / j_l
    let ug = 0.25 * PI / ag;
    let dl = 4.0 * al / sl.max(1e-8);
    let dg = 4.0 * ag / (sg + si).max(1e-8);
    let liq = x2 * (ul * dl).powf(-n) * ul * ul * sl / al;
    let gas = (ug * dg).powf(-n) * ug * ug * (sg / ag + si / al + si / ag);
    liq - gas - 4.0 * y
}

/// Equilibrium level by bisection (fixed 48 iterations: deterministic).
fn equilibrium_level(x2: f64, y: f64) -> f64 {
    // residual runs from +inf (liquid term dominates as h->0) down to -inf
    let (mut lo, mut hi) = (1.0e-4, 1.0 - 1.0e-4);
    if td_residual(lo, x2, y) < 0.0 {
        return lo; // gas-dominated everywhere: level ~ 0
    }
    if td_residual(hi, x2, y) > 0.0 {
        return hi; // liquid-dominated everywhere: level ~ D
    }
    for _ in 0..48 {
        let mid = 0.5 * (lo + hi);
        if td_residual(mid, x2, y) > 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// Superficial-flow Darcy friction gradient [Pa/m], Blasius-style.
fn dpdx_superficial(rho: f64, j: f64, d: f64, mu: f64) -> f64 {
    let re = (rho * j.abs() * d / mu).max(1.0);
    let f = if re < 2300.0 {
        64.0 / re
    } else {
        0.316 / re.powf(0.25)
    };
    f * rho * j * j / (2.0 * d)
}

/// Classify one cell. alpha is the void fraction, jg/jl the phase superficial
/// velocities [m/s], d diameter [m], sin_th/cos_th pipe inclination.
#[allow(clippy::too_many_arguments)]
pub fn classify(
    alpha: f64,
    jg: f64,
    jl: f64,
    d: f64,
    sin_th: f64,
    cos_th: f64,
    rho_g: f64,
    rho_l: f64,
    g: f64,
) -> Regime {
    if alpha <= 100.0 * ALPHA_EPS {
        return Regime::SingleLiquid;
    }
    if alpha >= 1.0 - 100.0 * ALPHA_EPS {
        return Regime::SingleGas;
    }
    if sin_th.abs() > 0.6 {
        // vertical branch: void-fraction thresholds
        return if alpha < 0.25 {
            Regime::Bubbly
        } else if alpha < 0.52 {
            Regime::Intermittent
        } else if alpha < 0.78 {
            Regime::Churn
        } else {
            Regime::Annular
        };
    }
    let jg = jg.abs().max(1.0e-6);
    let jl = jl.abs().max(1.0e-6);
    let cos_th = cos_th.abs().max(0.1);
    let dpg = dpdx_superficial(rho_g, jg, d, MU_G);
    let dpl = dpdx_superficial(rho_l, jl, d, MU_L);
    let x2 = dpl / dpg;
    let drho = (rho_l - rho_g).max(1.0);
    let y = -drho * g * sin_th / dpg; // TD's Y, positive downhill
    let h = equilibrium_level(x2, y);
    let (al_t, ag_t, _, _, si) = strat_geom(h);
    let (al_t, ag_t) = (al_t.max(1e-8), ag_t.max(1e-8));
    let ug = 0.25 * PI / ag_t;
    let ul = 0.25 * PI / al_t;
    // Kelvin-Helmholtz stratified stability (TD eq. with C2 = 1 - h~)
    let f2 = rho_g / drho * jg * jg / (d * g * cos_th);
    let c2 = (1.0 - h).max(1.0e-3);
    let kh_unstable = f2 * ug * ug * si / (c2 * c2 * ag_t) >= 1.0;
    if !kh_unstable {
        // stratified: smooth vs wavy via the K criterion (s = 0.01)
        let re_sl = rho_l * jl * d / MU_L;
        let k2 = f2 * re_sl;
        let s: f64 = 0.01;
        let wavy = k2 >= (2.0 / (ug * ul.sqrt() * s.sqrt())).powi(2);
        return if wavy {
            Regime::StratifiedWavy
        } else {
            Regime::StratifiedSmooth
        };
    }
    if h < 0.5 {
        return Regime::Annular;
    }
    // enough liquid for slugging; dispersed bubble if turbulence beats buoyancy
    let t2 = dpl / (drho * g * cos_th);
    let dispersed = t2 >= 8.0 * ag_t / (si * ul * ul * (ul * 4.0 * al_t).powf(-0.2));
    if dispersed {
        Regime::DispersedBubble
    } else {
        Regime::Intermittent
    }
}

pub fn regime_code(r: Regime) -> u8 {
    r as u8
}
