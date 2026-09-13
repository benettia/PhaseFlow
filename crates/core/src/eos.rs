//! Primitive recovery: (m_g, m_l, T) -> p, and the mixture sound speed.
//!
//! With both densities linear-in-p at fixed T, the closure alpha_g + alpha_l = 1
//! is exactly a quadratic in p — solved in closed form, then polished with two
//! fixed Newton iterations (fixed count: deterministic). Temperature enters
//! only through the two coefficients, so adding the energy equation cost the
//! inversion nothing.

use crate::fluid::Fluid;

pub const P_MIN: f64 = 1.0e2; // pressure floor [Pa]
pub const ALPHA_EPS: f64 = 1.0e-6; // void fraction clamp

/// Solve `m_g/rho_g(p,T) + m_l/rho_l(p,T) = 1` for p > 0.
pub fn pressure_from_masses(mg: f64, ml: f64, t: f64, f: &Fluid) -> f64 {
    let al2 = f.liq_a * f.liq_a;
    let c = f.liq_c(t); // rho_l = c + p/al2
    let a = mg.max(0.0) * f.r_gas() * t; // alpha_g = a/p
    let ml = ml.max(0.0);
    // a/p + ml/(c + p/al2) = 1  =>  p^2/al2 + (c - a/al2 - ml) p - a*c = 0
    let b = c - a / al2 - ml;
    let disc = (b * b + 4.0 * a * c / al2).max(0.0);
    let mut p = 0.5 * al2 * (-b + disc.sqrt());
    if p.is_nan() || p < P_MIN {
        p = P_MIN;
    }
    // Newton polish, exactly two iterations
    for _ in 0..2 {
        let rl = c + p / al2;
        let fv = a / p + ml / rl - 1.0;
        let df = -a / (p * p) - ml / (al2 * rl * rl);
        if df != 0.0 {
            p -= fv / df;
        }
        if p.is_nan() || p < P_MIN {
            p = P_MIN;
        }
    }
    p
}

/// Wood's two-phase sound speed: dips hard at intermediate void fraction
/// (~20 m/s for air-water at 1 bar, far below either pure phase). `thermal`
/// selects the adiabatic gas sound speed — see `Fluid::a_gas2`.
pub fn wood_sound_speed(alpha: f64, p: f64, t: f64, f: &Fluid, thermal: bool) -> f64 {
    let a = alpha.clamp(ALPHA_EPS, 1.0 - ALPHA_EPS);
    let rg = f.rho_gas(p, t);
    let rl = f.rho_liq(p, t);
    let ag2 = f.a_gas2(t, thermal);
    let al2 = f.liq_a * f.liq_a;
    let rho_m = a * rg + (1.0 - a) * rl;
    let inv = rho_m * (a / (rg * ag2) + (1.0 - a) / (rl * al2));
    (1.0 / inv).sqrt()
}

/// Mixture density at a given state [kg/m3].
pub fn rho_mix(alpha: f64, p: f64, t: f64, f: &Fluid) -> f64 {
    alpha * f.rho_gas(p, t) + (1.0 - alpha) * f.rho_liq(p, t)
}
