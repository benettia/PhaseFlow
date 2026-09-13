//! Fluid properties. Everything the closures need to know about *what* is
//! flowing, in one `Copy` struct carried by the `Sim`.
//!
//! Both phases are compressible and temperature-dependent:
//!
//! ```text
//! gas     rho_g = p / (Z R_s T)              R_s = R_UNIVERSAL / mw
//! liquid  rho_l = rho_l_ref (1 - beta dT) + (p - p_ref) / a_l^2
//! ```
//!
//! Two deliberate limitations, stated rather than hidden:
//!
//! * **Z is a constant, not Z(p,T).** That makes the gas enthalpy a function
//!   of temperature alone, so the Joule-Thomson coefficient is identically
//!   zero: this model does not produce JT cooling across a choke. Expansion
//!   cooling during blowdown (the p dV work term) and frictional heating are
//!   modelled; isenthalpic throttling cooling is not.
//! * **No mass transfer.** The two phases are immiscible and inert — no
//!   flashing, no condensation, no dissolved gas coming out of solution.
//!
//! Property values live here so a scenario can say "this is a gas-oil line
//! at 40 bar" rather than inheriting whatever constants the solver was
//! written with.

/// Universal gas constant [J/(kmol K)].
pub const R_UNIVERSAL: f64 = 8314.462618;

/// Lower/upper temperature guards. Outside this band the state is corrupt,
/// not merely extreme, and `compute_prim` reports it as a failed cell.
pub const T_MIN: f64 = 100.0;
pub const T_MAX: f64 = 1500.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fluid {
    // --- gas ---
    /// Molar mass [kg/kmol].
    pub gas_mw: f64,
    /// Compressibility factor Z [-] (1.0 = ideal).
    pub gas_z: f64,
    /// Dynamic viscosity at `t_ref` [Pa s]; scaled as (T/T_ref)^0.76.
    pub gas_mu: f64,
    /// Specific heat at constant pressure [J/(kg K)].
    pub gas_cp: f64,

    // --- liquid ---
    /// Density at (`p_ref`, `t_ref`) [kg/m3].
    pub liq_rho: f64,
    /// Sound speed [m/s] — sets the isothermal compressibility.
    pub liq_a: f64,
    /// Volumetric thermal expansion [1/K].
    pub liq_beta: f64,
    /// Dynamic viscosity at `t_ref` [Pa s].
    pub liq_mu: f64,
    /// Andrade slope B [K]: mu(T) = mu_ref exp(B (1/T - 1/T_ref)).
    /// Zero disables the temperature dependence.
    pub liq_mu_b: f64,
    /// Specific heat [J/(kg K)].
    pub liq_cp: f64,

    // --- interface ---
    /// Gas-liquid surface tension [N/m].
    pub sigma: f64,

    // --- reference state the above are quoted at ---
    pub p_ref: f64,
    pub t_ref: f64,
}

impl Default for Fluid {
    /// Air and water at 15 C, 1 atm — the laboratory pair every two-phase
    /// correlation was fitted against.
    fn default() -> Self {
        Self::air_water()
    }
}

impl Fluid {
    /// Air / water at 15 C. `a_g = sqrt(R_s T) = 287.6 m/s` (the *isothermal*
    /// sound speed: the adiabatic 340 m/s applies only with the energy
    /// equation on, where `a_gas` applies the gamma correction).
    pub fn air_water() -> Self {
        Self {
            gas_mw: 28.96,
            gas_z: 1.0,
            gas_mu: 1.81e-5,
            gas_cp: 1005.0,
            liq_rho: 999.1,
            liq_a: 1480.0,
            liq_beta: 1.5e-4,
            liq_mu: 1.14e-3,
            liq_mu_b: 1800.0,
            liq_cp: 4186.0,
            sigma: 0.0734,
            p_ref: 1.0e5,
            t_ref: 288.15,
        }
    }

    /// Lean natural gas / medium crude at 40 C — a production flowline.
    pub fn gas_oil() -> Self {
        Self {
            gas_mw: 18.5,
            gas_z: 0.92,
            gas_mu: 1.30e-5,
            gas_cp: 2250.0,
            liq_rho: 850.0,
            liq_a: 1230.0,
            liq_beta: 7.0e-4,
            liq_mu: 5.0e-3,
            liq_mu_b: 3500.0,
            liq_cp: 2000.0,
            sigma: 0.025,
            p_ref: 1.0e5,
            t_ref: 313.15,
        }
    }

    /// Rich gas / condensate at 60 C — low surface tension, light liquid.
    pub fn gas_condensate() -> Self {
        Self {
            gas_mw: 22.0,
            gas_z: 0.88,
            gas_mu: 1.45e-5,
            gas_cp: 2400.0,
            liq_rho: 750.0,
            liq_a: 1050.0,
            liq_beta: 1.0e-3,
            liq_mu: 8.0e-4,
            liq_mu_b: 2200.0,
            liq_cp: 2200.0,
            sigma: 0.015,
            p_ref: 1.0e5,
            t_ref: 333.15,
        }
    }

    /// Look up a named pair. Unknown names are the caller's error to report.
    pub fn by_name(name: &str) -> Option<Fluid> {
        match name {
            "air-water" => Some(Self::air_water()),
            "gas-oil" => Some(Self::gas_oil()),
            "gas-condensate" => Some(Self::gas_condensate()),
            _ => None,
        }
    }

    /// Specific gas constant Z R / mw [J/(kg K)].
    pub fn r_gas(&self) -> f64 {
        self.gas_z * R_UNIVERSAL / self.gas_mw
    }

    /// Gas heat capacity at constant volume [J/(kg K)].
    pub fn gas_cv(&self) -> f64 {
        (self.gas_cp - self.r_gas()).max(1.0)
    }

    /// Ratio of specific heats.
    pub fn gamma(&self) -> f64 {
        self.gas_cp / self.gas_cv()
    }

    pub fn rho_gas(&self, p: f64, t: f64) -> f64 {
        p / (self.r_gas() * t)
    }

    pub fn rho_liq(&self, p: f64, t: f64) -> f64 {
        self.liq_c(t) + p / (self.liq_a * self.liq_a)
    }

    /// The temperature-only part of the liquid density: `rho_l = c(T) + p/a_l^2`.
    /// Isolating it keeps the pressure inversion a clean quadratic.
    pub fn liq_c(&self, t: f64) -> f64 {
        self.liq_rho * (1.0 - self.liq_beta * (t - self.t_ref))
            - self.p_ref / (self.liq_a * self.liq_a)
    }

    /// Squared gas sound speed used for *acoustics* [m2/s2]. Isothermal
    /// (`R_s T`) without the energy equation; adiabatic (`gamma R_s T`) with
    /// it, because then the gas really does carry entropy along.
    pub fn a_gas2(&self, t: f64, thermal: bool) -> f64 {
        let g = if thermal { self.gamma() } else { 1.0 };
        g * self.r_gas() * t
    }

    /// Gas viscosity [Pa s]. Power-law in T (Sutherland to within ~1 % over
    /// 250-450 K, and one fewer fitted constant to carry).
    pub fn mu_gas(&self, t: f64) -> f64 {
        self.gas_mu * (t / self.t_ref).powf(0.76)
    }

    /// Liquid viscosity [Pa s], Andrade. This is the property that actually
    /// moves a pressure drop when a line cools down.
    pub fn mu_liq(&self, t: f64) -> f64 {
        if self.liq_mu_b == 0.0 {
            return self.liq_mu;
        }
        self.liq_mu * (self.liq_mu_b * (1.0 / t - 1.0 / self.t_ref)).exp()
    }
}
