//! Slip law, wall friction, and the AUSMV splitting functions.

pub const SIGMA: f64 = 0.072; // surface tension [N/m]
pub const MU_G: f64 = 1.8e-5; // gas viscosity [Pa s]
pub const MU_L: f64 = 1.0e-3; // liquid viscosity [Pa s]
pub const ROUGHNESS: f64 = 4.6e-5; // pipe wall roughness [m]

/// Zuber-Findlay profile parameter: ~1.2 in bubbly/slug, -> 1.0 as alpha -> 1.
/// The form 1 + 0.2(1 - a^2)^2 keeps 1 - C0*a > 0 on (0,1) (no singular
/// velocity solve) AND makes C0 - 1 vanish faster than (1 - a), which is what
/// forces v_l -> v_g in the pure-gas limit (slip -> 0 at both ends).
pub fn c0(alpha: f64) -> f64 {
    let w = 1.0 - alpha * alpha;
    1.0 + 0.2 * w * w
}

/// Drift velocity: Harmathy bubble rise scaled by along-axis buoyancy
/// (sin theta) and damped to zero as alpha_g -> 1 so the single-phase gas
/// limit is exact. (The alpha_g -> 0 limit is exact regardless: no gas mass.)
pub fn drift_velocity(alpha: f64, rho_g: f64, rho_l: f64, sin_th: f64, g: f64) -> f64 {
    let drho = (rho_l - rho_g).max(0.0);
    let vh = 1.53 * (g.abs() * SIGMA * drho / (rho_l * rho_l)).powf(0.25);
    vh * (1.0 - alpha) * sin_th
}

/// Recover (v_g, v_l) from mixture momentum + slip law:
///   mg*vg + ml*vl = mom
///   (1 - C0*a)*vg - C0*(1-a)*vl = vd
pub fn phase_velocities(mg: f64, ml: f64, mom: f64, alpha: f64, c0: f64, vd: f64) -> (f64, f64) {
    let det = -(mg * c0 * (1.0 - alpha) + ml * (1.0 - c0 * alpha));
    let det = if det > -1.0e-12 { -1.0e-12 } else { det };
    let vg = (-mom * c0 * (1.0 - alpha) - ml * vd) / det;
    let vl = (mg * vd - (1.0 - c0 * alpha) * mom) / det;
    (vg, vl)
}

/// Churchill friction factor (Darcy), laminar -> turbulent in one formula.
pub fn churchill_f(re: f64, rel_rough: f64) -> f64 {
    let re = re.max(1.0e-3);
    let t = (7.0 / re).powf(0.9) + 0.27 * rel_rough;
    let a = (2.457 * (1.0 / t).ln()).powi(16);
    let b = (37530.0 / re).powi(16);
    8.0 * ((8.0 / re).powi(12) + 1.0 / (a + b).powf(1.5)).powf(1.0 / 12.0)
}

/// Wall friction momentum source [Pa/m], Darcy-Weisbach on the mixture.
pub fn wall_friction(rho_m: f64, vm: f64, d: f64, mu_m: f64) -> f64 {
    let re = rho_m * vm.abs() * d / mu_m;
    let f = churchill_f(re, ROUGHNESS / d);
    -f * rho_m * vm * vm.abs() / (2.0 * d)
}

// --- van Leer / AUSMV splitting functions ---

pub fn psi_plus(v: f64, c: f64) -> f64 {
    if v >= c {
        v
    } else if v <= -c {
        0.0
    } else {
        (v + c) * (v + c) / (4.0 * c)
    }
}

pub fn psi_minus(v: f64, c: f64) -> f64 {
    if v <= -c {
        v
    } else if v >= c {
        0.0
    } else {
        -(v - c) * (v - c) / (4.0 * c)
    }
}

pub fn p_plus(v: f64, c: f64) -> f64 {
    if v >= c {
        1.0
    } else if v <= -c {
        0.0
    } else {
        let m = v / c;
        0.25 * (m + 1.0) * (m + 1.0) * (2.0 - m)
    }
}

pub fn p_minus(v: f64, c: f64) -> f64 {
    if v <= -c {
        1.0
    } else if v >= c {
        0.0
    } else {
        let m = v / c;
        0.25 * (m - 1.0) * (m - 1.0) * (2.0 + m)
    }
}
