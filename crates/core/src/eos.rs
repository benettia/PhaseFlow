//! Isothermal equations of state and the per-cell primitive recovery.
//! Gas: rho_g = p / a_g^2. Liquid: rho_l = rho_l0 + (p - p0) / a_l^2.
//! Recovery of p from (m_g, m_l) closes alpha_g + alpha_l = 1; for this EOS
//! pair the constraint is exactly a quadratic in p, solved in closed form and
//! polished with two fixed Newton iterations (fixed count: deterministic).

pub const A_G: f64 = 316.0; // gas isothermal sound speed [m/s]
pub const A_L: f64 = 1000.0; // liquid sound speed [m/s]
pub const RHO_L0: f64 = 1000.0; // liquid ref density [kg/m3]
pub const P0: f64 = 1.0e5; // ref pressure [Pa]
pub const P_MIN: f64 = 1.0e2; // pressure floor [Pa]
pub const ALPHA_EPS: f64 = 1.0e-6; // void fraction clamp

pub fn rho_gas(p: f64) -> f64 {
    p / (A_G * A_G)
}

pub fn rho_liq(p: f64) -> f64 {
    RHO_L0 + (p - P0) / (A_L * A_L)
}

/// Solve m_g/rho_g(p) + m_l/rho_l(p) = 1 for p > 0.
pub fn pressure_from_masses(mg: f64, ml: f64) -> f64 {
    let ag2 = A_G * A_G;
    let al2 = A_L * A_L;
    let c = RHO_L0 - P0 / al2; // rho_l = c + p/al2
    let a = mg.max(0.0) * ag2;
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
        let f = a / p + ml / rl - 1.0;
        let df = -a / (p * p) - ml / (al2 * rl * rl);
        if df != 0.0 {
            p -= f / df;
        }
        if p.is_nan() || p < P_MIN {
            p = P_MIN;
        }
    }
    p
}

/// Wood's two-phase sound speed: dips hard at intermediate void fraction.
pub fn wood_sound_speed(alpha: f64, p: f64) -> f64 {
    let a = alpha.clamp(ALPHA_EPS, 1.0 - ALPHA_EPS);
    let rg = rho_gas(p);
    let rl = rho_liq(p);
    let rho_m = a * rg + (1.0 - a) * rl;
    let inv = rho_m * (a / (rg * A_G * A_G) + (1.0 - a) / (rl * A_L * A_L));
    (1.0 / inv).sqrt()
}
